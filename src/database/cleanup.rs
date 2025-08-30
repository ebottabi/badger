use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{info, warn, error, debug, instrument};
use chrono::{DateTime, Utc, TimeZone};
use sqlx::Row;

use super::{BadgerDatabase, DatabaseError};

/// Data lifecycle management service
pub struct CleanupService {
    db: Arc<BadgerDatabase>,
    retention_config: RetentionConfig,
    archive_path: PathBuf,
    cleanup_interval: Duration,
}

/// Retention configuration
#[derive(Debug, Clone)]
pub struct RetentionConfig {
    /// Keep hot data for this many days (full performance)
    pub hot_data_days: u32,
    /// Keep warm data for this many days (compressed, limited indexes)
    pub warm_data_days: u32,
    /// Archive cold data older than this (separate compressed files)
    pub cold_data_days: u32,
    /// Permanently delete data older than this
    pub delete_data_days: u32,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            hot_data_days: 7,      // 1 week hot
            warm_data_days: 30,    // 1 month warm
            cold_data_days: 90,    // 3 months cold
            delete_data_days: 365, // 1 year delete
        }
    }
}

/// Cleanup statistics
#[derive(Debug, Clone)]
pub struct CleanupStats {
    pub hot_records: i64,
    pub warm_records: i64,
    pub cold_archived: i64,
    pub deleted_records: i64,
    pub disk_space_freed_mb: f64,
    pub last_cleanup: DateTime<Utc>,
}

impl CleanupService {
    pub fn new(
        db: Arc<BadgerDatabase>, 
        archive_path: PathBuf,
        retention_config: Option<RetentionConfig>
    ) -> Self {
        Self {
            db,
            retention_config: retention_config.unwrap_or_default(),
            archive_path,
            cleanup_interval: Duration::from_secs(3600), // Run every hour
        }
    }

    /// Start the cleanup service
    #[instrument(skip(self))]
    pub async fn run(self) -> Result<(), DatabaseError> {
        info!("🧹 Cleanup Service starting with retention policy:");
        info!("   📊 Hot data: {} days", self.retention_config.hot_data_days);
        info!("   🔥 Warm data: {} days", self.retention_config.warm_data_days);
        info!("   ❄️  Cold archive: {} days", self.retention_config.cold_data_days);
        info!("   🗑️  Delete after: {} days", self.retention_config.delete_data_days);

        // Ensure archive directory exists
        if let Err(e) = tokio::fs::create_dir_all(&self.archive_path).await {
            error!("Failed to create archive directory: {}", e);
            return Err(DatabaseError::InitializationError(
                format!("Could not create archive directory: {}", e)
            ));
        }

        let mut cleanup_timer = interval(self.cleanup_interval);
        let mut daily_cleanup_timer = interval(Duration::from_secs(86400)); // Daily full cleanup

        loop {
            tokio::select! {
                // Hourly light cleanup
                _ = cleanup_timer.tick() => {
                    if let Err(e) = self.run_light_cleanup().await {
                        warn!("Light cleanup failed: {}", e);
                    }
                }
                
                // Daily comprehensive cleanup
                _ = daily_cleanup_timer.tick() => {
                    if let Err(e) = self.run_full_cleanup().await {
                        error!("Full cleanup failed: {}", e);
                    }
                }
            }
        }
    }

    /// Light cleanup - only remove very old data
    async fn run_light_cleanup(&self) -> Result<(), DatabaseError> {
        debug!("🧹 Running light cleanup");
        
        let delete_threshold = Utc::now().timestamp() - (self.retention_config.delete_data_days as i64 * 86400);
        
        // Count and delete very old market events
        let events_to_delete = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM market_events WHERE timestamp < ?"
        )
        .bind(delete_threshold)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to count old events: {}", e)))?;

        let mut deleted_events = 0;
        if events_to_delete > 0 {
            sqlx::query("DELETE FROM market_events WHERE timestamp < ?")
                .bind(delete_threshold)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to delete old events: {}", e)))?;
            deleted_events = events_to_delete;
        }

