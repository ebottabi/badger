/// Additional display helper functions

use colored::Colorize;
use chrono::{DateTime, Utc};
use crate::client::event_parser::UniversalPumpEvent;

pub fn print_detailed_token_info(event: &UniversalPumpEvent) {
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
    
    // Additional details for instant buy signals
    if let Some(uri) = &event.uri {
        if uri.contains("ipfs") {
            println!("{} {}", "🔗 Metadata:", uri);
        }
    }
}

pub fn print_comprehensive_links(event: &UniversalPumpEvent) {
    println!("\n{}", "🔗 COMPREHENSIVE LINKS:".bold());
    
    // Primary trading/analysis links
    println!("   {} {}", "📊 DEX Screener:", 
        format!("https://dexscreener.com/solana/{}", event.mint));
    println!("   {} {}", "🦅 Birdeye:", 
        format!("https://birdeye.so/token/{}?chain=solana", event.mint));
    println!("   {} {}", "🚀 Pump.fun:", 
        format!("https://pump.fun/{}", event.mint));
    
    // Blockchain explorers
    println!("   {} {}", "🟪 Solscan Token:", 
        format!("https://solscan.io/token/{}", event.mint));
    println!("   {} {}", "👤 Creator Profile:", 
        format!("https://solscan.io/account/{}", event.trader_public_key));
    
    // Additional tools
    println!("   {} {}", "🔍 Token Analysis:", 
        format!("https://rugcheck.xyz/tokens/{}", event.mint));
    println!("   {} {}", "📋 Token Info:", 
        format!("https://solanatracker.io/token/{}", event.mint));
    
    if let Some(signature) = &event.signature {
        println!("   {} {}", "📄 Creation Tx:", 
            format!("https://solscan.io/tx/{}", signature));
        println!("   {} {}", "🔗 Solana Explorer:", 
            format!("https://explorer.solana.com/tx/{}", signature));
    }
    
    // Copy-paste friendly mint address
    println!("\n{} {}", "📋 Mint Address (copy):", event.mint.bold());
}

pub fn print_quick_links(mint: &str, trader: &str, signature: Option<&str>) {
    println!("   🔗 Token: {}", 
        format!("https://dexscreener.com/solana/{}", mint));
    println!("   👤 Trader: {}", 
        format!("https://solscan.io/account/{}", trader));
    
    if let Some(sig) = signature {
        println!("   📄 Tx: {}", 
            format!("https://solscan.io/tx/{}", sig));
    }
}