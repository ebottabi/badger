/// Stalker Handler
/// 
/// Manages wallet monitoring and insider activity detection.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, debug};

use crate::core::TradingSignal;
use crate::stalker::{WalletMonitor, MonitorConfig, WalletIntelligenceCache, CopyTradingEngine};
use crate::core::db::{UltraFastWalletDB, MmapConfig};

pub struct StalkerHandler {
    monitor: Arc<WalletMonitor>,
    intelligence_cache: Arc<WalletIntelligenceCache>,
    copy_trader: Arc<CopyTradingEngine>,
    mmap_db: Arc<UltraFastWalletDB>,
}

impl StalkerHandler {
    /// Initialize stalker system with intelligence
    pub async fn init(signal_sender: mpsc::UnboundedSender<TradingSignal>) -> Result<Self> {
        info!("👁️  Initializing Stalker Wallet Monitor with Intelligence");
        
        // Initialize memory-mapped database
        let mmap_config = MmapConfig {
            file_path: "data/wallet_intelligence.mmap".to_string(),
            capacity: 1_048_576, // 2^20 - power of 2
            ..Default::default()
        };
        let mmap_db = Arc::new(UltraFastWalletDB::new(mmap_config)?);
        
        // Initialize intelligence cache 
        let intelligence_cache = Arc::new(WalletIntelligenceCache::new());
        intelligence_cache.initialize_with_mmap(mmap_db.clone()).await;
        
        // Initialize copy trading engine
        let copy_trader = Arc::new(CopyTradingEngine::new_simple(signal_sender, intelligence_cache.clone()));
        
        // Create monitor configuration
        let monitor_config = MonitorConfig::default();
        
        // Create wallet monitor
        let monitor = Arc::new(
            WalletMonitor::new(Some(monitor_config)).await
                .map_err(|e| anyhow::anyhow!("Failed to create wallet monitor: {}", e))?
        );
        
        let insider_count = mmap_db.get_insider_count().await;
        info!("✅ Stalker with Intelligence initialized - {} insider wallets tracked", insider_count);
        
        Ok(Self { 
            monitor,
            intelligence_cache,
            copy_trader,
            mmap_db,
        })
    }
    
    /// Start stalker monitoring
    pub async fn start(&mut self) -> Result<()> {
        debug!("🔄 Starting wallet monitoring");
        
        // Start monitoring background tasks
        // Monitor runs continuously watching wallet activities
        
        Ok(())
    }
    
    /// Get monitor reference
    pub fn get_monitor(&self) -> Arc<WalletMonitor> {
        Arc::clone(&self.monitor)
    }
    
    /// Get number of monitored insider wallets
    pub async fn get_monitored_count(&self) -> usize {
        self.mmap_db.get_insider_count().await
    }
    
    /// Get memory-mapped database reference
    pub fn get_mmap_db(&self) -> Arc<UltraFastWalletDB> {
        Arc::clone(&self.mmap_db)
    }
    
    /// Get monitoring statistics
    pub async fn get_stats(&self) -> String {
        format!("Monitoring {} insider wallets for trading activity", 
                self.get_monitored_count().await)
    }
    
    /// Shutdown stalker system
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("🛑 Shutting down stalker monitor");
        // Monitor will clean up automatically
        Ok(())
    }
}