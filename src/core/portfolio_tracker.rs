/// Comprehensive Portfolio Tracking System
/// 
/// This module provides real-time portfolio tracking for Solana trading wallets,
/// including position tracking, P&L calculations, and performance analytics.

use anyhow::{Result, Context};
use solana_sdk::program_pack::Pack;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    commitment_config::CommitmentConfig,
};
use solana_account_decoder::UiAccountData;
use spl_token::state::Mint;
use std::collections::{HashMap, BTreeMap};
use std::sync::{Arc, RwLock};
use std::str::FromStr;
use chrono::{DateTime, Utc, Duration as ChronoDuration};
use tracing::{info, debug, warn, error, instrument};
use tokio::time::Duration;
use dashmap::DashMap;

use crate::core::wallet_management::{WalletManager, WalletType};
use crate::strike::dex_client::{DexClient, DexConfig};
use crate::core::db::UltraFastWalletDB;

/// Individual token position in portfolio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    /// Token mint address
    pub mint: String,
    /// Token symbol (if known)
    pub symbol: Option<String>,
    /// Token decimals
    pub decimals: u8,
    /// Current token quantity (raw, without decimals applied)
    pub quantity: u64,
    /// Average entry price in SOL
    pub entry_price_sol: f64,
    /// Current market price in SOL
    pub current_price_sol: f64,
    /// Total SOL invested in this position
    pub cost_basis_sol: f64,
    /// Current value in SOL
    pub current_value_sol: f64,
    /// Unrealized P&L in SOL
    pub unrealized_pnl_sol: f64,
    /// Realized P&L from all trades in SOL
    pub realized_pnl_sol: f64,
    /// Position opening timestamp
    pub opened_at: DateTime<Utc>,
    /// Last price update timestamp
    pub last_updated: DateTime<Utc>,
    /// Token account address
    pub token_account: String,
    /// Position entry history
    pub entries: Vec<PositionEntry>,
}

impl Position {
    /// Create new position
    pub fn new(
        mint: String,
        symbol: Option<String>,
        decimals: u8,
        quantity: u64,
        entry_price_sol: f64,
        token_account: String,
    ) -> Self {
        let cost_basis_sol = (quantity as f64 / 10_u64.pow(decimals as u32) as f64) * entry_price_sol;
        let current_value_sol = cost_basis_sol; // Initially same as cost basis
        
        let entry = PositionEntry {
            timestamp: Utc::now(),
            quantity_delta: quantity as i64,
            price_sol: entry_price_sol,
            transaction_signature: None,
        };

        Self {
            mint,
            symbol,
            decimals,
            quantity,
            entry_price_sol,
            current_price_sol: entry_price_sol,
            cost_basis_sol,
            current_value_sol,
            unrealized_pnl_sol: 0.0,
            realized_pnl_sol: 0.0,
            opened_at: Utc::now(),
            last_updated: Utc::now(),
            token_account,
            entries: vec![entry],
        }
    }

    /// Update position with new price
    pub fn update_price(&mut self, new_price_sol: f64) {
        self.current_price_sol = new_price_sol;
        self.current_value_sol = (self.quantity as f64 / 10_u64.pow(self.decimals as u32) as f64) * new_price_sol;
        self.unrealized_pnl_sol = self.current_value_sol - self.cost_basis_sol;
        self.last_updated = Utc::now();
    }

    /// Add to position (averaging down/up)
    pub fn add_quantity(&mut self, additional_quantity: u64, price_sol: f64, signature: Option<String>) {
        let additional_cost = (additional_quantity as f64 / 10_u64.pow(self.decimals as u32) as f64) * price_sol;
        let total_cost = self.cost_basis_sol + additional_cost;
        let total_quantity = self.quantity + additional_quantity;
        
        // Update average entry price
        self.entry_price_sol = total_cost / (total_quantity as f64 / 10_u64.pow(self.decimals as u32) as f64);
        self.quantity = total_quantity;
        self.cost_basis_sol = total_cost;
        
        // Add entry record
        let entry = PositionEntry {
            timestamp: Utc::now(),
            quantity_delta: additional_quantity as i64,
            price_sol,
            transaction_signature: signature,
        };
        self.entries.push(entry);
        
        // Recalculate current values
        self.update_price(self.current_price_sol);
    }

