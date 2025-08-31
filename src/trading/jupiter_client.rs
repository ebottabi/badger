/// Jupiter API Client for Real Trade Execution
/// 
/// This module provides integration with Jupiter V6 API for executing
/// copy trades identified by the wallet intelligence system.

use anyhow::{Result, Context};
use reqwest::{Client, header::{HeaderMap, HeaderValue, CONTENT_TYPE}};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{info, warn, error, debug};
use tokio::time::Duration;
use solana_sdk::signature::Signer;

use crate::core::{TradingSignal, WalletManager, WalletType};

/// Jupiter V6 API client for swap execution
pub struct JupiterClient {
    /// HTTP client for API requests
    client: Client,
    /// Jupiter API base URL
    api_url: String,
    /// RPC endpoint for transaction submission
    rpc_url: String,
    /// Wallet manager for accessing trading wallet
    wallet_manager: std::sync::Arc<tokio::sync::RwLock<WalletManager>>,
    /// Maximum slippage tolerance (basis points, e.g., 100 = 1%)
    max_slippage_bps: u16,
    /// Priority fee in lamports (for faster transaction processing)
    priority_fee_lamports: u64,
}

/// Jupiter quote request for getting swap rates
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteRequest {
    /// Input token mint
    pub input_mint: String,
    /// Output token mint  
    pub output_mint: String,
    /// Amount to swap (in token's base units)
    pub amount: u64,
    /// Slippage tolerance in basis points (e.g., 100 = 1%)
    pub slippage_bps: u16,
    /// Whether to only use direct routes
    pub only_direct_routes: Option<bool>,
    /// Platform fee basis points
    pub platform_fee_bps: Option<u16>,
}

/// Jupiter quote response
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QuoteResponse {
    /// Input mint
    pub input_mint: String,
    /// Output mint
    pub output_mint: String,
    /// Input amount
    pub in_amount: String,
    /// Output amount
    pub out_amount: String,
    /// Other amount (for exact output)
    pub other_amount_threshold: String,
    /// Swap mode (ExactIn or ExactOut)
    pub swap_mode: String,
    /// Slippage basis points
    pub slippage_bps: u16,
    /// Platform fee
    pub platform_fee: Option<PlatformFee>,
    /// Price impact percentage
    pub price_impact_pct: String,
    /// Market infos for the route
    pub market_infos: Vec<MarketInfo>,
    /// Route plan
    pub route_plan: Vec<RoutePlanItem>,
    /// Context slot
    pub context_slot: u64,
    /// Time taken for quote
    pub time_taken: f64,
}

/// Platform fee information
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlatformFee {
    /// Fee amount
    pub amount: String,
    /// Fee basis points
    pub fee_bps: u16,
}

/// Market info for the swap route
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MarketInfo {
    /// AMM ID
    pub id: String,
    /// Label (e.g., "Raydium", "Orca")
    pub label: String,
    /// Input mint
    pub input_mint: String,
    /// Output mint
    pub output_mint: String,
    /// Not enough liquidity flag
    pub not_enough_liquidity: bool,
    /// In amount
    pub in_amount: String,
    /// Out amount
    pub out_amount: String,
    /// Price impact percentage
    pub price_impact_pct: Option<String>,
    /// LP fee
    pub lp_fee: Option<LpFee>,
    /// Platform fee
    pub platform_fee: Option<PlatformFee>,
}

/// LP fee information
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LpFee {
    /// Fee amount
    pub amount: String,
    /// Fee mint
    pub mint: String,
    /// Fee basis points
    pub pct: String,
}

/// Route plan item
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]  
pub struct RoutePlanItem {
    /// Swap info
    pub swap_info: SwapInfo,
    /// Percent
    pub percent: u8,
}

/// Swap info within route plan
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SwapInfo {
    /// AMM key
    pub amm_key: String,
    /// Label
    pub label: String,
    /// Input mint
    pub input_mint: String,
    /// Output mint
    pub output_mint: String,
    /// In amount
    pub in_amount: String,
    /// Out amount
    pub out_amount: String,
    /// Fee amount
    pub fee_amount: String,
    /// Fee mint
    pub fee_mint: String,
}

/// Swap request for executing trades
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapRequest {
    /// User's public key
    pub user_public_key: String,
    /// Quote response from Jupiter
    pub quote_response: QuoteResponse,
    /// Configuration for the swap
    pub config: SwapConfig,
}

