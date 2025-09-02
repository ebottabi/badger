/// DexScreener API client for existing token momentum detection

use std::time::Duration;
use reqwest;
use serde::{Deserialize, Serialize};
use anyhow::Result;

#[derive(Debug, Clone, Deserialize)]
pub struct DexScreenerPair {
    #[serde(rename = "chainId")]
    pub chain_id: String,
    #[serde(rename = "dexId")]
    pub dex_id: String,
    pub url: String,
    #[serde(rename = "pairAddress")]
    pub pair_address: String,
    #[serde(rename = "baseToken")]
    pub base_token: DexScreenerToken,
    #[serde(rename = "quoteToken")]
    pub quote_token: DexScreenerToken,
    #[serde(rename = "priceNative")]
    pub price_native: String,
    #[serde(rename = "priceUsd")]
    pub price_usd: Option<String>,
    pub txns: DexScreenerTransactions,
    pub volume: DexScreenerVolume,
    #[serde(rename = "priceChange")]
    pub price_change: DexScreenerPriceChange,
    pub liquidity: Option<DexScreenerLiquidity>,
    pub fdv: Option<f64>,
    #[serde(rename = "marketCap")]
    pub market_cap: Option<f64>,
    #[serde(rename = "pairCreatedAt")]
    pub pair_created_at: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DexScreenerToken {
    pub address: String,
    pub name: String,
    pub symbol: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DexScreenerTransactions {
    pub m5: DexScreenerTransactionCount,
    pub h1: DexScreenerTransactionCount,
    pub h6: DexScreenerTransactionCount,
    pub h24: DexScreenerTransactionCount,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DexScreenerTransactionCount {
    pub buys: u64,
    pub sells: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DexScreenerVolume {
    pub h24: f64,
    pub h6: f64,
    pub h1: f64,
    pub m5: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DexScreenerPriceChange {
    pub m5: Option<f64>,
    pub h1: Option<f64>,
    pub h6: Option<f64>,
    pub h24: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DexScreenerLiquidity {
    pub usd: Option<f64>,
    pub base: Option<f64>,
    pub quote: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DexScreenerResponse {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    pub pairs: Option<Vec<DexScreenerPair>>,
}

pub struct DexScreenerMomentumClient {
    client: reqwest::Client,
}

impl DexScreenerMomentumClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
    
    /// Get trending Solana tokens with recent momentum
    pub async fn get_trending_solana_tokens(&self) -> Result<Vec<DexScreenerPair>> {
        let url = "https://api.dexscreener.com/latest/dex/tokens/So11111111111111111111111111111111111111112";
        
        let response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
            
        let data: DexScreenerResponse = response.json().await?;
        
        if let Some(pairs) = data.pairs {
            // Filter for momentum criteria
            let momentum_pairs: Vec<DexScreenerPair> = pairs
                .into_iter()
                .filter(|pair| self.has_momentum_signals(pair))
                .collect();
                
            Ok(momentum_pairs)
        } else {
            Ok(vec![])
        }
    }
    
    /// Search for tokens by specific criteria and momentum
    pub async fn search_momentum_tokens(&self) -> Result<Vec<DexScreenerPair>> {
        // Use DexScreener's search endpoint for active Solana pairs
        let url = "https://api.dexscreener.com/latest/dex/search/?q=SOL";
        
        let response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
            
        let data: DexScreenerResponse = response.json().await?;
        
        if let Some(pairs) = data.pairs {
            // Filter for Solana pairs with momentum
            let solana_momentum: Vec<DexScreenerPair> = pairs
                .into_iter()
                .filter(|pair| {
                    pair.chain_id == "solana" && 
                    self.has_momentum_signals(pair) &&
                    self.is_mature_enough(pair)
                })
                .take(50) // Limit to top 50 momentum tokens
                .collect();
                
            Ok(solana_momentum)
        } else {
            Ok(vec![])
        }
    }
    
    /// Check if token has momentum signals
    fn has_momentum_signals(&self, pair: &DexScreenerPair) -> bool {
        // Volume momentum: High 1h volume
        let high_volume = pair.volume.h1 > 50.0; // $50+ volume in last hour
        
        // Price momentum: Positive price change in multiple timeframes
        let price_momentum = pair.price_change.m5.unwrap_or(0.0) > 5.0 || 
                           pair.price_change.h1.unwrap_or(0.0) > 10.0;
        
        // Transaction momentum: Active buying
        let tx_momentum = pair.txns.h1.buys > 20; // 20+ buys in last hour
        
        // Liquidity filter: Minimum liquidity
        let adequate_liquidity = if let Some(ref liquidity) = pair.liquidity {
            liquidity.usd.unwrap_or(0.0) > 5000.0 // $5k+ liquidity
        } else {
            false
        };
        
        high_volume && price_momentum && tx_momentum && adequate_liquidity
    }
    
    /// Check if token is mature enough (10+ minutes old)
    fn is_mature_enough(&self, pair: &DexScreenerPair) -> bool {
        if let Some(created_at) = pair.pair_created_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            
            let age_minutes = (now - created_at) / (1000 * 60);
            age_minutes >= 10 // At least 10 minutes old
        } else {
            true // If no creation time, assume it's mature
        }
    }
    
    /// Get detailed momentum score for a token pair
    pub fn calculate_momentum_score(&self, pair: &DexScreenerPair) -> f64 {
        let mut score = 0.0;
        
        // Volume score (0-30 points)
        score += (pair.volume.h1 / 100.0).min(30.0);
        
        // Price momentum score (0-25 points)
        if let Some(price_change_1h) = pair.price_change.h1 {
            score += (price_change_1h / 2.0).min(25.0).max(0.0);
        }
        
        // Transaction activity score (0-25 points)
        let tx_score = (pair.txns.h1.buys as f64 / 4.0).min(25.0);
        score += tx_score;
        
        // Liquidity score (0-20 points)
        if let Some(ref liquidity) = pair.liquidity {
            if let Some(usd_liquidity) = liquidity.usd {
                score += (usd_liquidity / 1000.0).min(20.0);
            }
        }
        
        score.min(100.0) // Cap at 100
    }
    
    /// Print momentum summary for a pair
    pub fn print_momentum_pair(&self, pair: &DexScreenerPair) {
        let score = self.calculate_momentum_score(pair);
        
        println!("\n🔥 MOMENTUM DETECTED: {} ({})", pair.base_token.name, pair.base_token.symbol);
        println!("📊 Score: {:.1}/100", score);
        println!("💰 Volume (1h): ${:.0} | Price: ${}", pair.volume.h1, pair.price_usd.as_deref().unwrap_or("N/A"));
        println!("📈 Price Change (1h): {:.1}%", pair.price_change.h1.unwrap_or(0.0));
        println!("🔄 Trades (1h): {} buys, {} sells", pair.txns.h1.buys, pair.txns.h1.sells);
        
        if let Some(ref liquidity) = pair.liquidity {
            println!("💧 Liquidity: ${:.0}", liquidity.usd.unwrap_or(0.0));
        }
        
        println!("🌐 DexScreener: {}", pair.url);
        println!("🪙 Token: {}", pair.base_token.address);
    }
}