/// Momentum-based signal processor for existing tokens with proven traction

use std::sync::Arc;
use std::collections::HashMap;
use std::time::{SystemTime, Duration};
use tokio::time::interval;
use colored::Colorize;

use crate::momentum::{MomentumTracker, MomentumCriteria, VolumeMetrics};
use crate::execution::StrategyExecutor;
use crate::algo::mathematical_engine::{MathematicalAnalysis, BuySignalStrength};
use crate::config::Config;
use crate::util::price_feed::PriceFeed;

pub struct MomentumSignalProcessor {
    criteria: MomentumCriteria,
    momentum_tracker: Arc<MomentumTracker>,
    strategy_executor: Option<Arc<StrategyExecutor>>,
    price_feed: Arc<PriceFeed>,
    config: Option<Config>,
    processed_signals: HashMap<String, SystemTime>, // Prevent duplicate signals
}

impl MomentumSignalProcessor {
    pub fn new(config: &Config) -> Self {
        Self {
            criteria: MomentumCriteria::from_config(config),
            momentum_tracker: Arc::new(MomentumTracker::new()),
            strategy_executor: None,
            price_feed: Arc::new(PriceFeed::new()),
            config: Some(config.clone()),
            processed_signals: HashMap::new(),
        }
    }
    
    pub fn new_default() -> Self {
        Self {
            criteria: MomentumCriteria::default(),
            momentum_tracker: Arc::new(MomentumTracker::new()),
            strategy_executor: None,
            price_feed: Arc::new(PriceFeed::new()),
            config: None,
            processed_signals: HashMap::new(),
        }
    }
    
    pub fn with_executor(mut self, executor: Arc<StrategyExecutor>) -> Self {
        self.strategy_executor = Some(executor);
        self
    }
    
    pub async fn start_momentum_tracking(&mut self) -> anyhow::Result<()> {
        println!("🚀 Starting momentum-based signal processor...");
        
        // Connect to WebSocket
        let tracker = Arc::get_mut(&mut self.momentum_tracker).unwrap();
        tracker.connect().await?;
        
        // Start momentum scanning loop
        self.start_scanning_loop().await;
        
        Ok(())
    }
    
    async fn start_scanning_loop(&mut self) {
        println!("🔍 Starting momentum scanning loop (every 1 second)");
        let mut scan_interval = interval(Duration::from_secs(1));
        
        loop {
            scan_interval.tick().await;
            
            // Clean up old processed signals (older than 1 hour)
            self.cleanup_processed_signals();
            
            // Scan for momentum opportunities
            match self.scan_for_momentum_signals().await {
                Ok(signals_found) => {
                    if signals_found > 0 {
                        println!("📊 Momentum scan complete: {} signals processed", signals_found);
                    }
                }
                Err(e) => {
                    println!("❌ Error during momentum scan: {}", e);
                }
            }
            
            // Print summary every 5 minutes (300 scans at 1 second intervals)
            static mut SCAN_COUNT: usize = 0;
            unsafe {
                SCAN_COUNT += 1;
                if SCAN_COUNT % 300 == 0 {
                    self.momentum_tracker.print_momentum_summary();
                }
            }
        }
    }
    
    async fn scan_for_momentum_signals(&mut self) -> anyhow::Result<usize> {
        // Get SOL/USD rate
        let sol_usd_rate = self.price_feed.get_sol_usd_rate().await.unwrap_or(200.0);
        
        // Get momentum candidates from tracker
        let candidates = self.momentum_tracker.get_momentum_candidates(
            self.criteria.min_volume_spike_percent,
            self.criteria.min_trade_count_hour,
            self.criteria.min_unique_buyers_hour,
        );
        
        if candidates.is_empty() {
            return Ok(0);
        }
        
        println!("\n🎯 Scanning {} momentum candidates...", candidates.len());
        
        let mut signals_processed = 0;
        
        for (mint, metrics) in candidates {
            // Skip if we already processed this token recently
            if self.is_recently_processed(&mint) {
                continue;
            }
            
            // Validate momentum signal
            if !self.criteria.validate_momentum_signal(&mint, &metrics, sol_usd_rate) {
                continue;
            }
            
            // Print momentum signal
            self.print_momentum_signal(&mint, &metrics, sol_usd_rate).await;
            
            // Execute momentum buy signal
            self.execute_momentum_signal(&mint, &metrics).await;
            
            // Mark as processed
            self.processed_signals.insert(mint.clone(), SystemTime::now());
            
            signals_processed += 1;
        }
        
        Ok(signals_processed)
    }
    
