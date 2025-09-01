/// Comprehensive Fund Management System
/// 
/// This module provides advanced fund management capabilities including:
/// - Secure cold wallet transfers
/// - Automated profit harvesting 
/// - Risk controls and position limits
/// - Portfolio rebalancing logic
/// - Integration with existing wallet and portfolio systems

use anyhow::{Result, Context, bail};
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    transaction::Transaction,
    instruction::Instruction,
    signature::{Signature, Keypair, Signer},
    commitment_config::CommitmentConfig,
    system_instruction,
    native_token::LAMPORTS_PER_SOL,
};
use spl_token::{
    instruction::transfer,
    state::Account as TokenAccount,
};
use solana_sdk::program_pack::Pack;
use spl_associated_token_account::{
    instruction::create_associated_token_account,
    get_associated_token_address,
};
use std::collections::{HashMap, BTreeMap};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};
use chrono::{DateTime, Utc, Duration as ChronoDuration};
use tracing::{info, warn, error};
use tokio::time::{sleep, interval};
use dashmap::DashMap;

use crate::core::{
    wallet_management::{WalletManager, WalletType},
    portfolio_tracker::{PortfolioTracker, PositionUpdate, PositionUpdateType},
    constants::SOL_MINT,
};
use crate::strike::dex_client::{DexClient, DexConfig, SwapRequest};
use crate::core::db::UltraFastWalletDB;

/// Fund management configuration
#[derive(Debug, Clone)]
pub struct FundManagerConfig {
    /// Solana RPC endpoint
    pub rpc_endpoint: String,
    /// DEX configuration for swaps
    pub dex_config: DexConfig,
    /// Minimum SOL balance to maintain in trading wallet
    pub min_trading_balance_sol: f64,
    /// Maximum position size as percentage of portfolio
    pub max_position_size_percent: f64,
    /// Daily loss limit in SOL
    pub daily_loss_limit_sol: f64,
    /// Profit harvesting threshold (take profit at X% gain)
    pub profit_harvest_threshold_percent: f64,
    /// Stop loss threshold (stop at X% loss)
    pub stop_loss_threshold_percent: f64,
    /// Portfolio rebalancing interval in seconds
    pub rebalance_interval_secs: u64,
    /// Cold wallet transfer minimum amount in SOL
    pub cold_transfer_minimum_sol: f64,
    /// Maximum retries for failed transactions
    pub max_transaction_retries: u32,
    /// Transaction confirmation timeout in seconds
    pub confirmation_timeout_secs: u64,
    /// Risk monitoring interval in seconds
    pub risk_check_interval_secs: u64,
}

impl Default for FundManagerConfig {
    fn default() -> Self {
        Self {
            rpc_endpoint: "https://api.mainnet-beta.solana.com".to_string(),
            dex_config: DexConfig::default(),
            min_trading_balance_sol: 1.0,
            max_position_size_percent: 10.0, // Max 10% per position
            daily_loss_limit_sol: 5.0,
            profit_harvest_threshold_percent: 50.0, // Take profit at 50% gain
            stop_loss_threshold_percent: -20.0, // Stop loss at 20% loss
            rebalance_interval_secs: 3600, // Rebalance every hour
            cold_transfer_minimum_sol: 10.0,
            max_transaction_retries: 3,
            confirmation_timeout_secs: 60,
            risk_check_interval_secs: 300, // Risk check every 5 minutes
        }
    }
}

/// Cold wallet transfer request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColdTransferRequest {
    /// Asset to transfer (SOL_MINT for SOL, or token mint address)
    pub asset_mint: String,
    /// Amount to transfer (in base units)
    pub amount: u64,
    /// Optional memo for the transfer
    pub memo: Option<String>,
    /// Priority fee in lamports
    pub priority_fee: u64,
    /// Request timestamp
    pub requested_at: DateTime<Utc>,
}

/// Transfer validation result
#[derive(Debug, Clone)]
pub struct TransferValidation {
    pub is_valid: bool,
    pub reason: Option<String>,
    pub recommended_amount: Option<u64>,
}

/// Profit harvest configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarvestConfig {
    /// Minimum profit threshold in SOL
    pub min_profit_sol: f64,
    /// Percentage of profit to harvest (0-100)
    pub harvest_percentage: f64,
    /// Harvest interval in seconds
    pub interval_secs: u64,
    /// Only harvest if position is above this percentage gain
    pub min_gain_threshold_percent: f64,
    /// Keep at least this amount for continued trading
    pub reserve_amount_sol: f64,
}

impl Default for HarvestConfig {
    fn default() -> Self {
        Self {
            min_profit_sol: 1.0,
            harvest_percentage: 75.0, // Harvest 75% of profits
            interval_secs: 3600, // Check every hour
            min_gain_threshold_percent: 25.0, // At least 25% gain
            reserve_amount_sol: 5.0, // Keep 5 SOL for trading
        }
    }
}

