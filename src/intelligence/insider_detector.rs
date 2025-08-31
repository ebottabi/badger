/// Insider Detection Engine with Mathematical Algorithms
/// 
/// This module implements the core mathematical algorithms for identifying
/// high-performing insider wallets based on win rate, profit analysis, 
/// early entry patterns, and recency weighting.

use super::types::*;
use crate::database::{BadgerDatabase, DatabaseError};
use std::sync::Arc;
use std::collections::HashMap;
use sqlx::Row;
use chrono::Utc;
use tracing::{info, debug, warn, error, instrument};

/// Insider detection and analysis engine
pub struct InsiderDetector {
    db: Arc<BadgerDatabase>,
}

impl InsiderDetector {
    /// Create new insider detector
    pub fn new(db: Arc<BadgerDatabase>) -> Self {
        Self { db }
    }
    
    /// Initialize database schema for insider detection
    #[instrument(skip(self))]
    pub async fn initialize_schema(&self) -> Result<(), DatabaseError> {
        info!("🔧 Insider detector schema initialization (skipped - handled by migration system)");
        
        // Schema creation is handled by the migration system
        info!("✅ Insider detector schema ready");
        return Ok(());
        
        // OLD CODE (disabled):
        let _create_insider_wallets_table = r#"
            CREATE TABLE IF NOT EXISTS insider_wallets (
                address TEXT PRIMARY KEY,
                confidence_score REAL NOT NULL,
                win_rate REAL NOT NULL,
                avg_profit_percentage REAL NOT NULL,
                early_entry_score REAL NOT NULL,
                total_trades INTEGER NOT NULL,
                profitable_trades INTEGER NOT NULL,
                last_trade_timestamp INTEGER NOT NULL,
                first_detected_timestamp INTEGER NOT NULL,
                recent_activity_score REAL NOT NULL DEFAULT 0.0,
                status TEXT NOT NULL CHECK (status IN ('ACTIVE', 'MONITORING', 'BLACKLISTED', 'COOLDOWN')),
                total_copied_trades INTEGER DEFAULT 0,
                successful_copied_trades INTEGER DEFAULT 0,
                total_copy_profit_sol REAL DEFAULT 0.0,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )
        "#;
        
        let create_wallet_trade_analysis_table = r#"
            CREATE TABLE IF NOT EXISTS wallet_trade_analysis (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_address TEXT NOT NULL,
                token_mint TEXT NOT NULL,
                trade_type TEXT NOT NULL CHECK (trade_type IN ('BUY', 'SELL')),
                amount_sol REAL NOT NULL,
                price REAL NOT NULL,
                timestamp INTEGER NOT NULL,
                token_launch_timestamp INTEGER,
                entry_delay_minutes INTEGER,
                early_entry_score REAL,
                trade_outcome TEXT CHECK (trade_outcome IN ('WIN', 'LOSS', 'PENDING')),
                profit_percentage REAL,
                was_copied BOOLEAN DEFAULT 0,
                copy_result TEXT CHECK (copy_result IN ('SUCCESS', 'FAILED', 'SKIPPED')),
                detected_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                
                FOREIGN KEY (wallet_address) REFERENCES insider_wallets (address)
            )
        "#;
        
        let create_wallet_discovery_log_table = r#"
            CREATE TABLE IF NOT EXISTS wallet_discovery_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_address TEXT NOT NULL,
                discovery_method TEXT NOT NULL CHECK (discovery_method IN ('EARLY_ENTRY', 'HIGH_PROFIT', 'PATTERN_MATCH', 'MANUAL')),
                initial_confidence REAL NOT NULL,
                discovery_timestamp INTEGER NOT NULL,
                first_qualifying_trade_id INTEGER,
                promotion_to_active INTEGER,
                
                FOREIGN KEY (wallet_address) REFERENCES insider_wallets (address)
            )
        "#;
        
