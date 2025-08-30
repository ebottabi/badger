use anyhow::{Result, bail};
use chrono::Utc;
use serde_json::Value;
use tracing::{debug, warn};

use crate::core::dex_types::*;
use crate::core::dex_types::constants::*;
use crate::core::dex_types::utils::*;

/// Master parser that routes to specific DEX parsers based on program ID
pub struct DexEventParser;

impl DexEventParser {
    /// Parse program account update and extract DEX-specific events
    pub fn parse_program_update(subscription_id: u64, data: &Value) -> Result<Vec<MarketEvent>> {
        let mut events = Vec::new();
        
        // Extract basic program update information
        let context = data.get("context").and_then(|c| c.as_object());
        let slot = context
            .and_then(|c| c.get("slot"))
            .and_then(|s| s.as_u64())
            .unwrap_or(0);
            
        let value = data.get("value").and_then(|v| v.as_object());
        if value.is_none() {
            return Ok(events);
        }
        let value = value.unwrap();
        
        let account_info = value.get("account").and_then(|a| a.as_object());
        let pubkey = value.get("pubkey").and_then(|p| p.as_str()).unwrap_or("");
        
        if let Some(account) = account_info {
            let owner = account.get("owner").and_then(|o| o.as_str()).unwrap_or("");
            
            // Route to specific parser based on program ID
            match owner {
                RAYDIUM_AMM_PROGRAM => {
                    debug!("🔥 Parsing Raydium event for account: {}", shorten_pubkey(pubkey));
                    if let Ok(raydium_events) = Self::parse_raydium_event(account, pubkey, slot) {
                        events.extend(raydium_events);
                    }
                }
                JUPITER_V6_PROGRAM => {
                    debug!("⚡ Parsing Jupiter event for account: {}", shorten_pubkey(pubkey));
                    if let Ok(jupiter_events) = Self::parse_jupiter_event(account, pubkey, slot) {
                        events.extend(jupiter_events);
                    }
                }
                ORCA_WHIRLPOOL_PROGRAM => {
                    debug!("🌊 Parsing Orca event for account: {}", shorten_pubkey(pubkey));
                    if let Ok(orca_events) = Self::parse_orca_event(account, pubkey, slot) {
                        events.extend(orca_events);
                    }
                }
                SPL_TOKEN_PROGRAM => {
                    debug!("🪙 Parsing SPL Token event for account: {}", shorten_pubkey(pubkey));
                    if let Ok(token_events) = Self::parse_spl_token_event(account, pubkey, slot) {
                        events.extend(token_events);
                    }
                }
                PUMP_FUN_PROGRAM => {
                    debug!("🚀 Parsing Pump.fun event for account: {}", shorten_pubkey(pubkey));
                    if let Ok(pump_events) = Self::parse_pump_fun_event(account, pubkey, slot) {
                        events.extend(pump_events);
                    }
                }
                _ => {
                    // Unknown program, skip
                    debug!("Unknown program owner: {}", owner);
                }
            }
        }
        
        Ok(events)
    }
    
