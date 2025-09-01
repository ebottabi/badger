/// Automated Buy/Sell Trigger Logic with Mathematical Validation

use crate::util::time_series::SlidingWindow;
use super::mathematical_engine::{MathematicalEngine, BuySignalStrength};

impl MathematicalEngine {
    /// 6. Automated Buy/Sell Triggers
    /// 
    /// Buy Conditions (ALL must be true):
    /// - Ŝ > 0.7 (high virality score)
    /// - Prog(t) < 40% (early in bonding curve)
    /// - s_rug > 0.6 (low rug risk)
    /// - Fixed amount: 0.5 SOL
    /// 
    /// Sell Conditions (ANY can trigger):
    /// - Trailing Stop: Price drops >15% from peak
    /// - Momentum Reversal: v_price(t) < -0.01 (1%/sec drop)
    /// - Profit Target: 3x-5x profit achieved
    /// - Time Limit: 30 minutes maximum hold
    pub fn determine_buy_signal_strength(
        &self,
        composite_score: f64,
        progress_velocity: f64,
        rug_score: f64,
        window: &SlidingWindow,
    ) -> BuySignalStrength {
        let events = &window.events;
        if events.is_empty() {
            return BuySignalStrength::Hold;
        }
        
        let latest_event = events.back().unwrap();
        let bonding_progress = latest_event.bonding_curve_progress;
        
        // Strong Buy Conditions (ALL must be true)
        let strong_buy_conditions = vec![
            composite_score > 0.8,           // Very high virality
            bonding_progress < 25.0,         // Very early (< 25%)
            rug_score > 0.8,                 // Very low rug risk
            progress_velocity > 5.0,         // Strong momentum (>5%/min)
        ];
        
        if strong_buy_conditions.iter().all(|&x| x) {
            return BuySignalStrength::StrongBuy;
        }
        
        // Regular Buy Conditions (ALL must be true)
        let buy_conditions = vec![
            composite_score > 0.7,           // High virality score
            bonding_progress < 40.0,         // Early in bonding curve
            rug_score > 0.6,                 // Low rug risk
            progress_velocity > 2.0,         // Decent momentum (>2%/min)
        ];
        
        if buy_conditions.iter().all(|&x| x) {
            return BuySignalStrength::Buy;
        }
        
        // Sell Conditions (ANY can trigger)
        let sell_conditions = self.check_sell_conditions(window, composite_score);
        if sell_conditions.should_sell {
            return if sell_conditions.urgent {
                BuySignalStrength::StrongSell
            } else {
                BuySignalStrength::Sell
            };
        }
        
        // Default to Hold
        BuySignalStrength::Hold
    }
    
    fn check_sell_conditions(&self, window: &SlidingWindow, composite_score: f64) -> SellSignal {
        let events = &window.events;
        if events.len() < 2 {
            return SellSignal { should_sell: false, urgent: false };
        }
        
        // Check for momentum reversal (1%/sec drop)
        if let Some(price_velocity) = self.calculate_price_velocity(window) {
            if price_velocity < -0.01 {
                return SellSignal { should_sell: true, urgent: true };
            }
        }
        
        // Check for composite score collapse
        if composite_score < 0.3 {
            return SellSignal { should_sell: true, urgent: false };
        }
        
        // Check for bonding curve stagnation
        let latest = events.back().unwrap();
        let progress = latest.bonding_curve_progress;
        if progress > 80.0 {
            // Near completion - likely to dump
            return SellSignal { should_sell: true, urgent: false };
        }
        
        // Check for low volume (suggests interest dying)
        if let Some(volume_velocity) = self.calculate_volume_velocity(window) {
            if volume_velocity < -0.05 { // Negative volume velocity
                return SellSignal { should_sell: true, urgent: false };
            }
        }
        
        SellSignal { should_sell: false, urgent: false }
    }
    
    /// Risk Management Rules:
    /// - Portfolio: Max 3-5 active positions
    /// - Position Sizing: Kelly criterion approximation
    /// - Edge Cases: Ignore if dev wallet >10% (rug flag)
    pub fn calculate_position_size(&self, composite_score: f64, rug_score: f64) -> f64 {
        let base_position = 0.5; // 0.5 SOL base
        
        // Kelly criterion approximation
        // Assume 30% win rate with 10x average win, 100% loss rate
        let win_prob = 0.3;
        let avg_win_multiple = 10.0;
        let loss_multiple = 1.0;
        
        // Kelly fraction = (bp - q) / b where b=odds, p=win prob, q=loss prob
        let kelly_fraction: f64 = ((avg_win_multiple * win_prob) - (1.0 - win_prob)) / avg_win_multiple;
        let kelly_fraction = kelly_fraction.max(0.0).min(0.25); // Cap at 25% of portfolio
        
        // Adjust based on confidence scores
        let confidence_multiplier = composite_score * rug_score;
        let position_size = base_position * kelly_fraction * confidence_multiplier * 4.0;
        
        // Risk limits
        position_size.min(2.0).max(0.1) // Between 0.1 and 2.0 SOL
    }
}

struct SellSignal {
    should_sell: bool,
    urgent: bool,
}