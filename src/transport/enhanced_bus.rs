use anyhow::Result;
use tokio::sync::broadcast;
use tracing::{debug, warn, error, instrument};
use std::sync::Arc;

use crate::core::{MarketEvent, TradingSignal};

/// Multi-channel event bus for different event types in the Badger trading system
/// 
/// This bus provides typed channels for:
/// - MarketEvent: Real-time market data from DEX programs
/// - TradingSignal: Buy/sell signals generated from market analysis
/// - WalletEvent: Insider wallet activity and tracking
/// - SystemAlert: System status, errors, and performance alerts
#[derive(Debug, Clone)]
pub struct EnhancedTransportBus {
    market_events: broadcast::Sender<MarketEvent>,
    trading_signals: broadcast::Sender<TradingSignal>,
    wallet_events: broadcast::Sender<WalletEvent>,
    system_alerts: broadcast::Sender<SystemAlert>,
    stats: Arc<tokio::sync::RwLock<BusStatistics>>,
}

/// Statistics for monitoring bus performance
#[derive(Debug, Clone)]
pub struct BusStatistics {
    pub market_events_sent: u64,
    pub trading_signals_sent: u64,
    pub wallet_events_sent: u64,
    pub system_alerts_sent: u64,
    pub market_subscribers: usize,
    pub signal_subscribers: usize,
    pub wallet_subscribers: usize,
    pub alert_subscribers: usize,
}

impl Default for BusStatistics {
    fn default() -> Self {
        Self {
            market_events_sent: 0,
            trading_signals_sent: 0,
            wallet_events_sent: 0,
            system_alerts_sent: 0,
            market_subscribers: 0,
            signal_subscribers: 0,
            wallet_subscribers: 0,
            alert_subscribers: 0,
        }
    }
}

/// Wallet events for insider tracking and copy trading
#[derive(Debug, Clone, PartialEq)]
pub enum WalletEvent {
    InsiderActivity {
        wallet: String,
        action: InsiderAction,
        token_mint: String,
        amount_sol: f64,
        confidence: f64,
        slot: u64,
    },
    NewInsiderDetected {
        wallet: String,
        success_rate: f64,
        total_trades: u32,
        avg_profit_sol: f64,
    },
    WalletBlacklisted {
        wallet: String,
        reason: String,
        evidence: Vec<String>,
    },
    LargeWalletMovement {
        wallet: String,
        token_mint: String,
        direction: MovementDirection,
        amount_sol: f64,
        price_impact: Option<f64>,
    },
}

/// Types of insider actions detected
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InsiderAction {
    EarlyBuy,        // Buying within first hour of token launch
    LargeSell,       // Selling significant position
    LiquidityAdd,    // Adding liquidity to pool
    LiquidityRemove, // Removing liquidity (rug pull indicator)
    Accumulation,    // Building position over time
    Distribution,    // Selling position over time
}

/// Direction of wallet token movement
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MovementDirection {
    In,   // Buying/receiving tokens
    Out,  // Selling/sending tokens
}

/// System alerts for monitoring and error handling
#[derive(Debug, Clone, PartialEq)]
pub enum SystemAlert {
    ConnectionIssue {
        service: String,
        endpoint: String,
        error: String,
        retry_count: u32,
    },
    HighTrafficDetected {
        events_per_minute: u64,
        threshold: u64,
        service: String,
    },
    ExecutionError {
        order_id: String,
        token_mint: String,
        error: String,
        amount_sol: f64,
    },
    ConfigurationChange {
        setting: String,
        old_value: String,
        new_value: String,
        service: String,
    },
    PerformanceWarning {
        metric: String,
        current_value: f64,
        threshold: f64,
        service: String,
    },
    ServiceStartup {
        service: String,
        version: String,
    },
    ServiceShutdown {
        service: String,
        reason: String,
        uptime_seconds: u64,
    },
}

