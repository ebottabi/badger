use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::core::{DexType, SwapType};

/// Enhanced market events with comprehensive metadata for production trading
/// 
/// These events extend the basic MarketEvent types with additional data needed
/// for sophisticated trading strategies and insider detection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EnhancedMarketEvent {
    /// New liquidity pool created with comprehensive metadata
    PoolCreated {
        pool: EnhancedPoolInfo,
        creator: String,
        initial_liquidity_sol: f64,
        creation_tx: String,
        block_time: DateTime<Utc>,
        slot: u64,
    },
    /// Pool burned or LP tokens removed (rug pull indicator)
    PoolBurned {
        pool_address: String,
        burn_tx: String,
        tokens_burned: u64,
        remaining_liquidity_sol: f64,
        burn_reason: BurnReason,
        block_time: DateTime<Utc>,
        slot: u64,
    },
    /// New token launched with comprehensive metadata
    TokenLaunched {
        token: EnhancedTokenMetadata,
        launch_tx: String,
        first_pool_address: Option<String>,
        time_to_first_pool_seconds: Option<u64>,
        block_time: DateTime<Utc>,
        slot: u64,
    },
    /// Significant liquidity change in pool
    LiquidityChanged {
        pool_address: String,
        change_type: LiquidityChangeType,
        amount_sol: f64,
        new_total_sol: f64,
        provider_wallet: String,
        transaction_signature: String,
        price_impact: Option<f64>,
        block_time: DateTime<Utc>,
        slot: u64,
    },
    /// Large swap detected with enhanced metadata
    SwapDetected {
        swap: EnhancedSwapEvent,
        volume_rank: VolumeRank,
        wallet_history: WalletSwapHistory,
        market_impact: MarketImpact,
    },
    /// Large token transfer detected (potential insider activity)
    LargeTransferDetected {
        transfer: EnhancedLargeTransfer,
        sender_history: WalletTransferHistory,
        receiver_history: WalletTransferHistory,
        transfer_pattern: TransferPattern,
    },
    /// Multiple related events detected (potential coordinated activity)
    CoordinatedActivity {
        activity_type: CoordinatedActivityType,
        wallets_involved: Vec<String>,
        tokens_involved: Vec<String>,
        total_value_sol: f64,
        time_window_seconds: u64,
        confidence_score: f64,
        evidence: Vec<String>, // Transaction signatures
        block_time: DateTime<Utc>,
        slot: u64,
    },
    /// Whale activity detected
    WhaleActivity {
        wallet: String,
        action: WhaleAction,
        token_mint: String,
        amount_sol: f64,
        percentage_of_supply: Option<f64>,
        price_impact: Option<f64>,
        transaction_signature: String,
        block_time: DateTime<Utc>,
        slot: u64,
    },
    /// Token metadata updated (name, symbol, image changes)
    TokenMetadataUpdated {
        mint: String,
        old_metadata: EnhancedTokenMetadata,
        new_metadata: EnhancedTokenMetadata,
        update_authority: String,
        update_tx: String,
        block_time: DateTime<Utc>,
        slot: u64,
    },
}

/// Enhanced pool information with comprehensive trading data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnhancedPoolInfo {
    // Basic pool info
    pub address: String,
    pub base_mint: String,
    pub quote_mint: String,
    pub base_vault: String,
    pub quote_vault: String,
    pub lp_mint: String,
    pub market_id: Option<String>,
    pub dex: DexType,
    pub created_at: DateTime<Utc>,
    pub slot: u64,
    
    // Enhanced metadata
    pub pool_type: PoolType,
    pub fee_rate: Option<f64>,
    pub current_base_amount: u64,
    pub current_quote_amount: u64,
    pub current_price: Option<f64>,
    pub volume_24h_sol: Option<f64>,
    pub unique_traders_24h: Option<u32>,
    pub liquidity_locked: bool,
    pub lock_duration_days: Option<u32>,
    pub creator_wallet: String,
    pub creator_reputation: Option<f64>,
    pub risk_score: Option<f64>,
    pub tags: Vec<String>, // ["meme", "ai", "defi", etc.]
    pub verified: bool,
    pub audit_status: AuditStatus,
}