        // Count and delete very old trading signals
        let signals_to_delete = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM trading_signals WHERE timestamp < ?"
        )
        .bind(delete_threshold)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to count old signals: {}", e)))?;

        let mut deleted_signals = 0;
        if signals_to_delete > 0 {
            sqlx::query("DELETE FROM trading_signals WHERE timestamp < ?")
                .bind(delete_threshold)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to delete old signals: {}", e)))?;
            deleted_signals = signals_to_delete;
        }

        if deleted_events > 0 || deleted_signals > 0 {
            info!("🗑️ Light cleanup: deleted {} events, {} signals", deleted_events, deleted_signals);
        }

        Ok(())
    }

    /// Full cleanup with archiving
    async fn run_full_cleanup(&self) -> Result<CleanupStats, DatabaseError> {
        info!("🧹 Running full cleanup and archiving");
        
        let now = Utc::now().timestamp();
        let hot_threshold = now - (self.retention_config.hot_data_days as i64 * 86400);
        let warm_threshold = now - (self.retention_config.warm_data_days as i64 * 86400);
        let cold_threshold = now - (self.retention_config.cold_data_days as i64 * 86400);
        let delete_threshold = now - (self.retention_config.delete_data_days as i64 * 86400);

        // Archive cold data before deletion
        let archived_count = self.archive_cold_data(cold_threshold, delete_threshold).await?;

        // Get current counts
        let hot_count = self.get_record_count_newer_than(hot_threshold).await?;
        let warm_count = self.get_record_count_between(warm_threshold, hot_threshold).await?;

        // Delete ancient data
        let deleted_count = self.delete_ancient_data(delete_threshold).await?;

        // Run VACUUM to reclaim space
        self.vacuum_database().await?;

        // Calculate disk space freed (approximate)
        let avg_record_size_kb = 2.0; // Estimate 2KB per record
        let disk_freed_mb = (deleted_count as f64 * avg_record_size_kb) / 1024.0;

        let stats = CleanupStats {
            hot_records: hot_count,
            warm_records: warm_count,
            cold_archived: archived_count,
            deleted_records: deleted_count,
            disk_space_freed_mb: disk_freed_mb,
            last_cleanup: Utc::now(),
        };

        info!("✅ Full cleanup completed:");
        info!("   📊 Hot records: {}", stats.hot_records);
        info!("   🔥 Warm records: {}", stats.warm_records);
        info!("   ❄️  Archived records: {}", stats.cold_archived);
        info!("   🗑️  Deleted records: {}", stats.deleted_records);
        info!("   💾 Disk space freed: {:.2} MB", stats.disk_space_freed_mb);

        Ok(stats)
    }

    async fn archive_cold_data(&self, cold_threshold: i64, delete_threshold: i64) -> Result<i64, DatabaseError> {
        // Get records to archive (between cold and delete thresholds)
        let records_to_archive = sqlx::query(
            "SELECT event_id, event_type, timestamp, slot, data, processed_at, created_at 
             FROM market_events 
             WHERE timestamp < ? AND timestamp >= ?"
        )
        .bind(cold_threshold)
        .bind(delete_threshold)
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to fetch archive data: {}", e)))?;

        if records_to_archive.is_empty() {
            return Ok(0);
        }

        // Create archive file
        let archive_filename = format!("badger_archive_{}.db", Utc::now().format("%Y%m%d_%H%M%S"));
        let archive_path = self.archive_path.join(archive_filename);

        // Create archive database with compressed data
        let archive_connection = sqlx::SqlitePool::connect(&format!("sqlite:{}", archive_path.display())).await
            .map_err(|e| DatabaseError::ConnectionError(format!("Failed to create archive: {}", e)))?;

        // Create archive schema
        sqlx::query(r#"
            CREATE TABLE archived_market_events (
                event_id TEXT PRIMARY KEY,
                event_type TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                slot INTEGER,
                data TEXT NOT NULL,
                processed_at INTEGER NOT NULL,
                created_at INTEGER DEFAULT (strftime('%s', 'now'))
            )
        "#)
        .execute(&archive_connection)
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to create archive schema: {}", e)))?;

        // Insert records into archive
        let mut archived_count = 0;
        for record in &records_to_archive {
            sqlx::query(
                "INSERT INTO archived_market_events 
                 (event_id, event_type, timestamp, slot, data, processed_at, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(record.get::<String, _>("event_id"))
            .bind(record.get::<String, _>("event_type"))
            .bind(record.get::<i64, _>("timestamp"))
            .bind(record.get::<Option<i64>, _>("slot"))
            .bind(record.get::<String, _>("data"))
            .bind(record.get::<i64, _>("processed_at"))
            .bind(record.get::<Option<i64>, _>("created_at"))
            .execute(&archive_connection)
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to archive record: {}", e)))?;
            
            archived_count += 1;
        }

        archive_connection.close().await;

        info!("📦 Archived {} records to {}", archived_count, archive_path.display());
        Ok(archived_count)
    }

    async fn get_record_count_newer_than(&self, threshold: i64) -> Result<i64, DatabaseError> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM market_events WHERE timestamp >= ?
             UNION ALL
             SELECT COUNT(*) FROM trading_signals WHERE timestamp >= ?"
        )
        .bind(threshold)
        .bind(threshold)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to count hot records: {}", e)))?;
        
        Ok(count)
    }

    async fn get_record_count_between(&self, start_threshold: i64, end_threshold: i64) -> Result<i64, DatabaseError> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM market_events WHERE timestamp >= ? AND timestamp < ?
             UNION ALL
             SELECT COUNT(*) FROM trading_signals WHERE timestamp >= ? AND timestamp < ?"
        )
        .bind(start_threshold)
        .bind(end_threshold)
        .bind(start_threshold)
        .bind(end_threshold)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to count warm records: {}", e)))?;
        
        Ok(count)
    }

    async fn delete_ancient_data(&self, delete_threshold: i64) -> Result<i64, DatabaseError> {
        let mut total_deleted = 0;

        // Count and delete ancient market events
        let events_to_delete = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM market_events WHERE timestamp < ?"
        )
        .bind(delete_threshold)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to count ancient events: {}", e)))?;

        if events_to_delete > 0 {
            sqlx::query("DELETE FROM market_events WHERE timestamp < ?")
                .bind(delete_threshold)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to delete ancient events: {}", e)))?;
            
            total_deleted += events_to_delete;
            debug!("🗑️ Deleted {} ancient market events", events_to_delete);
        }

        // Count and delete ancient trading signals
        let signals_to_delete = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM trading_signals WHERE timestamp < ?"
        )
        .bind(delete_threshold)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to count ancient signals: {}", e)))?;

        if signals_to_delete > 0 {
            sqlx::query("DELETE FROM trading_signals WHERE timestamp < ?")
                .bind(delete_threshold)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to delete ancient signals: {}", e)))?;
            
            total_deleted += signals_to_delete;
            debug!("🗑️ Deleted {} ancient trading signals", signals_to_delete);
        }

        // Count and delete ancient wallet scores
        let wallets_to_delete = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM wallet_scores WHERE last_updated < ?"
        )
        .bind(delete_threshold)
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to count ancient wallet scores: {}", e)))?;

        if wallets_to_delete > 0 {
            sqlx::query("DELETE FROM wallet_scores WHERE last_updated < ?")
                .bind(delete_threshold)
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to delete ancient wallet scores: {}", e)))?;
            
            total_deleted += wallets_to_delete;
            debug!("🗑️ Deleted {} ancient wallet scores", wallets_to_delete);
        }

        Ok(total_deleted)
    }

    async fn vacuum_database(&self) -> Result<(), DatabaseError> {
        debug!("🧹 Running VACUUM to reclaim disk space");
        
        sqlx::query("VACUUM")
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("VACUUM failed: {}", e)))?;

        debug!("✅ VACUUM completed");
        Ok(())
    }

    /// Get cleanup statistics without running cleanup
    pub async fn get_stats(&self) -> Result<CleanupStats, DatabaseError> {
        let now = Utc::now().timestamp();
        let hot_threshold = now - (self.retention_config.hot_data_days as i64 * 86400);
        let warm_threshold = now - (self.retention_config.warm_data_days as i64 * 86400);
        
        let hot_count = self.get_record_count_newer_than(hot_threshold).await?;
        let warm_count = self.get_record_count_between(warm_threshold, hot_threshold).await?;

        Ok(CleanupStats {
            hot_records: hot_count,
            warm_records: warm_count,
            cold_archived: 0,
            deleted_records: 0,
            disk_space_freed_mb: 0.0,
            last_cleanup: Utc::now(),
        })
    }

    /// Manual cleanup trigger
    pub async fn trigger_cleanup(&self) -> Result<CleanupStats, DatabaseError> {
        info!("🧹 Manual cleanup triggered");
        self.run_full_cleanup().await
    }
}

