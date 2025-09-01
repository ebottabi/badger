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

use algo::analyzer::PumpRealtimeAnalyzer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "🚀 Pump.fun Real-time Analyzer + Trend Detection".bright_magenta().bold());
    println!("{}", "⚡ Real-time decisions + 📊 15s sliding window analysis".cyan());
    
    let mut analyzer = PumpRealtimeAnalyzer::new();
    analyzer.start_monitoring().await?;
    
    Ok(())
}