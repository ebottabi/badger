/// Position tracking and management

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::fs;
use std::path::Path;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PositionStatus {
    Open,
    PartialExit,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub mint: String,
    pub symbol: String,
    pub entry_price: f64,
    pub entry_time: DateTime<Utc>,
    pub sol_invested: f64,
    pub tokens_held: f64,
    pub peak_price: f64,
    pub current_price: f64,
    pub status: PositionStatus,
    pub entry_usd: f64,
    pub current_value_usd: f64,
    pub profit_loss_usd: f64,
    pub sol_to_usd_rate: f64,
}

impl Position {
    pub fn new(mint: String, symbol: String, entry_price: f64, sol_invested: f64, tokens_held: f64, sol_to_usd_rate: f64) -> Self {
        let now = Utc::now();
        let entry_usd = sol_invested * sol_to_usd_rate;
        let current_value_usd = entry_usd; // Same as entry at creation
        
        Self {
            mint,
            symbol,
            entry_price,
            entry_time: now,
            sol_invested,
            tokens_held,
            peak_price: entry_price,
            current_price: entry_price,
            status: PositionStatus::Open,
            entry_usd,
            current_value_usd,
            profit_loss_usd: 0.0, // No profit/loss at entry
            sol_to_usd_rate,
        }
    }
    
    pub fn update_price(&mut self, new_price: f64) {
        self.current_price = new_price;
        if new_price > self.peak_price {
            self.peak_price = new_price;
        }
        
        // new_price is already in USD from Jupiter API, so tokens_held * new_price gives USD value
        self.current_value_usd = self.tokens_held * new_price;
        self.profit_loss_usd = self.current_value_usd - self.entry_usd;
    }
    
    pub fn get_pnl_percent(&self) -> f64 {
        // Use USD values to avoid price unit confusion
        if self.entry_usd > 0.0 {
            ((self.current_value_usd - self.entry_usd) / self.entry_usd) * 100.0
        } else {
            0.0
        }
    }
    
    pub fn get_current_value_sol(&self) -> f64 {
        self.tokens_held * self.current_price
    }
    
    pub fn get_drawdown_from_peak(&self) -> f64 {
        ((self.peak_price - self.current_price) / self.peak_price) * 100.0
    }
    
    pub fn get_age_hours(&self) -> f64 {
        let now = Utc::now();
        now.signed_duration_since(self.entry_time).num_seconds() as f64 / 3600.0
    }
}

pub struct PositionManager {
    positions: Arc<RwLock<HashMap<String, Position>>>,
    file_path: String,
}

impl PositionManager {
    pub fn new() -> Self {
        let file_path = "data/positions.json".to_string();
        let manager = Self {
            positions: Arc::new(RwLock::new(HashMap::new())),
            file_path,
        };
        
        // Load existing positions from file
        if let Err(e) = manager.load_positions() {
            println!("⚠️ Could not load positions: {}", e);
        }
        
        manager
    }
    
    pub fn add_position(&self, position: Position) {
        println!("🔧 PositionManager::add_position called for {}", position.mint);
        
        let mut positions = self.positions.write().unwrap();
        positions.insert(position.mint.clone(), position.clone());
        println!("📝 Position inserted into memory. Total positions now: {}", positions.len());
        drop(positions); // Release lock before file I/O
        
        println!("💾 Attempting to save to file: {}", self.file_path);
        if let Err(e) = self.save_positions() {
            println!("❌ CRITICAL: Failed to save positions: {}", e);
            println!("🚨 DATA LOSS RISK: Position only exists in memory!");
        } else {
            println!("✅ Positions successfully saved to disk");
            
            // Verify the save by reading back
            match self.load_and_verify() {
                Ok(count) => println!("✅ Verification: {} positions confirmed on disk", count),
                Err(e) => println!("❌ Verification failed: {}", e),
            }
        }
    }
    
