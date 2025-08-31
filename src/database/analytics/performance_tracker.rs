use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc, Duration};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tracing::{debug, info, warn, error, instrument};

use super::position_tracker::{Position, PositionTracker};
use super::pnl_calculator::{PnLCalculator, PortfolioPnL};
use super::super::{BadgerDatabase, DatabaseError};

/// Performance metrics for bot trading analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub period_start: i64,
    pub period_end: i64,
    pub total_trades: i64,
    pub winning_trades: i64,
    pub losing_trades: i64,
    pub win_rate: f64,
    pub average_win: f64,
    pub average_loss: f64,
    pub profit_factor: f64,
    pub sharpe_ratio: Option<f64>,
    pub sortino_ratio: Option<f64>,
    pub max_drawdown: f64,
    pub max_drawdown_duration: i64, // in seconds
    pub total_return: f64,
    pub annualized_return: f64,
    pub volatility: f64,
    pub calmar_ratio: Option<f64>,
    pub trades_per_day: f64,
    pub average_hold_time: f64, // in hours
    pub best_trade: Option<f64>,
    pub worst_trade: Option<f64>,
    pub consecutive_wins: i64,
    pub consecutive_losses: i64,
    pub calculated_at: i64,
}

/// Signal performance tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalPerformance {
    pub signal_type: String,
    pub total_signals: i64,
    pub successful_signals: i64,
    pub success_rate: f64,
    pub average_pnl: f64,
    pub total_pnl: f64,
    pub average_confidence: f64,
    pub confidence_accuracy: f64, // correlation between confidence and success
    pub response_time: f64, // average time from signal to position
    pub calculated_at: i64,
}

/// Trading session performance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingSession {
    pub session_id: String,
    pub start_time: i64,
    pub end_time: Option<i64>,
    pub trades_count: i64,
    pub session_pnl: f64,
    pub best_trade: Option<f64>,
    pub worst_trade: Option<f64>,
    pub win_rate: f64,
    pub duration_hours: f64,
    pub trades_per_hour: f64,
    pub status: String, // "ACTIVE", "COMPLETED", "PAUSED"
}

/// Performance tracker for comprehensive bot analytics
pub struct PerformanceTracker {
    db: Arc<BadgerDatabase>,
    position_tracker: Arc<PositionTracker>,
    pnl_calculator: Arc<PnLCalculator>,
    current_session_id: Arc<tokio::sync::RwLock<Option<String>>>,
}