    /// Reduce position (partial/full sell)
    pub fn reduce_quantity(&mut self, reduction_quantity: u64, price_sol: f64, signature: Option<String>) -> f64 {
        if reduction_quantity > self.quantity {
            warn!("Cannot reduce position by more than current quantity");
            return 0.0;
        }

        let reduction_fraction = reduction_quantity as f64 / self.quantity as f64;
        let realized_pnl = reduction_fraction * self.unrealized_pnl_sol;
        
        self.quantity -= reduction_quantity;
        self.cost_basis_sol *= 1.0 - reduction_fraction;
        self.realized_pnl_sol += realized_pnl;
        
        // Add entry record
        let entry = PositionEntry {
            timestamp: Utc::now(),
            quantity_delta: -(reduction_quantity as i64),
            price_sol,
            transaction_signature: signature,
        };
        self.entries.push(entry);
        
        // Recalculate current values
        self.update_price(self.current_price_sol);
        
        realized_pnl
    }

    /// Get position size as percentage of total portfolio value
    pub fn get_position_percentage(&self, total_portfolio_value: f64) -> f64 {
        if total_portfolio_value <= 0.0 {
            0.0
        } else {
            (self.current_value_sol / total_portfolio_value) * 100.0
        }
    }

    /// Check if position is closed
    pub fn is_closed(&self) -> bool {
        self.quantity == 0
    }
}

/// Individual position entry/exit record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionEntry {
    pub timestamp: DateTime<Utc>,
    pub quantity_delta: i64, // Positive for buys, negative for sells
    pub price_sol: f64,
    pub transaction_signature: Option<String>,
}

/// Complete portfolio state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portfolio {
    /// Wallet address being tracked
    pub wallet_address: String,
    /// SOL balance
    pub sol_balance: f64,
    /// All token positions (mint -> position)
    pub positions: HashMap<String, Position>,
    /// Total portfolio value in SOL
    pub total_value_sol: f64,
    /// Total unrealized P&L across all positions
    pub total_unrealized_pnl_sol: f64,
    /// Total realized P&L across all positions
    pub total_realized_pnl_sol: f64,
    /// Portfolio creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub last_updated: DateTime<Utc>,
    /// Historical snapshots for performance tracking
    pub snapshots: BTreeMap<DateTime<Utc>, PortfolioSnapshot>,
}

impl Portfolio {
    /// Create new empty portfolio
    pub fn new(wallet_address: String) -> Self {
        Self {
            wallet_address,
            sol_balance: 0.0,
            positions: HashMap::new(),
            total_value_sol: 0.0,
            total_unrealized_pnl_sol: 0.0,
            total_realized_pnl_sol: 0.0,
            created_at: Utc::now(),
            last_updated: Utc::now(),
            snapshots: BTreeMap::new(),
        }
    }

    /// Update portfolio totals
    pub fn recalculate_totals(&mut self) {
        self.total_value_sol = self.sol_balance;
        self.total_unrealized_pnl_sol = 0.0;
        self.total_realized_pnl_sol = 0.0;

        for position in self.positions.values() {
            self.total_value_sol += position.current_value_sol;
            self.total_unrealized_pnl_sol += position.unrealized_pnl_sol;
            self.total_realized_pnl_sol += position.realized_pnl_sol;
        }

        self.last_updated = Utc::now();
    }

    /// Take portfolio snapshot for historical tracking
    pub fn take_snapshot(&mut self) {
        let snapshot = PortfolioSnapshot {
            timestamp: Utc::now(),
            total_value_sol: self.total_value_sol,
            sol_balance: self.sol_balance,
            position_count: self.positions.len(),
            unrealized_pnl_sol: self.total_unrealized_pnl_sol,
            realized_pnl_sol: self.total_realized_pnl_sol,
        };

        self.snapshots.insert(snapshot.timestamp, snapshot);

        // Keep only last 30 days of hourly snapshots
        let cutoff = Utc::now() - ChronoDuration::days(30);
        self.snapshots.retain(|&timestamp, _| timestamp >= cutoff);
    }

    /// Get asset allocation breakdown
    pub fn get_asset_allocation(&self) -> HashMap<String, f64> {
        let mut allocation = HashMap::new();
        
        if self.total_value_sol <= 0.0 {
            return allocation;
        }

        // SOL allocation
        let sol_percentage = (self.sol_balance / self.total_value_sol) * 100.0;
        allocation.insert("SOL".to_string(), sol_percentage);

        // Token allocations
        for (mint, position) in &self.positions {
            let percentage = position.get_position_percentage(self.total_value_sol);
            let symbol = position.symbol.clone().unwrap_or_else(|| {
                format!("{}..{}", &mint[..4], &mint[mint.len()-4..])
            });
            allocation.insert(symbol, percentage);
        }

        allocation
    }
}

