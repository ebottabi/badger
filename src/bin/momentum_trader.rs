/// Momentum-based trading bot for existing pump.fun tokens

use std::sync::Arc;
use anyhow::Result;

use badger::{Config, PositionManager, TradingClient, RiskManager, StrategyExecutor, MomentumSignalProcessor};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Starting Badger Momentum Trading Bot...");
    
    // Load configuration
    let config = Config::load_from_file("config.toml")?;
    println!("✅ Configuration loaded");
    
    // Initialize core components
    let position_manager = Arc::new(PositionManager::new());
    let trading_client = Arc::new(TradingClient::new(
        config.wallet.public_key.clone(),
        config.wallet.pump_api_key.clone(),
        config.trading.slippage_tolerance_percent,
        config.trading.max_retry_attempts,
        config.trading.priority_fee_sol,
    ));
    let risk_manager = Arc::new(RiskManager::new(config.clone()));
    
    // Create strategy executor
    let strategy_executor = Arc::new(StrategyExecutor::new(
        config.clone(),
        position_manager.clone(),
        trading_client.clone(),
        risk_manager.clone(),
    ));
    
    // Initialize momentum signal processor
    let mut momentum_processor = MomentumSignalProcessor::new(&config)
        .with_executor(strategy_executor);
    
    println!("🔥 Badger Momentum Bot initialized successfully!");
    println!("📊 Strategy: Volume spike detection on existing tokens");
    println!("💰 Capital: ${:.2}", config.strategy.total_capital_usd);
    println!("🎯 Target: {:.0}x returns", config.strategy.target_multiplier);
    println!("\n{}", "═".repeat(60));
    
    // Start momentum tracking and signal processing
    momentum_processor.start_momentum_tracking().await?;
    
    // The momentum processor runs indefinitely
    println!("🛑 Momentum bot stopped");
    Ok(())
}