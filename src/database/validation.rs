use std::collections::HashMap;
use serde_json::Value;
use thiserror::Error;
use tracing::{debug, warn, error};

use crate::core::{MarketEvent, TradingSignal};
use super::DatabaseError;

/// Validation errors
#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Schema validation failed: {0}")]
    SchemaError(String),
    
    #[error("Required field missing: {0}")]
    MissingField(String),
    
    #[error("Invalid field value: {field} = {value}")]
    InvalidValue { field: String, value: String },
    
    #[error("Data consistency check failed: {0}")]
    ConsistencyError(String),
}

/// Validation result
pub type ValidationResult = Result<(), ValidationError>;

/// Trait for event validation
pub trait EventValidator: Send + Sync {
    fn validate(&self, event: &dyn EventData) -> ValidationResult;
    fn get_event_type(&self) -> &'static str;
}

/// Trait for events that can be validated
pub trait EventData {
    fn as_json(&self) -> Result<Value, serde_json::Error>;
    fn get_timestamp(&self) -> i64;
    fn get_event_id(&self) -> String;
}

/// Market event validator
pub struct MarketEventValidator;

impl EventValidator for MarketEventValidator {
    fn validate(&self, event: &dyn EventData) -> ValidationResult {
        let json_data = event.as_json()
            .map_err(|e| ValidationError::SchemaError(format!("JSON serialization failed: {}", e)))?;

        // Validate timestamp is not in the future
        let timestamp = event.get_timestamp();
        let now = chrono::Utc::now().timestamp();
        if timestamp > now + 300 { // Allow 5 minutes tolerance
            return Err(ValidationError::InvalidValue {
                field: "timestamp".to_string(),
                value: timestamp.to_string(),
            });
        }

        // Validate timestamp is not too old (older than 24 hours)
        if timestamp < now - 86400 {
            return Err(ValidationError::InvalidValue {
                field: "timestamp".to_string(),
                value: format!("timestamp too old: {}", timestamp),
            });
        }

        // Validate event ID is not empty
        let event_id = event.get_event_id();
        if event_id.is_empty() {
            return Err(ValidationError::MissingField("event_id".to_string()));
        }

        // Validate JSON structure based on event type
        self.validate_json_structure(&json_data)?;

        debug!("✅ Market event validation passed: {}", event_id);
        Ok(())
    }

    fn get_event_type(&self) -> &'static str {
        "market_event"
    }
}

impl MarketEventValidator {
    fn validate_json_structure(&self, json: &Value) -> ValidationResult {
        // Ensure required fields exist based on event type
        if let Some(event_obj) = json.as_object() {
            // Check for common required fields that should exist in any market event
            for required_field in &["timestamp", "event_type"] {
                if !event_obj.contains_key(*required_field) {
                    return Err(ValidationError::MissingField(required_field.to_string()));
                }
            }
        }

        Ok(())
    }
}

/// Trading signal validator
pub struct TradingSignalValidator;

impl EventValidator for TradingSignalValidator {
    fn validate(&self, event: &dyn EventData) -> ValidationResult {
        let json_data = event.as_json()
            .map_err(|e| ValidationError::SchemaError(format!("JSON serialization failed: {}", e)))?;

        // Validate timestamp
        let timestamp = event.get_timestamp();
        let now = chrono::Utc::now().timestamp();
        if timestamp > now + 300 {
            return Err(ValidationError::InvalidValue {
                field: "timestamp".to_string(),
                value: timestamp.to_string(),
            });
        }

        // Validate signal ID
        let signal_id = event.get_event_id();
        if signal_id.is_empty() {
            return Err(ValidationError::MissingField("signal_id".to_string()));
        }

        // Validate confidence is between 0 and 1
        if let Some(confidence) = json_data.get("confidence").and_then(|c| c.as_f64()) {
            if confidence < 0.0 || confidence > 1.0 {
                return Err(ValidationError::InvalidValue {
                    field: "confidence".to_string(),
                    value: confidence.to_string(),
                });
            }
        } else {
            return Err(ValidationError::MissingField("confidence".to_string()));
        }

        // Validate token mint is not empty
        if let Some(token_mint) = json_data.get("token_mint").and_then(|t| t.as_str()) {
            if token_mint.is_empty() {
                return Err(ValidationError::MissingField("token_mint".to_string()));
            }
        } else {
            return Err(ValidationError::MissingField("token_mint".to_string()));
        }

        debug!("✅ Trading signal validation passed: {}", signal_id);
        Ok(())
    }

    fn get_event_type(&self) -> &'static str {
        "trading_signal"
    }
}

/// Main validation service
pub struct ValidationService {
    validators: HashMap<String, Box<dyn EventValidator>>,
    strict_mode: bool,
}

impl ValidationService {
    pub fn new(strict_mode: bool) -> Self {
        let mut validators: HashMap<String, Box<dyn EventValidator>> = HashMap::new();
        
        validators.insert("market_event".to_string(), Box::new(MarketEventValidator));
        validators.insert("trading_signal".to_string(), Box::new(TradingSignalValidator));

        Self {
            validators,
            strict_mode,
        }
    }

    /// Validate market event
    pub fn validate_market_event(&self, event: &MarketEvent) -> ValidationResult {
        if let Some(validator) = self.validators.get("market_event") {
            validator.validate(event)?;
        } else if self.strict_mode {
            return Err(ValidationError::SchemaError("No validator found for market event".to_string()));
        }
        Ok(())
    }

    /// Validate trading signal
    pub fn validate_trading_signal(&self, signal: &TradingSignal) -> ValidationResult {
        if let Some(validator) = self.validators.get("trading_signal") {
            validator.validate(signal)?;
        } else if self.strict_mode {
            return Err(ValidationError::SchemaError("No validator found for trading signal".to_string()));
        }
        Ok(())
    }