/// Portfolio performance snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioSnapshot {
    pub timestamp: DateTime<Utc>,
    pub total_value_sol: f64,
    pub sol_balance: f64,
    pub position_count: usize,
    pub unrealized_pnl_sol: f64,
    pub realized_pnl_sol: f64,
}

/// Portfolio performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Total return percentage
    pub total_return_percent: f64,
    /// Daily P&L in SOL
    pub daily_pnl_sol: f64,
    /// Weekly P&L in SOL
    pub weekly_pnl_sol: f64,
    /// Monthly P&L in SOL
    pub monthly_pnl_sol: f64,
    /// Win rate (successful trades / total trades)
    pub win_rate: f64,
    /// Average gain per winning trade
    pub avg_win_sol: f64,
    /// Average loss per losing trade
    pub avg_loss_sol: f64,
    /// Maximum drawdown percentage
    pub max_drawdown_percent: f64,
    /// Sharpe ratio (risk-adjusted returns)
    pub sharpe_ratio: Option<f64>,
    /// Number of active positions
    pub active_positions: usize,
    /// Portfolio diversity score (0-1)
    pub diversity_score: f64,
}

/// Position update event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionUpdate {
    pub wallet_address: String,
    pub mint: String,
    pub update_type: PositionUpdateType,
    pub quantity: u64,
    pub price_sol: f64,
    pub timestamp: DateTime<Utc>,
    pub transaction_signature: Option<String>,
}

/// Types of position updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PositionUpdateType {
    Open,       // New position opened
    Add,        // Added to existing position
    Reduce,     // Reduced position size
    Close,      // Position fully closed
    PriceUpdate, // Price update without quantity change
}

/// Portfolio tracking configuration
#[derive(Debug, Clone)]
pub struct PortfolioConfig {
    /// Primary Solana RPC endpoint
    pub rpc_endpoint: String,
    /// Fallback RPC endpoints for reliability
    pub fallback_rpc_endpoints: Vec<String>,
    /// DEX client configuration for pricing
    pub dex_config: DexConfig,
    /// Update interval for real-time tracking
    pub update_interval_secs: u64,
    /// How often to take portfolio snapshots
    pub snapshot_interval_secs: u64,
    /// Native SOL mint address
    pub sol_mint: String,
    /// Maximum number of concurrent price updates
    pub max_concurrent_updates: usize,
}

impl Default for PortfolioConfig {
    fn default() -> Self {
        Self {
            rpc_endpoint: "https://api.mainnet-beta.solana.com".to_string(),
            fallback_rpc_endpoints: vec![
                "https://solana-mainnet.phantom.app".to_string(),
                "https://rpc.solanabeach.io".to_string(),
            ],
            dex_config: DexConfig::default(),
            update_interval_secs: 30, // Update every 30 seconds
            snapshot_interval_secs: 3600, // Hourly snapshots
            sol_mint: "So11111111111111111111111111111111111111112".to_string(),
            max_concurrent_updates: 20,
        }
    }
}

/// Main portfolio tracking system
pub struct PortfolioTracker {
    /// Solana RPC client
    rpc_client: RpcClient,
    /// DEX client for price fetching
    dex_client: DexClient,
    /// Memory-mapped database for fast lookups
    mmap_db: Arc<UltraFastWalletDB>,
    /// Portfolios by wallet address
    portfolios: Arc<RwLock<HashMap<String, Portfolio>>>,
    /// Token metadata cache (mint -> (symbol, decimals))
    token_cache: Arc<DashMap<String, (Option<String>, u8)>>,
    /// Configuration
    config: PortfolioConfig,
    /// Last snapshot timestamp
    last_snapshot: Arc<RwLock<DateTime<Utc>>>,
}

impl PortfolioTracker {
    /// Create new portfolio tracker
    #[instrument]
    pub fn new(config: PortfolioConfig, mmap_db: Arc<UltraFastWalletDB>) -> Result<Self> {
        info!("Initializing portfolio tracker with RPC: {}", config.rpc_endpoint);

        let rpc_client = RpcClient::new_with_commitment(
            config.rpc_endpoint.clone(),
            CommitmentConfig::confirmed(),
        );

        let dex_client = DexClient::new(config.dex_config.clone())
            .context("Failed to initialize DEX client")?;

        Ok(Self {
            rpc_client,
            dex_client,
            mmap_db,
            portfolios: Arc::new(RwLock::new(HashMap::new())),
            token_cache: Arc::new(DashMap::new()),
            config,
            last_snapshot: Arc::new(RwLock::new(Utc::now())),
        })
    }

