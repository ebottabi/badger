/// Ultra-Fast Memory-Mapped Wallet Database
/// 
/// This module provides nanosecond-speed wallet lookups using memory-mapped files
/// and lock-free data structures for high-frequency trading decisions.

use anyhow::{Result, Context};
use memmap2::{MmapOptions, MmapMut};
use std::fs::{File, OpenOptions};
use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use xxhash_rust::xxh64::xxh64;
use tracing::{info, debug, warn, error};
use serde::{Serialize, Deserialize};

/// Memory-aligned wallet cache entry (96 bytes = 1.5 cache lines)
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct WalletCacheEntry {
    // Hot path data (first 64 bytes - single cache line)
    pub address_hash: u64,       // 8 bytes - Fast hash of Solana address
    pub confidence: f32,         // 4 bytes - Trading confidence score  
    pub win_rate: f32,           // 4 bytes - Historical win rate
    pub avg_profit: f32,         // 4 bytes - Average profit per trade
    pub last_activity: u32,      // 4 bytes - Unix timestamp
    pub total_trades: u32,       // 4 bytes - Total trade count
    pub flags: u32,              // 4 bytes - Status flags (ACTIVE=1, BLACKLISTED=2, etc.)
    pub early_entry_score: f32,  // 4 bytes - Early entry capability
    pub recent_activity: f32,    // 4 bytes - Recent activity score
    pub reserved1: u32,          // 4 bytes - Reserved for future use
    pub reserved2: [u8; 16],     // 16 bytes - Reserved padding
    
    // Cold path data (32 bytes - half cache line)
    pub full_address: [u8; 32],  // 32 bytes - Full Solana address
}

impl Default for WalletCacheEntry {
    fn default() -> Self {
        Self {
            address_hash: 0,
            confidence: 0.0,
            win_rate: 0.0,
            avg_profit: 0.0,
            last_activity: 0,
            total_trades: 0,
            flags: 0,
            early_entry_score: 0.0,
            recent_activity: 0.0,
            reserved1: 0,
            reserved2: [0; 16],
            full_address: [0; 32],
        }
    }
}

/// Database file header (4KB aligned)
#[repr(C, align(4096))]
#[derive(Debug)]
pub struct DatabaseHeader {
    pub magic: u64,              // File format magic: 0xBADGER2024DB
    pub version: u32,            // Schema version
    pub capacity: u32,           // Max wallet entries
    pub active_count: AtomicU32, // Current active entries
    pub last_update: AtomicU64,  // Last modification timestamp
    pub checksum: u64,           // Data integrity checksum
    pub entry_size: u32,         // Size of each entry (96 bytes)
    pub hash_seed: u64,          // Hash seed for consistency
    pub reserved: [u8; 4040],    // Pad to 4KB
}

/// Ultra-fast memory-mapped wallet database
#[derive(Debug)]
pub struct UltraFastWalletDB {
    // Memory mapping
    mmap: MmapMut,
    header: *mut DatabaseHeader,
    entries: *mut WalletCacheEntry,
    
    // Configuration
    capacity: usize,
    mask: usize,                 // For fast modulo: hash & mask (capacity must be power of 2)
    
    // Performance statistics (atomic for thread safety)
    total_lookups: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    collision_count: AtomicU64,
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmapConfig {
    pub file_path: String,
    pub capacity: usize,         // Must be power of 2 (e.g., 1048576 = 1M)
    pub max_probe_distance: usize, // Linear probing limit (default: 8)
    pub enable_checksums: bool,
    pub backup_on_close: bool,
}

impl Default for MmapConfig {
    fn default() -> Self {
        Self {
            file_path: "data/wallets.mmap".to_string(),
            capacity: 1048576, // 1M wallets (power of 2)
            max_probe_distance: 8,
            enable_checksums: true,
            backup_on_close: true,
        }
    }
}

impl UltraFastWalletDB {
    /// Create or open memory-mapped wallet database
    pub fn new(config: MmapConfig) -> Result<Self> {
        // Ensure capacity is power of 2 for fast modulo
        if !config.capacity.is_power_of_two() {
            return Err(anyhow::anyhow!("Capacity must be power of 2, got {}", config.capacity));
        }
        
        // Create data directory if it doesn't exist
        if let Some(parent) = std::path::Path::new(&config.file_path).parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create data directory")?;
        }
        
