/// Background Synchronization Engine
/// 
/// This module handles all background database operations including:
/// - Periodic cache synchronization with database
/// - New insider wallet discovery
/// - Performance score recalculation
/// - Cache cleanup and optimization
/// 
/// All operations are designed to be non-blocking to the hot trading path.

use super::types::*;
use super::cache::WalletIntelligenceCache;
use super::insider_detector::InsiderDetector;
use crate::database::{BadgerDatabase, DatabaseError};
use std::sync::Arc;
use sqlx::Row;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{Duration, interval};
use chrono::Utc;
use tracing::{info, debug, warn, error, instrument};

/// Background synchronization engine
pub struct BackgroundSyncEngine {
    /// Database connection
    db: Arc<BadgerDatabase>,
    
    /// Memory cache reference
    cache: Arc<WalletIntelligenceCache>,
    
    /// Insider detection engine
    insider_detector: Arc<InsiderDetector>,
    
    /// Background update receiver
    update_receiver: Arc<Mutex<mpsc::UnboundedReceiver<BackgroundUpdate>>>,
    
    /// Configuration
    config: BackgroundSyncConfig,
    
    /// Token launch tracking for age calculations
    token_launches: Arc<tokio::sync::RwLock<std::collections::HashMap<String, i64>>>,
}

/// Configuration for background sync operations
#[derive(Debug, Clone)]
pub struct BackgroundSyncConfig {
    /// Interval for cache synchronization (seconds)
    pub sync_interval_seconds: u64,
    
    /// Interval for insider discovery (seconds)
    pub discovery_interval_seconds: u64,
    
    /// How many days back to look for new insiders
    pub discovery_lookback_days: i32,
    
    /// Maximum number of background updates to process per batch
    pub max_batch_size: usize,
    
    /// Cleanup interval for old data (seconds)
    pub cleanup_interval_seconds: u64,
}

impl Default for BackgroundSyncConfig {
    fn default() -> Self {
        Self {
            sync_interval_seconds: 30,      // Sync every 30 seconds
            discovery_interval_seconds: 300, // Discover every 5 minutes
            discovery_lookback_days: 7,     // Look back 7 days
            max_batch_size: 100,            // Process 100 updates per batch
            cleanup_interval_seconds: 3600, // Cleanup every hour
        }
    }
}

