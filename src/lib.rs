/// Badger Trading Bot Library
/// 
/// A comprehensive Solana meme coin trading system with momentum-based signals

pub mod client;
pub mod algo;
pub mod util;
pub mod config;
pub mod execution;
pub mod momentum;

// Re-export common types for convenience
pub use config::Config;
pub use execution::{PositionManager, TradingClient, RiskManager, StrategyExecutor};
pub use momentum::{MomentumSignalProcessor, MomentumTracker, VolumeMetrics};