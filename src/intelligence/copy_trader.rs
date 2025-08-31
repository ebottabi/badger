/// Copy Trading Engine
/// 
/// This module generates copy trading signals based on insider wallet activity
/// and integrates with the Strike service for trade execution. It handles:
/// - Buy signal generation when insiders enter positions
/// - Sell signal generation when insiders exit
/// - Position sizing based on confidence scores
/// - Signal timing and urgency management

use super::types::*;
use super::cache::WalletIntelligenceCache;
use crate::core::{TradingSignal, SignalSource};
use crate::database::{BadgerDatabase, DatabaseError};
use std::sync::Arc;
use tokio::sync::mpsc;
use sqlx::Row;
use chrono::Utc;
use tracing::{info, debug, warn, error, instrument};

/// Copy trading signal generation engine
pub struct CopyTradingEngine {
    /// Cache for instant insider lookups
    cache: Arc<WalletIntelligenceCache>,
    
    /// Database for persistent signal tracking
    db: Arc<BadgerDatabase>,
    
    /// Channel to send trading signals to Strike service
    signal_sender: mpsc::UnboundedSender<TradingSignal>,
    
    /// Configuration
    config: CopyTradingConfig,
}

/// Configuration for copy trading
#[derive(Debug, Clone)]
pub struct CopyTradingConfig {
    /// Minimum confidence score to copy trade
    pub min_confidence_threshold: f64,
    
    /// Maximum token age in minutes to copy
    pub max_token_age_minutes: u32,
    
    /// Base position size in SOL
    pub base_position_sol: f64,
    
    /// Maximum position size multiplier
    pub max_position_multiplier: f64,
    
    /// Enable copy trading (master switch)
    pub copy_trading_enabled: bool,
    
    /// Maximum daily copy trades
    pub max_daily_copy_trades: u32,
    
    /// Minimum SOL balance required to copy trade
    pub min_sol_balance: f64,
}

impl Default for CopyTradingConfig {
    fn default() -> Self {
        Self {
            min_confidence_threshold: 0.75,
            max_token_age_minutes: 30,
            base_position_sol: 0.1,
            max_position_multiplier: 2.0,
            copy_trading_enabled: true,
            max_daily_copy_trades: 50,
            min_sol_balance: 1.0,
        }
    }
}

impl CopyTradingEngine {
    /// Create new copy trading engine with database reference
    pub fn new(
        signal_sender: mpsc::UnboundedSender<TradingSignal>,
        cache: Arc<WalletIntelligenceCache>,
        db: Arc<BadgerDatabase>,
    ) -> Self {
        Self {
            cache,
            db,
            signal_sender,
            config: CopyTradingConfig::default(),
        }
    }
    
    
    /// Create with custom configuration
    pub fn with_config(
        signal_sender: mpsc::UnboundedSender<TradingSignal>,
        cache: Arc<WalletIntelligenceCache>,
        db: Arc<BadgerDatabase>,
        config: CopyTradingConfig,
    ) -> Self {
        Self {
            cache,
            db,
            signal_sender,
            config,
        }
    }
    
    /// Initialize database schema for copy trading
    #[instrument(skip(self))]
    pub async fn initialize_schema(&self) -> Result<(), DatabaseError> {
        info!("🔧 Initializing copy trading database schema");
        
        let create_copy_signals_table = r#"
            CREATE TABLE IF NOT EXISTS copy_trading_signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                insider_wallet TEXT NOT NULL,
                token_mint TEXT NOT NULL,
                signal_type TEXT NOT NULL CHECK (signal_type IN ('BUY', 'SELL')),
                insider_confidence REAL NOT NULL,
                position_size_sol REAL NOT NULL,
                copy_delay_seconds INTEGER NOT NULL,
                urgency TEXT NOT NULL CHECK (urgency IN ('IMMEDIATE', 'HIGH', 'NORMAL')),
                signal_timestamp INTEGER NOT NULL,
                execution_timestamp INTEGER,
                execution_status TEXT CHECK (execution_status IN ('PENDING', 'EXECUTED', 'FAILED', 'SKIPPED', 'TIMEOUT')),
                our_position_id INTEGER,
                execution_price REAL,
                slippage_percentage REAL,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                
                FOREIGN KEY (our_position_id) REFERENCES positions (id),
                FOREIGN KEY (insider_wallet) REFERENCES insider_wallets (address)
            )
        "#;
        