impl BackgroundSyncEngine {
    /// Create new background sync engine
    pub fn new(
        db: Arc<BadgerDatabase>,
        cache: Arc<WalletIntelligenceCache>,
        update_receiver: Arc<Mutex<mpsc::UnboundedReceiver<BackgroundUpdate>>>,
    ) -> Self {
        let insider_detector = Arc::new(InsiderDetector::new(db.clone()));
        
        Self {
            db,
            cache,
            insider_detector,
            update_receiver,
            config: BackgroundSyncConfig::default(),
            token_launches: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
    
    /// Create with custom configuration
    pub fn with_config(
        db: Arc<BadgerDatabase>,
        cache: Arc<WalletIntelligenceCache>,
        update_receiver: Arc<Mutex<mpsc::UnboundedReceiver<BackgroundUpdate>>>,
        config: BackgroundSyncConfig,
    ) -> Self {
        let mut engine = Self::new(db, cache, update_receiver);
        engine.config = config;
        engine
    }
    
    /// Start background synchronization (main loop)
    #[instrument(skip(self))]
    pub async fn run_background_sync(&self) -> Result<(), DatabaseError> {
        info!("🔄 Starting background synchronization engine");
        
        // Create intervals for different operations
        let mut sync_interval = interval(Duration::from_secs(self.config.sync_interval_seconds));
        let mut discovery_interval = interval(Duration::from_secs(self.config.discovery_interval_seconds));
        let mut cleanup_interval = interval(Duration::from_secs(self.config.cleanup_interval_seconds));
        
        // Skip initial ticks
        sync_interval.tick().await;
        discovery_interval.tick().await;
        cleanup_interval.tick().await;
        
        loop {
            tokio::select! {
                // Periodic cache synchronization
                _ = sync_interval.tick() => {
                    if let Err(e) = self.sync_cache_with_database().await {
                        error!("Cache sync failed: {}", e);
                    }
                }
                
                // Periodic insider discovery
                _ = discovery_interval.tick() => {
                    if let Err(e) = self.discover_and_add_new_insiders().await {
                        error!("Insider discovery failed: {}", e);
                    }
                }
                
                // Periodic cleanup
                _ = cleanup_interval.tick() => {
                    if let Err(e) = self.cleanup_old_data().await {
                        error!("Data cleanup failed: {}", e);
                    }
                }
                
                // Process background updates
                _ = self.process_background_updates() => {
                    // This runs continuously
                }
            }
        }
    }
    
    /// Process background update messages from the hot path
    async fn process_background_updates(&self) {
        let mut receiver = self.update_receiver.lock().await;
        let mut batch = Vec::new();
        
        // Collect updates in batches for efficiency
        while batch.len() < self.config.max_batch_size {
            match receiver.try_recv() {
                Ok(update) => batch.push(update),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    warn!("Background update channel disconnected");
                    return;
                }
            }
        }
        
        if !batch.is_empty() {
            if let Err(e) = self.process_update_batch(batch).await {
                error!("Failed to process background update batch: {}", e);
            }
        }
        
        // Small delay to prevent busy waiting
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    /// Process a batch of background updates
    #[instrument(skip(self, batch))]
    async fn process_update_batch(&self, batch: Vec<BackgroundUpdate>) -> Result<(), DatabaseError> {
        debug!("📦 Processing background update batch of {} items", batch.len());
        
        for update in batch {
            match update {
                BackgroundUpdate::InsiderTrade { wallet, token, trade_data } => {
                    self.handle_insider_trade(wallet, token, trade_data).await?;
                }
                
                BackgroundUpdate::TokenLaunched { token_mint, launch_timestamp } => {
                    self.handle_token_launch(token_mint, launch_timestamp).await?;
                }
                
                BackgroundUpdate::CopyTradeResult { copy_signal_id, result } => {
                    self.handle_copy_trade_result(copy_signal_id, result).await?;
                }
                
                BackgroundUpdate::RefreshCache => {
                    self.sync_cache_with_database().await?;
                }
                
                BackgroundUpdate::DiscoverInsiders => {
                    self.discover_and_add_new_insiders().await?;
                }
            }
        }
        
        Ok(())
    }
    
    /// Handle insider trade update
    async fn handle_insider_trade(
        &self,
        wallet: String,
        token: String,
        trade_data: TradeData,
    ) -> Result<(), DatabaseError> {
        // Get token launch timestamp for early entry calculation
        let token_launch_timestamp = {
            let launches = self.token_launches.read().await;
            launches.get(&token).copied()
        };
        
        // Record the trade for analysis
        self.insider_detector.record_insider_trade(
            &wallet,
            &token,
            &trade_data,
            token_launch_timestamp,
        ).await?;
        
        // If this is a new wallet, analyze it for potential insider status
        if self.cache.get_insider_details(&wallet).await.is_none() && 
           !self.cache.is_blacklisted(&wallet).await {
            
            if let Some(new_insider) = self.insider_detector.analyze_wallet_performance(&wallet).await? {
                if new_insider.is_qualified_insider() {
                    // Save to database
                    self.insider_detector.save_insider_wallet(&new_insider).await?;
                    
                    // Add to cache
                    self.cache.add_insider(new_insider).await;
                    
                    info!("🎯 New insider wallet discovered: {} (confidence: {:.3})", 
                          wallet, self.cache.get_insider_details(&wallet).await.unwrap().confidence_score);
                }
            }
        }
        
        Ok(())
    }
    
    /// Handle token launch update
    async fn handle_token_launch(&self, token_mint: String, launch_timestamp: i64) -> Result<(), DatabaseError> {
        // Store token launch time for age calculations
        {
            let mut launches = self.token_launches.write().await;
            launches.insert(token_mint.clone(), launch_timestamp);
            
            // Keep only recent launches (last 24 hours)
            let cutoff = Utc::now().timestamp() - (24 * 3600);
            launches.retain(|_, &mut timestamp| timestamp > cutoff);
        }
        
        // Also update cache
        self.cache.record_token_launch(token_mint, launch_timestamp).await;
        
        Ok(())
    }
    
    /// Handle copy trade result for performance tracking
    async fn handle_copy_trade_result(
        &self,
        copy_signal_id: i64,
        result: CopyTradeResult,
    ) -> Result<(), DatabaseError> {
        // Update copy trading performance in database
        let result_str = match result.result {
            TradeResult::Win => "WIN",
            TradeResult::Loss => "LOSS",
            TradeResult::Pending => "PENDING",
        };
        
        let exit_reason_str = match result.exit_reason {
            ExitReason::InsiderExit => "INSIDER_EXIT",
            ExitReason::TakeProfit => "TAKE_PROFIT",
            ExitReason::StopLoss => "STOP_LOSS",
            ExitReason::TimeDecay => "TIME_DECAY",
            ExitReason::Manual => "MANUAL",
        };
        
        // This will be handled by the performance tracker module
        // For now, just log the result
        debug!("📊 Copy trade result: signal_id={}, wallet={}, result={:?}", 
               copy_signal_id, result.insider_wallet, result.result);
        
        Ok(())
    }
    
    /// Synchronize cache with fresh database calculations
    #[instrument(skip(self))]
    async fn sync_cache_with_database(&self) -> Result<(), DatabaseError> {
        debug!("🔄 Starting cache synchronization with database");
        
        // Get fresh insider scores from database
        let fresh_scores = self.insider_detector.calculate_fresh_insider_scores().await?;
        
        if fresh_scores.is_empty() {
            debug!("No insider scores to update");
            return Ok(());
        }
        
        let fresh_scores_len = fresh_scores.len();
        
        // Batch update cache
        let mut updates = Vec::new();
        
        for (wallet_address, fresh_score) in fresh_scores {
            if let Some(mut insider) = self.cache.get_insider_details(&wallet_address).await {
                // Update scores
                insider.confidence_score = fresh_score.confidence;
                insider.win_rate = fresh_score.win_rate;
                insider.avg_profit_percentage = fresh_score.avg_profit;
                insider.recent_activity_score = fresh_score.recent_activity;
                
                // Update status based on new scores
                if insider.should_blacklist() {
                    insider.status = WalletStatus::Blacklisted;
                } else if insider.should_promote_to_active() {
                    insider.status = WalletStatus::Active;
                }
                
                // Save updated wallet to database
                self.insider_detector.save_insider_wallet(&insider).await?;
                
                updates.push((wallet_address, insider));
            }
        }
        
        // Batch update cache
        self.cache.batch_update_insiders(updates).await;
        
        info!("✅ Cache synchronized with {} updated insider scores", fresh_scores_len);
        Ok(())
    }
    
    /// Discover new insider wallets and add them to the system
    #[instrument(skip(self))]
    async fn discover_and_add_new_insiders(&self) -> Result<(), DatabaseError> {
        debug!("🔍 Starting insider wallet discovery");
        
        // Discover new candidates
        let candidates = self.insider_detector.discover_new_insiders(self.config.discovery_lookback_days).await?;
        
        if candidates.is_empty() {
            debug!("No new insider candidates found");
            return Ok(());
        }
        
        let candidates_len = candidates.len();
        let mut new_insiders_added = 0;
        
        for candidate in candidates {
            // Analyze the candidate in detail
            if let Some(insider) = self.insider_detector.analyze_wallet_performance(&candidate.address).await? {
                if insider.is_qualified_insider() {
                    // Save to database
                    self.insider_detector.save_insider_wallet(&insider).await?;
                    
                    // Log discovery
                    sqlx::query(
                        r#"
                        INSERT INTO wallet_discovery_log (
                            wallet_address, discovery_method, initial_confidence, discovery_timestamp
                        ) VALUES (?, ?, ?, ?)
                        "#
                    )
                    .bind(&insider.address)
                    .bind(match candidate.discovery_method {
                        DiscoveryMethod::EarlyEntry => "EARLY_ENTRY",
                        DiscoveryMethod::HighProfit => "HIGH_PROFIT", 
                        DiscoveryMethod::PatternMatch => "PATTERN_MATCH",
                        DiscoveryMethod::Manual => "MANUAL",
                    })
                    .bind(candidate.initial_confidence)
                    .bind(Utc::now().timestamp())
                    .execute(self.db.get_pool())
                    .await
                    .map_err(|e| DatabaseError::QueryError(format!("Failed to log wallet discovery: {}", e)))?;
                    
                    // Add to cache
                    self.cache.add_insider(insider).await;
                    
                    new_insiders_added += 1;
                }
            }
        }
        
        if new_insiders_added > 0 {
            info!("🎯 Discovered and added {} new insider wallets", new_insiders_added);
        } else {
            debug!("No qualified insider wallets found from {} candidates", candidates_len);
        }
        
        Ok(())
    }
    
    /// Clean up old data to maintain performance
    #[instrument(skip(self))]
    async fn cleanup_old_data(&self) -> Result<(), DatabaseError> {
        debug!("🧹 Starting data cleanup");
        
        let now = Utc::now().timestamp();
        let cutoff_timestamp = now - (30 * 24 * 3600); // 30 days ago
        
        // Clean up old wallet trade analysis data
        let deleted_trades_result = sqlx::query(
            "DELETE FROM wallet_trade_analysis WHERE detected_at < ?"
        )
        .bind(cutoff_timestamp)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to cleanup old trades: {}", e)))?;
        
        let deleted_trades = deleted_trades_result.rows_affected();
        
        // Remove inactive insider wallets that haven't traded in 60 days
        let inactive_cutoff = now - (60 * 24 * 3600);
        let inactive_wallets = sqlx::query(
            r#"
            SELECT address FROM insider_wallets 
            WHERE last_trade_timestamp < ? 
            AND status != 'ACTIVE'
            "#
        )
        .bind(inactive_cutoff)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to find inactive wallets: {}", e)))?;
        
        let mut removed_count = 0;
        for record in &inactive_wallets {
            let address: String = record.try_get("address").unwrap_or_default();
            
            // Remove from database
            sqlx::query("DELETE FROM insider_wallets WHERE address = ?")
                .bind(&address)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to delete inactive wallet: {}", e)))?;
            
            // Remove from cache
            self.cache.remove_insider(&address).await;
            removed_count += 1;
        }
        
        // Clean up token launches cache
        {
            let mut launches = self.token_launches.write().await;
            let token_cutoff = now - (24 * 3600); // Keep only last 24 hours
            launches.retain(|_, &mut timestamp| timestamp > token_cutoff);
        }
        
        if deleted_trades > 0 || removed_count > 0 {
            info!("🧹 Cleanup completed: {} old trades, {} inactive wallets removed", 
                  deleted_trades, removed_count);
        }
        
        Ok(())
    }
    
    /// Get background sync statistics
    pub async fn get_sync_statistics(&self) -> SyncStatistics {
        let cache_stats = self.cache.get_statistics().await;
        
        SyncStatistics {
            cache_insider_count: cache_stats.insider_count,
            cache_active_count: cache_stats.active_count,
            cache_blacklisted_count: cache_stats.blacklisted_count,
            cache_hit_rate: cache_stats.hit_rate,
            token_launches_tracked: {
                let launches = self.token_launches.read().await;
                launches.len()
            },
            last_sync_timestamp: cache_stats.last_update,
        }
    }
}

/// Background sync statistics
#[derive(Debug, Clone)]
pub struct SyncStatistics {
    pub cache_insider_count: usize,
    pub cache_active_count: usize,
    pub cache_blacklisted_count: usize,
    pub cache_hit_rate: f64,
    pub token_launches_tracked: usize,
    pub last_sync_timestamp: i64,
}