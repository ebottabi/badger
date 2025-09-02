/// Signal processing for instant buy/sell decisions

use std::sync::Arc;
use chrono::{Utc, Duration as ChronoDuration};
use colored::Colorize;
use crate::client::event_parser::UniversalPumpEvent;
use crate::util::criteria::InstantBuyCriteria;
use crate::util::display::{print_instant_buy_signal, print_whale_buy_signal};
use crate::util::time_series::TimeSeriesPoint;
use crate::execution::StrategyExecutor;
use crate::algo::mathematical_engine::{MathematicalAnalysis, BuySignalStrength};
use crate::config::Config;
use crate::util::price_feed::PriceFeed;
use serde_json;

pub struct SignalProcessor {
    criteria: InstantBuyCriteria,
    strategy_executor: Option<Arc<StrategyExecutor>>,
    price_feed: Arc<PriceFeed>,
    config: Option<Config>,
}

impl SignalProcessor {
    pub fn new(config: &Config) -> Self {
        Self {
            criteria: InstantBuyCriteria::from_config(config),
            strategy_executor: None,
            price_feed: Arc::new(PriceFeed::new()),
            config: Some(config.clone()),
        }
    }
    
    pub fn new_default() -> Self {
        Self {
            criteria: InstantBuyCriteria::default(),
            strategy_executor: None,
            price_feed: Arc::new(PriceFeed::new()),
            config: None,
        }
    }
    
    pub fn with_executor(mut self, executor: Arc<StrategyExecutor>) -> Self {
        self.strategy_executor = Some(executor);
        self
    }
    
    
    
