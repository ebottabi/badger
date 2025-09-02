/// Background position monitoring for stop losses and profit taking

use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use colored::Colorize;

use crate::config::Config;
use super::{PositionManager, TradingClient, RiskManager, PortfolioTracker};
use crate::util::price_feed::PriceFeed;
use crate::util::portfolio_display::PortfolioDisplayer;

pub struct PositionMonitor {
    config: Config,
    position_manager: Arc<PositionManager>,
    trading_client: Arc<TradingClient>,
    risk_manager: Arc<RiskManager>,
    portfolio_tracker: Arc<std::sync::Mutex<PortfolioTracker>>,
    price_feed: Arc<PriceFeed>,
    portfolio_displayer: PortfolioDisplayer,
}

impl PositionMonitor {
    pub fn new(
        config: Config,
        position_manager: Arc<PositionManager>,
        trading_client: Arc<TradingClient>,
        risk_manager: Arc<RiskManager>,
        portfolio_tracker: Arc<std::sync::Mutex<PortfolioTracker>>,
    ) -> Self {
        let price_feed = Arc::new(PriceFeed::new());
        let portfolio_displayer = PortfolioDisplayer::new(
            Arc::clone(&position_manager),
            Arc::clone(&portfolio_tracker),
            Arc::clone(&price_feed)
        );
        
        Self {
            config,
            position_manager,
            trading_client,
            risk_manager,
            portfolio_tracker,
            price_feed,
            portfolio_displayer,
        }
    }
    
    pub async fn start_monitoring(self: Arc<Self>) {
        println!("{} Starting position monitoring...", "📊".bright_green());
        
        // Monitor positions every 30 seconds, display portfolio every 5 minutes
        let mut monitor_timer = interval(Duration::from_secs(5));
        let mut display_counter = 0;
        
        loop {
            monitor_timer.tick().await;
            self.check_all_positions().await;
            
            // Display portfolio every 10th cycle (5 minutes)
            display_counter += 1;
            if display_counter >= 10 {
                self.portfolio_displayer.print_portfolio_status().await;
                display_counter = 0;
            }
        }
    }
    
    async fn check_all_positions(&self) {
        let positions = self.position_manager.get_open_positions();
        
        if positions.is_empty() {
            return;
        }
        
        println!("\n{} Monitoring {} open positions...", "👀".bright_blue(), positions.len());
        
        for position in positions {
            // Update current price
            if let Ok(current_price) = self.trading_client.get_current_price(&position.mint).await {
                self.position_manager.update_price(&position.mint, current_price);
                
                // Get updated position
                if let Some(updated_position) = self.position_manager.get_position(&position.mint) {
                    self.evaluate_position(&updated_position).await;
                }
            }
        }
    }
    
    async fn evaluate_position(&self, position: &super::Position) {
        let mint = &position.mint;
        let pnl_percent = position.get_pnl_percent();
        let age_hours = position.get_age_hours();
        
        let (rug_check, dex_screener, pump_fun) = self.trading_client.get_verification_links(mint);
        
        println!("{}", "=".repeat(80));
        println!("📈 Position Update: {} ({})", position.symbol, mint);
        println!("💰 Entry: ${:.2}", position.entry_usd);
        println!("📊 Current: ${:.2}", position.current_value_usd);
        println!("💵 P&L: ${:.2} ({:.1}%)", position.profit_loss_usd, pnl_percent);
        println!("⏰ Age: {:.1}h", age_hours);
        println!("💳 Wallet: {}", self.trading_client.get_wallet_address());
        println!("🔍 Token Links:");
        println!("   📈 DEX Screener: {}", dex_screener);
        println!("   🌊 Pump.fun: {}", pump_fun);
        println!("   🛡️ Rug Check: {}", rug_check);
        println!("🏦 Wallet Links:");
        println!("   📊 Holdings: https://solscan.io/account/{}?tab=tokens", self.trading_client.get_wallet_address());
        println!("   💰 Balance: https://solscan.io/account/{}", self.trading_client.get_wallet_address());
        println!("{}", "=".repeat(80));
        
        // Check for force exit (time-based)
        if self.risk_manager.should_force_exit(position) {
            println!("⏰ Force exit triggered for {} ({}h old)", mint, age_hours);
            self.execute_exit(mint, 100.0, "TIME_LIMIT").await;
            return;
        }
        
        // Check for stop loss
        if self.risk_manager.should_stop_loss(position) {
            let loss_usd = position.entry_usd - position.current_value_usd;
            println!("🛑 USD stop loss triggered for {} (Lost ${:.2})", mint, loss_usd);
            self.execute_exit(mint, 100.0, "STOP_LOSS").await;
            return;
        }
        
        // Check for profit taking
        if let Some(exit_percentage) = self.risk_manager.should_take_profit(position) {
            if exit_percentage >= 100.0 {
                println!("🎯 Exit condition met for {} (${:.2} → ${:.2})", 
                        mint, position.entry_usd, position.current_value_usd);
                self.execute_exit(mint, 100.0, "EXIT_STRATEGY").await;
            } else {
                println!("💎 Partial profit taking for {} ({}% exit - diamond hands on {}%)", 
                        mint, exit_percentage, 100.0 - exit_percentage);
                self.execute_exit(mint, exit_percentage, "PARTIAL_PROFIT").await;
            }
            return;
        }
        
        // Check for emergency exit conditions
        if self.risk_manager.should_emergency_exit(position) {
            println!("🚨 Emergency exit for {} (high risk detected)", mint);
            self.execute_exit(mint, 100.0, "EMERGENCY").await;
            return;
        }
    }
    
    async fn execute_exit(&self, mint: &str, percentage: f64, reason: &str) {
        println!("\n{} EXECUTING EXIT: {} ({}% position)", 
                 "🚪".bright_red().bold(), reason, percentage);
        
        // Execute the sell
        match self.trading_client.sell_token(mint, percentage).await {
            Ok(tx_id) => {
                println!("✅ Exit executed successfully:");
                println!("   💳 Wallet: {}", self.trading_client.get_wallet_address());
                println!("   🔗 Transaction: https://solscan.io/tx/{}", tx_id);
                println!("   📊 Token Details: https://solscan.io/token/{}", mint);
                println!("   📈 All Wallet Trades: https://solscan.io/account/{}?tab=transfers", self.trading_client.get_wallet_address());
                println!("   💼 Portfolio View: https://solscan.io/account/{}", self.trading_client.get_wallet_address());
                
                // Update position status
                if percentage >= 100.0 {
                    self.position_manager.close_position(mint);
                }
                
                // Update portfolio tracker
                let current_price_usd = self.trading_client.get_current_price(mint).await.unwrap_or(0.0);
                let balance = self.trading_client.get_token_balance(mint).await.unwrap_or(0.0);
                let tokens_sold = balance * (percentage / 100.0);
                let usd_received = tokens_sold * current_price_usd; // Direct USD calculation
                
                // Get SOL/USD rate for conversion
                let sol_to_usd_rate = self.price_feed.get_sol_usd_rate().await.unwrap_or(180.0);
                let sol_received = usd_received / sol_to_usd_rate; // Convert USD to SOL
                
                if let Ok(mut tracker) = self.portfolio_tracker.lock() {
                    let _ = tracker.add_sell(mint, tokens_sold, sol_received, current_price_usd, &tx_id, sol_to_usd_rate);
                }
            }
            Err(e) => {
                println!("❌ Exit failed: {}", e);
            }
        }
    }
}