    /// Parse Raydium AMM program events (pool creation, swaps)
    fn parse_raydium_event(account: &serde_json::Map<String, Value>, pubkey: &str, slot: u64) -> Result<Vec<MarketEvent>> {
        let mut events = Vec::new();
        
        // Check if this is pool creation or swap
        let lamports = account.get("lamports").and_then(|l| l.as_u64()).unwrap_or(0);
        let data = account.get("data").and_then(|d| d.as_object());
        
        if let Some(data_obj) = data {
            // Check for parsed data (newer accounts)
            if let Some(parsed) = data_obj.get("parsed").and_then(|p| p.as_object()) {
                let account_type = parsed.get("type").and_then(|t| t.as_str()).unwrap_or("");
                
                match account_type {
                    "pool" | "ammPool" => {
                        // This might be a new pool
                        if let Some(info) = parsed.get("info").and_then(|i| i.as_object()) {
                            let pool = Self::extract_raydium_pool_info(info, pubkey, slot)?;
                            
                            events.push(MarketEvent::PoolCreated {
                                pool: pool.clone(),
                                creator: pool.creator_wallet.clone(),
                                initial_liquidity_sol: lamports_to_sol(lamports),
                            });
                        }
                    }
                    _ => {}
                }
            } else if let Some(raw_data) = data_obj.get("data") {
                // Raw account data - try to detect pool structure
                if lamports > sol_to_lamports(MIN_POOL_LIQUIDITY_SOL) {
                    debug!("Potential Raydium pool detected: {} with {:.3} SOL", 
                        shorten_pubkey(pubkey), lamports_to_sol(lamports));
                    
                    // Create minimal pool info for raw accounts
                    let pool = PoolInfo {
                        address: pubkey.to_string(),
                        base_mint: "unknown".to_string(),
                        quote_mint: "unknown".to_string(),
                        base_vault: "unknown".to_string(),
                        quote_vault: "unknown".to_string(),
                        lp_mint: "unknown".to_string(),
                        market_id: None,
                        dex: DexType::Raydium,
                        created_at: Utc::now(),
                        creator_wallet: "unknown".to_string(),
                        initial_base_amount: 0,
                        initial_quote_amount: lamports,
                        slot,
                    };
                    
                    events.push(MarketEvent::PoolCreated {
                        pool,
                        creator: "unknown".to_string(),
                        initial_liquidity_sol: lamports_to_sol(lamports),
                    });
                }
            }
        }
        
        Ok(events)
    }
    
    /// Parse Jupiter V6 aggregator events
    fn parse_jupiter_event(account: &serde_json::Map<String, Value>, pubkey: &str, slot: u64) -> Result<Vec<MarketEvent>> {
        let mut events = Vec::new();
        
        let lamports = account.get("lamports").and_then(|l| l.as_u64()).unwrap_or(0);
        
        // Jupiter events are typically swap-related, look for significant balance changes
        if lamports > sol_to_lamports(1.0) { // >1 SOL activity
            debug!("Jupiter activity detected: {} with {:.3} SOL", 
                shorten_pubkey(pubkey), lamports_to_sol(lamports));
            
            // For now, we'll detect this as general swap activity
            // In production, you'd parse the actual instruction data
        }
        
        Ok(events)
    }
    
    /// Parse Orca Whirlpool events
    fn parse_orca_event(account: &serde_json::Map<String, Value>, pubkey: &str, slot: u64) -> Result<Vec<MarketEvent>> {
        let mut events = Vec::new();
        
        let lamports = account.get("lamports").and_then(|l| l.as_u64()).unwrap_or(0);
        
        if lamports > sol_to_lamports(MIN_POOL_LIQUIDITY_SOL) {
            debug!("Potential Orca pool activity: {} with {:.3} SOL", 
                shorten_pubkey(pubkey), lamports_to_sol(lamports));
        }
        
        Ok(events)
    }
    
