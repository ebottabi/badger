/// Pump.fun WebSocket Data Scraper - Real-time Terminal Output
/// 
/// Connects to PumpPortal's WebSocket API to stream live pump.fun data
/// WebSocket Endpoint: wss://pumpportal.fun/api/data

use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use colored::Colorize;
use std::collections::HashSet;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🚀 Pump.fun WebSocket Monitor Starting...".bright_magenta().bold());
    println!("{}", "📡 Connecting to PumpPortal WebSocket API".cyan());
    
    let mut scraper = PumpWebSocketScraper::new();
    scraper.start_monitoring().await?;
    
    Ok(())
}

struct PumpWebSocketScraper {
    seen_tokens: HashSet<String>,
}

impl PumpWebSocketScraper {
    fn new() -> Self {
        Self {
            seen_tokens: HashSet::new(),
        }
    }
    
    async fn start_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let ws_url = "wss://pumpportal.fun/api/data";
        
        println!("🔗 Connecting to: {}", ws_url.cyan());
        
        let (ws_stream, _) = connect_async(ws_url).await?;
        println!("✅ {}", "WebSocket connected successfully!".green().bold());
        
        let (mut write, mut read) = ws_stream.split();
        
        // Subscribe to different data streams
        self.send_subscriptions(&mut write).await?;
        
