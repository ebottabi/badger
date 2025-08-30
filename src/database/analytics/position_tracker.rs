use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, FromRow};
use tracing::{debug, info, warn, error, instrument};

use crate::core::{MarketEvent, TradingSignal};
use super::super::{BadgerDatabase, DatabaseError};

/// Position entry representing a trade position
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Position {
    pub id: i64,
    pub token_mint: String,
    pub entry_price: f64,
    pub exit_price: Option<f64>,
    pub quantity: f64,
    pub entry_timestamp: i64,
    pub exit_timestamp: Option<i64>,
    pub position_type: String, // "BUY" or "SELL"
    pub status: String, // "OPEN", "CLOSED", "PARTIAL"
    pub pnl: Option<f64>,
    pub fees: f64,
    pub signal_id: Option<String>,
    pub insider_wallet: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Position summary for analytics
#[derive(Debug, Clone)]
pub struct PositionSummary {
    pub total_positions: i64,
    pub open_positions: i64,
    pub closed_positions: i64,
    pub total_pnl: f64,
    pub total_fees: f64,
    pub average_hold_time: f64, // in seconds
    pub win_rate: f64,
    pub best_trade: Option<f64>,
    pub worst_trade: Option<f64>,
}

/// Real-time position tracker for trading analytics
pub struct PositionTracker {
    db: Arc<BadgerDatabase>,
    open_positions: Arc<tokio::sync::RwLock<HashMap<String, Position>>>,
}

impl PositionTracker {
    pub fn new(db: Arc<BadgerDatabase>) -> Self {
        Self {
            db,
            open_positions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Initialize database schema for positions
    #[instrument(skip(self))]
    pub async fn initialize_schema(&self) -> Result<(), DatabaseError> {
        info!("🔧 Initializing position tracker database schema");

        let create_positions_table = r#"
            CREATE TABLE IF NOT EXISTS positions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                token_mint TEXT NOT NULL,
                entry_price REAL NOT NULL,
                exit_price REAL,
                quantity REAL NOT NULL,
                entry_timestamp INTEGER NOT NULL,
                exit_timestamp INTEGER,
                position_type TEXT NOT NULL CHECK (position_type IN ('BUY', 'SELL')),
                status TEXT NOT NULL DEFAULT 'OPEN' CHECK (status IN ('OPEN', 'CLOSED', 'PARTIAL')),
                pnl REAL,
                fees REAL DEFAULT 0.0,
                signal_id TEXT,
                insider_wallet TEXT,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )
        "#;

        let create_position_updates_table = r#"
            CREATE TABLE IF NOT EXISTS position_updates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                position_id INTEGER NOT NULL,
                update_type TEXT NOT NULL,
                old_value TEXT,
                new_value TEXT,
                timestamp INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                FOREIGN KEY (position_id) REFERENCES positions (id)
            )
        "#;

        // Create indexes for better query performance
        let create_indexes = vec![
            "CREATE INDEX IF NOT EXISTS idx_positions_token_mint ON positions(token_mint)",
            "CREATE INDEX IF NOT EXISTS idx_positions_status ON positions(status)",
            "CREATE INDEX IF NOT EXISTS idx_positions_entry_timestamp ON positions(entry_timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_positions_insider_wallet ON positions(insider_wallet)",
            "CREATE INDEX IF NOT EXISTS idx_positions_signal_id ON positions(signal_id)",
        ];

        // Execute schema creation
        sqlx::query(create_positions_table)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to create positions table: {}", e)))?;

        sqlx::query(create_position_updates_table)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to create position_updates table: {}", e)))?;

        for index_sql in create_indexes {
            sqlx::query(index_sql)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to create index: {}", e)))?;
        }

        info!("✅ Position tracker database schema initialized");
        Ok(())
    }

    /// Open a new position based on trading signal
    #[instrument(skip(self, signal))]
    pub async fn open_position(
        &self,
        signal: &TradingSignal,
        entry_price: f64,
        quantity: f64,
        fees: f64,
        insider_wallet: Option<String>,
    ) -> Result<Position, DatabaseError> {
        let now = Utc::now().timestamp();

        let position = Position {
            id: 0, // Will be set by database
            token_mint: signal.get_token_mint(),
            entry_price,
            exit_price: None,
            quantity,
            entry_timestamp: now,
            exit_timestamp: None,
            position_type: "BUY".to_string(),
            status: "OPEN".to_string(),
            pnl: None,
            fees,
            signal_id: Some(signal.get_signal_id()),
            insider_wallet,
            created_at: now,
            updated_at: now,
        };

        // Insert position into database
        let position_id = sqlx::query(r#"
            INSERT INTO positions (
                token_mint, entry_price, quantity, entry_timestamp, 
                position_type, status, fees, signal_id, insider_wallet,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#)
        .bind(&position.token_mint)
        .bind(position.entry_price)
        .bind(position.quantity)
        .bind(position.entry_timestamp)
        .bind(&position.position_type)
        .bind(&position.status)
        .bind(position.fees)
        .bind(&position.signal_id)
        .bind(&position.insider_wallet)
        .bind(position.created_at)
        .bind(position.updated_at)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to insert position: {}", e)))?
        .last_insert_rowid();

        let mut opened_position = position;
        opened_position.id = position_id;

        // Store in memory for quick access
        {
            let mut open_positions = self.open_positions.write().await;
            open_positions.insert(opened_position.token_mint.clone(), opened_position.clone());
        }

        info!(
            "🔓 Opened position #{} for {} @ ${:.6} (qty: {}, fees: ${:.4})",
            position_id, opened_position.token_mint, entry_price, quantity, fees
        );

        Ok(opened_position)
    }

    /// Close a position and calculate P&L
    #[instrument(skip(self))]
    pub async fn close_position(
        &self,
        token_mint: &str,
        exit_price: f64,
        exit_fees: f64,
    ) -> Result<Option<Position>, DatabaseError> {
        let now = Utc::now().timestamp();

        // Find open position
        let position_id = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM positions WHERE token_mint = ? AND status = 'OPEN' ORDER BY entry_timestamp DESC LIMIT 1"
        )
        .bind(token_mint)
        .fetch_optional(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to find position: {}", e)))?;

        let position_id = match position_id {
            Some(id) => id,
            None => {
                warn!("No open position found for token: {}", token_mint);
                return Ok(None);
            }
        };

        // Get position details for P&L calculation
        let position = sqlx::query_as::<_, Position>(
            "SELECT * FROM positions WHERE id = ?"
        )
        .bind(position_id)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch position: {}", e)))?;

        // Calculate P&L: (exit_price - entry_price) * quantity - total_fees
        let total_fees = position.fees + exit_fees;
        let gross_pnl = (exit_price - position.entry_price) * position.quantity;
        let net_pnl = gross_pnl - total_fees;
        
        // Calculate ROI for logging before position is moved
        let roi_percentage = (net_pnl / (position.entry_price * position.quantity)) * 100.0;

        // Update position as closed
        sqlx::query(r#"
            UPDATE positions 
            SET exit_price = ?, exit_timestamp = ?, status = 'CLOSED', 
                pnl = ?, fees = ?, updated_at = ?
            WHERE id = ?
        "#)
        .bind(exit_price)
        .bind(now)
        .bind(net_pnl)
        .bind(total_fees)
        .bind(now)
        .bind(position_id)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to update position: {}", e)))?;

        // Log position update
        sqlx::query(r#"
            INSERT INTO position_updates (position_id, update_type, old_value, new_value)
            VALUES (?, 'CLOSE', 'OPEN', 'CLOSED')
        "#)
        .bind(position_id)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to log position update: {}", e)))?;

        // Remove from memory
        {
            let mut open_positions = self.open_positions.write().await;
            open_positions.remove(token_mint);
        }

        let mut closed_position = position;
        closed_position.exit_price = Some(exit_price);
        closed_position.exit_timestamp = Some(now);
        closed_position.status = "CLOSED".to_string();
        closed_position.pnl = Some(net_pnl);
        closed_position.fees = total_fees;
        closed_position.updated_at = now;
        info!(
            "🔒 Closed position #{} for {} @ ${:.6} | P&L: ${:.4} ({:.2}%)",
            position_id,
            token_mint,
            exit_price,
            net_pnl,
            roi_percentage
        );

        Ok(Some(closed_position))
    }

    /// Get all open positions
    pub async fn get_open_positions(&self) -> Result<Vec<Position>, DatabaseError> {
        let positions = sqlx::query_as::<_, Position>(
            "SELECT * FROM positions WHERE status = 'OPEN' ORDER BY entry_timestamp DESC"
        )
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch open positions: {}", e)))?;

        Ok(positions)
    }

    /// Get position summary and analytics
    pub async fn get_position_summary(&self) -> Result<PositionSummary, DatabaseError> {
        let summary_row = sqlx::query(r#"
            SELECT 
                COUNT(*) as total_positions,
                SUM(CASE WHEN status = 'OPEN' THEN 1 ELSE 0 END) as open_positions,
                SUM(CASE WHEN status = 'CLOSED' THEN 1 ELSE 0 END) as closed_positions,
                COALESCE(SUM(CASE WHEN status = 'CLOSED' THEN pnl ELSE 0 END), 0) as total_pnl,
                COALESCE(SUM(fees), 0) as total_fees,
                COALESCE(AVG(CASE WHEN status = 'CLOSED' AND exit_timestamp IS NOT NULL 
                    THEN exit_timestamp - entry_timestamp ELSE NULL END), 0) as avg_hold_time,
                COALESCE(MAX(CASE WHEN status = 'CLOSED' THEN pnl ELSE NULL END), 0) as best_trade,
                COALESCE(MIN(CASE WHEN status = 'CLOSED' THEN pnl ELSE NULL END), 0) as worst_trade
            FROM positions
        "#)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch position summary: {}", e)))?;

        let total_positions: i64 = summary_row.get("total_positions");
        let closed_positions: i64 = summary_row.get("closed_positions");

        // Calculate win rate
        let winning_trades = if closed_positions > 0 {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM positions WHERE status = 'CLOSED' AND pnl > 0"
            )
            .fetch_one(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to calculate win rate: {}", e)))?
        } else {
            0
        };

        let win_rate = if closed_positions > 0 {
            winning_trades as f64 / closed_positions as f64
        } else {
            0.0
        };

        Ok(PositionSummary {
            total_positions,
            open_positions: summary_row.get("open_positions"),
            closed_positions,
            total_pnl: summary_row.get("total_pnl"),
            total_fees: summary_row.get("total_fees"),
            average_hold_time: summary_row.get("avg_hold_time"),
            win_rate,
            best_trade: if summary_row.get::<f64, _>("best_trade") > 0.0 {
                Some(summary_row.get("best_trade"))
            } else {
                None
            },
            worst_trade: if summary_row.get::<f64, _>("worst_trade") < 0.0 {
                Some(summary_row.get("worst_trade"))
            } else {
                None
            },
        })
    }

    /// Get positions by insider wallet
    pub async fn get_positions_by_insider(&self, insider_wallet: &str) -> Result<Vec<Position>, DatabaseError> {
        let positions = sqlx::query_as::<_, Position>(
            "SELECT * FROM positions WHERE insider_wallet = ? ORDER BY entry_timestamp DESC"
        )
        .bind(insider_wallet)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch insider positions: {}", e)))?;

        Ok(positions)
    }

    /// Update position price for real-time tracking
    pub async fn update_position_price(&self, token_mint: &str, current_price: f64) -> Result<(), DatabaseError> {
        // Update in-memory positions
        {
            let mut open_positions = self.open_positions.write().await;
            if let Some(position) = open_positions.get_mut(token_mint) {
                // Calculate unrealized P&L
                let unrealized_pnl = (current_price - position.entry_price) * position.quantity - position.fees;
                position.pnl = Some(unrealized_pnl);
                position.updated_at = Utc::now().timestamp();
            }
        }

        // Optionally update database for historical tracking
        sqlx::query(
            "UPDATE positions SET updated_at = ? WHERE token_mint = ? AND status = 'OPEN'"
        )
        .bind(Utc::now().timestamp())
        .bind(token_mint)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to update position timestamp: {}", e)))?;

        Ok(())
    }

    /// Get recent position history
    pub async fn get_recent_positions(&self, limit: i64) -> Result<Vec<Position>, DatabaseError> {
        let positions = sqlx::query_as::<_, Position>(
            "SELECT * FROM positions ORDER BY created_at DESC LIMIT ?"
        )
        .bind(limit)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch recent positions: {}", e)))?;

        Ok(positions)
    }
}