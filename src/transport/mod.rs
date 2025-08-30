pub mod enhanced_bus;
pub mod events;
pub mod signals;
pub mod routing;

// Legacy modules (will be deprecated)
pub mod market_bus;
pub mod signal_bus;
pub mod alert_bus;

// Enhanced transport exports (primary Phase 2 exports)
pub use enhanced_bus::{
    EnhancedTransportBus, BusStatistics, BusHealthStatus, 
    WalletEvent, SystemAlert, InsiderAction as EnhancedInsiderAction, 
    MovementDirection
};
pub use events::{
    EnhancedMarketEvent, EnhancedPoolInfo, EnhancedTokenMetadata, 
    EnhancedSwapEvent, EnhancedLargeTransfer, PoolType, BurnReason, 
    LiquidityChangeType, VolumeRank, RiskLevel as EnhancedRiskLevel, 
    AuditStatus, TransferType, CoordinatedActivityType, WhaleAction, 
    TransferPattern, TokenHolder, AuditReport, AuditFinding, 
    WalletSwapHistory, WalletTransferHistory, MarketImpact
};
pub use signals::{
    EnhancedTradingSignal, SignalUrgency, ExecutionStrategy, SellStrategy,
    AlertType as EnhancedAlertType, AlertSeverity, RiskType, RecommendedAction,
    PriceLevel, PriceLevelType, PriceLevelAction, RiskMonitoring,
    AlertEvidence, EvidenceType, RiskEvidence
};
pub use routing::{
    ServiceRegistry, ServiceInfo, ServiceType, ServiceCapability, 
    ServiceStatus, SubscriptionInfo, EventType, EventFilter, 
    FilterOperator, RoutingRule, RoutingCondition, RegistryStatistics,
    ServiceStatistics, RegistryHealthStatus
};

// Legacy exports (for backward compatibility)
pub use market_bus::MarketBus;
pub use signal_bus::*;
pub use alert_bus::*;