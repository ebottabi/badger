use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tracing::{debug, info, warn, error, instrument};

use super::position_tracker::{Position, PositionTracker};
use super::super::{BadgerDatabase, DatabaseError};

/// P&L calculation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PnLResult {
    pub position_id: i64,
    pub token_mint: String,
    pub realized_pnl: Option<f64>,
    pub unrealized_pnl: Option<f64>,
    pub total_fees: f64,
    pub roi_percentage: f64,
    pub hold_duration: Option<i64>, // in seconds
    pub calculated_at: i64,
}

/// Portfolio P&L summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioPnL {
    pub total_realized_pnl: f64,
    pub total_unrealized_pnl: f64,
    pub total_fees: f64,
    pub net_pnl: f64,
    pub total_invested: f64,
    pub portfolio_roi: f64,
    pub win_rate: f64,
    pub profit_factor: f64, // gross_profit / gross_loss
    pub sharpe_ratio: Option<f64>,
    pub max_drawdown: f64,
    pub calculated_at: i64,
}

/// Token-specific P&L analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPnL {
    pub token_mint: String,
    pub total_positions: i64,
    pub closed_positions: i64,
    pub total_pnl: f64,
    pub best_trade: Option<f64>,
    pub worst_trade: Option<f64>,
    pub average_roi: f64,
    pub win_rate: f64,
    pub total_volume: f64,
    pub calculated_at: i64,
}

/// Real-time P&L calculation engine
pub struct PnLCalculator {
    db: Arc<BadgerDatabase>,
    position_tracker: Arc<PositionTracker>,
    current_prices: Arc<tokio::sync::RwLock<HashMap<String, f64>>>,
}

