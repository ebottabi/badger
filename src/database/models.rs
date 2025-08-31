use serde::{Deserialize, Serialize};
use sqlx::{Row, FromRow, SqlitePool};
use std::collections::HashMap;
use std::time::Duration;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Real SQLite database implementation for Phase 3
#[derive(Debug, Clone)]
pub struct BadgerDatabase {
    pool: SqlitePool,
}

impl BadgerDatabase {
    /// Create a new database connection and run migrations
    pub async fn new(database_url: &str) -> Result<Self, super::DatabaseError> {
        // Extract the file path from the database URL
        let db_path = database_url.strip_prefix("sqlite:").unwrap_or(database_url);
        
        // Create data directory if it doesn't exist (async)
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| super::DatabaseError::ConnectionError(format!("Failed to create data directory: {}", e)))?;
        }

        // Enhanced SQLite configuration with performance optimizations
        use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous, SqlitePoolOptions};
        use std::str::FromStr;
        
        let connection_options = SqliteConnectOptions::from_str(database_url)
            .map_err(|e| super::DatabaseError::ConnectionError(format!("Invalid database URL: {}", e)))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)      // Better performance than FULL
            .busy_timeout(Duration::from_secs(30))       // Handle contention
            .pragma("cache_size", "-64000")              // 64MB cache
            .pragma("temp_store", "memory")              // Temp tables in memory
            .pragma("mmap_size", "268435456")            // 256MB memory map
            .pragma("optimize", "1")                     // Enable query optimizer
            .pragma("wal_autocheckpoint", "1000");       // Checkpoint every 1000 pages
            
        // Advanced connection pooling configuration
        let pool = SqlitePoolOptions::new()
            .min_connections(2)                          // Always maintain 2 connections
            .max_connections(8)                          // Scale up to 8 under load
            .acquire_timeout(Duration::from_secs(10))    // Wait up to 10s for connection
            .idle_timeout(Duration::from_secs(300))      // Close idle connections after 5min
            .max_lifetime(Duration::from_secs(1800))     // Replace connections after 30min
            .connect_with(connection_options)
            .await
            .map_err(|e| super::DatabaseError::ConnectionError(format!("Failed to connect to database: {}", e)))?;

        let db = Self { pool };

        // Run minimal model initialization (session setup only)
        db.run_migrations().await?;

        tracing::info!("✅ BadgerDatabase connected to: {}", database_url);
        Ok(db)
    }

    /// Run database migrations to create tables and indexes
    /// DISABLED: Now using comprehensive migration system from migrations/ directory
    async fn run_migrations(&self) -> Result<(), super::DatabaseError> {
        tracing::info!("🔄 Skipping old model-based migrations (using migration files)");
        
        // Session initialization will be handled after main migrations complete
        tracing::info!("✅ Model initialization completed successfully");
        Ok(())
    }

    /// Initialize a new session
    pub async fn initialize_session(&self) -> Result<(), super::DatabaseError> {
        // Initialize session using the migration schema columns
        sqlx::query(r#"
            INSERT OR IGNORE INTO session_stats (id) VALUES (1)
        "#)
        .execute(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to initialize session: {}", e)))?;

        Ok(())
    }

    /// Store a market event in the database
    pub async fn store_market_event(&self, event: crate::core::MarketEvent) -> Result<(), super::DatabaseError> {
        let event_id = Uuid::new_v4().to_string();
        let timestamp = Utc::now().timestamp();
        let event_type = format!("{:?}", std::mem::discriminant(&event));
        let data = serde_json::to_string(&event)
            .map_err(|e| super::DatabaseError::SerializationError(e.to_string()))?;

        // Extract slot information if available
        let slot = match &event {
            crate::core::MarketEvent::PoolCreated { pool, .. } => Some(pool.slot as i64),
            crate::core::MarketEvent::SwapDetected { swap } => Some(swap.slot as i64),
            _ => None,
        };

        sqlx::query(r#"
            INSERT INTO market_events (event_id, event_type, timestamp, slot, data, processed_at)
            VALUES (?, ?, ?, ?, ?, ?)
        "#)
        .bind(&event_id)
        .bind(&event_type)
        .bind(timestamp)
        .bind(slot)
        .bind(&data)
        .bind(timestamp)
        .execute(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to store market event: {}", e)))?;

        // Update session stats
        sqlx::query(r#"
            UPDATE session_stats 
            SET events_processed = events_processed + 1, 
                last_updated = strftime('%s', 'now')
            WHERE id = 1
        "#)
        .execute(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to update session stats: {}", e)))?;

        tracing::debug!("✅ Market event stored: {}", event_type);
        Ok(())
    }

    /// Store a trading signal in the database
    pub async fn store_trading_signal(&self, signal: crate::core::TradingSignal) -> Result<(), super::DatabaseError> {
        let signal_id = Uuid::new_v4().to_string();
        let timestamp = Utc::now().timestamp();
        let signal_type = format!("{:?}", std::mem::discriminant(&signal));
        let data = serde_json::to_string(&signal)
            .map_err(|e| super::DatabaseError::SerializationError(e.to_string()))?;

        // Extract signal details
        let (token_mint, confidence, amount_sol, reason) = match &signal {
            crate::core::TradingSignal::Buy { token_mint, confidence, max_amount_sol, reason, .. } => {
                (token_mint.clone(), Some(*confidence), Some(*max_amount_sol), Some(reason.clone()))
            }
            crate::core::TradingSignal::Sell { token_mint, reason, .. } => {
                (token_mint.clone(), None, None, Some(reason.clone()))
            }
            crate::core::TradingSignal::SwapActivity { token_mint, .. } => {
                (token_mint.clone(), None, None, Some("Swap activity detected".to_string()))
            }
        };

        sqlx::query(r#"
            INSERT INTO trading_signals (signal_id, signal_type, token_mint, confidence, amount_sol, reason, timestamp, data)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#)
        .bind(&signal_id)
        .bind(&signal_type)
        .bind(&token_mint)
        .bind(confidence)
        .bind(amount_sol)
        .bind(reason)
        .bind(timestamp)
        .bind(&data)
        .execute(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to store trading signal: {}", e)))?;

        // Update session stats
        sqlx::query(r#"
            UPDATE session_stats 
            SET signals_generated = signals_generated + 1,
                last_updated = strftime('%s', 'now')
            WHERE id = 1
        "#)
        .execute(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to update session stats: {}", e)))?;

        tracing::debug!("✅ Trading signal stored: {} for {}", signal_type, token_mint);
        Ok(())
    }

    /// Update wallet score in the database
    pub async fn update_wallet_score(&self, wallet_address: String, composite_score: f64) -> Result<(), super::DatabaseError> {
        let timestamp = Utc::now().timestamp();

        sqlx::query(r#"
            INSERT INTO wallet_scores 
            (wallet_address, composite_score, first_seen, last_updated, total_trades)
            VALUES (?, ?, ?, ?, 1)
            ON CONFLICT(wallet_address) DO UPDATE SET
                composite_score = ?,
                total_trades = total_trades + 1,
                last_updated = ?
        "#)
        .bind(&wallet_address)
        .bind(composite_score)
        .bind(timestamp)
        .bind(timestamp)
        .bind(composite_score)
        .bind(timestamp)
        .execute(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to update wallet score: {}", e)))?;

        tracing::debug!("✅ Wallet score updated: {} = {:.1}", &wallet_address[..8], composite_score);
        Ok(())
    }

    /// Get session statistics
    pub async fn get_session_stats(&self) -> Result<SessionStats, super::DatabaseError> {
        let row = sqlx::query_as::<_, SessionStats>(r#"
            SELECT session_start, events_processed, signals_generated, 
                   trades_executed, total_pnl, last_updated
            FROM session_stats WHERE id = 1
        "#)
        .fetch_one(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to get session stats: {}", e)))?;

        Ok(row)
    }

    /// Get top wallets by score
    pub async fn get_top_wallets(&self, limit: i64) -> Result<Vec<WalletScore>, super::DatabaseError> {
        let wallets = sqlx::query_as::<_, WalletScore>(r#"
            SELECT wallet_address, composite_score, insider_score, activity_score, 
                   performance_score, total_trades, successful_trades, total_volume_sol,
                   first_seen, last_updated
            FROM wallet_scores 
            ORDER BY composite_score DESC 
            LIMIT ?
        "#)
        .bind(limit)
        .fetch_all(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to get top wallets: {}", e)))?;

        Ok(wallets)
    }

    /// Get recent market events
    pub async fn get_recent_market_events(&self, limit: i64) -> Result<Vec<StoredMarketEvent>, super::DatabaseError> {
        let events = sqlx::query_as::<_, StoredMarketEvent>(r#"
            SELECT event_id as id, event_type, timestamp, data
            FROM market_events 
            ORDER BY timestamp DESC 
            LIMIT ?
        "#)
        .bind(limit)
        .fetch_all(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to get recent events: {}", e)))?;

        Ok(events)
    }

    /// Update analytics with provided data
    pub async fn update_analytics(&self, analytics: AnalyticsData) -> Result<(), super::DatabaseError> {
        let query = r#"
            INSERT OR REPLACE INTO analytics (
                id, session_start, total_trades, winning_trades, losing_trades,
                win_rate, total_pnl, sharpe_ratio, max_drawdown, current_portfolio_value,
                calculated_at
            ) VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%s', 'now'))
        "#;

        sqlx::query(query)
            .bind(analytics.session_start)
            .bind(analytics.total_trades as i64)
            .bind(analytics.winning_trades as i64)
            .bind(analytics.losing_trades as i64)
            .bind(analytics.win_rate)
            .bind(analytics.total_pnl)
            .bind(analytics.sharpe_ratio)
            .bind(analytics.max_drawdown)
            .bind(analytics.current_portfolio_value)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Calculate and store analytics
    pub async fn calculate_and_store_analytics(&self) -> Result<AnalyticsData, super::DatabaseError> {
        let timestamp = Utc::now().timestamp();

        // Get current session stats
        let session_stats = self.get_session_stats().await?;

        // Calculate basic analytics (would be more complex with real trading data)
        let total_trades = session_stats.signals_generated as u32;
        let winning_trades = ((total_trades as f32) * 0.7) as u32; // Mock 70% win rate
        let losing_trades = total_trades - winning_trades;
        let win_rate = if total_trades > 0 { winning_trades as f64 / total_trades as f64 } else { 0.0 };
        let total_pnl = (total_trades as f64) * 0.1; // Mock 0.1 SOL per trade
        let sharpe_ratio = if total_trades > 10 { 1.8 } else { 0.0 };
        let max_drawdown = 0.05; // Mock 5% max drawdown
        let current_portfolio_value = 100.0 + total_pnl;

        let analytics = AnalyticsData {
            session_start: session_stats.session_start,
            total_pnl,
            win_rate,
            total_trades,
            winning_trades,
            losing_trades,
            sharpe_ratio,
            max_drawdown,
            current_portfolio_value,
        };

        // Store analytics in database
        sqlx::query(r#"
            INSERT OR REPLACE INTO analytics 
            (id, session_start, total_pnl, win_rate, total_trades, winning_trades, 
             losing_trades, sharpe_ratio, max_drawdown, current_portfolio_value, calculated_at)
            VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#)
        .bind(analytics.session_start)
        .bind(analytics.total_pnl)
        .bind(analytics.win_rate)
        .bind(analytics.total_trades as i64)
        .bind(analytics.winning_trades as i64)
        .bind(analytics.losing_trades as i64)
        .bind(analytics.sharpe_ratio)
        .bind(analytics.max_drawdown)
        .bind(analytics.current_portfolio_value)
        .bind(timestamp)
        .execute(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to store analytics: {}", e)))?;

        Ok(analytics)
    }

    /// Get current analytics summary
    pub async fn get_analytics_summary(&self) -> Result<AnalyticsData, super::DatabaseError> {
        let row = sqlx::query_as::<_, AnalyticsData>(r#"
            SELECT session_start, total_pnl, win_rate, total_trades, winning_trades,
                   losing_trades, sharpe_ratio, max_drawdown, current_portfolio_value
            FROM analytics WHERE id = 1
        "#)
        .fetch_optional(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to get analytics: {}", e)))?;

        // If no analytics exist yet, return default
        Ok(row.unwrap_or_else(|| AnalyticsData {
            session_start: Utc::now().timestamp(),
            total_pnl: 0.0,
            win_rate: 0.0,
            total_trades: 0,
            winning_trades: 0,
            losing_trades: 0,
            sharpe_ratio: 0.0,
            max_drawdown: 0.0,
            current_portfolio_value: 100.0,
        }))
    }

    /// Update session uptime (now just updates last_updated timestamp)
    pub async fn update_uptime(&self) -> Result<(), super::DatabaseError> {
        sqlx::query(r#"
            UPDATE session_stats 
            SET last_updated = strftime('%s', 'now')
            WHERE id = 1
        "#)
        .execute(&self.pool).await
        .map_err(|e| super::DatabaseError::QueryError(format!("Failed to update uptime: {}", e)))?;

        Ok(())
    }

    /// Get database pool reference for advanced operations
    pub fn get_pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Begin a new database transaction
    pub async fn begin_transaction(&self) -> Result<sqlx::Transaction<sqlx::Sqlite>, super::DatabaseError> {
        self.pool.begin().await
            .map_err(|e| super::DatabaseError::QueryError(format!("Failed to begin transaction: {}", e)))
    }

    /// Get database health information
    pub async fn get_health_info(&self) -> Result<DatabaseHealth, super::DatabaseError> {
        let market_events_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM market_events")
            .fetch_one(&self.pool).await
            .map_err(|e| super::DatabaseError::QueryError(format!("Failed to count market events: {}", e)))?;

        let trading_signals_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM trading_signals")
            .fetch_one(&self.pool).await
            .map_err(|e| super::DatabaseError::QueryError(format!("Failed to count trading signals: {}", e)))?;

        let wallets_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM wallet_scores")
            .fetch_one(&self.pool).await
            .map_err(|e| super::DatabaseError::QueryError(format!("Failed to count wallets: {}", e)))?;

        Ok(DatabaseHealth {
            market_events_count,
            trading_signals_count,
            wallets_count,
            is_connected: true,
        })
    }
}

// Database model structs with SQLite derive implementations
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SessionStats {
    pub session_start: i64,
    pub events_processed: i64,
    pub signals_generated: i64,
    pub trades_executed: i64,
    pub total_pnl: f64,
    pub last_updated: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WalletScore {
    pub wallet_address: String,
    pub composite_score: f64,
    pub insider_score: f64,
    pub activity_score: f64,
    pub performance_score: f64,
    pub total_trades: i64,
    pub successful_trades: i64,
    pub total_volume_sol: f64,
    pub first_seen: i64,
    pub last_updated: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct StoredMarketEvent {
    pub id: String,
    pub event_type: String,
    pub timestamp: i64,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AnalyticsData {
    pub session_start: i64,
    pub total_pnl: f64,
    pub win_rate: f64,
    pub total_trades: u32,
    pub winning_trades: u32,
    pub losing_trades: u32,
    pub sharpe_ratio: f64,
    pub max_drawdown: f64,
    pub current_portfolio_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseHealth {
    pub market_events_count: i64,
    pub trading_signals_count: i64,
    pub wallets_count: i64,
    pub is_connected: bool,
}