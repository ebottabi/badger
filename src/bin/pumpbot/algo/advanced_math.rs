/// Advanced mathematical models: HHI, Growth Prediction, Composite Scoring

use std::collections::HashMap;
use crate::util::time_series::{SlidingWindow, TimeSeriesPoint};
use super::mathematical_engine::{MathematicalEngine, BuySignalStrength};

impl MathematicalEngine {
    /// 3. Holder Distribution Score (Rug Risk using HHI)
    /// Formula: HHI = Σ(balance_i / total_tokens)² for all holders
    /// Rug Score: s_rug = 1 - (HHI-1/H)/(1-1/H) (0-1, low=diversified, high=rug risk)
    pub fn calculate_holder_distribution_score(&self, window: &SlidingWindow) -> f64 {
        // Simplified implementation - in real version would need RPC calls
        // For now, estimate based on unique traders and trade patterns
        let events = &window.events;
        let unique_traders: std::collections::HashSet<_> = 
            events.iter().map(|e| &e.trader).collect();
        
        let trader_count = unique_traders.len() as f64;
        let total_trades = events.len() as f64;
        
        if trader_count == 0.0 || total_trades == 0.0 {
            return 0.0; // High rug risk if no data
        }
        
        // Simple heuristic: more unique traders per trade = better distribution
        let diversity_ratio = trader_count / total_trades;
        
        // Convert to 0-1 score where 1 = good distribution, 0 = concentrated
        let distribution_score = (diversity_ratio * 2.0).min(1.0);
        
        // Additional check: large single transactions indicate concentration
        let max_trade = events.iter()
            .map(|e| e.volume_sol)
            .fold(0.0, f64::max);
        let avg_trade = events.iter()
            .map(|e| e.volume_sol)
            .sum::<f64>() / total_trades;
        
        if avg_trade > 0.0 {
            let concentration_penalty = (max_trade / avg_trade / 10.0).min(0.5);
            (distribution_score - concentration_penalty).max(0.0)
        } else {
            distribution_score
        }
    }
    
    /// 4. Predictive Growth Model
    /// Exponential Fit: MC(t) = MC₀ · e^(r·t) where r = growth rate
    /// 5-min Projection: MC_proj = MC(t) · e^(r·300)
    /// Score: s_growth = min(1, ln(MC_proj/MC(t)) / ln(5)) for 5x threshold
    pub fn calculate_predictive_growth_score(&self, window: &SlidingWindow) -> Option<f64> {
        let events = &window.events;
        if events.len() < 5 {
            return Some(0.0);
        }
        
        // Get last 5 data points for exponential fitting
        let recent_events: Vec<_> = events.iter().rev().take(5).collect();
        if recent_events.len() < 5 {
            return Some(0.0);
        }
        
        // Simple linear regression on log(market_cap) to estimate growth rate
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_xy = 0.0;
        let mut sum_x2 = 0.0;
        let n = recent_events.len() as f64;
        
        let start_time = recent_events.last().unwrap().timestamp;
        
        for (i, event) in recent_events.iter().enumerate() {
            if event.market_cap_sol > 0.0 {
                let x = i as f64; // Time index
                let y = event.market_cap_sol.ln(); // Log market cap
                
                sum_x += x;
                sum_y += y;
                sum_xy += x * y;
                sum_x2 += x * x;
            }
        }
        
        // Linear regression slope = growth rate
        let denominator = n * sum_x2 - sum_x * sum_x;
        if denominator.abs() < 1e-10 {
            return Some(0.0);
        }
        
        let growth_rate = (n * sum_xy - sum_x * sum_y) / denominator;
        
        // Project 5 minutes (300 seconds) ahead
        let current_mc = recent_events[0].market_cap_sol;
        if current_mc <= 0.0 {
            return Some(0.0);
        }
        
        let projected_mc = current_mc * (growth_rate * 5.0).exp(); // 5 data points ~= 5 minutes
        let growth_multiple = projected_mc / current_mc;
        
        // Score based on 5x threshold
        if growth_multiple > 1.0 {
            let score = (growth_multiple.ln() / 5.0_f64.ln()).min(1.0);
            Some(score.max(0.0))
        } else {
            Some(0.0)
        }
    }
    
    /// 5. Composite Virality Score
    /// Formula: S(t) = w₁v_prog + w₂v_vol + w₃v_price + w₄v_social + w₅s_growth - w₆(1-s_rug)
    /// Normalization: Sigmoid function Ŝ = 1/(1+e^(-k(S-μ))) with k=5, μ=0.5
    pub fn calculate_composite_virality_score(
        &self,
        progress_velocity: f64,
        volume_velocity: f64,
        price_velocity: f64,
        social_velocity: f64, // Placeholder
        growth_score: f64,
        rug_score: f64,
    ) -> f64 {
        let weights = &self.weights;
        
        // Normalize components to 0-1 range
        let norm_progress = (progress_velocity / 10.0).min(1.0).max(0.0); // 10%/min max
        let norm_volume = (volume_velocity / self.historical_std_dev).min(1.0).max(0.0);
        let norm_price = (price_velocity * 1000.0).min(1.0).max(0.0); // Scale log-returns
        let norm_social = social_velocity.min(1.0).max(0.0);
        let norm_growth = growth_score.min(1.0).max(0.0);
        let norm_rug_penalty = (1.0 - rug_score).min(1.0).max(0.0);
        
        // Weighted combination
        let raw_score = weights.progress * norm_progress +
                       weights.volume * norm_volume +
                       weights.price * norm_price +
                       weights.social * norm_social +
                       weights.growth * norm_growth -
                       weights.rug_penalty * norm_rug_penalty;
        
        // Sigmoid normalization: Ŝ = 1/(1+e^(-k(S-μ))) with k=5, μ=0.5
        let k = 5.0;
        let mu = 0.5;
        let sigmoid_score = 1.0 / (1.0 + (-k * (raw_score - mu)).exp());
        
        sigmoid_score.min(1.0).max(0.0)
    }
}