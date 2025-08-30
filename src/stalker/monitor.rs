use anyhow::{Result, Context};
use crate::core::types::{Wallet, Signal, Token, SignalType};
use crate::transport::alert_bus::AlertBus;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    commitment_config::CommitmentConfig,
    account::Account,
};
use solana_account_decoder::{UiAccount, UiAccountEncoding};
use dashmap::DashMap;
use tokio::time::{sleep, Duration, Instant};
use tracing::{info, debug, warn, error, instrument};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

/// Configuration for account monitoring
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// Solana RPC endpoint for account queries
    pub rpc_endpoint: String,
    /// How often to check account balances (in seconds)
    pub polling_interval_secs: u64,
    /// Minimum SOL balance change to trigger alerts
    pub min_sol_change_threshold: f64,
    /// Minimum token balance change percentage to trigger alerts
    pub min_token_change_percent: f64,
    /// Maximum number of recent transactions to track per wallet
    pub max_transaction_history: usize,
    /// RPC request timeout in seconds
    pub rpc_timeout_secs: u64,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            rpc_endpoint: "https://api.mainnet-beta.solana.com".to_string(),
            polling_interval_secs: 5, // Check every 5 seconds
            min_sol_change_threshold: 0.01, // 0.01 SOL minimum change
            min_token_change_percent: 5.0, // 5% minimum token change
            max_transaction_history: 100,
            rpc_timeout_secs: 10,
        }
    }
}

/// Account state snapshot for tracking changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSnapshot {
    /// Account public key
    pub pubkey: String,
    /// SOL balance in lamports
    pub lamports: u64,
    /// Token account balances (mint -> amount)
    pub token_balances: HashMap<String, TokenAccountInfo>,
    /// Last update timestamp
    pub last_updated: DateTime<Utc>,
    /// Transaction count at time of snapshot
    pub transaction_count: Option<u64>,
}

/// Token account information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAccountInfo {
    /// Token mint address
    pub mint: String,
    /// Token amount (raw, not UI amount)
    pub amount: u64,
    /// Number of decimals
    pub decimals: u8,
    /// UI amount (human readable)
    pub ui_amount: f64,
    /// Account owner
    pub owner: String,
}

/// Activity alert generated from account monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityAlert {
    /// Wallet that triggered the alert
    pub wallet: Wallet,
    /// Type of activity detected
    pub activity_type: ActivityType,
    /// Previous account state
    pub previous_state: AccountSnapshot,
    /// Current account state
    pub current_state: AccountSnapshot,
    /// Alert timestamp
    pub timestamp: DateTime<Utc>,
    /// Calculated significance score (0-100)
    pub significance_score: f32,
}

/// Types of account activity that can be detected
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActivityType {
    /// Large SOL balance change
    SolBalanceChange {
        /// Amount changed in SOL
        change_sol: f64,
        /// Direction of change
        direction: BalanceDirection,
    },
    /// Significant token balance change
    TokenBalanceChange {
        /// Token mint that changed
        token_mint: String,
        /// Token symbol if known
        token_symbol: Option<String>,
        /// Percentage change
        change_percent: f64,
        /// Direction of change
        direction: BalanceDirection,
        /// Amount changed (raw)
        change_amount: u64,
    },
    /// New token account created
    NewTokenAccount {
        /// Token mint
        token_mint: String,
        /// Token symbol if known
        token_symbol: Option<String>,
    },
    /// Token account closed
    TokenAccountClosed {
        /// Token mint
        token_mint: String,
        /// Token symbol if known
        token_symbol: Option<String>,
    },
}

/// Direction of balance change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BalanceDirection {
    /// Balance increased
    Increase,
    /// Balance decreased
    Decrease,
}

