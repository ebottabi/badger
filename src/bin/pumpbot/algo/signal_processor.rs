/// Signal processing for instant buy/sell decisions

use chrono::{Utc, Duration as ChronoDuration};
use colored::Colorize;
use crate::client::event_parser::UniversalPumpEvent;
use crate::util::criteria::InstantBuyCriteria;
use crate::util::display::{print_instant_buy_signal, print_whale_buy_signal};
use crate::util::time_series::TimeSeriesPoint;

pub struct SignalProcessor {
    criteria: InstantBuyCriteria,
}

impl SignalProcessor {
    pub fn new() -> Self {
        Self {
            criteria: InstantBuyCriteria::default(),
        }
    }
    
    pub async fn process_instant_signals(&self, event: &UniversalPumpEvent) {
        let token_age = self.calculate_token_age(event);
        
        // Age filter: Only consider tokens <5 minutes old
        if token_age.num_minutes() > self.criteria.max_token_age_minutes {
            return;
        }
        
        match event.tx_type.as_str() {
            "create" => {
                if self.criteria.is_instant_buy_signal(event) {
                    print_instant_buy_signal(event, &self.criteria);
                } else {
                   // self.print_token_creation(event);
                }
            }
            "buy" => {
                if self.criteria.is_whale_buy(event) {
                    print_whale_buy_signal(event);
                } else {
                    self.print_buy_event(event);
                }
            }
            "sell" => {
                self.print_sell_event(event);
            }
            _ => {
                self.print_other_event(event);
            }
        }
    }
    
    pub fn create_time_series_point(&self, event: &UniversalPumpEvent) -> TimeSeriesPoint {
        let v_sol_curve = event.v_sol_in_bonding_curve.unwrap_or(0.0);
        let v_tokens_curve = event.v_tokens_in_bonding_curve.unwrap_or(0.0);
        
        // Calculate bonding curve progress (typical max is 85 SOL for pump.fun)
        let bonding_progress = if v_sol_curve > 0.0 {
            ((85.0 - v_sol_curve) / 85.0) * 100.0
        } else {
            0.0
        }.max(0.0).min(100.0);
        
        TimeSeriesPoint {
            timestamp: Utc::now(),
            price_sol: Self::calculate_token_price_static(event),
            volume_sol: event.sol_amount.unwrap_or(0.0),
            market_cap_sol: event.market_cap_sol.unwrap_or(0.0),
            tx_type: event.tx_type.clone(),
            trader: event.trader_public_key.clone(),
            bonding_curve_progress: bonding_progress,
            v_sol_in_bonding_curve: v_sol_curve,
            v_tokens_in_bonding_curve: v_tokens_curve,
            holder_count: None, // Will be enhanced later with RPC calls
            initial_buy: event.initial_buy,
        }
    }
    
    fn calculate_token_price_static(event: &UniversalPumpEvent) -> f64 {
        let market_cap = event.market_cap_sol.unwrap_or(0.0);
        let total_supply = 1_000_000_000.0; // 1B tokens typical
        
        if total_supply > 0.0 {
            market_cap / total_supply
        } else {
            0.0
        }
    }
    
    fn calculate_token_age(&self, _event: &UniversalPumpEvent) -> ChronoDuration {
        // For new tokens, age is essentially 0
        ChronoDuration::seconds(0)
    }
    
    pub fn print_token_creation(&self, event: &UniversalPumpEvent) {
        println!("\n{} {}", "🆕", "NEW TOKEN CREATED".bold());
        println!("{}", "=".repeat(70));
        self.print_token_details(event);
        self.print_public_links(&event.mint, event.signature.as_deref());
        println!("{}", "=".repeat(70));
    }
    
    pub fn print_buy_event(&self, event: &UniversalPumpEvent) {
        println!("\n{} {} {}", 
            "💸",
            "BUY".bold(),
            event.symbol.bold()
        );
        self.print_trade_details(event);
    }
    
    pub fn print_sell_event(&self, event: &UniversalPumpEvent) {
        println!("\n{} {} {}", 
            "💸",
            "SELL".bold(),
            event.symbol.bold()
        );
        self.print_trade_details(event);
    }
    
    pub fn print_other_event(&self, event: &UniversalPumpEvent) {
        println!("\n{} {} - {}", 
            "📊",
            event.tx_type.to_uppercase().bold(),
            event.symbol.bold()
        );
        println!("   Mint: {}", event.mint);
        println!("   Pool: {}", event.pool.as_deref().unwrap_or("unknown"));
    }
    
    fn print_token_details(&self, event: &UniversalPumpEvent) {
        use crate::util::display_helpers::print_quick_links;
        
        println!("{} {}", "📛 Name:", event.name.bold());
        println!("{} {}", "🏷️  Symbol:", event.symbol.bold());
        println!("{} {}", "🪙 Mint:", event.mint);
        println!("{} {}", "👨‍💻 Creator:", event.trader_public_key);
        
        if let Some(pool) = &event.pool {
            println!("{} {}", "🏊 Pool:", pool);
        }
        
        let mcap = event.market_cap_sol.unwrap_or(0.0);
        println!("{} {:.2} SOL", "💰 Market Cap:", mcap);
        
        if let Some(sol_amount) = event.sol_amount {
            println!("{} {:.4} SOL", "💵 Initial Buy:", sol_amount);
        }
    }
    
    fn print_trade_details(&self, event: &UniversalPumpEvent) {
        use crate::util::display_helpers::print_quick_links;
        
        if let Some(sol_amount) = event.sol_amount {
            println!("   Amount: {:.4} SOL (${:.2})", 
                sol_amount,
                sol_amount * 180.0
            );
        }
        
        println!("   Trader: {}", event.trader_public_key);
        print_quick_links(&event.mint, &event.trader_public_key, event.signature.as_deref());
    }
    
    fn print_public_links(&self, mint: &str, signature: Option<&str>) {
        println!("\n{}", "🔗 PUBLIC LINKS:".bold());
        println!("   🟪 Solscan: {}", 
            format!("https://solscan.io/token/{}", mint));
        println!("   📊 DEX Screener: {}", 
            format!("https://dexscreener.com/solana/{}", mint));
        println!("   🦅 Birdeye: {}", 
            format!("https://birdeye.so/token/{}?chain=solana", mint));
        println!("   🚀 Pump.fun: {}", 
            format!("https://pump.fun/{}", mint));
        
        if let Some(sig) = signature {
            println!("   📄 Transaction: {}", 
                format!("https://solscan.io/tx/{}", sig));
        }
    }
}