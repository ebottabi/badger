/// Main strategy executor coordinating all execution phases

use std::sync::Arc;
use tokio::{time::Duration, sync::Semaphore};
use colored::Colorize;
use serde_json;

use crate::config::Config;
use crate::algo::mathematical_engine::MathematicalAnalysis;
use super::{PositionManager, TradingClient, RiskManager, Position, PortfolioTracker};
use crate::util::price_feed::PriceFeed;

pub struct StrategyExecutor {
    config: Config,
    position_manager: Arc<PositionManager>,
    trading_client: Arc<TradingClient>,
    risk_manager: Arc<RiskManager>,
    portfolio_tracker: Arc<std::sync::Mutex<PortfolioTracker>>,
    price_feed: Arc<PriceFeed>,
    execution_semaphore: Arc<Semaphore>, // Global semaphore to prevent concurrent trades
}

impl StrategyExecutor {
    pub fn new(
        config: Config,
        position_manager: Arc<PositionManager>,
        trading_client: Arc<TradingClient>,
        risk_manager: Arc<RiskManager>,
    ) -> Self {
        let portfolio_tracker = Arc::new(std::sync::Mutex::new(
            PortfolioTracker::new("data/portfolio.json")
        ));
        
        Self {
            config,
            position_manager,
            trading_client,
            risk_manager,
            portfolio_tracker,
            price_feed: Arc::new(PriceFeed::new()),
            execution_semaphore: Arc::new(Semaphore::new(1)), // Allow only 1 concurrent execution
        }
    }
    
    pub async fn handle_buy_signal(&self, analysis: &MathematicalAnalysis, mint: &str, symbol: &str, market_cap_sol: Option<f64>) {
        println!("\n{} Processing buy signal for {}", "⚡".bright_yellow(), mint);
        
        // CRITICAL: Acquire global execution permit to prevent race conditions
        let _execution_permit = match self.execution_semaphore.try_acquire() {
            Ok(permit) => {
                println!("🔒 Execution permit acquired for {}", mint);
                permit
            },
            Err(_) => {
                println!("🚫 EXECUTION BUSY: Another trade in progress, skipping {}", mint);
                return;
            }
        };
        
        // GLOBAL EMERGENCY STOP: Check if emergency stop file exists
        if std::path::Path::new("data/EMERGENCY_STOP").exists() {
            println!("🚨 GLOBAL EMERGENCY STOP ACTIVATED - Delete data/EMERGENCY_STOP to resume trading");
            return;
        }
        
        // EMERGENCY STOP: Check position limit before any processing
        let current_positions = self.position_manager.get_open_positions();
        let max_positions = self.config.allocation.max_positions as usize;
        
        if current_positions.len() >= max_positions {
            println!("🚨 EMERGENCY STOP: Already at maximum positions ({}/{})", current_positions.len(), max_positions);
            println!("   Current positions:");
            for pos in &current_positions {
                println!("     - {} ({:.6} SOL)", pos.mint, pos.sol_invested);
            }
            
            // Create emergency stop file to prevent further trading
            if let Err(e) = std::fs::write("data/EMERGENCY_STOP", "Position limit exceeded. Delete this file to resume trading.") {
                println!("⚠️ Failed to create emergency stop file: {}", e);
            }
            return;
        }
        
        // Phase 1: Validation (0-5s)
        if !self.validate_signal(analysis, mint).await {
            return;
        }
        
        // Phase 2: Position sizing (5-10s)
        let position_size = self.calculate_position_size(analysis).await;
        
        // Phase 3: Print verification links (10-15s)
        self.print_verification_info(mint, position_size);
        
        // Phase 4: Execute trade (15-60s)
        println!("🎯 About to execute buy for {} with position size ${:.4}", mint, position_size);
        match self.execute_buy(mint, symbol, position_size, market_cap_sol).await {
            Ok(()) => {
                println!("✅ Execute buy completed successfully for {}", mint);
            },
            Err(e) => {
                println!("❌ Trade execution failed for {}: {}", mint, e);
            }
        }
        
        // Permit will be automatically released when _execution_permit goes out of scope
        println!("🔓 Execution permit released for {}", mint);
    }
    
