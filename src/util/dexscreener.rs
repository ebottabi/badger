/// DexScreener API client for market cap verification and token data

use std::time::{Duration, Instant};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const DEXSCREENER_API: &str = "https://api.dexscreener.com/latest/dex/tokens";
const CACHE_DURATION_SECS: u64 = 30;
const API_TIMEOUT_SECS: u64 = 15;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DexScreenerResponse {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    pub pairs: Option<Vec<TokenPair>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenPair {
    #[serde(rename = "chainId")]
    pub chain_id: String,
    #[serde(rename = "dexId")]
    pub dex_id: String,
    pub url: Option<String>,
    #[serde(rename = "pairAddress")]
    pub pair_address: String,
    #[serde(rename = "baseToken")]
    pub base_token: BaseToken,
    #[serde(rename = "quoteToken")]
    pub quote_token: QuoteToken,
    #[serde(rename = "priceNative")]
    pub price_native: Option<String>,
    #[serde(rename = "priceUsd")]
    pub price_usd: Option<String>,
    pub txns: Option<Transactions>,
    pub volume: Option<Volume>,
    #[serde(rename = "priceChange")]
    pub price_change: Option<PriceChange>,
    pub fdv: Option<f64>,
    #[serde(rename = "marketCap")]
    pub market_cap: Option<f64>,
    #[serde(rename = "pairCreatedAt")]
    pub pair_created_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BaseToken {
    pub address: String,
    pub name: String,
    pub symbol: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuoteToken {
    pub address: String,
    pub name: String,
    pub symbol: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Transactions {
    pub m5: TransactionCount,
    pub h1: TransactionCount,
    pub h6: TransactionCount,
    pub h24: TransactionCount,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TransactionCount {
    pub buys: u32,
    pub sells: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Volume {
    pub h24: f64,
    pub h6: f64,
    pub h1: f64,
    pub m5: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PriceChange {
    pub m5: f64,
    pub h1: f64,
    pub h6: f64,
    pub h24: f64,
}

#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub schema_version: String,
    pub pairs: Vec<TokenPair>,
}

pub struct DexScreenerClient {
    client: Client,
    cache: Arc<Mutex<HashMap<String, (TokenInfo, Instant)>>>,
}

impl DexScreenerClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(API_TIMEOUT_SECS))
                .build()
                .expect("Failed to create DexScreener HTTP client"),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    pub async fn get_token_info(&self, mint: &str) -> Result<TokenInfo> {
        // Check cache first
        if let Some(cached_info) = self.get_cached_token_info(mint) {
            return Ok(cached_info);
        }
        
        // Fetch from DexScreener API
        let token_info = self.fetch_token_info(mint).await?;
        
        // Cache the result
        self.cache_token_info(mint, token_info.clone());
        
        Ok(token_info)
    }
    
    pub async fn verify_market_cap(&self, mint: &str, min_market_cap: f64, max_market_cap: f64) -> Result<bool> {
        match self.get_token_info(mint).await {
            Ok(token_info) => {
                if let Some(pair) = token_info.pairs.first() {
                    let market_cap = pair.market_cap.unwrap_or(0.0);
                    let is_valid = market_cap >= min_market_cap && market_cap <= max_market_cap;
                    
                    println!("🔍 DexScreener Market Cap Check for {}:", mint);
                    println!("   Market Cap: ${:.2}", market_cap);
                    println!("   Min Required: ${:.2}", min_market_cap);
                    println!("   Max Allowed: ${:.2}", max_market_cap);
                    println!("   Valid: {}", if is_valid { "✅" } else { "❌" });
                    
                    Ok(is_valid)
                } else {
                    println!("❌ No pairs found in DexScreener response for {}", mint);
                    Ok(false) // Treat as invalid if no pairs found
                }
            },
            Err(e) => {
                println!("❌ DexScreener lookup failed for {}: {}", mint, e);
                println!("   This token may be too new or not yet indexed");
                Ok(false) // Treat as invalid if token not found on DexScreener
            }
        }
    }
    
    async fn fetch_token_info(&self, mint: &str) -> Result<TokenInfo> {
        let url = format!("{}/{}", DEXSCREENER_API, mint);
        
        println!("🌐 Fetching token info from DexScreener: {}", url);
        
        let response = self.client
            .get(&url)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("DexScreener API error: {}", response.status()));
        }
        
        let response_text = response.text().await?;
        println!("📄 Raw DexScreener response: {}", response_text);
        
        let data: DexScreenerResponse = serde_json::from_str(&response_text)
            .map_err(|e| anyhow::anyhow!("Failed to parse DexScreener response: {}", e))?;
        
        let pairs = match data.pairs {
            Some(pairs) if !pairs.is_empty() => pairs,
            Some(_) => {
                println!("⚠️ DexScreener returned empty pairs array for token: {}", mint);
                return Err(anyhow::anyhow!("No trading pairs found for token on DexScreener"));
            },
            None => {
                println!("⚠️ DexScreener returned null pairs data for token: {}", mint);
                return Err(anyhow::anyhow!("Token not found on DexScreener"));
            }
        };
        
        // Return the raw response structure
        let token_info = TokenInfo {
            schema_version: data.schema_version,
            pairs,
        };
        
        // Log the first pair for debugging
        if let Some(pair) = token_info.pairs.first() {
            println!("📊 DexScreener data for {} ({}):", pair.base_token.symbol, pair.base_token.name);
            println!("   Market Cap: ${:.2}", pair.market_cap.unwrap_or(0.0));
            println!("   Price USD: ${:.8}", pair.price_usd.as_ref().unwrap_or(&"N/A".to_string()));
            println!("   Price SOL: {:.8}", pair.price_native.as_ref().unwrap_or(&"N/A".to_string()));
        }
        
        Ok(token_info)
    }
    
    fn get_cached_token_info(&self, mint: &str) -> Option<TokenInfo> {
        let cache = self.cache.lock().unwrap();
        if let Some((info, timestamp)) = cache.get(mint) {
            if timestamp.elapsed().as_secs() < CACHE_DURATION_SECS {
                println!("💾 Using cached DexScreener data for {}", mint);
                return Some(info.clone());
            }
        }
        None
    }
    
    fn cache_token_info(&self, mint: &str, info: TokenInfo) {
        let mut cache = self.cache.lock().unwrap();
        cache.insert(mint.to_string(), (info, Instant::now()));
    }
}

impl Default for DexScreenerClient {
    fn default() -> Self {
        Self::new()
    }
}