    /// Add wallet for portfolio tracking
    #[instrument(skip(self))]
    pub async fn track_wallet(&self, wallet_address: String) -> Result<()> {
        info!("Adding wallet to portfolio tracking: {}", wallet_address);

        let mut portfolios = self.portfolios.write().unwrap();
        if !portfolios.contains_key(&wallet_address) {
            portfolios.insert(wallet_address.clone(), Portfolio::new(wallet_address.clone()));
            info!("Created new portfolio for wallet: {}", wallet_address);
        }
        drop(portfolios);

        // Initial portfolio sync
        self.sync_wallet_portfolio(&wallet_address).await?;
        
        Ok(())
    }

    /// Remove wallet from tracking
    pub fn untrack_wallet(&self, wallet_address: &str) -> Result<()> {
        let mut portfolios = self.portfolios.write().unwrap();
        portfolios.remove(wallet_address);
        info!("Removed wallet from portfolio tracking: {}", wallet_address);
        Ok(())
    }

    /// Sync portfolio with current on-chain state
    #[instrument(skip(self))]
    pub async fn sync_wallet_portfolio(&self, wallet_address: &str) -> Result<()> {
        debug!("Syncing portfolio for wallet: {}", wallet_address);

        let pubkey = Pubkey::from_str(wallet_address)
            .context("Invalid wallet address")?;

        // Get SOL balance
        let sol_balance = self.get_sol_balance(&pubkey).await?;
        
        // Get all token accounts
        let token_accounts = self.get_token_accounts(&pubkey).await?;
        
        let mut portfolios = self.portfolios.write().unwrap();
        let portfolio = portfolios.get_mut(wallet_address)
            .context("Portfolio not found for wallet")?;

        // Update SOL balance
        portfolio.sol_balance = sol_balance;

        // Update token positions
        for (mint, account_info) in token_accounts {
            let (quantity, token_account_address) = account_info;
            
            if quantity == 0 {
                // Remove closed positions
                portfolio.positions.remove(&mint);
                continue;
            }

            // Get or create position
            if let Some(position) = portfolio.positions.get_mut(&mint) {
                // Update existing position quantity if it changed
                if position.quantity != quantity {
                    let delta = quantity as i64 - position.quantity as i64;
                    if delta > 0 {
                        // Additional tokens acquired (averaging could happen here)
                        position.quantity = quantity;
                    } else {
                        // Tokens were sold
                        let reduction = position.quantity - quantity;
                        position.reduce_quantity(reduction, position.current_price_sol, None);
                    }
                }
            } else {
                // New position detected
                let (symbol, decimals) = self.get_token_metadata(&mint).await.unwrap_or((None, 9));
                
                // Try to get current price (default to 0 if unavailable)
                let current_price = self.get_token_price_sol(&mint).await.unwrap_or(0.0);
                
                let position = Position::new(
                    mint.clone(),
                    symbol,
                    decimals,
                    quantity,
                    current_price, // Use current price as entry price for discovered positions
                    token_account_address,
                );
                
                portfolio.positions.insert(mint, position);
            }
        }

        // Update prices for all positions
        for (mint, position) in portfolio.positions.iter_mut() {
            if let Ok(price) = self.get_token_price_sol(mint).await {
                position.update_price(price);
            }
        }

        // Recalculate totals
        portfolio.recalculate_totals();

        info!("Portfolio synced for {}: {} SOL, {} positions, total value: {:.6} SOL", 
              wallet_address, 
              portfolio.sol_balance, 
              portfolio.positions.len(),
              portfolio.total_value_sol);

        Ok(())
    }

