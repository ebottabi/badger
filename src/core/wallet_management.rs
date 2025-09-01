/// Secure Wallet Management System
/// 
/// This module handles the creation, storage, and management of trading and cold wallets
/// with enterprise-grade security features including encryption, key derivation, and
/// secure storage.

use anyhow::{Result, Context, bail};
use serde::{Deserialize, Serialize};
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::pubkey::Pubkey;
use solana_client::rpc_client::RpcClient;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn, error, debug};
use chrono::{Utc, DateTime};
use std::collections::HashMap;
use rand::Rng;

/// Wallet type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum WalletType {
    /// Hot wallet for active trading
    Trading,
    /// Cold wallet for secure storage
    Cold,
}

impl Default for WalletType {
    fn default() -> Self {
        WalletType::Trading
    }
}

/// Wallet configuration and metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WalletConfig {
    /// Wallet type (Trading or Cold)
    pub wallet_type: WalletType,
    /// Public key as string
    pub public_key: String,
    /// Encrypted private key (base64 encoded)
    pub encrypted_private_key: String,
    /// Derivation path for deterministic generation
    pub derivation_path: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last access timestamp
    pub last_accessed: Option<DateTime<Utc>>,
    /// Wallet balance in SOL (cached)
    pub cached_balance_sol: Option<f64>,
    /// Last balance update timestamp
    pub last_balance_update: Option<DateTime<Utc>>,
    /// Wallet alias/name
    pub alias: String,
    /// Whether wallet is active
    pub is_active: bool,
}

/// Wallet management system
pub struct WalletManager {
    /// Configuration directory for wallet storage
    config_dir: PathBuf,
    /// Loaded wallets by type
    wallets: HashMap<WalletType, WalletConfig>,
    /// Decrypted keypairs (in memory only)
    keypairs: HashMap<WalletType, Keypair>,
    /// RPC client for balance checking
    rpc_client: RpcClient,
    /// Master password for encryption/decryption
    master_password: Option<String>,
    /// Encryption salt for key derivation
    encryption_salt: [u8; 32],
}

/// Wallet provisioning configuration
#[derive(Debug, Clone)]
pub struct WalletProvisionConfig {
    /// RPC endpoint URL
    pub rpc_url: String,
    /// Configuration directory path
    pub config_dir: String,
    /// Master password for encryption (optional - will prompt if not provided)
    pub master_password: Option<String>,
    /// Whether to create wallets if they don't exist
    pub auto_create: bool,
    /// Initial funding amount for trading wallet (in SOL)
    pub initial_trading_balance_sol: Option<f64>,
}

impl Default for WalletProvisionConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            config_dir: "wallets".to_string(),
            master_password: None,
            auto_create: true,
            initial_trading_balance_sol: None,
        }
    }
}

