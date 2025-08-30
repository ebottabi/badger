use anyhow::{Result, Context};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn, error, instrument};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::{MarketEvent, TradingSignal};
use crate::transport::{EnhancedTransportBus, WalletEvent, SystemAlert, EnhancedTradingSignal};

/// Service registry for managing service communication and event routing
/// 
/// This registry tracks active services, their capabilities, and routes
/// events between them using the enhanced transport bus.
#[derive(Debug)]
pub struct ServiceRegistry {
    services: Arc<RwLock<HashMap<ServiceId, ServiceInfo>>>,
    transport: Arc<EnhancedTransportBus>,
    routing_rules: Arc<RwLock<Vec<RoutingRule>>>,
    statistics: Arc<RwLock<RegistryStatistics>>,
}

/// Unique identifier for services in the registry
pub type ServiceId = String;

/// Information about a registered service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub id: ServiceId,
    pub name: String,
    pub service_type: ServiceType,
    pub version: String,
    pub capabilities: Vec<ServiceCapability>,
    pub subscriptions: Vec<SubscriptionInfo>,
    pub status: ServiceStatus,
    pub registered_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

/// Types of services in the Badger ecosystem
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ServiceType {
    /// WebSocket ingestion service (Phase 1)
    Ingestion,
    /// Token discovery and analysis service (Phase 3)  
    Scout,
    /// Wallet tracking and insider detection (Phase 4)
    Stalker,
    /// Trade execution service (Phase 5)
    Strike,
    /// Database and analytics service (Phase 6)
    Database,
    /// External service (Jupiter, price feeds, etc.)
    External,
    /// Utility service (logging, monitoring, etc.)
    Utility,
}

/// Capabilities that services can provide
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ServiceCapability {
    /// Can produce market events
    MarketEventProducer,
    /// Can consume market events
    MarketEventConsumer,
    /// Can produce trading signals
    TradingSignalProducer,
    /// Can consume trading signals
    TradingSignalConsumer,
    /// Can execute trades
    TradeExecutor,
    /// Can analyze tokens
    TokenAnalyzer,
    /// Can track wallets
    WalletTracker,
    /// Can detect risks
    RiskDetector,
    /// Can provide price data
    PriceProvider,
    /// Can store data
    DataStorage,
}

/// Current status of a service
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ServiceStatus {
    /// Service is starting up
    Starting,
    /// Service is healthy and operational
    Healthy,
    /// Service has warnings but is operational
    Warning,
    /// Service has errors but may still function
    Error,
    /// Service is not responding
    Unresponsive,
    /// Service is shutting down
    Stopping,
    /// Service has stopped
    Stopped,
}

/// Information about service subscriptions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubscriptionInfo {
    pub event_type: EventType,
    pub filters: Vec<EventFilter>,
    pub subscribed_at: DateTime<Utc>,
}

/// Types of events that can be subscribed to
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventType {
    MarketEvent,
    TradingSignal,
    WalletEvent,
    SystemAlert,
}

/// Filters for event subscriptions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventFilter {
    pub field: String,
    pub operator: FilterOperator,
    pub value: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum FilterOperator {
    Equals,
    Contains,
    GreaterThan,
    LessThan,
    In,
    NotIn,
}

/// Rules for routing events between services
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    pub id: String,
    pub name: String,
    pub source_service_type: Option<ServiceType>,
    pub target_service_types: Vec<ServiceType>,
    pub event_type: EventType,
    pub conditions: Vec<RoutingCondition>,
    pub priority: u32,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

/// Conditions for routing rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingCondition {
    pub field: String,
    pub operator: FilterOperator,
    pub value: String,
}

/// Statistics for monitoring registry performance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryStatistics {
    pub total_services: u32,
    pub healthy_services: u32,
    pub events_routed: u64,
    pub failed_routes: u64,
    pub last_updated: DateTime<Utc>,
    pub service_statistics: HashMap<ServiceId, ServiceStatistics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatistics {
    pub events_received: u64,
    pub events_sent: u64,
    pub errors: u64,
    pub last_activity: DateTime<Utc>,
    pub uptime_seconds: u64,
}

