/// Fast hashing utilities for memory-mapped database
/// 
/// This module provides optimized hashing functions for Solana addresses
/// and other data types used in high-frequency trading decisions.

use xxhash_rust::xxh64::xxh64;

/// Fast hash function for Solana addresses
#[inline(always)]
pub fn hash_solana_address(address: &[u8; 32]) -> u64 {
    xxh64(address, 0)
}

/// Fast hash function with custom seed
#[inline(always)]
pub fn hash_with_seed(data: &[u8], seed: u64) -> u64 {
    xxh64(data, seed)
}

/// Convert string address to bytes if valid
pub fn parse_solana_address(address: &str) -> Option<[u8; 32]> {
    match bs58::decode(address).into_vec() {
        Ok(bytes) if bytes.len() == 32 => {
            let mut addr = [0u8; 32];
            addr.copy_from_slice(&bytes);
            Some(addr)
        }
        _ => None,
    }
}

/// Convert bytes to base58 string
pub fn bytes_to_address(bytes: &[u8; 32]) -> String {
    bs58::encode(bytes).into_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_address_parsing() {
        let test_address = "11111111111111111111111111111112"; // Valid base58
        let parsed = parse_solana_address(test_address);
        assert!(parsed.is_some());
        
        let bytes = parsed.unwrap();
        let back_to_string = bytes_to_address(&bytes);
        assert_eq!(test_address, back_to_string);
    }
    
    #[test]
    fn test_hash_consistency() {
        let address = [42u8; 32];
        let hash1 = hash_solana_address(&address);
        let hash2 = hash_solana_address(&address);
        assert_eq!(hash1, hash2);
        
        let hash_with_seed = hash_with_seed(&address, 123);
        assert_ne!(hash1, hash_with_seed); // Different seeds should give different hashes
    }
}