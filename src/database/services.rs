use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{info, warn, error, debug, instrument};

use crate::core::{MarketEvent, TradingSignal};
use crate::transport::{EnhancedTransportBus, ServiceRegistry, WalletEvent, SystemAlert};
use crate::transport::{ServiceInfo, ServiceType, ServiceCapability, ServiceStatus, EventType, SubscriptionInfo};

use super::models::{BadgerDatabase, AnalyticsData, WalletScore};
use super::DatabaseError;

/// PersistenceService - Main database coordinator
/// 
/// Subscribes to transport bus events and stores them for persistence
pub struct PersistenceService {
    db: Arc<BadgerDatabase>,
    transport_bus: Arc<EnhancedTransportBus>,
    service_registry: Arc<ServiceRegistry>,
    batch_size: usize,
    batch_timeout: Duration,
}

impl PersistenceService {
    pub async fn new(
        db: Arc<BadgerDatabase>,
        transport_bus: Arc<EnhancedTransportBus>,
        service_registry: Arc<ServiceRegistry>,
    ) -> Result<Self, DatabaseError> {
        Ok(Self {
            db,
            transport_bus,
            service_registry,
            batch_size: 100, // Smaller batch size for simplicity
            batch_timeout: Duration::from_secs(10),
        })
    }

    #[instrument(skip(self))]
    pub async fn run(self) -> Result<(), DatabaseError> {
        info!("🗄️ PersistenceService starting - database storage active");

        // Register with service registry
        let service_info = ServiceInfo {
            id: "persistence-service-001".to_string(),
            name: "Database Persistence Service".to_string(),
            service_type: ServiceType::Storage,
            version: "1.0.0".to_string(),
            capabilities: vec![
                ServiceCapability::MarketEventConsumer,
                ServiceCapability::TradingSignalConsumer,
            ],
            subscriptions: vec![
                SubscriptionInfo {
                    event_type: EventType::MarketEvent,
                    filters: vec![],
                    subscribed_at: chrono::Utc::now(),
                },
                SubscriptionInfo {
                    event_type: EventType::TradingSignal,
                    filters: vec![],
                    subscribed_at: chrono::Utc::now(),
                },
            ],
            status: ServiceStatus::Starting,
            registered_at: chrono::Utc::now(),
            last_heartbeat: chrono::Utc::now(),
            metadata: std::collections::HashMap::new(),
        };

        if let Err(e) = self.service_registry.register_service(service_info).await {
            warn!("Failed to register persistence service: {}", e);
        }

        // Subscribe to transport bus events
        let mut market_events = self.transport_bus.subscribe_market_events().await;
        let mut trading_signals = self.transport_bus.subscribe_trading_signals().await;
        let mut wallet_events = self.transport_bus.subscribe_wallet_events().await;

        // Update service status to healthy
        if let Err(e) = self.service_registry.update_service_status(
            "persistence-service-001", 
            ServiceStatus::Healthy
        ).await {
            warn!("Failed to update persistence service status: {}", e);
        }

        // Persistence timer for disk writes
        let mut persist_timer = tokio::time::interval(Duration::from_secs(60));
        let mut stats_timer = tokio::time::interval(Duration::from_secs(30));

        info!("📦 PersistenceService subscriptions active - storing events");

        loop {
            tokio::select! {
                // Process market events
                Ok(market_event) = market_events.recv() => {
                    if let Err(e) = self.db.store_market_event(market_event).await {
                        warn!("Failed to store market event: {}", e);
                    } else {
                        debug!("✅ Market event stored");
                    }
                }

                // Process trading signals
                Ok(trading_signal) = trading_signals.recv() => {
                    if let Err(e) = self.db.store_trading_signal(trading_signal).await {
                        warn!("Failed to store trading signal: {}", e);
                    } else {
                        debug!("✅ Trading signal stored");
                    }
                }

                // Process wallet events (for now just log)
                Ok(wallet_event) = wallet_events.recv() => {
                    debug!("👛 Wallet event received (not yet stored): {:?}", std::mem::discriminant(&wallet_event));
                }

                // Periodic health check
                _ = persist_timer.tick() => {
                    // SQLite auto-persists, just log health
                    debug!("💾 Database health check - SQLite auto-persisting");
                }

                // Periodic stats logging
                _ = stats_timer.tick() => {
                    match self.db.get_session_stats().await {
                        Ok(stats) => {
                            info!("📊 Database Stats: {} events, {} signals stored", 
                                stats.total_market_events, stats.total_trading_signals);
                        }
                        Err(e) => {
                            warn!("Failed to get session stats: {}", e);
                        }
                    }
                }
            }
        }
    }
}