/// Backup service for database recovery
pub struct BackupService {
    db: Arc<BadgerDatabase>,
    backup_path: PathBuf,
    backup_interval: Duration,
}

impl BackupService {
    pub fn new(db: Arc<BadgerDatabase>, backup_path: PathBuf) -> Self {
        Self {
            db,
            backup_path,
            backup_interval: Duration::from_secs(3600 * 6), // Every 6 hours
        }
    }

    /// Start backup service
    pub async fn run(self) -> Result<(), DatabaseError> {
        info!("💾 Backup Service starting");
        
        // Ensure backup directory exists
        if let Err(e) = tokio::fs::create_dir_all(&self.backup_path).await {
            error!("Failed to create backup directory: {}", e);
            return Err(DatabaseError::InitializationError(
                format!("Could not create backup directory: {}", e)
            ));
        }

        let mut backup_timer = interval(self.backup_interval);

        loop {
            backup_timer.tick().await;
            
            if let Err(e) = self.create_backup().await {
                error!("Backup creation failed: {}", e);
            }
        }
    }

    async fn create_backup(&self) -> Result<(), DatabaseError> {
        let backup_filename = format!("badger_backup_{}.db", Utc::now().format("%Y%m%d_%H%M%S"));
        let backup_path = self.backup_path.join(backup_filename);

        info!("💾 Creating backup: {}", backup_path.display());

        // Use SQLite's backup API through VACUUM INTO
        sqlx::query("VACUUM INTO ?")
            .bind(backup_path.to_string_lossy().as_ref())
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Backup failed: {}", e)))?;

        info!("✅ Backup created successfully: {}", backup_path.display());
        Ok(())
    }
}