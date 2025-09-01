/// Handler Module for Component Management
/// 
/// This module provides clean handlers for each major component of the trading system.
/// Each handler encapsulates initialization, configuration, and lifecycle management
/// for its respective component, promoting separation of concerns and maintainability.

pub mod wallet_handler;
pub mod scout_handler;
pub mod stalker_handler;
pub mod strike_handler;
pub mod portfolio_handler;
pub mod fund_handler;
pub mod transport_handler;

// Re-export handlers for easy access
pub use wallet_handler::WalletHandler;
pub use scout_handler::ScoutHandler;
pub use stalker_handler::StalkerHandler;
pub use strike_handler::StrikeHandler;
pub use portfolio_handler::PortfolioHandler;
pub use fund_handler::FundHandler;
pub use transport_handler::TransportHandler;

use anyhow::Result;
use std::sync::Arc;

/// Main system orchestrator that coordinates all handlers
pub struct SystemOrchestrator {
    pub wallet: WalletHandler,
    pub scout: ScoutHandler,
    pub stalker: StalkerHandler,
    pub strike: StrikeHandler,
    pub portfolio: PortfolioHandler,
    pub fund: FundHandler,
    pub transport: TransportHandler,
}

impl SystemOrchestrator {
    /// Initialize the complete trading system
    pub async fn init() -> Result<Self> {
        tracing::info!("🚀 Initializing Badger Trading System");
        
        // Initialize transport layer first
        let transport = TransportHandler::init().await?;
        
        // Initialize wallet management 
        let wallet = WalletHandler::init().await?;
        
        // Initialize scout (market scanning)
        let scout = ScoutHandler::init().await?;
        
        // Initialize stalker (wallet monitoring with intelligence)
        let stalker = StalkerHandler::init(transport.get_signal_sender()).await?;
        
        // Initialize strike (trade execution)
        let strike = StrikeHandler::init().await?;
        
        // Initialize portfolio tracking
        let portfolio = PortfolioHandler::init(
            Arc::clone(&wallet.get_manager()),
            stalker.get_mmap_db()
        ).await?;
        
        // Initialize fund management
        let fund = FundHandler::init(
            Arc::clone(&wallet.get_manager()),
            Arc::clone(&portfolio.get_tracker()),
            stalker.get_mmap_db()
        ).await?;
        
        tracing::info!("✅ All system components initialized successfully");
        
        Ok(Self {
            wallet,
            scout,
            stalker,
            strike,
            portfolio,
            fund,
            transport,
        })
    }
    
    /// Start all background services
    pub async fn start(&mut self) -> Result<()> {
        tracing::info!("🔄 Starting all system services");
        
        // Start services in proper dependency order
        self.scout.start().await?;
        self.stalker.start().await?;
        self.strike.start().await?;
        
        // Start portfolio tracking and let it fully initialize before fund management
        tracing::info!("🔄 Starting portfolio tracking...");
        self.portfolio.start(Arc::clone(self.wallet.get_manager())).await?;
        
        // Wait a moment for portfolio to populate with wallet data
        tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
        tracing::info!("🔄 Portfolio initialization complete, starting fund management...");
        
        // Start fund management after portfolio is ready
        self.fund.start().await?;
        self.transport.start().await?;
        
        tracing::info!("✅ All services started successfully");
        Ok(())
    }
    
    /// Get system status report
    pub async fn get_status(&self) -> String {
        format!(
            "🦡 Badger Trading System Status:\n\
            💰 Wallets: {}\n\
            🔍 Scout: Active token scanning\n\
            👁️  Stalker: Monitoring {} insider wallets\n\
            ⚡ Strike: Ready for trade execution\n\
            📊 Portfolio: Real-time tracking active\n\
            🏦 Fund Manager: Automated management running\n\
            🚌 Transport: {} services registered",
            self.wallet.get_status().await,
            self.stalker.get_monitored_count().await,
            self.transport.get_service_count().await
        )
    }
    
    /// Graceful shutdown of all services
    pub async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("🛑 Shutting down all system services");
        
        // Shutdown in reverse order
        self.transport.shutdown().await?;
        self.fund.shutdown().await?;
        self.portfolio.shutdown().await?;
        self.strike.shutdown().await?;
        self.stalker.shutdown().await?;
        self.scout.shutdown().await?;
        
        tracing::info!("✅ All services shut down cleanly");
        Ok(())
    }
}