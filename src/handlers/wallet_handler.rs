/// Wallet Management Handler
/// 
/// Handles all wallet-related operations including initialization, provisioning,
/// and secure management of trading and cold storage wallets.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, error};

use crate::core::{WalletManager, WalletProvisionConfig, WalletType};

pub struct WalletHandler {
    manager: Arc<RwLock<WalletManager>>,
}

impl WalletHandler {
    /// Initialize wallet management system
    pub async fn init() -> Result<Self> {
        info!("💰 Initializing Wallet Management System");
        
        // Create wallet provisioning configuration
        let wallet_config = WalletProvisionConfig {
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            config_dir: "wallets".to_string(),
            master_password: None, // Will be generated/prompted as needed
            auto_create: true,
            initial_trading_balance_sol: Some(0.1), // Start with 0.1 SOL for testing
        };

        // Create wallet manager
        let mut wallet_manager = WalletManager::new(wallet_config)
            .map_err(|e| anyhow::anyhow!("Failed to create wallet manager: {}", e))?;

        // Initialize and provision wallets
        wallet_manager.initialize().await
            .map_err(|e| anyhow::anyhow!("Failed to initialize wallet system: {}", e))?;

        let manager = Arc::new(RwLock::new(wallet_manager));
        
        info!("✅ Wallet Management System initialized");
        
        Ok(Self { manager })
    }
    
    /// Get wallet manager reference
    pub fn get_manager(&self) -> &Arc<RwLock<WalletManager>> {
        &self.manager
    }
    
    /// Get wallet system status
    pub async fn get_status(&self) -> String {
        match self.manager.read().await.get_available_wallets().len() {
            0 => "No wallets configured".to_string(),
            n => format!("{} wallets available", n),
        }
    }
    
    /// Get trading wallet address
    pub async fn get_trading_wallet_address(&self) -> Result<String> {
        let manager = self.manager.read().await;
        let config = manager.get_wallet_config(&WalletType::Trading)?;
        Ok(config.public_key.clone())
    }
    
    /// Get cold wallet address  
    pub async fn get_cold_wallet_address(&self) -> Result<String> {
        let manager = self.manager.read().await;
        let config = manager.get_wallet_config(&WalletType::Cold)?;
        Ok(config.public_key.clone())
    }
    
    /// Get wallet balance
    pub async fn get_wallet_balance(&self, wallet_type: WalletType) -> Result<f64> {
        let mut manager = self.manager.write().await;
        manager.get_balance(&wallet_type, true).await
    }
    
    /// List all configured wallets with their balances
    pub async fn list_wallets(&self) -> Result<Vec<(WalletType, String, f64)>> {
        let wallet_types = {
            let manager = self.manager.read().await;
            manager.get_available_wallets()
        };
        
        let mut wallets = Vec::new();
        
        for wallet_type in wallet_types {
            let mut manager = self.manager.write().await;
            let config = manager.get_wallet_config(&wallet_type)?;
            let public_key = config.public_key.clone();
            let balance = manager.get_balance(&wallet_type, false).await.unwrap_or(0.0);
            drop(manager); // Release the lock before pushing to vector
            wallets.push((wallet_type, public_key, balance));
        }
        
        Ok(wallets)
    }
    
    /// Get wallet statistics
    pub async fn get_wallet_stats(&self) -> Result<String> {
        let wallets = self.list_wallets().await?;
        let total_balance: f64 = wallets.iter().map(|(_, _, balance)| balance).sum();
        
        Ok(format!(
            "Total Wallets: {}\nTotal Balance: {:.4} SOL\nWallet Details:\n{}",
            wallets.len(),
            total_balance,
            wallets.iter()
                .map(|(wtype, addr, balance)| format!("  {:?}: {} ({:.4} SOL)", wtype, &addr[..8], balance))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}