/// Swap configuration
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapConfig {
    /// Wrap and unwrap SOL automatically
    pub wrap_and_unwrap_sol: bool,
    /// Use shared accounts
    pub use_shared_accounts: bool,
    /// Fee account (for platform fees)
    pub fee_account: Option<String>,
    /// Priority fee in lamports
    pub compute_unit_price_micro_lamports: Option<u64>,
    /// Dynamic compute units
    pub dynamic_compute_unit_limit: Option<bool>,
}

/// Jupiter swap response with transaction
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapResponse {
    /// Serialized swap transaction
    pub swap_transaction: String,
    /// Last valid block height
    pub last_valid_block_height: u64,
    /// Priority fee information
    pub priority_fee_info: Option<PriorityFeeInfo>,
    /// Dynamic slippage report
    pub dynamic_slippage_report: Option<Value>,
    /// Simulate error (if any)
    pub simulate_error: Option<Value>,
}

/// Priority fee information
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorityFeeInfo {
    /// Compute budget instructions
    pub compute_budget_instructions: Vec<Value>,
    /// Priority fee
    pub priority_fee_lamports: u64,
}

/// Trade execution result
#[derive(Debug, Clone)]
pub struct TradeExecutionResult {
    /// Success flag
    pub success: bool,
    /// Transaction signature (if successful)
    pub signature: Option<String>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Input amount actually swapped
    pub input_amount: Option<u64>,
    /// Output amount received
    pub output_amount: Option<u64>,
    /// Price impact percentage
    pub price_impact: Option<f64>,
    /// Gas fee paid
    pub gas_fee_lamports: Option<u64>,
    /// Execution time in milliseconds
    pub execution_time_ms: u64,
}

