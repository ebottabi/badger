/// Wallet Intelligence System for Ultra-Fast Insider Copy Trading
/// 
/// This module provides nanosecond-speed insider wallet detection and copy trading
/// capabilities using hot memory caches and background database synchronization.
/// 
/// Key Components:
/// - `cache`: Ultra-fast memory cache for instant decisions
/// - `insider_detector`: Background analysis for wallet discovery  
/// - `copy_trader`: Copy trading signal generation and execution
/// - `background_sync`: Database synchronization engine
/// - `performance_tracker`: Results tracking and optimization

pub mod cache;
pub mod insider_detector;
pub mod copy_trader;
pub mod background_sync;
pub mod performance_tracker;
pub mod types;
pub mod mmap_db;
pub mod hash_utils;

// Re-export core types for easy access
pub use cache::WalletIntelligenceCache;
pub use insider_detector::InsiderDetector;
pub use copy_trader::CopyTradingEngine;
pub use background_sync::BackgroundSyncEngine;
pub use performance_tracker::PerformanceTracker;
pub use types::*;
pub use mmap_db::*;
pub use hash_utils::*;

use crate::database::BadgerDatabase;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, error};

/// Main orchestrator for the wallet intelligence system
pub struct WalletIntelligenceEngine {
    /// Database connection
    db: Arc<BadgerDatabase>,
    
    /// Ultra-fast memory cache for nanosecond decisions
    cache: Arc<WalletIntelligenceCache>,
    
    /// Insider detection and scoring
    insider_detector: Arc<InsiderDetector>,
    
    /// Copy trading signal generation
    copy_trader: Arc<CopyTradingEngine>,
    
    /// Background database synchronization
    background_sync: Arc<BackgroundSyncEngine>,
    
    /// Performance tracking and feedback
    performance_tracker: Arc<PerformanceTracker>,
    
    /// Channel for background updates (non-blocking)
    background_sender: mpsc::UnboundedSender<BackgroundUpdate>,
    background_receiver: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<BackgroundUpdate>>>,
}

impl WalletIntelligenceEngine {
    /// Create new wallet intelligence engine
    pub async fn new(
        db: Arc<BadgerDatabase>,
        signal_sender: mpsc::UnboundedSender<crate::core::TradingSignal>,
    ) -> Result<Self, crate::database::DatabaseError> {
        info!("🧠 Initializing Wallet Intelligence Engine");
        
        // Create background update channel
        let (background_sender, background_receiver) = mpsc::unbounded_channel();
        
        // Initialize cache with existing insider wallets from database
        let cache = Arc::new(WalletIntelligenceCache::new());
        let insider_detector = Arc::new(InsiderDetector::new(db.clone()));
        let copy_trader = Arc::new(CopyTradingEngine::new(signal_sender, cache.clone(), db.clone()));
        
        // Create receiver for background sync  
        let sync_receiver = Arc::new(tokio::sync::Mutex::new(background_receiver));
        let background_sync = Arc::new(BackgroundSyncEngine::new(
            db.clone(), 
            cache.clone(),
            sync_receiver.clone(),
        ));
        let performance_tracker = Arc::new(PerformanceTracker::new(db.clone()));
        
        // Load existing insider wallets into cache
        let existing_insiders = insider_detector.load_existing_insiders().await?;
        cache.initialize_with_insiders(existing_insiders).await;
        
        info!("✅ Wallet Intelligence Engine initialized with {} insider wallets", 
              cache.get_insider_count().await);
        
        Ok(Self {
            db,
            cache,
            insider_detector,
            copy_trader,
            background_sync,
            performance_tracker,
            background_sender,
            background_receiver: sync_receiver,
        })
    }
    
    /// Initialize database schema for wallet intelligence
    pub async fn initialize_schema(&self) -> Result<(), crate::database::DatabaseError> {
        info!("🔧 Initializing wallet intelligence database schema");
        
        // Initialize all component schemas
        self.insider_detector.initialize_schema().await?;
        self.copy_trader.initialize_schema().await?;
        self.performance_tracker.initialize_schema().await?;
        
        info!("✅ Wallet intelligence database schema initialized");
        Ok(())
    }
    
    /// Start background processing tasks
    pub async fn start_background_tasks(&self) -> Result<(), crate::database::DatabaseError> {
        info!("🚀 Starting wallet intelligence background tasks");
        
        // Start background sync engine
        let background_sync = self.background_sync.clone();
        tokio::spawn(async move {
            if let Err(e) = background_sync.run_background_sync().await {
                error!("Background sync engine failed: {}", e);
            }
        });
        
        info!("✅ Background tasks started");
        Ok(())
    }
    