/// Statistics for monitored wallets
#[derive(Debug, Clone)]
pub struct MonitoringStats {
    /// Total wallets being monitored
    pub total_wallets: usize,
    /// Number of active wallets (changed in last 24h)
    pub active_wallets_24h: usize,
    /// Total alerts generated
    pub total_alerts: usize,
    /// High significance alerts (score > 80)
    pub high_significance_alerts: usize,
    /// Average polling time in milliseconds
    pub average_poll_time_ms: f64,
    /// Last successful poll timestamp
    pub last_poll_time: Option<DateTime<Utc>>,
    /// Number of RPC errors in last hour
    pub rpc_errors_last_hour: usize,
}

/// Advanced wallet monitor with real Solana account tracking
#[derive(Debug)]
pub struct WalletMonitor {
    /// Wallets being tracked (address -> wallet info)
    tracked_wallets: Arc<DashMap<String, Wallet>>,
    /// Current account snapshots (address -> snapshot)
    account_snapshots: Arc<DashMap<String, AccountSnapshot>>,
    /// Alert bus for publishing activity alerts
    alert_bus: AlertBus,
    /// Solana RPC client
    rpc_client: RpcClient,
    /// Monitor configuration
    config: MonitorConfig,
    /// Monitoring statistics
    stats: Arc<tokio::sync::RwLock<MonitoringStats>>,
}

impl WalletMonitor {
    /// Creates a new wallet monitor with real Solana integration
    /// 
    /// # Arguments
    /// * `config` - Optional monitor configuration (uses defaults if None)
    /// 
    /// # Returns
    /// * `Result<Self>` - Monitor instance ready for account tracking
    #[instrument]
    pub async fn new(config: Option<MonitorConfig>) -> Result<Self> {
        let config = config.unwrap_or_default();
        
        info!(
            rpc_endpoint = %config.rpc_endpoint,
            polling_interval = config.polling_interval_secs,
            sol_threshold = config.min_sol_change_threshold,
            "Initializing WalletMonitor with real Solana account tracking"
        );
        
        // Initialize Solana RPC client
        let rpc_client = RpcClient::new_with_timeout_and_commitment(
            config.rpc_endpoint.clone(),
            Duration::from_secs(config.rpc_timeout_secs),
            CommitmentConfig::confirmed(),
        );
        
        // Test RPC connection
        match rpc_client.get_slot().await {
            Ok(slot) => {
                info!(slot = slot, "Successfully connected to Solana RPC");
            }
            Err(e) => {
                error!(error = %e, "Failed to connect to Solana RPC");
                return Err(e.into());
            }
        }
        
        let stats = MonitoringStats {
            total_wallets: 0,
            active_wallets_24h: 0,
            total_alerts: 0,
            high_significance_alerts: 0,
            average_poll_time_ms: 0.0,
            last_poll_time: None,
            rpc_errors_last_hour: 0,
        };
        
        Ok(Self {
            tracked_wallets: Arc::new(DashMap::new()),
            account_snapshots: Arc::new(DashMap::new()),
            alert_bus: AlertBus::new(),
            rpc_client,
            config,
            stats: Arc::new(tokio::sync::RwLock::new(stats)),
        })
    }
    
    /// Loads tracked wallets from configuration file
    /// 
    /// # Arguments
    /// * `config_path` - Path to wallets configuration file
    /// 
    /// # Returns
    /// * `Result<usize>` - Number of wallets loaded
    #[instrument(skip(self))]
    pub async fn load_wallets_from_config(&self, config_path: &str) -> Result<usize> {
        info!(config_path = %config_path, "Loading tracked wallets from configuration");
        
        let config_content = tokio::fs::read_to_string(config_path).await
            .with_context(|| format!("Failed to read wallet config file: {}", config_path))?;
        
        #[derive(Deserialize)]
        struct WalletConfig {
            tracked_wallets: Vec<Wallet>,
        }
        
        let wallet_config: WalletConfig = serde_json::from_str(&config_content)
            .context("Failed to parse wallet configuration JSON")?;
        
        let mut loaded_count = 0;
        for wallet in wallet_config.tracked_wallets {
            // Validate wallet address
            if let Err(e) = Pubkey::from_str(&wallet.address) {
                warn!(
                    address = %wallet.address,
                    error = %e,
                    "Invalid wallet address, skipping"
                );
                continue;
            }
            
            self.add_wallet(wallet);
            loaded_count += 1;
        }
        
        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_wallets = loaded_count;
        }
        
