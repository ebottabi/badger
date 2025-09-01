/// Mathematical calculation implementations for the engine

use std::collections::HashMap;
use crate::util::time_series::{SlidingWindow, TimeSeriesPoint};
use super::mathematical_engine::{MathematicalEngine, BuySignalStrength};

impl MathematicalEngine {
    /// 1. Progress Velocity (Bonding Curve Hype Momentum)
    /// Formula: v_prog(t) = (Prog(t) - Prog(t-Δt)) / Δt (30-second intervals)
    pub fn calculate_progress_velocity(&self, window: &SlidingWindow) -> Option<f64> {
        let events = &window.events;
        if events.len() < 2 {
            return None;
        }
        
        // Get data points 30 seconds apart
        let latest = events.back()?;
        let mut earlier_point = None;
        
        for event in events.iter().rev() {
            let time_diff = latest.timestamp.signed_duration_since(event.timestamp);
            if time_diff.num_seconds() >= 30 {
                earlier_point = Some(event);
                break;
            }
        }
        
        if let Some(earlier) = earlier_point {
            let progress_change = latest.bonding_curve_progress - earlier.bonding_curve_progress;
            let time_diff_mins = latest.timestamp
                .signed_duration_since(earlier.timestamp)
                .num_seconds() as f64 / 60.0;
            
            if time_diff_mins > 0.0 {
                Some(progress_change / time_diff_mins) // %/min
            } else {
                Some(0.0)
            }
        } else {
            Some(0.0)
        }
    }
    
    /// 2. Volume & Price Velocity
    /// Volume Surge: v_vol(t) = (V(t) - V(t-Δt)) / Δt
    /// Price Momentum: v_price(t) = ln(P(t)) - ln(P(t-Δt)) / Δt
    pub fn calculate_volume_velocity(&self, window: &SlidingWindow) -> Option<f64> {
        let events = &window.events;
        if events.len() < 2 {
            return None;
        }
        
        // Calculate cumulative volume in last 30 seconds vs previous 30 seconds
        let latest_time = events.back()?.timestamp;
        let mut recent_volume = 0.0;
        let mut earlier_volume = 0.0;
        
        for event in events.iter().rev() {
            let time_diff = latest_time.signed_duration_since(event.timestamp);
            if time_diff.num_seconds() <= 30 {
                recent_volume += event.volume_sol;
            } else if time_diff.num_seconds() <= 60 {
                earlier_volume += event.volume_sol;
            }
        }
        
        let volume_change = recent_volume - earlier_volume;
        let time_interval_secs = 30.0;
        
        Some(volume_change / time_interval_secs) // SOL/sec
    }
    
    pub fn calculate_price_velocity(&self, window: &SlidingWindow) -> Option<f64> {
        let events = &window.events;
        if events.len() < 2 {
            return None;
        }
        
        let latest = events.back()?;
        let mut earlier_point = None;
        
        // Find point 30 seconds ago
        for event in events.iter().rev() {
            let time_diff = latest.timestamp.signed_duration_since(event.timestamp);
            if time_diff.num_seconds() >= 30 {
                earlier_point = Some(event);
                break;
            }
        }
        
        if let Some(earlier) = earlier_point {
            let latest_price = latest.market_cap_sol;
            let earlier_price = earlier.market_cap_sol;
            
            if latest_price > 0.0 && earlier_price > 0.0 {
                let log_return = (latest_price / earlier_price).ln();
                let time_diff_secs = latest.timestamp
                    .signed_duration_since(earlier.timestamp)
                    .num_seconds() as f64;
                
                if time_diff_secs > 0.0 {
                    Some(log_return / time_diff_secs) // log-return per second
                } else {
                    Some(0.0)
                }
            } else {
                Some(0.0)
            }
        } else {
            Some(0.0)
        }
    }
}