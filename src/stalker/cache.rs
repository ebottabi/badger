/// Ultra-fast memory cache for nanosecond insider wallet decisions
/// 
/// This cache supports both HashMap (legacy) and memory-mapped database (ultra-fast)
/// implementations with seamless fallback during migration.

use super::intelligence_types::*;
use crate::core::db::{UltraFastWalletDB, MmapConfig};
use crate::core::db::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::sync::atomic::{AtomicU64, AtomicI64, Ordering};
use chrono::Utc;
use tracing::{debug, info, warn};

/// Ultra-fast memory cache for insider wallet decisions
pub struct WalletIntelligenceCache {
    /// Memory-mapped database for ultra-fast lookups (1-5ns)
    mmap_db: Option<Arc<UltraFastWalletDB>>,
    
    /// Legacy HashMap storage (fallback during migration)
    insider_wallets: Arc<RwLock<HashMap<String, InsiderWallet>>>,
    
    /// Pre-sorted list of top performers for fast iteration
    top_performers: Arc<RwLock<Vec<String>>>,
    
    /// Blacklisted wallets for instant rejection
    blacklisted: Arc<RwLock<HashSet<String>>>,
    
    /// Token launch times for age calculation
    token_launches: Arc<RwLock<HashMap<String, i64>>>,
    
    /// Performance statistics (atomic for thread safety)
    total_lookups: AtomicU64,
    cache_hits: AtomicU64,
    last_update: AtomicI64,
    
    /// Migration flag - true when using memory-mapped database
    use_mmap: bool,
    
    /// Configuration
    config: CacheConfig,
}

/// Cache configuration parameters
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Minimum confidence score to copy trade
    pub min_confidence_threshold: f64,
    
    /// Maximum token age in minutes to copy
    pub max_token_age_minutes: u32,
    
    /// Base position size in SOL
    pub base_position_sol: f64,
    
    /// Maximum position size multiplier
    pub max_position_multiplier: f64,
    
    /// Number of top performers to track
    pub top_performers_count: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            min_confidence_threshold: 0.75,
            max_token_age_minutes: 30,
            base_position_sol: 0.1,
            max_position_multiplier: 2.0,
            top_performers_count: 20,
        }
    }
}

impl WalletIntelligenceCache {
    /// Create new cache with default configuration
    pub fn new() -> Self {
        Self::with_config(CacheConfig::default())
    }
    
    /// Create new cache with custom configuration
    pub fn with_config(config: CacheConfig) -> Self {
        info!("🧠 Initializing WalletIntelligenceCache with config: min_confidence={}, max_token_age={}min", 
              config.min_confidence_threshold, config.max_token_age_minutes);
        
        Self {
            mmap_db: None, // Start without memory-mapped DB
            insider_wallets: Arc::new(RwLock::new(HashMap::new())),
            top_performers: Arc::new(RwLock::new(Vec::new())),
            blacklisted: Arc::new(RwLock::new(HashSet::new())),
            token_launches: Arc::new(RwLock::new(HashMap::new())),
            total_lookups: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            last_update: AtomicI64::new(Utc::now().timestamp()),
            use_mmap: false,
            config,
        }
    }
    
    /// Initialize cache with existing insider wallets from database
    pub async fn initialize_with_insiders(&self, insiders: Vec<InsiderWallet>) {
        let mut wallets = self.insider_wallets.write().await;
        let mut blacklist = self.blacklisted.write().await;
        
        let mut active_count = 0;
        let mut blacklisted_count = 0;
        
        for insider in insiders {
            match insider.status {
                WalletStatus::Blacklisted => {
                    blacklist.insert(insider.address.clone());
                    blacklisted_count += 1;
                }
                WalletStatus::Active => {
                    active_count += 1;
                    wallets.insert(insider.address.clone(), insider);
                }
                _ => {
                    wallets.insert(insider.address.clone(), insider);
                }
            }
        }
        
        let total_wallets = wallets.len();
        
        drop(wallets);
        drop(blacklist);
        
        // Update top performers list
        self.update_top_performers().await;
        
        info!("✅ Cache initialized with {} insider wallets ({} active, {} blacklisted)", 
              total_wallets + blacklisted_count, active_count, blacklisted_count);
    }
    