        info!(loaded_count = loaded_count, "Successfully loaded wallets from configuration");
        
        Ok(loaded_count)
    }
    
    /// Starts the wallet monitoring loop
    /// 
    /// This method runs indefinitely, polling Solana accounts for changes
    /// and generating alerts when significant activity is detected.
    /// 
    /// # Returns
    /// * `Result<()>` - Ok if monitoring runs successfully until shutdown
    #[instrument(skip(self))]
    pub async fn run(&self) -> Result<()> {
        info!("WalletMonitor: Starting real-time wallet monitoring with Solana integration");
        
        let polling_interval = Duration::from_secs(self.config.polling_interval_secs);
        let mut poll_counter = 0u64;
        
        // Log initial monitoring setup
        let wallet_count = self.tracked_wallets.len();
        info!(
            wallet_count = wallet_count,
            polling_interval_secs = self.config.polling_interval_secs,
            sol_threshold = self.config.min_sol_change_threshold,
            token_threshold_percent = self.config.min_token_change_percent,
            "Wallet monitoring setup complete"
        );
        
        loop {
            poll_counter += 1;
            let poll_start = Instant::now();
            
            debug!(poll_number = poll_counter, "Starting wallet polling cycle");
            
            // Poll all tracked wallets
            match self.poll_all_wallets().await {
                Ok(alerts_generated) => {
                    let poll_duration = poll_start.elapsed();
                    
                    debug!(
                        poll_number = poll_counter,
                        duration_ms = poll_duration.as_millis(),
                        alerts_generated = alerts_generated,
                        "Polling cycle completed"
                    );
                    
                    // Update statistics
                    self.update_polling_stats(poll_duration.as_millis() as f64).await;
                    
                    // Log periodic status updates
                    if poll_counter % 12 == 0 { // Every minute (assuming 5 second polls)
                        self.log_monitoring_status().await;
                    }
                }
                Err(e) => {
                    error!(
                        error = %e,
                        poll_number = poll_counter,
                        "Wallet polling cycle failed"
                    );
                    
                    // Increment error counter
                    {
                        let mut stats = self.stats.write().await;
                        stats.rpc_errors_last_hour += 1;
                    }
                }
            }
            
            // Wait for next polling cycle
            sleep(polling_interval).await;
        }
    }
    
    /// Polls all tracked wallets for account changes
    /// 
    /// # Returns
    /// * `Result<usize>` - Number of alerts generated
    #[instrument(skip(self))]
    async fn poll_all_wallets(&self) -> Result<usize> {
        if self.tracked_wallets.is_empty() {
            debug!("No wallets to monitor");
            return Ok(0);
        }
        
        let mut alerts_generated = 0;
        
        // Process wallets in batches to avoid overwhelming RPC
        let batch_size = 10; // Poll 10 wallets at a time
        let wallet_addresses: Vec<String> = self.tracked_wallets.iter()
            .map(|entry| entry.key().clone())
            .collect();
        
        for chunk in wallet_addresses.chunks(batch_size) {
            let mut batch_tasks = Vec::new();
            
            for address in chunk {
                let address = address.clone();
                let rpc_client = self.rpc_client.clone();
                let snapshots = self.account_snapshots.clone();
                let config = self.config.clone();
                
                let task = tokio::spawn(async move {
                    Self::poll_single_wallet(address, rpc_client, snapshots, config).await
                });
                
                batch_tasks.push(task);
            }
            
            // Wait for batch to complete and process results
            for task in batch_tasks {
                match task.await {
                    Ok(Ok(snapshot_opt)) => {
                        if let Some(snapshot) = snapshot_opt {
                            // Check for significant changes and generate alerts
                            if let Some(alert) = self.analyze_account_changes(&snapshot).await {
                                alerts_generated += 1;
                                
                                info!(
                                    wallet_address = %alert.wallet.address,
                                    wallet_label = %alert.wallet.label,
                                    activity_type = ?alert.activity_type,
                                    significance = alert.significance_score,
                                    "🚨 Wallet activity detected"
                                );
                                
                                // Publish alert to alert bus
                                if let Err(e) = self.alert_bus.publish_alert(&alert).await {
                                    error!(error = %e, "Failed to publish activity alert");
                                }
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        debug!(error = %e, "Failed to poll single wallet");
                    }
                    Err(e) => {
                        warn!(error = %e, "Wallet polling task panicked");
                    }
                }
            }
            
            // Small delay between batches to avoid rate limiting
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        Ok(alerts_generated)
    }
    
    /// Polls a single wallet account for changes
    /// 
    /// # Arguments
    /// * `address` - Wallet address to poll
    /// * `rpc_client` - Solana RPC client
    /// * `snapshots` - Shared snapshot storage
    /// * `config` - Monitor configuration
    /// 
    /// # Returns
    /// * `Result<Option<AccountSnapshot>>` - New snapshot if account changed
    #[instrument(skip(rpc_client, snapshots))]
    async fn poll_single_wallet(
        address: String,
        rpc_client: RpcClient,
        snapshots: Arc<DashMap<String, AccountSnapshot>>,
        config: MonitorConfig,
    ) -> Result<Option<AccountSnapshot>> {
        let pubkey = Pubkey::from_str(&address)
            .context("Invalid wallet address")?;
        
        // Get account info
        let account_info = match rpc_client.get_account(&pubkey).await {
            Ok(info) => info,
            Err(e) => {
                debug!(
                    address = %address,
                    error = %e,
                    "Failed to get account info (account may not exist)"
                );
                return Ok(None);
            }
        };
        
        // Get token accounts for this wallet
        let token_accounts = match rpc_client.get_token_accounts_by_owner(
            &pubkey,
            solana_client::rpc_request::TokenAccountsFilter::ProgramId(
                spl_token::id()
            ),
        ).await {
            Ok(accounts) => accounts,
            Err(e) => {
                debug!(
                    address = %address,
                    error = %e,
                    "Failed to get token accounts"
                );
                Vec::new()
            }
        };
        
        // Build token balances map
        let mut token_balances = HashMap::new();
        for token_account in token_accounts {
            if let Ok(parsed_account) = serde_json::from_value::<spl_token::state::Account>(
                token_account.account.data
            ) {
                let token_info = TokenAccountInfo {
                    mint: parsed_account.mint.to_string(),
                    amount: parsed_account.amount,
                    decimals: 9, // Default to 9, would need to query mint for actual
                    ui_amount: parsed_account.amount as f64 / 10f64.powi(9),
                    owner: parsed_account.owner.to_string(),
                };
                
                token_balances.insert(parsed_account.mint.to_string(), token_info);
            }
        }
        
        // Create new snapshot
        let new_snapshot = AccountSnapshot {
            pubkey: address.clone(),
            lamports: account_info.lamports,
            token_balances,
            last_updated: Utc::now(),
            transaction_count: None, // Would need additional RPC call
        };
        
        // Check if this is a significant change
        let is_significant = if let Some(previous) = snapshots.get(&address) {
            Self::is_significant_change(&previous, &new_snapshot, &config)
        } else {
            // First time seeing this account
            true
        };
        
        // Update snapshot
        snapshots.insert(address, new_snapshot.clone());
        
        if is_significant {
            Ok(Some(new_snapshot))
        } else {
            Ok(None)
        }
    }
    
    /// Determines if account changes are significant enough to alert on
    /// 
    /// # Arguments
    /// * `previous` - Previous account snapshot
    /// * `current` - Current account snapshot
    /// * `config` - Monitor configuration with thresholds
    /// 
    /// # Returns
    /// * `bool` - True if changes are significant
    fn is_significant_change(
        previous: &AccountSnapshot,
        current: &AccountSnapshot,
        config: &MonitorConfig,
    ) -> bool {
        // Check SOL balance change
        let sol_change = (current.lamports as f64 - previous.lamports as f64) / 1_000_000_000.0;
        if sol_change.abs() >= config.min_sol_change_threshold {
            return true;
        }
        
        // Check token balance changes
        for (mint, current_token) in &current.token_balances {
            if let Some(previous_token) = previous.token_balances.get(mint) {
                // Token existed before - check for significant change
                let change_percent = if previous_token.amount > 0 {
                    ((current_token.amount as f64 - previous_token.amount as f64) 
                     / previous_token.amount as f64).abs() * 100.0
                } else if current_token.amount > 0 {
                    100.0 // New tokens appeared
                } else {
                    0.0
                };
                
                if change_percent >= config.min_token_change_percent {
                    return true;
                }
            } else {
                // New token account
                return true;
            }
        }
        
        // Check for closed token accounts
        for mint in previous.token_balances.keys() {
            if !current.token_balances.contains_key(mint) {
                return true;
            }
        }
        
        false
    }
    
    /// Analyzes account changes and generates activity alerts
    /// 
    /// # Arguments
    /// * `current_snapshot` - Current account state
    /// 
    /// # Returns
    /// * `Option<ActivityAlert>` - Alert if significant activity detected
    #[instrument(skip(self))]
    async fn analyze_account_changes(&self, current_snapshot: &AccountSnapshot) -> Option<ActivityAlert> {
        let wallet = self.tracked_wallets.get(&current_snapshot.pubkey)?;
        
        let previous_snapshot = self.account_snapshots.get(&current_snapshot.pubkey)?;
        
        // Analyze SOL balance changes
        let sol_change = (current_snapshot.lamports as f64 - previous_snapshot.lamports as f64) / 1_000_000_000.0;
        
        if sol_change.abs() >= self.config.min_sol_change_threshold {
            let activity_type = ActivityType::SolBalanceChange {
                change_sol: sol_change,
                direction: if sol_change > 0.0 { 
                    BalanceDirection::Increase 
                } else { 
                    BalanceDirection::Decrease 
                },
            };
            
            let significance_score = Self::calculate_significance_score(&activity_type, &wallet);
            
            return Some(ActivityAlert {
                wallet: wallet.clone(),
                activity_type,
                previous_state: previous_snapshot.clone(),
                current_state: current_snapshot.clone(),
                timestamp: Utc::now(),
                significance_score,
            });
        }
        
        // Analyze token balance changes
        for (mint, current_token) in &current_snapshot.token_balances {
            if let Some(previous_token) = previous_snapshot.token_balances.get(mint) {
                let change_percent = if previous_token.amount > 0 {
                    ((current_token.amount as f64 - previous_token.amount as f64) 
                     / previous_token.amount as f64) * 100.0
                } else if current_token.amount > 0 {
                    100.0
                } else {
                    0.0
                };
                
                if change_percent.abs() >= self.config.min_token_change_percent {
                    let activity_type = ActivityType::TokenBalanceChange {
                        token_mint: mint.clone(),
                        token_symbol: None, // Would need token metadata lookup
                        change_percent,
                        direction: if change_percent > 0.0 { 
                            BalanceDirection::Increase 
                        } else { 
                            BalanceDirection::Decrease 
                        },
                        change_amount: if current_token.amount > previous_token.amount {
                            current_token.amount - previous_token.amount
                        } else {
                            previous_token.amount - current_token.amount
                        },
                    };
                    
                    let significance_score = Self::calculate_significance_score(&activity_type, &wallet);
                    
                    return Some(ActivityAlert {
                        wallet: wallet.clone(),
                        activity_type,
                        previous_state: previous_snapshot.clone(),
                        current_state: current_snapshot.clone(),
                        timestamp: Utc::now(),
                        significance_score,
                    });
                }
            } else {
                // New token account
                let activity_type = ActivityType::NewTokenAccount {
                    token_mint: mint.clone(),
                    token_symbol: None,
                };
                
                let significance_score = Self::calculate_significance_score(&activity_type, &wallet);
                
                return Some(ActivityAlert {
                    wallet: wallet.clone(),
                    activity_type,
                    previous_state: previous_snapshot.clone(),
                    current_state: current_snapshot.clone(),
                    timestamp: Utc::now(),
                    significance_score,
                });
            }
        }
        
        None
    }
    
    /// Calculates significance score for an activity (0-100)
    /// 
    /// # Arguments
    /// * `activity_type` - Type of activity detected
    /// * `wallet` - Wallet information
    /// 
    /// # Returns
    /// * `f32` - Significance score (0-100)
    fn calculate_significance_score(activity_type: &ActivityType, wallet: &Wallet) -> f32 {
        let mut score = 50.0; // Base score
        
        // Adjust based on wallet tier
        match wallet.tier.as_str() {
            "high" => score += 30.0,
            "medium" => score += 15.0,
            "low" => score += 5.0,
            _ => {}
        }
        
        // Adjust based on activity type
        match activity_type {
            ActivityType::SolBalanceChange { change_sol, .. } => {
                // Larger SOL changes are more significant
                let sol_impact = (change_sol.abs() * 10.0).min(20.0);
                score += sol_impact as f32;
            }
            ActivityType::TokenBalanceChange { change_percent, .. } => {
                // Larger percentage changes are more significant
                let percent_impact = (change_percent.abs() / 10.0).min(25.0);
                score += percent_impact as f32;
            }
            ActivityType::NewTokenAccount { .. } => {
                score += 15.0; // New tokens are moderately significant
            }
            ActivityType::TokenAccountClosed { .. } => {
                score += 20.0; // Closing accounts is more significant
            }
        }
        
        // Cap at 100
        score.min(100.0)
    }
    
    /// Updates polling statistics
    /// 
    /// # Arguments
    /// * `poll_duration_ms` - Duration of last poll in milliseconds
    async fn update_polling_stats(&self, poll_duration_ms: f64) {
        let mut stats = self.stats.write().await;
        
        // Update average poll time (simple moving average)
        if stats.average_poll_time_ms == 0.0 {
            stats.average_poll_time_ms = poll_duration_ms;
        } else {
            stats.average_poll_time_ms = (stats.average_poll_time_ms * 0.9) + (poll_duration_ms * 0.1);
        }
        
        stats.last_poll_time = Some(Utc::now());
    }
    
    /// Logs periodic monitoring status updates
    #[instrument(skip(self))]
    async fn log_monitoring_status(&self) {
        let stats = self.stats.read().await;
        
        info!(
            total_wallets = stats.total_wallets,
            active_wallets_24h = stats.active_wallets_24h,
            total_alerts = stats.total_alerts,
            high_significance_alerts = stats.high_significance_alerts,
            avg_poll_time_ms = stats.average_poll_time_ms,
            rpc_errors = stats.rpc_errors_last_hour,
            "📊 Wallet monitoring status"
        );
    }
    
    /// Adds a wallet to the monitoring list
    /// 
    /// # Arguments
    /// * `wallet` - Wallet to add to monitoring
    #[instrument(skip(self))]
    pub fn add_wallet(&self, wallet: Wallet) {
        info!(
            wallet_address = %wallet.address,
            wallet_label = %wallet.label,
            wallet_tier = %wallet.tier,
            min_sol_amount = wallet.min_sol_amount,
            "Adding wallet to real-time monitoring"
        );
        
        self.tracked_wallets.insert(wallet.address.clone(), wallet);
    }
    
    /// Gets current monitoring statistics
    /// 
    /// # Returns
    /// * `MonitoringStats` - Current monitoring statistics
    pub async fn get_monitoring_stats(&self) -> MonitoringStats {
        let stats = self.stats.read().await;
        stats.clone()
    }
    
    /// Gets snapshot of a specific wallet
    /// 
    /// # Arguments
    /// * `wallet_address` - Wallet address to get snapshot for
    /// 
    /// # Returns
    /// * `Option<AccountSnapshot>` - Current account snapshot if available
    pub fn get_wallet_snapshot(&self, wallet_address: &str) -> Option<AccountSnapshot> {
        self.account_snapshots.get(wallet_address).map(|entry| entry.clone())
    }
}