    /// Get validation statistics
    pub fn get_validation_stats(&self) -> ValidationStats {
        ValidationStats {
            total_validators: self.validators.len(),
            strict_mode: self.strict_mode,
            supported_types: self.validators.keys().cloned().collect(),
        }
    }
}

/// Validation statistics
#[derive(Debug, Clone)]
pub struct ValidationStats {
    pub total_validators: usize,
    pub strict_mode: bool,
    pub supported_types: Vec<String>,
}

/// Implement EventData for MarketEvent
impl EventData for MarketEvent {
    fn as_json(&self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    fn get_timestamp(&self) -> i64 {
        self.get_timestamp()
    }

    fn get_event_id(&self) -> String {
        self.get_event_id()
    }
}

/// Implement EventData for TradingSignal
impl EventData for TradingSignal {
    fn as_json(&self) -> Result<Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    fn get_timestamp(&self) -> i64 {
        self.get_timestamp()
    }

    fn get_event_id(&self) -> String {
        self.get_signal_id()
    }
}

/// Data consistency checker
pub struct ConsistencyChecker {
    db: std::sync::Arc<super::BadgerDatabase>,
}

impl ConsistencyChecker {
    pub fn new(db: std::sync::Arc<super::BadgerDatabase>) -> Self {
        Self { db }
    }

    /// Run all consistency checks
    pub async fn run_full_check(&self) -> Result<ConsistencyReport, DatabaseError> {
        let mut checks = Vec::new();

        checks.push(self.check_referential_integrity().await?);
        checks.push(self.check_timestamp_ordering().await?);
        checks.push(self.check_duplicate_events().await?);
        checks.push(self.check_data_completeness().await?);

        let total_checks = checks.len();
        let failed_checks = checks.iter().filter(|c| !c.passed).count();

        Ok(ConsistencyReport {
            checks,
            total_checks,
            failed_checks,
            overall_status: if failed_checks == 0 { "PASSED" } else { "FAILED" }.to_string(),
        })
    }

    async fn check_referential_integrity(&self) -> Result<CheckResult, DatabaseError> {
        let orphaned_signals = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM trading_signals ts 
             LEFT JOIN market_events me ON ts.signal_id LIKE '%' || substr(me.event_id, -8) || '%'
             WHERE ts.signal_id IS NOT NULL AND me.event_id IS NULL"
        )
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Referential integrity check failed: {}", e)))?;

        Ok(CheckResult {
            name: "referential_integrity".to_string(),
            passed: orphaned_signals == 0,
            message: format!("Found {} orphaned trading signals", orphaned_signals),
            details: if orphaned_signals > 0 { 
                Some(format!("Trading signals exist without corresponding market events: {}", orphaned_signals))
            } else { 
                None 
            },
        })
    }

    async fn check_timestamp_ordering(&self) -> Result<CheckResult, DatabaseError> {
        let invalid_timestamps = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM market_events 
             WHERE timestamp > strftime('%s', 'now') + 300 
             OR timestamp < strftime('%s', 'now') - 86400"
        )
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Timestamp check failed: {}", e)))?;

        Ok(CheckResult {
            name: "timestamp_ordering".to_string(),
            passed: invalid_timestamps == 0,
            message: format!("Found {} events with invalid timestamps", invalid_timestamps),
            details: if invalid_timestamps > 0 {
                Some("Events found with timestamps in future or too far in past".to_string())
            } else {
                None
            },
        })
    }

    async fn check_duplicate_events(&self) -> Result<CheckResult, DatabaseError> {
        let duplicates = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM (
                SELECT event_id, COUNT(*) as cnt 
                FROM market_events 
                GROUP BY event_id 
                HAVING cnt > 1
            )"
        )
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Duplicate check failed: {}", e)))?;

        Ok(CheckResult {
            name: "duplicate_events".to_string(),
            passed: duplicates == 0,
            message: format!("Found {} duplicate events", duplicates),
            details: if duplicates > 0 {
                Some("Duplicate event IDs found in database".to_string())
            } else {
                None
            },
        })
    }

    async fn check_data_completeness(&self) -> Result<CheckResult, DatabaseError> {
        let incomplete_records = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM market_events 
             WHERE event_id IS NULL OR event_id = '' 
             OR event_type IS NULL OR event_type = ''
             OR data IS NULL OR data = ''"
        )
        .fetch_one(self.db.get_pool())
        .await
        .map_err(|e| DatabaseError::QueryError(format!("Completeness check failed: {}", e)))?;

        Ok(CheckResult {
            name: "data_completeness".to_string(),
            passed: incomplete_records == 0,
            message: format!("Found {} incomplete records", incomplete_records),
            details: if incomplete_records > 0 {
                Some("Records found with missing required fields".to_string())
            } else {
                None
            },
        })
    }
}

/// Consistency check result
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub details: Option<String>,
}

/// Full consistency report
#[derive(Debug, Clone)]
pub struct ConsistencyReport {
    pub checks: Vec<CheckResult>,
    pub total_checks: usize,
    pub failed_checks: usize,
    pub overall_status: String,
}

impl ConsistencyReport {
    pub fn print_summary(&self) {
        println!("🔍 DATA CONSISTENCY REPORT");
        println!("   Status: {}", self.overall_status);
        println!("   Checks: {}/{} passed", self.total_checks - self.failed_checks, self.total_checks);
        
        for check in &self.checks {
            let status = if check.passed { "✅" } else { "❌" };
            println!("   {} {}: {}", status, check.name, check.message);
            
            if let Some(details) = &check.details {
                println!("      Details: {}", details);
            }
        }
    }
}