        // Listen for real-time messages
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    self.handle_message(&text);
                }
                Ok(Message::Close(_)) => {
                    println!("🔌 {}", "WebSocket connection closed".yellow());
                    break;
                }
                Err(e) => {
                    println!("❌ WebSocket error: {}", e.to_string().red());
                    break;
                }
                _ => {}
            }
        }
        
        Ok(())
    }
    
    async fn send_subscriptions(&self, write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>) -> Result<(), Box<dyn std::error::Error>> {
        let subscriptions = vec![
            // Subscribe to new token creations
            serde_json::json!({
                "method": "subscribeNewToken"
            }),
            // Subscribe to all token trades (use "*" for all tokens)
            serde_json::json!({
                "method": "subscribeTokenTrade", 
                "keys": ["*"]
            }),
            // Subscribe to account trades (optional - can add specific wallets)
            serde_json::json!({
                "method": "subscribeAccountTrade",
                "keys": ["*"]
            }),
        ];
        
        for subscription in subscriptions {
            let message = subscription.to_string();
            write.send(Message::Text(message.clone())).await?;
            println!("📤 Sent subscription: {}", message.dimmed());
        }
        
        println!("\n{}", "🎯 Monitoring for real-time pump.fun data...".bright_green());
        println!("{}", "=".repeat(80).dimmed());
        
        Ok(())
    }
    
    fn handle_message(&mut self, message: &str) {
        // Try to parse as PumpPortal event format
        if let Ok(pump_event) = serde_json::from_str::<PumpPortalEvent>(message) {
            match pump_event.tx_type.as_str() {
                "create" => {
                    if !self.seen_tokens.contains(&pump_event.mint) {
                        self.seen_tokens.insert(pump_event.mint.clone());
                        //self.print_token_creation(&pump_event);
                    }
                }
                "buy" | "sell" => {
                    self.print_trade_event(&pump_event);
                }
                _ => {
                    self.print_other_event(&pump_event);
                }
            }
            return;
        }
        
        // Handle subscription confirmations and other messages
        if message.contains("Successfully subscribed") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(message) {
                if let Some(msg) = json.get("message") {
                    println!("✅ {}", msg.as_str().unwrap_or("").green());
                }
            }
            return;
        }
        
        // If we can't parse it, check if it contains useful data and print raw
        if message.contains("mint") || message.contains("signature") || message.contains("token") {
            self.print_unknown_event(message);
        }
    }
    
    fn print_token_creation(&self, event: &PumpPortalEvent) {
        let now = Utc::now();
        
        println!("\n{} {}", "🆕".bright_green(), "NEW TOKEN CREATED".bright_green().bold());
        println!("{}", "=".repeat(70).green());
        
        println!("{} {}", "📛 Name:".bright_white(), event.name.bright_cyan().bold());
        println!("{} {}", "🏷️  Symbol:".bright_white(), event.symbol.yellow().bold());
        println!("{} {}", "🪙 Mint:".bright_white(), event.mint.blue().underline());
        println!("{} {}", "👨‍💻 Creator:".bright_white(), event.trader_public_key.magenta().underline());
        
        let mcap_sol = event.market_cap_sol.unwrap_or(0.0);
        let mcap_color = if mcap_sol > 100.0 { format!("{:.2} SOL", mcap_sol).green() }
                        else if mcap_sol > 10.0 { format!("{:.2} SOL", mcap_sol).yellow() }
                        else { format!("{:.2} SOL", mcap_sol).red() };
        println!("{} {}", "💰 Market Cap:".bright_white(), mcap_color.bold());
        
        if let Some(initial_buy) = event.initial_buy {
            println!("{} {} tokens", "🛒 Initial Buy:".bright_white(), 
                format!("{:.0}", initial_buy).cyan());
        }
        
        if let Some(sol_amount) = event.sol_amount {
            println!("{} {} SOL", "💵 SOL Amount:".bright_white(), 
                format!("{:.4}", sol_amount).yellow());
        }
        
        if let Some(uri) = &event.uri {
            if uri.contains("ipfs") {
                println!("{} {}", "🔗 IPFS:".bright_white(), uri.blue().underline());
            }
        }
        
        if let Some(signature) = &event.signature {
            println!("{} {}", "📄 Transaction:".bright_white(), signature.magenta());
        }
        
        // Add public links
        println!("\n{}", "🔗 PUBLIC LINKS:".bright_blue().bold());
        println!("{} {}", "   🟪 Solscan:".dimmed(), 
            format!("https://solscan.io/token/{}", event.mint).blue().underline());
        println!("{} {}", "   📊 DEX Screener:".dimmed(), 
            format!("https://dexscreener.com/solana/{}", event.mint).blue().underline());
        println!("{} {}", "   🦅 Birdeye:".dimmed(), 
            format!("https://birdeye.so/token/{}?chain=solana", event.mint).blue().underline());
        println!("{} {}", "   🚀 Pump.fun:".dimmed(), 
            format!("https://pump.fun/{}", event.mint).blue().underline());
        
        if let Some(signature) = &event.signature {
            println!("{} {}", "   📄 Transaction:".dimmed(), 
                format!("https://solscan.io/tx/{}", signature).magenta().underline());
        }
        
        println!("{} {}", "⏰ Detected:".bright_white(), now.format("%H:%M:%S UTC").to_string().cyan());
        
        println!("{}", "=".repeat(70).green());
    }
    
    fn print_trade_event(&self, event: &PumpPortalEvent) {
        let trade_type = if event.tx_type == "buy" { "BUY".green().bold() } else { "SELL".red().bold() };
        let now = Utc::now();
        
        println!("\n{} {} {}", 
            "💸".bright_yellow(),
            trade_type,
            event.symbol.bright_white().bold()
        );
        
        if let Some(sol_amount) = event.sol_amount {
            println!("   {} {} SOL (${:.2})", 
                "Amount:".dimmed(),
                format!("{:.4}", sol_amount).yellow(),
                sol_amount * 180.0 // Rough SOL to USD conversion
            );
        }
        
        println!("   {} {}", "Trader:".dimmed(), event.trader_public_key.cyan().underline());
        println!("   {} {}", "Mint:".dimmed(), event.mint.blue());
        
        // Add quick links for trades
        println!("   {} {}", "🔗 Token:".dimmed(), 
            format!("https://dexscreener.com/solana/{}", event.mint).blue().underline());
        println!("   {} {}", "👤 Trader:".dimmed(), 
            format!("https://solscan.io/account/{}", event.trader_public_key).cyan().underline());
        
        if let Some(signature) = &event.signature {
            println!("   {} {}", "📄 Tx:".dimmed(), 
                format!("https://solscan.io/tx/{}", signature).magenta().underline());
        }
        
        println!("   {} {}", "Time:".dimmed(), now.format("%H:%M:%S").to_string().cyan());
        
        println!("{}", "-".repeat(50).dimmed());
    }
    
    fn print_other_event(&self, event: &PumpPortalEvent) {
        println!("\n{} {} Event - {}", 
            "📊".bright_blue(),
            event.tx_type.to_uppercase().white().bold(),
            event.symbol.bright_white()
        );
        
        println!("   {} {}", "Mint:".dimmed(), event.mint.blue());
        println!("   {} {}", "Trader:".dimmed(), event.trader_public_key.yellow());
        
        if let Some(sol_amount) = event.sol_amount {
            println!("   {} {} SOL", "Amount:".dimmed(), format!("{:.4}", sol_amount).cyan());
        }
        
        // Add links for other events
        println!("   {} {}", "🔗 View:".dimmed(), 
            format!("https://pump.fun/{}", event.mint).blue().underline());
        
        println!("{}", "-".repeat(40).dimmed());
    }

    fn print_new_token(&self, token: &NewTokenEvent) {
        let now = Utc::now();
        
        println!("\n{} {}", "🆕".bright_green(), "NEW TOKEN DETECTED".bright_green().bold());
        println!("{}", "=".repeat(70).green());
        
        println!("{} {}", "📛 Name:".bright_white(), token.name.bright_cyan().bold());
        println!("{} {}", "🏷️  Symbol:".bright_white(), token.symbol.yellow().bold());
        println!("{} {}", "🪙 Mint:".bright_white(), token.mint.blue().underline());
        println!("{} {}", "👨‍💻 Creator:".bright_white(), token.creator.magenta().underline());
        
        if let Some(mcap) = token.market_cap {
            let mcap_color = if mcap > 100000.0 { format!("${:.2}", mcap).green() }
                            else if mcap > 10000.0 { format!("${:.2}", mcap).yellow() }
                            else { format!("${:.2}", mcap).red() };
            println!("{} {}", "💰 Market Cap:".bright_white(), mcap_color.bold());
        }
        
        if let Some(desc) = &token.description {
            if !desc.trim().is_empty() {
                println!("{} {}", "📝 Description:".bright_white(), desc.trim().white());
            }
        }
        
        if let Some(twitter) = &token.twitter {
            println!("{} {}", "🐦 Twitter:".bright_white(), twitter.blue().underline());
        }
        
        if let Some(website) = &token.website {
            println!("{} {}", "🌐 Website:".bright_white(), website.blue().underline());
        }
        
        println!("{} {}", "⏰ Detected:".bright_white(), now.format("%H:%M:%S UTC").to_string().cyan());
        
        println!("{}", "=".repeat(70).green());
    }
    
    fn print_trade(&self, trade: &TradeEvent) {
        let trade_type = if trade.is_buy { "BUY".green().bold() } else { "SELL".red().bold() };
        let now = Utc::now();
        
        println!("\n{} {} {}", 
            "💸".bright_yellow(),
            trade_type,
            trade.symbol.bright_white().bold()
        );
        
        println!("   {} {} | {} {} SOL | {} {}", 
            "Token Amount:".dimmed(),
            format!("{:.2}", trade.token_amount).white(),
            "SOL Amount:".dimmed(),
            format!("{:.4}", trade.sol_amount).yellow(),
            "USD:".dimmed(),
            format!("${:.2}", trade.sol_amount * 180.0).green() // Rough SOL price estimation
        );
        
        println!("   {} {}", "Trader:".dimmed(), trade.user.cyan().underline());
        println!("   {} {}", "Mint:".dimmed(), trade.mint.blue());
        
        if let Some(signature) = &trade.signature {
            println!("   {} {}", "Tx:".dimmed(), signature.magenta());
        }
        
        println!("   {} {}", "Time:".dimmed(), now.format("%H:%M:%S").to_string().cyan());
        
        println!("{}", "-".repeat(50).dimmed());
    }
    
    fn print_account_trade(&self, trade: &AccountTradeEvent) {
        let trade_type = if trade.is_buy { "BUY".green().bold() } else { "SELL".red().bold() };
        
        println!("\n{} {} - Account Activity", 
            "👤".bright_blue(),
            trade_type
        );
        
        println!("   {} {}", "Account:".dimmed(), trade.account.yellow().underline());
        
        if let Some(symbol) = &trade.symbol {
            println!("   {} {}", "Token:".dimmed(), symbol.white().bold());
        }
        
        if let Some(mint) = &trade.mint {
            println!("   {} {}", "Mint:".dimmed(), mint.blue());
        }
        
        println!("   {} {} SOL", "Amount:".dimmed(), format!("{:.4}", trade.sol_amount.unwrap_or(0.0)).yellow());
        
        println!("{}", "-".repeat(40).dimmed());
    }
    
    fn print_unknown_event(&self, message: &str) {
        println!("\n{} Unknown Event Data:", "🔍".bright_blue());
        
        // Try to pretty print if it's JSON
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(message) {
            if let Ok(pretty) = serde_json::to_string_pretty(&json) {
                println!("{}", pretty.dimmed());
                return;
            }
        }
        
        // Otherwise print raw
        println!("{}", message.dimmed());
        println!("{}", "-".repeat(40).dimmed());
    }
}

