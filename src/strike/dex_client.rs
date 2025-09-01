use anyhow::{Result, Context, bail};
use serde::{Deserialize, Serialize};
use tokio::time::{timeout, Duration};
use tracing::{info, debug, warn, error, instrument};
use reqwest::Client;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    transaction::Transaction,
    instruction::Instruction,
    signature::{Signature, Keypair, Signer},
    commitment_config::CommitmentConfig,
};
use std::str::FromStr;
use std::collections::HashMap;

/// Configuration for DEX operations
#[derive(Debug, Clone)]
pub struct DexConfig {
    /// Solana RPC endpoint for transaction submission
    pub rpc_endpoint: String,
    /// Jupiter API base URL
    pub jupiter_api_url: String,
    /// Maximum slippage tolerance in basis points (100 = 1%)
    pub max_slippage_bps: u16,
    /// Priority fee in lamports for transaction priority
    pub priority_fee_lamports: u64,
    /// Request timeout in seconds
    pub request_timeout_secs: u64,
    /// Maximum retries for failed transactions
    pub max_retries: u32,
}

impl Default for DexConfig {
    fn default() -> Self {
        Self {
            rpc_endpoint: "https://api.mainnet-beta.solana.com".to_string(),
            jupiter_api_url: "https://quote-api.jup.ag/v6".to_string(),
            max_slippage_bps: 50, // 0.5% default slippage
            priority_fee_lamports: 1000, // 0.000001 SOL priority fee
            request_timeout_secs: 30,
            max_retries: 3,
        }
    }
}

/// Swap parameters for DEX operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapRequest {
    /// Input token mint address
    pub input_mint: String,
    /// Output token mint address
    pub output_mint: String,
    /// Input amount in token's smallest unit
    pub amount: u64,
    /// Maximum slippage tolerance in basis points
    pub slippage_bps: u16,
    /// User's wallet public key
    pub user_public_key: String,
    /// Whether to auto-create token accounts if needed
    pub auto_create_token_accounts: bool,
}

/// Result of a swap operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapResult {
    /// Transaction signature
    pub signature: String,
    /// Input token mint
    pub input_mint: String,
    /// Output token mint
    pub output_mint: String,
    /// Actual input amount used
    pub input_amount: u64,
    /// Actual output amount received
    pub output_amount: u64,
    /// Transaction fee paid (in lamports)
    pub fee_lamports: u64,
    /// Price impact as percentage
    pub price_impact_percent: Option<f64>,
    /// Route information
    pub route_info: Option<RouteInfo>,
}

/// Route information for executed swap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteInfo {
    /// DEX(es) used in the route
    pub dexes: Vec<String>,
    /// Intermediate tokens (for multi-hop swaps)
    pub intermediate_tokens: Vec<String>,
    /// Market IDs used
    pub market_ids: Vec<String>,
}

/// Jupiter API quote response structure
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JupiterQuote {
    /// Input mint
    pub input_mint: String,
    /// Input amount
    pub in_amount: String,
    /// Output mint  
    pub output_mint: String,
    /// Output amount
    pub out_amount: String,
    /// Other amount threshold
    pub other_amount_threshold: String,
    /// Swap mode
    pub swap_mode: String,
    /// Slippage basis points
    pub slippage_bps: u16,
    /// Platform fee basis points
    pub platform_fee: Option<PlatformFee>,
    /// Price impact percentage
    pub price_impact_pct: String,
    /// Route plan
    pub route_plan: Vec<RoutePlan>,
    /// Context slot
    pub context_slot: u64,
    /// Time taken for quote
    pub time_taken: f64,
}

/// Platform fee structure
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformFee {
    /// Fee amount
    pub amount: String,
    /// Fee account
    pub fee_bps: u16,
}

/// Route plan step
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePlan {
    /// Swap information
    pub swap_info: SwapInfo,
    /// Percentage of route
    pub percent: u8,
}

/// Swap information for route step
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapInfo {
    /// AMM key
    pub amm_key: String,
    /// Label (DEX name)
    pub label: String,
    /// Input mint
    pub input_mint: String,
    /// Output mint
    pub output_mint: String,
    /// Input amount
    pub in_amount: String,
    /// Output amount
    pub out_amount: String,
    /// Fee amount
    pub fee_amount: String,
    /// Fee mint
    pub fee_mint: String,
}