    /// Parse SPL Token program events (new mints, transfers)
    fn parse_spl_token_event(account: &serde_json::Map<String, Value>, pubkey: &str, slot: u64) -> Result<Vec<MarketEvent>> {
        let mut events = Vec::new();
        
        let data = account.get("data").and_then(|d| d.as_object());
        
        if let Some(data_obj) = data {
            if let Some(parsed) = data_obj.get("parsed").and_then(|p| p.as_object()) {
                let account_type = parsed.get("type").and_then(|t| t.as_str()).unwrap_or("");
                
                match account_type {
                    "mint" => {
                        // New token mint detected
                        if let Some(info) = parsed.get("info").and_then(|i| i.as_object()) {
                            let token = Self::extract_token_metadata(info, pubkey, slot)?;
                            
                            events.push(MarketEvent::TokenLaunched { token });
                        }
                    }
                    "account" => {
                        // Token account - check for large transfers
                        if let Some(info) = parsed.get("info").and_then(|i| i.as_object()) {
                            if let Some(amount) = info.get("tokenAmount")
                                .and_then(|ta| ta.as_object())
                                .and_then(|ta| ta.get("uiAmount"))
                                .and_then(|ui| ui.as_f64()) 
                            {
                                if amount > 1000000.0 { // Large token transfer
                                    debug!("Large token transfer detected: {:.0} tokens in account {}", 
                                        amount, shorten_pubkey(pubkey));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        
        Ok(events)
    }
    
    /// Parse Pump.fun events (meme coin launches)
    fn parse_pump_fun_event(account: &serde_json::Map<String, Value>, pubkey: &str, slot: u64) -> Result<Vec<MarketEvent>> {
        let mut events = Vec::new();
        
        let lamports = account.get("lamports").and_then(|l| l.as_u64()).unwrap_or(0);
        
        // Pump.fun typically creates tokens with initial liquidity
        if lamports > sol_to_lamports(0.1) { // >0.1 SOL
            debug!("Pump.fun activity detected: {} with {:.3} SOL", 
                shorten_pubkey(pubkey), lamports_to_sol(lamports));
            
            // This could be a new meme coin launch
            let token = TokenMetadata {
                mint: pubkey.to_string(),
                name: "Unknown Pump.fun Token".to_string(),
                symbol: "PUMP".to_string(),
                decimals: 6,
                supply: 1_000_000_000_000,
                mint_authority: Some("pump.fun".to_string()),
                freeze_authority: None,
                is_mutable: true,
                created_at: Utc::now(),
                slot,
            };
            
            events.push(MarketEvent::TokenLaunched { token });
        }
        
        Ok(events)
    }
    
    /// Extract detailed pool information from Raydium parsed data
    fn extract_raydium_pool_info(info: &serde_json::Map<String, Value>, pubkey: &str, slot: u64) -> Result<PoolInfo> {
        let base_mint = info.get("baseMint")
            .or_else(|| info.get("base_mint"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
            
        let quote_mint = info.get("quoteMint")
            .or_else(|| info.get("quote_mint"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
            
        Ok(PoolInfo {
            address: pubkey.to_string(),
            base_mint,
            quote_mint,
            base_vault: info.get("baseVault")
                .or_else(|| info.get("base_vault"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            quote_vault: info.get("quoteVault")
                .or_else(|| info.get("quote_vault"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            lp_mint: info.get("lpMint")
                .or_else(|| info.get("lp_mint"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            market_id: info.get("marketId")
                .or_else(|| info.get("market_id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            dex: DexType::Raydium,
            created_at: Utc::now(),
            creator_wallet: "unknown".to_string(),
            initial_base_amount: 0,
            initial_quote_amount: 0,
            slot,
        })
    }
    
    /// Extract token metadata from SPL token mint info
    fn extract_token_metadata(info: &serde_json::Map<String, Value>, pubkey: &str, slot: u64) -> Result<TokenMetadata> {
        let decimals = info.get("decimals")
            .and_then(|d| d.as_u64())
            .unwrap_or(6) as u8;
            
        let supply = info.get("supply")
            .and_then(|s| s.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
            
        let mint_authority = info.get("mintAuthority")
            .and_then(|ma| ma.as_str())
            .map(|s| s.to_string());
            
        let freeze_authority = info.get("freezeAuthority")
            .and_then(|fa| fa.as_str())
            .map(|s| s.to_string());
            
        let is_mutable = info.get("isInitialized")
            .and_then(|ii| ii.as_bool())
            .unwrap_or(true);
        
        Ok(TokenMetadata {
            mint: pubkey.to_string(),
            name: "Unknown Token".to_string(), // Would need metadata API call
            symbol: "UNK".to_string(),
            decimals,
            supply,
            mint_authority,
            freeze_authority,
            is_mutable,
            created_at: Utc::now(),
            slot,
        })
    }
}

/// Specific instruction parsers for each DEX
pub mod instruction_parsers {
    use super::*;
    
    /// Parse Raydium swap instructions from transaction data
    pub fn parse_raydium_swap(instruction_data: &[u8]) -> Result<SwapEvent> {
        // This would parse the actual instruction data
        // For now, return placeholder
        bail!("Raydium swap parsing not implemented")
    }
    
    /// Parse Jupiter swap instructions
    pub fn parse_jupiter_swap(instruction_data: &[u8]) -> Result<SwapEvent> {
        // Parse Jupiter aggregator route
        bail!("Jupiter swap parsing not implemented")
    }
}