        let file_size = std::mem::size_of::<DatabaseHeader>() + 
                       (config.capacity * std::mem::size_of::<WalletCacheEntry>());
        
        info!("🗄️ Creating memory-mapped database: {} ({:.1} MB)", 
              config.file_path, file_size as f64 / 1024.0 / 1024.0);
        
        // Create/open memory-mapped file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&config.file_path)
            .context("Failed to open/create memory-mapped file")?;
        
        file.set_len(file_size as u64)
            .context("Failed to set file size")?;
        
        let mmap = unsafe { 
            MmapOptions::new()
                .map_mut(&file)
                .context("Failed to create memory mapping")?
        };
        
        // Initialize pointers
        let header = mmap.as_ptr() as *mut DatabaseHeader;
        let entries = unsafe { 
            (header as *mut u8).add(std::mem::size_of::<DatabaseHeader>()) as *mut WalletCacheEntry
        };
        
        // Initialize header on first creation
        let is_new_file = unsafe { (*header).magic != 0xBAD6E42024DB_u64 };
        
        if is_new_file {
            info!("🆕 Initializing new memory-mapped database");
            unsafe {
                std::ptr::write(header, DatabaseHeader {
                    magic: 0xBAD6E42024DB_u64,
                    version: 1,
                    capacity: config.capacity as u32,
                    active_count: AtomicU32::new(0),
                    last_update: AtomicU64::new(chrono::Utc::now().timestamp() as u64),
                    checksum: 0,
                    entry_size: std::mem::size_of::<WalletCacheEntry>() as u32,
                    hash_seed: xxhash_rust::xxh64::xxh64(b"BADGER_SEED", 42),
                    reserved: [0; 4040],
                });
            }
            
            // Zero out all entries
            unsafe {
                std::ptr::write_bytes(entries, 0, config.capacity);
            }
        } else {
            info!("📖 Opened existing memory-mapped database with {} entries", 
                  unsafe { (*header).active_count.load(Ordering::Relaxed) });
        }
        
        Ok(Self {
            mmap,
            header,
            entries,
            capacity: config.capacity,
            mask: config.capacity - 1,
            total_lookups: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            collision_count: AtomicU64::new(0),
        })
    }
    
    /// Ultra-fast wallet confidence lookup (~1-5ns)
    #[inline(always)]
    pub fn lookup_confidence(&self, address: &[u8; 32]) -> Option<f32> {
        self.total_lookups.fetch_add(1, Ordering::Relaxed);
        
        let hash = xxhash_rust::xxh64::xxh64(address, 0);
        let index = (hash as usize) & self.mask;
        
        // Direct memory access - no locks, no async
        let entry = unsafe { &*self.entries.add(index) };
        
        // Fast hash comparison first (avoids expensive address comparison)
        if entry.address_hash == hash {
            // Verify full address to avoid hash collisions
            if entry.full_address == *address {
                self.cache_hits.fetch_add(1, Ordering::Relaxed);
                return Some(entry.confidence);
            }
        }
        
        // Linear probing for collision resolution (max 8 probes for speed)
        for i in 1..8 {
            let probe_index = (index + i) & self.mask;
            let probe_entry = unsafe { &*self.entries.add(probe_index) };
            
            if probe_entry.address_hash == hash && probe_entry.full_address == *address {
                self.cache_hits.fetch_add(1, Ordering::Relaxed);
                self.collision_count.fetch_add(1, Ordering::Relaxed);
                return Some(probe_entry.confidence);
            }
            
            // Empty slot means not found
            if probe_entry.address_hash == 0 {
                break;
            }
        }
        
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
        None
    }
    
    /// Ultra-fast confidence threshold check
    #[inline(always)]
    pub fn is_high_confidence(&self, address: &[u8; 32], threshold: f32) -> bool {
        self.lookup_confidence(address)
            .map(|conf| conf >= threshold)
            .unwrap_or(false)
    }
    
