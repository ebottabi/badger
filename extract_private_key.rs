/// Utility to extract private key from Badger wallet for Phantom import
/// 
/// This tool decrypts your wallet and shows the private key in formats
/// compatible with Phantom and other Solana wallets.

use std::fs;
use serde_json;
use base64;
use solana_sdk::signature::Keypair;
use bs58;

#[derive(serde::Deserialize)]
struct WalletConfig {
    pub wallet_type: String,
    pub public_key: String,
    pub encrypted_private_key: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔑 Badger Wallet Private Key Extractor");
    println!("=====================================");
    
    // Read master password
    let master_password = fs::read_to_string("wallets/master.key")?;
    let master_password = master_password.trim();
    println!("📖 Loaded master password");
    
    // Read encryption salt
    let salt_bytes = fs::read("wallets/wallet.salt")?;
    if salt_bytes.len() != 32 {
        return Err("Invalid salt file size".into());
    }
    println!("🧂 Loaded encryption salt");
    
    // Process both wallet types
    for wallet_file in ["trading_wallet.json", "cold_wallet.json"] {
        let wallet_path = format!("wallets/{}", wallet_file);
        
        if !std::path::Path::new(&wallet_path).exists() {
            println!("⏭️  Skipping {} (not found)", wallet_file);
            continue;
        }
        
        println!("\n🔍 Processing {}:", wallet_file);
        
        // Read wallet config
        let config_data = fs::read_to_string(&wallet_path)?;
        let config: WalletConfig = serde_json::from_str(&config_data)?;
        
        // Decrypt private key
        let decrypted_key = decrypt_private_key(&config.encrypted_private_key, master_password, &salt_bytes)?;
        
        // Create keypair
        let keypair = Keypair::from_bytes(&decrypted_key)?;
        
        // Verify public key matches
        if keypair.pubkey().to_string() != config.public_key {
            return Err(format!("Public key mismatch for {}", wallet_file).into());
        }
        
        println!("✅ Wallet Type: {}", config.wallet_type);
        println!("📍 Public Key: {}", config.public_key);
        
        // Export in different formats
        println!("\n📤 EXPORT FORMATS:");
        
        // 1. Base58 Private Key (most common)
        let private_key_base58 = bs58::encode(&decrypted_key).into_string();
        println!("🔐 Private Key (Base58): {}", private_key_base58);
        
        // 2. Byte Array (for some wallets)
        println!("🔢 Private Key (Bytes): {:?}", decrypted_key);
        
        // 3. Hex format (alternative)
        let private_key_hex = hex::encode(&decrypted_key);
        println!("🔤 Private Key (Hex): {}", private_key_hex);
        
        println!("\n🦄 FOR PHANTOM WALLET:");
        println!("1. Open Phantom → Add/Import Wallet → Import Private Key");
        println!("2. Paste this Base58 key: {}", private_key_base58);
        println!("3. Phantom will import and show balance");
        
        println!("═══════════════════════════════════════");
    }
    
    println!("\n⚠️  SECURITY WARNING:");
    println!("• Never share private keys with anyone");
    println!("• Delete this output after importing to Phantom");
    println!("• Private keys = full control of wallet funds");
    
    Ok(())
}

fn decrypt_private_key(encrypted_key: &str, master_password: &str, salt: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Simple key derivation (matches wallet_management.rs)
    let mut key = [0u8; 64];
    derive_key(master_password.as_bytes(), salt, &mut key);
    
    // Decode base64
    let encrypted_bytes = base64::decode(encrypted_key)?;
    
    // Decrypt using XOR
    let mut decrypted = Vec::new();
    for (i, &byte) in encrypted_bytes.iter().enumerate() {
        decrypted.push(byte ^ key[i % key.len()]);
    }
    
    Ok(decrypted)
}

fn derive_key(password: &[u8], salt: &[u8], output: &mut [u8]) {
    // Simple key derivation (matches wallet_management.rs)
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    password.hash(&mut hasher);
    salt.hash(&mut hasher);
    
    let hash = hasher.finish().to_le_bytes();
    for i in 0..output.len() {
        output[i] = hash[i % hash.len()];
    }
}