/// AnalyticsService - Real-time analytics engine
pub struct AnalyticsService {
    db: Arc<BadgerDatabase>,
    transport_bus: Arc<EnhancedTransportBus>,
    service_registry: Arc<ServiceRegistry>,
}

impl AnalyticsService {
    pub async fn new(
        db: Arc<BadgerDatabase>,
        transport_bus: Arc<EnhancedTransportBus>,
        service_registry: Arc<ServiceRegistry>,
    ) -> Result<Self, DatabaseError> {
        Ok(Self {
            db,
            transport_bus,
            service_registry,
        })
    }

    #[instrument(skip(self))]
    pub async fn run(self) -> Result<(), DatabaseError> {
        info!("📊 AnalyticsService starting - real-time analytics engine active");

        // Register with service registry
        let service_info = ServiceInfo {
            id: "analytics-service-001".to_string(),
            name: "Real-time Analytics Service".to_string(),
            service_type: ServiceType::Analytics,
            version: "1.0.0".to_string(),
            capabilities: vec![
                ServiceCapability::MarketEventConsumer,
                ServiceCapability::TradingSignalConsumer,
            ],
            subscriptions: vec![
                SubscriptionInfo {
                    event_type: EventType::MarketEvent,
                    filters: vec![],
                    subscribed_at: chrono::Utc::now(),
                },
            ],
            status: ServiceStatus::Starting,
            registered_at: chrono::Utc::now(),
            last_heartbeat: chrono::Utc::now(),
            metadata: std::collections::HashMap::new(),
        };

        if let Err(e) = self.service_registry.register_service(service_info).await {
            warn!("Failed to register analytics service: {}", e);
        }

        // Subscribe to events for analytics
        let mut market_events = self.transport_bus.subscribe_market_events().await;
        let mut trading_signals = self.transport_bus.subscribe_trading_signals().await;

        // Analytics calculation timer
        let mut analytics_timer = tokio::time::interval(Duration::from_secs(30));
        let mut report_timer = tokio::time::interval(Duration::from_secs(60));

        // Update service status
        if let Err(e) = self.service_registry.update_service_status(
            "analytics-service-001", 
            ServiceStatus::Healthy
        ).await {
            warn!("Failed to update analytics service status: {}", e);
        }

        info!("📈 AnalyticsService subscriptions active - calculating metrics");

        loop {
            tokio::select! {
                // Process market events for analytics
                Ok(market_event) = market_events.recv() => {
                    self.process_market_event_for_analytics(&market_event).await;
                }

                // Process trading signals for analytics
                Ok(trading_signal) = trading_signals.recv() => {
                    self.process_trading_signal_for_analytics(&trading_signal).await;
                }

                // Calculate analytics periodically
                _ = analytics_timer.tick() => {
                    self.calculate_analytics().await;
                }

                // Report analytics periodically
                _ = report_timer.tick() => {
                    self.report_analytics().await;
                }
            }
        }
    }

    async fn process_market_event_for_analytics(&self, _event: &MarketEvent) {
        // For now, just increment counters
        debug!("📊 Processing market event for analytics");
    }

    async fn process_trading_signal_for_analytics(&self, _signal: &TradingSignal) {
        // For now, just increment counters
        debug!("📊 Processing trading signal for analytics");
    }

    async fn calculate_analytics(&self) {
        // Skip analytics calculation until we have real trading data with P&L
        debug!("📊 Analytics calculation skipped - no real trading data yet");
    }

    async fn report_analytics(&self) {
        // Only report real database statistics, no mock analytics
        match self.db.get_session_stats().await {
            Ok(stats) => {
                info!("📊 DATABASE STATISTICS:");
                info!("   🗄️ Market Events Stored: {}", stats.total_market_events);
                info!("   📶 Trading Signals: {}", stats.total_trading_signals);
                info!("   ⏱️ Session Runtime: {:.1}m", stats.uptime_seconds as f64 / 60.0);
                info!("   💾 Database Size: {} records", stats.total_market_events + stats.total_trading_signals);
            }
            Err(e) => {
                warn!("Failed to get session stats: {}", e);
            }
        }
    }
}

/// WalletTrackerService - Advanced wallet monitoring
pub struct WalletTrackerService {
    db: Arc<BadgerDatabase>,
    transport_bus: Arc<EnhancedTransportBus>,
    service_registry: Arc<ServiceRegistry>,
}