    async fn validate_signal(&self, analysis: &MathematicalAnalysis, mint: &str) -> bool {
        // EMERGENCY SAFEGUARD: Check if we can open more positions
        let active_positions = self.position_manager.get_open_positions();
        let position_count = active_positions.len();
        
        // CRITICAL: Also count active positions from portfolio tracker as backup
        let portfolio_positions = if let Ok(tracker) = self.portfolio_tracker.lock() {
            tracker.get_all_active_positions().len()
        } else {
            0
        };
        
        let max_positions = self.config.allocation.max_positions as usize;
        let actual_position_count = position_count.max(portfolio_positions);
        
        println!("🔒 POSITION LIMIT CHECK:");
        println!("   Position manager positions: {}", position_count);
        println!("   Portfolio tracker positions: {}", portfolio_positions);
        println!("   Actual position count: {}", actual_position_count);
        println!("   Max allowed positions: {}", max_positions);
        
        if actual_position_count >= max_positions {
            println!("❌ REJECTED: Maximum positions ({}) reached", max_positions);
            return false;
        }
        if !self.risk_manager.can_open_position(active_positions.len()) {
            println!("🚫 RISK MANAGER REJECTION for {}", mint);
            return false;
        }
        
        // Check if already have position in this token (check both trackers)
        if self.position_manager.get_position(mint).is_some() {
            println!("⚠️ Position manager already has position in {}, skipping", mint);
            return false;
        }
        
        // Also check portfolio tracker as backup
        if let Ok(tracker) = self.portfolio_tracker.lock() {
            if tracker.get_position(mint).is_some() {
                println!("⚠️ Portfolio tracker already has position in {}, skipping", mint);
                return false;
            }
        }
        
        // Check capital limits
        let total_invested = self.position_manager.get_total_invested();
        let sol_rate = match self.price_feed.get_sol_usd_rate().await {
            Ok(rate) => rate,
            Err(e) => {
                println!("⚠️ Failed to get SOL/USD rate: {}, skipping capital check", e);
                return true; // Continue with signal if we can't get rate
            }
        };
        let total_invested_usd = total_invested * sol_rate;
        
        if total_invested_usd >= self.config.strategy.total_capital_usd {
            println!("💰 CAPITAL LIMIT REACHED: ${:.2}/${:.2} invested", 
                     total_invested_usd, self.config.strategy.total_capital_usd);
            return false;
        }
        
        // Validate entry criteria from config
        let entry = &self.config.entry_criteria;
        
        // Check virality score threshold (if configured)
        if let Some(min_virality) = entry.min_virality_score {
            if analysis.composite_virality_score < min_virality {
                println!("⚠️ Virality score {:.2} < {:.2} threshold, skipping", 
                         analysis.composite_virality_score, min_virality);
                return false;
            }
        }
        
        // Check rug score (holder distribution) (if configured)
        if let Some(min_rug_score) = entry.min_rug_score {
            if analysis.holder_distribution_score < min_rug_score {
                println!("⚠️ Rug score {:.2} < {:.2} threshold, skipping", 
                         analysis.holder_distribution_score, min_rug_score);
                return false;
            }
        }
        
        // Check progress velocity (if configured)
        if let Some(min_velocity) = entry.min_progress_velocity {
            if analysis.progress_velocity < min_velocity {
                println!("⚠️ Progress velocity {:.2} < {:.2} threshold, skipping", 
                         analysis.progress_velocity, min_velocity);
                return false;
            }
        }
        
        // For now, skip bonding curve progress check since we don't have that data in analysis
        // TODO: Add bonding_curve_progress to MathematicalAnalysis struct
        
        println!("✅ Signal validation passed for {}", mint);
        true
    }
    
    async fn calculate_position_size(&self, analysis: &MathematicalAnalysis) -> f64 {
        // Simple allocation based on config percentages
        let total_capital = self.config.strategy.total_capital_usd;
        let total_invested_sol = self.position_manager.get_total_invested();
        
        // Convert SOL invested to USD
        let sol_rate = match self.price_feed.get_sol_usd_rate().await {
            Ok(rate) => rate,
            Err(_) => 200.0, // Fallback rate
        };
        let total_invested_usd = total_invested_sol * sol_rate;
        let available_capital = (total_capital - total_invested_usd).max(0.0);
        
        // Use main position percentage from config
        let position_percentage = self.config.allocation.main_position_percent / 100.0;
        let position_size = available_capital * position_percentage;
        
        println!("💰 POSITION SIZING (Config-Based):");
        println!("   Total capital: ${:.2}", total_capital);
        println!("   Total invested SOL: {:.4} SOL", total_invested_sol);
        println!("   SOL rate: ${:.2}/SOL", sol_rate);
        println!("   Total invested USD: ${:.2}", total_invested_usd);
        println!("   Available capital: ${:.2}", available_capital);
        println!("   Position percentage: {:.0}%", self.config.allocation.main_position_percent);
        println!("   Position size: ${:.2}", position_size);
        
        // Ensure we don't exceed available capital
        let final_position_size = position_size.min(available_capital);
        
        if final_position_size != position_size {
            println!("   ⚠️ Position size capped to available capital: ${:.2}", final_position_size);
        }
        
        final_position_size
    }
    
