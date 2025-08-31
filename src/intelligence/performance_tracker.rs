/// Performance Tracker for Copy Trading Feedback Loop
/// 
/// This module tracks the performance of copy trading activities and provides
/// feedback to improve insider wallet scoring and selection. It handles:
/// - Copy trade result recording
/// - Performance analytics calculation
/// - Insider wallet score feedback
/// - Success rate monitoring

use super::types::*;
use crate::database::{BadgerDatabase, DatabaseError};
use std::sync::Arc;
use sqlx::Row;
use chrono::Utc;
use tracing::{info, debug, warn, error, instrument};

/// Performance tracking and analytics engine
pub struct PerformanceTracker {
    /// Database connection
    db: Arc<BadgerDatabase>,
    
    /// Configuration
    config: PerformanceConfig,
}

/// Configuration for performance tracking
#[derive(Debug, Clone)]
pub struct PerformanceConfig {
    /// Minimum trades required for reliable statistics
    pub min_trades_for_stats: u32,
    
    /// Time window for recent performance calculation (days)
    pub recent_performance_days: u32,
    
    /// Minimum win rate to maintain active status
    pub min_win_rate_threshold: f64,
    
    /// Minimum profit percentage to count as a win
    pub min_profit_threshold: f64,
    
    /// Performance decay rate for time weighting
    pub performance_decay_rate: f64,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            min_trades_for_stats: 5,
            recent_performance_days: 14,
            min_win_rate_threshold: 0.60,
            min_profit_threshold: 0.30,
            performance_decay_rate: 0.1,
        }
    }
}

impl PerformanceTracker {
    /// Create new performance tracker
    pub fn new(db: Arc<BadgerDatabase>) -> Self {
        Self {
            db,
            config: PerformanceConfig::default(),
        }
    }
    
    /// Create with custom configuration
    pub fn with_config(db: Arc<BadgerDatabase>, config: PerformanceConfig) -> Self {
        Self { db, config }
    }
    
    /// Initialize database schema for performance tracking
    #[instrument(skip(self))]
    pub async fn initialize_schema(&self) -> Result<(), DatabaseError> {
        info!("🔧 Initializing performance tracker database schema");
        
        // The copy_trading_performance table is created by the copy_trader module
        // We just need to ensure our indexes are created
        let create_indexes = vec![
            "CREATE INDEX IF NOT EXISTS idx_performance_wallet_date ON copy_trading_performance(insider_wallet, created_at DESC)",
            "CREATE INDEX IF NOT EXISTS idx_performance_result_date ON copy_trading_performance(result, created_at DESC)",
            "CREATE INDEX IF NOT EXISTS idx_performance_token_date ON copy_trading_performance(token_mint, created_at DESC)",
        ];
        
        for index_sql in create_indexes {
            sqlx::query(index_sql)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to create performance index: {}", e)))?;
        }
        
        info!("✅ Performance tracker database schema initialized");
        Ok(())
    }
    