impl WalletTrackerService {
    pub async fn new(
        db: Arc<BadgerDatabase>,
        transport_bus: Arc<EnhancedTransportBus>,
        service_registry: Arc<ServiceRegistry>,
    ) -> Result<Self, DatabaseError> {
        Ok(Self {
            db,
            transport_bus,
            service_registry,
        })
    }

    #[instrument(skip(self))]
    pub async fn run(self) -> Result<(), DatabaseError> {
        info!("🕵️ WalletTrackerService starting - wallet intelligence active");

        // Register with service registry
        let service_info = ServiceInfo {
            id: "wallet-tracker-service-001".to_string(),
            name: "Wallet Intelligence Service".to_string(),
            service_type: ServiceType::Analytics,
            version: "1.0.0".to_string(),
            capabilities: vec![
                ServiceCapability::WalletEventProducer,
                ServiceCapability::MarketEventConsumer,
            ],
            subscriptions: vec![
                SubscriptionInfo {
                    event_type: EventType::MarketEvent,
                    filters: vec![],
                    subscribed_at: chrono::Utc::now(),
                },
            ],
            status: ServiceStatus::Starting,
            registered_at: chrono::Utc::now(),
            last_heartbeat: chrono::Utc::now(),
            metadata: std::collections::HashMap::new(),
        };

        if let Err(e) = self.service_registry.register_service(service_info).await {
            warn!("Failed to register wallet tracker service: {}", e);
        }

        // Subscribe to events
        let mut market_events = self.transport_bus.subscribe_market_events().await;

        // Wallet scoring timer
        let mut scoring_timer = tokio::time::interval(Duration::from_secs(45));
        let mut report_timer = tokio::time::interval(Duration::from_secs(120));

        // Update service status
        if let Err(e) = self.service_registry.update_service_status(
            "wallet-tracker-service-001", 
            ServiceStatus::Healthy
        ).await {
            warn!("Failed to update wallet tracker service status: {}", e);
        }

        info!("👁️ WalletTrackerService subscriptions active - tracking wallets");

        loop {
            tokio::select! {
                // Process market events for wallet tracking
                Ok(market_event) = market_events.recv() => {
                    self.process_market_event_for_wallets(&market_event).await;
                }

                // Update wallet scores periodically
                _ = scoring_timer.tick() => {
                    self.update_wallet_scores().await;
                }

                // Report top wallets periodically
                _ = report_timer.tick() => {
                    self.report_top_wallets().await;
                }
            }
        }
    }

    async fn process_market_event_for_wallets(&self, event: &MarketEvent) {
        // Track wallet addresses without mock scoring - wait for real trading data
        match event {
            MarketEvent::SwapDetected { swap } => {
                debug!("🎯 Wallet tracked: {}", &swap.wallet[..8]);
                // TODO: Implement real wallet scoring based on actual trading performance
            }
            MarketEvent::PoolCreated { creator, .. } => {
                debug!("🔥 Pool creator tracked: {}", &creator[..8]);  
                // TODO: Implement real creator scoring based on pool performance
            }
            _ => {
                // Other events don't currently extract wallet info
            }
        }
    }

    async fn update_wallet_scores(&self) {
        debug!("🧮 Updating wallet intelligence scores");
        // Additional scoring logic would go here
    }

    async fn report_top_wallets(&self) {
        // Skip wallet reporting until we have real scoring data
        debug!("🏆 Wallet reporting skipped - no real wallet scores yet");
    }
}

/// QueryService - High-performance data queries
pub struct QueryService {
    db: Arc<BadgerDatabase>,
}

impl QueryService {
    pub async fn new(db: Arc<BadgerDatabase>) -> Result<Self, DatabaseError> {
        Ok(Self { db })
    }

    pub async fn get_session_summary(&self) -> Result<super::models::SessionStats, super::DatabaseError> {
        self.db.get_session_stats().await
    }

    pub async fn get_analytics_summary(&self) -> Result<super::models::AnalyticsData, super::DatabaseError> {
        self.db.get_analytics_summary().await
    }

    pub async fn get_top_wallets(&self, limit: usize) -> Result<Vec<WalletScore>, super::DatabaseError> {
        self.db.get_top_wallets(limit as i64).await
    }

    pub async fn get_recent_events(&self, limit: usize) -> Result<Vec<super::models::StoredMarketEvent>, super::DatabaseError> {
        self.db.get_recent_market_events(limit as i64).await
    }
}