/// Jupiter API swap response
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JupiterSwapResponse {
    /// Serialized transaction
    pub swap_transaction: String,
    /// Last valid block height
    pub last_valid_block_height: Option<u64>,
}

/// Unified DEX client for multiple DEX integrations
pub struct DexClient {
    /// HTTP client for API requests
    http_client: Client,
    /// Solana RPC client
    rpc_client: RpcClient,
    /// Configuration
    config: DexConfig,
    /// Jupiter client
    jupiter_client: JupiterClient,
}

impl DexClient {
    /// Creates a new DEX client with the given configuration
    /// 
    /// # Arguments
    /// * `config` - DEX client configuration
    /// 
    /// # Returns
    /// * `Result<Self>` - DEX client instance
    #[instrument]
    pub fn new(config: DexConfig) -> Result<Self> {
        info!("Initializing DEX client with RPC: {}", config.rpc_endpoint);
        
        // Create HTTP client with timeout
        let http_client = Client::builder()
            .timeout(Duration::from_secs(config.request_timeout_secs))
            .build()
            .context("Failed to create HTTP client")?;
        
        // Create Solana RPC client
        let rpc_client = RpcClient::new_with_commitment(
            config.rpc_endpoint.clone(),
            CommitmentConfig::confirmed()
        );
        
        // Create Jupiter client
        let jupiter_client = JupiterClient::new(
            config.jupiter_api_url.clone(),
            http_client.clone(),
        );
        
        info!("DEX client initialized successfully");
        
        Ok(Self {
            http_client,
            rpc_client,
            config,
            jupiter_client,
        })
    }
    
    /// Executes a token swap using the best available route
    /// 
    /// # Arguments
    /// * `swap_request` - Swap parameters
    /// * `wallet_keypair` - User's wallet keypair for signing
    /// 
    /// # Returns
    /// * `Result<SwapResult>` - Result of the swap operation
    #[instrument(skip(self, _wallet_keypair))]
    pub async fn execute_swap(
        &self,
        swap_request: &SwapRequest,
        _wallet_keypair: &Keypair,
    ) -> Result<SwapResult> {
        info!(
            input_mint = %swap_request.input_mint,
            output_mint = %swap_request.output_mint,
            amount = swap_request.amount,
            slippage_bps = swap_request.slippage_bps,
            "Executing token swap"
        );
        
        // Get quote from Jupiter (best aggregator)
        let quote = self.jupiter_client.get_quote(swap_request).await?;
        
        info!(
            input_amount = %quote.in_amount,
            output_amount = %quote.out_amount,
            price_impact = %quote.price_impact_pct,
            route_steps = quote.route_plan.len(),
            "Received swap quote from Jupiter"
        );
        
        // Get swap transaction from Jupiter
        let swap_transaction = self.jupiter_client.get_swap_transaction(&quote, _wallet_keypair).await?;
        
        // Execute the transaction
        let signature = self.submit_transaction(&swap_transaction, _wallet_keypair).await?;
        
        // Parse amounts from quote
        let input_amount = quote.in_amount.parse::<u64>()
            .context("Failed to parse input amount")?;
        let output_amount = quote.out_amount.parse::<u64>()
            .context("Failed to parse output amount")?;
        
        // Extract route information
        let route_info = self.extract_route_info(&quote)?;
        
        // Calculate actual fee (this would normally come from transaction confirmation)
        let fee_lamports = self.config.priority_fee_lamports + 5000; // Base fee estimate
        
        let result = SwapResult {
            signature: signature.to_string(),
            input_mint: swap_request.input_mint.clone(),
            output_mint: swap_request.output_mint.clone(),
            input_amount,
            output_amount,
            fee_lamports,
            price_impact_percent: quote.price_impact_pct.parse().ok(),
            route_info: Some(route_info),
        };
        
        info!(
            signature = %result.signature,
            input_amount = result.input_amount,
            output_amount = result.output_amount,
            fee_lamports = result.fee_lamports,
            "Swap executed successfully"
        );
        
        Ok(result)
    }
    