    /// Get SOL balance for wallet with fallback RPC endpoints
    async fn get_sol_balance(&self, pubkey: &Pubkey) -> Result<f64> {
        // Try primary endpoint first
        match self.rpc_client.get_balance(pubkey) {
            Ok(balance_lamports) => {
                debug!("Successfully fetched SOL balance for {}: {} lamports", pubkey, balance_lamports);
                return Ok(balance_lamports as f64 / 1_000_000_000.0);
            }
            Err(e) => {
                warn!("Failed to get SOL balance from primary endpoint {}: {}", 
                     self.config.rpc_endpoint, e);
            }
        }

        // Try fallback endpoints
        for (i, fallback_endpoint) in self.config.fallback_rpc_endpoints.iter().enumerate() {
            debug!("Trying fallback RPC endpoint {}: {}", i + 1, fallback_endpoint);
            
            let fallback_client = RpcClient::new_with_commitment(
                fallback_endpoint.clone(),
                CommitmentConfig::confirmed(),
            );
            
            match fallback_client.get_balance(pubkey) {
                Ok(balance_lamports) => {
                    info!("Successfully fetched SOL balance using fallback endpoint {}: {}", 
                          fallback_endpoint, balance_lamports);
                    return Ok(balance_lamports as f64 / 1_000_000_000.0);
                }
                Err(e) => {
                    warn!("Fallback endpoint {} failed: {}", fallback_endpoint, e);
                    continue;
                }
            }
        }
        
        // All endpoints failed - return 0.0 to prevent system crash
        warn!("All RPC endpoints failed for wallet {}. Returning 0.0 SOL balance", pubkey);
        Ok(0.0)
    }

    /// Get all token accounts for wallet
    async fn get_token_accounts(&self, pubkey: &Pubkey) -> Result<HashMap<String, (u64, String)>> {
        let token_accounts = match self.rpc_client
            .get_token_accounts_by_owner(
                pubkey,
                solana_client::rpc_request::TokenAccountsFilter::ProgramId(spl_token::id()),
            ) {
                Ok(accounts) => {
                    debug!("Successfully fetched {} token accounts for wallet {}", accounts.len(), pubkey);
                    accounts
                }
                Err(e) => {
                    warn!("Failed to get token accounts for wallet {}: {} - RPC endpoint: {}", 
                         pubkey, e, self.config.rpc_endpoint);
                    warn!("Returning empty token accounts to prevent system failure");
                    return Ok(HashMap::new());
                }
            };

        let mut accounts = HashMap::new();

        for account in token_accounts {
            // Extract raw data from UiAccountData
            if let solana_account_decoder::UiAccountData::Binary(data, _) = &account.account.data {
                if let Ok(decoded_data) = bs58::decode(data).into_vec() {
                    if let Ok(token_account) = spl_token::state::Account::unpack(&decoded_data) {
                        if token_account.amount > 0 {
                            accounts.insert(
                                token_account.mint.to_string(),
                                (token_account.amount, account.pubkey.to_string())
                            );
                        }
                    }
                }
            }
        }

        Ok(accounts)
    }

    /// Get token metadata (symbol and decimals)
    async fn get_token_metadata(&self, mint: &str) -> Result<(Option<String>, u8)> {
        // Check cache first
        if let Some(cached) = self.token_cache.get(mint) {
            return Ok(cached.clone());
        }

        let mint_pubkey = Pubkey::from_str(mint)
            .context("Invalid mint address")?;

        // Get mint account to get decimals
        let mint_account = self.rpc_client.get_account(&mint_pubkey)
            .context("Failed to get mint account")?;

        let mint_data = Mint::unpack(&mint_account.data)
            .context("Failed to parse mint data")?;

        // For now, we don't have symbol resolution (could integrate with token lists)
        let metadata = (None, mint_data.decimals);
        
        // Cache the result
        self.token_cache.insert(mint.to_string(), metadata.clone());
        
        Ok(metadata)
    }

    /// Get current token price in SOL using DEX
    async fn get_token_price_sol(&self, mint: &str) -> Result<f64> {
        if mint == self.config.sol_mint {
            return Ok(1.0); // SOL/SOL = 1
        }

        // Get price quote from DEX (using 1 token as base amount)
        let price = self.dex_client.get_price(
            mint,
            &self.config.sol_mint,
            1_000_000_000, // 1 token with 9 decimals
        ).await.context("Failed to get token price")?;

        Ok(price)
    }