    pub fn get_position(&self, mint: &str) -> Option<Position> {
        let positions = self.positions.read().unwrap();
        positions.get(mint).cloned()
    }
    
    pub fn update_price(&self, mint: &str, new_price: f64) -> bool {
        let mut positions = self.positions.write().unwrap();
        if let Some(position) = positions.get_mut(mint) {
            position.update_price(new_price);
            drop(positions); // Release lock before file I/O
            
            if let Err(e) = self.save_positions() {
                println!("⚠️ Failed to save positions: {}", e);
            }
            true
        } else {
            false
        }
    }
    
    pub fn has_position(&self, mint: &str) -> bool {
        let positions = self.positions.read().unwrap();
        positions.contains_key(mint)
    }
    
    pub fn get_all_positions(&self) -> Vec<Position> {
        let positions = self.positions.read().unwrap();
        positions.values().cloned().collect()
    }
    
    pub fn get_open_positions(&self) -> Vec<Position> {
        let positions = self.positions.read().unwrap();
        positions.values()
            .filter(|p| matches!(p.status, PositionStatus::Open | PositionStatus::PartialExit))
            .cloned()
            .collect()
    }
    
    pub fn close_position(&self, mint: &str) {
        let mut positions = self.positions.write().unwrap();
        if let Some(position) = positions.get_mut(mint) {
            position.status = PositionStatus::Closed;
        }
        drop(positions); // Release lock before file I/O
        
        if let Err(e) = self.save_positions() {
            println!("⚠️ Failed to save positions: {}", e);
        }
    }
    
    pub fn get_total_invested(&self) -> f64 {
        let positions = self.positions.read().unwrap();
        positions.values()
            .filter(|p| p.status == PositionStatus::Open)  // Only count open positions
            .map(|p| p.sol_invested)
            .sum()
    }
    
    fn save_positions(&self) -> Result<()> {
        let positions = self.positions.read().unwrap();
        
        println!("🔍 save_positions: Attempting to save {} positions", positions.len());
        
        // Create data directory if it doesn't exist
        if let Some(parent) = Path::new(&self.file_path).parent() {
            println!("📁 Creating directory: {:?}", parent);
            fs::create_dir_all(parent)?;
        }
        
        // Convert positions to JSON
        let json_data = serde_json::to_string_pretty(&*positions)?;
        println!("📄 Generated JSON data ({} bytes)", json_data.len());
        
        println!("✏️ Writing to file: {}", self.file_path);
        fs::write(&self.file_path, &json_data)?;
        
        println!("🔍 File write completed. Checking file size...");
        if let Ok(metadata) = fs::metadata(&self.file_path) {
            println!("📏 File size on disk: {} bytes", metadata.len());
        }
        
        Ok(())
    }
    
    fn load_and_verify(&self) -> Result<usize> {
        let json_data = fs::read_to_string(&self.file_path)?;
        let loaded_positions: HashMap<String, Position> = serde_json::from_str(&json_data)?;
        Ok(loaded_positions.len())
    }
    
    fn load_positions(&self) -> Result<()> {
        if !Path::new(&self.file_path).exists() {
            // Create empty positions file
            if let Some(parent) = Path::new(&self.file_path).parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&self.file_path, "{}")?;
            println!("📁 Created new positions file: {}", self.file_path);
            return Ok(()); 
        }
        
        let json_data = fs::read_to_string(&self.file_path)?;
        let loaded_positions: HashMap<String, Position> = serde_json::from_str(&json_data)?;
        
        let mut positions = self.positions.write().unwrap();
        *positions = loaded_positions;
        
        let open_count = positions.values()
            .filter(|p| matches!(p.status, PositionStatus::Open | PositionStatus::PartialExit))
            .count();
        let closed_count = positions.len() - open_count;
        
        println!("📂 Loaded {} total positions from file ({} open, {} closed)", 
                positions.len(), open_count, closed_count);
        
        Ok(())
    }
}