impl EnhancedTransportBus {
    /// Create a new enhanced transport bus with default channel sizes
    /// 
    /// Channel sizes are optimized for high-throughput Solana data:
    /// - MarketEvent: 50,000 capacity (high frequency pool/token events)
    /// - TradingSignal: 10,000 capacity (medium frequency signals)  
    /// - WalletEvent: 5,000 capacity (lower frequency insider events)
    /// - SystemAlert: 1,000 capacity (low frequency system events)
    #[instrument]
    pub fn new() -> Self {
        debug!("Initializing EnhancedTransportBus with production channel sizes");
        
        let (market_tx, _) = broadcast::channel(50_000);
        let (signal_tx, _) = broadcast::channel(10_000);
        let (wallet_tx, _) = broadcast::channel(5_000);
        let (alert_tx, _) = broadcast::channel(1_000);
        
        let bus = Self {
            market_events: market_tx,
            trading_signals: signal_tx,
            wallet_events: wallet_tx,
            system_alerts: alert_tx,
            stats: Arc::new(tokio::sync::RwLock::new(BusStatistics::default())),
        };
        
        debug!("EnhancedTransportBus initialized successfully");
        bus
    }
    
    /// Create a new transport bus with custom channel sizes for specific use cases
    #[instrument]
    pub fn with_capacity(
        market_capacity: usize,
        signal_capacity: usize,
        wallet_capacity: usize,
        alert_capacity: usize,
    ) -> Self {
        debug!(
            market_capacity = market_capacity,
            signal_capacity = signal_capacity,
            wallet_capacity = wallet_capacity,
            alert_capacity = alert_capacity,
            "Creating EnhancedTransportBus with custom capacities"
        );
        
        let (market_tx, _) = broadcast::channel(market_capacity);
        let (signal_tx, _) = broadcast::channel(signal_capacity);
        let (wallet_tx, _) = broadcast::channel(wallet_capacity);
        let (alert_tx, _) = broadcast::channel(alert_capacity);
        
        Self {
            market_events: market_tx,
            trading_signals: signal_tx,
            wallet_events: wallet_tx,
            system_alerts: alert_tx,
            stats: Arc::new(tokio::sync::RwLock::new(BusStatistics::default())),
        }
    }
    
    // Market Event Publishers
    
    /// Publish a market event (pool creation, token launch, swap, etc.)
    #[instrument(skip(self, event), fields(event_type = ?std::mem::discriminant(&event)))]
    pub async fn publish_market_event(&self, event: MarketEvent) -> Result<usize> {
        match self.market_events.send(event) {
            Ok(subscriber_count) => {
                let mut stats = self.stats.write().await;
                stats.market_events_sent += 1;
                debug!(
                    subscriber_count = subscriber_count,
                    total_sent = stats.market_events_sent,
                    "Published market event"
                );
                Ok(subscriber_count)
            }
            Err(e) => {
                debug!(error = %e, "Market event not published - no subscribers");
                Err(anyhow::anyhow!("No market event subscribers: {}", e))
            }
        }
    }
    
    /// Publish a trading signal (buy, sell, alert)
    #[instrument(skip(self, signal), fields(signal_type = ?std::mem::discriminant(&signal)))]
    pub async fn publish_trading_signal(&self, signal: TradingSignal) -> Result<usize> {
        match self.trading_signals.send(signal) {
            Ok(subscriber_count) => {
                let mut stats = self.stats.write().await;
                stats.trading_signals_sent += 1;
                debug!(
                    subscriber_count = subscriber_count,
                    total_sent = stats.trading_signals_sent,
                    "Published trading signal"
                );
                Ok(subscriber_count)
            }
            Err(e) => {
                debug!(error = %e, "Trading signal not published - no subscribers");
                Err(anyhow::anyhow!("No trading signal subscribers: {}", e))
            }
        }
    }
    
    /// Publish a wallet event (insider activity, new insider detected, etc.)
    #[instrument(skip(self, event), fields(event_type = ?std::mem::discriminant(&event)))]
    pub async fn publish_wallet_event(&self, event: WalletEvent) -> Result<usize> {
        match self.wallet_events.send(event) {
            Ok(subscriber_count) => {
                let mut stats = self.stats.write().await;
                stats.wallet_events_sent += 1;
                debug!(
                    subscriber_count = subscriber_count,
                    total_sent = stats.wallet_events_sent,
                    "Published wallet event"
                );
                Ok(subscriber_count)
            }
            Err(e) => {
                debug!(error = %e, "Wallet event not published - no subscribers");
                Err(anyhow::anyhow!("No wallet event subscribers: {}", e))
            }
        }
    }
    
