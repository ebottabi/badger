use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::{SignalSource, DexType};

/// Enhanced trading signals with comprehensive metadata for production trading
/// 
/// These signals provide detailed information needed for automated trading
/// decisions including confidence scores, risk assessment, urgency levels,
/// and execution parameters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EnhancedTradingSignal {
    /// Buy signal with comprehensive execution parameters
    Buy {
        token_mint: String,
        confidence: f64,
        max_amount_sol: f64,
        reason: String,
        source: SignalSource,
        urgency: SignalUrgency,
        risk_level: RiskLevel,
        expected_roi: Option<f64>,
        time_horizon_minutes: u32,
        stop_loss_percentage: Option<f64>,
        take_profit_percentage: Option<f64>,
        max_slippage_percentage: f64,
        preferred_dex: Option<DexType>,
        execution_strategy: ExecutionStrategy,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        signal_id: String,
    },
    /// Sell signal with detailed exit parameters
    Sell {
        token_mint: String,
        position_size_sol: f64,
        target_price: Option<f64>,
        stop_loss_price: Option<f64>,
        reason: String,
        urgency: SignalUrgency,
        sell_strategy: SellStrategy,
        max_slippage_percentage: f64,
        partial_sell_percentage: Option<f64>, // For partial exits
        preferred_dex: Option<DexType>,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        signal_id: String,
    },
    /// Hold signal with review parameters
    Hold {
        token_mint: String,
        position_size_sol: f64,
        reason: String,
        review_time_minutes: u32,
        watch_levels: Vec<PriceLevel>,
        risk_monitoring: RiskMonitoring,
        created_at: DateTime<Utc>,
        next_review_at: DateTime<Utc>,
        signal_id: String,
    },
    /// Alert signal for manual intervention
    Alert {
        message: String,
        alert_type: AlertType,
        severity: AlertSeverity,
        requires_action: bool,
        action_deadline: Option<DateTime<Utc>>,
        related_tokens: Vec<String>,
        related_wallets: Vec<String>,
        evidence: Vec<AlertEvidence>,
        created_at: DateTime<Utc>,
        signal_id: String,
    },
    /// Copy trade signal from insider wallet activity
    CopyTrade {
        insider_wallet: String,
        insider_action: InsiderAction,
        token_mint: String,
        insider_amount_sol: f64,
        copy_percentage: f64, // What % of insider position to copy
        confidence: f64,
        insider_success_rate: f64,
        max_copy_amount_sol: f64,
        delay_seconds: u32, // Delay to avoid MEV
        reason: String,
        urgency: SignalUrgency,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        signal_id: String,
    },
    /// Risk warning signal
    RiskWarning {
        token_mint: String,
        risk_type: RiskType,
        risk_level: RiskLevel,
        description: String,
        recommended_action: RecommendedAction,
        evidence: Vec<RiskEvidence>,
        confidence: f64,
        immediate_action_required: bool,
        created_at: DateTime<Utc>,
        signal_id: String,
    },
}

/// Signal urgency levels for execution prioritization
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SignalUrgency {
    /// Low priority - execute within 5-10 minutes
    Low,
    /// Medium priority - execute within 1-2 minutes
    Medium,
    /// High priority - execute within 10-30 seconds
    High,
    /// Critical priority - execute immediately (< 10 seconds)
    Critical,
}

/// Risk levels for position sizing and execution decisions
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum RiskLevel {
    /// Very low risk - use normal position sizes
    VeryLow,
    /// Low risk - slight reduction in position size
    Low,
    /// Medium risk - moderate position size reduction
    Medium,
    /// High risk - significant position size reduction
    High,
    /// Very high risk - minimal position size or avoid
    VeryHigh,
}

/// Execution strategies for buy orders
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStrategy {
    /// Execute immediately at market price
    Market,
    /// Split order into smaller chunks over time (TWAP)
    TWAP,
    /// Wait for better price within time limit
    Limit,
    /// Use dollar cost averaging approach
    DCA,
    /// Stealth execution to avoid MEV
    Stealth,
}

/// Sell strategies for exit orders
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SellStrategy {
    /// Sell everything immediately
    Market,
    /// Gradual exit to avoid price impact
    Gradual,
    /// Sell based on technical levels
    LevelBased,
    /// Sell based on time targets
    TimeBased,
    /// Emergency exit (rug pull detected)
    Emergency,
}