    fn estimate_win_probability(&self, analysis: &MathematicalAnalysis) -> f64 {
        // Estimate win probability based on signal strength
        let base_prob = 0.4; // 40% base probability
        
        // Adjust based on composite virality score (0-1 scale)
        let virality_boost = (analysis.composite_virality_score - 0.5) * 0.3; // ±15% adjustment
        
        // Adjust based on momentum (progress velocity)
        let momentum_boost = (analysis.progress_velocity / 10.0).min(0.2); // Up to +20%
        
        // Adjust based on risk (holder distribution)
        let risk_penalty = if analysis.holder_distribution_score < 0.5 { -0.1 } else { 0.0 };
        
        let final_prob = base_prob + virality_boost + momentum_boost + risk_penalty;
        final_prob.max(0.1).min(0.8) // Clamp between 10% and 80%
    }
    
    fn print_verification_info(&self, mint: &str, position_size_usd: f64) {
        let (rug_check, dex_screener, pump_fun) = self.trading_client.get_verification_links(mint);
        
        println!("\n{} {}", "🚨", "EXECUTION READY".bright_red().bold());
        println!("💰 Position Size: ${:.4}", position_size_usd);
        println!("📊 Verification Links:");
        println!("   🔍 Rug Check: {}", rug_check);
        println!("   📈 DEX Screener: {}", dex_screener);
        println!("   🚀 Pump.fun: {}", pump_fun);
        println!("\n{} Executing in {}ms...", "⏱️".bright_yellow(), self.config.trading.execution_delay_ms);
    }
    
    pub async fn handle_sell_signal(&self, analysis: &MathematicalAnalysis, mint: &str, percentage: f64) {
        // Check if we have a position for this token
        if !self.position_manager.has_position(mint) {
            return;
        }
        
        self.print_sell_verification_info(mint, percentage);
        self.execute_sell(mint, percentage).await.unwrap_or_else(|e| {
            println!("❌ Sell execution failed: {}", e);
        });
    }
    
    fn print_sell_verification_info(&self, mint: &str, percentage: f64) {
        let (rug_check, dex_screener, pump_fun) = self.trading_client.get_verification_links(mint);
        
        println!("\n{} {}", "📉", "SELL SIGNAL TRIGGERED".bright_red().bold());
        println!("💸 Selling {}% of position", percentage);
        println!("📊 Verification Links:");
        println!("   🔍 Rug Check: {}", rug_check);
        println!("   📈 DEX Screener: {}", dex_screener);
        println!("   🚀 Pump.fun: {}", pump_fun);
        println!("\n{} Executing sell in {}ms...", "⏱️".bright_yellow(), self.config.trading.execution_delay_ms);
    }
    
    async fn execute_sell(&self, mint: &str, percentage: f64) -> Result<(), Box<dyn std::error::Error>> {
        // Execute delay for verification
        tokio::time::sleep(Duration::from_millis(self.config.trading.execution_delay_ms)).await;
        
        // Execute the sell
        match self.trading_client.sell_token(mint, percentage).await {
            Ok(tx_id) => {
                self.position_manager.close_position(mint);
                println!("✅ SELL EXECUTED: {}", tx_id);
            }
            Err(e) => {
                println!("❌ Sell failed: {}", e);
                return Err(e.into());
            }
        }
        
        Ok(())
    }
    