impl PerformanceTracker {
    pub fn new(
        db: Arc<BadgerDatabase>, 
        position_tracker: Arc<PositionTracker>,
        pnl_calculator: Arc<PnLCalculator>
    ) -> Self {
        Self {
            db,
            position_tracker,
            pnl_calculator,
            current_session_id: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Initialize performance tracking schema
    #[instrument(skip(self))]
    pub async fn initialize_schema(&self) -> Result<(), DatabaseError> {
        info!("🔧 Performance tracker schema initialization (skipped - handled by migration system)");
        
        // Schema creation is handled by the migration system
        info!("✅ Performance tracker schema ready");
        return Ok(());

        // OLD CODE (disabled):
        let _create_performance_snapshots = r#"
            CREATE TABLE IF NOT EXISTS performance_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                period_type TEXT NOT NULL CHECK (period_type IN ('HOURLY', 'DAILY', 'WEEKLY', 'MONTHLY')),
                period_start INTEGER NOT NULL,
                period_end INTEGER NOT NULL,
                total_trades INTEGER NOT NULL DEFAULT 0,
                winning_trades INTEGER NOT NULL DEFAULT 0,
                losing_trades INTEGER NOT NULL DEFAULT 0,
                win_rate REAL NOT NULL DEFAULT 0.0,
                average_win REAL NOT NULL DEFAULT 0.0,
                average_loss REAL NOT NULL DEFAULT 0.0,
                profit_factor REAL NOT NULL DEFAULT 0.0,
                sharpe_ratio REAL,
                sortino_ratio REAL,
                max_drawdown REAL NOT NULL DEFAULT 0.0,
                max_drawdown_duration INTEGER NOT NULL DEFAULT 0,
                total_return REAL NOT NULL DEFAULT 0.0,
                annualized_return REAL NOT NULL DEFAULT 0.0,
                volatility REAL NOT NULL DEFAULT 0.0,
                calmar_ratio REAL,
                trades_per_day REAL NOT NULL DEFAULT 0.0,
                average_hold_time REAL NOT NULL DEFAULT 0.0,
                best_trade REAL,
                worst_trade REAL,
                consecutive_wins INTEGER NOT NULL DEFAULT 0,
                consecutive_losses INTEGER NOT NULL DEFAULT 0,
                calculated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )
        "#;

        let create_signal_performance = r#"
            CREATE TABLE IF NOT EXISTS signal_performance (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                signal_type TEXT NOT NULL,
                period_start INTEGER NOT NULL,
                period_end INTEGER NOT NULL,
                total_signals INTEGER NOT NULL DEFAULT 0,
                successful_signals INTEGER NOT NULL DEFAULT 0,
                success_rate REAL NOT NULL DEFAULT 0.0,
                average_pnl REAL NOT NULL DEFAULT 0.0,
                total_pnl REAL NOT NULL DEFAULT 0.0,
                average_confidence REAL NOT NULL DEFAULT 0.0,
                confidence_accuracy REAL NOT NULL DEFAULT 0.0,
                response_time REAL NOT NULL DEFAULT 0.0,
                calculated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                UNIQUE(signal_type, period_start)
            )
        "#;

        let create_trading_sessions = r#"
            CREATE TABLE IF NOT EXISTS trading_sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL UNIQUE,
                start_time INTEGER NOT NULL,
                end_time INTEGER,
                trades_count INTEGER NOT NULL DEFAULT 0,
                session_pnl REAL NOT NULL DEFAULT 0.0,
                best_trade REAL,
                worst_trade REAL,
                win_rate REAL NOT NULL DEFAULT 0.0,
                duration_hours REAL NOT NULL DEFAULT 0.0,
                trades_per_hour REAL NOT NULL DEFAULT 0.0,
                status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'COMPLETED', 'PAUSED')),
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )
        "#;

        // Create indexes
        let create_indexes = vec![
            "CREATE INDEX IF NOT EXISTS idx_perf_snapshots_period ON performance_snapshots(period_type, period_start)",
            "CREATE INDEX IF NOT EXISTS idx_signal_perf_type ON signal_performance(signal_type)",
            "CREATE INDEX IF NOT EXISTS idx_trading_sessions_status ON trading_sessions(status)",
            "CREATE INDEX IF NOT EXISTS idx_trading_sessions_start ON trading_sessions(start_time)",
        ];

        // OLD CODE (unreachable due to early return):
        /*
        // Execute schema creation
        for table_sql in [create_performance_snapshots, create_signal_performance, create_trading_sessions] {
            sqlx::query(table_sql)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to create performance table: {}", e)))?;
        }

        for index_sql in create_indexes {
            sqlx::query(index_sql)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to create index: {}", e)))?;
        }

        info!("✅ Performance tracker database schema initialized");
        Ok(())
        */
    }

    /// Calculate comprehensive performance metrics for a period
    #[instrument(skip(self))]
    pub async fn calculate_performance(&self, period_start: i64, period_end: i64) -> Result<PerformanceMetrics, DatabaseError> {
        debug!("📊 Calculating performance metrics for period {} to {}", period_start, period_end);

        // Get all positions in the period
        let positions = sqlx::query_as::<_, Position>(r#"
            SELECT * FROM positions 
            WHERE entry_timestamp >= ? AND entry_timestamp <= ? 
            AND status = 'CLOSED'
            ORDER BY entry_timestamp
        "#)
        .bind(period_start)
        .bind(period_end)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch period positions: {}", e)))?;

        if positions.is_empty() {
            return Ok(PerformanceMetrics {
                period_start,
                period_end,
                total_trades: 0,
                winning_trades: 0,
                losing_trades: 0,
                win_rate: 0.0,
                average_win: 0.0,
                average_loss: 0.0,
                profit_factor: 0.0,
                sharpe_ratio: None,
                sortino_ratio: None,
                max_drawdown: 0.0,
                max_drawdown_duration: 0,
                total_return: 0.0,
                annualized_return: 0.0,
                volatility: 0.0,
                calmar_ratio: None,
                trades_per_day: 0.0,
                average_hold_time: 0.0,
                best_trade: None,
                worst_trade: None,
                consecutive_wins: 0,
                consecutive_losses: 0,
                calculated_at: Utc::now().timestamp(),
            });
        }

        let total_trades = positions.len() as i64;
        let mut wins = Vec::new();
        let mut losses = Vec::new();
        let mut all_returns = Vec::new();
        let mut hold_times = Vec::new();
        let mut best_trade = None;
        let mut worst_trade = None;
        let mut total_pnl = 0.0;

        // Analyze each position
        for position in &positions {
            if let Some(pnl) = position.pnl {
                total_pnl += pnl;
                all_returns.push(pnl);

                if pnl > 0.0 {
                    wins.push(pnl);
                } else {
                    losses.push(pnl);
                }

                // Track best/worst trades
                best_trade = match best_trade {
                    None => Some(pnl),
                    Some(current) => Some(current.max(pnl)),
                };

                worst_trade = match worst_trade {
                    None => Some(pnl),
                    Some(current) => Some(current.min(pnl)),
                };

                // Calculate hold time
                if let Some(exit_time) = position.exit_timestamp {
                    let hold_time_hours = (exit_time - position.entry_timestamp) as f64 / 3600.0;
                    hold_times.push(hold_time_hours);
                }
            }
        }

        let winning_trades = wins.len() as i64;
        let losing_trades = losses.len() as i64;
        let win_rate = winning_trades as f64 / total_trades as f64;

        let average_win = if !wins.is_empty() {
            wins.iter().sum::<f64>() / wins.len() as f64
        } else {
            0.0
        };

        let average_loss = if !losses.is_empty() {
            losses.iter().sum::<f64>() / losses.len() as f64
        } else {
            0.0
        };

        let profit_factor = if average_loss.abs() > 0.0 {
            (average_win * winning_trades as f64) / (average_loss.abs() * losing_trades as f64)
        } else if average_win > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };

        // Calculate Sharpe ratio
        let sharpe_ratio = if all_returns.len() > 1 {
            let mean_return = all_returns.iter().sum::<f64>() / all_returns.len() as f64;
            let variance = all_returns.iter()
                .map(|&x| (x - mean_return).powi(2))
                .sum::<f64>() / (all_returns.len() - 1) as f64;
            let std_dev = variance.sqrt();
            
            if std_dev > 0.0 {
                Some(mean_return / std_dev)
            } else {
                None
            }
        } else {
            None
        };

        // Calculate Sortino ratio (using downside deviation)
        let sortino_ratio = if all_returns.len() > 1 {
            let mean_return = all_returns.iter().sum::<f64>() / all_returns.len() as f64;
            let downside_deviation = {
                let negative_returns: Vec<f64> = all_returns.iter()
                    .filter(|&&x| x < 0.0)
                    .map(|&x| x.powi(2))
                    .collect();
                
                if !negative_returns.is_empty() {
                    (negative_returns.iter().sum::<f64>() / negative_returns.len() as f64).sqrt()
                } else {
                    0.0
                }
            };
            
            if downside_deviation > 0.0 {
                Some(mean_return / downside_deviation)
            } else {
                None
            }
        } else {
            None
        };

        // Calculate drawdown metrics
        let (max_drawdown, max_drawdown_duration) = self.calculate_drawdown_metrics(&positions).await;

        // Calculate returns and volatility
        let period_days = (period_end - period_start) as f64 / 86400.0;
        let total_return = total_pnl;
        let annualized_return = if period_days > 0.0 {
            (total_return / period_days) * 365.0
        } else {
            0.0
        };

        let volatility = if all_returns.len() > 1 {
            let mean = all_returns.iter().sum::<f64>() / all_returns.len() as f64;
            let variance = all_returns.iter()
                .map(|&x| (x - mean).powi(2))
                .sum::<f64>() / (all_returns.len() - 1) as f64;
            variance.sqrt()
        } else {
            0.0
        };

        // Calculate Calmar ratio
        let calmar_ratio = if max_drawdown > 0.0 {
            Some(annualized_return / max_drawdown)
        } else {
            None
        };

        let trades_per_day = if period_days > 0.0 {
            total_trades as f64 / period_days
        } else {
            0.0
        };

        let average_hold_time = if !hold_times.is_empty() {
            hold_times.iter().sum::<f64>() / hold_times.len() as f64
        } else {
            0.0
        };

        // Calculate consecutive wins/losses
        let (consecutive_wins, consecutive_losses) = self.calculate_consecutive_streaks(&positions);

        Ok(PerformanceMetrics {
            period_start,
            period_end,
            total_trades,
            winning_trades,
            losing_trades,
            win_rate,
            average_win,
            average_loss,
            profit_factor,
            sharpe_ratio,
            sortino_ratio,
            max_drawdown,
            max_drawdown_duration,
            total_return,
            annualized_return,
            volatility,
            calmar_ratio,
            trades_per_day,
            average_hold_time,
            best_trade,
            worst_trade,
            consecutive_wins,
            consecutive_losses,
            calculated_at: Utc::now().timestamp(),
        })
    }

    /// Calculate signal performance metrics
    #[instrument(skip(self))]
    pub async fn calculate_signal_performance(&self, signal_type: &str, period_start: i64, period_end: i64) -> Result<SignalPerformance, DatabaseError> {
        // Get positions created from this signal type
        let positions = sqlx::query_as::<_, Position>(r#"
            SELECT p.* FROM positions p
            JOIN trading_signals ts ON p.signal_id = ts.signal_id
            WHERE ts.signal_type = ? 
            AND p.entry_timestamp >= ? 
            AND p.entry_timestamp <= ?
            AND p.status = 'CLOSED'
        "#)
        .bind(signal_type)
        .bind(period_start)
        .bind(period_end)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch signal positions: {}", e)))?;

        let total_signals = positions.len() as i64;
        let successful_signals = positions.iter()
            .filter(|p| p.pnl.unwrap_or(0.0) > 0.0)
            .count() as i64;

        let success_rate = if total_signals > 0 {
            successful_signals as f64 / total_signals as f64
        } else {
            0.0
        };

        let total_pnl = positions.iter()
            .map(|p| p.pnl.unwrap_or(0.0))
            .sum::<f64>();

        let average_pnl = if total_signals > 0 {
            total_pnl / total_signals as f64
        } else {
            0.0
        };

        // Get average confidence for signals
        let average_confidence = sqlx::query_scalar::<_, f64>(r#"
            SELECT COALESCE(CAST(AVG(confidence) AS REAL), 0.0) FROM trading_signals 
            WHERE signal_type = ? AND timestamp >= ? AND timestamp <= ?
        "#)
        .bind(signal_type)
        .bind(period_start)
        .bind(period_end)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to calculate average confidence: {}", e)))?;

        // Calculate confidence accuracy (correlation between confidence and success)
        let confidence_accuracy = self.calculate_confidence_accuracy(signal_type, period_start, period_end).await?;

        // Calculate average response time from signal to position
        let response_time = self.calculate_response_time(signal_type, period_start, period_end).await?;

        Ok(SignalPerformance {
            signal_type: signal_type.to_string(),
            total_signals,
            successful_signals,
            success_rate,
            average_pnl,
            total_pnl,
            average_confidence,
            confidence_accuracy,
            response_time,
            calculated_at: Utc::now().timestamp(),
        })
    }

    /// Start a new trading session
    pub async fn start_trading_session(&self) -> Result<String, DatabaseError> {
        let session_id = format!("session_{}", Utc::now().timestamp());
        let start_time = Utc::now().timestamp();

        sqlx::query(r#"
            INSERT INTO trading_sessions (session_id, start_time, status)
            VALUES (?, ?, 'ACTIVE')
        "#)
        .bind(&session_id)
        .bind(start_time)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to start trading session: {}", e)))?;

        {
            let mut current_session = self.current_session_id.write().await;
            *current_session = Some(session_id.clone());
        }

        info!("🚀 Started trading session: {}", session_id);
        Ok(session_id)
    }

    /// End current trading session
    pub async fn end_trading_session(&self) -> Result<(), DatabaseError> {
        let session_id = {
            let current_session = self.current_session_id.read().await;
            current_session.clone()
        };

        if let Some(session_id) = session_id {
            let end_time = Utc::now().timestamp();
            
            // Update session with final stats
            let session_stats = self.calculate_session_stats(&session_id).await?;
            
            sqlx::query(r#"
                UPDATE trading_sessions 
                SET end_time = ?, executed_trades = ?, total_pnl = ?, status = 'COMPLETED'
                WHERE session_id = ?
            "#)
            .bind(end_time)
            .bind(session_stats.trades_count as i32)  // Use executed_trades column
            .bind(session_stats.session_pnl)          // Use total_pnl column  
            .bind(&session_id)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to end trading session: {}", e)))?;

            {
                let mut current_session = self.current_session_id.write().await;
                *current_session = None;
            }

            info!("🏁 Ended trading session: {} (P&L: ${:.4})", session_id, session_stats.session_pnl);
        }

        Ok(())
    }

    /// Save performance snapshot
    pub async fn save_performance_snapshot(&self, metrics: &PerformanceMetrics, period_type: &str) -> Result<(), DatabaseError> {
        sqlx::query(r#"
            INSERT INTO performance_snapshots (
                period_type, period_start, period_end, total_trades, winning_trades, losing_trades,
                win_rate, average_win, average_loss, profit_factor, sharpe_ratio, sortino_ratio,
                max_drawdown, max_drawdown_duration, total_return, annualized_return, volatility,
                calmar_ratio, trades_per_day, average_hold_time, best_trade, worst_trade,
                consecutive_wins, consecutive_losses, calculated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#)
        .bind(period_type)
        .bind(metrics.period_start)
        .bind(metrics.period_end)
        .bind(metrics.total_trades)
        .bind(metrics.winning_trades)
        .bind(metrics.losing_trades)
        .bind(metrics.win_rate)
        .bind(metrics.average_win)
        .bind(metrics.average_loss)
        .bind(metrics.profit_factor)
        .bind(metrics.sharpe_ratio)
        .bind(metrics.sortino_ratio)
        .bind(metrics.max_drawdown)
        .bind(metrics.max_drawdown_duration)
        .bind(metrics.total_return)
        .bind(metrics.annualized_return)
        .bind(metrics.volatility)
        .bind(metrics.calmar_ratio)
        .bind(metrics.trades_per_day)
        .bind(metrics.average_hold_time)
        .bind(metrics.best_trade)
        .bind(metrics.worst_trade)
        .bind(metrics.consecutive_wins)
        .bind(metrics.consecutive_losses)
        .bind(metrics.calculated_at)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to save performance snapshot: {}", e)))?;

        debug!("💾 Saved {} performance snapshot", period_type);
        Ok(())
    }

    /// Get latest performance metrics
    pub async fn get_latest_performance(&self, period_type: &str) -> Result<Option<PerformanceMetrics>, DatabaseError> {
        let row = sqlx::query(r#"
            SELECT * FROM performance_snapshots 
            WHERE period_type = ? 
            ORDER BY period_end DESC 
            LIMIT 1
        "#)
        .bind(period_type)
        .fetch_optional(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch latest performance: {}", e)))?;

        if let Some(row) = row {
            Ok(Some(PerformanceMetrics {
                period_start: row.get("period_start"),
                period_end: row.get("period_end"),
                total_trades: row.get("total_trades"),
                winning_trades: row.get("winning_trades"),
                losing_trades: row.get("losing_trades"),
                win_rate: row.get("win_rate"),
                average_win: row.get("average_win"),
                average_loss: row.get("average_loss"),
                profit_factor: row.get("profit_factor"),
                sharpe_ratio: row.get("sharpe_ratio"),
                sortino_ratio: row.get("sortino_ratio"),
                max_drawdown: row.get("max_drawdown"),
                max_drawdown_duration: row.get("max_drawdown_duration"),
                total_return: row.get("total_return"),
                annualized_return: row.get("annualized_return"),
                volatility: row.get("volatility"),
                calmar_ratio: row.get("calmar_ratio"),
                trades_per_day: row.get("trades_per_day"),
                average_hold_time: row.get("average_hold_time"),
                best_trade: row.get("best_trade"),
                worst_trade: row.get("worst_trade"),
                consecutive_wins: row.get("consecutive_wins"),
                consecutive_losses: row.get("consecutive_losses"),
                calculated_at: row.get("calculated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    // Helper methods for calculations

    async fn calculate_drawdown_metrics(&self, positions: &[Position]) -> (f64, i64) {
        let mut running_pnl = 0.0;
        let mut peak_pnl = 0.0;
        let mut max_drawdown = 0.0;
        let mut max_drawdown_duration = 0i64;
        let mut drawdown_start: Option<i64> = None;

        for position in positions {
            if let Some(pnl) = position.pnl {
                running_pnl += pnl;
                
                if running_pnl > peak_pnl {
                    peak_pnl = running_pnl;
                    drawdown_start = None; // Reset drawdown tracking
                } else {
                    if drawdown_start.is_none() {
                        drawdown_start = position.exit_timestamp;
                    }
                }

                let current_drawdown = if peak_pnl > 0.0 {
                    ((peak_pnl - running_pnl) / peak_pnl) * 100.0
                } else {
                    0.0
                };

                if current_drawdown > max_drawdown {
                    max_drawdown = current_drawdown;
                    
                    if let (Some(start), Some(end)) = (drawdown_start, position.exit_timestamp) {
                        max_drawdown_duration = end - start;
                    }
                }
            }
        }

        (max_drawdown, max_drawdown_duration)
    }

    fn calculate_consecutive_streaks(&self, positions: &[Position]) -> (i64, i64) {
        let mut max_wins = 0i64;
        let mut max_losses = 0i64;
        let mut current_wins = 0i64;
        let mut current_losses = 0i64;

        for position in positions {
            if let Some(pnl) = position.pnl {
                if pnl > 0.0 {
                    current_wins += 1;
                    current_losses = 0;
                    max_wins = max_wins.max(current_wins);
                } else {
                    current_losses += 1;
                    current_wins = 0;
                    max_losses = max_losses.max(current_losses);
                }
            }
        }

        (max_wins, max_losses)
    }

    async fn calculate_confidence_accuracy(&self, signal_type: &str, period_start: i64, period_end: i64) -> Result<f64, DatabaseError> {
        // This would calculate the correlation between signal confidence and actual success
        // For now, returning a placeholder calculation
        let success_rate = sqlx::query_scalar::<_, f64>(r#"
            SELECT COALESCE(
                COUNT(CASE WHEN p.pnl > 0 THEN 1 END) * 1.0 / COUNT(*),
                0.0
            ) FROM positions p
            JOIN trading_signals ts ON p.signal_id = ts.signal_id
            WHERE ts.signal_type = ? AND p.entry_timestamp >= ? AND p.entry_timestamp <= ?
            AND p.status = 'CLOSED'
        "#)
        .bind(signal_type)
        .bind(period_start)
        .bind(period_end)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to calculate confidence accuracy: {}", e)))?;

        Ok(success_rate)
    }

    async fn calculate_response_time(&self, signal_type: &str, period_start: i64, period_end: i64) -> Result<f64, DatabaseError> {
        // Calculate average time from signal to position opening
        let avg_response = sqlx::query_scalar::<_, f64>(r#"
            SELECT COALESCE(CAST(AVG(p.entry_timestamp - ts.timestamp) AS REAL), 0.0)
            FROM positions p
            JOIN trading_signals ts ON p.signal_id = ts.signal_id
            WHERE ts.signal_type = ? AND p.entry_timestamp >= ? AND p.entry_timestamp <= ?
        "#)
        .bind(signal_type)
        .bind(period_start)
        .bind(period_end)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to calculate response time: {}", e)))?;

        Ok(avg_response)
    }

    async fn calculate_session_stats(&self, session_id: &str) -> Result<TradingSession, DatabaseError> {
        // Simplified stats calculation using migration schema columns
        let session_row = sqlx::query(r#"
            SELECT 
                session_id, start_time, end_time, executed_trades, total_pnl
            FROM trading_sessions 
            WHERE session_id = ?
        "#)
        .bind(session_id)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to calculate session stats: {}", e)))?;

        let start_time: i64 = session_row.get("start_time");
        let end_time: Option<i64> = session_row.get("end_time");
        let current_time = Utc::now().timestamp();
        let duration_seconds = end_time.unwrap_or(current_time) - start_time;
        let duration_hours = duration_seconds as f64 / 3600.0;
        let trades_count: i64 = session_row.try_get("executed_trades").unwrap_or(0);
        let session_pnl: f64 = session_row.try_get("total_pnl").unwrap_or(0.0);
        
        let trades_per_hour = if duration_hours > 0.0 {
            trades_count as f64 / duration_hours
        } else {
            0.0
        };

        Ok(TradingSession {
            session_id: session_row.get("session_id"),
            start_time,
            end_time,
            trades_count,
            session_pnl,
            best_trade: None,  // Not available in simplified schema
            worst_trade: None, // Not available in simplified schema
            win_rate: 0.0,     // Not available in simplified schema
            duration_hours,
            trades_per_hour,
            status: "ACTIVE".to_string(),
        })
    }
}