    /// Submits a transaction to the Solana network with retry logic
    /// 
    /// # Arguments
    /// * `transaction` - Transaction to submit
    /// * `wallet_keypair` - Signing keypair
    /// 
    /// # Returns
    /// * `Result<Signature>` - Transaction signature
    #[instrument(skip(self, transaction, _wallet_keypair))]
    async fn submit_transaction(
        &self,
        transaction: &Transaction,
        _wallet_keypair: &Keypair,
    ) -> Result<Signature> {
        let mut attempts = 0;
        let max_attempts = self.config.max_retries + 1;
        
        while attempts < max_attempts {
            attempts += 1;
            
            debug!(attempt = attempts, max_attempts, "Submitting transaction to network");
            
            match self.rpc_client.send_and_confirm_transaction(transaction) {
                Ok(signature) => {
                    info!(
                        signature = %signature,
                        attempts = attempts,
                        "Transaction confirmed successfully"
                    );
                    return Ok(signature);
                }
                Err(e) => {
                    error!(
                        error = %e,
                        attempt = attempts,
                        max_attempts = max_attempts,
                        "Transaction submission failed"
                    );
                    
                    if attempts >= max_attempts {
                        return Err(anyhow::anyhow!("Transaction failed after {} attempts: {}", max_attempts, e));
                    }
                    
                    // Wait before retrying (exponential backoff)
                    let delay_ms = 1000 * (2_u64.pow(attempts - 1));
                    debug!(delay_ms = delay_ms, "Waiting before retry");
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
        
        unreachable!("Should have returned or failed in the loop above")
    }
    
    /// Extracts route information from Jupiter quote
    /// 
    /// # Arguments
    /// * `quote` - Jupiter quote response
    /// 
    /// # Returns
    /// * `Result<RouteInfo>` - Extracted route information
    fn extract_route_info(&self, quote: &JupiterQuote) -> Result<RouteInfo> {
        let mut dexes = Vec::new();
        let mut intermediate_tokens = Vec::new();
        let mut market_ids = Vec::new();
        
        for route_step in &quote.route_plan {
            let swap_info = &route_step.swap_info;
            
            // Collect unique DEX names
            if !dexes.contains(&swap_info.label) {
                dexes.push(swap_info.label.clone());
            }
            
            // Collect intermediate tokens (excluding input/output)
            if swap_info.input_mint != quote.input_mint 
                && swap_info.input_mint != quote.output_mint 
                && !intermediate_tokens.contains(&swap_info.input_mint) {
                intermediate_tokens.push(swap_info.input_mint.clone());
            }
            
            if swap_info.output_mint != quote.input_mint 
                && swap_info.output_mint != quote.output_mint 
                && !intermediate_tokens.contains(&swap_info.output_mint) {
                intermediate_tokens.push(swap_info.output_mint.clone());
            }
            
            // Collect market IDs
            market_ids.push(swap_info.amm_key.clone());
        }
        
        Ok(RouteInfo {
            dexes,
            intermediate_tokens,
            market_ids,
        })
    }
    
    /// Gets the current price for a token pair without executing a swap
    /// 
    /// # Arguments
    /// * `input_mint` - Input token mint
    /// * `output_mint` - Output token mint
    /// * `amount` - Amount to get price for
    /// 
    /// # Returns
    /// * `Result<f64>` - Price ratio (output/input)
    #[instrument(skip(self))]
    pub async fn get_price(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
    ) -> Result<f64> {
        debug!(
            input_mint = input_mint,
            output_mint = output_mint,
            amount = amount,
            "Getting price quote"
        );
        
        let swap_request = SwapRequest {
            input_mint: input_mint.to_string(),
            output_mint: output_mint.to_string(),
            amount,
            slippage_bps: self.config.max_slippage_bps,
            user_public_key: "11111111111111111111111111111111".to_string(), // Dummy key for quote
            auto_create_token_accounts: false,
        };
        
        let quote = self.jupiter_client.get_quote(&swap_request).await?;
        
        let input_amount = quote.in_amount.parse::<f64>()
            .context("Failed to parse input amount")?;
        let output_amount = quote.out_amount.parse::<f64>()
            .context("Failed to parse output amount")?;
        
        let price = if input_amount > 0.0 {
            output_amount / input_amount
        } else {
            0.0
        };
        
        debug!(
            input_amount = input_amount,
            output_amount = output_amount,
            price = price,
            "Price quote retrieved"
        );
        
        Ok(price)
    }
}

/// Jupiter API client for swap aggregation
struct JupiterClient {
    /// API base URL
    api_url: String,
    /// HTTP client
    http_client: Client,
}

impl JupiterClient {
    /// Creates a new Jupiter client
    /// 
    /// # Arguments
    /// * `api_url` - Jupiter API base URL
    /// * `http_client` - HTTP client for requests
    /// 
    /// # Returns
    /// * `Self` - Jupiter client instance
    fn new(api_url: String, http_client: Client) -> Self {
        Self {
            api_url,
            http_client,
        }
    }
    
    /// Gets a quote from Jupiter API
    /// 
    /// # Arguments
    /// * `swap_request` - Swap parameters
    /// 
    /// # Returns
    /// * `Result<JupiterQuote>` - Quote from Jupiter
    #[instrument(skip(self))]
    async fn get_quote(&self, swap_request: &SwapRequest) -> Result<JupiterQuote> {
        let url = format!("{}/quote", self.api_url);
        
        let amount_str = swap_request.amount.to_string();
        let slippage_str = swap_request.slippage_bps.to_string();
        
        let mut params = HashMap::new();
        params.insert("inputMint", swap_request.input_mint.as_str());
        params.insert("outputMint", swap_request.output_mint.as_str());
        params.insert("amount", &amount_str);
        params.insert("slippageBps", &slippage_str);
        
        debug!(url = %url, params = ?params, "Requesting quote from Jupiter");
        
        let response = timeout(
            Duration::from_secs(30),
            self.http_client.get(&url).query(&params).send()
        ).await
            .context("Jupiter API request timeout")?
            .context("Failed to send Jupiter quote request")?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            bail!("Jupiter quote request failed with status {}: {}", status, error_text);
        }
        
        let quote: JupiterQuote = response.json().await
            .context("Failed to parse Jupiter quote response")?;
        
        debug!(
            input_amount = %quote.in_amount,
            output_amount = %quote.out_amount,
            price_impact = %quote.price_impact_pct,
            "Received quote from Jupiter"
        );
        
        Ok(quote)
    }
    
    /// Gets a swap transaction from Jupiter API
    /// 
    /// # Arguments
    /// * `quote` - Jupiter quote
    /// * `wallet_keypair` - User's wallet keypair
    /// 
    /// # Returns
    /// * `Result<Transaction>` - Swap transaction ready for signing
    #[instrument(skip(self, _wallet_keypair))]
    async fn get_swap_transaction(
        &self,
        quote: &JupiterQuote,
        _wallet_keypair: &Keypair,
    ) -> Result<Transaction> {
        let url = format!("{}/swap", self.api_url);
        
        let request_body = serde_json::json!({
            "quoteResponse": quote,
            "userPublicKey": _wallet_keypair.pubkey().to_string(),
            "wrapAndUnwrapSol": true,
            "useSharedAccounts": true,
            "feeAccount": serde_json::Value::Null,
            "dynamicComputeUnitLimit": true,
            "prioritizationFeeLamports": "auto"
        });
        
        debug!(url = %url, "Requesting swap transaction from Jupiter");
        
        let response = timeout(
            Duration::from_secs(30),
            self.http_client.post(&url).json(&request_body).send()
        ).await
            .context("Jupiter swap API request timeout")?
            .context("Failed to send Jupiter swap request")?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            bail!("Jupiter swap request failed with status {}: {}", status, error_text);
        }
        
        let swap_response: JupiterSwapResponse = response.json().await
            .context("Failed to parse Jupiter swap response")?;
        
        // Decode the base64 transaction
        let transaction_bytes = base64::decode(&swap_response.swap_transaction)
            .context("Failed to decode swap transaction")?;
        
        let mut transaction: Transaction = bincode::deserialize(&transaction_bytes)
            .context("Failed to deserialize swap transaction")?;
        
        // Sign the transaction
        transaction.partial_sign(&[_wallet_keypair], transaction.message.recent_blockhash);
        
        debug!("Swap transaction prepared and signed");
        
        Ok(transaction)
    }
}