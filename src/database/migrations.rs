/// Database Migration System
/// 
/// This module handles database schema initialization and migrations.
/// It ensures the database is always in the correct state for the application.

use super::{BadgerDatabase, DatabaseError};
use sqlx::Row;
use std::fs;
use std::path::Path;
use tracing::{info, error, debug};

/// Migration system for database schema management
pub struct MigrationRunner {
    /// Database connection
    db: std::sync::Arc<BadgerDatabase>,
}

/// Migration file information
#[derive(Debug, Clone)]
pub struct Migration {
    /// Migration version (e.g., 001)
    pub version: String,
    /// Migration name/description
    pub name: String,
    /// Full file path
    pub file_path: String,
    /// SQL content
    pub sql_content: String,
}

impl MigrationRunner {
    /// Create new migration runner
    pub fn new(db: std::sync::Arc<BadgerDatabase>) -> Self {
        Self { db }
    }

    /// Initialize migration system and run pending migrations
    pub async fn run_migrations(&self) -> Result<(), DatabaseError> {
        info!("🔄 Starting database migration system");

        // Create migrations tracking table
        self.create_migrations_table().await?;

        // Load migration files from filesystem
        let migrations = self.load_migration_files().await?;
        info!("📁 Found {} migration files", migrations.len());

        // Get already applied migrations
        let applied_migrations = self.get_applied_migrations().await?;
        info!("✅ {} migrations already applied", applied_migrations.len());

        // Run pending migrations
        let mut applied_count = 0;
        for migration in &migrations {
            if !applied_migrations.contains(&migration.version) {
                info!("🔄 Applying migration: {} - {}", migration.version, migration.name);
                self.apply_migration(migration).await?;
                applied_count += 1;
            }
        }

        if applied_count > 0 {
            info!("✅ Applied {} new migrations successfully", applied_count);
        } else {
            info!("✅ Database schema is up to date");
        }

        // Verify database integrity
        self.verify_schema().await?;

        Ok(())
    }

    /// Create migrations tracking table
    async fn create_migrations_table(&self) -> Result<(), DatabaseError> {
        let sql = r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                checksum TEXT NOT NULL
            )
        "#;

        sqlx::query(sql)
            .execute(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to create migrations table: {}", e)))?;

        debug!("📊 Migration tracking table created/verified");
        Ok(())
    }

    /// Load migration files from the migrations directory
    async fn load_migration_files(&self) -> Result<Vec<Migration>, DatabaseError> {
        let migrations_dir = Path::new("migrations");
        
        if !migrations_dir.exists() {
            return Err(DatabaseError::QueryError(
                "Migrations directory not found. Please create 'migrations/' directory.".to_string()
            ));
        }

        let mut migrations = Vec::new();

        // Read migration files
        let entries = fs::read_dir(migrations_dir)
            .map_err(|e| DatabaseError::QueryError(format!("Failed to read migrations directory: {}", e)))?;

        for entry in entries {
            let entry = entry.map_err(|e| DatabaseError::QueryError(format!("Failed to read directory entry: {}", e)))?;
            let path = entry.path();

            // Only process .sql files
            if path.extension().and_then(|s| s.to_str()) == Some("sql") {
                if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                    // Parse filename format: 001_migration_name.sql
                    if let Some((version, name)) = self.parse_migration_filename(filename) {
                        let sql_content = fs::read_to_string(&path)
                            .map_err(|e| DatabaseError::QueryError(format!("Failed to read migration file {}: {}", filename, e)))?;

                        migrations.push(Migration {
                            version,
                            name,
                            file_path: path.to_string_lossy().to_string(),
                            sql_content,
                        });
                    } else {
                        debug!("⚠️  Skipping migration file with invalid format: {}", filename);
                    }
                }
            }
        }

        // Sort migrations by version
        migrations.sort_by(|a, b| a.version.cmp(&b.version));

