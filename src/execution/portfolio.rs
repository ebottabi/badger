/// File-based portfolio tracking system

use std::fs;
use std::path::Path;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioEntry {
    pub mint: String,
    pub symbol: String,
    pub entry_price_sol: f64,
    pub entry_time: DateTime<Utc>,
    pub tokens_held: f64,
    pub sol_invested: f64,
    pub status: String, // "active", "partial_exit", "closed"
    pub transactions: Vec<Transaction>,
    pub entry_usd: f64,
    pub current_value_usd: f64,
    pub profit_loss_usd: f64,
    pub sol_to_usd_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub tx_type: String, // "buy", "sell"
    pub amount: f64,
    pub price_sol: f64,
    pub timestamp: DateTime<Utc>,
    pub tx_id: String,
    pub amount_usd: f64,
    pub sol_to_usd_rate: f64,
}

pub struct PortfolioTracker {
    file_path: String,
    positions: HashMap<String, PortfolioEntry>,
}

impl PortfolioTracker {
    pub fn new(file_path: &str) -> Self {
        let mut tracker = Self {
            file_path: file_path.to_string(),
            positions: HashMap::new(),
        };
        tracker.load_from_file().unwrap_or_default();
        tracker
    }
    
    pub fn add_buy(&mut self, mint: &str, symbol: &str, tokens: f64, sol_amount: f64, price: f64, tx_id: &str, sol_to_usd_rate: f64) -> Result<()> {
        let transaction = Transaction {
            tx_type: "buy".to_string(),
            amount: tokens,
            price_sol: price,
            timestamp: Utc::now(),
            tx_id: tx_id.to_string(),
            amount_usd: sol_amount * sol_to_usd_rate,
            sol_to_usd_rate,
        };
        
        if let Some(entry) = self.positions.get_mut(mint) {
            // Update existing position
            let prev_value = entry.tokens_held * entry.entry_price_sol;
            let new_value = tokens * price;
            entry.tokens_held += tokens;
            entry.sol_invested += sol_amount;
            entry.entry_price_sol = (prev_value + new_value) / entry.tokens_held;
            entry.entry_usd += sol_amount * sol_to_usd_rate;
            entry.transactions.push(transaction);
        } else {
            // Create new position
            let entry_usd = sol_amount * sol_to_usd_rate;
            let entry = PortfolioEntry {
                mint: mint.to_string(),
                symbol: symbol.to_string(),
                entry_price_sol: price,
                entry_time: Utc::now(),
                tokens_held: tokens,
                sol_invested: sol_amount,
                status: "active".to_string(),
                transactions: vec![transaction],
                entry_usd,
                current_value_usd: entry_usd,
                profit_loss_usd: 0.0,
                sol_to_usd_rate,
            };
            self.positions.insert(mint.to_string(), entry);
        }
        
        self.save_to_file()
    }
    
    pub fn add_sell(&mut self, mint: &str, tokens_sold: f64, sol_received: f64, price_usd: f64, tx_id: &str, sol_to_usd_rate: f64) -> Result<()> {
        if let Some(entry) = self.positions.get_mut(mint) {
            let transaction = Transaction {
                tx_type: "sell".to_string(),
                amount: tokens_sold,
                price_sol: price_usd / sol_to_usd_rate, // Convert USD price to SOL price for consistency
                timestamp: Utc::now(),
                tx_id: tx_id.to_string(),
                amount_usd: tokens_sold * price_usd, // Direct USD calculation
                sol_to_usd_rate,
            };
            
            entry.tokens_held -= tokens_sold;
            entry.transactions.push(transaction);
            
            // Update status
            entry.status = if entry.tokens_held <= 0.0 {
                "closed".to_string()
            } else {
                "partial_exit".to_string()
            };
        }
        
        self.save_to_file()
    }
    
    pub fn get_position(&self, mint: &str) -> Option<&PortfolioEntry> {
        self.positions.get(mint)
    }
    
    pub fn get_all_active_positions(&self) -> Vec<&PortfolioEntry> {
        self.positions.values()
            .filter(|p| p.status == "active" || p.status == "partial_exit")
            .collect()
    }
    
    fn save_to_file(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.positions)?;
        fs::write(&self.file_path, json)?;
        Ok(())
    }
    
    fn load_from_file(&mut self) -> Result<()> {
        if Path::new(&self.file_path).exists() {
            let content = fs::read_to_string(&self.file_path)?;
            self.positions = serde_json::from_str(&content)?;
        } else {
            // Create empty portfolio file
            if let Some(parent) = Path::new(&self.file_path).parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&self.file_path, "{}")?;
            println!("📁 Created new portfolio file: {}", self.file_path);
        }
        Ok(())
    }
}