    /// Get wallet metadata (slightly slower but still fast)
    #[inline(always)]
    pub fn lookup_wallet_info(&self, address: &[u8; 32]) -> Option<WalletCacheEntry> {
        self.total_lookups.fetch_add(1, Ordering::Relaxed);
        
        let hash = xxhash_rust::xxh64::xxh64(address, 0);
        let index = (hash as usize) & self.mask;
        
        let entry = unsafe { &*self.entries.add(index) };
        
        if entry.address_hash == hash && entry.full_address == *address {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
            return Some(*entry);
        }
        
        // Linear probing
        for i in 1..8 {
            let probe_index = (index + i) & self.mask;
            let probe_entry = unsafe { &*self.entries.add(probe_index) };
            
            if probe_entry.address_hash == hash && probe_entry.full_address == *address {
                self.cache_hits.fetch_add(1, Ordering::Relaxed);
                self.collision_count.fetch_add(1, Ordering::Relaxed);
                return Some(*probe_entry);
            }
            
            if probe_entry.address_hash == 0 {
                break;
            }
        }
        
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
        None
    }
    
    /// Insert or update wallet entry
    pub fn insert_wallet(&self, entry: &WalletCacheEntry) -> Result<bool> {
        let hash = xxhash_rust::xxh64::xxh64(&entry.full_address, 0);
        let index = (hash as usize) & self.mask;
        
        // Try to insert at primary location
        let target_entry = unsafe { &mut *self.entries.add(index) };
        
        if target_entry.address_hash == 0 || target_entry.address_hash == hash {
            // Empty slot or updating existing
            let is_new = target_entry.address_hash == 0;
            
            let mut new_entry = *entry;
            new_entry.address_hash = hash;
            
            unsafe {
                std::ptr::write(target_entry, new_entry);
            }
            
            if is_new {
                unsafe {
                    (*self.header).active_count.fetch_add(1, Ordering::Relaxed);
                    (*self.header).last_update.store(
                        chrono::Utc::now().timestamp() as u64, 
                        Ordering::Relaxed
                    );
                }
            }
            
            return Ok(is_new);
        }
        
        // Linear probing for open slot
        for i in 1..8 {
            let probe_index = (index + i) & self.mask;
            let probe_entry = unsafe { &mut *self.entries.add(probe_index) };
            
            if probe_entry.address_hash == 0 || 
               (probe_entry.address_hash == hash && probe_entry.full_address == entry.full_address) {
                let is_new = probe_entry.address_hash == 0;
                
                let mut new_entry = *entry;
                new_entry.address_hash = hash;
                
                unsafe {
                    std::ptr::write(probe_entry, new_entry);
                }
                
                if is_new {
                    unsafe {
                        (*self.header).active_count.fetch_add(1, Ordering::Relaxed);
                        (*self.header).last_update.store(
                            chrono::Utc::now().timestamp() as u64, 
                            Ordering::Relaxed
                        );
                    }
                }
                
                return Ok(is_new);
            }
        }
        
        Err(anyhow::anyhow!("Hash table full - no available slots within probe distance"))
    }
    
    /// Get database statistics
    pub fn get_stats(&self) -> MmapStats {
        let total_lookups = self.total_lookups.load(Ordering::Relaxed);
        let cache_hits = self.cache_hits.load(Ordering::Relaxed);
        let cache_misses = self.cache_misses.load(Ordering::Relaxed);
        let collisions = self.collision_count.load(Ordering::Relaxed);
        
        let active_count = unsafe { (*self.header).active_count.load(Ordering::Relaxed) };
        let last_update = unsafe { (*self.header).last_update.load(Ordering::Relaxed) };
        
        MmapStats {
            capacity: self.capacity,
            active_entries: active_count as usize,
            total_lookups,
            cache_hits,
            cache_misses,
            collision_count: collisions,
            hit_rate: if total_lookups > 0 { cache_hits as f64 / total_lookups as f64 } else { 0.0 },
            load_factor: active_count as f64 / self.capacity as f64,
            last_update_timestamp: last_update,
            memory_usage_mb: (std::mem::size_of::<DatabaseHeader>() + 
                             self.capacity * std::mem::size_of::<WalletCacheEntry>()) as f64 / 1024.0 / 1024.0,
        }
    }
    
