/// Buy signal criteria and filtering logic

use crate::client::event_parser::UniversalPumpEvent;
use crate::config::Config;
use crate::util::dexscreener::DexScreenerClient;
use reqwest;
use serde_json;
use std::sync::Arc;

pub struct InstantBuyCriteria {
    pub min_market_cap_sol: f64,
    pub min_initial_buy_sol: f64,
    pub max_token_age_minutes: i64,
    pub min_whale_buy_sol: f64,
    pub min_market_cap_usd: f64,
    pub max_market_cap_usd: f64,
    dexscreener_client: Arc<DexScreenerClient>,
}

impl Default for InstantBuyCriteria {
    fn default() -> Self {
        Self {
            min_market_cap_sol: 30.0,      // 30 SOL minimum market cap
            min_initial_buy_sol: 3.0,      // 3 SOL minimum initial buy
            max_token_age_minutes: 5,      // Only tokens <5 minutes old
            min_whale_buy_sol: 8.0,        // 8 SOL+ considered whale buy
            min_market_cap_usd: 6500.0,   // Minimum $6.5K market cap
            max_market_cap_usd: 50000.0,   // Maximum $50K market cap
            dexscreener_client: Arc::new(DexScreenerClient::new()),
        }
    }
}

impl InstantBuyCriteria {
    pub fn from_config(config: &Config) -> Self {
        Self {
            min_market_cap_sol: 30.0,      // 30 SOL minimum market cap
            min_initial_buy_sol: 3.0,      // 3 SOL minimum initial buy
            max_token_age_minutes: config.entry_criteria.max_token_age_minutes.unwrap_or(5),
            min_whale_buy_sol: 8.0,        // 8 SOL+ considered whale buy
            min_market_cap_usd: config.entry_criteria.min_market_cap_usd,
            max_market_cap_usd: config.entry_criteria.max_market_cap_usd,
            dexscreener_client: Arc::new(DexScreenerClient::new()),
        }
    }

    pub fn is_instant_buy_signal(&self, event: &UniversalPumpEvent) -> bool {
        let market_cap = event.market_cap_sol.unwrap_or(0.0);
        let initial_buy = event.sol_amount.unwrap_or(0.0);
        
        market_cap >= self.min_market_cap_sol &&
        initial_buy >= self.min_initial_buy_sol &&
        !self.is_suspicious_token(event)
    }
    
    pub async fn is_valid_token(&self, event: &UniversalPumpEvent, sol_to_usd_rate: f64, _config: Option<&Config>) -> bool {
        // Basic filters first
        if !self.is_instant_buy_signal(event) {
            return false;
        }
        
        // Try manual market cap calculation first (primary)
        let market_cap_sol = event.market_cap_sol.unwrap_or(0.0);
        let market_cap_usd = market_cap_sol * sol_to_usd_rate;
        
        if market_cap_usd >= self.min_market_cap_usd && market_cap_usd <= self.max_market_cap_usd {
            println!("✅ All filters passed for token {} (Manual: ${:.0})", event.mint, market_cap_usd);
            return true;
        }
        
        // If manual calculation fails, try DexScreener as fallback
        println!("⚠️ Manual market cap failed: ${:.0} (min: ${:.0}, max: ${:.0})", 
                market_cap_usd, self.min_market_cap_usd, self.max_market_cap_usd);
        println!("🔄 Trying DexScreener for verification...");
        
        match self.dexscreener_client.verify_market_cap(
            &event.mint, 
            self.min_market_cap_usd, 
            self.max_market_cap_usd
        ).await {
            Ok(is_valid) => {
                if is_valid {
                    println!("✅ All filters passed for token {} (DexScreener fallback)", event.mint);
                    true
                } else {
                    println!("❌ Both manual and DexScreener market cap checks failed for {}", event.mint);
                    false
                }
            },
            Err(_e) => {
                println!("❌ Both manual and DexScreener market cap checks failed for {}", event.mint);
                false
            }
        }
    }
    
