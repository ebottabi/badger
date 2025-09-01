/// Mathematical Engine for Advanced Pump Analysis
/// 
/// Implements sophisticated mathematical models for viral token detection:
/// - Progress Velocity (bonding curve momentum)
/// - Volume & Price Velocity with log-returns
/// - Holder Distribution Score (HHI)
/// - Predictive Growth Model
/// - Composite Virality Score

use std::collections::HashMap;
use crate::util::time_series::{SlidingWindow, TimeSeriesPoint};

#[derive(Debug, Clone)]
pub struct MathematicalAnalysis {
    pub progress_velocity: f64,         // %/min bonding curve fill rate
    pub volume_velocity: f64,           // SOL/sec surge velocity
    pub price_velocity: f64,            // log-return velocity (exponential growth)
    pub holder_distribution_score: f64,  // HHI-based rug risk (0-1, low=safe)
    pub predictive_growth_score: f64,   // 5-min projection score (0-1)
    pub composite_virality_score: f64,  // Weighted combination (0-1)
    pub buy_signal_strength: BuySignalStrength,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BuySignalStrength {
    StrongBuy,      // All conditions perfect
    Buy,            // Good conditions 
    Hold,           // Neutral/waiting
    Sell,           // Negative momentum
    StrongSell,     // Major red flags
}

pub struct MathematicalEngine {
    pub weights: ViralityWeights,
    pub historical_std_dev: f64,  // For normalization (0.1 SOL/sec typical)
}

#[derive(Debug, Clone)]
pub struct ViralityWeights {
    pub progress: f64,    // w1: 0.25
    pub volume: f64,      // w2: 0.20  
    pub price: f64,       // w3: 0.15
    pub social: f64,      // w4: 0.20 (placeholder)
    pub growth: f64,      // w5: 0.15
    pub rug_penalty: f64, // w6: 0.05
}

impl Default for ViralityWeights {
    fn default() -> Self {
        Self {
            progress: 0.25,
            volume: 0.20,
            price: 0.15,
            social: 0.20,
            growth: 0.15,
            rug_penalty: 0.05,
        }
    }
}

impl MathematicalEngine {
    pub fn new() -> Self {
        Self {
            weights: ViralityWeights::default(),
            historical_std_dev: 0.1, // 0.1 SOL/sec for hot tokens
        }
    }
    
    pub fn analyze(&self, window: &SlidingWindow) -> Option<MathematicalAnalysis> {
        if window.events.len() < 3 {
            return None;
        }
        
        let progress_velocity = self.calculate_progress_velocity(window)?;
        let volume_velocity = self.calculate_volume_velocity(window)?;
        let price_velocity = self.calculate_price_velocity(window)?;
        let holder_score = self.calculate_holder_distribution_score(window);
        let growth_score = self.calculate_predictive_growth_score(window)?;
        
        let composite_score = self.calculate_composite_virality_score(
            progress_velocity,
            volume_velocity,
            price_velocity,
            0.0, // social placeholder
            growth_score,
            holder_score,
        );
        
        let signal_strength = self.determine_buy_signal_strength(
            composite_score,
            progress_velocity,
            holder_score,
            window,
        );
        
        Some(MathematicalAnalysis {
            progress_velocity,
            volume_velocity,
            price_velocity,
            holder_distribution_score: holder_score,
            predictive_growth_score: growth_score,
            composite_virality_score: composite_score,
            buy_signal_strength: signal_strength,
        })
    }
}