impl PnLCalculator {
    pub fn new(db: Arc<BadgerDatabase>, position_tracker: Arc<PositionTracker>) -> Self {
        Self {
            db,
            position_tracker,
            current_prices: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Initialize P&L calculation schema
    #[instrument(skip(self))]
    pub async fn initialize_schema(&self) -> Result<(), DatabaseError> {
        info!("🔧 Initializing P&L calculator database schema");

        let create_pnl_snapshots_table = r#"
            CREATE TABLE IF NOT EXISTS pnl_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                snapshot_type TEXT NOT NULL CHECK (snapshot_type IN ('DAILY', 'HOURLY', 'REALTIME')),
                total_realized_pnl REAL NOT NULL DEFAULT 0.0,
                total_unrealized_pnl REAL NOT NULL DEFAULT 0.0,
                total_fees REAL NOT NULL DEFAULT 0.0,
                net_pnl REAL NOT NULL DEFAULT 0.0,
                total_invested REAL NOT NULL DEFAULT 0.0,
                portfolio_roi REAL NOT NULL DEFAULT 0.0,
                win_rate REAL NOT NULL DEFAULT 0.0,
                profit_factor REAL NOT NULL DEFAULT 0.0,
                sharpe_ratio REAL,
                max_drawdown REAL NOT NULL DEFAULT 0.0,
                timestamp INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )
        "#;

        let create_token_pnl_table = r#"
            CREATE TABLE IF NOT EXISTS token_pnl_summary (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                token_mint TEXT NOT NULL,
                total_positions INTEGER NOT NULL DEFAULT 0,
                closed_positions INTEGER NOT NULL DEFAULT 0,
                total_pnl REAL NOT NULL DEFAULT 0.0,
                best_trade REAL,
                worst_trade REAL,
                average_roi REAL NOT NULL DEFAULT 0.0,
                win_rate REAL NOT NULL DEFAULT 0.0,
                total_volume REAL NOT NULL DEFAULT 0.0,
                last_updated INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                UNIQUE(token_mint)
            )
        "#;

        // Create indexes
        let create_indexes = vec![
            "CREATE INDEX IF NOT EXISTS idx_pnl_snapshots_timestamp ON pnl_snapshots(timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_pnl_snapshots_type ON pnl_snapshots(snapshot_type)",
            "CREATE INDEX IF NOT EXISTS idx_token_pnl_mint ON token_pnl_summary(token_mint)",
        ];

        // Execute schema creation
        sqlx::query(create_pnl_snapshots_table)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to create pnl_snapshots table: {}", e)))?;

        sqlx::query(create_token_pnl_table)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to create token_pnl_summary table: {}", e)))?;

        for index_sql in create_indexes {
            sqlx::query(index_sql)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to create index: {}", e)))?;
        }

        info!("✅ P&L calculator database schema initialized");
        Ok(())
    }

    /// Update current price for a token
    pub async fn update_price(&self, token_mint: &str, price: f64) {
        let mut prices = self.current_prices.write().await;
        prices.insert(token_mint.to_string(), price);
        debug!("💰 Updated price for {}: ${:.6}", token_mint, price);
    }

    /// Calculate P&L for a specific position
    #[instrument(skip(self))]
    pub async fn calculate_position_pnl(&self, position: &Position) -> Result<PnLResult, DatabaseError> {
        let now = Utc::now().timestamp();
        let mut pnl_result = PnLResult {
            position_id: position.id,
            token_mint: position.token_mint.clone(),
            realized_pnl: None,
            unrealized_pnl: None,
            total_fees: position.fees,
            roi_percentage: 0.0,
            hold_duration: None,
            calculated_at: now,
        };

        if position.status == "CLOSED" {
            // Realized P&L calculation
            if let Some(pnl) = position.pnl {
                pnl_result.realized_pnl = Some(pnl);
                let investment = position.entry_price * position.quantity;
                pnl_result.roi_percentage = if investment > 0.0 {
                    (pnl / investment) * 100.0
                } else {
                    0.0
                };

                if let Some(exit_timestamp) = position.exit_timestamp {
                    pnl_result.hold_duration = Some(exit_timestamp - position.entry_timestamp);
                }
            }
        } else {
            // Unrealized P&L calculation
            if let Some(current_price) = self.get_current_price(&position.token_mint).await {
                let unrealized_pnl = (current_price - position.entry_price) * position.quantity - position.fees;
                pnl_result.unrealized_pnl = Some(unrealized_pnl);
                
                let investment = position.entry_price * position.quantity;
                pnl_result.roi_percentage = if investment > 0.0 {
                    (unrealized_pnl / investment) * 100.0
                } else {
                    0.0
                };

                pnl_result.hold_duration = Some(now - position.entry_timestamp);
            }
        }

        Ok(pnl_result)
    }

    /// Calculate portfolio-wide P&L
    #[instrument(skip(self))]
    pub async fn calculate_portfolio_pnl(&self) -> Result<PortfolioPnL, DatabaseError> {
        let now = Utc::now().timestamp();

        // Get all positions for calculation
        let all_positions = sqlx::query_as::<_, Position>(
            "SELECT * FROM positions ORDER BY entry_timestamp"
        )
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch positions: {}", e)))?;

        let mut total_realized_pnl = 0.0;
        let mut total_unrealized_pnl = 0.0;
        let mut total_fees = 0.0;
        let mut total_invested = 0.0;
        let mut winning_trades = 0;
        let mut losing_trades = 0;
        let mut gross_profit = 0.0;
        let mut gross_loss = 0.0;
        let mut pnl_history = Vec::new();

        for position in &all_positions {
            let investment = position.entry_price * position.quantity;
            total_invested += investment;
            total_fees += position.fees;

            if position.status == "CLOSED" {
                if let Some(pnl) = position.pnl {
                    total_realized_pnl += pnl;
                    pnl_history.push(pnl);
                    
                    if pnl > 0.0 {
                        winning_trades += 1;
                        gross_profit += pnl;
                    } else {
                        losing_trades += 1;
                        gross_loss += pnl.abs();
                    }
                }
            } else {
                // Calculate unrealized P&L for open positions
                if let Some(current_price) = self.get_current_price(&position.token_mint).await {
                    let unrealized = (current_price - position.entry_price) * position.quantity - position.fees;
                    total_unrealized_pnl += unrealized;
                }
            }
        }

        let net_pnl = total_realized_pnl + total_unrealized_pnl - total_fees;
        let total_trades = winning_trades + losing_trades;
        let win_rate = if total_trades > 0 {
            winning_trades as f64 / total_trades as f64
        } else {
            0.0
        };

        let profit_factor = if gross_loss > 0.0 {
            gross_profit / gross_loss
        } else if gross_profit > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };

        let portfolio_roi = if total_invested > 0.0 {
            (net_pnl / total_invested) * 100.0
        } else {
            0.0
        };

        // Calculate Sharpe ratio (simplified - using all P&L as returns)
        let sharpe_ratio = if pnl_history.len() > 1 {
            let mean_return = pnl_history.iter().sum::<f64>() / pnl_history.len() as f64;
            let variance = pnl_history.iter()
                .map(|&x| (x - mean_return).powi(2))
                .sum::<f64>() / (pnl_history.len() - 1) as f64;
            let std_dev = variance.sqrt();
            
            if std_dev > 0.0 {
                Some(mean_return / std_dev)
            } else {
                None
            }
        } else {
            None
        };

        // Calculate maximum drawdown
        let max_drawdown = self.calculate_max_drawdown(&all_positions).await;

        Ok(PortfolioPnL {
            total_realized_pnl,
            total_unrealized_pnl,
            total_fees,
            net_pnl,
            total_invested,
            portfolio_roi,
            win_rate,
            profit_factor,
            sharpe_ratio,
            max_drawdown,
            calculated_at: now,
        })
    }