    // Fast validation - skips age check (already done in signal processor)
    pub async fn is_valid_token_fast(&self, event: &UniversalPumpEvent, sol_to_usd_rate: f64) -> bool {
        // Basic filters first
        if !self.is_instant_buy_signal(event) {
            return false;
        }
        
        // Try manual market cap calculation first (primary)
        let market_cap_sol = event.market_cap_sol.unwrap_or(0.0);
        let market_cap_usd = market_cap_sol * sol_to_usd_rate;
        
        if market_cap_usd >= self.min_market_cap_usd && market_cap_usd <= self.max_market_cap_usd {
            println!("✅ Fast filters passed for token {} (Manual: ${:.0})", event.mint, market_cap_usd);
            return true;
        }
        
        // If manual calculation fails, try DexScreener as fallback
        println!("⚠️ Manual market cap failed: ${:.0} (min: ${:.0}, max: ${:.0})", 
                market_cap_usd, self.min_market_cap_usd, self.max_market_cap_usd);
        println!("🔄 Trying DexScreener for verification...");
        
        match self.dexscreener_client.verify_market_cap(
            &event.mint, 
            self.min_market_cap_usd, 
            self.max_market_cap_usd
        ).await {
            Ok(is_valid) => {
                if is_valid {
                    println!("✅ Fast filters passed for token {} (DexScreener fallback)", event.mint);
                    true
                } else {
                    println!("❌ Both manual and DexScreener market cap checks failed for {}", event.mint);
                    false
                }
            },
            Err(_e) => {
                println!("❌ Both manual and DexScreener market cap checks failed for {}", event.mint);
                false
            }
        }
    }
    
    pub fn is_whale_buy(&self, event: &UniversalPumpEvent) -> bool {
        event.sol_amount.unwrap_or(0.0) >= self.min_whale_buy_sol
    }
    
    
    fn is_suspicious_token(&self, event: &UniversalPumpEvent) -> bool {
        let name_lower = event.name.to_lowercase();
        let symbol_lower = event.symbol.to_lowercase();
        
        // Flag obvious scam patterns
        name_lower.contains("elon") || 
        name_lower.contains("trump") ||
        symbol_lower.len() > 10 ||
        event.name.chars().any(|c| !c.is_ascii())
    }

    // Get DexScreener data for additional validation (non-blocking)
    pub async fn get_dexscreener_enrichment(&self, mint: &str) -> Option<DexScreenerData> {
        let client = reqwest::Client::new();
        let url = format!("https://api.dexscreener.com/latest/dex/tokens/{}", mint);
        
        match client.get(&url)
            .timeout(std::time::Duration::from_secs(3)) // Short timeout
            .send().await 
        {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<serde_json::Value>().await {
                        Ok(data) => {
                            if let Some(pairs) = data.get("pairs").and_then(|p| p.as_array()) {
                                if let Some(pair) = pairs.first() {
                                    return Some(DexScreenerData {
                                        liquidity_usd: pair.get("liquidity").and_then(|l| l.get("usd")).and_then(|u| u.as_f64()),
                                        volume_24h: pair.get("volume").and_then(|v| v.get("h24")).and_then(|h| h.as_f64()),
                                        price_change_5m: pair.get("priceChange").and_then(|p| p.get("m5")).and_then(|m| m.as_f64()),
                                        fdv: pair.get("fdv").and_then(|f| f.as_f64()),
                                        txns_5m: pair.get("txns").and_then(|t| t.get("m5")).and_then(|m| m.get("buys")).and_then(|b| b.as_u64()),
                                    });
                                }
                            }
                        }
                        Err(e) => println!("⚠️ DexScreener parse error: {}", e),
                    }
                } else {
                    println!("⚠️ DexScreener API status: {}", response.status());
                }
            }
            Err(e) => println!("⚠️ DexScreener request failed: {}", e),
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct DexScreenerData {
    pub liquidity_usd: Option<f64>,
    pub volume_24h: Option<f64>,
    pub price_change_5m: Option<f64>,
    pub fdv: Option<f64>,
    pub txns_5m: Option<u64>,
}