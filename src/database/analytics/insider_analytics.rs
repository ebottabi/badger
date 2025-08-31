use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tracing::{debug, info, warn, error, instrument};

use super::position_tracker::{Position, PositionTracker};
use super::super::{BadgerDatabase, DatabaseError};
use crate::core::{MarketEvent, TradingSignal};

/// Insider wallet profile and performance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsiderProfile {
    pub wallet_address: String,
    pub first_seen: i64,
    pub last_activity: i64,
    pub total_trades: i64,
    pub successful_trades: i64,
    pub success_rate: f64,
    pub total_volume: f64,
    pub average_trade_size: f64,
    pub total_pnl: f64,
    pub roi_percentage: f64,
    pub average_hold_time: f64, // in hours
    pub favorite_tokens: Vec<String>,
    pub trading_frequency: f64, // trades per day
    pub confidence_score: f64, // 0-100 based on performance
    pub risk_score: f64, // 0-100 based on volatility
    pub copy_worthiness: f64, // 0-100 overall score
    pub last_updated: i64,
}

/// Insider trading pattern analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsiderPattern {
    pub wallet_address: String,
    pub pattern_type: String, // "EARLY_BUYER", "WHALE_ACCUMULATOR", "PUMP_DETECTOR", "SNIPER"
    pub confidence: f64,
    pub frequency: f64, // how often this pattern occurs
    pub avg_profit: f64,
    pub typical_hold_time: f64,
    pub risk_level: String, // "LOW", "MEDIUM", "HIGH"
    pub last_detected: i64,
}

/// Token insider activity summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInsiderActivity {
    pub token_mint: String,
    pub insider_count: i64,
    pub total_insider_volume: f64,
    pub insider_buy_pressure: f64,
    pub insider_sell_pressure: f64,
    pub top_insider_wallets: Vec<String>,
    pub unusual_activity: bool,
    pub activity_score: f64, // 0-100
    pub first_insider_entry: Option<i64>,
    pub last_insider_activity: i64,
}

/// Copy trade recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyTradeSignal {
    pub insider_wallet: String,
    pub token_mint: String,
    pub action: String, // "BUY", "SELL"
    pub confidence: f64,
    pub recommended_size: f64, // percentage of portfolio
    pub expected_hold_time: f64, // in hours
    pub risk_level: String,
    pub reasoning: String,
    pub created_at: i64,
}

/// Insider wallet analytics and tracking system
pub struct InsiderAnalytics {
    db: Arc<BadgerDatabase>,
    position_tracker: Arc<PositionTracker>,
    tracked_wallets: Arc<tokio::sync::RwLock<HashMap<String, InsiderProfile>>>,
}

