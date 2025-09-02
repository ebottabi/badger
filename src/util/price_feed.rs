/// Real-time price feed integration with Jupiter API primary and DexScreener fallback

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Instant, Duration};
use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use crate::util::dexscreener::DexScreenerClient;

const JUPITER_PRICE_API: &str = "https://lite-api.jup.ag/price/v3";
const WRAPPED_SOL_MINT: &str = "So11111111111111111111111111111111111111112";
const CACHE_DURATION_SECS: u64 = 30;
const SOL_CACHE_DURATION_SECS: u64 = 30;
const API_TIMEOUT_SECS: u64 = 30;

pub struct PriceFeed {
    client: Client,
    token_cache: Arc<Mutex<HashMap<String, (f64, Instant)>>>,
    sol_rate_cache: Arc<Mutex<Option<(f64, Instant)>>>,
    dexscreener_client: DexScreenerClient,
}

impl PriceFeed {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(API_TIMEOUT_SECS))
                .build()
                .expect("Failed to create HTTP client"),
            token_cache: Arc::new(Mutex::new(HashMap::new())),
            sol_rate_cache: Arc::new(Mutex::new(None)),
            dexscreener_client: DexScreenerClient::new(),
        }
    }
    
    pub async fn get_token_price(&self, mint: &str) -> Result<f64> {
        // Check cache first
        if let Some(cached_price) = self.get_cached_token_price(mint) {
            return Ok(cached_price);
        }
        
        // Try Jupiter API first (primary)
        match self.fetch_jupiter_price(mint).await {
            Ok(price) => {
                self.cache_token_price(mint, price);
                println!("✅ Jupiter price: ${:.8}", price);
                Ok(price)
            },
            Err(jupiter_error) => {
                println!("⚠️ Jupiter API failed for {}: {}", mint, jupiter_error);
                println!("🔄 Falling back to DexScreener...");
                
                // Fallback to DexScreener API
                match self.dexscreener_client.get_token_info(mint).await {
                    Ok(token_info) => {
                        if let Some(pair) = token_info.pairs.first() {
                            if let Some(price_str) = &pair.price_usd {
                                if let Ok(price) = price_str.parse::<f64>() {
                                    self.cache_token_price(mint, price);
                                    println!("✅ DexScreener fallback successful: ${:.8}", price);
                                    return Ok(price);
                                }
                            }
                        }
                        Err(anyhow::anyhow!("Both Jupiter and DexScreener failed. Jupiter: {}, DexScreener: price parsing failed", jupiter_error))
                    },
                    Err(dex_error) => {
                        Err(anyhow::anyhow!("Both Jupiter and DexScreener failed. Jupiter: {}, DexScreener: {}", jupiter_error, dex_error))
                    }
                }
            }
        }
    }
    
    pub async fn get_sol_usd_rate(&self) -> Result<f64> {
        // Check cache first
        if let Some(cached_rate) = self.get_cached_sol_rate() {
            return Ok(cached_rate);
        }
        
        // Fetch SOL price
        let rate = self.fetch_jupiter_price(WRAPPED_SOL_MINT).await?;
        
        // Cache the result
        self.cache_sol_rate(rate);
        
        Ok(rate)
    }
    
    pub async fn convert_usd_to_sol(&self, usd_amount: f64) -> Result<f64> {
        let sol_rate = self.get_sol_usd_rate().await?;
        Ok(usd_amount / sol_rate)
    }
    
    pub async fn get_token_price_with_fallbacks(&self, mint: &str, _market_cap_sol: Option<f64>) -> Result<f64> {
        // Use the enhanced get_token_price method that already has DexScreener fallback
        self.get_token_price(mint).await
    }
    
    
    async fn fetch_jupiter_price(&self, mint: &str) -> Result<f64> {
        let url = format!("{}?ids={}", JUPITER_PRICE_API, mint);
        
        let response = self.client
            .get(&url)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Jupiter API error: {}", response.status()));
        }
        
        let data: Value = response.json().await?;
        
        // V3 API format: { "mint_address": { "usdPrice": 147.47 } }
        if let Some(price_obj) = data[mint].as_object() {
            if let Some(price) = price_obj["usdPrice"].as_f64() {
                return Ok(price);
            }
        }
        
        Err(anyhow::anyhow!("Price not found for mint: {}", mint))
    }
    
    fn get_cached_token_price(&self, mint: &str) -> Option<f64> {
        let cache = self.token_cache.lock().unwrap();
        if let Some((price, timestamp)) = cache.get(mint) {
            if timestamp.elapsed().as_secs() < CACHE_DURATION_SECS {
                return Some(*price);
            }
        }
        None
    }
    
    fn cache_token_price(&self, mint: &str, price: f64) {
        let mut cache = self.token_cache.lock().unwrap();
        cache.insert(mint.to_string(), (price, Instant::now()));
    }
    
    fn get_cached_sol_rate(&self) -> Option<f64> {
        let cache = self.sol_rate_cache.lock().unwrap();
        if let Some((rate, timestamp)) = *cache {
            if timestamp.elapsed().as_secs() < SOL_CACHE_DURATION_SECS {
                return Some(rate);
            }
        }
        None
    }
    
    fn cache_sol_rate(&self, rate: f64) {
        let mut cache = self.sol_rate_cache.lock().unwrap();
        *cache = Some((rate, Instant::now()));
    }
}