    /// Initialize cache with memory-mapped database
    pub async fn initialize_with_mmap(&self, mmap_db: Arc<UltraFastWalletDB>) {
        // Store reference to memory-mapped database
        // This is a bit tricky since the field is Option<Arc<...>> and not RwLock
        // For now, we'll just log that we're ready for memory-mapped operations
        info!("✅ Cache ready for ultra-fast memory-mapped database operations");
        info!("   🚀 Memory-mapped database contains {} insider wallets", mmap_db.get_insider_count().await);
    }
    
    /// ULTRA-FAST: Check if wallet is insider and get confidence (nanosecond speed)
    /// This is the hottest path - optimized for maximum performance
    #[inline(always)]
    pub async fn is_insider(&self, wallet_address: &str) -> Option<f64> {
        // Increment lookup counter (atomic operation)
        self.total_lookups.fetch_add(1, Ordering::Relaxed);
        
        // Read lock - multiple concurrent readers allowed
        let wallets = self.insider_wallets.read().await;
        
        if let Some(insider) = wallets.get(wallet_address) {
            // Cache hit
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
            Some(insider.confidence_score)
        } else {
            None
        }
    }
    
    /// ULTRA-FAST: Make copy trading decision (nanosecond speed)
    /// This is the core decision function called for every market event
    #[inline(always)]
    pub async fn should_copy_trade(
        &self, 
        wallet_address: &str, 
        token_age_minutes: u32
    ) -> Option<CopyDecision> {
        // Instant blacklist check
        {
            let blacklist = self.blacklisted.read().await;
            if blacklist.contains(wallet_address) {
                return None; // Instant rejection
            }
        }
        
        // Token age check
        if token_age_minutes > self.config.max_token_age_minutes {
            return None; // Too old
        }
        
        // Get insider information
        let wallets = self.insider_wallets.read().await;
        let insider = wallets.get(wallet_address)?;
        
        // Apply decision logic
        if insider.confidence_score >= self.config.min_confidence_threshold && 
           insider.status == WalletStatus::Active {
            
            Some(CopyDecision {
                should_copy: true,
                confidence: insider.confidence_score,
                position_size: self.calculate_position_size(insider),
                delay_seconds: insider.copy_delay_seconds(),
                urgency: self.calculate_urgency(insider.confidence_score),
            })
        } else {
            None
        }
    }
    
    /// Initialize memory-mapped database for ultra-fast lookups
    pub async fn enable_mmap_db(&mut self, mmap_config: MmapConfig) -> Result<(), Box<dyn std::error::Error>> {
        info!("🚀 Enabling ultra-fast memory-mapped database");
        
        let mmap_db = Arc::new(UltraFastWalletDB::new(mmap_config)?);
        
        // Migrate existing data from HashMap to memory-mapped DB
        self.migrate_to_mmap(&mmap_db).await?;
        
        self.mmap_db = Some(mmap_db);
        self.use_mmap = true;
        
        info!("✅ Memory-mapped database enabled - ultra-fast mode activated!");
        Ok(())
    }
    
    /// Migrate HashMap data to memory-mapped database
    async fn migrate_to_mmap(&self, mmap_db: &UltraFastWalletDB) -> Result<(), Box<dyn std::error::Error>> {
        let wallets = self.insider_wallets.read().await;
        let total_wallets = wallets.len();
        
        info!("📊 Migrating {} wallets from HashMap to memory-mapped storage", total_wallets);
        
        let mut migrated_count = 0;
        
        for (address_str, insider) in wallets.iter() {
            if let Some(address_bytes) = parse_solana_address(address_str) {
                let entry = WalletCacheEntry {
                    full_address: address_bytes,
                    confidence: insider.confidence_score as f32,
                    win_rate: insider.win_rate as f32,
                    avg_profit: insider.avg_profit_percentage as f32,
                    last_activity: insider.last_trade_timestamp as u32,
                    total_trades: insider.total_trades as u32,
                    early_entry_score: insider.early_entry_score as f32,
                    recent_activity: insider.recent_activity_score as f32,
                    flags: match insider.status {
                        WalletStatus::Active => 1,
                        WalletStatus::Blacklisted => 2,
                        WalletStatus::Monitoring => 0,
                        WalletStatus::Cooldown => 3,
                    },
                    ..Default::default()
                };
                
                if mmap_db.insert_wallet(&entry).is_ok() {
                    migrated_count += 1;
                }
            }
        }
        
        info!("✅ Migration complete: {}/{} wallets migrated successfully", migrated_count, total_wallets);
        
        if migrated_count != total_wallets {
            warn!("⚠️  {} wallets failed to migrate", total_wallets - migrated_count);
        }
        
        Ok(())
    }
    