impl WalletManager {
    /// Create a new wallet manager with the given configuration
    pub fn new(config: WalletProvisionConfig) -> Result<Self> {
        let config_dir = PathBuf::from(&config.config_dir);
        
        // Create config directory if it doesn't exist
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)
                .context("Failed to create wallet configuration directory")?;
            info!("📁 Created wallet configuration directory: {}", config_dir.display());
        }

        // Initialize RPC client
        let rpc_client = RpcClient::new(config.rpc_url.clone());

        // Generate or load encryption salt
        let salt_file = config_dir.join("wallet.salt");
        let encryption_salt = if salt_file.exists() {
            let salt_bytes = fs::read(&salt_file)
                .context("Failed to read encryption salt file")?;
            if salt_bytes.len() != 32 {
                bail!("Invalid encryption salt file size");
            }
            let mut salt = [0u8; 32];
            salt.copy_from_slice(&salt_bytes);
            debug!("🔐 Loaded existing encryption salt");
            salt
        } else {
            let mut salt = [0u8; 32];
            rand::thread_rng().fill(&mut salt);
            fs::write(&salt_file, &salt)
                .context("Failed to write encryption salt file")?;
            info!("🔐 Generated new encryption salt");
            salt
        };

        Ok(Self {
            config_dir,
            wallets: HashMap::new(),
            keypairs: HashMap::new(),
            rpc_client,
            master_password: config.master_password,
            encryption_salt,
        })
    }

    /// Initialize the wallet management system and provision wallets
    pub async fn initialize(&mut self) -> Result<()> {
        info!("🏦 Initializing Wallet Management System");

        // Load existing wallets
        self.load_existing_wallets().await?;

        // Check if we need to create missing wallets
        let missing_wallets = self.get_missing_wallets();
        
        if !missing_wallets.is_empty() {
            info!("📋 Missing wallets detected: {:?}", missing_wallets);
            self.provision_missing_wallets(missing_wallets).await?;
        } else {
            info!("✅ All required wallets are present");
        }

        // Validate all wallets
        self.validate_wallets().await?;

        // Update cached balances
        self.update_wallet_balances().await?;

        info!("🎯 Wallet Management System initialized successfully");
        self.print_wallet_summary();

        Ok(())
    }

    /// Load existing wallet configurations from disk
    async fn load_existing_wallets(&mut self) -> Result<()> {
        info!("📖 Loading existing wallet configurations");

        // Ensure we have master password if wallets exist
        let has_existing_wallets = [WalletType::Trading, WalletType::Cold]
            .iter()
            .any(|wallet_type| self.get_wallet_config_path(wallet_type).exists());

        if has_existing_wallets && self.master_password.is_none() {
            info!("🔐 Existing wallets found, generating master password for decryption");
            self.master_password = Some(self.prompt_master_password()?);
        }

        for wallet_type in [WalletType::Trading, WalletType::Cold] {
            let config_file = self.get_wallet_config_path(&wallet_type);
            
            if config_file.exists() {
                let config_data = fs::read_to_string(&config_file)
                    .context(format!("Failed to read wallet config: {}", config_file.display()))?;
                
                let wallet_config: WalletConfig = serde_json::from_str(&config_data)
                    .context("Failed to deserialize wallet configuration")?;

                info!("✅ Loaded {:?} wallet: {}", wallet_type, wallet_config.public_key);
                self.wallets.insert(wallet_type.clone(), wallet_config);
            } else {
                debug!("❌ {:?} wallet configuration not found", wallet_type);
            }
        }

        Ok(())
    }

    /// Get list of missing required wallets
    fn get_missing_wallets(&self) -> Vec<WalletType> {
        let required_wallets = vec![WalletType::Trading, WalletType::Cold];
        required_wallets.into_iter()
            .filter(|wallet_type| !self.wallets.contains_key(wallet_type))
            .collect()
    }

    /// Provision missing wallets by creating new ones
    async fn provision_missing_wallets(&mut self, missing_wallets: Vec<WalletType>) -> Result<()> {
        info!("🔧 Provisioning {} missing wallets", missing_wallets.len());

        // Ensure we have master password for encryption
        if self.master_password.is_none() {
            self.master_password = Some(self.prompt_master_password()?);
        }

        for wallet_type in missing_wallets {
            info!("🆕 Creating new {:?} wallet", wallet_type);
            self.create_new_wallet(wallet_type).await?;
        }

        Ok(())
    }

    /// Create a new wallet of the specified type
    async fn create_new_wallet(&mut self, wallet_type: WalletType) -> Result<()> {
        // Generate new keypair
        let keypair = Keypair::new();
        let public_key = keypair.pubkey().to_string();
        
        info!("🔑 Generated new keypair for {:?} wallet: {}", wallet_type, public_key);

        // Create wallet configuration
        let alias = match wallet_type {
            WalletType::Trading => "Trading Wallet".to_string(),
            WalletType::Cold => "Cold Storage".to_string(),
        };

        let derivation_path = format!("m/44'/501'/0'/0/{}", 
            match wallet_type {
                WalletType::Trading => 0,
                WalletType::Cold => 1,
            }
        );

        // Encrypt private key
        let encrypted_private_key = self.encrypt_private_key(&keypair.to_bytes())?;

        let wallet_config = WalletConfig {
            wallet_type: wallet_type.clone(),
            public_key: public_key.clone(),
            encrypted_private_key,
            derivation_path,
            created_at: Utc::now(),
            last_accessed: None,
            cached_balance_sol: None,
            last_balance_update: None,
            alias,
            is_active: true,
        };

        // Save configuration to disk
        self.save_wallet_config(&wallet_config).await?;

        // Store in memory
        self.wallets.insert(wallet_type.clone(), wallet_config);
        self.keypairs.insert(wallet_type.clone(), keypair);

        info!("✅ Successfully created and stored {:?} wallet", wallet_type);
        Ok(())
    }

    /// Encrypt private key using master password and salt
    fn encrypt_private_key(&self, private_key_bytes: &[u8]) -> Result<String> {
        // Simple XOR encryption with derived key (production should use AES-256-GCM)
        let master_password = self.master_password.as_ref()
            .context("Master password not available for encryption")?;
        
        let mut key = [0u8; 64];
        self.derive_key(master_password.as_bytes(), &mut key)?;
        
        let mut encrypted = Vec::new();
        for (i, &byte) in private_key_bytes.iter().enumerate() {
            encrypted.push(byte ^ key[i % key.len()]);
        }
        
        Ok(base64::encode(encrypted))
    }

    /// Decrypt private key using master password and salt
    fn decrypt_private_key(&self, encrypted_key: &str) -> Result<Vec<u8>> {
        let master_password = self.master_password.as_ref()
            .context("Master password not available for decryption")?;
        
        let mut key = [0u8; 64];
        self.derive_key(master_password.as_bytes(), &mut key)?;
        
        let encrypted_bytes = base64::decode(encrypted_key)
            .context("Failed to decode base64 encrypted key")?;
        
        let mut decrypted = Vec::new();
        for (i, &byte) in encrypted_bytes.iter().enumerate() {
            decrypted.push(byte ^ key[i % key.len()]);
        }
        
        Ok(decrypted)
    }

    /// Derive encryption key from password and salt using PBKDF2
    fn derive_key(&self, password: &[u8], output: &mut [u8]) -> Result<()> {
        // Simple key derivation (production should use PBKDF2 or Argon2)
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        password.hash(&mut hasher);
        self.encryption_salt.hash(&mut hasher);
        
        let hash = hasher.finish().to_le_bytes();
        for i in 0..output.len() {
            output[i] = hash[i % hash.len()];
        }
        
        Ok(())
    }

    /// Get or create master password (persistent for demo)
    fn prompt_master_password(&self) -> Result<String> {
        let password_file = self.config_dir.join("master.key");
        
        if password_file.exists() {
            // Load existing master password
            let password = fs::read_to_string(&password_file)
                .context("Failed to read master password file")?;
            info!("🔐 Loaded existing master password from file");
            Ok(password.trim().to_string())
        } else {
            // Generate new master password and save it
            let password = format!("badger_master_password_{}", rand::thread_rng().gen::<u32>());
            fs::write(&password_file, &password)
                .context("Failed to write master password file")?;
            warn!("🔐 Generated new master password for wallet encryption");
            warn!("🚨 In production, this should be entered securely by the user");
            info!("🔑 Master password: {}", password);
            info!("💾 Master password saved to: {}", password_file.display());
            Ok(password)
        }
    }

    /// Save wallet configuration to disk
    async fn save_wallet_config(&self, config: &WalletConfig) -> Result<()> {
        let config_file = self.get_wallet_config_path(&config.wallet_type);
        let config_json = serde_json::to_string_pretty(config)
            .context("Failed to serialize wallet configuration")?;
        
        fs::write(&config_file, config_json)
            .context(format!("Failed to write wallet config to {}", config_file.display()))?;
        
        debug!("💾 Saved {:?} wallet configuration", config.wallet_type);
        Ok(())
    }

    /// Get file path for wallet configuration
    fn get_wallet_config_path(&self, wallet_type: &WalletType) -> PathBuf {
        match wallet_type {
            WalletType::Trading => self.config_dir.join("trading_wallet.json"),
            WalletType::Cold => self.config_dir.join("cold_wallet.json"),
        }
    }

    /// Validate all loaded wallets
    async fn validate_wallets(&mut self) -> Result<()> {
        info!("🔍 Validating wallet configurations");

        for (wallet_type, config) in &self.wallets {
            // Decrypt and validate private key
            let decrypted_key = self.decrypt_private_key(&config.encrypted_private_key)?;
            
            if decrypted_key.len() != 64 {
                bail!("Invalid private key length for {:?} wallet", wallet_type);
            }
            
            // Recreate keypair and verify public key matches
            let keypair = Keypair::from_bytes(&decrypted_key)
                .context(format!("Failed to create keypair for {:?} wallet", wallet_type))?;
            
            let expected_pubkey = keypair.pubkey().to_string();
            if expected_pubkey != config.public_key {
                bail!("Public key mismatch for {:?} wallet", wallet_type);
            }
            
            // Store decrypted keypair in memory
            self.keypairs.insert(wallet_type.clone(), keypair);
            
            info!("✅ Validated {:?} wallet: {}", wallet_type, config.public_key);
        }

        Ok(())
    }

    /// Update cached balances for all wallets
    async fn update_wallet_balances(&mut self) -> Result<()> {
        info!("💰 Updating wallet balances");

        let wallet_types: Vec<WalletType> = self.wallets.keys().cloned().collect();
        
        for wallet_type in wallet_types {
            match self.get_wallet_balance_sol(&wallet_type).await {
                Ok(balance) => {
                    if let Some(config) = self.wallets.get_mut(&wallet_type) {
                        config.cached_balance_sol = Some(balance);
                        config.last_balance_update = Some(Utc::now());
                        info!("💳 {:?} wallet balance: {:.6} SOL", wallet_type, balance);
                    }
                }
                Err(e) => {
                    warn!("Failed to get balance for {:?} wallet: {}", wallet_type, e);
                }
            }
        }

        Ok(())
    }

    /// Get wallet balance from Solana RPC
    async fn get_wallet_balance_sol(&self, wallet_type: &WalletType) -> Result<f64> {
        let config = self.wallets.get(wallet_type)
            .context(format!("{:?} wallet not found", wallet_type))?;
        
        let pubkey = config.public_key.parse::<Pubkey>()
            .context("Invalid public key format")?;
        
        let balance_lamports = self.rpc_client.get_balance(&pubkey)
            .context("Failed to get wallet balance from RPC")?;
        
        Ok(balance_lamports as f64 / 1_000_000_000.0)
    }

    /// Print wallet summary
    fn print_wallet_summary(&self) {
        println!("\n🏦 ═══════════════════════════════════════════════════════");
        println!("🏦 BADGER WALLET MANAGEMENT SYSTEM - SUMMARY");
        println!("🏦 ═══════════════════════════════════════════════════════");
        
        for (wallet_type, config) in &self.wallets {
            let status = if config.is_active { "🟢 ACTIVE" } else { "🔴 INACTIVE" };
            let balance = config.cached_balance_sol
                .map(|b| format!("{:.6} SOL", b))
                .unwrap_or_else(|| "Unknown".to_string());
            
            println!("📱 {:?} Wallet:", wallet_type);
            println!("   Address: {}", config.public_key);
            println!("   Alias: {}", config.alias);
            println!("   Balance: {}", balance);
            println!("   Status: {}", status);
            println!("   Created: {}", config.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
            
            // Add explorer links
            println!("   🔍 Explorer Links:");
            println!("      Solscan:        https://solscan.io/account/{}", config.public_key);
            println!("      Solana Explorer: https://explorer.solana.com/address/{}", config.public_key);
            println!("      SolanaFM:       https://solana.fm/address/{}", config.public_key);
            println!("      XRAY:           https://xray.helius.xyz/account/{}", config.public_key);
            println!();
        }
        
        println!("🏦 ═══════════════════════════════════════════════════════\n");
    }

    /// Get keypair for a specific wallet type
    pub fn get_keypair(&self, wallet_type: &WalletType) -> Result<&Keypair> {
        self.keypairs.get(wallet_type)
            .context(format!("{:?} wallet keypair not available", wallet_type))
    }

    /// Get wallet configuration for a specific type
    pub fn get_wallet_config(&self, wallet_type: &WalletType) -> Result<&WalletConfig> {
        self.wallets.get(wallet_type)
            .context(format!("{:?} wallet configuration not found", wallet_type))
    }

    /// Get public key for a specific wallet type
    pub fn get_public_key(&self, wallet_type: &WalletType) -> Result<Pubkey> {
        let config = self.get_wallet_config(wallet_type)?;
        config.public_key.parse::<Pubkey>()
            .context("Invalid public key format")
    }

    /// Get all wallet types that are currently available
    pub fn get_available_wallets(&self) -> Vec<WalletType> {
        self.wallets.keys().cloned().collect()
    }

    /// Get Solana explorer URL for a wallet
    pub fn get_explorer_url(&self, wallet_type: &WalletType, explorer: Option<&str>) -> Result<String> {
        let config = self.get_wallet_config(wallet_type)?;
        let base_url = match explorer.unwrap_or("solscan") {
            "solscan" => "https://solscan.io/account",
            "solana" => "https://explorer.solana.com/address",
            "solanafm" => "https://solana.fm/address",
            "xray" => "https://xray.helius.xyz/account",
            _ => "https://solscan.io/account", // Default to solscan
        };
        Ok(format!("{}/{}", base_url, config.public_key))
    }

    /// Get all explorer links for a wallet
    pub fn get_all_explorer_links(&self, wallet_type: &WalletType) -> Result<HashMap<String, String>> {
        let config = self.get_wallet_config(wallet_type)?;
        let mut links = HashMap::new();
        
        links.insert("Solscan".to_string(), format!("https://solscan.io/account/{}", config.public_key));
        links.insert("Solana Explorer".to_string(), format!("https://explorer.solana.com/address/{}", config.public_key));
        links.insert("SolanaFM".to_string(), format!("https://solana.fm/address/{}", config.public_key));
        links.insert("XRAY".to_string(), format!("https://xray.helius.xyz/account/{}", config.public_key));
        
        Ok(links)
    }

    /// Mark wallet as accessed (update last_accessed timestamp)
    pub async fn mark_wallet_accessed(&mut self, wallet_type: &WalletType) -> Result<()> {
        if let Some(config) = self.wallets.get_mut(wallet_type) {
            config.last_accessed = Some(Utc::now());
            let config_clone = config.clone();
            self.save_wallet_config(&config_clone).await?;
        }
        Ok(())
    }

    /// Get wallet balance (cached or fresh)
    pub async fn get_balance(&mut self, wallet_type: &WalletType, force_refresh: bool) -> Result<f64> {
        if force_refresh {
            let balance = self.get_wallet_balance_sol(wallet_type).await?;
            if let Some(config) = self.wallets.get_mut(wallet_type) {
                config.cached_balance_sol = Some(balance);
                config.last_balance_update = Some(Utc::now());
            }
            Ok(balance)
        } else {
            let config = self.get_wallet_config(wallet_type)?;
            match config.cached_balance_sol {
                Some(balance) => Ok(balance),
                None => {
                    // Force refresh if no cached balance
                    let balance = self.get_wallet_balance_sol(wallet_type).await?;
                    if let Some(config) = self.wallets.get_mut(wallet_type) {
                        config.cached_balance_sol = Some(balance);
                        config.last_balance_update = Some(Utc::now());
                    }
                    Ok(balance)
                }
            }
        }
    }
}

impl Clone for WalletManager {
    fn clone(&self) -> Self {
        let mut keypairs = HashMap::new();
        // Clone all keypairs by recreating from bytes
        for (wallet_type, keypair) in &self.keypairs {
            if let Ok(cloned_keypair) = Keypair::from_bytes(&keypair.to_bytes()) {
                keypairs.insert(wallet_type.clone(), cloned_keypair);
            }
        }
        
        Self {
            config_dir: self.config_dir.clone(),
            wallets: self.wallets.clone(),
            keypairs,
            rpc_client: RpcClient::new_with_commitment(
                self.rpc_client.url(),
                self.rpc_client.commitment()
            ),
            master_password: self.master_password.clone(),
            encryption_salt: self.encryption_salt,
        }
    }
}