/// Alert types for different scenarios
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum AlertType {
    /// New opportunity detected
    Opportunity,
    /// Risk detected in current position
    Risk,
    /// System or execution error
    Error,
    /// Market anomaly detected
    Anomaly,
    /// Insider activity detected
    InsiderActivity,
    /// Large whale movement
    WhaleActivity,
    /// Coordinated activity detected
    CoordinatedActivity,
}

/// Alert severity levels
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum AlertSeverity {
    /// Informational only
    Info,
    /// Warning - attention recommended
    Warning,
    /// Error - action needed
    Error,
    /// Critical - immediate action required
    Critical,
}

/// Types of risks that can be detected
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum RiskType {
    /// Honeypot contract detected
    Honeypot,
    /// Potential rug pull indicators
    RugPull,
    /// High price impact trades
    PriceImpact,
    /// Low liquidity warning
    LowLiquidity,
    /// Wash trading detected
    WashTrading,
    /// Bot activity detected
    BotActivity,
    /// Unusual trading patterns
    UnusualPatterns,
    /// Technical analysis risk
    Technical,
}

/// Recommended actions for risk warnings
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum RecommendedAction {
    /// Hold current position
    Hold,
    /// Reduce position size
    Reduce,
    /// Exit position completely
    Exit,
    /// Avoid entering position
    Avoid,
    /// Monitor closely
    Monitor,
}

/// Types of insider actions to copy
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum InsiderAction {
    /// Insider buying early in token launch
    EarlyBuy,
    /// Insider taking large position
    LargeBuy,
    /// Insider adding liquidity
    LiquidityAdd,
    /// Insider selling (warning signal)
    Sell,
    /// Insider removing liquidity (warning)
    LiquidityRemove,
}