        // Create indexes for performance
        let create_indexes = vec![
            "CREATE INDEX IF NOT EXISTS idx_insider_wallets_confidence ON insider_wallets(confidence_score DESC)",
            "CREATE INDEX IF NOT EXISTS idx_insider_wallets_status ON insider_wallets(status)",
            "CREATE INDEX IF NOT EXISTS idx_insider_wallets_last_trade ON insider_wallets(last_trade_timestamp DESC)",
            "CREATE INDEX IF NOT EXISTS idx_wallet_trades_address_timestamp ON wallet_trade_analysis(wallet_address, timestamp DESC)",
            "CREATE INDEX IF NOT EXISTS idx_wallet_trades_token_timestamp ON wallet_trade_analysis(token_mint, timestamp DESC)",
            "CREATE INDEX IF NOT EXISTS idx_wallet_trades_outcome ON wallet_trade_analysis(trade_outcome)",
        ];
        
        // OLD CODE (unreachable due to early return):
        /*
        // Execute schema creation
        sqlx::query(create_insider_wallets_table)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to create insider_wallets table: {}", e)))?;
        
        sqlx::query(create_wallet_trade_analysis_table)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to create wallet_trade_analysis table: {}", e)))?;
        
        sqlx::query(create_wallet_discovery_log_table)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to create wallet_discovery_log table: {}", e)))?;
        
        for index_sql in create_indexes {
            sqlx::query(index_sql)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to create index: {}", e)))?;
        }
        
        info!("✅ Insider detector database schema initialized");
        Ok(())
        */
    }
    
    /// Load existing insider wallets from database
    #[instrument(skip(self))]
    pub async fn load_existing_insiders(&self) -> Result<Vec<InsiderWallet>, DatabaseError> {
        let rows = sqlx::query(
            r#"
            SELECT 
                address, confidence_score, win_rate, avg_profit_percentage, 
                early_entry_score, total_trades, profitable_trades, 
                last_trade_timestamp, first_detected_timestamp, 
                recent_activity_score, status
            FROM insider_wallets 
            ORDER BY confidence_score DESC
            "#
        )
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to load insider wallets: {}", e)))?;
        
        let mut loaded_insiders = Vec::new();
        
        for row in rows {
            // Parse status string to enum
            let status_str: String = row.try_get("status")
                .map_err(|e| DatabaseError::QueryError(format!("Failed to parse status: {}", e)))?;
            let status = match status_str.as_str() {
                "ACTIVE" => WalletStatus::Active,
                "MONITORING" => WalletStatus::Monitoring,
                "BLACKLISTED" => WalletStatus::Blacklisted,
                "COOLDOWN" => WalletStatus::Cooldown,
                _ => WalletStatus::Monitoring, // Default fallback
            };
            
            let insider = InsiderWallet {
                address: row.try_get("address")
                    .map_err(|e| DatabaseError::QueryError(format!("Failed to parse address: {}", e)))?,
                confidence_score: row.try_get("confidence_score")
                    .map_err(|e| DatabaseError::QueryError(format!("Failed to parse confidence_score: {}", e)))?,
                win_rate: row.try_get("win_rate")
                    .map_err(|e| DatabaseError::QueryError(format!("Failed to parse win_rate: {}", e)))?,
                avg_profit_percentage: row.try_get("avg_profit_percentage")
                    .map_err(|e| DatabaseError::QueryError(format!("Failed to parse avg_profit_percentage: {}", e)))?,
                early_entry_score: row.try_get("early_entry_score")
                    .map_err(|e| DatabaseError::QueryError(format!("Failed to parse early_entry_score: {}", e)))?,
                total_trades: row.try_get::<i64, _>("total_trades")
                    .map_err(|e| DatabaseError::QueryError(format!("Failed to parse total_trades: {}", e)))? as u32,
                profitable_trades: row.try_get::<i64, _>("profitable_trades")
                    .map_err(|e| DatabaseError::QueryError(format!("Failed to parse profitable_trades: {}", e)))? as u32,
                last_trade_timestamp: row.try_get("last_trade_timestamp")
                    .map_err(|e| DatabaseError::QueryError(format!("Failed to parse last_trade_timestamp: {}", e)))?,
                first_detected_timestamp: row.try_get("first_detected_timestamp")
                    .map_err(|e| DatabaseError::QueryError(format!("Failed to parse first_detected_timestamp: {}", e)))?,
                recent_activity_score: row.try_get("recent_activity_score")
                    .map_err(|e| DatabaseError::QueryError(format!("Failed to parse recent_activity_score: {}", e)))?,
                status,
            };
            
            loaded_insiders.push(insider);
        }
        
        info!("📊 Loaded {} existing insider wallets from database", loaded_insiders.len());
        Ok(loaded_insiders)
    }
    
    /// Discover new insider wallets from recent successful trades
    #[instrument(skip(self))]
    pub async fn discover_new_insiders(&self, days_lookback: i32) -> Result<Vec<WalletCandidate>, DatabaseError> {
        let cutoff_timestamp = Utc::now().timestamp() - (days_lookback as i64 * 24 * 3600);
        
        debug!("🔍 Starting insider wallet discovery for last {} days (since timestamp: {})", days_lookback, cutoff_timestamp);
        
        // Find wallets with high win rates and profits that aren't already tracked
        let high_performance_candidates = self.find_high_performance_candidates(cutoff_timestamp).await?;
        
        // Find wallets with consistent early entry patterns
        let early_entry_candidates = self.find_early_entry_candidates(cutoff_timestamp).await?;
        
        // Find wallets with unusual profit patterns
        let high_profit_candidates = self.find_high_profit_candidates(cutoff_timestamp).await?;
        
        // Combine and deduplicate candidates
        let mut candidates_map: std::collections::HashMap<String, WalletCandidate> = std::collections::HashMap::new();
        
        // Process high performance candidates
        for candidate in high_performance_candidates {
            candidates_map.insert(candidate.address.clone(), candidate);
        }
        
        // Process early entry candidates (higher priority)
        for candidate in early_entry_candidates {
            candidates_map.insert(candidate.address.clone(), candidate);
        }
        
        // Process high profit candidates (highest priority) 
        for candidate in high_profit_candidates {
            candidates_map.insert(candidate.address.clone(), candidate);
        }
        
        let wallet_candidates: Vec<WalletCandidate> = candidates_map.into_values().collect();
        
        info!("🔍 Discovered {} new insider wallet candidates", wallet_candidates.len());
        Ok(wallet_candidates)
    }
    
    /// Find wallets with high win rates and consistent profits
    async fn find_high_performance_candidates(&self, cutoff_timestamp: i64) -> Result<Vec<WalletCandidate>, DatabaseError> {
        let rows = sqlx::query(
            r#"
            SELECT 
                wallet_address,
                COUNT(*) as total_trades,
                SUM(CASE WHEN trade_outcome = 'WIN' THEN 1 ELSE 0 END) as winning_trades,
                AVG(CASE WHEN trade_outcome = 'WIN' THEN profit_percentage ELSE 0 END) as avg_profit
            FROM wallet_trade_analysis 
            WHERE timestamp >= ? 
                AND trade_outcome IN ('WIN', 'LOSS')
                AND wallet_address NOT IN (SELECT address FROM insider_wallets)
            GROUP BY wallet_address
            HAVING total_trades >= 5 
                AND (winning_trades * 1.0 / total_trades) >= 0.70
                AND avg_profit >= 0.40
            ORDER BY (winning_trades * 1.0 / total_trades) DESC, avg_profit DESC
            LIMIT 20
            "#
        )
        .bind(cutoff_timestamp)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to find high performance candidates: {}", e)))?;
        
        let mut candidates = Vec::new();
        
        for row in rows {
            let wallet_address: String = row.try_get("wallet_address")?;
            let total_trades: i64 = row.try_get("total_trades")?;
            let winning_trades: i64 = row.try_get("winning_trades")?;
            let avg_profit: f64 = row.try_get("avg_profit").unwrap_or(0.0);
            
            let win_rate = winning_trades as f64 / total_trades as f64;
            let confidence = (win_rate * 0.6) + (avg_profit * 0.4); // Weighted confidence
            
            // Get qualifying trades for this wallet
            let qualifying_trades = self.get_wallet_qualifying_trades(&wallet_address, cutoff_timestamp).await?;
            
            candidates.push(WalletCandidate {
                address: wallet_address,
                initial_confidence: confidence,
                discovery_method: DiscoveryMethod::HighProfit,
                qualifying_trades,
            });
        }
        
        debug!("📊 Found {} high performance candidates", candidates.len());
        Ok(candidates)
    }
    
    /// Find wallets with consistent early entry patterns
    async fn find_early_entry_candidates(&self, cutoff_timestamp: i64) -> Result<Vec<WalletCandidate>, DatabaseError> {
        let rows = sqlx::query(
            r#"
            SELECT 
                wallet_address,
                COUNT(*) as total_trades,
                AVG(entry_delay_minutes) as avg_entry_delay,
                SUM(CASE WHEN entry_delay_minutes <= 5 THEN 1 ELSE 0 END) as early_entries,
                AVG(early_entry_score) as avg_early_score
            FROM wallet_trade_analysis 
            WHERE timestamp >= ? 
                AND entry_delay_minutes IS NOT NULL
                AND trade_outcome = 'WIN'
                AND wallet_address NOT IN (SELECT address FROM insider_wallets)
            GROUP BY wallet_address
            HAVING total_trades >= 3
                AND avg_entry_delay <= 10.0
                AND (early_entries * 1.0 / total_trades) >= 0.60
            ORDER BY avg_early_score DESC, avg_entry_delay ASC
            LIMIT 15
            "#
        )
        .bind(cutoff_timestamp)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to find early entry candidates: {}", e)))?;
        
        let mut candidates = Vec::new();
        
        for row in rows {
            let wallet_address: String = row.try_get("wallet_address")?;
            let avg_early_score: f64 = row.try_get("avg_early_score").unwrap_or(0.0);
            let early_entries: i64 = row.try_get("early_entries")?;
            let total_trades: i64 = row.try_get("total_trades")?;
            
            let early_rate = early_entries as f64 / total_trades as f64;
            let confidence = (avg_early_score / 100.0 * 0.7) + (early_rate * 0.3);
            
            // Get qualifying trades for this wallet
            let qualifying_trades = self.get_wallet_qualifying_trades(&wallet_address, cutoff_timestamp).await?;
            
            candidates.push(WalletCandidate {
                address: wallet_address,
                initial_confidence: confidence,
                discovery_method: DiscoveryMethod::EarlyEntry,
                qualifying_trades,
            });
        }
        
        debug!("⚡ Found {} early entry candidates", candidates.len());
        Ok(candidates)
    }
    
    /// Find wallets with unusual high profit patterns
    async fn find_high_profit_candidates(&self, cutoff_timestamp: i64) -> Result<Vec<WalletCandidate>, DatabaseError> {
        let rows = sqlx::query(
            r#"
            SELECT 
                wallet_address,
                COUNT(*) as total_trades,
                MAX(profit_percentage) as max_profit,
                AVG(profit_percentage) as avg_profit,
                COUNT(CASE WHEN profit_percentage > 1.0 THEN 1 END) as big_wins
            FROM wallet_trade_analysis 
            WHERE timestamp >= ? 
                AND trade_outcome = 'WIN'
                AND profit_percentage > 0.50
                AND wallet_address NOT IN (SELECT address FROM insider_wallets)
            GROUP BY wallet_address
            HAVING total_trades >= 3
                AND max_profit >= 2.0
                AND avg_profit >= 0.80
            ORDER BY max_profit DESC, avg_profit DESC
            LIMIT 10
            "#
        )
        .bind(cutoff_timestamp)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to find high profit candidates: {}", e)))?;
        
        let mut candidates = Vec::new();
        
        for row in rows {
            let wallet_address: String = row.try_get("wallet_address")?;
            let max_profit: f64 = row.try_get("max_profit").unwrap_or(0.0);
            let avg_profit: f64 = row.try_get("avg_profit").unwrap_or(0.0);
            let big_wins: i64 = row.try_get("big_wins")?;
            let total_trades: i64 = row.try_get("total_trades")?;
            
            let big_win_rate = big_wins as f64 / total_trades as f64;
            let confidence = (avg_profit * 0.5) + (max_profit.min(5.0) / 5.0 * 0.3) + (big_win_rate * 0.2);
            
            // Get qualifying trades for this wallet
            let qualifying_trades = self.get_wallet_qualifying_trades(&wallet_address, cutoff_timestamp).await?;
            
            candidates.push(WalletCandidate {
                address: wallet_address,
                initial_confidence: confidence,
                discovery_method: DiscoveryMethod::HighProfit,
                qualifying_trades,
            });
        }
        
        debug!("💰 Found {} high profit candidates", candidates.len());
        Ok(candidates)
    }
    
    /// Get qualifying trades for a wallet candidate
    async fn get_wallet_qualifying_trades(&self, wallet_address: &str, cutoff_timestamp: i64) -> Result<Vec<TradeData>, DatabaseError> {
        let rows = sqlx::query(
            r#"
            SELECT amount_sol, price, timestamp, trade_type
            FROM wallet_trade_analysis 
            WHERE wallet_address = ? 
                AND timestamp >= ?
                AND trade_outcome = 'WIN'
            ORDER BY timestamp DESC
            LIMIT 10
            "#
        )
        .bind(wallet_address)
        .bind(cutoff_timestamp)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get qualifying trades: {}", e)))?;
        
        let mut trades = Vec::new();
        
        for row in rows {
            trades.push(TradeData {
                amount_sol: row.try_get("amount_sol")?,
                price: row.try_get("price")?,
                timestamp: row.try_get("timestamp")?,
                trade_type: row.try_get("trade_type")?,
            });
        }
        
        Ok(trades)
    }
    
    /// Mathematical Algorithm: Check if wallet meets insider criteria
    fn meets_insider_criteria(&self, win_rate: f64, avg_profit: f64, total_trades: u32) -> bool {
        win_rate >= 0.70 &&           // 70%+ win rate
        avg_profit >= 0.40 &&         // 40%+ average profit
        total_trades >= 5             // Minimum statistical significance
    }
    
    /// Mathematical Algorithm: Calculate confidence score with recency weighting
    /// Formula: Base Score * Recency Weight
    /// Base Score = 0.4 * Win Rate + 0.3 * Avg Profit + 0.2 * Early Entry + 0.1 * Volume
    /// Recency Weight = e^(-days_since_last_trade / 7)
    fn calculate_confidence_score(
        &self,
        win_rate: f64,
        avg_profit_percentage: f64,
        early_entry_score: f64,
        volume_score: f64,
        last_trade_timestamp: i64,
    ) -> f64 {
        // Base score calculation (weighted sum)
        let base_score = 
            0.4 * win_rate +                           // 40% weight on win rate
            0.3 * (avg_profit_percentage / 1.0) +      // 30% weight on profit (normalized to ~1.0)
            0.2 * (early_entry_score / 100.0) +       // 20% weight on early entry (normalized)
            0.1 * volume_score;                        // 10% weight on volume
        
        // Recency weighting (exponential decay)
        let days_since_last_trade = (Utc::now().timestamp() - last_trade_timestamp) as f64 / 86400.0;
        let recency_weight = (-days_since_last_trade / 7.0).exp(); // 7-day decay constant
        
        // Final confidence score
        (base_score * recency_weight).min(1.0) // Cap at 1.0
    }
    
    /// Mathematical Algorithm: Calculate early entry score
    /// Formula: (1 / (Entry Minutes + 1)) * 100
    /// Higher scores for faster entries after token launch
    fn calculate_early_entry_score(&self, entry_minutes_after_launch: u32) -> f64 {
        1.0 / (entry_minutes_after_launch as f64 + 1.0) * 100.0
    }
    
    /// Analyze wallet's trading patterns and calculate comprehensive metrics
    #[instrument(skip(self))]
    pub async fn analyze_wallet_performance(&self, wallet_address: &str) -> Result<Option<InsiderWallet>, DatabaseError> {
        debug!("📊 Analyzing wallet performance for: {}", wallet_address);
        
        // Get all trades for this wallet
        let trade_rows = sqlx::query(
            r#"
            SELECT 
                trade_type, amount_sol, price, timestamp, token_launch_timestamp,
                entry_delay_minutes, early_entry_score, trade_outcome, profit_percentage
            FROM wallet_trade_analysis 
            WHERE wallet_address = ?
                AND trade_outcome IN ('WIN', 'LOSS')
            ORDER BY timestamp DESC
            "#
        )
        .bind(wallet_address)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get wallet trades: {}", e)))?;
        
        if trade_rows.is_empty() {
            debug!("No trades found for wallet: {}", wallet_address);
            return Ok(None);
        }
        
        // Calculate basic statistics
        let total_trades = trade_rows.len() as u32;
        let mut profitable_trades = 0u32;
        let mut total_profit = 0.0f64;
        let mut profit_sum = 0.0f64;
        let mut early_entry_scores = Vec::new();
        let mut volume_sol = 0.0f64;
        let mut first_trade_timestamp = i64::MAX;
        let mut last_trade_timestamp = 0i64;
        
        for row in &trade_rows {
            let timestamp: i64 = row.try_get("timestamp")?;
            let amount_sol: f64 = row.try_get("amount_sol")?;
            let trade_outcome: String = row.try_get("trade_outcome")?;
            
            // Track time range
            first_trade_timestamp = first_trade_timestamp.min(timestamp);
            last_trade_timestamp = last_trade_timestamp.max(timestamp);
            
            // Track volume
            volume_sol += amount_sol;
            
            // Track profitability
            if trade_outcome == "WIN" {
                profitable_trades += 1;
                if let Ok(profit_pct) = row.try_get::<f64, _>("profit_percentage") {
                    profit_sum += profit_pct;
                    total_profit += profit_pct;
                }
            }
            
            // Track early entry patterns
            if let Ok(Some(early_score)) = row.try_get::<Option<f64>, _>("early_entry_score") {
                early_entry_scores.push(early_score);
            }
        }
        
        // Calculate key metrics
        let win_rate = profitable_trades as f64 / total_trades as f64;
        let avg_profit_percentage = if profitable_trades > 0 {
            profit_sum / profitable_trades as f64
        } else {
            0.0
        };
        
        let early_entry_score = if !early_entry_scores.is_empty() {
            early_entry_scores.iter().sum::<f64>() / early_entry_scores.len() as f64
        } else {
            0.0
        };
        
        // Calculate volume score (normalized)
        let avg_volume_per_trade = volume_sol / total_trades as f64;
        let volume_score = (avg_volume_per_trade / 10.0).min(1.0); // Normalize to 0-1, assuming 10 SOL is max normal
        
        // Check if wallet meets insider criteria
        if !self.meets_insider_criteria(win_rate, avg_profit_percentage, total_trades) {
            debug!("Wallet {} doesn't meet insider criteria: win_rate={:.3}, avg_profit={:.3}, trades={}", 
                   wallet_address, win_rate, avg_profit_percentage, total_trades);
            return Ok(None);
        }
        
        // Calculate confidence score using the existing algorithm
        let confidence_score = self.calculate_confidence_score(
            win_rate,
            avg_profit_percentage,
            early_entry_score,
            volume_score,
            last_trade_timestamp,
        );
        
        // Calculate recent activity score
        let recent_activity_score = self.calculate_recent_activity_score(&trade_rows);
        
        // Determine initial status based on performance
        let status = if confidence_score >= 0.80 && win_rate >= 0.80 {
            WalletStatus::Active
        } else if confidence_score >= 0.70 {
            WalletStatus::Monitoring  
        } else if recent_activity_score < 0.30 {
            WalletStatus::Cooldown
        } else {
            WalletStatus::Monitoring
        };
        
        let insider_wallet = InsiderWallet {
            address: wallet_address.to_string(),
            confidence_score,
            win_rate,
            avg_profit_percentage,
            early_entry_score,
            total_trades,
            profitable_trades,
            last_trade_timestamp,
            first_detected_timestamp: first_trade_timestamp,
            recent_activity_score,
            status: status.clone(),
        };
        
        info!("🎯 Analyzed wallet {}: confidence={:.3}, win_rate={:.3}, avg_profit={:.3}, status={:?}",
              &wallet_address[..8], confidence_score, win_rate, avg_profit_percentage, status);
        
        Ok(Some(insider_wallet))
    }
    
    /// Calculate recent activity score with time-based weighting
    fn calculate_recent_activity_score(&self, trades: &[sqlx::sqlite::SqliteRow]) -> f64 {
        if trades.is_empty() {
            return 0.0;
        }
        
        let now = Utc::now().timestamp();
        let mut weighted_score = 0.0;
        let mut total_weight = 0.0;
        
        for trade in trades {
            if let Some(exit_timestamp) = trade.try_get::<Option<i64>, _>("exit_timestamp").ok().flatten() {
                let days_ago = (now - exit_timestamp) as f64 / 86400.0;
                
                // Exponential decay: more recent trades have higher weight
                let weight = (-days_ago / 30.0).exp(); // 30-day decay constant
                let trade_score = if let Some(pnl) = trade.try_get::<Option<f64>, _>("pnl").ok().flatten() {
                    if pnl > 0.0 { 1.0 } else { 0.0 } // 1.0 for profitable, 0.0 for loss
                } else {
                    0.5 // Unknown outcome
                };
                
                weighted_score += trade_score * weight;
                total_weight += weight;
            }
        }
        
        if total_weight > 0.0 {
            weighted_score / total_weight
        } else {
            0.0
        }
    }
    
    /// Save insider wallet to database
    #[instrument(skip(self, insider))]
    pub async fn save_insider_wallet(&self, insider: &InsiderWallet) -> Result<(), DatabaseError> {
        let status_str = match insider.status {
            WalletStatus::Active => "ACTIVE",
            WalletStatus::Monitoring => "MONITORING",
            WalletStatus::Blacklisted => "BLACKLISTED",
            WalletStatus::Cooldown => "COOLDOWN",
        };
        
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO insider_wallets (
                address, confidence_score, win_rate, avg_profit_percentage,
                early_entry_score, total_trades, profitable_trades,
                last_trade_timestamp, first_detected_timestamp, recent_activity_score,
                status, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%s', 'now'))
            "#
        )
        .bind(&insider.address)
        .bind(insider.confidence_score)
        .bind(insider.win_rate)
        .bind(insider.avg_profit_percentage)
        .bind(insider.early_entry_score)
        .bind(insider.total_trades as i64)
        .bind(insider.profitable_trades as i64)
        .bind(insider.last_trade_timestamp)
        .bind(insider.first_detected_timestamp)
        .bind(insider.recent_activity_score)
        .bind(status_str)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to save insider wallet: {}", e)))?;
        
        debug!("💾 Saved insider wallet: {} (confidence: {:.3})", insider.address, insider.confidence_score);
        Ok(())
    }
    
    /// Get fresh insider scores for cache synchronization
    #[instrument(skip(self))]
    pub async fn calculate_fresh_insider_scores(&self) -> Result<Vec<(String, FreshInsiderScore)>, DatabaseError> {
        debug!("🔄 Starting fresh insider score recalculation");
        
        // Get all existing insider wallets
        let wallet_rows = sqlx::query(
            "SELECT address FROM insider_wallets ORDER BY confidence_score DESC"
        )
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get insider wallet addresses: {}", e)))?;
        
        let mut fresh_scores = Vec::new();
        
        for wallet_row in wallet_rows {
            let wallet_address: String = wallet_row.try_get("address")?;
            
            // Get recent trade data for this wallet (last 30 days for performance)
            let recent_cutoff = chrono::Utc::now().timestamp() - (30 * 24 * 3600);
            let trade_rows = sqlx::query(
                r#"
                SELECT 
                    trade_type, amount_sol, price, timestamp, token_launch_timestamp,
                    entry_delay_minutes, early_entry_score, trade_outcome, profit_percentage
                FROM wallet_trade_analysis 
                WHERE wallet_address = ?
                    AND timestamp >= ?
                    AND trade_outcome IN ('WIN', 'LOSS')
                ORDER BY timestamp DESC
                "#
            )
            .bind(&wallet_address)
            .bind(recent_cutoff)
            .fetch_all(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to get recent trades for wallet {}: {}", wallet_address, e)))?;
            
            if trade_rows.is_empty() {
                debug!("No recent trades for wallet {}, skipping score recalculation", &wallet_address[..8]);
                continue;
            }
            
            // Calculate fresh metrics
            let total_trades = trade_rows.len();
            let mut profitable_trades = 0;
            let mut profit_sum = 0.0f64;
            let mut early_entry_scores = Vec::new();
            let mut volume_sol = 0.0f64;
            let mut last_trade_timestamp = 0i64;
            
            for row in &trade_rows {
                let timestamp: i64 = row.try_get("timestamp")?;
                let amount_sol: f64 = row.try_get("amount_sol")?;
                let trade_outcome: String = row.try_get("trade_outcome")?;
                
                // Track latest trade
                last_trade_timestamp = last_trade_timestamp.max(timestamp);
                
                // Track volume
                volume_sol += amount_sol;
                
                // Track profitability
                if trade_outcome == "WIN" {
                    profitable_trades += 1;
                    if let Ok(profit_pct) = row.try_get::<f64, _>("profit_percentage") {
                        profit_sum += profit_pct;
                    }
                }
                
                // Track early entry patterns
                if let Ok(Some(early_score)) = row.try_get::<Option<f64>, _>("early_entry_score") {
                    early_entry_scores.push(early_score);
                }
            }
            
            // Calculate fresh metrics
            let win_rate = profitable_trades as f64 / total_trades as f64;
            let avg_profit = if profitable_trades > 0 {
                profit_sum / profitable_trades as f64
            } else {
                0.0
            };
            
            let early_entry_score = if !early_entry_scores.is_empty() {
                early_entry_scores.iter().sum::<f64>() / early_entry_scores.len() as f64
            } else {
                0.0
            };
            
            // Calculate volume score (normalized)
            let avg_volume_per_trade = volume_sol / total_trades as f64;
            let volume_score = (avg_volume_per_trade / 10.0).min(1.0);
            
            // Calculate fresh confidence score using existing algorithm
            let fresh_confidence = self.calculate_confidence_score(
                win_rate,
                avg_profit,
                early_entry_score,
                volume_score,
                last_trade_timestamp,
            );
            
            // Calculate fresh recent activity score
            let fresh_recent_activity = self.calculate_recent_activity_score(&trade_rows);
            
            let fresh_score = FreshInsiderScore {
                confidence: fresh_confidence,
                win_rate,
                avg_profit,
                recent_activity: fresh_recent_activity,
            };
            
            fresh_scores.push((wallet_address.clone(), fresh_score));
            
            debug!("Recalculated scores for wallet {}: confidence={:.3}, win_rate={:.3}, avg_profit={:.3}",
                   &wallet_address[..8], fresh_confidence, win_rate, avg_profit);
        }
        
        info!("📊 Calculated fresh scores for {} insider wallets", fresh_scores.len());
        Ok(fresh_scores)
    }
    
    /// Record trade for insider analysis
    pub async fn record_insider_trade(
        &self,
        wallet_address: &str,
        token_mint: &str,
        trade_data: &TradeData,
        token_launch_timestamp: Option<i64>,
    ) -> Result<(), DatabaseError> {
        let entry_delay_minutes = if let Some(launch_time) = token_launch_timestamp {
            Some(((trade_data.timestamp - launch_time) / 60).max(0) as i32)
        } else {
            None
        };
        
        let early_entry_score = entry_delay_minutes
            .map(|delay| self.calculate_early_entry_score(delay as u32))
            .unwrap_or(0.0);
        
        sqlx::query(
            r#"
            INSERT INTO wallet_trade_analysis (
                wallet_address, token_mint, trade_type, amount_sol, price,
                timestamp, token_launch_timestamp, entry_delay_minutes, 
                early_entry_score, trade_outcome
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'PENDING')
            "#
        )
        .bind(wallet_address)
        .bind(token_mint)
        .bind(&trade_data.trade_type)
        .bind(trade_data.amount_sol)
        .bind(trade_data.price)
        .bind(trade_data.timestamp)
        .bind(token_launch_timestamp)
        .bind(entry_delay_minutes)
        .bind(early_entry_score)
        .bind("PENDING")
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to record insider trade: {}", e)))?;
        
        Ok(())
    }
}