/// Display utilities for terminal output

use colored::Colorize;
use chrono::{DateTime, Utc};
use crate::client::event_parser::UniversalPumpEvent;
use crate::algo::trend_analysis::{TrendAnalysis, TrendStrength};
use crate::util::criteria::InstantBuyCriteria;
use crate::util::display_helpers::{print_detailed_token_info, print_comprehensive_links, print_quick_links};

pub fn print_instant_buy_signal(event: &UniversalPumpEvent, criteria: &InstantBuyCriteria) {
    println!("\n{} {}", "🚨", "INSTANT BUY SIGNAL".bold());
    println!("{}", "=".repeat(70));
    print_detailed_token_info(event);
    
    // Additional buy signal analysis
    println!("\n{}", "📈 BUY SIGNAL ANALYSIS:".bold());
    println!("   Market Cap: {} (meets minimum {} SOL threshold)", 
        format!("{:.2} SOL", event.market_cap_sol.unwrap_or(0.0)),
        criteria.min_market_cap_sol);
    println!("   Initial Buy: {} (meets minimum {} SOL threshold)", 
        format!("{:.4} SOL", event.sol_amount.unwrap_or(0.0)),
        criteria.min_initial_buy_sol);
    println!("   Token Age: Less than {} minutes (fresh token)", 
        criteria.max_token_age_minutes);
    println!("   Safety Check: Passed basic scam detection filters");
    
    // Potential metrics
    let total_supply = 1_000_000_000.0;
    let initial_tokens = event.initial_buy.unwrap_or_else(|| 
        (event.sol_amount.unwrap_or(0.0) / event.market_cap_sol.unwrap_or(1.0)) * total_supply
    );
    let creator_percentage = (initial_tokens / total_supply) * 100.0;
    
    println!("   Creator Holdings: {:.1}% of total supply", creator_percentage);
    
    if let Some(bonding_curve) = event.v_sol_in_bonding_curve {
        let bonding_progress = 100.0 - ((bonding_curve / 85.0) * 100.0).min(100.0);
        println!("   Bonding Progress: {:.1}% completed", bonding_progress);
    }
    
    print_comprehensive_links(event);
    
    println!("\n{} {} {}", "⚡", "RECOMMENDATION:", "CONSIDER IMMEDIATE BUY - Strong fundamentals detected!".bold());
    println!("{}", "=".repeat(70));
}

pub fn print_whale_buy_signal(event: &UniversalPumpEvent) {
    println!("\n{} {} - {}", 
        "🐋", 
        "WHALE BUY DETECTED".bold(),
        event.symbol.bold()
    );
    println!("   Amount: {} SOL", 
        format!("{:.4}", event.sol_amount.unwrap_or(0.0))
    );
    println!("   Trader: {}", event.trader_public_key);
    print_quick_links(&event.mint, &event.trader_public_key, event.signature.as_deref());
}