/// Price levels to monitor for hold signals
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PriceLevel {
    pub price: f64,
    pub level_type: PriceLevelType,
    pub action: PriceLevelAction,
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum PriceLevelType {
    Support,
    Resistance,
    StopLoss,
    TakeProfit,
    Entry,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum PriceLevelAction {
    Buy,
    Sell,
    Alert,
    Hold,
}

/// Risk monitoring parameters for hold signals
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RiskMonitoring {
    pub max_drawdown_percentage: f64,
    pub volume_threshold_change: f64,
    pub price_volatility_threshold: f64,
    pub monitor_insider_activity: bool,
    pub monitor_whale_activity: bool,
    pub auto_exit_on_rug_pull: bool,
}

/// Evidence for alert signals
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlertEvidence {
    pub evidence_type: EvidenceType,
    pub description: String,
    pub data: String, // JSON string with specific data
    pub confidence: f64,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum EvidenceType {
    Transaction,
    WalletActivity,
    PriceMovement,
    VolumeSpike,
    LiquidityChange,
    ContractInteraction,
}

/// Evidence for risk warnings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RiskEvidence {
    pub risk_indicator: String,
    pub value: f64,
    pub threshold: f64,
    pub severity: AlertSeverity,
    pub description: String,
    pub detected_at: DateTime<Utc>,
}

impl EnhancedTradingSignal {
    /// Get the signal ID for tracking
    pub fn signal_id(&self) -> &str {
        match self {
            EnhancedTradingSignal::Buy { signal_id, .. } => signal_id,
            EnhancedTradingSignal::Sell { signal_id, .. } => signal_id,
            EnhancedTradingSignal::Hold { signal_id, .. } => signal_id,
            EnhancedTradingSignal::Alert { signal_id, .. } => signal_id,
            EnhancedTradingSignal::CopyTrade { signal_id, .. } => signal_id,
            EnhancedTradingSignal::RiskWarning { signal_id, .. } => signal_id,
        }
    }
    
    /// Get the creation timestamp
    pub fn created_at(&self) -> DateTime<Utc> {
        match self {
            EnhancedTradingSignal::Buy { created_at, .. } => *created_at,
            EnhancedTradingSignal::Sell { created_at, .. } => *created_at,
            EnhancedTradingSignal::Hold { created_at, .. } => *created_at,
            EnhancedTradingSignal::Alert { created_at, .. } => *created_at,
            EnhancedTradingSignal::CopyTrade { created_at, .. } => *created_at,
            EnhancedTradingSignal::RiskWarning { created_at, .. } => *created_at,
        }
    }
    
    /// Get the urgency level
    pub fn urgency(&self) -> Option<SignalUrgency> {
        match self {
            EnhancedTradingSignal::Buy { urgency, .. } => Some(*urgency),
            EnhancedTradingSignal::Sell { urgency, .. } => Some(*urgency),
            EnhancedTradingSignal::CopyTrade { urgency, .. } => Some(*urgency),
            _ => None,
        }
    }
    
    /// Check if signal has expired
    pub fn is_expired(&self) -> bool {
        let now = Utc::now();
        match self {
            EnhancedTradingSignal::Buy { expires_at, .. } => now > *expires_at,
            EnhancedTradingSignal::Sell { expires_at, .. } => now > *expires_at,
            EnhancedTradingSignal::CopyTrade { expires_at, .. } => now > *expires_at,
            _ => false,
        }
    }
    
    /// Get related token mint if applicable
    pub fn token_mint(&self) -> Option<&str> {
        match self {
            EnhancedTradingSignal::Buy { token_mint, .. } => Some(token_mint),
            EnhancedTradingSignal::Sell { token_mint, .. } => Some(token_mint),
            EnhancedTradingSignal::Hold { token_mint, .. } => Some(token_mint),
            EnhancedTradingSignal::CopyTrade { token_mint, .. } => Some(token_mint),
            EnhancedTradingSignal::RiskWarning { token_mint, .. } => Some(token_mint),
            EnhancedTradingSignal::Alert { related_tokens, .. } => {
                related_tokens.first().map(|s| s.as_str())
            }
        }
    }
    
    /// Generate a unique signal ID
    pub fn generate_signal_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let random: u32 = rand::random();
        format!("signal_{}_{}", timestamp, random)
    }
}

impl From<crate::core::TradingSignal> for EnhancedTradingSignal {
    fn from(signal: crate::core::TradingSignal) -> Self {
        let now = Utc::now();
        let signal_id = EnhancedTradingSignal::generate_signal_id();
        
        match signal {
            crate::core::TradingSignal::Buy { token_mint, confidence, max_amount_sol, reason, source } => {
                EnhancedTradingSignal::Buy {
                    token_mint,
                    confidence,
                    max_amount_sol,
                    reason,
                    source,
                    urgency: if confidence > 0.8 { SignalUrgency::High } else { SignalUrgency::Medium },
                    risk_level: if confidence > 0.7 { RiskLevel::Medium } else { RiskLevel::High },
                    expected_roi: None,
                    time_horizon_minutes: 60, // Default 1 hour
                    stop_loss_percentage: Some(20.0), // Default 20% stop loss
                    take_profit_percentage: Some(100.0), // Default 100% take profit
                    max_slippage_percentage: 5.0, // Default 5% max slippage
                    preferred_dex: None,
                    execution_strategy: ExecutionStrategy::Market,
                    created_at: now,
                    expires_at: now + chrono::Duration::hours(1),
                    signal_id,
                }
            }
            crate::core::TradingSignal::Sell { token_mint, price_target, stop_loss, reason } => {
                EnhancedTradingSignal::Sell {
                    token_mint,
                    position_size_sol: 0.0, // Unknown from basic signal
                    target_price: Some(price_target),
                    stop_loss_price: Some(stop_loss),
                    reason,
                    urgency: SignalUrgency::Medium,
                    sell_strategy: SellStrategy::Market,
                    max_slippage_percentage: 5.0,
                    partial_sell_percentage: None,
                    preferred_dex: None,
                    created_at: now,
                    expires_at: now + chrono::Duration::hours(1),
                    signal_id,
                }
            }
            crate::core::TradingSignal::SwapActivity { token_mint, volume_increase, whale_activity } => {
                EnhancedTradingSignal::Alert {
                    message: format!("Swap activity detected: {}% volume increase, whale: {}", 
                                   volume_increase * 100.0, whale_activity),
                    alert_type: if whale_activity { AlertType::WhaleActivity } else { AlertType::Opportunity },
                    severity: if whale_activity { AlertSeverity::Warning } else { AlertSeverity::Info },
                    requires_action: whale_activity,
                    action_deadline: if whale_activity { Some(now + chrono::Duration::minutes(5)) } else { None },
                    related_tokens: vec![token_mint],
                    related_wallets: Vec::new(),
                    evidence: Vec::new(),
                    created_at: now,
                    signal_id,
                }
            }
        }
    }
}

impl Default for RiskMonitoring {
    fn default() -> Self {
        Self {
            max_drawdown_percentage: 25.0,
            volume_threshold_change: 200.0, // 200% volume increase threshold
            price_volatility_threshold: 50.0, // 50% price volatility threshold
            monitor_insider_activity: true,
            monitor_whale_activity: true,
            auto_exit_on_rug_pull: true,
        }
    }
}