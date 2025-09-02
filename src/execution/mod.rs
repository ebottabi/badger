/// Execution system for automated trading

pub mod position;
pub mod trading;
pub mod strategy;
pub mod risk;
pub mod portfolio;
pub mod monitor;

pub use position::{Position, PositionStatus, PositionManager};
pub use trading::TradingClient;
pub use strategy::StrategyExecutor;
pub use risk::RiskManager;
pub use portfolio::PortfolioTracker;
pub use monitor::PositionMonitor;