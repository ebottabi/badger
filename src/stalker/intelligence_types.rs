/// Core data types for the wallet intelligence system
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicU64;
use chrono::{DateTime, Utc};

/// Insider wallet information stored in memory cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsiderWallet {
    /// Wallet address (primary key)
    pub address: String,
    
    /// Overall confidence score (0.0-1.0) - higher = better insider
    pub confidence_score: f64,
    
    /// Win rate percentage (0.0-1.0) - profitable trades / total trades
    pub win_rate: f64,
    
    /// Average profit percentage on winning trades (e.g., 0.45 = 45%)
    pub avg_profit_percentage: f64,
    
    /// Early entry score (0.0-100.0) - how fast they enter after launch
    pub early_entry_score: f64,
    
    /// Total number of trades tracked
    pub total_trades: u32,
    
    /// Number of profitable trades
    pub profitable_trades: u32,
    
    /// Timestamp of last trade activity
    pub last_trade_timestamp: i64,
    
    /// Timestamp when we first detected this wallet
    pub first_detected_timestamp: i64,
    
    /// Recent activity score (weighted recent performance)
    pub recent_activity_score: f64,
    
    /// Current wallet status
    pub status: WalletStatus,
}

/// Wallet status for filtering and decision making
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WalletStatus {
    /// Currently copying trades from this wallet
    Active,
    
    /// Watching performance before promoting to active
    Monitoring,
    
    /// Poor performance, ignore trades from this wallet
    Blacklisted,
    
    /// Temporary pause after losses
    Cooldown,
}

/// Copy trading decision from memory cache lookup
#[derive(Debug, Clone)]
pub struct CopyDecision {
    /// Whether to copy the trade
    pub should_copy: bool,
    
    /// Confidence score of the insider wallet
    pub confidence: f64,
    
    /// Position size in SOL for this trade
    pub position_size: f64,
    
    /// Delay in seconds before executing copy trade
    pub delay_seconds: u32,
    
    /// Signal urgency level
    pub urgency: SignalUrgency,
}

/// Copy trading signal generated for execution
#[derive(Debug, Clone)]
pub struct CopyTradingSignal {
    /// Address of the insider wallet we're copying
    pub insider_wallet: String,
    
    /// Token mint address being traded
    pub token_mint: String,
    
    /// Type of signal (buy or sell)
    pub signal_type: CopySignalType,
    
    /// Confidence score of the insider wallet
    pub insider_confidence: f64,
    
    /// Position size in SOL
    pub position_size_sol: f64,
    
    /// Delay before execution
    pub copy_delay_seconds: u32,
    
    /// Priority level for execution
    pub urgency: SignalUrgency,
    
    /// Timestamp when signal was generated
    pub timestamp: i64,
}

/// Type of copy trading signal
#[derive(Debug, Clone)]
pub enum CopySignalType {
    /// Buy signal when insider enters position
    Buy {
        /// Price at which insider bought
        insider_entry_price: f64,
        /// Minutes after token launch when insider bought
        token_launch_delay_minutes: u32,
    },
    
    /// Sell signal when insider exits position
    Sell {
        /// Price at which insider sold
        insider_exit_price: f64,
        /// Insider's profit percentage on this trade
        insider_profit_percentage: f64,
    },
}

/// Signal urgency for execution prioritization
#[derive(Debug, Clone)]
pub enum SignalUrgency {
    /// Execute within 5 seconds - highest confidence insiders
    Immediate,
    
    /// Execute within 15 seconds - good insiders
    High,
    
    /// Execute within 30 seconds - normal execution
    Normal,
    
    /// Execute within 60 seconds - low confidence
    Low,
}

/// Trade data for background processing
#[derive(Debug, Clone)]
pub struct TradeData {
    /// Amount in SOL
    pub amount_sol: f64,
    
    /// Price per token
    pub price: f64,
    
    /// Trade timestamp
    pub timestamp: i64,
    
    /// Trade type (BUY or SELL)
    pub trade_type: String,
}

/// Background update message types
#[derive(Debug, Clone)]
pub enum BackgroundUpdate {
    /// New insider trade detected
    InsiderTrade {
        wallet: String,
        token: String,
        trade_data: TradeData,
    },
    
    /// Token launch detected
    TokenLaunched {
        token_mint: String,
        launch_timestamp: i64,
    },
    
    /// Copy trade result for performance tracking
    CopyTradeResult {
        copy_signal_id: i64,
        result: CopyTradeResult,
    },
    
    /// Force cache refresh
    RefreshCache,
    
    /// Discover new insider wallets
    DiscoverInsiders,
}

/// Result of a copy trade for performance tracking
#[derive(Debug, Clone)]
pub struct CopyTradeResult {
    /// ID of the insider wallet
    pub insider_wallet: String,
    
    /// Token that was traded
    pub token_mint: String,
    
    /// Our entry price
    pub our_entry_price: Option<f64>,
    