/// Enhanced token metadata with comprehensive information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnhancedTokenMetadata {
    // Basic token info
    pub mint: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub supply: u64,
    pub mint_authority: Option<String>,
    pub freeze_authority: Option<String>,
    pub is_mutable: bool,
    pub created_at: DateTime<Utc>,
    pub slot: u64,
    
    // Enhanced metadata
    pub description: Option<String>,
    pub image_uri: Option<String>,
    pub website: Option<String>,
    pub twitter: Option<String>,
    pub telegram: Option<String>,
    pub discord: Option<String>,
    pub holder_count: Option<u64>,
    pub top_holders: Vec<TokenHolder>,
    pub market_cap_sol: Option<f64>,
    pub fully_diluted_valuation_sol: Option<f64>,
    pub circulating_supply: Option<u64>,
    pub tags: Vec<String>,
    pub safety_score: Option<f64>,
    pub rug_pull_risk: RiskLevel,
    pub honeypot_risk: RiskLevel,
    pub verified_creator: bool,
    pub audit_reports: Vec<AuditReport>,
}

/// Enhanced swap event with comprehensive trading data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnhancedSwapEvent {
    // Basic swap info
    pub signature: String,
    pub slot: u64,
    pub swap_type: SwapType,
    pub token_in: String,
    pub token_out: String,
    pub amount_in: u64,
    pub amount_out: u64,
    pub wallet: String,
    pub dex: DexType,
    pub timestamp: DateTime<Utc>,
    
    // Enhanced metadata
    pub dex_route: Vec<DexType>, // For aggregated swaps
    pub slippage: Option<f64>,
    pub price_impact: Option<f64>,
    pub fee_sol: Option<f64>,
    pub mev_protection: bool,
    pub execution_time_ms: Option<u64>,
    pub price_before: Option<f64>,
    pub price_after: Option<f64>,
    pub volume_rank: VolumeRank,
    pub is_arbitrage: bool,
    pub is_sandwich_attack: bool,
    pub gas_fees_sol: Option<f64>,
    pub success: bool,
    pub failure_reason: Option<String>,
}

/// Enhanced large transfer with insider detection data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnhancedLargeTransfer {
    // Basic transfer info
    pub signature: String,
    pub slot: u64,
    pub from_wallet: String,
    pub to_wallet: String,
    pub token_mint: String,
    pub amount: u64,
    pub amount_sol: Option<f64>,
    pub timestamp: DateTime<Utc>,
    
    // Enhanced metadata
    pub transfer_type: TransferType,
    pub percentage_of_supply: Option<f64>,
    pub sender_balance_before: Option<u64>,
    pub sender_balance_after: Option<u64>,
    pub receiver_balance_before: Option<u64>,
    pub receiver_balance_after: Option<u64>,
    pub is_first_transfer: bool,
    pub sender_label: Option<String>, // Exchange, known wallet, etc.
    pub receiver_label: Option<String>,
    pub related_pool_activity: Vec<String>, // Related pool transaction signatures
    pub insider_confidence: Option<f64>,
}