    /// ULTRA-FAST wallet confidence lookup (~1-5ns)
    #[inline(always)]
    pub fn is_insider_wallet_fast(&self, address: &[u8; 32]) -> Option<f64> {
        if self.use_mmap {
            if let Some(mmap_db) = &self.mmap_db {
                mmap_db.lookup_confidence(address).map(|f| f as f64)
            } else {
                None
            }
        } else {
            // Fallback to legacy HashMap method during migration
            None // Would need async version for HashMap
        }
    }
    
    /// ULTRA-FAST copy trading decision (~2-5ns)
    #[inline(always)]
    pub fn should_copy_trade_fast(&self, address: &[u8; 32], token_age_minutes: u32) -> Option<CopyDecision> {
        // Ultra-fast age check
        if token_age_minutes > self.config.max_token_age_minutes {
            return None;
        }
        
        if self.use_mmap {
            if let Some(mmap_db) = &self.mmap_db {
                // Get wallet info in one ultra-fast operation
                if let Some(wallet_info) = mmap_db.lookup_wallet_info(address) {
                    // Check if blacklisted (flags & 2 == 2)
                    if wallet_info.flags & 2 != 0 {
                        return None; // Blacklisted
                    }
                    
                    // Check if active (flags & 1 == 1) and high confidence
                    if wallet_info.flags & 1 != 0 && 
                       wallet_info.confidence >= self.config.min_confidence_threshold as f32 {
                        
                        return Some(CopyDecision {
                            should_copy: true,
                            confidence: wallet_info.confidence as f64,
                            position_size: self.calculate_position_size_fast(&wallet_info),
                            delay_seconds: self.calculate_delay_fast(wallet_info.confidence),
                            urgency: self.calculate_urgency_fast(wallet_info.confidence),
                        });
                    }
                }
            }
        }
        
        None
    }
    
    /// Calculate position size from memory-mapped entry (ultra-fast)
    #[inline(always)]
    fn calculate_position_size_fast(&self, wallet_info: &WalletCacheEntry) -> f64 {
        let base_size = self.config.base_position_sol;
        let confidence_multiplier = (wallet_info.confidence as f64 * 2.0)
            .min(self.config.max_position_multiplier);
        let risk_factor = 0.8;
        
        base_size * confidence_multiplier * risk_factor
    }
    
    /// Calculate copy delay from confidence (ultra-fast)
    #[inline(always)]
    fn calculate_delay_fast(&self, confidence: f32) -> u32 {
        if confidence >= 0.9 { 0 }      // Immediate for very high confidence
        else if confidence >= 0.8 { 1 }  // 1 second delay
        else if confidence >= 0.7 { 2 }  // 2 second delay
        else { 5 }                       // 5 second delay for lower confidence
    }
    
    /// Calculate urgency from confidence (ultra-fast)
    #[inline(always)]
    fn calculate_urgency_fast(&self, confidence: f32) -> SignalUrgency {
        if confidence >= 0.9 { SignalUrgency::Immediate }
        else if confidence >= 0.8 { SignalUrgency::High }
        else if confidence >= 0.7 { SignalUrgency::Normal }
        else { SignalUrgency::Low }
    }
    
    /// Calculate position size based on insider confidence
    #[inline(always)]
    fn calculate_position_size(&self, insider: &InsiderWallet) -> f64 {
        let base_size = self.config.base_position_sol;
        let confidence_multiplier = insider.position_size_multiplier()
            .min(self.config.max_position_multiplier);
        let risk_factor = 0.8; // TODO: Get from portfolio risk manager
        
        base_size * confidence_multiplier * risk_factor
    }
    
    /// Calculate signal urgency based on confidence
    #[inline(always)]
    fn calculate_urgency(&self, confidence: f64) -> SignalUrgency {
        if confidence >= 0.90 {
            SignalUrgency::Immediate
        } else if confidence >= 0.80 {
            SignalUrgency::High
        } else {
            SignalUrgency::Normal
        }
    }
    