    /// Update position manually (for trade tracking)
    #[instrument(skip(self))]
    pub async fn update_position(
        &self, 
        wallet_address: &str, 
        update: PositionUpdate
    ) -> Result<()> {
        info!("Processing position update for {}: {:?}", wallet_address, update.update_type);

        // Get token metadata first, outside the lock
        let (symbol, decimals) = match update.update_type {
            PositionUpdateType::Open => {
                self.get_token_metadata(&update.mint).await.unwrap_or((None, 9))
            },
            _ => (None, 9), // Not needed for other update types
        };

        let mut portfolios = self.portfolios.write().unwrap();
        let portfolio = portfolios.get_mut(wallet_address)
            .context("Portfolio not found for wallet")?;

        match update.update_type {
            PositionUpdateType::Open => {
                let position = Position::new(
                    update.mint.clone(),
                    symbol,
                    decimals,
                    update.quantity,
                    update.price_sol,
                    "".to_string(), // Token account will be filled during sync
                );
                portfolio.positions.insert(update.mint, position);
            }
            PositionUpdateType::Add => {
                if let Some(position) = portfolio.positions.get_mut(&update.mint) {
                    position.add_quantity(update.quantity, update.price_sol, update.transaction_signature);
                }
            }
            PositionUpdateType::Reduce => {
                if let Some(position) = portfolio.positions.get_mut(&update.mint) {
                    position.reduce_quantity(update.quantity, update.price_sol, update.transaction_signature);
                }
            }
            PositionUpdateType::Close => {
                portfolio.positions.remove(&update.mint);
            }
            PositionUpdateType::PriceUpdate => {
                if let Some(position) = portfolio.positions.get_mut(&update.mint) {
                    position.update_price(update.price_sol);
                }
            }
        }

        portfolio.recalculate_totals();
        Ok(())
    }

    /// Get portfolio for specific wallet
    pub fn get_portfolio(&self, wallet_address: &str) -> Option<Portfolio> {
        let portfolios = self.portfolios.read().unwrap();
        portfolios.get(wallet_address).cloned()
    }

    /// Get all tracked portfolios
    pub fn get_all_portfolios(&self) -> HashMap<String, Portfolio> {
        let portfolios = self.portfolios.read().unwrap();
        portfolios.clone()
    }

    /// Calculate performance metrics for a portfolio
    pub fn calculate_performance_metrics(&self, wallet_address: &str) -> Result<PerformanceMetrics> {
        let portfolio = self.get_portfolio(wallet_address)
            .context("Portfolio not found")?;

        // Calculate metrics based on snapshots
        let mut daily_pnl = 0.0;
        let mut weekly_pnl = 0.0;
        let mut monthly_pnl = 0.0;

        let now = Utc::now();
        let day_ago = now - ChronoDuration::days(1);
        let week_ago = now - ChronoDuration::weeks(1);
        let month_ago = now - ChronoDuration::days(30);

        // Find closest snapshots to time periods
        if let Some(day_snapshot) = portfolio.snapshots.range(day_ago..).next() {
            daily_pnl = portfolio.total_value_sol - day_snapshot.1.total_value_sol;
        }

        if let Some(week_snapshot) = portfolio.snapshots.range(week_ago..).next() {
            weekly_pnl = portfolio.total_value_sol - week_snapshot.1.total_value_sol;
        }

        if let Some(month_snapshot) = portfolio.snapshots.range(month_ago..).next() {
            monthly_pnl = portfolio.total_value_sol - month_snapshot.1.total_value_sol;
        }

        // Calculate diversity score (concentration risk)
        let allocation = portfolio.get_asset_allocation();
        let diversity_score = self.calculate_diversity_score(&allocation);

        // Basic win rate calculation (would need trade history for accuracy)
        let winning_positions = portfolio.positions.values()
            .filter(|p| p.unrealized_pnl_sol > 0.0)
            .count();
        let total_positions = portfolio.positions.len().max(1);
        let win_rate = winning_positions as f64 / total_positions as f64;

        // Calculate average win/loss
        let (avg_win, avg_loss) = portfolio.positions.values().fold((0.0, 0.0), |(mut win_sum, mut loss_sum), pos| {
            if pos.unrealized_pnl_sol > 0.0 {
                win_sum += pos.unrealized_pnl_sol;
            } else if pos.unrealized_pnl_sol < 0.0 {
                loss_sum += pos.unrealized_pnl_sol.abs();
            }
            (win_sum, loss_sum)
        });

        let winning_count = portfolio.positions.values().filter(|p| p.unrealized_pnl_sol > 0.0).count().max(1);
        let losing_count = portfolio.positions.values().filter(|p| p.unrealized_pnl_sol < 0.0).count().max(1);

        Ok(PerformanceMetrics {
            total_return_percent: if portfolio.total_value_sol > 0.0 {
                (portfolio.total_unrealized_pnl_sol + portfolio.total_realized_pnl_sol) / portfolio.total_value_sol * 100.0
            } else { 0.0 },
            daily_pnl_sol: daily_pnl,
            weekly_pnl_sol: weekly_pnl,
            monthly_pnl_sol: monthly_pnl,
            win_rate: win_rate * 100.0,
            avg_win_sol: avg_win / winning_count as f64,
            avg_loss_sol: avg_loss / losing_count as f64,
            max_drawdown_percent: 0.0, // Would need historical data
            sharpe_ratio: None, // Would need risk-free rate and volatility calculation
            active_positions: portfolio.positions.len(),
            diversity_score,
        })
    }

