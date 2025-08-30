use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex};
use tracing::{info, warn, error, debug, instrument};

use crate::core::{MarketEvent, TradingSignal};
use crate::transport::{EnhancedTransportBus, ServiceRegistry, WalletEvent, SystemAlert};

pub mod models;
pub mod services;
pub mod batch;
pub mod validation;
pub mod cleanup;

pub use models::*;
pub use services::*;
pub use batch::*;
pub use validation::*;
pub use cleanup::*;

/// Enhanced database manager for Milestone 2 with real-time persistence
pub struct DatabaseManager {
    persistence_service: Option<PersistenceService>,
    analytics_service: Option<AnalyticsService>,
    wallet_tracker_service: Option<WalletTrackerService>,
    query_service: Option<QueryService>,
    enhanced_persistence: Option<EnhancedPersistenceService>,
    validation_service: Option<ValidationService>,
    cleanup_service: Option<CleanupService>,
}

impl DatabaseManager {
    pub fn new() -> Self {
        Self {
            persistence_service: None,
            analytics_service: None,
            wallet_tracker_service: None,
            query_service: None,
            enhanced_persistence: None,
            validation_service: None,
            cleanup_service: None,
        }
    }

    pub async fn initialize(
        &mut self,
        transport_bus: Arc<EnhancedTransportBus>,
        service_registry: Arc<ServiceRegistry>,
    ) -> Result<(), DatabaseError> {
        info!("🗄️ Initializing Database Manager for Phase 3");

        // Initialize SQLite database with enhanced configuration
        let db = Arc::new(BadgerDatabase::new("sqlite:data/badger.db").await?);

        // Create enhanced persistence service for high-performance batch processing
        self.enhanced_persistence = Some(EnhancedPersistenceService::new(db.clone()));

        // Create validation service
        self.validation_service = Some(ValidationService::new(true)); // Strict mode

        // Create cleanup service with default retention
        let cleanup_config = cleanup::RetentionConfig::default();
        self.cleanup_service = Some(CleanupService::new(
            db.clone(),
            std::path::PathBuf::from("data/archives"),
            Some(cleanup_config),
        ));

        // Keep original services for compatibility
        self.persistence_service = Some(PersistenceService::new(
            db.clone(),
            transport_bus.clone(),
            service_registry.clone(),
        ).await?);

        self.analytics_service = Some(AnalyticsService::new(
            db.clone(),
            transport_bus.clone(),
            service_registry.clone(),
        ).await?);

        self.wallet_tracker_service = Some(WalletTrackerService::new(
            db.clone(),
            transport_bus.clone(),
            service_registry.clone(),
        ).await?);

        self.query_service = Some(QueryService::new(db).await?);

        info!("✅ Database Manager initialized successfully");
        Ok(())
    }

    pub async fn start_all_services(&mut self) -> Result<Vec<tokio::task::JoinHandle<Result<(), DatabaseError>>>, DatabaseError> {
        let mut handles = Vec::new();

        // Start enhanced persistence service (primary batch processor)
        if let Some(enhanced_persistence) = self.enhanced_persistence.take() {
            let handle = tokio::spawn(async move {
                enhanced_persistence.run().await
            });
            handles.push(handle);
        }

        // Start cleanup service
        if let Some(cleanup_service) = self.cleanup_service.take() {
            let handle = tokio::spawn(async move {
                cleanup_service.run().await
            });
            handles.push(handle);
        }

        // Keep original services running for compatibility
        if let Some(persistence) = self.persistence_service.take() {
            let handle = tokio::spawn(async move {
                persistence.run().await
            });
            handles.push(handle);
        }

        if let Some(analytics) = self.analytics_service.take() {
            let handle = tokio::spawn(async move {
                analytics.run().await
            });
            handles.push(handle);
        }

        if let Some(wallet_tracker) = self.wallet_tracker_service.take() {
            let handle = tokio::spawn(async move {
                wallet_tracker.run().await
            });
            handles.push(handle);
        }

        info!("🚀 Enhanced database services started:");
        info!("   ⚡ Batch Processing: Active");
        info!("   🔍 Data Validation: Strict Mode");
        info!("   🧹 Cleanup Service: 7d/30d/90d/365d retention");
        info!("   📊 Analytics Engine: Active");
        info!("   🕵️ Wallet Tracker: Active");
        
        Ok(handles)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum DatabaseError {
    #[error("Database connection failed: {0}")]
    ConnectionError(String),
    
    #[error("Query execution failed: {0}")]
    QueryError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Service initialization failed: {0}")]
    InitializationError(String),
    
    #[error("SQLite error: {0}")]
    SqlxError(#[from] sqlx::Error),
    
    #[error("Migration error: {0}")]
    MigrationError(String),
}