        let create_copy_performance_table = r#"
            CREATE TABLE IF NOT EXISTS copy_trading_performance (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                insider_wallet TEXT NOT NULL,
                copy_signal_id INTEGER NOT NULL,
                token_mint TEXT NOT NULL,
                our_entry_price REAL,
                our_exit_price REAL,
                profit_loss_sol REAL,
                profit_percentage REAL,
                hold_duration_seconds INTEGER,
                result TEXT CHECK (result IN ('WIN', 'LOSS', 'PENDING')),
                exit_reason TEXT CHECK (exit_reason IN ('INSIDER_EXIT', 'TAKE_PROFIT', 'STOP_LOSS', 'TIME_DECAY', 'MANUAL')),
                insider_exit_price REAL,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                
                FOREIGN KEY (copy_signal_id) REFERENCES copy_trading_signals (id),
                FOREIGN KEY (insider_wallet) REFERENCES insider_wallets (address)
            )
        "#;
        
        // Create indexes for performance
        let create_indexes = vec![
            "CREATE INDEX IF NOT EXISTS idx_copy_signals_timestamp ON copy_trading_signals(signal_timestamp DESC)",
            "CREATE INDEX IF NOT EXISTS idx_copy_signals_status ON copy_trading_signals(execution_status)",
            "CREATE INDEX IF NOT EXISTS idx_copy_signals_insider ON copy_trading_signals(insider_wallet)",
            "CREATE INDEX IF NOT EXISTS idx_copy_performance_wallet ON copy_trading_performance(insider_wallet)",
            "CREATE INDEX IF NOT EXISTS idx_copy_performance_result ON copy_trading_performance(result)",
        ];
        
        // Execute schema creation
        sqlx::query(create_copy_signals_table)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to create copy_trading_signals table: {}", e)))?;
        
        sqlx::query(create_copy_performance_table)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to create copy_trading_performance table: {}", e)))?;
        
        for index_sql in create_indexes {
            sqlx::query(index_sql)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to create index: {}", e)))?;
        }
        