    /// Calculate portfolio diversity score (0-1, higher is more diverse)
    fn calculate_diversity_score(&self, allocation: &HashMap<String, f64>) -> f64 {
        if allocation.is_empty() {
            return 0.0;
        }

        // Calculate Herfindahl-Hirschman Index (HHI) and convert to diversity score
        let hhi: f64 = allocation.values()
            .map(|&percentage| (percentage / 100.0).powi(2))
            .sum();

        // Convert HHI to diversity score (1 - HHI gives higher score for more diversity)
        (1.0 - hhi).max(0.0)
    }

    /// Start real-time portfolio tracking background task
    pub async fn start_tracking(&self, wallet_manager: Arc<WalletManager>) -> Result<()> {
        info!("Starting real-time portfolio tracking");

        // Get trading wallet for tracking
        let trading_wallet = wallet_manager.get_public_key(&WalletType::Trading)
            .context("Trading wallet not found")?;
        
        self.track_wallet(trading_wallet.to_string()).await?;

        let portfolios = Arc::clone(&self.portfolios);
        let config = self.config.clone();
        let last_snapshot = Arc::clone(&self.last_snapshot);

        // Spawn background tracking task
        let tracker = self.clone_for_background();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(config.update_interval_secs));
            let mut snapshot_interval = tokio::time::interval(Duration::from_secs(config.snapshot_interval_secs));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Update all portfolios
                        let wallet_addresses: Vec<String> = {
                            let portfolios = portfolios.read().unwrap();
                            portfolios.keys().cloned().collect()
                        };

                        for wallet_address in wallet_addresses {
                            if let Err(e) = tracker.sync_wallet_portfolio(&wallet_address).await {
                                error!("Failed to sync portfolio for {}: {}", wallet_address, e);
                            }
                        }
                    }
                    _ = snapshot_interval.tick() => {
                        // Take snapshots of all portfolios
                        let wallet_addresses: Vec<String> = {
                            let portfolios = portfolios.read().unwrap();
                            portfolios.keys().cloned().collect()
                        };
                        
                        for wallet_address in wallet_addresses {
                            let mut portfolios = portfolios.write().unwrap();
                            if let Some(portfolio) = portfolios.get_mut(&wallet_address) {
                                portfolio.take_snapshot();
                            }
                        }
                        
                        let mut last_snapshot = last_snapshot.write().unwrap();
                        *last_snapshot = Utc::now();
                        
                        debug!("Portfolio snapshots taken");
                    }
                }
            }
        });

        Ok(())
    }

    /// Clone tracker for background tasks (shared components)
    fn clone_for_background(&self) -> PortfolioTrackerClone {
        PortfolioTrackerClone {
            rpc_client: RpcClient::new_with_commitment(
                self.config.rpc_endpoint.clone(),
                CommitmentConfig::confirmed(),
            ),
            dex_client: DexClient::new(self.config.dex_config.clone()).unwrap(),
            portfolios: Arc::clone(&self.portfolios),
            token_cache: Arc::clone(&self.token_cache),
            config: self.config.clone(),
        }
    }
}

/// Lightweight clone of portfolio tracker for background tasks
struct PortfolioTrackerClone {
    rpc_client: RpcClient,
    dex_client: DexClient,
    portfolios: Arc<RwLock<HashMap<String, Portfolio>>>,
    token_cache: Arc<DashMap<String, (Option<String>, u8)>>,
    config: PortfolioConfig,
}