impl JupiterClient {
    /// Create new Jupiter client with wallet manager
    pub fn new(
        wallet_manager: std::sync::Arc<tokio::sync::RwLock<WalletManager>>,
        api_url: Option<String>,
        rpc_url: Option<String>,
        max_slippage_bps: Option<u16>,
        priority_fee_lamports: Option<u64>,
    ) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        
        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            api_url: api_url.unwrap_or_else(|| "https://quote-api.jup.ag/v6".to_string()),
            rpc_url: rpc_url.unwrap_or_else(|| "https://api.mainnet-beta.solana.com".to_string()),
            wallet_manager,
            max_slippage_bps: max_slippage_bps.unwrap_or(100), // 1% default
            priority_fee_lamports: priority_fee_lamports.unwrap_or(10000), // 0.00001 SOL
        }
    }

    /// Get trading wallet keypair from wallet manager
    pub async fn get_trading_keypair(&self) -> Result<solana_sdk::signature::Keypair> {
        let wallet_manager = self.wallet_manager.read().await;
        let keypair = wallet_manager.get_keypair(&WalletType::Trading)
            .context("Failed to get trading wallet keypair")?;
        
        // Create a copy of the keypair since we can't return a reference
        let keypair_bytes = keypair.to_bytes();
        solana_sdk::signature::Keypair::from_bytes(&keypair_bytes)
            .context("Failed to recreate keypair from bytes")
    }

    /// Get trading wallet public key
    pub async fn get_trading_pubkey(&self) -> Result<solana_sdk::pubkey::Pubkey> {
        let wallet_manager = self.wallet_manager.read().await;
        wallet_manager.get_public_key(&WalletType::Trading)
            .context("Failed to get trading wallet public key")
    }

    /// Get quote for a token swap
    pub async fn get_quote(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
        slippage_bps: Option<u16>,
    ) -> Result<QuoteResponse> {
        let quote_request = QuoteRequest {
            input_mint: input_mint.to_string(),
            output_mint: output_mint.to_string(),
            amount,
            slippage_bps: slippage_bps.unwrap_or(self.max_slippage_bps),
            only_direct_routes: Some(false),
            platform_fee_bps: None,
        };

        debug!("🔍 Getting Jupiter quote: {} {} -> {}", 
               amount, input_mint, output_mint);

        let params = [
            ("inputMint", quote_request.input_mint.as_str()),
            ("outputMint", quote_request.output_mint.as_str()),
            ("amount", &amount.to_string()),
            ("slippageBps", &quote_request.slippage_bps.to_string()),
        ];

        let url = format!("{}/quote", self.api_url);
        
        let response = self.client
            .get(&url)
            .query(&params)
            .send()
            .await
            .context("Failed to request quote from Jupiter")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            anyhow::bail!("Jupiter quote request failed with status {}: {}", 
                         status, error_text);
        }

        let quote: QuoteResponse = response
            .json()
            .await
            .context("Failed to parse Jupiter quote response")?;

        debug!("📊 Quote received: {} {} -> {} {} (impact: {}%)", 
               quote.in_amount, input_mint, 
               quote.out_amount, output_mint,
               quote.price_impact_pct);

        Ok(quote)
    }

    /// Execute a trade based on TradingSignal
    pub async fn execute_trade(&self, signal: &TradingSignal) -> Result<TradeExecutionResult> {
        let start_time = std::time::Instant::now();

        // Get trading wallet keypair from wallet manager
        let wallet_keypair = self.get_trading_keypair().await?;

        // Extract trade parameters from signal
        let (input_mint, output_mint, amount) = match signal {
            TradingSignal::Buy { token_mint, max_amount_sol, .. } => {
                // Convert SOL amount to lamports
                let sol_lamports = (max_amount_sol * 1_000_000_000.0) as u64;
                ("So11111111111111111111111111111111111112".to_string(), // SOL mint
                 token_mint.clone(),
                 sol_lamports)
            }
            TradingSignal::Sell { token_mint, amount_tokens, .. } => {
                (token_mint.clone(),
                 "So11111111111111111111111111111111111112".to_string(), // SOL mint
                 amount_tokens.unwrap_or(0.0) as u64)
            }
            _ => {
                return Ok(TradeExecutionResult {
                    success: false,
                    signature: None,
                    error: Some("Unsupported signal type for execution".to_string()),
                    input_amount: None,
                    output_amount: None,
                    price_impact: None,
                    gas_fee_lamports: None,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                });
            }
        };

        info!("⚡ Executing trade: {} {} -> {} {}", 
              amount, input_mint, amount, output_mint);

        // Get quote from Jupiter
        let quote = match self.get_quote(&input_mint, &output_mint, amount, None).await {
            Ok(quote) => quote,
            Err(e) => {
                error!("Failed to get Jupiter quote: {}", e);
                return Ok(TradeExecutionResult {
                    success: false,
                    signature: None,
                    error: Some(format!("Quote failed: {}", e)),
                    input_amount: None,
                    output_amount: None,
                    price_impact: None,
                    gas_fee_lamports: None,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                });
            }
        };

        // Create swap request
        let swap_request = SwapRequest {
            user_public_key: wallet_keypair.pubkey().to_string(),
            quote_response: quote.clone(),
            config: SwapConfig {
                wrap_and_unwrap_sol: true,
                use_shared_accounts: true,
                fee_account: None,
                compute_unit_price_micro_lamports: Some(self.priority_fee_lamports),
                dynamic_compute_unit_limit: Some(true),
            },
        };

        // Get swap transaction from Jupiter
        let swap_response = match self.get_swap_transaction(swap_request).await {
            Ok(response) => response,
            Err(e) => {
                error!("Failed to get swap transaction: {}", e);
                return Ok(TradeExecutionResult {
                    success: false,
                    signature: None,
                    error: Some(format!("Swap transaction failed: {}", e)),
                    input_amount: None,
                    output_amount: None,
                    price_impact: None,
                    gas_fee_lamports: None,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                });
            }
        };

        // Sign and submit transaction
        match self.sign_and_submit_transaction(&swap_response, &wallet_keypair).await {
            Ok(signature) => {
                info!("✅ Trade executed successfully: {}", signature);
                
                // Parse amounts from quote
                let input_amount = quote.in_amount.parse().ok();
                let output_amount = quote.out_amount.parse().ok();
                let price_impact = quote.price_impact_pct.parse().ok();

                Ok(TradeExecutionResult {
                    success: true,
                    signature: Some(signature),
                    error: None,
                    input_amount,
                    output_amount,
                    price_impact,
                    gas_fee_lamports: Some(self.priority_fee_lamports),
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                })
            }
            Err(e) => {
                error!("Failed to submit transaction: {}", e);
                Ok(TradeExecutionResult {
                    success: false,
                    signature: None,
                    error: Some(format!("Transaction submission failed: {}", e)),
                    input_amount: None,
                    output_amount: None,
                    price_impact: None,
                    gas_fee_lamports: None,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                })
            }
        }
    }

    /// Get swap transaction from Jupiter API
    async fn get_swap_transaction(&self, swap_request: SwapRequest) -> Result<SwapResponse> {
        let url = format!("{}/swap", self.api_url);
        
        debug!("📤 Requesting swap transaction from Jupiter");
        
        let response = self.client
            .post(&url)
            .json(&swap_request)
            .send()
            .await
            .context("Failed to request swap transaction from Jupiter")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            anyhow::bail!("Jupiter swap transaction request failed with status {}: {}", 
                         status, error_text);
        }

        let swap_response: SwapResponse = response
            .json()
            .await
            .context("Failed to parse Jupiter swap response")?;

        debug!("📥 Swap transaction received from Jupiter");
        Ok(swap_response)
    }

    /// Sign and submit transaction to Solana network
    async fn sign_and_submit_transaction(
        &self,
        swap_response: &SwapResponse,
        wallet_keypair: &solana_sdk::signature::Keypair,
    ) -> Result<String> {
        use solana_sdk::transaction::Transaction;
        use solana_client::rpc_client::RpcClient;
        use solana_client::rpc_config::RpcSendTransactionConfig;
        
        // Deserialize transaction from base64
        let transaction_bytes = base64::decode(&swap_response.swap_transaction)
            .context("Failed to decode swap transaction")?;
        
        let mut transaction: Transaction = bincode::deserialize(&transaction_bytes)
            .context("Failed to deserialize transaction")?;

        // Sign the transaction
        transaction.sign(&[wallet_keypair], transaction.message.recent_blockhash);
        
        debug!("✍️ Transaction signed, submitting to network");

        // Create RPC client
        let rpc_client = RpcClient::new(self.rpc_url.clone());
        
        // Submit transaction with retry logic
        let config = RpcSendTransactionConfig {
            skip_preflight: false,
            preflight_commitment: None,
            encoding: None,
            max_retries: Some(3),
            min_context_slot: None,
        };

        let signature = rpc_client
            .send_and_confirm_transaction_with_spinner_and_config(
                &transaction, 
                rpc_client.commitment(),
                config
            )
            .context("Failed to send and confirm transaction")?;

        Ok(signature.to_string())
    }

    /// Check if trading is currently safe (price impact, liquidity, etc.)
    pub async fn is_safe_to_trade(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
    ) -> Result<bool> {
        // Get quote to check price impact and liquidity
        let quote = self.get_quote(input_mint, output_mint, amount, None).await?;
        
        // Parse price impact
        let price_impact: f64 = quote.price_impact_pct.parse().unwrap_or(100.0);
        
        // Safety checks
        let max_price_impact = 5.0; // 5% maximum price impact
        let min_output_amount = 1000; // Minimum viable output
        
        if price_impact > max_price_impact {
            warn!("⚠️ High price impact: {}% (max: {}%)", price_impact, max_price_impact);
            return Ok(false);
        }
        
        let output_amount: u64 = quote.out_amount.parse().unwrap_or(0);
        if output_amount < min_output_amount {
            warn!("⚠️ Low liquidity: output amount {} (min: {})", output_amount, min_output_amount);
            return Ok(false);
        }
        
        debug!("✅ Trade safety check passed: {}% impact, {} output", price_impact, output_amount);
        Ok(true)
    }

    /// Get current market price for a token pair
    pub async fn get_market_price(&self, input_mint: &str, output_mint: &str) -> Result<f64> {
        // Use small amount to get current exchange rate
        let test_amount = 1_000_000; // 0.001 SOL or equivalent
        
        let quote = self.get_quote(input_mint, output_mint, test_amount, Some(50)).await?;
        
        let input_amount: u64 = quote.in_amount.parse().unwrap_or(0);
        let output_amount: u64 = quote.out_amount.parse().unwrap_or(0);
        
        if input_amount == 0 {
            anyhow::bail!("Invalid quote: zero input amount");
        }
        
        let price = output_amount as f64 / input_amount as f64;
        Ok(price)
    }
}