/// Core Database Module
/// 
/// Memory-mapped database implementation for ultra-fast wallet intelligence
/// and insider detection with nanosecond lookup speeds.

pub mod mmap_db;
pub mod hash_utils;

// Re-export main types
pub use mmap_db::{UltraFastWalletDB, MmapConfig, WalletCacheEntry};
pub use hash_utils::*;