impl PortfolioTrackerClone {
    /// Sync portfolio (same implementation as main tracker)
    async fn sync_wallet_portfolio(&self, wallet_address: &str) -> Result<()> {
        debug!("Background sync for wallet: {}", wallet_address);

        let pubkey = Pubkey::from_str(wallet_address)
            .context("Invalid wallet address")?;

        // Get SOL balance
        let sol_balance = self.get_sol_balance(&pubkey).await?;
        
        // Get all token accounts  
        let _token_accounts = self.get_token_accounts(&pubkey).await?;
        
        // Get list of mints to update prices for
        let mints_to_update: Vec<String> = {
            let portfolios = self.portfolios.read().unwrap();
            if let Some(portfolio) = portfolios.get(wallet_address) {
                portfolio.positions.keys().cloned().collect()
            } else {
                return Err(anyhow::anyhow!("Portfolio not found for wallet: {}", wallet_address));
            }
        };

        // Update prices for each mint (outside the lock)
        let mut price_updates = HashMap::new();
        for mint in mints_to_update {
            if let Ok(price) = self.get_token_price_sol(&mint).await {
                price_updates.insert(mint, price);
            }
        }

        // Apply all updates with a single lock
        {
            let mut portfolios = self.portfolios.write().unwrap();
            if let Some(portfolio) = portfolios.get_mut(wallet_address) {
                // Update SOL balance
                portfolio.sol_balance = sol_balance;

                // Update position prices
                for (mint, price) in price_updates {
                    if let Some(position) = portfolio.positions.get_mut(&mint) {
                        position.update_price(price);
                    }
                }

                portfolio.recalculate_totals();
            }
        }

        Ok(())
    }

    async fn get_sol_balance(&self, pubkey: &Pubkey) -> Result<f64> {
        // Try primary endpoint first
        match self.rpc_client.get_balance(pubkey) {
            Ok(balance_lamports) => {
                debug!("Successfully fetched SOL balance for {}: {} lamports", pubkey, balance_lamports);
                return Ok(balance_lamports as f64 / 1_000_000_000.0);
            }
            Err(e) => {
                warn!("Failed to get SOL balance from primary endpoint {}: {}", 
                     self.config.rpc_endpoint, e);
            }
        }

        // Try fallback endpoints
        for (i, fallback_endpoint) in self.config.fallback_rpc_endpoints.iter().enumerate() {
            debug!("Trying fallback RPC endpoint {}: {}", i + 1, fallback_endpoint);
            
            let fallback_client = RpcClient::new_with_commitment(
                fallback_endpoint.clone(),
                CommitmentConfig::confirmed(),
            );
            
            match fallback_client.get_balance(pubkey) {
                Ok(balance_lamports) => {
                    info!("Successfully fetched SOL balance using fallback endpoint {}: {}", 
                          fallback_endpoint, balance_lamports);
                    return Ok(balance_lamports as f64 / 1_000_000_000.0);
                }
                Err(e) => {
                    warn!("Fallback endpoint {} failed: {}", fallback_endpoint, e);
                    continue;
                }
            }
        }
        
        // All endpoints failed - return 0.0 to prevent system crash
        warn!("All RPC endpoints failed for wallet {}. Returning 0.0 SOL balance", pubkey);
        Ok(0.0)
    }

    async fn get_token_accounts(&self, pubkey: &Pubkey) -> Result<HashMap<String, (u64, String)>> {
        let token_accounts = match self.rpc_client
            .get_token_accounts_by_owner(
                pubkey,
                solana_client::rpc_request::TokenAccountsFilter::ProgramId(spl_token::id()),
            ) {
                Ok(accounts) => {
                    debug!("Successfully fetched {} token accounts for wallet {}", accounts.len(), pubkey);
                    accounts
                }
                Err(e) => {
                    warn!("Failed to get token accounts for wallet {}: {} - RPC endpoint: {}", 
                         pubkey, e, self.config.rpc_endpoint);
                    warn!("Returning empty token accounts to prevent system failure");
                    return Ok(HashMap::new());
                }
            };

        let mut accounts = HashMap::new();
        for account in token_accounts {
            // Extract raw data from UiAccountData
            if let solana_account_decoder::UiAccountData::Binary(data, _) = &account.account.data {
                if let Ok(decoded_data) = bs58::decode(data).into_vec() {
                    if let Ok(token_account) = spl_token::state::Account::unpack(&decoded_data) {
                        if token_account.amount > 0 {
                            accounts.insert(
                                token_account.mint.to_string(),
                                (token_account.amount, account.pubkey.to_string())
                            );
                        }
                    }
                }
            }
        }

        Ok(accounts)
    }

    async fn get_token_price_sol(&self, mint: &str) -> Result<f64> {
        if mint == self.config.sol_mint {
            return Ok(1.0);
        }

        let price = self.dex_client.get_price(
            mint,
            &self.config.sol_mint,
            1_000_000_000,
        ).await.context("Failed to get token price")?;

        Ok(price)
    }
}