// PumpPortal event structure (the actual format we receive)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PumpPortalEvent {
    #[serde(alias = "mint")]
    mint: String,
    
    #[serde(alias = "name")]
    name: String,
    
    #[serde(alias = "symbol")]
    symbol: String,
    
    #[serde(alias = "traderPublicKey", alias = "trader_public_key")]
    trader_public_key: String,
    
    #[serde(alias = "txType", alias = "tx_type")]
    tx_type: String,
    
    #[serde(alias = "solAmount", alias = "sol_amount")]
    sol_amount: Option<f64>,
    
    #[serde(alias = "marketCapSol", alias = "market_cap_sol")]
    market_cap_sol: Option<f64>,
    
    #[serde(alias = "initialBuy", alias = "initial_buy")]
    initial_buy: Option<f64>,
    
    #[serde(alias = "signature")]
    signature: Option<String>,
    
    #[serde(alias = "uri")]
    uri: Option<String>,
    
    #[serde(alias = "bondingCurveKey", alias = "bonding_curve_key")]
    bonding_curve_key: Option<String>,
    
    #[serde(alias = "vSolInBondingCurve", alias = "v_sol_in_bonding_curve")]
    v_sol_in_bonding_curve: Option<f64>,
    
    #[serde(alias = "vTokensInBondingCurve", alias = "v_tokens_in_bonding_curve")]
    v_tokens_in_bonding_curve: Option<f64>,
}

