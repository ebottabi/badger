/// Strike Handler
/// 
/// Manages trade execution, DEX integration, and order processing.

use anyhow::Result;
use std::sync::Arc;
use tracing::{info, debug};

use crate::strike::{DexClient, DexConfig, TradeExecutor};

pub struct StrikeHandler {
    dex_client: Arc<DexClient>,
    trade_executor: Arc<TradeExecutor>,
}

impl StrikeHandler {
    /// Initialize strike system
    pub async fn init() -> Result<Self> {
        info!("⚡ Initializing Strike Trading Components");
        
        // Create DEX client configuration
        let dex_config = DexConfig::default();
        
        // Create DEX client
        let dex_client = Arc::new(
            DexClient::new(dex_config)
                .map_err(|e| anyhow::anyhow!("Failed to create DEX client: {}", e))?
        );
        
        // Create trade executor (orchestrator-managed wallets)
        let trade_executor = Arc::new(
            TradeExecutor::new_with_dex_only(Some(DexConfig::default())).await
                .map_err(|e| anyhow::anyhow!("Failed to create trade executor: {}", e))?
        );
        
        info!("✅ Strike Trading Components initialized");
        
        Ok(Self {
            dex_client,
            trade_executor,
        })
    }
    
    /// Start strike services
    pub async fn start(&mut self) -> Result<()> {
        debug!("🔄 Starting trade execution services");
        
        // Trade executor is ready for signal processing
        // No background tasks needed - responds to signals
        
        Ok(())
    }
    
    /// Get DEX client reference
    pub fn get_dex_client(&self) -> Arc<DexClient> {
        Arc::clone(&self.dex_client)
    }
    
    /// Get trade executor reference
    pub fn get_trade_executor(&self) -> Arc<TradeExecutor> {
        Arc::clone(&self.trade_executor)
    }
    
    /// Get trading statistics
    pub async fn get_stats(&self, wallet_pubkey: &str) -> Result<String> {
        // Get stats from trade executor
        match self.trade_executor.get_trading_stats(wallet_pubkey).await {
            Ok(stats) => Ok(format!(
                "Trading Stats for {}:\n\
                Total Trades: {}\n\
                Volume: {:.4} SOL\n\
                Successful: {} | Failed: {}\n\
                Net P&L: {:.4} SOL",
                &wallet_pubkey[..8],
                stats.total_trades_attempted,
                stats.total_volume_sol,
                stats.successful_trades,
                stats.failed_trades,
                stats.net_profit_loss_sol
            )),
            Err(e) => Ok(format!("Unable to get trading stats: {}", e))
        }
    }
    
    /// Shutdown strike system
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("🛑 Shutting down strike trading components");
        // Components will clean up automatically
        Ok(())
    }
}