impl InsiderAnalytics {
    pub fn new(db: Arc<BadgerDatabase>, position_tracker: Arc<PositionTracker>) -> Self {
        Self {
            db,
            position_tracker,
            tracked_wallets: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Initialize insider analytics schema
    #[instrument(skip(self))]
    pub async fn initialize_schema(&self) -> Result<(), DatabaseError> {
        info!("🔧 Initializing insider analytics database schema");

        let create_insider_profiles = r#"
            CREATE TABLE IF NOT EXISTS insider_profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_address TEXT NOT NULL UNIQUE,
                first_seen INTEGER NOT NULL,
                last_activity INTEGER NOT NULL,
                total_trades INTEGER NOT NULL DEFAULT 0,
                successful_trades INTEGER NOT NULL DEFAULT 0,
                success_rate REAL NOT NULL DEFAULT 0.0,
                total_volume REAL NOT NULL DEFAULT 0.0,
                average_trade_size REAL NOT NULL DEFAULT 0.0,
                total_pnl REAL NOT NULL DEFAULT 0.0,
                roi_percentage REAL NOT NULL DEFAULT 0.0,
                average_hold_time REAL NOT NULL DEFAULT 0.0,
                favorite_tokens TEXT, -- JSON array
                trading_frequency REAL NOT NULL DEFAULT 0.0,
                confidence_score REAL NOT NULL DEFAULT 0.0,
                risk_score REAL NOT NULL DEFAULT 0.0,
                copy_worthiness REAL NOT NULL DEFAULT 0.0,
                last_updated INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )
        "#;

        let create_insider_patterns = r#"
            CREATE TABLE IF NOT EXISTS insider_patterns (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_address TEXT NOT NULL,
                pattern_type TEXT NOT NULL,
                confidence REAL NOT NULL,
                frequency REAL NOT NULL DEFAULT 0.0,
                avg_profit REAL NOT NULL DEFAULT 0.0,
                typical_hold_time REAL NOT NULL DEFAULT 0.0,
                risk_level TEXT NOT NULL DEFAULT 'MEDIUM' CHECK (risk_level IN ('LOW', 'MEDIUM', 'HIGH')),
                last_detected INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                UNIQUE(wallet_address, pattern_type)
            )
        "#;

        let create_insider_activities = r#"
            CREATE TABLE IF NOT EXISTS insider_activities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_address TEXT NOT NULL,
                token_mint TEXT NOT NULL,
                activity_type TEXT NOT NULL CHECK (activity_type IN ('BUY', 'SELL', 'TRANSFER')),
                amount REAL NOT NULL,
                price REAL,
                transaction_hash TEXT,
                block_slot INTEGER,
                timestamp INTEGER NOT NULL,
                confidence REAL NOT NULL DEFAULT 1.0,
                detected_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )
        "#;

        let create_token_insider_summary = r#"
            CREATE TABLE IF NOT EXISTS token_insider_summary (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                token_mint TEXT NOT NULL UNIQUE,
                insider_count INTEGER NOT NULL DEFAULT 0,
                total_insider_volume REAL NOT NULL DEFAULT 0.0,
                insider_buy_pressure REAL NOT NULL DEFAULT 0.0,
                insider_sell_pressure REAL NOT NULL DEFAULT 0.0,
                top_insider_wallets TEXT, -- JSON array
                unusual_activity BOOLEAN NOT NULL DEFAULT 0,
                activity_score REAL NOT NULL DEFAULT 0.0,
                first_insider_entry INTEGER,
                last_insider_activity INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                last_updated INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )
        "#;

        let create_copy_trade_signals = r#"
            CREATE TABLE IF NOT EXISTS copy_trade_signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                insider_wallet TEXT NOT NULL,
                token_mint TEXT NOT NULL,
                action TEXT NOT NULL CHECK (action IN ('BUY', 'SELL')),
                confidence REAL NOT NULL,
                recommended_size REAL NOT NULL DEFAULT 0.0,
                expected_hold_time REAL NOT NULL DEFAULT 0.0,
                risk_level TEXT NOT NULL DEFAULT 'MEDIUM' CHECK (risk_level IN ('LOW', 'MEDIUM', 'HIGH')),
                reasoning TEXT,
                status TEXT NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'EXECUTED', 'EXPIRED', 'CANCELLED')),
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                expires_at INTEGER
            )
        "#;

        // Create indexes for better query performance
        let create_indexes = vec![
            "CREATE INDEX IF NOT EXISTS idx_insider_profiles_wallet ON insider_profiles(wallet_address)",
            "CREATE INDEX IF NOT EXISTS idx_insider_profiles_score ON insider_profiles(copy_worthiness DESC)",
            "CREATE INDEX IF NOT EXISTS idx_insider_activities_wallet ON insider_activities(wallet_address)",
            "CREATE INDEX IF NOT EXISTS idx_insider_activities_token ON insider_activities(token_mint)",
            "CREATE INDEX IF NOT EXISTS idx_insider_activities_timestamp ON insider_activities(timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_token_insider_token ON token_insider_summary(token_mint)",
            "CREATE INDEX IF NOT EXISTS idx_copy_signals_status ON copy_trade_signals(status)",
            "CREATE INDEX IF NOT EXISTS idx_copy_signals_created ON copy_trade_signals(created_at)",
        ];

        // Execute schema creation
        for table_sql in [
            create_insider_profiles, 
            create_insider_patterns, 
            create_insider_activities, 
            create_token_insider_summary, 
            create_copy_trade_signals
        ] {
            sqlx::query(table_sql)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to create insider analytics table: {}", e)))?;
        }

        for index_sql in create_indexes {
            sqlx::query(index_sql)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to create index: {}", e)))?;
        }

        info!("✅ Insider analytics database schema initialized");
        Ok(())
    }

    /// Track new insider wallet activity
    #[instrument(skip(self))]
    pub async fn track_insider_activity(
        &self,
        wallet_address: &str,
        token_mint: &str,
        activity_type: &str,
        amount: f64,
        price: Option<f64>,
        transaction_hash: Option<&str>,
        block_slot: Option<i64>,
    ) -> Result<(), DatabaseError> {
        let now = Utc::now().timestamp();

        // Insert activity record
        sqlx::query(r#"
            INSERT INTO insider_activities (
                wallet_address, token_mint, activity_type, amount, price, 
                transaction_hash, block_slot, timestamp, confidence
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1.0)
        "#)
        .bind(wallet_address)
        .bind(token_mint)
        .bind(activity_type)
        .bind(amount)
        .bind(price)
        .bind(transaction_hash)
        .bind(block_slot)
        .bind(now)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to track insider activity: {}", e)))?;

        // Update or create insider profile
        self.update_insider_profile(wallet_address).await?;

        // Update token insider summary
        self.update_token_insider_summary(token_mint).await?;

        debug!(
            "📈 Tracked insider activity: {} {} {} tokens for ${:.4}",
            wallet_address, activity_type, amount, price.unwrap_or(0.0)
        );

        Ok(())
    }

    /// Update insider profile based on recent activity
    #[instrument(skip(self))]
    async fn update_insider_profile(&self, wallet_address: &str) -> Result<(), DatabaseError> {
        let now = Utc::now().timestamp();

        // Calculate profile statistics
        let stats = sqlx::query(r#"
            SELECT 
                MIN(timestamp) as first_seen,
                MAX(timestamp) as last_activity,
                COUNT(*) as total_trades,
                COUNT(CASE WHEN activity_type = 'BUY' THEN 1 END) as buy_trades,
                COUNT(CASE WHEN activity_type = 'SELL' THEN 1 END) as sell_trades,
                COALESCE(CAST(SUM(amount * COALESCE(price, 0)) AS REAL), 0.0) as total_volume,
                COALESCE(CAST(AVG(amount * COALESCE(price, 0)) AS REAL), 0.0) as avg_trade_size
            FROM insider_activities 
            WHERE wallet_address = ?
        "#)
        .bind(wallet_address)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to calculate insider stats: {}", e)))?;

        let first_seen: i64 = stats.get("first_seen");
        let last_activity: i64 = stats.get("last_activity");
        let total_trades: i64 = stats.get("total_trades");
        let total_volume: f64 = stats.get("total_volume");
        let avg_trade_size: f64 = stats.get("avg_trade_size");

        // Calculate trading frequency (trades per day)
        let days_active = ((now - first_seen) as f64 / 86400.0).max(1.0);
        let trading_frequency = total_trades as f64 / days_active;

        // Get positions linked to this insider for P&L calculation
        let positions = self.position_tracker.get_positions_by_insider(wallet_address).await?;
        
        let mut successful_trades = 0i64;
        let mut total_pnl = 0.0;
        let mut hold_times = Vec::new();
        
        for position in &positions {
            if position.status == "CLOSED" {
                if let Some(pnl) = position.pnl {
                    total_pnl += pnl;
                    if pnl > 0.0 {
                        successful_trades += 1;
                    }
                    
                    if let Some(exit_time) = position.exit_timestamp {
                        let hold_time_hours = (exit_time - position.entry_timestamp) as f64 / 3600.0;
                        hold_times.push(hold_time_hours);
                    }
                }
            }
        }

        let success_rate = if positions.len() > 0 {
            successful_trades as f64 / positions.len() as f64
        } else {
            0.0
        };

        let roi_percentage = if total_volume > 0.0 {
            (total_pnl / total_volume) * 100.0
        } else {
            0.0
        };

        let avg_hold_time = if !hold_times.is_empty() {
            hold_times.iter().sum::<f64>() / hold_times.len() as f64
        } else {
            0.0
        };

        // Calculate confidence score (0-100)
        let confidence_score = self.calculate_confidence_score(success_rate, total_trades, roi_percentage, trading_frequency);

        // Calculate risk score (0-100)
        let risk_score = self.calculate_risk_score(&positions);

        // Calculate copy worthiness (0-100) - overall score
        let copy_worthiness = (confidence_score * 0.4 + (100.0 - risk_score) * 0.3 + success_rate * 100.0 * 0.3).min(100.0);

        // Get favorite tokens (top 5)
        let favorite_tokens = self.get_favorite_tokens(wallet_address, 5).await?;
        let favorite_tokens_json = serde_json::to_string(&favorite_tokens)
            .unwrap_or_else(|_| "[]".to_string());

        // Upsert profile
        sqlx::query(r#"
            INSERT INTO insider_profiles (
                wallet_address, first_seen, last_activity, total_trades, successful_trades,
                success_rate, total_volume, average_trade_size, total_pnl, roi_percentage,
                average_hold_time, favorite_tokens, trading_frequency, confidence_score,
                risk_score, copy_worthiness, last_updated
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(wallet_address) DO UPDATE SET
                last_activity = excluded.last_activity,
                total_trades = excluded.total_trades,
                successful_trades = excluded.successful_trades,
                success_rate = excluded.success_rate,
                total_volume = excluded.total_volume,
                average_trade_size = excluded.average_trade_size,
                total_pnl = excluded.total_pnl,
                roi_percentage = excluded.roi_percentage,
                average_hold_time = excluded.average_hold_time,
                favorite_tokens = excluded.favorite_tokens,
                trading_frequency = excluded.trading_frequency,
                confidence_score = excluded.confidence_score,
                risk_score = excluded.risk_score,
                copy_worthiness = excluded.copy_worthiness,
                last_updated = excluded.last_updated
        "#)
        .bind(wallet_address)
        .bind(first_seen)
        .bind(last_activity)
        .bind(total_trades)
        .bind(successful_trades)
        .bind(success_rate)
        .bind(total_volume)
        .bind(avg_trade_size)
        .bind(total_pnl)
        .bind(roi_percentage)
        .bind(avg_hold_time)
        .bind(favorite_tokens_json)
        .bind(trading_frequency)
        .bind(confidence_score)
        .bind(risk_score)
        .bind(copy_worthiness)
        .bind(now)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to update insider profile: {}", e)))?;

        // Update in-memory cache
        {
            let mut tracked_wallets = self.tracked_wallets.write().await;
            tracked_wallets.insert(wallet_address.to_string(), InsiderProfile {
                wallet_address: wallet_address.to_string(),
                first_seen,
                last_activity,
                total_trades,
                successful_trades,
                success_rate,
                total_volume,
                average_trade_size: avg_trade_size,
                total_pnl,
                roi_percentage,
                average_hold_time: avg_hold_time,
                favorite_tokens,
                trading_frequency,
                confidence_score,
                risk_score,
                copy_worthiness,
                last_updated: now,
            });
        }

        Ok(())
    }

    /// Update token insider summary
    async fn update_token_insider_summary(&self, token_mint: &str) -> Result<(), DatabaseError> {
        let now = Utc::now().timestamp();

        // Calculate token insider metrics
        let stats = sqlx::query(r#"
            SELECT 
                COUNT(DISTINCT wallet_address) as insider_count,
                COALESCE(CAST(SUM(amount * COALESCE(price, 0)) AS REAL), 0.0) as total_volume,
                COALESCE(CAST(SUM(CASE WHEN activity_type = 'BUY' THEN amount * COALESCE(price, 0) ELSE 0 END) AS REAL), 0.0) as buy_volume,
                COALESCE(CAST(SUM(CASE WHEN activity_type = 'SELL' THEN amount * COALESCE(price, 0) ELSE 0 END) AS REAL), 0.0) as sell_volume,
                MIN(timestamp) as first_activity,
                MAX(timestamp) as last_activity
            FROM insider_activities 
            WHERE token_mint = ?
        "#)
        .bind(token_mint)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to calculate token insider stats: {}", e)))?;

        let insider_count: i64 = stats.get("insider_count");
        let total_volume: f64 = stats.get("total_volume");
        let buy_volume: f64 = stats.get("buy_volume");
        let sell_volume: f64 = stats.get("sell_volume");
        let first_activity: Option<i64> = stats.get("first_activity");
        let last_activity: i64 = stats.get("last_activity");

        let buy_pressure = if total_volume > 0.0 {
            buy_volume / total_volume
        } else {
            0.0
        };

        let sell_pressure = if total_volume > 0.0 {
            sell_volume / total_volume
        } else {
            0.0
        };

        // Get top insider wallets (by volume)
        let top_wallets = sqlx::query_scalar::<_, String>(r#"
            SELECT wallet_address 
            FROM insider_activities 
            WHERE token_mint = ?
            GROUP BY wallet_address 
            ORDER BY CAST(SUM(amount * COALESCE(price, 0)) AS REAL) DESC 
            LIMIT 5
        "#)
        .bind(token_mint)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get top insider wallets: {}", e)))?;

        let top_wallets_json = serde_json::to_string(&top_wallets)
            .unwrap_or_else(|_| "[]".to_string());

        // Detect unusual activity (more than 3x normal volume in last hour)
        let recent_volume = sqlx::query_scalar::<_, f64>(r#"
            SELECT COALESCE(CAST(SUM(amount * COALESCE(price, 0)) AS REAL), 0.0)
            FROM insider_activities 
            WHERE token_mint = ? AND timestamp >= ?
        "#)
        .bind(token_mint)
        .bind(now - 3600) // Last hour
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to calculate recent volume: {}", e)))?;

        let historical_avg_volume = if total_volume > 0.0 && first_activity.is_some() {
            let days_since_first = ((now - first_activity.unwrap()) as f64 / 86400.0).max(1.0);
            total_volume / (days_since_first * 24.0) // Hourly average
        } else {
            0.0
        };

        let unusual_activity = recent_volume > historical_avg_volume * 3.0 && recent_volume > 1000.0;

        // Calculate activity score (0-100)
        let activity_score = self.calculate_activity_score(insider_count, buy_pressure, sell_pressure, unusual_activity);

        // Upsert token summary
        sqlx::query(r#"
            INSERT INTO token_insider_summary (
                token_mint, insider_count, total_insider_volume, insider_buy_pressure,
                insider_sell_pressure, top_insider_wallets, unusual_activity, activity_score,
                first_insider_entry, last_insider_activity, last_updated
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(token_mint) DO UPDATE SET
                insider_count = excluded.insider_count,
                total_insider_volume = excluded.total_insider_volume,
                insider_buy_pressure = excluded.insider_buy_pressure,
                insider_sell_pressure = excluded.insider_sell_pressure,
                top_insider_wallets = excluded.top_insider_wallets,
                unusual_activity = excluded.unusual_activity,
                activity_score = excluded.activity_score,
                last_insider_activity = excluded.last_insider_activity,
                last_updated = excluded.last_updated
        "#)
        .bind(token_mint)
        .bind(insider_count)
        .bind(total_volume)
        .bind(buy_pressure)
        .bind(sell_pressure)
        .bind(top_wallets_json)
        .bind(unusual_activity)
        .bind(activity_score)
        .bind(first_activity)
        .bind(last_activity)
        .bind(now)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to update token insider summary: {}", e)))?;

        Ok(())
    }

    /// Generate copy trade signal based on insider activity
    #[instrument(skip(self))]
    pub async fn generate_copy_trade_signal(
        &self,
        insider_wallet: &str,
        token_mint: &str,
        action: &str,
    ) -> Result<Option<CopyTradeSignal>, DatabaseError> {
        // Get insider profile
        let profile = self.get_insider_profile(insider_wallet).await?;
        
        if let Some(profile) = profile {
            // Only generate signals for high-quality insiders
            if profile.copy_worthiness < 60.0 {
                return Ok(None);
            }

            let confidence = (profile.copy_worthiness / 100.0 * profile.success_rate).min(1.0);
            
            let recommended_size = match profile.risk_score {
                r if r < 30.0 => 5.0,  // Low risk: 5% of portfolio
                r if r < 60.0 => 3.0,  // Medium risk: 3% of portfolio
                _ => 1.0,              // High risk: 1% of portfolio
            };

            let risk_level = match profile.risk_score {
                r if r < 30.0 => "LOW",
                r if r < 60.0 => "MEDIUM",
                _ => "HIGH",
            }.to_string();

            let reasoning = format!(
                "Insider {} has {:.1}% success rate, {:.1}% ROI, and {:.1}% copy worthiness score. Recent {} activity detected.",
                insider_wallet, 
                profile.success_rate * 100.0, 
                profile.roi_percentage, 
                profile.copy_worthiness,
                action.to_lowercase()
            );

            let signal = CopyTradeSignal {
                insider_wallet: insider_wallet.to_string(),
                token_mint: token_mint.to_string(),
                action: action.to_string(),
                confidence,
                recommended_size,
                expected_hold_time: profile.average_hold_time,
                risk_level,
                reasoning,
                created_at: Utc::now().timestamp(),
            };

            // Save signal to database
            sqlx::query(r#"
                INSERT INTO copy_trade_signals (
                    insider_wallet, token_mint, action, confidence, recommended_size,
                    expected_hold_time, risk_level, reasoning, expires_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#)
            .bind(&signal.insider_wallet)
            .bind(&signal.token_mint)
            .bind(&signal.action)
            .bind(signal.confidence)
            .bind(signal.recommended_size)
            .bind(signal.expected_hold_time)
            .bind(&signal.risk_level)
            .bind(&signal.reasoning)
            .bind(signal.created_at + 3600) // Expire in 1 hour
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to save copy trade signal: {}", e)))?;

            info!(
                "🚨 Generated copy trade signal: {} {} {} (confidence: {:.2})",
                action, token_mint, insider_wallet, confidence
            );

            Ok(Some(signal))
        } else {
            Ok(None)
        }
    }

    /// Get insider profile by wallet address
    pub async fn get_insider_profile(&self, wallet_address: &str) -> Result<Option<InsiderProfile>, DatabaseError> {
        // Check memory cache first
        {
            let tracked_wallets = self.tracked_wallets.read().await;
            if let Some(profile) = tracked_wallets.get(wallet_address) {
                return Ok(Some(profile.clone()));
            }
        }

        // Query database
        let row = sqlx::query(r#"
            SELECT * FROM insider_profiles WHERE wallet_address = ?
        "#)
        .bind(wallet_address)
        .fetch_optional(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch insider profile: {}", e)))?;

        if let Some(row) = row {
            let favorite_tokens_json: String = row.get("favorite_tokens");
            let favorite_tokens: Vec<String> = serde_json::from_str(&favorite_tokens_json)
                .unwrap_or_else(|_| Vec::new());

            let profile = InsiderProfile {
                wallet_address: row.get("wallet_address"),
                first_seen: row.get("first_seen"),
                last_activity: row.get("last_activity"),
                total_trades: row.get("total_trades"),
                successful_trades: row.get("successful_trades"),
                success_rate: row.get("success_rate"),
                total_volume: row.get("total_volume"),
                average_trade_size: row.get("average_trade_size"),
                total_pnl: row.get("total_pnl"),
                roi_percentage: row.get("roi_percentage"),
                average_hold_time: row.get("average_hold_time"),
                favorite_tokens,
                trading_frequency: row.get("trading_frequency"),
                confidence_score: row.get("confidence_score"),
                risk_score: row.get("risk_score"),
                copy_worthiness: row.get("copy_worthiness"),
                last_updated: row.get("last_updated"),
            };

            // Update cache
            {
                let mut tracked_wallets = self.tracked_wallets.write().await;
                tracked_wallets.insert(wallet_address.to_string(), profile.clone());
            }

            Ok(Some(profile))
        } else {
            Ok(None)
        }
    }

    /// Get top performing insiders
    pub async fn get_top_insiders(&self, limit: i64) -> Result<Vec<InsiderProfile>, DatabaseError> {
        let rows = sqlx::query(r#"
            SELECT * FROM insider_profiles 
            ORDER BY copy_worthiness DESC, success_rate DESC 
            LIMIT ?
        "#)
        .bind(limit)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch top insiders: {}", e)))?;

        let mut profiles = Vec::new();
        for row in rows {
            let favorite_tokens_json: String = row.get("favorite_tokens");
            let favorite_tokens: Vec<String> = serde_json::from_str(&favorite_tokens_json)
                .unwrap_or_else(|_| Vec::new());

            profiles.push(InsiderProfile {
                wallet_address: row.get("wallet_address"),
                first_seen: row.get("first_seen"),
                last_activity: row.get("last_activity"),
                total_trades: row.get("total_trades"),
                successful_trades: row.get("successful_trades"),
                success_rate: row.get("success_rate"),
                total_volume: row.get("total_volume"),
                average_trade_size: row.get("average_trade_size"),
                total_pnl: row.get("total_pnl"),
                roi_percentage: row.get("roi_percentage"),
                average_hold_time: row.get("average_hold_time"),
                favorite_tokens,
                trading_frequency: row.get("trading_frequency"),
                confidence_score: row.get("confidence_score"),
                risk_score: row.get("risk_score"),
                copy_worthiness: row.get("copy_worthiness"),
                last_updated: row.get("last_updated"),
            });
        }

        Ok(profiles)
    }

    /// Get token insider activity summary
    pub async fn get_token_insider_activity(&self, token_mint: &str) -> Result<Option<TokenInsiderActivity>, DatabaseError> {
        let row = sqlx::query(r#"
            SELECT * FROM token_insider_summary WHERE token_mint = ?
        "#)
        .bind(token_mint)
        .fetch_optional(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch token insider activity: {}", e)))?;

        if let Some(row) = row {
            let top_insider_wallets_json: String = row.get("top_insider_wallets");
            let top_insider_wallets: Vec<String> = serde_json::from_str(&top_insider_wallets_json)
                .unwrap_or_else(|_| Vec::new());

            Ok(Some(TokenInsiderActivity {
                token_mint: row.get("token_mint"),
                insider_count: row.get("insider_count"),
                total_insider_volume: row.get("total_insider_volume"),
                insider_buy_pressure: row.get("insider_buy_pressure"),
                insider_sell_pressure: row.get("insider_sell_pressure"),
                top_insider_wallets,
                unusual_activity: row.get("unusual_activity"),
                activity_score: row.get("activity_score"),
                first_insider_entry: row.get("first_insider_entry"),
                last_insider_activity: row.get("last_insider_activity"),
            }))
        } else {
            Ok(None)
        }
    }

    // Helper methods for calculations

    fn calculate_confidence_score(&self, success_rate: f64, total_trades: i64, roi: f64, frequency: f64) -> f64 {
        let base_score = success_rate * 100.0;
        let volume_bonus = (total_trades.min(100) as f64 / 100.0) * 20.0; // Up to 20 points for volume
        let roi_bonus = (roi.max(-100.0).min(100.0) / 100.0) * 30.0; // Up to 30 points for ROI
        let frequency_bonus = (frequency.min(10.0) / 10.0) * 10.0; // Up to 10 points for frequency
        
        (base_score + volume_bonus + roi_bonus + frequency_bonus).min(100.0).max(0.0)
    }

    fn calculate_risk_score(&self, positions: &[Position]) -> f64 {
        if positions.is_empty() {
            return 50.0; // Medium risk if no data
        }

        let returns: Vec<f64> = positions.iter()
            .filter_map(|p| p.pnl)
            .collect();

        if returns.len() < 2 {
            return 50.0;
        }

        let mean = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns.iter()
            .map(|&x| (x - mean).powi(2))
            .sum::<f64>() / (returns.len() - 1) as f64;
        let std_dev = variance.sqrt();

        // Convert standard deviation to risk score (0-100)
        (std_dev * 10.0).min(100.0).max(0.0)
    }

    fn calculate_activity_score(&self, insider_count: i64, buy_pressure: f64, sell_pressure: f64, unusual_activity: bool) -> f64 {
        let count_score = (insider_count.min(20) as f64 / 20.0) * 40.0; // Up to 40 points
        let pressure_score = (buy_pressure - sell_pressure + 1.0) * 25.0; // Up to 50 points (buy pressure weighted)
        let unusual_bonus = if unusual_activity { 15.0 } else { 0.0 }; // 15 point bonus
        
        (count_score + pressure_score + unusual_bonus).min(100.0).max(0.0)
    }

    async fn get_favorite_tokens(&self, wallet_address: &str, limit: usize) -> Result<Vec<String>, DatabaseError> {
        let tokens = sqlx::query_scalar::<_, String>(r#"
            SELECT token_mint 
            FROM insider_activities 
            WHERE wallet_address = ? 
            GROUP BY token_mint 
            ORDER BY COUNT(*) DESC, CAST(SUM(amount * COALESCE(price, 0)) AS REAL) DESC 
            LIMIT ?
        "#)
        .bind(wallet_address)
        .bind(limit as i64)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get favorite tokens: {}", e)))?;

        Ok(tokens)
    }
}