// Supporting enums and structs

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum PoolType {
    Standard,
    Stable,
    Concentrated,
    WeightedPool,
    Bootstrap,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum BurnReason {
    LiquidityRemoval,
    RugPull,
    ProjectEnd,
    Migration,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum LiquidityChangeType {
    Added,
    Removed,
    Locked,
    Unlocked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum VolumeRank {
    Small,   // < 0.1 SOL
    Medium,  // 0.1 - 1 SOL
    Large,   // 1 - 10 SOL
    Whale,   // > 10 SOL
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum AuditStatus {
    NotAudited,
    InProgress,
    Passed,
    Failed,
    Warning,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum TransferType {
    NormalTransfer,
    Exchange,
    LiquidityProvision,
    TokenSale,
    Airdrop,
    Suspicious,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum CoordinatedActivityType {
    RugPull,
    PumpAndDump,
    WashTrading,
    Sniping,
    LiquidityMigration,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum WhaleAction {
    LargeSwap,
    LiquidityProvision,
    TokenAccumulation,
    TokenDistribution,
    PoolCreation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum TransferPattern {
    Normal,
    Accumulation,
    Distribution,
    WashTrading,
    Suspicious,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenHolder {
    pub wallet: String,
    pub amount: u64,
    pub percentage: f64,
    pub label: Option<String>, // Exchange, team, etc.
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditReport {
    pub auditor: String,
    pub report_url: String,
    pub score: f64,
    pub findings: Vec<AuditFinding>,
    pub audit_date: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditFinding {
    pub severity: RiskLevel,
    pub category: String,
    pub description: String,
    pub recommendation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WalletSwapHistory {
    pub total_swaps_24h: u32,
    pub total_volume_24h_sol: f64,
    pub avg_swap_size_sol: f64,
    pub success_rate: f64,
    pub favorite_dex: Option<DexType>,
    pub risk_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WalletTransferHistory {
    pub total_transfers_24h: u32,
    pub total_amount_24h_sol: f64,
    pub avg_transfer_size_sol: f64,
    pub unique_tokens_24h: u32,
    pub suspicious_activity_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketImpact {
    pub price_change_percentage: f64,
    pub liquidity_change_percentage: f64,
    pub volume_spike_factor: f64,
    pub affected_pools: Vec<String>,
}

impl From<crate::core::MarketEvent> for EnhancedMarketEvent {
    fn from(event: crate::core::MarketEvent) -> Self {
        match event {
            crate::core::MarketEvent::PoolCreated { pool, creator, initial_liquidity_sol } => {
                EnhancedMarketEvent::PoolCreated {
                    pool: EnhancedPoolInfo {
                        address: pool.address,
                        base_mint: pool.base_mint,
                        quote_mint: pool.quote_mint,
                        base_vault: pool.base_vault,
                        quote_vault: pool.quote_vault,
                        lp_mint: pool.lp_mint,
                        market_id: pool.market_id,
                        dex: pool.dex,
                        created_at: pool.created_at,
                        slot: pool.slot,
                        pool_type: PoolType::Standard,
                        fee_rate: None,
                        current_base_amount: pool.initial_base_amount,
                        current_quote_amount: pool.initial_quote_amount,
                        current_price: None,
                        volume_24h_sol: None,
                        unique_traders_24h: None,
                        liquidity_locked: false,
                        lock_duration_days: None,
                        creator_wallet: pool.creator_wallet,
                        creator_reputation: None,
                        risk_score: None,
                        tags: Vec::new(),
                        verified: false,
                        audit_status: AuditStatus::NotAudited,
                    },
                    creator,
                    initial_liquidity_sol,
                    creation_tx: "unknown".to_string(),
                    block_time: pool.created_at,
                    slot: pool.slot,
                }
            }
            crate::core::MarketEvent::TokenLaunched { token } => {
                EnhancedMarketEvent::TokenLaunched {
                    token: EnhancedTokenMetadata {
                        mint: token.mint,
                        name: token.name,
                        symbol: token.symbol,
                        decimals: token.decimals,
                        supply: token.supply,
                        mint_authority: token.mint_authority,
                        freeze_authority: token.freeze_authority,
                        is_mutable: token.is_mutable,
                        created_at: token.created_at,
                        slot: token.slot,
                        description: None,
                        image_uri: None,
                        website: None,
                        twitter: None,
                        telegram: None,
                        discord: None,
                        holder_count: None,
                        top_holders: Vec::new(),
                        market_cap_sol: None,
                        fully_diluted_valuation_sol: None,
                        circulating_supply: None,
                        tags: Vec::new(),
                        safety_score: None,
                        rug_pull_risk: RiskLevel::Medium,
                        honeypot_risk: RiskLevel::Medium,
                        verified_creator: false,
                        audit_reports: Vec::new(),
                    },
                    launch_tx: "unknown".to_string(),
                    first_pool_address: None,
                    time_to_first_pool_seconds: None,
                    block_time: token.created_at,
                    slot: token.slot,
                }
            }
            crate::core::MarketEvent::SwapDetected { swap } => {
                EnhancedMarketEvent::SwapDetected {
                    swap: EnhancedSwapEvent {
                        signature: swap.signature,
                        slot: swap.slot,
                        swap_type: swap.swap_type,
                        token_in: swap.token_in,
                        token_out: swap.token_out,
                        amount_in: swap.amount_in,
                        amount_out: swap.amount_out,
                        wallet: swap.wallet,
                        dex: swap.dex,
                        timestamp: swap.timestamp,
                        dex_route: vec![swap.dex],
                        slippage: None,
                        price_impact: swap.price_impact,
                        fee_sol: None,
                        mev_protection: false,
                        execution_time_ms: None,
                        price_before: None,
                        price_after: None,
                        volume_rank: VolumeRank::Medium,
                        is_arbitrage: false,
                        is_sandwich_attack: false,
                        gas_fees_sol: None,
                        success: true,
                        failure_reason: None,
                    },
                    volume_rank: VolumeRank::Medium,
                    wallet_history: WalletSwapHistory {
                        total_swaps_24h: 0,
                        total_volume_24h_sol: 0.0,
                        avg_swap_size_sol: 0.0,
                        success_rate: 0.0,
                        favorite_dex: None,
                        risk_score: 0.5,
                    },
                    market_impact: MarketImpact {
                        price_change_percentage: 0.0,
                        liquidity_change_percentage: 0.0,
                        volume_spike_factor: 0.0,
                        affected_pools: Vec::new(),
                    },
                }
            }
            crate::core::MarketEvent::LargeTransferDetected { transfer } => {
                EnhancedMarketEvent::LargeTransferDetected {
                    transfer: EnhancedLargeTransfer {
                        signature: transfer.signature,
                        slot: transfer.slot,
                        from_wallet: transfer.from_wallet,
                        to_wallet: transfer.to_wallet,
                        token_mint: transfer.token_mint,
                        amount: transfer.amount,
                        amount_sol: transfer.amount_usd.map(|usd| usd / 100.0), // Rough conversion
                        timestamp: transfer.timestamp,
                        transfer_type: TransferType::NormalTransfer,
                        percentage_of_supply: None,
                        sender_balance_before: None,
                        sender_balance_after: None,
                        receiver_balance_before: None,
                        receiver_balance_after: None,
                        is_first_transfer: false,
                        sender_label: None,
                        receiver_label: None,
                        related_pool_activity: Vec::new(),
                        insider_confidence: None,
                    },
                    sender_history: WalletTransferHistory {
                        total_transfers_24h: 0,
                        total_amount_24h_sol: 0.0,
                        avg_transfer_size_sol: 0.0,
                        unique_tokens_24h: 0,
                        suspicious_activity_score: 0.0,
                    },
                    receiver_history: WalletTransferHistory {
                        total_transfers_24h: 0,
                        total_amount_24h_sol: 0.0,
                        avg_transfer_size_sol: 0.0,
                        unique_tokens_24h: 0,
                        suspicious_activity_score: 0.0,
                    },
                    transfer_pattern: TransferPattern::Normal,
                }
            }
            _ => {
                // For other events, create a default enhanced event
                EnhancedMarketEvent::CoordinatedActivity {
                    activity_type: CoordinatedActivityType::Sniping,
                    wallets_involved: Vec::new(),
                    tokens_involved: Vec::new(),
                    total_value_sol: 0.0,
                    time_window_seconds: 0,
                    confidence_score: 0.0,
                    evidence: Vec::new(),
                    block_time: Utc::now(),
                    slot: 0,
                }
            }
        }
    }
}