    async fn execute_buy(&self, mint: &str, symbol: &str, position_size_usd: f64, market_cap_sol: Option<f64>) -> Result<(), Box<dyn std::error::Error>> {
        // Convert USD to SOL using real exchange rate
        let sol_amount = self.price_feed.convert_usd_to_sol(position_size_usd).await?;
        
        // Execute delay for verification
        tokio::time::sleep(Duration::from_millis(self.config.trading.execution_delay_ms)).await;
        
        // Execute the trade with balance query
        println!("🔄 Calling execute_buy_with_balance_query for {} SOL", sol_amount);
        let (tx_id, tokens_received, effective_price) = match self.trading_client
            .execute_buy_with_balance_query(mint, sol_amount, market_cap_sol).await {
            Ok(result) => {
                println!("✅ execute_buy_with_balance_query succeeded");
                result
            },
            Err(e) => {
                println!("❌ execute_buy_with_balance_query failed: {}", e);
                println!("🔄 Attempting direct trade execution as fallback");
                
                // Fallback: try direct trade
                let tx_id = self.trading_client.buy_token(mint, sol_amount).await?;
                let estimated_price = sol_amount / 1000000.0; // Rough estimate for new tokens
                let tokens_received = sol_amount / estimated_price;
                
                println!("📊 Fallback trade executed:");
                println!("   Tx ID: {}", tx_id);
                println!("   Estimated price: {:.10} SOL", estimated_price);
                println!("   Estimated tokens: {:.0}", tokens_received);
                
                (tx_id, tokens_received, estimated_price)
            }
        };
        
        // Get SOL/USD rate for USD tracking
        let sol_to_usd_rate = self.price_feed.get_sol_usd_rate().await.unwrap_or(180.0);
        
        // Create position with actual trade data
        let position = Position::new(mint.to_string(), symbol.to_string(), effective_price, sol_amount, tokens_received, sol_to_usd_rate);
        
        println!("💾 SAVING POSITION DATA:");
        println!("   Mint: {}", mint);
        println!("   Symbol: {}", symbol);
        println!("   Effective price: {:.10} SOL", effective_price);
        println!("   SOL invested: {:.6}", sol_amount);
        println!("   Entry USD: ${:.2}", sol_amount * sol_to_usd_rate);
        println!("   Tokens received: {:.2}", tokens_received);
        
        self.position_manager.add_position(position);
        println!("✅ Position added to position manager");
        
        // EMERGENCY BACKUP: Save position data to backup file immediately
        let backup_data = format!("TRADE_BACKUP_{}.json", chrono::Utc::now().format("%Y%m%d_%H%M%S"));
        let backup_path = format!("data/{}", backup_data);
        let trade_record = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "mint": mint,
            "tx_id": tx_id,
            "sol_amount": sol_amount,
            "tokens_received": tokens_received,
            "effective_price": effective_price,
            "position_size_usd": position_size_usd
        });
        
        if let Err(e) = std::fs::write(&backup_path, serde_json::to_string_pretty(&trade_record).unwrap_or_default()) {
            println!("⚠️ Failed to create backup file: {}", e);
        } else {
            println!("💾 Emergency backup created: {}", backup_path);
        }
        
        // Update portfolio tracker
        if let Ok(mut tracker) = self.portfolio_tracker.lock() {
            if let Err(e) = tracker.add_buy(mint, symbol, tokens_received, sol_amount, effective_price, &tx_id, sol_to_usd_rate) {
                println!("❌ Failed to update portfolio tracker: {}", e);
            } else {
                println!("✅ Portfolio tracker updated");
            }
        } else {
            println!("❌ Failed to lock portfolio tracker");
        }
        
        // Verify positions were saved by reading them back
        let saved_positions = self.position_manager.get_all_positions();
        println!("🔍 VERIFICATION: Total positions after save: {}", saved_positions.len());
        for pos in &saved_positions {
            println!("   - {} ({:.6} SOL invested)", pos.mint, pos.sol_invested);
        }
        
        println!("✅ Position opened successfully:");
        println!("   💳 Wallet: {}", self.trading_client.get_wallet_address());
        println!("   🔗 Transaction: https://solscan.io/tx/{}", tx_id);
        println!("   📊 Token Details: https://solscan.io/token/{}", mint);
        println!("   🚀 Pump.fun: https://pump.fun/{}", mint);
        println!("");
        println!("   📈 WALLET ANALYTICS:");
        println!("      🔍 All Transactions: https://solscan.io/account/{}?tab=transfers", self.trading_client.get_wallet_address());
        println!("      📊 Token Holdings: https://solscan.io/account/{}?tab=tokens", self.trading_client.get_wallet_address());
        println!("      💰 SOL Balance: https://solscan.io/account/{}", self.trading_client.get_wallet_address());
        println!("      📊 DEX Screener: https://dexscreener.com/account/{}", self.trading_client.get_wallet_address());
        println!("      🌊 Solana FM: https://solana.fm/address/{}", self.trading_client.get_wallet_address());
        
        Ok(())
    }
}