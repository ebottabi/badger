// Core modules
pub mod core;

// Data ingestion modules  
pub mod ingest;

// Transport and communication modules
pub mod transport;

// Database and persistence modules (Phase 3)
pub mod database;

// Re-export commonly used types for convenience
pub use core::*;
pub use ingest::SolanaWebSocketClient;
pub use transport::*;
pub use database::DatabaseManager;