        Ok(migrations)
    }

    /// Parse SQL statements from migration content - simple and reliable approach
    fn parse_sql_statements(&self, content: &str) -> Vec<String> {
        // Remove comments and split by semicolons more reliably
        let mut cleaned_content = String::new();
        
        for line in content.lines() {
            let line = line.trim();
            
            // Skip empty lines and comment-only lines
            if line.is_empty() || line.starts_with("--") {
                continue;
            }
            
            // Remove inline comments but keep the rest of the line
            let cleaned_line = if let Some(comment_pos) = line.find("--") {
                line[..comment_pos].trim()
            } else {
                line
            };
            
            if !cleaned_line.is_empty() {
                if !cleaned_content.is_empty() {
                    cleaned_content.push(' ');
                }
                cleaned_content.push_str(cleaned_line);
            }
        }
        
        // Split by semicolons and clean up
        let statements: Vec<String> = cleaned_content
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        
        info!("✅ Parsed {} SQL statements from migration", statements.len());
        
        // Debug first statement to verify parsing
        if let Some(first_stmt) = statements.first() {
            info!("📋 First statement: {} chars", first_stmt.len());
            debug!("First 150 chars: {}", &first_stmt[..std::cmp::min(150, first_stmt.len())]);
        }
        
        statements
    }

    /// Parse migration filename to extract version and name
    fn parse_migration_filename(&self, filename: &str) -> Option<(String, String)> {
        // Expected format: 001_initial_schema.sql
        if let Some(name_without_ext) = filename.strip_suffix(".sql") {
            if let Some(underscore_pos) = name_without_ext.find('_') {
                let version = name_without_ext[..underscore_pos].to_string();
                let name = name_without_ext[underscore_pos + 1..].replace('_', " ");
                return Some((version, name));
            }
        }
        None
    }

    /// Get list of already applied migrations
    async fn get_applied_migrations(&self) -> Result<Vec<String>, DatabaseError> {
        let rows = sqlx::query("SELECT version FROM schema_migrations ORDER BY version")
            .fetch_all(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to get applied migrations: {}", e)))?;

        let versions = rows
            .into_iter()
            .map(|row| row.try_get::<String, _>("version").unwrap_or_default())
            .collect();

        Ok(versions)
    }

    /// Apply a single migration
    async fn apply_migration(&self, migration: &Migration) -> Result<(), DatabaseError> {
        // Calculate checksum for integrity verification
        let checksum = self.calculate_checksum(&migration.sql_content);

        // Execute migration SQL directly (SQLite DDL auto-commits)
        let statements = self.parse_sql_statements(&migration.sql_content);
        for (i, statement) in statements.iter().enumerate() {
            if !statement.is_empty() {
                debug!("Executing migration statement {}/{}: {} chars", i + 1, statements.len(), statement.len());
                
                // Extra debug for critical statements
                if (i + 1) == 11 || (i + 1) == 45 {
                    info!("=== DEBUG STATEMENT {} ===", i + 1);
                    info!("SQL: {}", statement);
                    info!("========================");
                }
                
                sqlx::query(&statement)
                    .execute(self.db.get_pool())
                    .await
                    .map_err(|e| DatabaseError::QueryError(
                        format!("Failed to execute migration {} statement {}: {}", migration.version, i + 1, e)
                    ))?;
                debug!("✅ Migration statement {}/{} executed successfully", i + 1, statements.len());
                
                // After statement 11, check if table was created properly
                if (i + 1) == 11 {
                    info!("=== CHECKING wallet_scores TABLE AFTER CREATION ===");
                    let schema_check = sqlx::query("SELECT sql FROM sqlite_master WHERE name = 'wallet_scores'")
                        .fetch_optional(self.db.get_pool())
                        .await;
                    
                    match schema_check {
                        Ok(Some(row)) => {
                            let sql: String = row.try_get("sql").unwrap_or_default();
                            info!("wallet_scores schema: {}", sql);
                            if sql.contains("win_rate") {
                                info!("✅ win_rate column confirmed in schema");
                            } else {
                                error!("❌ win_rate column NOT found in schema!");
                            }
                        }
                        Ok(None) => error!("❌ wallet_scores table not found after creation!"),
                        Err(e) => error!("❌ Error checking schema: {}", e),
                    }
                }
            }
        }

        // Record migration as applied (in separate transaction)
        sqlx::query(
            "INSERT INTO schema_migrations (version, name, checksum) VALUES (?, ?, ?)"
        )
        .bind(&migration.version)
        .bind(&migration.name)
        .bind(&checksum)
        .execute(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to record migration: {}", e)))?;

        info!("✅ Migration {} applied successfully", migration.version);
        Ok(())
    }

    /// Calculate simple checksum for migration integrity
    fn calculate_checksum(&self, content: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Verify database schema integrity after migrations
    async fn verify_schema(&self) -> Result<(), DatabaseError> {
        debug!("🔍 Verifying database schema integrity");

        // Check that all required tables exist
        let required_tables = vec![
            "market_events", "trading_signals", "token_launches",
            "insider_wallets", "wallet_trade_analysis", "wallet_discovery_log",
            "copy_trading_signals", "copy_trading_performance",
            "positions", "position_updates", "wallet_scores",
            "trading_sessions", "performance_snapshots", "session_stats",
        ];

        for table in &required_tables {
            let count = sqlx::query_scalar::<_, i64>(&format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='{}'", table
            ))
            .fetch_one(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to verify table {}: {}", table, e)))?;

            if count == 0 {
                return Err(DatabaseError::QueryError(format!("Required table '{}' is missing", table)));
            }
        }

        // Verify critical indexes exist
        let critical_indexes = vec![
            "idx_market_events_timestamp",
            "idx_insider_wallets_confidence_score",
            "idx_copy_trading_performance_insider",
            "idx_positions_status",
        ];

        for index in &critical_indexes {
            let count = sqlx::query_scalar::<_, i64>(&format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='{}'", index
            ))
            .fetch_one(self.db.get_pool())
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to verify index {}: {}", index, e)))?;

            if count == 0 {
                debug!("⚠️  Critical index '{}' is missing", index);
            }
        }

        info!("✅ Database schema verification completed");
        Ok(())
    }

    /// Get migration status information
    pub async fn get_migration_status(&self) -> Result<MigrationStatus, DatabaseError> {
        let applied_migrations = self.get_applied_migrations().await?;
        let available_migrations = self.load_migration_files().await?;

        let pending_migrations: Vec<String> = available_migrations
            .iter()
            .filter(|m| !applied_migrations.contains(&m.version))
            .map(|m| m.version.clone())
            .collect();

        Ok(MigrationStatus {
            applied_count: applied_migrations.len(),
            pending_count: pending_migrations.len(),
            total_available: available_migrations.len(),
            latest_applied: applied_migrations.last().cloned(),
            pending_versions: pending_migrations,
        })
    }

    /// Reset database (FOR DEVELOPMENT ONLY - drops all data)
    #[cfg(feature = "reset-db")]
    pub async fn reset_database(&self) -> Result<(), DatabaseError> {
        error!("🚨 DANGER: Resetting entire database - all data will be lost!");

        // Get all table names
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
        )
        .fetch_all(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Failed to get table list: {}", e)))?;

        // Drop all tables
        for table in tables {
            sqlx::query(&format!("DROP TABLE IF EXISTS {}", table))
                .execute(self.db.get_pool())
                .await
                .map_err(|e| DatabaseError::QueryError(format!("Failed to drop table {}: {}", table, e)))?;
            
            debug!("🗑️  Dropped table: {}", table);
        }

        info!("🗑️  Database reset completed - ready for fresh migrations");
        Ok(())
    }
}

/// Migration system status
#[derive(Debug, Clone)]
pub struct MigrationStatus {
    pub applied_count: usize,
    pub pending_count: usize,
    pub total_available: usize,
    pub latest_applied: Option<String>,
    pub pending_versions: Vec<String>,
}

impl MigrationStatus {
    /// Check if migrations are up to date
    pub fn is_up_to_date(&self) -> bool {
        self.pending_count == 0
    }

    /// Get status summary string
    pub fn summary(&self) -> String {
        if self.is_up_to_date() {
            format!("✅ Database is up to date ({} migrations applied)", self.applied_count)
        } else {
            format!("🔄 {} pending migrations (applied: {}, total: {})", 
                   self.pending_count, self.applied_count, self.total_available)
        }
    }
}