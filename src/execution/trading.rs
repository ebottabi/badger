/// Trading client for pump.fun interactions

use anyhow::Result;
use serde_json::json;
use reqwest::Client;
use std::sync::Arc;
use crate::util::price_feed::PriceFeed;

pub struct TradingClient {
    wallet_pubkey: String,
    api_key: String,
    client: Client,
    slippage_percent: f64,
    max_retry_attempts: u32,
    priority_fee_sol: f64,
    price_feed: Arc<PriceFeed>,
}

impl TradingClient {
    pub fn new(wallet_pubkey: String, api_key: String, slippage_percent: f64, max_retry_attempts: u32, priority_fee_sol: f64) -> Self {
        Self {
            wallet_pubkey,
            api_key,
            client: Client::new(),
            slippage_percent,
            max_retry_attempts,
            priority_fee_sol,
            price_feed: Arc::new(PriceFeed::new()),
        }
    }
    
    pub async fn buy_token(&self, mint: &str, sol_amount: f64) -> Result<String> {
        let payload = json!({
            "action": "buy",
            "mint": mint,
            "amount": sol_amount,
            "denominatedInSol": "true",
            "slippage": self.slippage_percent,
            "priorityFee": self.priority_fee_sol,
            "pool": "auto"
        });
        
        for attempt in 1..=self.max_retry_attempts {
            let response = self.client
                .post("https://pumpportal.fun/api/trade")
                .query(&[("api-key", &self.api_key)])
                .json(&payload)
                .send()
                .await?;
            
            if response.status().is_success() {
                let tx_data: serde_json::Value = response.json().await?;
                let tx_id = tx_data["signature"].as_str()
                    .unwrap_or("unknown").to_string();
                
                println!("🚀 BUY EXECUTED:");
                println!("   Token: {}", mint);
                println!("   Amount: {:.4} SOL", sol_amount);
                println!("   💳 Wallet: {}", self.wallet_pubkey);
                println!("   🔗 Transaction: https://solscan.io/tx/{}", tx_id);
                println!("   📊 Token Details: https://solscan.io/token/{}", mint);
                println!("   📈 Wallet Activity: https://solscan.io/account/{}?tab=transfers", self.wallet_pubkey);
                
                return Ok(tx_id);
            } else if attempt < self.max_retry_attempts {
                let error_text = response.text().await.unwrap_or_default();
                println!("⚠️ Buy attempt {} failed: {}, retrying...", attempt, error_text);
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            } else {
                let error_text = response.text().await?;
                return Err(anyhow::anyhow!("Buy failed after {} attempts: {}", self.max_retry_attempts, error_text));
            }
        }
        
        Err(anyhow::anyhow!("Buy failed: max retries exceeded"))
    }
    
    pub async fn execute_buy_with_balance_query(&self, mint: &str, sol_amount: f64, market_cap_sol: Option<f64>) -> Result<(String, f64, f64)> {
        let pre_balance = self.get_token_balance(mint).await.unwrap_or(0.0);
        
        let tx_id = self.buy_token(mint, sol_amount).await?;
        
        // Wait and retry for transaction confirmation
        let mut tokens_received = 0.0;
        for attempt in 1..=5 {
            tokio::time::sleep(tokio::time::Duration::from_secs(2 * attempt)).await;
            
            let post_balance = self.get_token_balance(mint).await.unwrap_or(0.0);
            tokens_received = post_balance - pre_balance;
            
            if tokens_received > 0.0 {
                println!("✅ Tokens received after {} attempts ({} sec wait)", attempt, 2 * attempt);
                break;
            } else if attempt == 5 {
                println!("⚠️ No tokens received after 5 attempts, continuing with price estimation");
                // If balance query fails, estimate tokens from trade amount
                match self.price_feed.get_token_price_with_fallbacks(mint, market_cap_sol).await {
                    Ok(estimated_price) => {
                        tokens_received = sol_amount / estimated_price;
                        println!("📊 Using price estimation: {:.2} tokens at price {:.10}", tokens_received, estimated_price);
                    },
                    Err(e) => {
                        println!("⚠️ Price estimation also failed: {}", e);
                        // Use a rough fallback based on typical new token prices
                        tokens_received = sol_amount / 0.000001; // Assume very small price
                        println!("📊 Using emergency fallback estimation: {:.0} tokens", tokens_received);
                    }
                }
            }
        }
        
        // Calculate effective price
        let effective_price = sol_amount / tokens_received;
        
        // Get market price for comparison using fallbacks
        let market_price = self.price_feed.get_token_price_with_fallbacks(mint, market_cap_sol).await
            .unwrap_or(effective_price);
        
        println!("📊 TRADE ANALYSIS:");
        println!("   Tokens received: {:.2}", tokens_received);
        println!("   Effective price: {:.10} SOL", effective_price);
        println!("   Market price: {:.10} SOL", market_price);
        
        Ok((tx_id, tokens_received, effective_price))
    }
    