/// Risk control configuration
#[derive(Debug, Clone)]
pub struct RiskConfig {
    /// Maximum portfolio value in SOL
    pub max_portfolio_value_sol: f64,
    /// Maximum number of open positions
    pub max_open_positions: usize,
    /// Maximum exposure to single asset (percentage)
    pub max_single_asset_exposure_percent: f64,
    /// Circuit breaker - stop all trading if daily loss exceeds this
    pub circuit_breaker_loss_sol: f64,
    /// Maximum drawdown percentage before reducing position sizes
    pub max_drawdown_percent: f64,
    /// Minimum liquidity required for position entry
    pub min_liquidity_sol: f64,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_portfolio_value_sol: 100.0,
            max_open_positions: 20,
            max_single_asset_exposure_percent: 15.0,
            circuit_breaker_loss_sol: 20.0,
            max_drawdown_percent: -25.0,
            min_liquidity_sol: 50.0,
        }
    }
}

/// Portfolio rebalancing target
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebalanceTarget {
    /// Asset mint address
    pub mint: String,
    /// Target allocation percentage (0-100)
    pub target_percent: f64,
    /// Tolerance before rebalancing (percentage points)
    pub tolerance: f64,
}

/// Rebalancing configuration
#[derive(Debug, Clone)]
pub struct RebalanceConfig {
    /// Target asset allocations
    pub targets: Vec<RebalanceTarget>,
    /// Minimum drift before rebalancing (percentage points)
    pub min_drift_threshold: f64,
    /// Maximum single rebalance trade size in SOL
    pub max_trade_size_sol: f64,
    /// Rebalancing strategy
    pub strategy: RebalanceStrategy,
}

/// Rebalancing strategy options
#[derive(Debug, Clone)]
pub enum RebalanceStrategy {
    /// Proportional: Adjust all positions proportionally
    Proportional,
    /// Threshold: Only rebalance assets exceeding drift threshold
    Threshold,
    /// Momentum: Consider recent performance in rebalancing
    Momentum,
}

/// Daily trading statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyStats {
    pub date: DateTime<Utc>,
    pub realized_pnl_sol: f64,
    pub unrealized_pnl_sol: f64,
    pub total_trades: u32,
    pub winning_trades: u32,
    pub largest_win_sol: f64,
    pub largest_loss_sol: f64,
    pub portfolio_value_start: f64,
    pub portfolio_value_end: f64,
}

/// Fund manager state
#[derive(Debug, Clone)]
pub struct FundManagerState {
    /// Whether the system is active
    pub is_active: bool,
    /// Circuit breaker status
    pub circuit_breaker_active: bool,
    /// Daily P&L tracking
    pub daily_pnl_sol: f64,
    /// Last daily reset timestamp
    pub last_daily_reset: DateTime<Utc>,
    /// Total funds transferred to cold storage
    pub total_cold_transfers_sol: f64,
    /// Number of successful harvests today
    pub harvests_today: u32,
    /// Last rebalance timestamp
    pub last_rebalance: DateTime<Utc>,
}

/// Main fund management system
pub struct FundManager {
    /// Solana RPC client
    rpc_client: RpcClient,
    /// DEX client for swaps and pricing
    dex_client: DexClient,
    /// Wallet manager for keypair access
    wallet_manager: Arc<WalletManager>,
    /// Portfolio tracker for position monitoring
    portfolio_tracker: Arc<PortfolioTracker>,
    /// Memory-mapped database
    mmap_db: Arc<UltraFastWalletDB>,
    
    /// Configuration
    config: FundManagerConfig,
    harvest_config: HarvestConfig,
    risk_config: RiskConfig,
    rebalance_config: RebalanceConfig,
    
    /// Runtime state
    state: Arc<RwLock<FundManagerState>>,
    /// Daily statistics
    daily_stats: Arc<RwLock<BTreeMap<String, DailyStats>>>,
    /// Pending transfers
    pending_transfers: Arc<DashMap<String, ColdTransferRequest>>,
    /// Risk metrics cache
    risk_metrics: Arc<RwLock<HashMap<String, f64>>>,
}

