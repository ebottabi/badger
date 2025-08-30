use anyhow::{Result, Context, bail};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    signature::{Keypair, Signature, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
};
use tracing::{info, debug, warn, error, instrument};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Wallet configuration for secure key management
#[derive(Debug, Clone)]
pub struct WalletConfig {
    /// Path to wallet keypair file (JSON format)
    pub keypair_path: Option<String>,
    /// Environment variable name containing private key
    pub private_key_env: Option<String>,
    /// Maximum transaction value in lamports (safety limit)
    pub max_transaction_value_lamports: u64,
    /// Whether to require manual approval for high-value transactions
    pub require_approval_for_large_transactions: bool,
    /// Approval threshold in lamports
    pub approval_threshold_lamports: u64,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            keypair_path: None,
            private_key_env: Some("SOLANA_PRIVATE_KEY".to_string()),
            max_transaction_value_lamports: 1_000_000_000, // 1 SOL
            require_approval_for_large_transactions: true,
            approval_threshold_lamports: 100_000_000, // 0.1 SOL
        }
    }
}

/// Transaction signing request with security validation
#[derive(Debug, Clone)]
pub struct SigningRequest {
    /// Transaction to sign
    pub transaction: Transaction,
    /// Estimated value being transferred (in lamports)
    pub estimated_value_lamports: u64,
    /// Description of the transaction for approval
    pub description: String,
    /// Whether this is a high-priority transaction
    pub is_priority: bool,
}

/// Result of transaction signing operation
#[derive(Debug, Clone)]
pub struct SigningResult {
    /// Signed transaction
    pub signed_transaction: Transaction,
    /// Transaction signature
    pub signature: Signature,
    /// Wallet public key used for signing
    pub signer_pubkey: Pubkey,
    /// Whether approval was required and granted
    pub approval_granted: bool,
}

/// Secure wallet manager with safety controls
pub struct WalletManager {
    /// Primary wallet keypair
    keypair: Keypair,
    /// Wallet configuration
    config: WalletConfig,
    /// Transaction history for audit
    transaction_history: Vec<TransactionRecord>,
    /// Approval callback for high-value transactions
    approval_callback: Option<Box<dyn Fn(&SigningRequest) -> bool + Send + Sync>>,
}

/// Transaction record for audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    /// Transaction signature
    pub signature: String,
    /// Timestamp when signed
    pub timestamp: i64,
    /// Transaction value in lamports
    pub value_lamports: u64,
    /// Transaction description
    pub description: String,
    /// Wallet public key used
    pub signer_pubkey: String,
    /// Whether approval was required
    pub required_approval: bool,
}

impl WalletManager {
    /// Creates a new wallet manager with the given configuration
    /// 
    /// # Arguments
    /// * `config` - Wallet configuration
    /// 
    /// # Returns
    /// * `Result<Self>` - Wallet manager instance
    #[instrument]
    pub fn new(config: WalletConfig) -> Result<Self> {
        info!("Initializing secure wallet manager");
        
        // Load keypair from configuration
        let keypair = Self::load_keypair(&config)?;
        
        info!(
            pubkey = %keypair.pubkey(),
            max_transaction_value = config.max_transaction_value_lamports,
            approval_threshold = config.approval_threshold_lamports,
            "Wallet manager initialized successfully"
        );
        
        Ok(Self {
            keypair,
            config,
            transaction_history: Vec::new(),
            approval_callback: None,
        })
    }
    