    pub async fn sell_token(&self, mint: &str, percentage: f64) -> Result<String> {
        // Get current token balance first
        let balance = self.get_token_balance(mint).await?;
        let sell_amount = balance * (percentage / 100.0);
        
        let payload = json!({
            "action": "sell",
            "mint": mint,
            "amount": sell_amount,
            "denominatedInSol": "false",
            "slippage": self.slippage_percent,
            "priorityFee": self.priority_fee_sol,
            "pool": "auto"
        });
        
        for attempt in 1..=self.max_retry_attempts {
            let response = self.client
                .post("https://pumpportal.fun/api/trade")
                .query(&[("api-key", &self.api_key)])
                .json(&payload)
                .send()
                .await?;
            
            if response.status().is_success() {
                let tx_data: serde_json::Value = response.json().await?;
                let tx_id = tx_data["signature"].as_str()
                    .unwrap_or("unknown").to_string();
                
                println!("💸 SELL EXECUTED:");
                println!("   Token: {}", mint);
                println!("   Amount: {:.0} tokens ({}%)", sell_amount, percentage);
                println!("   💳 Wallet: {}", self.wallet_pubkey);
                println!("   🔗 Transaction: https://solscan.io/tx/{}", tx_id);
                println!("   📊 Token Details: https://solscan.io/token/{}", mint);
                println!("   📈 Wallet Activity: https://solscan.io/account/{}?tab=transfers", self.wallet_pubkey);
                
                return Ok(tx_id);
            } else if attempt < self.max_retry_attempts {
                let error_text = response.text().await.unwrap_or_default();
                println!("⚠️ Sell attempt {} failed: {}, retrying...", attempt, error_text);
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            } else {
                let error_text = response.text().await?;
                return Err(anyhow::anyhow!("Sell failed after {} attempts: {}", self.max_retry_attempts, error_text));
            }
        }
        
        Err(anyhow::anyhow!("Sell failed: max retries exceeded"))
    }
    
    pub async fn get_token_balance(&self, mint: &str) -> Result<f64> {
        // Use Solana RPC to get actual token balance
        use solana_client::rpc_client::RpcClient;
        use solana_sdk::pubkey::Pubkey;
        use std::str::FromStr;
        
        let client = RpcClient::new("https://api.mainnet-beta.solana.com".to_string());
        let wallet_pubkey = Pubkey::from_str(&self.wallet_pubkey)?;
        let mint_pubkey = Pubkey::from_str(mint)?;
        
        // Get associated token account
        let token_account = spl_associated_token_account::get_associated_token_address(
            &wallet_pubkey, &mint_pubkey
        );
        
        match client.get_token_account_balance(&token_account) {
            Ok(balance) => Ok(balance.ui_amount.unwrap_or(0.0)),
            Err(_) => Ok(0.0), // Account doesn't exist or no balance
        }
    }
    
    pub async fn get_current_price(&self, mint: &str) -> Result<f64> {
        self.price_feed.get_token_price(mint).await
    }
    
    pub async fn estimate_tokens_for_sol(&self, mint: &str, sol_amount: f64) -> Result<f64> {
        let price = self.get_current_price(mint).await?;
        Ok(sol_amount / price)
    }
    
    pub fn get_verification_links(&self, mint: &str) -> (String, String, String) {
        (
            format!("https://rugcheck.xyz/tokens/{}", mint),
            format!("https://dexscreener.com/solana/{}", mint),
            format!("https://pump.fun/{}", mint),
        )
    }
    
    pub fn get_wallet_address(&self) -> &str {
        &self.wallet_pubkey
    }
}