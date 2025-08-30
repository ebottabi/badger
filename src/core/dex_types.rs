use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Supported DEX types on Solana
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DexType {
    Raydium,
    Jupiter,
    Orca,
    PumpFun,
    Unknown,
}

impl DexType {
    pub fn from_program_id(program_id: &str) -> Self {
        match program_id {
            "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8" => DexType::Raydium,
            "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4" => DexType::Jupiter,
            "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc" => DexType::Orca,
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P" => DexType::PumpFun,
            _ => DexType::Unknown,
        }
    }
    
    pub fn program_id(&self) -> &'static str {
        match self {
            DexType::Raydium => "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
            DexType::Jupiter => "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",
            DexType::Orca => "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc",
            DexType::PumpFun => "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
            DexType::Unknown => "",
        }
    }
}

/// Comprehensive pool information from DEX events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolInfo {
    pub address: String,
    pub base_mint: String,
    pub quote_mint: String,
    pub base_vault: String,
    pub quote_vault: String,
    pub lp_mint: String,
    pub market_id: Option<String>,
    pub dex: DexType,
    pub created_at: DateTime<Utc>,
    pub creator_wallet: String,
    pub initial_base_amount: u64,
    pub initial_quote_amount: u64,
    pub slot: u64,
}

/// Token metadata from SPL token program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMetadata {
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
}

/// Swap event data extracted from DEX transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapEvent {
    pub signature: String,
    pub slot: u64,
    pub swap_type: SwapType,
    pub token_in: String,
    pub token_out: String,
    pub amount_in: u64,
    pub amount_out: u64,
    pub wallet: String,
    pub dex: DexType,
    pub price_impact: Option<f64>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SwapType {
    Buy,   // SOL/USDC -> Token
    Sell,  // Token -> SOL/USDC
}

/// Large transfer event (potential insider activity)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeTransfer {
    pub signature: String,
    pub slot: u64,
    pub from_wallet: String,
    pub to_wallet: String,
    pub token_mint: String,
    pub amount: u64,
    pub amount_usd: Option<f64>,
    pub timestamp: DateTime<Utc>,
}

/// Market events emitted by the ingestion service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketEvent {
    /// New liquidity pool created
    PoolCreated {
        pool: PoolInfo,
        creator: String,
        initial_liquidity_sol: f64,
    },
    /// Pool burned or LP tokens removed
    PoolBurned {
        pool_address: String,
        burn_tx: String,
    },
    /// New token launched
    TokenLaunched {
        token: TokenMetadata,
    },
    /// Significant liquidity change in pool
    LiquidityChanged {
        pool_address: String,
        change_sol: f64,
        new_total_sol: f64,
    },
    /// Large swap detected
    SwapDetected {
        swap: SwapEvent,
    },
    /// Large token transfer detected
    LargeTransferDetected {
        transfer: LargeTransfer,
    },
}

/// Trading signals that can be generated from market events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradingSignal {
    /// Buy signal with confidence and reasoning
    Buy {
        token_mint: String,
        confidence: f64,
        max_amount_sol: f64,
        reason: String,
        source: SignalSource,
    },
    /// Sell signal with targets
    Sell {
        token_mint: String,
        price_target: f64,
        stop_loss: f64,
        reason: String,
    },
    /// General swap activity detected
    SwapActivity {
        token_mint: String,
        volume_increase: f64,
        whale_activity: bool,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SignalSource {
    NewPool,
    InsiderWallet,
    VolumeSpike,
    LiquidityAdd,
}

/// Constants for DEX program IDs and common tokens
pub mod constants {
    pub const RAYDIUM_AMM_PROGRAM: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
    pub const JUPITER_V6_PROGRAM: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
    pub const ORCA_WHIRLPOOL_PROGRAM: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";
    pub const SPL_TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    pub const PUMP_FUN_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
    
    // Common tokens
    pub const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
    pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    pub const USDT_MINT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
    
    // Thresholds for detection
    pub const LARGE_TRANSFER_THRESHOLD_SOL: f64 = 10.0; // >10 SOL
    pub const MIN_POOL_LIQUIDITY_SOL: f64 = 1.0; // >1 SOL minimum
    pub const HIGH_CONFIDENCE_THRESHOLD: f64 = 0.8; // >80% confidence
}

/// Utility functions for parsing Solana data
pub mod utils {
    use super::*;
    use solana_sdk::pubkey::Pubkey;
    
    /// Convert pubkey to shortened display format
    pub fn shorten_pubkey(pubkey: &str) -> String {
        if pubkey.len() >= 16 {
            format!("{}...{}", &pubkey[..8], &pubkey[pubkey.len()-8..])
        } else {
            pubkey.to_string()
        }
    }
    
    /// Convert lamports to SOL
    pub fn lamports_to_sol(lamports: u64) -> f64 {
        lamports as f64 / 1_000_000_000.0
    }
    
    /// Convert SOL to lamports
    pub fn sol_to_lamports(sol: f64) -> u64 {
        (sol * 1_000_000_000.0) as u64
    }
    
    /// Check if pubkey is valid Solana address
    pub fn is_valid_pubkey(pubkey_str: &str) -> bool {
        Pubkey::from_str(pubkey_str).is_ok()
    }
    
    /// Extract token amount considering decimals
    pub fn token_amount_to_ui(raw_amount: u64, decimals: u8) -> f64 {
        raw_amount as f64 / 10_f64.powi(decimals as i32)
    }
}