    /// Publish a system alert (errors, warnings, status updates)
    #[instrument(skip(self, alert), fields(alert_type = ?std::mem::discriminant(&alert)))]
    pub async fn publish_system_alert(&self, alert: SystemAlert) -> Result<usize> {
        match self.system_alerts.send(alert) {
            Ok(subscriber_count) => {
                let mut stats = self.stats.write().await;
                stats.system_alerts_sent += 1;
                debug!(
                    subscriber_count = subscriber_count,
                    total_sent = stats.system_alerts_sent,
                    "Published system alert"
                );
                Ok(subscriber_count)
            }
            Err(e) => {
                debug!(error = %e, "System alert not published - no subscribers");
                Err(anyhow::anyhow!("No system alert subscribers: {}", e))
            }
        }
    }
    
    // Event Subscribers
    
    /// Subscribe to market events (pools, tokens, swaps, transfers)
    #[instrument(skip(self))]
    pub async fn subscribe_market_events(&self) -> broadcast::Receiver<MarketEvent> {
        let receiver = self.market_events.subscribe();
        let mut stats = self.stats.write().await;
        stats.market_subscribers = self.market_events.receiver_count();
        debug!(
            total_subscribers = stats.market_subscribers,
            "New market event subscriber added"
        );
        receiver
    }
    
    /// Subscribe to trading signals
    #[instrument(skip(self))]
    pub async fn subscribe_trading_signals(&self) -> broadcast::Receiver<TradingSignal> {
        let receiver = self.trading_signals.subscribe();
        let mut stats = self.stats.write().await;
        stats.signal_subscribers = self.trading_signals.receiver_count();
        debug!(
            total_subscribers = stats.signal_subscribers,
            "New trading signal subscriber added"
        );
        receiver
    }
    
    /// Subscribe to wallet events (insider activity)
    #[instrument(skip(self))]
    pub async fn subscribe_wallet_events(&self) -> broadcast::Receiver<WalletEvent> {
        let receiver = self.wallet_events.subscribe();
        let mut stats = self.stats.write().await;
        stats.wallet_subscribers = self.wallet_events.receiver_count();
        debug!(
            total_subscribers = stats.wallet_subscribers,
            "New wallet event subscriber added"
        );
        receiver
    }
    
    /// Subscribe to system alerts
    #[instrument(skip(self))]
    pub async fn subscribe_system_alerts(&self) -> broadcast::Receiver<SystemAlert> {
        let receiver = self.system_alerts.subscribe();
        let mut stats = self.stats.write().await;
        stats.alert_subscribers = self.system_alerts.receiver_count();
        debug!(
            total_subscribers = stats.alert_subscribers,
            "New system alert subscriber added"
        );
        receiver
    }
    
    // Statistics and Monitoring
    
    /// Get current bus statistics for monitoring
    pub async fn get_statistics(&self) -> BusStatistics {
        let mut stats = self.stats.write().await;
        
        // Update subscriber counts
        stats.market_subscribers = self.market_events.receiver_count();
        stats.signal_subscribers = self.trading_signals.receiver_count();
        stats.wallet_subscribers = self.wallet_events.receiver_count();
        stats.alert_subscribers = self.system_alerts.receiver_count();
        
        stats.clone()
    }
    
    /// Check if bus has active subscribers for each channel
    pub async fn health_check(&self) -> BusHealthStatus {
        let stats = self.get_statistics().await;
        
        BusHealthStatus {
            market_events_healthy: stats.market_subscribers > 0,
            trading_signals_healthy: stats.signal_subscribers > 0,
            wallet_events_healthy: stats.wallet_subscribers > 0,
            system_alerts_healthy: stats.alert_subscribers > 0,
            total_events_processed: stats.market_events_sent + stats.trading_signals_sent + stats.wallet_events_sent + stats.system_alerts_sent,
        }
    }
}

/// Health status of the transport bus
#[derive(Debug, Clone)]
pub struct BusHealthStatus {
    pub market_events_healthy: bool,
    pub trading_signals_healthy: bool,
    pub wallet_events_healthy: bool,
    pub system_alerts_healthy: bool,
    pub total_events_processed: u64,
}

impl BusHealthStatus {
    pub fn is_healthy(&self) -> bool {
        self.market_events_healthy && self.trading_signals_healthy
    }
}

impl Default for EnhancedTransportBus {
    fn default() -> Self {
        Self::new()
    }
}