    /// Process market event for potential copy trading (ULTRA-FAST PATH)
    /// This function is optimized for nanosecond-speed decisions
    #[inline(always)]
    pub async fn process_market_event(&self, event: &crate::core::MarketEvent) -> Result<(), crate::database::DatabaseError> {
        use crate::core::MarketEvent;
        
        match event {
            MarketEvent::SwapDetected { swap } => {
                // INSTANT decision from memory cache (nanoseconds)
                if let Some(decision) = self.cache.should_copy_trade(
                    &swap.wallet, 
                    self.get_token_age_minutes(&swap.token_out, swap.timestamp.timestamp()).await
                ).await {
                    // Determine if this is a buy (they're getting a new token)
                    let trade_type = match swap.swap_type {
                        crate::core::SwapType::Buy => "BUY",
                        crate::core::SwapType::Sell => "SELL",
                    };
                    
                    if trade_type == "BUY" {
                        // Generate copy trading signal immediately
                        self.copy_trader.generate_buy_signal(
                            &swap.wallet,
                            &swap.token_out,
                            swap.price_impact.unwrap_or(0.0),
                            decision,
                            swap.timestamp.timestamp(),
                        ).await?;
                    }
                    
                    // Queue background update (non-blocking)
                    let _ = self.background_sender.send(BackgroundUpdate::InsiderTrade {
                        wallet: swap.wallet.clone(),
                        token: swap.token_out.clone(),
                        trade_data: TradeData {
                            amount_sol: swap.amount_in as f64,
                            price: swap.price_impact.unwrap_or(0.0),
                            timestamp: swap.timestamp.timestamp(),
                            trade_type: trade_type.to_string(),
                        },
                    });
                }
            }
            
            MarketEvent::TokenLaunched { token } => {
                // Store token launch time for age calculations
                let _ = self.background_sender.send(BackgroundUpdate::TokenLaunched {
                    token_mint: token.mint.clone(),
                    launch_timestamp: token.created_at.timestamp(),
                });
            }
            
            _ => {} // Handle other event types if needed
        }
        
        Ok(())
    }
    
    /// Get token age in minutes (used for early entry calculations)
    async fn get_token_age_minutes(&self, token_mint: &str, current_timestamp: i64) -> u32 {
        // First try to get token launch time from ultra-fast memory cache
        if let Some(launch_time) = self.cache.get_token_launch_time(token_mint).await {
            let age_seconds = (current_timestamp - launch_time).max(0);
            return (age_seconds / 60) as u32;
        }
        
        // If not in cache, query database for token launch time
        match self.get_token_launch_time_from_db(token_mint).await {
            Ok(Some(launch_time)) => {
                // Store in cache for future nanosecond lookups
                self.cache.cache_token_launch_time(token_mint, launch_time).await;
                
                let age_seconds = (current_timestamp - launch_time).max(0);
                (age_seconds / 60) as u32
            }
            Ok(None) => {
                // Token launch time not found - assume very new token (0 minutes)
                // This could happen for tokens that were just launched
                0
            }
            Err(e) => {
                error!("Failed to get token launch time for {}: {}", token_mint, e);
                // Return 0 to be conservative and treat as new token
                0
            }
        }
    }
    
    /// Get token launch time from database (fallback when not in cache)
    async fn get_token_launch_time_from_db(&self, token_mint: &str) -> Result<Option<i64>, crate::database::DatabaseError> {
        use crate::database::DatabaseError;
        use sqlx::Row;
        
        // First check if we have the token launch recorded from market events
        let launch_time = sqlx::query(
            "SELECT launch_timestamp FROM token_launches WHERE token_mint = ? LIMIT 1"
        )
        .bind(token_mint)
        .fetch_optional(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to query token launch time: {}", e)))?;
        
        if let Some(row) = launch_time {
            let timestamp: i64 = row.try_get("launch_timestamp")
                .map_err(|e| DatabaseError::QueryError(format!("Failed to extract launch timestamp: {}", e)))?;
            return Ok(Some(timestamp));
        }
        
        // If no explicit launch record, try to infer from first swap/transaction
        let first_activity = sqlx::query(
            r#"
            SELECT MIN(timestamp) as first_seen
            FROM (
                SELECT timestamp FROM market_events 
                WHERE data LIKE '%' || ? || '%'
                UNION ALL
                SELECT created_at as timestamp FROM copy_trading_performance 
                WHERE token_mint = ?
            ) AS combined_events
            "#
        )
        .bind(token_mint)
        .bind(token_mint)
        .fetch_optional(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to query first token activity: {}", e)))?;
        
        if let Some(row) = first_activity {
            if let Ok(timestamp) = row.try_get::<Option<i64>, _>("first_seen") {
                if let Some(ts) = timestamp {
                    // Store this inferred launch time for future lookups
                    let _ = sqlx::query(
                        "INSERT OR IGNORE INTO token_launches (token_mint, launch_timestamp) VALUES (?, ?)"
                    )
                    .bind(token_mint)
                    .bind(ts)
                    .execute(self.db.get_pool())
                    .await;
                    
                    return Ok(Some(ts));
                }
            }
        }
        
        Ok(None)
    }
    
    /// Update performance for copy trading result (feedback loop)
    pub async fn update_copy_performance(
        &self,
        copy_signal_id: i64,
        result: CopyTradeResult,
    ) -> Result<(), crate::database::DatabaseError> {
        // Update performance tracking
        self.performance_tracker.record_copy_result(copy_signal_id, result.clone()).await?;
        
        // Queue background update for insider score recalculation
        let _ = self.background_sender.send(BackgroundUpdate::CopyTradeResult {
            copy_signal_id,
            result,
        });
        
        Ok(())
    }
    
    /// Get current cache statistics for monitoring
    pub async fn get_cache_stats(&self) -> CacheStatistics {
        self.cache.get_statistics().await
    }
    
    /// Get current insider wallet count
    pub async fn get_insider_count(&self) -> usize {
        self.cache.get_insider_count().await
    }
}