        info!("✅ Copy trading database schema initialized");
        Ok(())
    }
    
    /// Generate buy signal when insider enters position
    #[instrument(skip(self))]
    pub async fn generate_buy_signal(
        &self,
        insider_wallet: &str,
        token_mint: &str,
        insider_entry_price: f64,
        copy_decision: CopyDecision,
        timestamp: i64,
    ) -> Result<(), DatabaseError> {
        if !self.config.copy_trading_enabled {
            debug!("Copy trading disabled, skipping signal for {}", insider_wallet);
            return Ok(());
        }
        
        // Check daily limits
        if let Err(e) = self.check_daily_limits().await {
            warn!("Daily copy trading limits exceeded: {}", e);
            return Ok(());
        }
        
        // Calculate token age for early entry bonus
        let token_age_minutes = self.cache.get_token_age_minutes(token_mint, timestamp).await.unwrap_or(0);
        
        // Create copy trading signal
        let copy_signal = CopyTradingSignal {
            insider_wallet: insider_wallet.to_string(),
            token_mint: token_mint.to_string(),
            signal_type: CopySignalType::Buy {
                insider_entry_price,
                token_launch_delay_minutes: token_age_minutes,
            },
            insider_confidence: copy_decision.confidence,
            position_size_sol: copy_decision.position_size,
            copy_delay_seconds: copy_decision.delay_seconds,
            urgency: copy_decision.urgency.clone(),
            timestamp,
        };
        
        // Save signal to database
        let signal_id = self.save_copy_signal(&copy_signal).await?;
        
        // Create trading signal for Strike service
        let trading_signal = TradingSignal::Buy {
            token_mint: token_mint.to_string(),
            confidence: copy_decision.confidence,
            max_amount_sol: copy_decision.position_size,
            reason: format!("Copy trading insider wallet: {}", insider_wallet),
            source: SignalSource::InsiderCopy,
            amount_sol: Some(copy_decision.position_size),
            max_slippage: Some(0.05), // 5% max slippage
            metadata: Some(format!("insider:{},signal_id:{}", insider_wallet, signal_id)),
        };
        
        // Add delay before sending signal (based on confidence)
        if copy_decision.delay_seconds > 0 {
            let delay_duration = std::time::Duration::from_secs(copy_decision.delay_seconds as u64);
            
            // Spawn delayed execution
            let signal_sender = self.signal_sender.clone();
            let delayed_signal = trading_signal;
            let db = self.db.clone();
            
            tokio::spawn(async move {
                tokio::time::sleep(delay_duration).await;
                
                // Send trading signal
                if let Err(e) = signal_sender.send(delayed_signal) {
                    error!("Failed to send delayed copy trading signal: {}", e);
                } else {
                    // Update execution timestamp
                    let _ = sqlx::query(
                        "UPDATE copy_trading_signals SET execution_timestamp = ?, execution_status = 'EXECUTED' WHERE id = ?"
                    )
                    .bind(Utc::now().timestamp())
                    .bind(signal_id)
                    .execute(db.get_pool())
                    .await;
                }
            });
        } else {
            // Send immediately
            self.signal_sender.send(trading_signal)
                .map_err(|e| DatabaseError::QueryError(format!("Failed to send copy trading signal: {}", e)))?;
            
            // Update execution timestamp
            sqlx::query(
                "UPDATE copy_trading_signals SET execution_timestamp = ?, execution_status = 'EXECUTED' WHERE id = ?"
            )
            .bind(timestamp)
            .bind(signal_id)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to update signal execution: {}", e)))?;
        }
        
        info!("📈 Generated copy buy signal: wallet={}, token={}, size={:.4} SOL, confidence={:.3}", 
              insider_wallet, token_mint, copy_decision.position_size, copy_decision.confidence);
        
        Ok(())
    }
    
    /// Generate sell signal when insider exits position
    #[instrument(skip(self))]
    pub async fn generate_sell_signal(
        &self,
        insider_wallet: &str,
        token_mint: &str,
        insider_exit_price: f64,
        insider_profit_percentage: f64,
        timestamp: i64,
    ) -> Result<(), DatabaseError> {
        if !self.config.copy_trading_enabled {
            return Ok(());
        }
        
        // Check if we have an open position for this token
        let open_position = sqlx::query(
            r#"
            SELECT id, entry_price, quantity 
            FROM positions 
            WHERE token_mint = ? AND status = 'OPEN' 
            ORDER BY entry_timestamp DESC LIMIT 1
            "#
        )
        .bind(token_mint)
        .fetch_optional(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to check open position: {}", e)))?;
        
        if let Some(position) = open_position {
            // Calculate our potential profit
            let entry_price: Option<f64> = position.try_get("entry_price").ok();
            let quantity: Option<f64> = position.try_get("quantity").ok();
            let our_profit_percentage = (insider_exit_price / entry_price.unwrap_or(1.0)) - 1.0;
            
            // Only sell if we have decent profit and the insider has good profit
            if our_profit_percentage >= 0.30 && insider_profit_percentage >= 0.40 {
                // Get insider confidence for decision making
                if let Some(confidence) = self.cache.is_insider(insider_wallet).await {
                    if confidence >= 0.75 {
                        let copy_signal = CopyTradingSignal {
                            insider_wallet: insider_wallet.to_string(),
                            token_mint: token_mint.to_string(),
                            signal_type: CopySignalType::Sell {
                                insider_exit_price,
                                insider_profit_percentage,
                            },
                            insider_confidence: confidence,
                            position_size_sol: 0.0, // Not applicable for sell
                            copy_delay_seconds: 5,   // Quick sell
                            urgency: SignalUrgency::High,
                            timestamp,
                        };
                        
                        // Save signal to database
                        let signal_id = self.save_copy_signal(&copy_signal).await?;
                        
                        // Create sell trading signal
                        let trading_signal = TradingSignal::Sell {
                            token_mint: token_mint.to_string(),
                            price_target: insider_exit_price,
                            stop_loss: insider_exit_price * 0.90, // 10% stop loss
                            reason: "insider_exit".to_string(),
                            amount_tokens: Some(quantity.unwrap_or(0.0)),
                            min_price: Some(insider_exit_price * 0.95), // 5% below insider exit price
                            source: Some(SignalSource::InsiderCopy),
                            metadata: Some(format!("insider:{},signal_id:{}", insider_wallet, signal_id)),
                        };
                        
                        // Send sell signal immediately (urgent)
                        self.signal_sender.send(trading_signal)
                            .map_err(|e| DatabaseError::QueryError(format!("Failed to send copy sell signal: {}", e)))?;
                        
                        // Update execution timestamp
                        sqlx::query(
                            "UPDATE copy_trading_signals SET execution_timestamp = ?, execution_status = 'EXECUTED' WHERE id = ?"
                        )
                        .bind(timestamp)
                        .bind(signal_id)
                        .execute(self.db.get_pool())
                        .await
                        .map_err(|e| DatabaseError::QueryError(format!("Failed to update sell signal execution: {}", e)))?;
                        
                        info!("📉 Generated copy sell signal: wallet={}, token={}, our_profit={:.2}%, insider_profit={:.2}%", 
                              insider_wallet, token_mint, our_profit_percentage * 100.0, insider_profit_percentage * 100.0);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Save copy trading signal to database
    async fn save_copy_signal(&self, signal: &CopyTradingSignal) -> Result<i64, DatabaseError> {
        let signal_type_str = match signal.signal_type {
            CopySignalType::Buy { .. } => "BUY",
            CopySignalType::Sell { .. } => "SELL",
        };
        
        let urgency_str = match signal.urgency {
            SignalUrgency::Immediate => "IMMEDIATE",
            SignalUrgency::High => "HIGH",
            SignalUrgency::Normal => "NORMAL",
            SignalUrgency::Low => "LOW",
        };
        
        let result = sqlx::query(
            r#"
            INSERT INTO copy_trading_signals (
                insider_wallet, token_mint, signal_type, insider_confidence,
                position_size_sol, copy_delay_seconds, urgency, signal_timestamp,
                execution_status
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'PENDING')
            "#
        )
        .bind(&signal.insider_wallet)
        .bind(&signal.token_mint)
        .bind(signal_type_str)
        .bind(signal.insider_confidence)
        .bind(signal.position_size_sol)
        .bind(signal.copy_delay_seconds as i64)
        .bind(urgency_str)
        .bind(signal.timestamp)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to save copy signal: {}", e)))?;
        
        let signal_id = result.last_insert_rowid();
        
        Ok(signal_id)
    }
    
    /// Check if we're within daily copy trading limits
    async fn check_daily_limits(&self) -> Result<(), DatabaseError> {
        let today_start = Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap().timestamp();
        
        let today_signals = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM copy_trading_signals WHERE signal_timestamp >= ? AND signal_type = 'BUY'",
        )
        .bind(today_start)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to check daily limits: {}", e)))?;
        
        if today_signals >= self.config.max_daily_copy_trades as i64 {
            return Err(DatabaseError::QueryError(format!(
                "Daily copy trading limit exceeded: {}/{}", 
                today_signals, self.config.max_daily_copy_trades
            )));
        }
        
        Ok(())
    }
    
    /// Get copy trading statistics for monitoring
    pub async fn get_copy_trading_stats(&self) -> Result<CopyTradingStats, DatabaseError> {
        let today_start = Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap().timestamp();
        let week_start = today_start - (7 * 24 * 3600);
        
        // Today's signals
        let today_signals = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM copy_trading_signals WHERE signal_timestamp >= ?",
        )
        .bind(today_start)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get today's signals: {}", e)))?;
        
        // Get comprehensive weekly performance statistics
        let week_stats_result = sqlx::query(
            r#"
            SELECT 
                COUNT(*) as total_trades,
                SUM(CASE WHEN result = 'WIN' THEN 1 ELSE 0 END) as winning_trades,
                SUM(COALESCE(profit_loss_sol, 0.0)) as total_pnl
            FROM copy_trading_performance 
            WHERE created_at >= ?
            "#
        )
        .bind(week_start)
        .fetch_optional(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get week performance: {}", e)))?;
        
        let (total_trades, win_rate, week_total_pnl_sol) = if let Some(row) = week_stats_result {
            let total: i64 = row.try_get("total_trades").unwrap_or(0);
            let wins: i64 = row.try_get("winning_trades").unwrap_or(0);
            let pnl: f64 = row.try_get("total_pnl").unwrap_or(0.0);
            
            let win_rate = if total > 0 {
                wins as f64 / total as f64
            } else {
                0.0
            };
            
            (total, win_rate, pnl)
        } else {
            (0, 0.0, 0.0)
        };
        
        Ok(CopyTradingStats {
            enabled: self.config.copy_trading_enabled,
            today_signals: today_signals as u32,
            daily_limit: self.config.max_daily_copy_trades,
            week_total_trades: total_trades as u32,
            week_win_rate: win_rate,
            week_total_pnl_sol: week_total_pnl_sol,
            min_confidence_threshold: self.config.min_confidence_threshold,
            base_position_sol: self.config.base_position_sol,
        })
    }
    
    /// Update configuration
    pub fn update_config(&mut self, new_config: CopyTradingConfig) {
        self.config = new_config;
        info!("🔧 Copy trading configuration updated");
    }
    
    /// Enable/disable copy trading
    pub fn set_enabled(&mut self, enabled: bool) {
        self.config.copy_trading_enabled = enabled;
        info!("🎯 Copy trading {}", if enabled { "enabled" } else { "disabled" });
    }
}

/// Copy trading statistics
#[derive(Debug, Clone)]
pub struct CopyTradingStats {
    pub enabled: bool,
    pub today_signals: u32,
    pub daily_limit: u32,
    pub week_total_trades: u32,
    pub week_win_rate: f64,
    pub week_total_pnl_sol: f64,
    pub min_confidence_threshold: f64,
    pub base_position_sol: f64,
}