    /// Our exit price
    pub our_exit_price: Option<f64>,
    
    /// Profit/loss in SOL
    pub profit_loss_sol: Option<f64>,
    
    /// Profit percentage
    pub profit_percentage: Option<f64>,
    
    /// How long we held the position
    pub hold_duration_seconds: Option<i64>,
    
    /// Final result
    pub result: TradeResult,
    
    /// Why we exited
    pub exit_reason: ExitReason,
}

/// Trade result classification
#[derive(Debug, Clone, PartialEq)]
pub enum TradeResult {
    /// Profitable trade
    Win,
    
    /// Losing trade
    Loss,
    
    /// Trade still open
    Pending,
}

/// Reason for exiting a position
#[derive(Debug, Clone)]
pub enum ExitReason {
    /// Insider sold, we followed
    InsiderExit,
    
    /// Hit our profit target
    TakeProfit,
    
    /// Hit our stop loss
    StopLoss,
    
    /// Position held too long
    TimeDecay,
    
    /// Manual exit
    Manual,
}

/// Fresh insider score calculation from database
#[derive(Debug, Clone)]
pub struct FreshInsiderScore {
    /// Updated confidence score
    pub confidence: f64,
    
    /// Updated win rate
    pub win_rate: f64,
    
    /// Updated average profit
    pub avg_profit: f64,
    
    /// Updated recent activity score
    pub recent_activity: f64,
}

/// Cache performance statistics
#[derive(Debug, Clone)]
pub struct CacheStatistics {
    /// Total insider wallets in cache
    pub insider_count: usize,
    
    /// Active wallets being copied
    pub active_count: usize,
    
    /// Blacklisted wallets
    pub blacklisted_count: usize,
    
    /// Total cache lookups performed
    pub total_lookups: u64,
    
    /// Cache hit rate percentage
    pub hit_rate: f64,
    
    /// Last cache update timestamp
    pub last_update: i64,
    
    /// Memory usage in bytes (approximate)
    pub memory_usage_bytes: usize,
}

/// Token launch information for age calculations
#[derive(Debug, Clone)]
pub struct TokenLaunchInfo {
    /// Token mint address
    pub mint_address: String,
    
    /// Launch timestamp
    pub launch_timestamp: i64,
}

/// Wallet discovery candidate information
#[derive(Debug, Clone)]
pub struct WalletCandidate {
    /// Wallet address
    pub address: String,
    
    /// Initial confidence score
    pub initial_confidence: f64,
    
    /// Method used to discover this wallet
    pub discovery_method: DiscoveryMethod,
    
    /// Supporting trade data
    pub qualifying_trades: Vec<TradeData>,
}

/// Method used to discover insider wallets
#[derive(Debug, Clone)]
pub enum DiscoveryMethod {
    /// Found through early entry pattern
    EarlyEntry,
    
    /// Found through high profit pattern
    HighProfit,
    
    /// Found through pattern matching
    PatternMatch,
    
    /// Manually added
    Manual,
}

impl Default for InsiderWallet {
    fn default() -> Self {
        Self {
            address: String::new(),
            confidence_score: 0.0,
            win_rate: 0.0,
            avg_profit_percentage: 0.0,
            early_entry_score: 0.0,
            total_trades: 0,
            profitable_trades: 0,
            last_trade_timestamp: 0,
            first_detected_timestamp: 0,
            recent_activity_score: 0.0,
            status: WalletStatus::Monitoring,
        }
    }
}

impl InsiderWallet {
    /// Check if this wallet meets insider criteria
    pub fn is_qualified_insider(&self) -> bool {
        self.win_rate >= 0.70 &&              // 70%+ win rate
        self.avg_profit_percentage >= 0.40 && // 40%+ average profit
        self.total_trades >= 10 &&            // Minimum trade history
        self.confidence_score >= 0.75         // High confidence
    }
    
    /// Check if wallet should be promoted from monitoring to active
    pub fn should_promote_to_active(&self) -> bool {
        self.status == WalletStatus::Monitoring &&
        self.is_qualified_insider() &&
        self.recent_activity_score > 0.5      // Recent activity required
    }
    
    /// Check if wallet should be blacklisted
    pub fn should_blacklist(&self) -> bool {
        self.win_rate < 0.30 ||               // Very poor win rate
        self.confidence_score < 0.25 ||       // Very low confidence
        (self.total_trades >= 20 && self.avg_profit_percentage < 0.10) // Many trades, poor profit
    }
    
    /// Calculate position size multiplier based on confidence
    pub fn position_size_multiplier(&self) -> f64 {
        (self.confidence_score * 2.0).min(2.0) // Max 2x multiplier
    }
    
    /// Calculate copy delay based on confidence (higher confidence = faster copy)
    pub fn copy_delay_seconds(&self) -> u32 {
        let base_delay = 5; // 5 seconds minimum
        let variable_delay = ((1.0 - self.confidence_score) * 25.0) as u32;
        base_delay + variable_delay
    }
}