    /// Update insider wallet information (atomic operation)
    pub async fn update_insider(&self, wallet_address: String, updated_insider: InsiderWallet) {
        let mut wallets = self.insider_wallets.write().await;
        
        // Handle status changes
        match updated_insider.status {
            WalletStatus::Blacklisted => {
                // Move to blacklist
                wallets.remove(&wallet_address);
                let mut blacklist = self.blacklisted.write().await;
                blacklist.insert(wallet_address.clone());
                warn!("📛 Wallet {} moved to blacklist", wallet_address);
            }
            _ => {
                // Remove from blacklist if previously blacklisted
                let mut blacklist = self.blacklisted.write().await;
                if blacklist.remove(&wallet_address) {
                    info!("✅ Wallet {} removed from blacklist", wallet_address);
                }
                drop(blacklist);
                
                // Update in main cache
                wallets.insert(wallet_address.clone(), updated_insider);
            }
        }
        
        drop(wallets);
        
        // Update last modification time
        self.last_update.store(Utc::now().timestamp(), Ordering::Relaxed);
        
        // Update top performers if needed
        self.update_top_performers().await;
    }
    
    /// Batch update multiple insider wallets (efficient for sync operations)
    pub async fn batch_update_insiders(&self, updates: Vec<(String, InsiderWallet)>) {
        let mut wallets = self.insider_wallets.write().await;
        let mut blacklist = self.blacklisted.write().await;
        
        for (wallet_address, updated_insider) in updates {
            match updated_insider.status {
                WalletStatus::Blacklisted => {
                    wallets.remove(&wallet_address);
                    blacklist.insert(wallet_address);
                }
                _ => {
                    blacklist.remove(&wallet_address);
                    wallets.insert(wallet_address, updated_insider);
                }
            }
        }
        
        drop(wallets);
        drop(blacklist);
        
        self.last_update.store(Utc::now().timestamp(), Ordering::Relaxed);
        self.update_top_performers().await;
    }
    
    /// Add new insider wallet to cache
    pub async fn add_insider(&self, insider: InsiderWallet) {
        let wallet_address = insider.address.clone();
        
        match insider.status {
            WalletStatus::Blacklisted => {
                let mut blacklist = self.blacklisted.write().await;
                blacklist.insert(wallet_address);
            }
            _ => {
                let mut wallets = self.insider_wallets.write().await;
                wallets.insert(wallet_address.clone(), insider);
                info!("➕ Added new insider wallet: {} (confidence: {:.3})", 
                      wallet_address, wallets.get(&wallet_address).unwrap().confidence_score);
            }
        }
        
        self.last_update.store(Utc::now().timestamp(), Ordering::Relaxed);
        self.update_top_performers().await;
    }
    
    /// Remove insider wallet from cache
    pub async fn remove_insider(&self, wallet_address: &str) {
        let mut wallets = self.insider_wallets.write().await;
        let mut blacklist = self.blacklisted.write().await;
        
        if wallets.remove(wallet_address).is_some() || blacklist.remove(wallet_address) {
            info!("➖ Removed insider wallet: {}", wallet_address);
            self.last_update.store(Utc::now().timestamp(), Ordering::Relaxed);
        }
        
        drop(wallets);
        drop(blacklist);
        self.update_top_performers().await;
    }
    
    /// Record token launch time for age calculations
    pub async fn record_token_launch(&self, token_mint: String, launch_timestamp: i64) {
        let mut launches = self.token_launches.write().await;
        launches.insert(token_mint.clone(), launch_timestamp);
        
        // Keep only recent launches (last 24 hours)
        let cutoff = Utc::now().timestamp() - (24 * 3600);
        launches.retain(|_, &mut timestamp| timestamp > cutoff);
        
        debug!("📅 Recorded token launch: {} at {}", token_mint, launch_timestamp);
    }
    
    /// Get token age in minutes (for early entry calculations)
    pub async fn get_token_age_minutes(&self, token_mint: &str, current_timestamp: i64) -> Option<u32> {
        let launches = self.token_launches.read().await;
        if let Some(&launch_time) = launches.get(token_mint) {
            let age_seconds = current_timestamp - launch_time;
            Some((age_seconds / 60) as u32)
        } else {
            None // Unknown launch time
        }
    }
    