    /// Calculate token-specific P&L analytics
    #[instrument(skip(self))]
    pub async fn calculate_token_pnl(&self, token_mint: &str) -> Result<TokenPnL, DatabaseError> {
        let now = Utc::now().timestamp();

        let positions = sqlx::query_as::<_, Position>(
            "SELECT * FROM positions WHERE token_mint = ?"
        )
        .bind(token_mint)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch token positions: {}", e)))?;

        let total_positions = positions.len() as i64;
        let closed_positions = positions.iter().filter(|p| p.status == "CLOSED").count() as i64;
        
        let mut total_pnl = 0.0;
        let mut best_trade = None;
        let mut worst_trade = None;
        let mut total_volume = 0.0;
        let mut winning_trades = 0;
        let mut roi_values = Vec::new();

        for position in &positions {
            let volume = position.entry_price * position.quantity;
            total_volume += volume;

            if position.status == "CLOSED" {
                if let Some(pnl) = position.pnl {
                    total_pnl += pnl;
                    
                    if pnl > 0.0 {
                        winning_trades += 1;
                    }

                    // Update best/worst trades
                    best_trade = match best_trade {
                        None => Some(pnl),
                        Some(current_best) => Some(current_best.max(pnl)),
                    };

                    worst_trade = match worst_trade {
                        None => Some(pnl),
                        Some(current_worst) => Some(current_worst.min(pnl)),
                    };

                    // Calculate ROI for this position
                    if volume > 0.0 {
                        roi_values.push((pnl / volume) * 100.0);
                    }
                }
            }
        }

        let win_rate = if closed_positions > 0 {
            winning_trades as f64 / closed_positions as f64
        } else {
            0.0
        };

        let average_roi = if !roi_values.is_empty() {
            roi_values.iter().sum::<f64>() / roi_values.len() as f64
        } else {
            0.0
        };

        Ok(TokenPnL {
            token_mint: token_mint.to_string(),
            total_positions,
            closed_positions,
            total_pnl,
            best_trade,
            worst_trade,
            average_roi,
            win_rate,
            total_volume,
            calculated_at: now,
        })
    }

    /// Save portfolio P&L snapshot
    pub async fn save_pnl_snapshot(&self, portfolio_pnl: &PortfolioPnL, snapshot_type: &str) -> Result<(), DatabaseError> {
        sqlx::query(r#"
            INSERT INTO pnl_snapshots (
                snapshot_type, total_realized_pnl, total_unrealized_pnl, total_fees,
                net_pnl, total_invested, portfolio_roi, win_rate, profit_factor,
                sharpe_ratio, max_drawdown, timestamp
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#)
        .bind(snapshot_type)
        .bind(portfolio_pnl.total_realized_pnl)
        .bind(portfolio_pnl.total_unrealized_pnl)
        .bind(portfolio_pnl.total_fees)
        .bind(portfolio_pnl.net_pnl)
        .bind(portfolio_pnl.total_invested)
        .bind(portfolio_pnl.portfolio_roi)
        .bind(portfolio_pnl.win_rate)
        .bind(portfolio_pnl.profit_factor)
        .bind(portfolio_pnl.sharpe_ratio)
        .bind(portfolio_pnl.max_drawdown)
        .bind(portfolio_pnl.calculated_at)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to save P&L snapshot: {}", e)))?;

        debug!("💾 Saved {} P&L snapshot: Net P&L ${:.4}", snapshot_type, portfolio_pnl.net_pnl);
        Ok(())
    }

    /// Get current price from memory
    async fn get_current_price(&self, token_mint: &str) -> Option<f64> {
        let prices = self.current_prices.read().await;
        prices.get(token_mint).copied()
    }

    /// Calculate maximum drawdown from position history
    async fn calculate_max_drawdown(&self, positions: &[Position]) -> f64 {
        let mut running_balance = 0.0;
        let mut peak_balance = 0.0;
        let mut max_drawdown = 0.0;

        let mut sorted_positions = positions.to_vec();
        sorted_positions.sort_by_key(|p| p.entry_timestamp);

        for position in &sorted_positions {
            if position.status == "CLOSED" {
                if let Some(pnl) = position.pnl {
                    running_balance += pnl;
                    
                    if running_balance > peak_balance {
                        peak_balance = running_balance;
                    }

                    let drawdown = if peak_balance > 0.0 {
                        ((peak_balance - running_balance) / peak_balance) * 100.0
                    } else {
                        0.0
                    };

                    if drawdown > max_drawdown {
                        max_drawdown = drawdown;
                    }
                }
            }
        }

        max_drawdown
    }

    /// Get P&L history for a time period
    pub async fn get_pnl_history(&self, hours_back: i64) -> Result<Vec<PortfolioPnL>, DatabaseError> {
        let since_timestamp = Utc::now().timestamp() - (hours_back * 3600);

        let snapshots = sqlx::query(r#"
            SELECT * FROM pnl_snapshots 
            WHERE timestamp >= ? 
            ORDER BY timestamp ASC
        "#)
        .bind(since_timestamp)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch P&L history: {}", e)))?;

        let mut history = Vec::new();
        for row in snapshots {
            history.push(PortfolioPnL {
                total_realized_pnl: row.get("total_realized_pnl"),
                total_unrealized_pnl: row.get("total_unrealized_pnl"),
                total_fees: row.get("total_fees"),
                net_pnl: row.get("net_pnl"),
                total_invested: row.get("total_invested"),
                portfolio_roi: row.get("portfolio_roi"),
                win_rate: row.get("win_rate"),
                profit_factor: row.get("profit_factor"),
                sharpe_ratio: row.get("sharpe_ratio"),
                max_drawdown: row.get("max_drawdown"),
                calculated_at: row.get("timestamp"),
            });
        }

        Ok(history)
    }
}