    async fn print_momentum_signal(&self, mint: &str, metrics: &VolumeMetrics, sol_usd_rate: f64) {
        println!("\n{} {}", "🔥", "MOMENTUM SIGNAL DETECTED".bright_green().bold());
        println!("{}", "═".repeat(80));
        println!("🪙 Token: {}", mint);
        println!("💰 Volume (1h): {:.1} SOL (${:.0})", metrics.volume_sol_1h, metrics.volume_sol_1h * sol_usd_rate);
        println!("📈 Price Change (1h): {:.1}%", metrics.price_change_1h_percent);
        println!("🔄 Trades: {} | 👥 Unique Buyers: {}", metrics.trades_1h.len(), metrics.unique_traders_1h);
        println!("💵 Buy Volume: {:.1} SOL | 💸 Sell Volume: {:.1} SOL", 
                 metrics.buy_volume_sol_1h, metrics.sell_volume_sol_1h);
        println!("💎 Current Price: {:.10} SOL (${:.8})", 
                 metrics.last_price_sol, metrics.last_price_sol * sol_usd_rate);
        
        let momentum_score = self.criteria.get_momentum_score(metrics);
        println!("🎯 Momentum Score: {:.1}/100", momentum_score);
        
        // Public links
        println!("\n{}", "🔗 ANALYSIS LINKS:".bold());
        println!("   📊 DEX Screener: https://dexscreener.com/solana/{}", mint);
        println!("   🚀 Pump.fun: https://pump.fun/{}", mint);
        println!("   🛡️ Rug Check: https://rugcheck.xyz/tokens/{}", mint);
        
        println!("{}", "═".repeat(80));
    }
    
    async fn execute_momentum_signal(&self, mint: &str, metrics: &VolumeMetrics) {
        if let Some(ref executor) = self.strategy_executor {
            // Convert momentum metrics to mathematical analysis format
            let momentum_score = self.criteria.get_momentum_score(metrics);
            let analysis = MathematicalAnalysis {
                progress_velocity: metrics.price_change_1h_percent / 5.0, // Scale to reasonable range
                volume_velocity: metrics.volume_sol_1h,
                price_velocity: metrics.price_change_1h_percent / 100.0, // Convert % to decimal
                holder_distribution_score: (metrics.unique_traders_1h as f64 / 50.0).min(1.0), // Scale to 0-1
                predictive_growth_score: momentum_score / 50.0, // Scale to 0-2
                composite_virality_score: (momentum_score / 100.0).min(1.0), // Scale to 0-1
                buy_signal_strength: if momentum_score > 80.0 {
                    BuySignalStrength::StrongBuy
                } else {
                    BuySignalStrength::Buy
                },
            };
            
            println!("🚀 EXECUTING MOMENTUM BUY SIGNAL for {}", mint);
            
            let executor_clone = Arc::clone(executor);
            let mint_clone = mint.to_string();
            let symbol_clone = format!("TOKEN_{}", &mint[..8]); // Use first 8 chars as symbol
            let estimated_market_cap_sol = self.estimate_market_cap_sol(metrics);
            
            tokio::spawn(async move {
                executor_clone.handle_buy_signal(&analysis, &mint_clone, &symbol_clone, Some(estimated_market_cap_sol)).await;
            });
        }
    }
    
    fn estimate_market_cap_sol(&self, metrics: &VolumeMetrics) -> f64 {
        // Estimate market cap in SOL based on price and typical supply
        let typical_supply = 1_000_000_000.0;
        metrics.last_price_sol * typical_supply
    }
    
    fn is_recently_processed(&self, mint: &str) -> bool {
        if let Some(&processed_time) = self.processed_signals.get(mint) {
            let time_since = SystemTime::now().duration_since(processed_time).unwrap_or_default();
            time_since < Duration::from_secs(3600) // Don't reprocess within 1 hour
        } else {
            false
        }
    }
    
    fn cleanup_processed_signals(&mut self) {
        let one_hour_ago = SystemTime::now() - Duration::from_secs(3600);
        self.processed_signals.retain(|_, &mut processed_time| processed_time > one_hour_ago);
    }
    
    pub fn get_active_tokens_count(&self) -> usize {
        // This would need to be implemented to get count from momentum tracker
        0 // Placeholder
    }
    
    pub fn get_momentum_metrics(&self, mint: &str) -> Option<VolumeMetrics> {
        self.momentum_tracker.get_token_metrics(mint)
    }
}