// Data structures for different event types (legacy, may not be needed)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewTokenEvent {
    #[serde(alias = "mint")]
    mint: String,
    
    #[serde(alias = "name")]
    name: String,
    
    #[serde(alias = "symbol")]
    symbol: String,
    
    #[serde(alias = "description")]
    description: Option<String>,
    
    #[serde(alias = "creator")]
    creator: String,
    
    #[serde(alias = "marketCap", alias = "market_cap")]
    market_cap: Option<f64>,
    
    #[serde(alias = "twitter")]
    twitter: Option<String>,
    
    #[serde(alias = "website")]
    website: Option<String>,
    
    #[serde(alias = "telegram")]
    telegram: Option<String>,
    
    #[serde(alias = "timestamp")]
    timestamp: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TradeEvent {
    #[serde(alias = "mint")]
    mint: String,
    
    #[serde(alias = "symbol")]
    symbol: String,
    
    #[serde(alias = "user")]
    user: String,
    
    #[serde(alias = "is_buy", alias = "isBuy")]
    is_buy: bool,
    
    #[serde(alias = "token_amount", alias = "tokenAmount")]
    token_amount: f64,
    
    #[serde(alias = "sol_amount", alias = "solAmount")]
    sol_amount: f64,
    
    #[serde(alias = "signature")]
    signature: Option<String>,
    
    #[serde(alias = "timestamp")]
    timestamp: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountTradeEvent {
    #[serde(alias = "account")]
    account: String,
    
    #[serde(alias = "mint")]
    mint: Option<String>,
    
    #[serde(alias = "symbol")]
    symbol: Option<String>,
    
    #[serde(alias = "is_buy", alias = "isBuy")]
    is_buy: bool,
    
    #[serde(alias = "sol_amount", alias = "solAmount")]
    sol_amount: Option<f64>,
    
    #[serde(alias = "signature")]
    signature: Option<String>,
    
    #[serde(alias = "timestamp")]
    timestamp: Option<i64>,
}