    /// Loads keypair from configuration (file or environment variable)
    /// 
    /// # Arguments
    /// * `config` - Wallet configuration
    /// 
    /// # Returns
    /// * `Result<Keypair>` - Loaded keypair
    #[instrument]
    fn load_keypair(config: &WalletConfig) -> Result<Keypair> {
        // Try loading from file first
        if let Some(keypair_path) = &config.keypair_path {
            debug!(path = %keypair_path, "Loading keypair from file");
            
            if Path::new(keypair_path).exists() {
                let keypair_bytes = fs::read(keypair_path)
                    .with_context(|| format!("Failed to read keypair file: {}", keypair_path))?;
                
                // Try parsing as JSON array format
                if let Ok(json_bytes) = serde_json::from_slice::<Vec<u8>>(&keypair_bytes) {
                    if json_bytes.len() == 64 {
                        let keypair = Keypair::from_bytes(&json_bytes)
                            .context("Failed to create keypair from JSON bytes")?;
                        
                        info!("Keypair loaded successfully from file");
                        return Ok(keypair);
                    }
                }
                
                // Try parsing as raw bytes
                if keypair_bytes.len() == 64 {
                    let keypair = Keypair::from_bytes(&keypair_bytes)
                        .context("Failed to create keypair from raw bytes")?;
                    
                    info!("Keypair loaded successfully from file (raw format)");
                    return Ok(keypair);
                }
                
                bail!("Invalid keypair file format. Expected 64 bytes or JSON array format.");
            } else {
                warn!(path = %keypair_path, "Keypair file not found, trying environment variable");
            }
        }
        
        // Try loading from environment variable
        if let Some(env_var) = &config.private_key_env {
            debug!(env_var = %env_var, "Loading keypair from environment variable");
            
            if let Ok(private_key_str) = std::env::var(env_var) {
                // Try parsing as base58 (Solana CLI format)
                if let Ok(bytes) = bs58::decode(&private_key_str).into_vec() {
                    if bytes.len() == 64 {
                        let keypair = Keypair::from_bytes(&bytes)
                            .context("Failed to create keypair from base58 string")?;
                        
                        info!("Keypair loaded successfully from environment variable");
                        return Ok(keypair);
                    }
                }
                
                // Try parsing as JSON array
                if let Ok(json_bytes) = serde_json::from_str::<Vec<u8>>(&private_key_str) {
                    if json_bytes.len() == 64 {
                        let keypair = Keypair::from_bytes(&json_bytes)
                            .context("Failed to create keypair from JSON string")?;
                        
                        info!("Keypair loaded successfully from environment variable (JSON format)");
                        return Ok(keypair);
                    }
                }
                
                bail!("Invalid private key format in environment variable. Expected base58 or JSON array.");
            } else {
                warn!(env_var = %env_var, "Environment variable not found");
            }
        }
        
        // If no keypair source is configured, generate a new one (for development only)
        warn!("No keypair source configured, generating new keypair (DEVELOPMENT ONLY)");
        let keypair = Keypair::new();
        info!(pubkey = %keypair.pubkey(), "Generated new keypair");
        
        Ok(keypair)
    }
    
    /// Signs a transaction with security validation and approval workflow
    /// 
    /// # Arguments
    /// * `signing_request` - Transaction signing request
    /// 
    /// # Returns
    /// * `Result<SigningResult>` - Signing result with security information
    #[instrument(skip(self))]
    pub async fn sign_transaction(&mut self, signing_request: SigningRequest) -> Result<SigningResult> {
        info!(
            value_lamports = signing_request.estimated_value_lamports,
            description = %signing_request.description,
            is_priority = signing_request.is_priority,
            "Processing transaction signing request"
        );
        
        // Security validation
        self.validate_transaction(&signing_request)?;
        
        // Check if approval is required
        let requires_approval = self.requires_approval(&signing_request);
        let mut approval_granted = true;
        
        if requires_approval {
            info!(
                value_lamports = signing_request.estimated_value_lamports,
                threshold = self.config.approval_threshold_lamports,
                "Transaction requires approval due to high value"
            );
            
            approval_granted = self.request_approval(&signing_request).await?;
            
            if !approval_granted {
                bail!("Transaction approval denied");
            }
            
            info!("Transaction approval granted");
        }
        
        // Sign the transaction
        let mut transaction = signing_request.transaction;
        let signature = transaction.signatures[0]; // Will be updated after signing
        
        transaction.partial_sign(&[&self.keypair], transaction.message.recent_blockhash);
        let actual_signature = transaction.signatures[0];
        
        // Record transaction for audit
        let transaction_record = TransactionRecord {
            signature: actual_signature.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            value_lamports: signing_request.estimated_value_lamports,
            description: signing_request.description.clone(),
            signer_pubkey: self.keypair.pubkey().to_string(),
            required_approval: requires_approval,
        };
        
        self.transaction_history.push(transaction_record);
        
        // Trim history to last 1000 transactions
        if self.transaction_history.len() > 1000 {
            self.transaction_history.remove(0);
        }
        
        let result = SigningResult {
            signed_transaction: transaction,
            signature: actual_signature,
            signer_pubkey: self.keypair.pubkey(),
            approval_granted,
        };
        
        info!(
            signature = %result.signature,
            signer_pubkey = %result.signer_pubkey,
            approval_granted = result.approval_granted,
            "Transaction signed successfully"
        );
        
        Ok(result)
    }
    
    /// Validates transaction security constraints
    /// 
    /// # Arguments
    /// * `signing_request` - Transaction to validate
    /// 
    /// # Returns
    /// * `Result<()>` - Ok if validation passes
    fn validate_transaction(&self, signing_request: &SigningRequest) -> Result<()> {
        // Check maximum transaction value
        if signing_request.estimated_value_lamports > self.config.max_transaction_value_lamports {
            bail!(
                "Transaction value {} lamports exceeds maximum allowed {} lamports",
                signing_request.estimated_value_lamports,
                self.config.max_transaction_value_lamports
            );
        }
        
        // Validate transaction structure
        if signing_request.transaction.message.instructions.is_empty() {
            bail!("Transaction has no instructions");
        }
        
        // Check that our wallet is a signer
        let our_pubkey = self.keypair.pubkey();
        let is_signer = signing_request.transaction.message.account_keys
            .iter()
            .enumerate()
            .any(|(i, pubkey)| {
                *pubkey == our_pubkey && 
                signing_request.transaction.message.header.num_required_signatures as usize > i
            });
        
        if !is_signer {
            bail!("Wallet is not required as a signer for this transaction");
        }
        
        debug!("Transaction validation passed");
        Ok(())
    }
    
