/// Momentum-based entry criteria for existing tokens

use crate::config::Config;
use crate::momentum::VolumeMetrics;
use std::time::{SystemTime, Duration};

pub struct MomentumCriteria {
    pub min_volume_spike_percent: f64,
    pub min_price_momentum_percent: f64,
    pub min_trade_count_hour: usize,
    pub min_unique_buyers_hour: usize,
    pub min_token_age_hours: u64,
    pub max_token_age_hours: u64,
    pub min_market_cap_usd: f64,
    pub max_market_cap_usd: f64,
    pub momentum_window_minutes: u64,
}

impl Default for MomentumCriteria {
    fn default() -> Self {
        Self {
            min_volume_spike_percent: 200.0,
            min_price_momentum_percent: 15.0,
            min_trade_count_hour: 50,
            min_unique_buyers_hour: 20,
            min_token_age_hours: 1,
            max_token_age_hours: 48,
            min_market_cap_usd: 15000.0,
            max_market_cap_usd: 100000.0,
            momentum_window_minutes: 60,
        }
    }
}

impl MomentumCriteria {
    pub fn from_config(config: &Config) -> Self {
        Self {
            min_volume_spike_percent: config.entry_criteria.min_volume_spike_percent.unwrap_or(200.0),
            min_price_momentum_percent: config.entry_criteria.min_price_momentum_percent.unwrap_or(15.0),
            min_trade_count_hour: config.entry_criteria.min_trade_count_hour.unwrap_or(50.0) as usize,
            min_unique_buyers_hour: config.entry_criteria.min_unique_buyers_hour.unwrap_or(20.0) as usize,
            min_token_age_hours: config.entry_criteria.min_token_age_hours.unwrap_or(1),
            max_token_age_hours: config.entry_criteria.max_token_age_hours.unwrap_or(48),
            min_market_cap_usd: config.entry_criteria.min_market_cap_usd,
            max_market_cap_usd: config.entry_criteria.max_market_cap_usd,
            momentum_window_minutes: config.entry_criteria.momentum_window_minutes.unwrap_or(60),
        }
    }
    
    pub fn validate_momentum_signal(&self, mint: &str, metrics: &VolumeMetrics, sol_to_usd_rate: f64) -> bool {
        // Check volume spike
        if metrics.volume_sol_1h * sol_to_usd_rate < self.min_volume_spike_percent {
            println!("❌ Volume spike too low for {}: ${:.0} (min: ${:.0})", 
                    mint, metrics.volume_sol_1h * sol_to_usd_rate, self.min_volume_spike_percent);
            return false;
        }
        
        // Check price momentum
        if metrics.price_change_1h_percent < self.min_price_momentum_percent {
            println!("❌ Price momentum too low for {}: {:.1}% (min: {:.1}%)", 
                    mint, metrics.price_change_1h_percent, self.min_price_momentum_percent);
            return false;
        }
        
        // Check trade count
        if metrics.trades_1h.len() < self.min_trade_count_hour {
            println!("❌ Trade count too low for {}: {} trades (min: {})", 
                    mint, metrics.trades_1h.len(), self.min_trade_count_hour);
            return false;
        }
        
        // Check unique buyer count
        if metrics.unique_traders_1h < self.min_unique_buyers_hour {
            println!("❌ Unique buyers too low for {}: {} buyers (min: {})", 
                    mint, metrics.unique_traders_1h, self.min_unique_buyers_hour);
            return false;
        }
        
        // Calculate estimated market cap from current price and volume
        let estimated_market_cap_usd = self.estimate_market_cap_from_metrics(metrics, sol_to_usd_rate);
        
        // Check market cap range
        if estimated_market_cap_usd < self.min_market_cap_usd || estimated_market_cap_usd > self.max_market_cap_usd {
            println!("❌ Market cap out of range for {}: ${:.0} (range: ${:.0}-${:.0})", 
                    mint, estimated_market_cap_usd, self.min_market_cap_usd, self.max_market_cap_usd);
            return false;
        }
        
        println!("✅ All momentum criteria passed for {}", mint);
        println!("   Volume 1h: {:.1} SOL (${:.0})", metrics.volume_sol_1h, metrics.volume_sol_1h * sol_to_usd_rate);
        println!("   Price momentum: {:.1}%", metrics.price_change_1h_percent);
        println!("   Trades: {} | Unique buyers: {}", metrics.trades_1h.len(), metrics.unique_traders_1h);
        println!("   Est. Market Cap: ${:.0}", estimated_market_cap_usd);
        
        true
    }
    
    fn estimate_market_cap_from_metrics(&self, metrics: &VolumeMetrics, sol_to_usd_rate: f64) -> f64 {
        // For pump.fun tokens, typical supply is 1B tokens
        // Use current price to estimate market cap
        let typical_supply = 1_000_000_000.0;
        let price_usd = metrics.last_price_sol * sol_to_usd_rate;
        price_usd * typical_supply
    }
    
    pub fn is_volume_spike(&self, current_volume: f64, baseline_volume: f64) -> bool {
        if baseline_volume <= 0.0 {
            return current_volume > 0.0;
        }
        
        let spike_percent = ((current_volume - baseline_volume) / baseline_volume) * 100.0;
        spike_percent >= self.min_volume_spike_percent
    }
    
    pub fn get_momentum_score(&self, metrics: &VolumeMetrics) -> f64 {
        // Calculate composite momentum score (0-100)
        let volume_score = (metrics.volume_sol_1h / 50.0).min(25.0); // Max 25 points for volume
        let price_score = (metrics.price_change_1h_percent / 2.0).min(25.0); // Max 25 points for price
        let trade_score = (metrics.trades_1h.len() as f64 / 10.0).min(25.0); // Max 25 points for trades
        let buyer_score = (metrics.unique_traders_1h as f64 / 2.0).min(25.0); // Max 25 points for buyers
        
        volume_score + price_score + trade_score + buyer_score
    }
}