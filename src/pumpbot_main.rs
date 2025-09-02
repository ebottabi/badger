/// Pump.fun Real-time WebSocket + Sliding Window Analysis
/// 
/// Features:
/// - Real-time decision making (<100ms)
/// - Sliding window trend analysis (15s intervals)
/// - Multi-format support (pump, bonk, raydium, etc.)
/// - Early sniping with <5 minute age filter

use colored::Colorize;

mod client;
mod algo;
mod util;
mod config;
mod execution;
mod momentum;

use algo::analyzer::PumpRealtimeAnalyzer;
use config::ConfigManager;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🚀 Pump.fun Real-time Analyzer + Auto Trading".bright_magenta().bold());
    println!("{}", "⚡ Real-time decisions + 📊 Mathematical execution".cyan());
    
    // Load configuration
    let config_manager = Arc::new(ConfigManager::new("config.toml")?);
    config_manager.start_hot_reload().await;
    
    println!("✅ Configuration loaded with hot-reload enabled");
    
    // Initialize analyzer with execution capabilities
    let mut analyzer = PumpRealtimeAnalyzer::new_with_execution(config_manager).await?;
    analyzer.start_monitoring().await?;
    
    Ok(())
}