    /// Record copy trade result for performance analysis
    #[instrument(skip(self, result))]
    pub async fn record_copy_result(&self, copy_signal_id: i64, result: CopyTradeResult) -> Result<(), DatabaseError> {
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
        
        // Insert performance record
        sqlx::query(
            r#"
            INSERT INTO copy_trading_performance (
                insider_wallet, copy_signal_id, token_mint,
                our_entry_price, our_exit_price, profit_loss_sol,
                profit_percentage, hold_duration_seconds, result,
                exit_reason, insider_exit_price
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(&result.insider_wallet)
        .bind(copy_signal_id)
        .bind(&result.token_mint)
        .bind(result.our_entry_price)
        .bind(result.our_exit_price)
        .bind(result.profit_loss_sol)
        .bind(result.profit_percentage)
        .bind(result.hold_duration_seconds.unwrap_or(0))
        .bind(result_str)
        .bind(exit_reason_str)
        .bind(result.our_exit_price) // Using our exit price as proxy for insider exit price
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to record copy result: {}", e)))?;
        
        // Update copy trading signal status
        let signal_status = match result.result {
            TradeResult::Win | TradeResult::Loss => "COMPLETED",
            TradeResult::Pending => "EXECUTED",
        };
        
        sqlx::query(
            "UPDATE copy_trading_signals SET execution_status = ? WHERE id = ?"
        )
        .bind(signal_status)
        .bind(copy_signal_id)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to update signal status: {}", e)))?;
        
        // Update insider wallet copy trading stats
        self.update_insider_copy_stats(&result.insider_wallet).await?;
        
        debug!("📊 Recorded copy trade result: wallet={}, result={:?}, profit={:.4} SOL", 
               result.insider_wallet, result.result, result.profit_loss_sol.unwrap_or(0.0));
        
        Ok(())
    }
    
    /// Update insider wallet copy trading statistics
    #[instrument(skip(self))]
    async fn update_insider_copy_stats(&self, insider_wallet: &str) -> Result<(), DatabaseError> {
        // Get comprehensive copy trading statistics for this insider wallet
        let copy_stats = sqlx::query(
            r#"
            SELECT 
                COUNT(*) as total_copied,
                SUM(CASE WHEN result = 'WIN' THEN 1 ELSE 0 END) as successful_copied,
                SUM(COALESCE(profit_loss_sol, 0.0)) as total_copy_profit
            FROM copy_trading_performance 
            WHERE insider_wallet = ?
            "#
        )
        .bind(insider_wallet)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get copy stats for insider {}: {}", insider_wallet, e)))?;
        
        let total_copied = copy_stats.try_get::<i64, _>("total_copied")
            .map_err(|e| DatabaseError::QueryError(format!("Failed to parse total_copied: {}", e)))?;
        let successful_copied = copy_stats.try_get::<i64, _>("successful_copied")
            .map_err(|e| DatabaseError::QueryError(format!("Failed to parse successful_copied: {}", e)))?;
        let total_copy_profit = copy_stats.try_get::<f64, _>("total_copy_profit")
            .map_err(|e| DatabaseError::QueryError(format!("Failed to parse total_copy_profit: {}", e)))?;
        
        // Update insider wallet record
        sqlx::query(
            r#"
            UPDATE insider_wallets 
            SET total_copied_trades = ?, 
                successful_copied_trades = ?,
                total_copy_profit_sol = ?,
                updated_at = strftime('%s', 'now')
            WHERE address = ?
            "#
        )
        .bind(total_copied)
        .bind(successful_copied as i64)
        .bind(total_copy_profit)
        .bind(insider_wallet)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to update insider copy stats: {}", e)))?;
        
        Ok(())
    }
    
    /// Calculate performance metrics for an insider wallet
    #[instrument(skip(self))]
    pub async fn calculate_wallet_performance_metrics(&self, insider_wallet: &str) -> Result<Option<WalletPerformanceMetrics>, DatabaseError> {
        let recent_cutoff = Utc::now().timestamp() - (self.config.recent_performance_days as i64 * 24 * 3600);
        
        // Simplified performance calculation to avoid compilation issues
        let total_trades = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM copy_trading_performance WHERE insider_wallet = ?"
        )
        .bind(insider_wallet)
        .fetch_one(self.db.get_pool())
        .await
        .unwrap_or(0);
        
        if total_trades == 0 {
            return Ok(None);
        }
        
        // Get comprehensive performance data for statistical analysis
        let performance_data = sqlx::query(
            r#"
            SELECT 
                result, profit_loss_sol, profit_percentage, hold_duration_seconds,
                exit_reason, created_at
            FROM copy_trading_performance 
            WHERE insider_wallet = ?
            ORDER BY created_at DESC
            "#
        )
        .bind(insider_wallet)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get performance data: {}", e)))?;
        
        if performance_data.is_empty() {
            return Ok(None);
        }
        
        // Statistical analysis variables
        let mut wins = 0u32;
        let mut losses = 0u32;
        let mut total_profit = 0.0f64;
        let mut total_loss = 0.0f64;
        let mut recent_wins = 0u32;
        let mut recent_trades = 0u32;
        let mut hold_durations = Vec::new();
        let mut profit_returns = Vec::new();
        let mut exit_reasons: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
        
        let recent_cutoff = Utc::now().timestamp() - (self.config.recent_performance_days as i64 * 24 * 3600);
        
        // Process each trade for statistical analysis
        for trade in &performance_data {
            let result: String = trade.try_get("result").unwrap_or_else(|_| "PENDING".to_string());
            let profit_loss: Option<f64> = trade.try_get("profit_loss_sol").ok();
            let profit_percentage: Option<f64> = trade.try_get("profit_percentage").ok();
            let hold_duration: Option<i64> = trade.try_get("hold_duration_seconds").ok();
            let exit_reason: String = trade.try_get("exit_reason").unwrap_or_else(|_| "UNKNOWN".to_string());
            let created_at: i64 = trade.try_get("created_at").unwrap_or(0);
            
            // Count wins/losses
            match result.as_str() {
                "WIN" => {
                    wins += 1;
                    if let Some(profit) = profit_loss {
                        if profit > 0.0 {
                            total_profit += profit;
                        }
                    }
                },
                "LOSS" => {
                    losses += 1;
                    if let Some(loss) = profit_loss {
                        if loss < 0.0 {
                            total_loss += loss.abs();
                        }
                    }
                },
                _ => {} // PENDING or other
            }
            
            // Track recent performance (last N days)
            if created_at >= recent_cutoff {
                recent_trades += 1;
                if result == "WIN" {
                    recent_wins += 1;
                }
            }
            
            // Collect hold durations for analysis
            if let Some(duration) = hold_duration {
                if duration > 0 {
                    hold_durations.push(duration);
                }
            }
            
            // Collect profit percentages for Sharpe ratio calculation
            if let Some(profit_pct) = profit_percentage {
                profit_returns.push(profit_pct);
            }
            
            // Count exit reasons
            *exit_reasons.entry(exit_reason).or_insert(0) += 1;
        }
        
        // Calculate core metrics
        let total_count = wins + losses;
        let win_rate = if total_count > 0 { wins as f64 / total_count as f64 } else { 0.0 };
        let recent_win_rate = if recent_trades > 0 { recent_wins as f64 / recent_trades as f64 } else { 0.0 };
        
        let avg_profit_per_win = if wins > 0 { total_profit / wins as f64 } else { 0.0 };
        let avg_loss_per_loss = if losses > 0 { total_loss / losses as f64 } else { 0.0 };
        
        let profit_factor = if total_loss > 0.0 { 
            total_profit / total_loss 
        } else if total_profit > 0.0 { 
            100.0 // Infinite profit factor (no losses)
        } else { 
            0.0
        };
        
        let avg_hold_duration = if !hold_durations.is_empty() {
            hold_durations.iter().sum::<i64>() as f64 / hold_durations.len() as f64
        } else {
            0.0
        };
        
        // Calculate Sharpe ratio (simplified version)
        let sharpe_ratio = if profit_returns.len() > 1 {
            let mean_return = profit_returns.iter().sum::<f64>() / profit_returns.len() as f64;
            let variance = profit_returns.iter()
                .map(|r| (r - mean_return).powi(2))
                .sum::<f64>() / (profit_returns.len() - 1) as f64;
            let std_dev = variance.sqrt();
            
            if std_dev > 0.0 { 
                mean_return / std_dev 
            } else { 
                0.0 
            }
        } else {
            0.0
        };
        
        // Calculate composite performance score
        let performance_score = self.calculate_performance_score(
            win_rate,
            recent_win_rate,
            profit_factor,
            sharpe_ratio,
            total_count,
        );
        
        Ok(Some(WalletPerformanceMetrics {
            insider_wallet: insider_wallet.to_string(),
            total_copy_trades: total_trades as u32,
            wins,
            losses,
            win_rate,
            recent_win_rate,
            total_profit_sol: total_profit,
            total_loss_sol: total_loss,
            net_profit_sol: total_profit - total_loss,
            avg_profit_per_win,
            avg_loss_per_loss,
            profit_factor,
            sharpe_ratio,
            avg_hold_duration_seconds: avg_hold_duration,
            performance_score,
            recent_trades,
            exit_reason_breakdown: exit_reasons,
            last_updated: Utc::now().timestamp(),
        }))
    }
    
    /// Calculate composite performance score (0.0-1.0)
    fn calculate_performance_score(
        &self,
        win_rate: f64,
        recent_win_rate: f64,
        profit_factor: f64,
        sharpe_ratio: f64,
        total_trades: u32,
    ) -> f64 {
        // Weights for different components
        let win_rate_weight = 0.3;
        let recent_win_rate_weight = 0.2;
        let profit_factor_weight = 0.2;
        let sharpe_weight = 0.2;
        let experience_weight = 0.1;
        
        // Normalize profit factor (cap at 10.0)
        let normalized_profit_factor = (profit_factor / 10.0).min(1.0);
        
        // Normalize Sharpe ratio (cap at 3.0)
        let normalized_sharpe = ((sharpe_ratio + 3.0) / 6.0).min(1.0).max(0.0);
        
        // Experience factor based on number of trades
        let experience_factor = (total_trades as f64 / 50.0).min(1.0);
        
        // Calculate composite score
        let base_score = 
            win_rate_weight * win_rate +
            recent_win_rate_weight * recent_win_rate +
            profit_factor_weight * normalized_profit_factor +
            sharpe_weight * normalized_sharpe +
            experience_weight * experience_factor;
        
        base_score.min(1.0).max(0.0)
    }
    
    /// Get overall copy trading performance summary
    #[instrument(skip(self))]
    pub async fn get_overall_performance_summary(&self) -> Result<OverallPerformanceSummary, DatabaseError> {
        let recent_cutoff = Utc::now().timestamp() - (self.config.recent_performance_days as i64 * 24 * 3600);
        
        // Get overall statistics across all copy trades
        let overall_stats = sqlx::query(
            r#"
            SELECT 
                COUNT(*) as total_trades,
                SUM(CASE WHEN result = 'WIN' THEN 1 ELSE 0 END) as winning_trades,
                SUM(COALESCE(profit_loss_sol, 0.0)) as total_pnl,
                COUNT(DISTINCT DATE(created_at, 'unixepoch')) as active_days
            FROM copy_trading_performance
            "#
        )
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get overall stats: {}", e)))?;
        
        // Get recent performance statistics
        let recent_stats = sqlx::query(
            r#"
            SELECT 
                COUNT(*) as recent_trades,
                SUM(CASE WHEN result = 'WIN' THEN 1 ELSE 0 END) as recent_wins,
                SUM(COALESCE(profit_loss_sol, 0.0)) as recent_pnl
            FROM copy_trading_performance
            WHERE created_at >= ?
            "#
        )
        .bind(recent_cutoff)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get recent stats: {}", e)))?;
        
        // Get top performing insider wallets for the summary
        let top_performers = sqlx::query(
            r#"
            SELECT 
                insider_wallet,
                COUNT(*) as trades,
                SUM(CASE WHEN result = 'WIN' THEN 1 ELSE 0 END) as wins,
                SUM(COALESCE(profit_loss_sol, 0.0)) as total_pnl,
                (SUM(CASE WHEN result = 'WIN' THEN 1.0 ELSE 0.0 END) / COUNT(*)) as win_rate
            FROM copy_trading_performance
            WHERE created_at >= ?
            GROUP BY insider_wallet
            HAVING COUNT(*) >= 5 AND win_rate >= 0.6
            ORDER BY total_pnl DESC
            LIMIT 5
            "#
        )
        .bind(recent_cutoff)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get top performers: {}", e)))?;
        
        // Extract overall statistics
        let total_trades: i64 = overall_stats.try_get("total_trades").unwrap_or(0);
        let winning_trades: i64 = overall_stats.try_get("winning_trades").unwrap_or(0);
        let total_pnl: f64 = overall_stats.try_get("total_pnl").unwrap_or(0.0);
        let active_days: i64 = overall_stats.try_get("active_days").unwrap_or(0);
        
        // Extract recent statistics
        let recent_trades: i64 = recent_stats.try_get("recent_trades").unwrap_or(0);
        let recent_wins: i64 = recent_stats.try_get("recent_wins").unwrap_or(0);
        let recent_pnl: f64 = recent_stats.try_get("recent_pnl").unwrap_or(0.0);
        
        // Calculate rates
        let overall_win_rate = if total_trades > 0 { winning_trades as f64 / total_trades as f64 } else { 0.0 };
        let recent_win_rate = if recent_trades > 0 { recent_wins as f64 / recent_trades as f64 } else { 0.0 };
        let avg_pnl_per_trade = if total_trades > 0 { total_pnl / total_trades as f64 } else { 0.0 };
        
        // Build top performers list
        let mut top_performers_list = Vec::new();
        for performer in top_performers {
            let wallet: String = performer.try_get("insider_wallet").unwrap_or_default();
            let trades: i64 = performer.try_get("trades").unwrap_or(0);
            let wins: i64 = performer.try_get("wins").unwrap_or(0);
            let pnl: f64 = performer.try_get("total_pnl").unwrap_or(0.0);
            let win_rate: f64 = performer.try_get("win_rate").unwrap_or(0.0);
            
            top_performers_list.push(TopPerformerSummary {
                insider_wallet: wallet,
                trades: trades as u32,
                win_rate,
                total_profit_sol: pnl,
            });
        }
        
        Ok(OverallPerformanceSummary {
            total_copy_trades: total_trades as u32,
            overall_win_rate,
            recent_win_rate,
            total_pnl_sol: total_pnl,
            recent_pnl_sol: recent_pnl,
            avg_pnl_per_trade,
            recent_trades: recent_trades as u32,
            active_days: active_days as u32,
            top_performers: top_performers_list,
            last_updated: Utc::now().timestamp(),
        })
    }
    
    /// Generate performance feedback for insider score adjustment
    pub async fn generate_performance_feedback(&self, insider_wallet: &str) -> Result<Option<PerformanceFeedback>, DatabaseError> {
        if let Some(metrics) = self.calculate_wallet_performance_metrics(insider_wallet).await? {
            let mut feedback = PerformanceFeedback {
                insider_wallet: insider_wallet.to_string(),
                score_adjustment: 0.0,
                status_recommendation: None,
                feedback_reason: String::new(),
                confidence: 0.0,
            };
            
            // Determine score adjustment based on performance
            if metrics.total_copy_trades >= self.config.min_trades_for_stats {
                if metrics.win_rate >= 0.70 && metrics.profit_factor >= 2.0 {
                    feedback.score_adjustment = 0.05; // Increase confidence
                    feedback.status_recommendation = Some(WalletStatus::Active);
                    feedback.feedback_reason = "Excellent copy trading performance".to_string();
                    feedback.confidence = 0.9;
                } else if metrics.win_rate < 0.40 || metrics.profit_factor < 0.5 {
                    feedback.score_adjustment = -0.10; // Decrease confidence
                    feedback.status_recommendation = Some(WalletStatus::Blacklisted);
                    feedback.feedback_reason = "Poor copy trading performance".to_string();
                    feedback.confidence = 0.8;
                } else if metrics.recent_win_rate < 0.30 && metrics.recent_trades >= 5 {
                    feedback.score_adjustment = -0.05; // Slight decrease
                    feedback.status_recommendation = Some(WalletStatus::Cooldown);
                    feedback.feedback_reason = "Recent performance decline".to_string();
                    feedback.confidence = 0.7;
                }
            }
            
            Ok(Some(feedback))
        } else {
            Ok(None)
        }
    }
}

/// Wallet performance metrics
#[derive(Debug, Clone)]
pub struct WalletPerformanceMetrics {
    pub insider_wallet: String,
    pub total_copy_trades: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub recent_win_rate: f64,
    pub total_profit_sol: f64,
    pub total_loss_sol: f64,
    pub net_profit_sol: f64,
    pub avg_profit_per_win: f64,
    pub avg_loss_per_loss: f64,
    pub profit_factor: f64,
    pub sharpe_ratio: f64,
    pub avg_hold_duration_seconds: f64,
    pub performance_score: f64,
    pub recent_trades: u32,
    pub exit_reason_breakdown: std::collections::HashMap<String, i32>,
    pub last_updated: i64,
}

/// Overall performance summary
#[derive(Debug, Clone)]
pub struct OverallPerformanceSummary {
    pub total_copy_trades: u32,
    pub overall_win_rate: f64,
    pub recent_win_rate: f64,
    pub total_pnl_sol: f64,
    pub recent_pnl_sol: f64,
    pub avg_pnl_per_trade: f64,
    pub recent_trades: u32,
    pub active_days: u32,
    pub top_performers: Vec<TopPerformerSummary>,
    pub last_updated: i64,
}

/// Top performer summary
#[derive(Debug, Clone)]
pub struct TopPerformerSummary {
    pub insider_wallet: String,
    pub trades: u32,
    pub win_rate: f64,
    pub total_profit_sol: f64,
}

/// Performance feedback for score adjustment
#[derive(Debug, Clone)]
pub struct PerformanceFeedback {
    pub insider_wallet: String,
    pub score_adjustment: f64,
    pub status_recommendation: Option<WalletStatus>,
    pub feedback_reason: String,
    pub confidence: f64,
}