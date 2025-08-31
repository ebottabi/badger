use std::fs;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔑 Simple Badger Wallet Private Key Extractor");
    println!("=============================================");
    
    // Check if wallets directory exists
    if !std::path::Path::new("wallets").exists() {
        println!("❌ No wallets directory found. Run badger first to create wallets.");
        return Ok(());
    }
    
    // Read master password
    let master_password = match fs::read_to_string("wallets/master.key") {
        Ok(password) => password.trim().to_string(),
        Err(_) => {
            println!("❌ Master key not found. Run badger first to initialize wallets.");
            return Ok(());
        }
    };
    
    // Read encryption salt
    let salt_bytes = match fs::read("wallets/wallet.salt") {
        Ok(bytes) => bytes,
        Err(_) => {
            println!("❌ Encryption salt not found. Run badger first to initialize wallets.");
            return Ok(());
        }
    };
    
    if salt_bytes.len() != 32 {
        println!("❌ Invalid salt file size");
        return Ok(());
    }
    
    println!("📖 Loaded master password and encryption salt");
    
    // Process both wallet types
    let wallet_files = ["trading_wallet.json", "cold_wallet.json"];
    
    for wallet_file in wallet_files {
        let wallet_path = format!("wallets/{}", wallet_file);
        
        if !std::path::Path::new(&wallet_path).exists() {
            println!("⏭️  Skipping {} (not found)", wallet_file);
            continue;
        }
        
        println!("\n🔍 Processing {}:", wallet_file);
        
        // Read and parse wallet config
        let config_data = fs::read_to_string(&wallet_path)?;
        let config: Value = serde_json::from_str(&config_data)?;
        
        let wallet_type = config["wallet_type"].as_str().unwrap_or("Unknown");
        let public_key = config["public_key"].as_str().unwrap_or("");
        let encrypted_private_key = config["encrypted_private_key"].as_str().unwrap_or("");
        
        if encrypted_private_key.is_empty() {
            println!("❌ No encrypted private key found in {}", wallet_file);
            continue;
        }
        
        // Decrypt private key
        match decrypt_private_key(encrypted_private_key, &master_password, &salt_bytes) {
            Ok(decrypted_key) => {
                if decrypted_key.len() != 64 {
                    println!("❌ Invalid private key length for {}", wallet_file);
                    continue;
                }
                
                // Convert to Base58 (Phantom format)
                let private_key_base58 = bs58::encode(&decrypted_key).into_string();
                
                println!("✅ Wallet Type: {}", wallet_type);
                println!("📍 Public Key: {}", public_key);
                println!("");
                println!("🔐 PRIVATE KEY (Base58 - for Phantom):");
                println!("   {}", private_key_base58);
                println!("");
                println!("🔗 View on Solscan: https://solscan.io/account/{}", public_key);
                println!("🔗 View on Solana Explorer: https://explorer.solana.com/address/{}", public_key);
                println!("");
                println!("📱 TO IMPORT INTO PHANTOM:");
                println!("   1. Open Phantom Wallet app");
                println!("   2. Settings → Import Wallet → Private Key");
                println!("   3. Paste this key: {}", private_key_base58);
                println!("   4. Your wallet will appear with your SOL balance!");
                
                println!("═══════════════════════════════════════════════");
            }
            Err(e) => {
                println!("❌ Failed to decrypt private key for {}: {}", wallet_file, e);
            }
        }
    }
    
    println!("\n⚠️  SECURITY WARNING:");
    println!("• NEVER share these private keys with anyone!");
    println!("• Private keys = complete control of your funds");
    println!("• Only paste them into trusted wallets like Phantom");
    println!("• Clear your terminal history: history -c");
    
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
    // Simple key derivation (matches wallet_management.rs exactly)
    let mut hasher = DefaultHasher::new();
    password.hash(&mut hasher);
    salt.hash(&mut hasher);
    
    let hash = hasher.finish().to_le_bytes();
    for i in 0..output.len() {
        output[i] = hash[i % hash.len()];
    }
}