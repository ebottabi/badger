// Core modules
pub mod core;

// Data ingestion modules  
pub mod ingest;

// Transport and communication modules
pub mod transport;

// Token scanning modules (Scout)
pub mod scout;

// Wallet monitoring modules (Stalker)  
pub mod stalker;

// Trade execution modules (Strike)
pub mod strike;

// Database modules removed - using memory-mapped files only

// Handler modules for clean component management
pub mod handlers;

// Re-export commonly used types for convenience
pub use core::*;
pub use ingest::SolanaWebSocketClient;
pub use transport::*;