    pub async fn process_instant_signals(&self, event: &UniversalPumpEvent) {
        let token_age = self.calculate_token_age(event);
        
        // Age filter: Only consider tokens <5 minutes old
        if token_age.num_minutes() > self.criteria.max_token_age_minutes {
            return;
        }
        
        match event.tx_type.as_str() {
            "create" => {
                // Get SOL/USD rate for market cap filtering
                let sol_usd_rate = match self.price_feed.get_sol_usd_rate().await {
                    Ok(rate) => rate,
                    Err(e) => {
                        println!("⚠️ Failed to get SOL/USD rate, using default 180: {}", e);
                        180.0 // Fallback rate
                    }
                };
                
                if self.criteria.is_valid_token_fast(event, sol_usd_rate).await {
                    // Print instant buy signal
                    self.print_enhanced_instant_buy_signal(event, &self.criteria, None).await;
                    
                    // Execute instant buy trade
                    self.execute_instant_buy_signal(event).await;
                } else {
                    // Still track token creation for monitoring
                    self.handle_token_creation(event).await;
                }
            }
            "buy" => {
                // Get SOL/USD rate for whale buy filtering
                let sol_usd_rate = match self.price_feed.get_sol_usd_rate().await {
                    Ok(rate) => rate,
                    Err(e) => {
                        println!("⚠️ Failed to get SOL/USD rate for whale check, using default 180: {}", e);
                        180.0
                    }
                };
                
                // Apply enhanced filtering for whale buys too
                if self.criteria.is_whale_buy(event) && self.criteria.is_valid_token_fast(event, sol_usd_rate).await {
                    // Print whale buy signal
                    self.print_enhanced_whale_buy_signal(event, None).await;
                    
                    // Execute whale follow trade
                    self.execute_whale_follow_signal(event).await;
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
    
    async fn execute_instant_buy_signal(&self, event: &UniversalPumpEvent) {
        if let Some(ref executor) = self.strategy_executor {
            // Convert instant buy signal to mathematical analysis format
            let analysis = MathematicalAnalysis {
                progress_velocity: 5.0, // High velocity for instant buy
                volume_velocity: event.sol_amount.unwrap_or(0.0),
                price_velocity: 0.02, // Positive momentum
                holder_distribution_score: 0.8, // Assume good distribution for instant buy
                predictive_growth_score: 2.0, // High growth potential
                composite_virality_score: 0.85, // High virality for instant signal
                buy_signal_strength: BuySignalStrength::StrongBuy,
            };
            
            println!("🚀 EXECUTING INSTANT BUY SIGNAL for {}", event.mint);
            
            let executor_clone = Arc::clone(executor);
            let mint_clone = event.mint.clone();
            let symbol_clone = event.symbol.clone();
            let market_cap = event.market_cap_sol;
            tokio::spawn(async move {
                executor_clone.handle_buy_signal(&analysis, &mint_clone, &symbol_clone, market_cap).await;
            });
        }
    }
    
    async fn execute_whale_follow_signal(&self, event: &UniversalPumpEvent) {
        if let Some(ref executor) = self.strategy_executor {
            let sol_amount = event.sol_amount.unwrap_or(0.0);
            // Only follow whales with 8+ SOL trades
            if sol_amount >= 8.0 {
                let analysis = MathematicalAnalysis {
                    progress_velocity: 3.0,
                    volume_velocity: sol_amount,
                    price_velocity: 0.015,
                    holder_distribution_score: 0.7,
                    predictive_growth_score: 1.5,
                    composite_virality_score: 0.75,
                    buy_signal_strength: BuySignalStrength::Buy,
                };
                
                println!("🐋 EXECUTING WHALE FOLLOW for {} ({:.1} SOL)", event.mint, sol_amount);
                
                let executor_clone = Arc::clone(executor);
                let mint_clone = event.mint.clone();
                let symbol_clone = event.symbol.clone();
                let market_cap = event.market_cap_sol;
                tokio::spawn(async move {
                    executor_clone.handle_buy_signal(&analysis, &mint_clone, &symbol_clone, market_cap).await;
                });
            }
        }
    }
    
    async fn handle_token_creation(&self, event: &UniversalPumpEvent) {
        // Monitor token creation but don't trade immediately
        // This feeds into the sliding window analysis for later decisions
        println!("👶 New token created: {} (monitoring...)", event.mint);
    }

    async fn print_enhanced_instant_buy_signal(&self, event: &UniversalPumpEvent, _criteria: &crate::util::criteria::InstantBuyCriteria, dex_data: Option<&crate::util::criteria::DexScreenerData>) {
        use crate::util::display::print_instant_buy_signal;
        use colored::Colorize;
        
        // Print standard signal first
        print_instant_buy_signal(event, _criteria);
        
        // Add DexScreener enrichment if available
        if let Some(data) = dex_data {
            println!("\n{} {}", "📊", "DEXSCREENER ENRICHMENT".bold().cyan());
            println!("{}", "─".repeat(50));
            
            if let Some(liquidity) = data.liquidity_usd {
                println!("💧 Liquidity: ${:.0}", liquidity);
            }
            
            if let Some(volume) = data.volume_24h {
                println!("📈 Volume 24h: ${:.0}", volume);
            }
            
            if let Some(price_change) = data.price_change_5m {
                let color = if price_change > 0.0 { "green" } else { "red" };
                println!("📊 Price Change 5m: {:.2}%", price_change.to_string().color(color));
            }
            
            if let Some(fdv) = data.fdv {
                println!("💰 FDV: ${:.0}", fdv);
            }
            
            if let Some(txns) = data.txns_5m {
                println!("🔄 Transactions 5m: {}", txns);
            }
            
            println!("{}", "─".repeat(50));
        } else {
            println!("⚠️ DexScreener data not available (new token)");
        }
    }

    async fn print_enhanced_whale_buy_signal(&self, event: &UniversalPumpEvent, dex_data: Option<&crate::util::criteria::DexScreenerData>) {
        use crate::util::display::print_whale_buy_signal;
        use colored::Colorize;
        
        // Print standard whale signal first
        print_whale_buy_signal(event);
        
        // Add DexScreener enrichment if available
        if let Some(data) = dex_data {
            println!("\n{} {}", "🐋", "WHALE + DEXSCREENER DATA".bold().blue());
            println!("{}", "─".repeat(50));
            
            if let Some(liquidity) = data.liquidity_usd {
                println!("💧 Liquidity: ${:.0}", liquidity);
            }
            
            if let Some(volume) = data.volume_24h {
                println!("📈 Volume 24h: ${:.0}", volume);
            }
            
            if let Some(price_change) = data.price_change_5m {
                let color = if price_change > 0.0 { "green" } else { "red" };
                println!("📊 Price Change 5m: {:.2}%", price_change.to_string().color(color));
            }
            
            println!("{}", "─".repeat(50));
        }
    }
}