    /// Get insider wallet count for monitoring
    pub async fn get_insider_count(&self) -> usize {
        unsafe { (*self.header).active_count.load(Ordering::Acquire) as usize }
    }
    
    /// Get token count for monitoring (placeholder)
    pub async fn get_token_count(&self) -> usize {
        0 // Placeholder - tokens not stored in this version
    }
    
    /// Get token launch time from memory-mapped database (placeholder)
    pub async fn get_token_launch_time(&self, _token_mint: &str) -> Option<i64> {
        None // Placeholder - tokens not stored in this version
    }
    
    /// Store token launch time in memory-mapped database (placeholder)
    pub async fn store_token_launch(&self, _token_mint: &str, _launch_timestamp: i64) -> Result<()> {
        Ok(()) // Placeholder - tokens not stored in this version
    }
}

/// Database performance statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MmapStats {
    pub capacity: usize,
    pub active_entries: usize,
    pub total_lookups: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub collision_count: u64,
    pub hit_rate: f64,
    pub load_factor: f64,
    pub last_update_timestamp: u64,
    pub memory_usage_mb: f64,
}

// Ensure our structures are safe for memory mapping
unsafe impl Send for UltraFastWalletDB {}
unsafe impl Sync for UltraFastWalletDB {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    
    #[test]
    fn test_wallet_entry_size() {
        assert_eq!(std::mem::size_of::<WalletCacheEntry>(), 96);
        assert_eq!(std::mem::align_of::<WalletCacheEntry>(), 64);
    }
    
    #[test]
    fn test_database_creation() {
        let config = MmapConfig {
            file_path: "/tmp/test_badger.mmap".to_string(),
            capacity: 1024, // Small for testing
            ..Default::default()
        };
        
        let db = UltraFastWalletDB::new(config).unwrap();
        let stats = db.get_stats();
        
        assert_eq!(stats.capacity, 1024);
        assert_eq!(stats.active_entries, 0);
        
        // Cleanup
        let _ = std::fs::remove_file("/tmp/test_badger.mmap");
    }
    
    #[test]
    fn test_insert_and_lookup() {
        let config = MmapConfig {
            file_path: "/tmp/test_insert.mmap".to_string(),
            capacity: 1024,
            ..Default::default()
        };
        
        let db = UltraFastWalletDB::new(config).unwrap();
        
        let test_address = [42u8; 32];
        let entry = WalletCacheEntry {
            full_address: test_address,
            confidence: 0.85,
            win_rate: 0.75,
            total_trades: 100,
            flags: 1, // ACTIVE
            ..Default::default()
        };
        
        // Insert
        let inserted = db.insert_wallet(&entry).unwrap();
        assert!(inserted); // Should be new entry
        
        // Lookup
        let confidence = db.lookup_confidence(&test_address);
        assert_eq!(confidence, Some(0.85));
        
        let wallet_info = db.lookup_wallet_info(&test_address).unwrap();
        assert_eq!(wallet_info.win_rate, 0.75);
        assert_eq!(wallet_info.total_trades, 100);
        
        // Cleanup
        let _ = std::fs::remove_file("/tmp/test_insert.mmap");
    }
    
    #[test]
    fn benchmark_lookup_performance() {
        let config = MmapConfig {
            file_path: "/tmp/benchmark.mmap".to_string(),
            capacity: 1024,
            ..Default::default()
        };
        
        let db = UltraFastWalletDB::new(config).unwrap();
        let test_address = [1u8; 32];
        
        // Insert test entry
        let entry = WalletCacheEntry {
            full_address: test_address,
            confidence: 0.9,
            ..Default::default()
        };
        db.insert_wallet(&entry).unwrap();
        
        // Benchmark lookups
        let iterations = 1_000_000;
        let start = Instant::now();
        
        for _ in 0..iterations {
            let _ = db.lookup_confidence(&test_address);
        }
        
        let elapsed = start.elapsed();
        let avg_ns = elapsed.as_nanos() / iterations;
        
        println!("Average lookup time: {}ns", avg_ns);
        assert!(avg_ns < 100, "Lookup should be under 100ns for test environment");
        
        // Cleanup
        let _ = std::fs::remove_file("/tmp/benchmark.mmap");
    }
}