    /// Determines if transaction requires manual approval
    /// 
    /// # Arguments
    /// * `signing_request` - Transaction to check
    /// 
    /// # Returns
    /// * `bool` - True if approval is required
    fn requires_approval(&self, signing_request: &SigningRequest) -> bool {
        if !self.config.require_approval_for_large_transactions {
            return false;
        }
        
        signing_request.estimated_value_lamports >= self.config.approval_threshold_lamports
    }
    
    /// Requests approval for high-value transactions
    /// 
    /// # Arguments
    /// * `signing_request` - Transaction requiring approval
    /// 
    /// # Returns
    /// * `Result<bool>` - True if approval is granted
    async fn request_approval(&self, signing_request: &SigningRequest) -> Result<bool> {
        if let Some(callback) = &self.approval_callback {
            let approved = callback(signing_request);
            info!(approved = approved, "Approval callback result");
            return Ok(approved);
        }
        
        // Default approval logic (for production, this should be replaced with actual approval mechanism)
        warn!("No approval callback configured, defaulting to auto-approval (NOT SAFE FOR PRODUCTION)");
        
        // In production, this should connect to:
        // - Hardware wallet confirmation
        // - Multi-signature approval system
        // - Manual operator approval interface
        // - Risk management system
        
        Ok(true) // Auto-approve for development
    }
    
    /// Sets approval callback for high-value transactions
    /// 
    /// # Arguments
    /// * `callback` - Approval callback function
    pub fn set_approval_callback<F>(&mut self, callback: F) 
    where
        F: Fn(&SigningRequest) -> bool + Send + Sync + 'static,
    {
        self.approval_callback = Some(Box::new(callback));
        info!("Approval callback configured");
    }
    
    /// Gets wallet public key
    /// 
    /// # Returns
    /// * `Pubkey` - Wallet public key
    pub fn pubkey(&self) -> Pubkey {
        self.keypair.pubkey()
    }
    
    /// Gets transaction history for audit
    /// 
    /// # Returns
    /// * `&[TransactionRecord]` - Transaction history
    pub fn get_transaction_history(&self) -> &[TransactionRecord] {
        &self.transaction_history
    }
    
    /// Gets wallet statistics
    /// 
    /// # Returns
    /// * `WalletStats` - Wallet usage statistics
    pub fn get_wallet_stats(&self) -> WalletStats {
        let total_transactions = self.transaction_history.len();
        let total_value: u64 = self.transaction_history
            .iter()
            .map(|record| record.value_lamports)
            .sum();
        
        let high_value_transactions = self.transaction_history
            .iter()
            .filter(|record| record.value_lamports >= self.config.approval_threshold_lamports)
            .count();
        
        let transactions_requiring_approval = self.transaction_history
            .iter()
            .filter(|record| record.required_approval)
            .count();
        
        WalletStats {
            wallet_pubkey: self.keypair.pubkey(),
            total_transactions,
            total_value_lamports: total_value,
            high_value_transactions,
            transactions_requiring_approval,
            max_transaction_value_lamports: self.config.max_transaction_value_lamports,
            approval_threshold_lamports: self.config.approval_threshold_lamports,
        }
    }
}

/// Wallet usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletStats {
    /// Wallet public key
    pub wallet_pubkey: Pubkey,
    /// Total number of transactions signed
    pub total_transactions: usize,
    /// Total value of all transactions in lamports
    pub total_value_lamports: u64,
    /// Number of high-value transactions
    pub high_value_transactions: usize,
    /// Number of transactions that required approval
    pub transactions_requiring_approval: usize,
    /// Maximum allowed transaction value
    pub max_transaction_value_lamports: u64,
    /// Approval threshold
    pub approval_threshold_lamports: u64,
}

impl WalletStats {
    /// Gets average transaction value
    pub fn average_transaction_value(&self) -> f64 {
        if self.total_transactions > 0 {
            self.total_value_lamports as f64 / self.total_transactions as f64
        } else {
            0.0
        }
    }
    
    /// Gets percentage of transactions requiring approval
    pub fn approval_rate_percent(&self) -> f64 {
        if self.total_transactions > 0 {
            (self.transactions_requiring_approval as f64 / self.total_transactions as f64) * 100.0
        } else {
            0.0
        }
    }
}