impl FundManager {
    /// Create new fund manager
    pub fn new(
        config: FundManagerConfig,
        harvest_config: HarvestConfig,
        risk_config: RiskConfig,
        rebalance_config: RebalanceConfig,
        wallet_manager: Arc<WalletManager>,
        portfolio_tracker: Arc<PortfolioTracker>,
        mmap_db: Arc<UltraFastWalletDB>,
    ) -> Result<Self> {
        info!("Initializing comprehensive fund management system");

        let rpc_client = RpcClient::new_with_commitment(
            config.rpc_endpoint.clone(),
            CommitmentConfig::confirmed(),
        );

        let dex_client = DexClient::new(config.dex_config.clone())
            .context("Failed to initialize DEX client")?;

        let initial_state = FundManagerState {
            is_active: true,
            circuit_breaker_active: false,
            daily_pnl_sol: 0.0,
            last_daily_reset: Utc::now(),
            total_cold_transfers_sol: 0.0,
            harvests_today: 0,
            last_rebalance: Utc::now(),
        };

        Ok(Self {
            rpc_client,
            dex_client,
            wallet_manager,
            portfolio_tracker,
            mmap_db,
            config,
            harvest_config,
            risk_config,
            rebalance_config,
            state: Arc::new(RwLock::new(initial_state)),
            daily_stats: Arc::new(RwLock::new(BTreeMap::new())),
            pending_transfers: Arc::new(DashMap::new()),
            risk_metrics: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Start the fund management background processes
    pub async fn start(&self) -> Result<()> {
        info!("Starting fund management background processes");

        // Start risk monitoring
        let risk_monitor = self.clone_for_background();
        tokio::spawn(async move {
            risk_monitor.risk_monitoring_loop().await;
        });

        // Start profit harvesting
        let harvest_manager = self.clone_for_background();
        tokio::spawn(async move {
            harvest_manager.profit_harvesting_loop().await;
        });

        // Start portfolio rebalancing
        let rebalance_manager = self.clone_for_background();
        tokio::spawn(async move {
            rebalance_manager.rebalancing_loop().await;
        });

        // Start daily statistics tracking
        let stats_manager = self.clone_for_background();
        tokio::spawn(async move {
            stats_manager.daily_stats_loop().await;
        });

        info!("All fund management processes started successfully");
        Ok(())
    }

    /// Transfer SOL from trading to cold wallet
    pub async fn transfer_sol_to_cold(&self, amount_sol: f64, memo: Option<String>) -> Result<Signature> {
        info!("Initiating SOL transfer to cold wallet: {} SOL", amount_sol);

        // Validate transfer
        let validation = self.validate_cold_transfer(&ColdTransferRequest {
            asset_mint: SOL_MINT.to_string(),
            amount: (amount_sol * LAMPORTS_PER_SOL as f64) as u64,
            memo: memo.clone(),
            priority_fee: self.config.dex_config.priority_fee_lamports,
            requested_at: Utc::now(),
        }).await?;

        if !validation.is_valid {
            bail!("Transfer validation failed: {:?}", validation.reason);
        }

        // Get keypairs
        let trading_keypair = self.wallet_manager.get_keypair(&WalletType::Trading)?;
        let cold_wallet_pubkey = self.wallet_manager.get_public_key(&WalletType::Cold)?;

        let amount_lamports = (amount_sol * LAMPORTS_PER_SOL as f64) as u64;

        // Create transfer instruction
        let transfer_instruction = system_instruction::transfer(
            &trading_keypair.pubkey(),
            &cold_wallet_pubkey,
            amount_lamports,
        );

        // Execute with retry logic
        let signature = self.execute_transaction_with_retry(
            vec![transfer_instruction],
            vec![trading_keypair],
        ).await?;

        // Update state
        {
            let mut state = self.state.write().unwrap();
            state.total_cold_transfers_sol += amount_sol;
        }

        info!("Successfully transferred {} SOL to cold wallet. Signature: {}", amount_sol, signature);
        Ok(signature)
    }

    /// Transfer SPL token from trading to cold wallet
    pub async fn transfer_token_to_cold(&self, mint: &str, amount: u64, _memo: Option<String>) -> Result<Signature> {
        info!("Initiating token transfer to cold wallet: {} units of {}", amount, mint);

        let mint_pubkey = mint.parse::<Pubkey>()
            .context("Invalid mint address")?;
        
        let trading_keypair = self.wallet_manager.get_keypair(&WalletType::Trading)?;
        let cold_wallet_pubkey = self.wallet_manager.get_public_key(&WalletType::Cold)?;

        // Get or create associated token accounts
        let trading_ata = get_associated_token_address(&trading_keypair.pubkey(), &mint_pubkey);
        let cold_ata = get_associated_token_address(&cold_wallet_pubkey, &mint_pubkey);

        let mut instructions = Vec::new();

        // Check if cold wallet ATA exists, create if not
        if self.rpc_client.get_account(&cold_ata).is_err() {
            let create_ata_instruction = create_associated_token_account(
                &trading_keypair.pubkey(),
                &cold_wallet_pubkey,
                &mint_pubkey,
                &spl_token::id(),
            );
            instructions.push(create_ata_instruction);
        }

        // Create transfer instruction
        let transfer_instruction = transfer(
            &spl_token::id(),
            &trading_ata,
            &cold_ata,
            &trading_keypair.pubkey(),
            &[],
            amount,
        )?;
        instructions.push(transfer_instruction);

        // Execute with retry logic
        let signature = self.execute_transaction_with_retry(
            instructions,
            vec![trading_keypair],
        ).await?;

        info!("Successfully transferred {} tokens ({}) to cold wallet. Signature: {}", amount, mint, signature);
        Ok(signature)
    }

    /// Validate cold wallet transfer
    async fn validate_cold_transfer(&self, request: &ColdTransferRequest) -> Result<TransferValidation> {
        let trading_pubkey = self.wallet_manager.get_public_key(&WalletType::Trading)?;

        if request.asset_mint == SOL_MINT {
            // SOL transfer validation
            let current_balance = self.rpc_client.get_balance(&trading_pubkey)?;
            let current_sol = current_balance as f64 / LAMPORTS_PER_SOL as f64;
            let requested_sol = request.amount as f64 / LAMPORTS_PER_SOL as f64;
            
            let remaining_balance = current_sol - requested_sol;
            
            if remaining_balance < self.config.min_trading_balance_sol {
                return Ok(TransferValidation {
                    is_valid: false,
                    reason: Some(format!("Transfer would leave only {} SOL, minimum is {}", 
                                       remaining_balance, self.config.min_trading_balance_sol)),
                    recommended_amount: Some(((current_sol - self.config.min_trading_balance_sol).max(0.0) * LAMPORTS_PER_SOL as f64) as u64),
                });
            }

            if requested_sol < self.config.cold_transfer_minimum_sol {
                return Ok(TransferValidation {
                    is_valid: false,
                    reason: Some(format!("Transfer amount {} SOL is below minimum {}", 
                                       requested_sol, self.config.cold_transfer_minimum_sol)),
                    recommended_amount: Some((self.config.cold_transfer_minimum_sol * LAMPORTS_PER_SOL as f64) as u64),
                });
            }
        } else {
            // SPL token transfer validation
            let mint_pubkey = request.asset_mint.parse::<Pubkey>()
                .context("Invalid mint address")?;
            let trading_ata = get_associated_token_address(&trading_pubkey, &mint_pubkey);
            
            match self.rpc_client.get_account(&trading_ata) {
                Ok(account) => {
                    let token_account = TokenAccount::unpack(&account.data)
                        .context("Failed to parse token account")?;
                    
                    if token_account.amount < request.amount {
                        return Ok(TransferValidation {
                            is_valid: false,
                            reason: Some(format!("Insufficient token balance: {} available, {} requested", 
                                               token_account.amount, request.amount)),
                            recommended_amount: Some(token_account.amount),
                        });
                    }
                },
                Err(_) => {
                    return Ok(TransferValidation {
                        is_valid: false,
                        reason: Some("Token account does not exist".to_string()),
                        recommended_amount: None,
                    });
                }
            }
        }

        Ok(TransferValidation {
            is_valid: true,
            reason: None,
            recommended_amount: None,
        })
    }

    /// Execute transaction with retry logic and confirmation
    async fn execute_transaction_with_retry(
        &self,
        instructions: Vec<Instruction>,
        signers: Vec<&Keypair>,
    ) -> Result<Signature> {
        let mut attempts = 0;
        
        while attempts < self.config.max_transaction_retries {
            attempts += 1;
            
            match self.try_execute_transaction(&instructions, &signers).await {
                Ok(signature) => {
                    info!("Transaction successful on attempt {}: {}", attempts, signature);
                    
                    // Wait for confirmation
                    if let Ok(_) = self.wait_for_confirmation(&signature).await {
                        return Ok(signature);
                    } else {
                        warn!("Transaction {} confirmed but not visible in getTransaction", signature);
                        return Ok(signature);
                    }
                },
                Err(e) => {
                    warn!("Transaction attempt {} failed: {}", attempts, e);
                    if attempts >= self.config.max_transaction_retries {
                        return Err(e);
                    }
                    // Wait before retry
                    sleep(Duration::from_millis(1000 * attempts as u64)).await;
                }
            }
        }

        bail!("Transaction failed after {} attempts", self.config.max_transaction_retries)
    }

    /// Try to execute a single transaction
    async fn try_execute_transaction(
        &self,
        instructions: &[Instruction],
        signers: &[&Keypair],
    ) -> Result<Signature> {
        let recent_blockhash = self.rpc_client.get_latest_blockhash()
            .context("Failed to get recent blockhash")?;

        let mut transaction = Transaction::new_with_payer(instructions, Some(&signers[0].pubkey()));
        transaction.sign(signers, recent_blockhash);

        let signature = self.rpc_client.send_and_confirm_transaction(&transaction)
            .context("Failed to send transaction")?;

        Ok(signature)
    }

    /// Wait for transaction confirmation
    async fn wait_for_confirmation(&self, signature: &Signature) -> Result<()> {
        let start_time = SystemTime::now();
        let timeout = Duration::from_secs(self.config.confirmation_timeout_secs);

        while start_time.elapsed().unwrap() < timeout {
            match self.rpc_client.get_signature_status(&signature) {
                Ok(Some(Ok(()))) => return Ok(()),
                Ok(Some(Err(e))) => bail!("Transaction failed: {:?}", e),
                Ok(None) => {
                    // Transaction not yet processed
                    sleep(Duration::from_secs(2)).await;
                    continue;
                },
                Err(e) => {
                    warn!("Error checking transaction status: {}", e);
                    sleep(Duration::from_secs(2)).await;
                    continue;
                }
            }
        }

        bail!("Transaction confirmation timeout")
    }

    /// Automated profit harvesting logic
    async fn profit_harvesting_loop(&self) {
        let mut harvest_interval = interval(Duration::from_secs(self.harvest_config.interval_secs));
        
        loop {
            harvest_interval.tick().await;
            
            if let Err(e) = self.check_and_harvest_profits().await {
                error!("Profit harvesting error: {}", e);
            }
        }
    }

    /// Check positions and harvest profits if thresholds are met
    async fn check_and_harvest_profits(&self) -> Result<()> {
        let (is_active, circuit_breaker_active) = {
            let state = self.state.read().unwrap();
            (state.is_active, state.circuit_breaker_active)
        };
        
        if !is_active || circuit_breaker_active {
            return Ok(());
        }

        let trading_wallet = self.wallet_manager.get_public_key(&WalletType::Trading)?.to_string();
        let portfolio = self.portfolio_tracker.get_portfolio(&trading_wallet)
            .context("Trading wallet portfolio not found")?;

        for (mint, position) in &portfolio.positions {
            let gain_percent = if position.cost_basis_sol > 0.0 {
                (position.unrealized_pnl_sol / position.cost_basis_sol) * 100.0
            } else {
                0.0
            };

            // Check if position meets harvest criteria
            if gain_percent >= self.harvest_config.min_gain_threshold_percent && 
               position.unrealized_pnl_sol >= self.harvest_config.min_profit_sol {
                
                info!("Position {} meets harvest criteria: {:.2}% gain, {:.6} SOL profit", 
                      mint, gain_percent, position.unrealized_pnl_sol);

                // Calculate harvest amount (percentage of position)
                let harvest_percentage = self.harvest_config.harvest_percentage / 100.0;
                let harvest_quantity = (position.quantity as f64 * harvest_percentage) as u64;

                if harvest_quantity > 0 {
                    match self.execute_profit_harvest(mint, harvest_quantity).await {
                        Ok(signature) => {
                            info!("Profit harvested for {}: {} tokens, signature: {}", 
                                  mint, harvest_quantity, signature);
                            
                            let mut state = self.state.write().unwrap();
                            state.harvests_today += 1;
                        },
                        Err(e) => {
                            error!("Failed to harvest profit for {}: {}", mint, e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Execute profit harvest by selling portion of position
    async fn execute_profit_harvest(&self, mint: &str, quantity: u64) -> Result<Signature> {
        // Create swap request to sell tokens for SOL
        let swap_request = SwapRequest {
            input_mint: mint.to_string(),
            output_mint: SOL_MINT.to_string(),
            amount: quantity,
            slippage_bps: self.config.dex_config.max_slippage_bps,
            user_public_key: self.wallet_manager.get_public_key(&WalletType::Trading)?.to_string(),
            auto_create_token_accounts: true,
        };

        // Execute swap via DEX
        let swap_result = self.dex_client.execute_swap(
            &swap_request,
            self.wallet_manager.get_keypair(&WalletType::Trading)?,
        ).await.context("Failed to execute profit harvest swap")?;
        
        let signature: Signature = swap_result.signature.parse()
            .context("Failed to parse signature")?;

        // Update portfolio position
        let trading_wallet = self.wallet_manager.get_public_key(&WalletType::Trading)?.to_string();
        let position_update = PositionUpdate {
            wallet_address: trading_wallet,
            mint: mint.to_string(),
            update_type: PositionUpdateType::Reduce,
            quantity,
            price_sol: 0.0, // Will be calculated from swap result
            timestamp: Utc::now(),
            transaction_signature: Some(signature.to_string()),
        };

        let wallet_address = position_update.wallet_address.clone();
        self.portfolio_tracker.update_position(&wallet_address, position_update).await?;

        Ok(signature)
    }

    /// Risk monitoring background loop
    async fn risk_monitoring_loop(&self) {
        let mut risk_interval = interval(Duration::from_secs(self.config.risk_check_interval_secs));
        
        loop {
            risk_interval.tick().await;
            
            if let Err(e) = self.check_risk_controls().await {
                error!("Risk monitoring error: {}", e);
            }
        }
    }

    /// Check and enforce risk controls
    async fn check_risk_controls(&self) -> Result<()> {
        let trading_wallet = self.wallet_manager.get_public_key(&WalletType::Trading)?.to_string();
        let portfolio = self.portfolio_tracker.get_portfolio(&trading_wallet)
            .context("Trading wallet portfolio not found")?;

        // Check circuit breaker conditions
        let daily_loss = {
            let state = self.state.read().unwrap();
            state.daily_pnl_sol
        };

        if daily_loss <= -self.risk_config.circuit_breaker_loss_sol {
            warn!("Circuit breaker activated: Daily loss {} SOL exceeds limit {}", 
                  daily_loss, self.risk_config.circuit_breaker_loss_sol);
            
            {
                let mut state = self.state.write().unwrap();
                state.circuit_breaker_active = true;
                state.is_active = false;
            }

            // Close all positions immediately
            self.emergency_position_closure().await?;
            return Ok(());
        }

        // Check maximum portfolio value
        if portfolio.total_value_sol > self.risk_config.max_portfolio_value_sol {
            warn!("Portfolio value {} SOL exceeds maximum {}", 
                  portfolio.total_value_sol, self.risk_config.max_portfolio_value_sol);
            
            // Transfer excess to cold storage
            let excess = portfolio.total_value_sol - self.risk_config.max_portfolio_value_sol;
            if let Err(e) = self.transfer_sol_to_cold(excess * 0.5, Some("Risk limit excess".to_string())).await {
                error!("Failed to transfer excess funds to cold storage: {}", e);
            }
        }

        // Check position limits
        if portfolio.positions.len() > self.risk_config.max_open_positions {
            warn!("Too many open positions: {} exceeds maximum {}", 
                  portfolio.positions.len(), self.risk_config.max_open_positions);
            
            // Close smallest positions first
            self.close_smallest_positions(portfolio.positions.len() - self.risk_config.max_open_positions).await?;
        }

        // Check individual position sizes
        for (mint, position) in &portfolio.positions {
            let position_percent = position.get_position_percentage(portfolio.total_value_sol);
            
            if position_percent > self.risk_config.max_single_asset_exposure_percent {
                warn!("Position {} exceeds maximum exposure: {:.2}% > {:.2}%", 
                      mint, position_percent, self.risk_config.max_single_asset_exposure_percent);
                
                // Reduce position size
                let reduction_percent = position_percent - self.risk_config.max_single_asset_exposure_percent;
                let reduction_quantity = (position.quantity as f64 * (reduction_percent / position_percent)) as u64;
                
                if let Err(e) = self.execute_profit_harvest(mint, reduction_quantity).await {
                    error!("Failed to reduce oversized position {}: {}", mint, e);
                }
            }
        }

        // Check stop losses
        for (mint, position) in &portfolio.positions {
            let loss_percent = if position.cost_basis_sol > 0.0 {
                (position.unrealized_pnl_sol / position.cost_basis_sol) * 100.0
            } else {
                0.0
            };

            if loss_percent <= self.config.stop_loss_threshold_percent {
                warn!("Stop loss triggered for {}: {:.2}% loss", mint, loss_percent);
                
                // Close entire position
                if let Err(e) = self.execute_profit_harvest(mint, position.quantity).await {
                    error!("Failed to execute stop loss for {}: {}", mint, e);
                }
            }
        }

        Ok(())
    }

    /// Emergency closure of all positions
    async fn emergency_position_closure(&self) -> Result<()> {
        warn!("Executing emergency position closure");
        
        let trading_wallet = self.wallet_manager.get_public_key(&WalletType::Trading)?.to_string();
        let portfolio = self.portfolio_tracker.get_portfolio(&trading_wallet)
            .context("Trading wallet portfolio not found")?;

        for (mint, position) in &portfolio.positions {
            if let Err(e) = self.execute_profit_harvest(mint, position.quantity).await {
                error!("Failed to close position {} during emergency: {}", mint, e);
            } else {
                info!("Emergency closed position: {}", mint);
            }
        }

        info!("Emergency position closure completed");
        Ok(())
    }

    /// Close smallest positions to reduce position count
    async fn close_smallest_positions(&self, count_to_close: usize) -> Result<()> {
        let trading_wallet = self.wallet_manager.get_public_key(&WalletType::Trading)?.to_string();
        let portfolio = self.portfolio_tracker.get_portfolio(&trading_wallet)
            .context("Trading wallet portfolio not found")?;

        // Sort positions by value (smallest first)
        let mut positions: Vec<_> = portfolio.positions.iter().collect();
        positions.sort_by(|a, b| a.1.current_value_sol.partial_cmp(&b.1.current_value_sol).unwrap());

        for (mint, position) in positions.iter().take(count_to_close) {
            if let Err(e) = self.execute_profit_harvest(mint, position.quantity).await {
                error!("Failed to close small position {}: {}", mint, e);
            } else {
                info!("Closed small position: {}", mint);
            }
        }

        Ok(())
    }

    /// Portfolio rebalancing background loop
    async fn rebalancing_loop(&self) {
        let mut rebalance_interval = interval(Duration::from_secs(self.config.rebalance_interval_secs));
        
        loop {
            rebalance_interval.tick().await;
            
            if let Err(e) = self.check_and_rebalance().await {
                error!("Rebalancing error: {}", e);
            }
        }
    }

    /// Check if rebalancing is needed and execute
    async fn check_and_rebalance(&self) -> Result<()> {
        let (is_active, circuit_breaker_active) = {
            let state = self.state.read().unwrap();
            (state.is_active, state.circuit_breaker_active)
        };
        
        if !is_active || circuit_breaker_active {
            return Ok(());
        }

        let trading_wallet = self.wallet_manager.get_public_key(&WalletType::Trading)?.to_string();
        let portfolio = self.portfolio_tracker.get_portfolio(&trading_wallet)
            .context("Trading wallet portfolio not found")?;

        let current_allocations = portfolio.get_asset_allocation();
        let mut rebalance_needed = false;
        let mut rebalance_trades = Vec::new();

        for target in &self.rebalance_config.targets {
            let current_percent = current_allocations.get(&target.mint).cloned().unwrap_or(0.0);
            let drift = (current_percent - target.target_percent).abs();

            if drift > target.tolerance && drift > self.rebalance_config.min_drift_threshold {
                rebalance_needed = true;
                
                let target_value = portfolio.total_value_sol * (target.target_percent / 100.0);
                let current_value = if let Some(position) = portfolio.positions.get(&target.mint) {
                    position.current_value_sol
                } else if target.mint == "SOL" {
                    portfolio.sol_balance
                } else {
                    0.0
                };

                let trade_value = target_value - current_value;
                
                if trade_value.abs() > 0.1 { // Minimum trade size
                    rebalance_trades.push((target.mint.clone(), trade_value));
                    info!("Rebalance needed for {}: current {:.2}%, target {:.2}%, trade value: {:.6} SOL", 
                          target.mint, current_percent, target.target_percent, trade_value);
                }
            }
        }

        if rebalance_needed && !rebalance_trades.is_empty() {
            info!("Executing portfolio rebalancing with {} trades", rebalance_trades.len());
            
            for (mint, trade_value) in rebalance_trades {
                if let Err(e) = self.execute_rebalance_trade(&mint, trade_value).await {
                    error!("Failed to execute rebalance trade for {}: {}", mint, e);
                }
            }

            let mut state = self.state.write().unwrap();
            state.last_rebalance = Utc::now();
        }

        Ok(())
    }

    /// Execute individual rebalance trade
    async fn execute_rebalance_trade(&self, mint: &str, trade_value_sol: f64) -> Result<()> {
        let trade_size = trade_value_sol.abs().min(self.rebalance_config.max_trade_size_sol);
        
        if trade_value_sol > 0.0 {
            // Need to buy this asset
            let swap_request = SwapRequest {
                input_mint: SOL_MINT.to_string(),
                output_mint: mint.to_string(),
                amount: (trade_size * LAMPORTS_PER_SOL as f64) as u64,
                slippage_bps: self.config.dex_config.max_slippage_bps,
                user_public_key: self.wallet_manager.get_public_key(&WalletType::Trading)?.to_string(),
                auto_create_token_accounts: true,
            };
            
            let swap_result = self.dex_client.execute_swap(
                &swap_request,
                self.wallet_manager.get_keypair(&WalletType::Trading)?,
            ).await?;
            
            let signature: Signature = swap_result.signature.parse()
                .context("Failed to parse signature")?;
            
            info!("Rebalance buy executed for {}: {:.6} SOL, signature: {}", mint, trade_size, signature);
        } else {
            // Need to sell this asset
            let trading_wallet = self.wallet_manager.get_public_key(&WalletType::Trading)?.to_string();
            let portfolio = self.portfolio_tracker.get_portfolio(&trading_wallet)
                .context("Portfolio not found")?;
            
            if let Some(position) = portfolio.positions.get(mint) {
                let sell_ratio = trade_size / position.current_value_sol;
                let sell_quantity = (position.quantity as f64 * sell_ratio) as u64;
                
                let signature = self.execute_profit_harvest(mint, sell_quantity).await?;
                info!("Rebalance sell executed for {}: {} tokens, signature: {}", mint, sell_quantity, signature);
            }
        }

        Ok(())
    }

    /// Daily statistics tracking loop
    async fn daily_stats_loop(&self) {
        let mut day_change_interval = interval(Duration::from_secs(86400)); // 24 hours
        
        loop {
            day_change_interval.tick().await;
            
            if let Err(e) = self.update_daily_stats().await {
                error!("Daily stats update error: {}", e);
            }
        }
    }

    /// Update daily statistics and reset counters
    async fn update_daily_stats(&self) -> Result<()> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let trading_wallet = self.wallet_manager.get_public_key(&WalletType::Trading)?.to_string();
        
        if let Some(portfolio) = self.portfolio_tracker.get_portfolio(&trading_wallet) {
            let daily_stats = DailyStats {
                date: Utc::now(),
                realized_pnl_sol: portfolio.total_realized_pnl_sol,
                unrealized_pnl_sol: portfolio.total_unrealized_pnl_sol,
                total_trades: 0, // Would need trade tracking
                winning_trades: 0,
                largest_win_sol: 0.0,
                largest_loss_sol: 0.0,
                portfolio_value_start: 0.0, // Would need previous day data
                portfolio_value_end: portfolio.total_value_sol,
            };

            let mut stats_map = self.daily_stats.write().unwrap();
            stats_map.insert(today, daily_stats);

            // Keep only last 30 days
            let cutoff = Utc::now() - ChronoDuration::days(30);
            stats_map.retain(|_, stats| stats.date >= cutoff);
        }

        // Reset daily counters
        let mut state = self.state.write().unwrap();
        state.daily_pnl_sol = 0.0;
        state.harvests_today = 0;
        state.last_daily_reset = Utc::now();

        info!("Daily statistics updated and counters reset");
        Ok(())
    }

    /// Get current fund manager state
    pub fn get_state(&self) -> FundManagerState {
        self.state.read().unwrap().clone()
    }

    /// Get daily statistics
    pub fn get_daily_stats(&self) -> BTreeMap<String, DailyStats> {
        self.daily_stats.read().unwrap().clone()
    }

    /// Manually trigger profit harvest
    pub async fn manual_harvest(&self, mint: &str, percentage: f64) -> Result<Signature> {
        let trading_wallet = self.wallet_manager.get_public_key(&WalletType::Trading)?.to_string();
        let portfolio = self.portfolio_tracker.get_portfolio(&trading_wallet)
            .context("Portfolio not found")?;

        if let Some(position) = portfolio.positions.get(mint) {
            let harvest_quantity = (position.quantity as f64 * (percentage / 100.0)) as u64;
            self.execute_profit_harvest(mint, harvest_quantity).await
        } else {
            bail!("Position not found for mint: {}", mint);
        }
    }

    /// Manually trigger rebalancing
    pub async fn manual_rebalance(&self) -> Result<()> {
        info!("Manual rebalancing triggered");
        self.check_and_rebalance().await
    }

    /// Enable/disable circuit breaker
    pub fn set_circuit_breaker(&self, active: bool) {
        let mut state = self.state.write().unwrap();
        state.circuit_breaker_active = active;
        state.is_active = !active;
        info!("Circuit breaker set to: {}", active);
    }

    /// Get risk metrics
    pub fn get_risk_metrics(&self) -> HashMap<String, f64> {
        self.risk_metrics.read().unwrap().clone()
    }

    /// Clone for background tasks
    fn clone_for_background(&self) -> Self {
        Self {
            rpc_client: RpcClient::new_with_commitment(
                self.config.rpc_endpoint.clone(),
                CommitmentConfig::confirmed(),
            ),
            dex_client: DexClient::new(self.config.dex_config.clone()).unwrap(),
            wallet_manager: Arc::clone(&self.wallet_manager),
            portfolio_tracker: Arc::clone(&self.portfolio_tracker),
            mmap_db: Arc::clone(&self.mmap_db),
            config: self.config.clone(),
            harvest_config: self.harvest_config.clone(),
            risk_config: self.risk_config.clone(),
            rebalance_config: self.rebalance_config.clone(),
            state: Arc::clone(&self.state),
            daily_stats: Arc::clone(&self.daily_stats),
            pending_transfers: Arc::clone(&self.pending_transfers),
            risk_metrics: Arc::clone(&self.risk_metrics),
        }
    }
}

impl std::fmt::Debug for FundManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FundManager")
            .field("config", &self.config)
            .field("state", &self.state)
            .finish()
    }
}