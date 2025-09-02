/// Trend analysis algorithms and calculations

use std::collections::HashSet;
use crate::util::time_series::SlidingWindow;

#[derive(Debug)]
pub struct TrendAnalysis {
    pub price_momentum_percent_per_min: f64,
    pub volume_acceleration_percent: f64,
    pub unique_traders: usize,
    pub trade_frequency_per_min: f64,
    pub buy_sell_ratio: f64,
    pub trend_strength: TrendStrength,
}

#[derive(Debug, PartialEq)]
pub enum TrendStrength {
    StrongBullish,
    Bullish,
    Neutral,
    Bearish,
    StrongBearish,
}

impl std::fmt::Display for TrendStrength {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TrendStrength::StrongBullish => write!(f, "STRONG BULLISH"),
            TrendStrength::Bullish => write!(f, "BULLISH"),
            TrendStrength::Neutral => write!(f, "NEUTRAL"),
            TrendStrength::Bearish => write!(f, "BEARISH"),
            TrendStrength::StrongBearish => write!(f, "STRONG BEARISH"),
        }
    }
}

pub fn calculate_trend_analysis(window: &SlidingWindow) -> Option<TrendAnalysis> {
    if window.events.len() < 2 {
        return None;
    }
    
    let events = &window.events;
    let total_duration_mins = events.back()?.timestamp
        .signed_duration_since(events.front()?.timestamp)
        .num_seconds() as f64 / 60.0;
        
    if total_duration_mins <= 0.0 {
        return None;
    }
    
    // Calculate price momentum
    let first_price = events.front()?.market_cap_sol;
    let last_price = events.back()?.market_cap_sol;
    let price_change_percent = if first_price > 0.0 {
        ((last_price - first_price) / first_price) * 100.0
    } else {
        0.0
    };
    let price_momentum_per_min = price_change_percent / total_duration_mins;
    
    // Calculate volume metrics
    let _total_volume: f64 = events.iter().map(|e| e.volume_sol).sum();
    let buy_volume: f64 = events.iter()
        .filter(|e| e.tx_type == "buy")
        .map(|e| e.volume_sol)
        .sum();
    let sell_volume: f64 = events.iter()
        .filter(|e| e.tx_type == "sell")
        .map(|e| e.volume_sol)
        .sum();
        
    let buy_sell_ratio = if sell_volume > 0.0 {
        buy_volume / sell_volume
    } else if buy_volume > 0.0 {
        10.0 // All buys, no sells
    } else {
        1.0
    };
    
    // Calculate unique traders
    let unique_traders: HashSet<String> = events.iter()
        .map(|e| e.trader.clone())
        .collect();
        
    // Calculate trade frequency
    let trade_count = events.len() as f64;
    let trade_frequency_per_min = trade_count / total_duration_mins;
    
    // Determine trend strength
    let trend_strength = match price_momentum_per_min {
        x if x > 200.0 => TrendStrength::StrongBullish,
        x if x > 50.0 => TrendStrength::Bullish,
        x if x < -200.0 => TrendStrength::StrongBearish,
        x if x < -50.0 => TrendStrength::Bearish,
        _ => TrendStrength::Neutral,
    };
    
    Some(TrendAnalysis {
        price_momentum_percent_per_min: price_momentum_per_min,
        volume_acceleration_percent: 0.0, // Could calculate if needed
        unique_traders: unique_traders.len(),
        trade_frequency_per_min,
        buy_sell_ratio,
        trend_strength,
    })
}