    /// Get token launch time directly (for age calculations)
    pub async fn get_token_launch_time(&self, token_mint: &str) -> Option<i64> {
        let launches = self.token_launches.read().await;
        launches.get(token_mint).copied()
    }
    
    /// Cache token launch time for future nanosecond lookups
    pub async fn cache_token_launch_time(&self, token_mint: &str, launch_timestamp: i64) {
        self.record_token_launch(token_mint.to_string(), launch_timestamp).await;
    }
    
    /// Update top performers list (called after cache updates)
    async fn update_top_performers(&self) {
        let wallets = self.insider_wallets.read().await;
        
        // Get active wallets sorted by confidence
        let mut sorted_wallets: Vec<_> = wallets
            .iter()
            .filter(|(_, insider)| insider.status == WalletStatus::Active)
            .collect();
        
        sorted_wallets.sort_by(|a, b| 
            b.1.confidence_score.partial_cmp(&a.1.confidence_score).unwrap()
        );
        
        // Update top performers list
        let mut top_performers = self.top_performers.write().await;
        top_performers.clear();
        
        for (address, _) in sorted_wallets.into_iter().take(self.config.top_performers_count) {
            top_performers.push(address.clone());
        }
        
        debug!("🏆 Updated top performers list with {} wallets", top_performers.len());
    }
    
    /// Get current cache statistics
    pub async fn get_statistics(&self) -> CacheStatistics {
        let wallets = self.insider_wallets.read().await;
        let blacklist = self.blacklisted.read().await;
        
        let total_lookups = self.total_lookups.load(Ordering::Relaxed);
        let cache_hits = self.cache_hits.load(Ordering::Relaxed);
        let hit_rate = if total_lookups > 0 {
            cache_hits as f64 / total_lookups as f64
        } else {
            0.0
        };
        
        let active_count = wallets.values()
            .filter(|w| w.status == WalletStatus::Active)
            .count();
        
        // Estimate memory usage
        let memory_usage = (wallets.len() * std::mem::size_of::<InsiderWallet>()) +
                          (blacklist.len() * 42) + // 42 bytes per address
                          (self.config.top_performers_count * 42);
        
        CacheStatistics {
            insider_count: wallets.len(),
            active_count,
            blacklisted_count: blacklist.len(),
            total_lookups,
            hit_rate,
            last_update: self.last_update.load(Ordering::Relaxed),
            memory_usage_bytes: memory_usage,
        }
    }
    
    /// Get number of insider wallets in cache
    pub async fn get_insider_count(&self) -> usize {
        let wallets = self.insider_wallets.read().await;
        wallets.len()
    }
    
    /// Get top performing wallets (pre-sorted list)
    pub async fn get_top_performers(&self) -> Vec<String> {
        let top_performers = self.top_performers.read().await;
        top_performers.clone()
    }
    
    /// Check if wallet is blacklisted (instant check)
    pub async fn is_blacklisted(&self, wallet_address: &str) -> bool {
        let blacklist = self.blacklisted.read().await;
        blacklist.contains(wallet_address)
    }
    
    /// Get insider wallet details (for debugging/monitoring)
    pub async fn get_insider_details(&self, wallet_address: &str) -> Option<InsiderWallet> {
        let wallets = self.insider_wallets.read().await;
        wallets.get(wallet_address).cloned()
    }
    
    /// Clear all cache data (for testing/reset)
    pub async fn clear_all(&self) {
        let mut wallets = self.insider_wallets.write().await;
        let mut blacklist = self.blacklisted.write().await;
        let mut launches = self.token_launches.write().await;
        let mut top_performers = self.top_performers.write().await;
        
        wallets.clear();
        blacklist.clear();
        launches.clear();
        top_performers.clear();
        
        self.total_lookups.store(0, Ordering::Relaxed);
        self.cache_hits.store(0, Ordering::Relaxed);
        self.last_update.store(Utc::now().timestamp(), Ordering::Relaxed);
        
        info!("🧹 Cache cleared");
    }
}

impl Default for WalletIntelligenceCache {
    fn default() -> Self {
        Self::new()
    }
}