impl ServiceRegistry {
    /// Create a new service registry with the given transport bus
    #[instrument]
    pub fn new(transport: Arc<EnhancedTransportBus>) -> Self {
        debug!("Creating new ServiceRegistry");
        
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            transport,
            routing_rules: Arc::new(RwLock::new(Vec::new())),
            statistics: Arc::new(RwLock::new(RegistryStatistics {
                total_services: 0,
                healthy_services: 0,
                events_routed: 0,
                failed_routes: 0,
                last_updated: Utc::now(),
                service_statistics: HashMap::new(),
            })),
        }
    }
    
    /// Register a new service in the registry
    #[instrument(skip(self), fields(service_id = %service_info.id))]
    pub async fn register_service(&self, service_info: ServiceInfo) -> Result<()> {
        debug!("Registering service: {} ({})", service_info.name, service_info.id);
        
        // Validate service info
        if service_info.id.is_empty() {
            return Err(anyhow::anyhow!("Service ID cannot be empty"));
        }
        
        if service_info.name.is_empty() {
            return Err(anyhow::anyhow!("Service name cannot be empty"));
        }
        
        let service_id = service_info.id.clone();
        
        // Add service to registry
        {
            let mut services = self.services.write().await;
            services.insert(service_id.clone(), service_info.clone());
        }
        
        // Update statistics
        {
            let mut stats = self.statistics.write().await;
            stats.total_services = self.services.read().await.len() as u32;
            stats.service_statistics.insert(service_id.clone(), ServiceStatistics {
                events_received: 0,
                events_sent: 0,
                errors: 0,
                last_activity: Utc::now(),
                uptime_seconds: 0,
            });
            stats.last_updated = Utc::now();
        }
        
        // Emit system alert about new service (best effort - no warning if no subscribers yet)
        let alert = SystemAlert::ServiceStartup {
            service: service_info.name,
            version: service_info.version,
        };
        
        if let Err(_e) = self.transport.publish_system_alert(alert).await {
            debug!("Service startup alert not published - no system alert subscribers yet");
        }
        
        debug!("Service registered successfully: {}", service_id);
        Ok(())
    }
    
    /// Unregister a service from the registry
    #[instrument(skip(self))]
    pub async fn unregister_service(&self, service_id: &str, reason: String) -> Result<()> {
        debug!("Unregistering service: {} (reason: {})", service_id, reason);
        
        // Get service info before removal
        let service_info = {
            let services = self.services.read().await;
            services.get(service_id).cloned()
        };
        
        // Remove from registry
        let removed = {
            let mut services = self.services.write().await;
            services.remove(service_id).is_some()
        };
        
        if !removed {
            return Err(anyhow::anyhow!("Service not found: {}", service_id));
        }
        
        // Update statistics
        {
            let mut stats = self.statistics.write().await;
            stats.total_services = self.services.read().await.len() as u32;
            
            if let Some(_service_stats) = stats.service_statistics.remove(service_id) {
                let uptime = (Utc::now() - service_info.as_ref().map(|s| s.registered_at).unwrap_or(Utc::now()))
                    .num_seconds() as u64;
                    
                // Emit service shutdown alert
                if let Some(info) = service_info {
                    let alert = SystemAlert::ServiceShutdown {
                        service: info.name,
                        reason: reason.clone(),
                        uptime_seconds: uptime,
                    };
                    
                    if let Err(e) = self.transport.publish_system_alert(alert).await {
                        warn!("Failed to publish service shutdown alert: {}", e);
                    }
                }
            }
            
            stats.last_updated = Utc::now();
        }
        
        debug!("Service unregistered successfully: {}", service_id);
        Ok(())
    }
    
    /// Update service status (heartbeat)
    #[instrument(skip(self))]
    pub async fn update_service_status(&self, service_id: &str, status: ServiceStatus) -> Result<()> {
        let (old_status, service_name) = {
            let mut services = self.services.write().await;
            
            if let Some(service) = services.get_mut(service_id) {
                let old_status = service.status;
                let service_name = service.name.clone();
                service.status = status;
                service.last_heartbeat = Utc::now();
                (old_status, service_name)
            } else {
                return Err(anyhow::anyhow!("Service not found: {}", service_id));
            }
        };
        
        // Update service statistics
        {
            let mut stats = self.statistics.write().await;
            if let Some(service_stats) = stats.service_statistics.get_mut(service_id) {
                service_stats.last_activity = Utc::now();
            }
            
            // Recalculate healthy services count
            let services = self.services.read().await;
            stats.healthy_services = services.values()
                .filter(|s| matches!(s.status, ServiceStatus::Healthy))
                .count() as u32;
                
            stats.last_updated = Utc::now();
        }
        
        // Emit alert if status changed to error or unresponsive
        if old_status != status && matches!(status, ServiceStatus::Error | ServiceStatus::Unresponsive) {
            let alert = SystemAlert::ConnectionIssue {
                service: service_name,
                endpoint: "internal".to_string(),
                error: format!("Service status changed to {:?}", status),
                retry_count: 0,
            };
            
            if let Err(e) = self.transport.publish_system_alert(alert).await {
                warn!("Failed to publish service status alert: {}", e);
            }
        }
        
        Ok(())
    }
    
    /// Route a market event to appropriate services
    #[instrument(skip(self, event))]
    pub async fn route_market_event(&self, event: MarketEvent, source_service: Option<&str>) -> Result<usize> {
        debug!("Routing market event from service: {:?}", source_service);
        
        // Publish to transport bus
        let subscriber_count = self.transport.publish_market_event(event.clone()).await
            .context("Failed to publish market event")?;
        
        // Update routing statistics
        {
            let mut stats = self.statistics.write().await;
            stats.events_routed += 1;
            stats.last_updated = Utc::now();
            
            if let Some(service_id) = source_service {
                if let Some(service_stats) = stats.service_statistics.get_mut(service_id) {
                    service_stats.events_sent += 1;
                    service_stats.last_activity = Utc::now();
                }
            }
        }
        
        debug!("Market event routed to {} subscribers", subscriber_count);
        Ok(subscriber_count)
    }
    
    /// Route a trading signal to appropriate services
    #[instrument(skip(self, signal))]
    pub async fn route_trading_signal(&self, signal: TradingSignal, source_service: Option<&str>) -> Result<usize> {
        debug!("Routing trading signal from service: {:?}", source_service);
        
        // Publish the signal directly (conversion to enhanced signal happens internally)
        let subscriber_count = self.transport.publish_trading_signal(signal).await
            .context("Failed to publish trading signal")?;
        
        // Update routing statistics
        {
            let mut stats = self.statistics.write().await;
            stats.events_routed += 1;
            stats.last_updated = Utc::now();
            
            if let Some(service_id) = source_service {
                if let Some(service_stats) = stats.service_statistics.get_mut(service_id) {
                    service_stats.events_sent += 1;
                    service_stats.last_activity = Utc::now();
                }
            }
        }
        
        debug!("Trading signal routed to {} subscribers", subscriber_count);
        Ok(subscriber_count)
    }
    
    /// Route a wallet event to appropriate services
    #[instrument(skip(self, event))]
    pub async fn route_wallet_event(&self, event: WalletEvent, source_service: Option<&str>) -> Result<usize> {
        debug!("Routing wallet event from service: {:?}", source_service);
        
        let subscriber_count = self.transport.publish_wallet_event(event).await
            .context("Failed to publish wallet event")?;
        
        // Update routing statistics
        {
            let mut stats = self.statistics.write().await;
            stats.events_routed += 1;
            stats.last_updated = Utc::now();
            
            if let Some(service_id) = source_service {
                if let Some(service_stats) = stats.service_statistics.get_mut(service_id) {
                    service_stats.events_sent += 1;
                    service_stats.last_activity = Utc::now();
                }
            }
        }
        
        debug!("Wallet event routed to {} subscribers", subscriber_count);
        Ok(subscriber_count)
    }
    
    /// Route a system alert to appropriate services
    #[instrument(skip(self, alert))]
    pub async fn route_system_alert(&self, alert: SystemAlert, source_service: Option<&str>) -> Result<usize> {
        debug!("Routing system alert from service: {:?}", source_service);
        
        let subscriber_count = self.transport.publish_system_alert(alert).await
            .context("Failed to publish system alert")?;
        
        // Update routing statistics
        {
            let mut stats = self.statistics.write().await;
            stats.events_routed += 1;
            stats.last_updated = Utc::now();
            
            if let Some(service_id) = source_service {
                if let Some(service_stats) = stats.service_statistics.get_mut(service_id) {
                    service_stats.events_sent += 1;
                    service_stats.last_activity = Utc::now();
                }
            }
        }
        
        debug!("System alert routed to {} subscribers", subscriber_count);
        Ok(subscriber_count)
    }
    
    /// Get list of services by type
    #[instrument(skip(self))]
    pub async fn get_services_by_type(&self, service_type: ServiceType) -> Vec<ServiceInfo> {
        let services = self.services.read().await;
        services.values()
            .filter(|service| service.service_type == service_type)
            .cloned()
            .collect()
    }
    
    /// Get list of services by capability
    #[instrument(skip(self))]
    pub async fn get_services_by_capability(&self, capability: ServiceCapability) -> Vec<ServiceInfo> {
        let services = self.services.read().await;
        services.values()
            .filter(|service| service.capabilities.contains(&capability))
            .cloned()
            .collect()
    }
    
    /// Get registry statistics
    pub async fn get_statistics(&self) -> RegistryStatistics {
        let stats = self.statistics.read().await;
        stats.clone()
    }
    
    /// Get health status of all services
    pub async fn health_check(&self) -> RegistryHealthStatus {
        let services = self.services.read().await;
        let stats = self.statistics.read().await;
        
        let total_services = services.len() as u32;
        let healthy_services = services.values()
            .filter(|s| matches!(s.status, ServiceStatus::Healthy))
            .count() as u32;
        let warning_services = services.values()
            .filter(|s| matches!(s.status, ServiceStatus::Warning))
            .count() as u32;
        let error_services = services.values()
            .filter(|s| matches!(s.status, ServiceStatus::Error | ServiceStatus::Unresponsive))
            .count() as u32;
            
        RegistryHealthStatus {
            total_services,
            healthy_services,
            warning_services,
            error_services,
            events_routed: stats.events_routed,
            failed_routes: stats.failed_routes,
            is_healthy: error_services == 0 && healthy_services > 0,
        }
    }
}

/// Health status of the service registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryHealthStatus {
    pub total_services: u32,
    pub healthy_services: u32,
    pub warning_services: u32,
    pub error_services: u32,
    pub events_routed: u64,
    pub failed_routes: u64,
    pub is_healthy: bool,
}

impl Default for RegistryStatistics {
    fn default() -> Self {
        Self {
            total_services: 0,
            healthy_services: 0,
            events_routed: 0,
            failed_routes: 0,
            last_updated: Utc::now(),
            service_statistics: HashMap::new(),
        }
    }
}