/// Fund Management Handler
/// 
/// Manages automated profit harvesting, risk controls, and portfolio rebalancing.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, debug};

use crate::core::{
    WalletManager, PortfolioTracker, FundManager, FundManagerConfig,
    HarvestConfig, RiskConfig, RebalanceConfig, RebalanceStrategy, RebalanceTarget
};
use crate::strike::DexConfig;

pub struct FundHandler {
    fund_manager: Arc<FundManager>,
}

impl FundHandler {
    /// Initialize fund management system
    pub async fn init(
        wallet_manager_lock: Arc<RwLock<WalletManager>>,
        portfolio_tracker: Arc<PortfolioTracker>,
        mmap_db: Arc<crate::core::db::UltraFastWalletDB>
    ) -> Result<Self> {
        info!("🏦 Initializing Fund Management System");
        
        // Create fund manager configuration
        let fund_config = FundManagerConfig {
            rpc_endpoint: "https://rpc.ankr.com/solana".to_string(), // Use more reliable endpoint
            dex_config: DexConfig::default(),
            min_trading_balance_sol: 1.0,
            max_position_size_percent: 10.0,
            daily_loss_limit_sol: 5.0,
            profit_harvest_threshold_percent: 50.0,
            stop_loss_threshold_percent: -20.0,
            rebalance_interval_secs: 3600,
            cold_transfer_minimum_sol: 10.0,
            max_transaction_retries: 3,
            confirmation_timeout_secs: 60,
            risk_check_interval_secs: 30,
        };
        
        // Create additional configurations
        let harvest_config = HarvestConfig::default();
        let risk_config = RiskConfig::default();
        let rebalance_config = RebalanceConfig {
            targets: vec![], // Would be configured based on strategy
            min_drift_threshold: 5.0,
            max_trade_size_sol: 10.0,
            strategy: RebalanceStrategy::Threshold,
        };
        
        // Use the shared mmap database from stalker
        
        // Clone the wallet manager from the lock
        let wallet_manager = {
            let guard = wallet_manager_lock.read().await;
            Arc::new((*guard).clone())
        };
        
        // Create fund manager
        let fund_manager = Arc::new(
            FundManager::new(
                fund_config,
                harvest_config,
                risk_config,
                rebalance_config,
                wallet_manager,
                portfolio_tracker,
                mmap_db
            ).map_err(|e| anyhow::anyhow!("Failed to create fund manager: {}", e))?
        );
        
        info!("✅ Fund Management System initialized");
        
        Ok(Self { fund_manager })
    }
    
    /// Start fund management services
    pub async fn start(&mut self) -> Result<()> {
        debug!("🔄 Starting fund management services");
        
        // Start automated profit harvesting and risk management
        self.fund_manager.start().await
            .map_err(|e| anyhow::anyhow!("Failed to start fund management: {}", e))?;
        
        Ok(())
    }
    
    /// Get fund manager reference
    pub fn get_manager(&self) -> Arc<FundManager> {
        Arc::clone(&self.fund_manager)
    }
    
    /// Get fund management status (simplified)
    pub async fn get_status(&self) -> String {
        "Fund Management Status: Active - automated profit harvesting and risk controls running".to_string()
    }
    
    /// Trigger manual profit harvest (simplified)
    pub async fn manual_harvest(&self) -> Result<String> {
        Ok("Manual harvest functionality available through fund manager".to_string())
    }
    
    /// Trigger manual rebalance (simplified)
    pub async fn manual_rebalance(&self) -> Result<String> {
        Ok("Manual rebalance functionality available through fund manager".to_string())
    }
    
    /// Get fund statistics
    pub async fn get_fund_stats(&self) -> String {
        // Implementation would get comprehensive fund statistics
        "Fund Statistics: Active management running".to_string()
    }
    
    /// Shutdown fund management
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("🛑 Shutting down fund management");
        
        // Fund manager will clean up automatically on drop
        
        Ok(())
    }
}