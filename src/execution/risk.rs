/// Risk management for trading positions

use super::Position;
use crate::config::Config;

pub struct RiskManager {
    config: Config,
}

impl RiskManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    pub fn should_stop_loss(&self, position: &Position) -> bool {
        // USD-based stop loss: exit if we've lost more than max_loss_usd
        let loss_usd = position.entry_usd - position.current_value_usd;
        let max_loss = self.config.risk_management.max_loss_usd;
        
        if loss_usd >= max_loss {
            println!("🛑 USD stop loss triggered: Lost ${:.2} (max allowed: ${:.2})", 
                    loss_usd, max_loss);
            true
        } else {
            false
        }
    }
    
    pub fn should_take_profit(&self, position: &Position) -> Option<f64> {
        let entry_usd = position.entry_usd;
        let current_value_usd = position.current_value_usd;
        let profit_usd = current_value_usd - entry_usd;
        let age_minutes = position.get_age_hours() * 60.0;
        let multiplier = current_value_usd / entry_usd;
        
        // Rule 1: MINIMUM HOLD TIME - Never exit before min_hold_minutes
        if age_minutes < self.config.risk_management.min_hold_minutes {
            println!("⏰ Minimum hold time not reached: {:.1}m (min: {:.1}m) - HOLDING", 
                    age_minutes, self.config.risk_management.min_hold_minutes);
            return None;
        }
        
        // Rule 2: TRAILING STOP - Exit if drawdown from peak exceeds threshold
        // Note: peak_price is already in USD from Jupiter API, no conversion needed
        let peak_value_usd = position.peak_price * position.tokens_held;
        let drawdown_from_peak_usd = peak_value_usd - current_value_usd;
        let drawdown_percent = if peak_value_usd > 0.0 {
            (drawdown_from_peak_usd / peak_value_usd) * 100.0
        } else {
            0.0
        };
        
        if drawdown_percent >= self.config.risk_management.trailing_stop_percent {
            println!("📉 Trailing stop triggered: {:.1}% drawdown from peak (${:.2} → ${:.2})", 
                    drawdown_percent, peak_value_usd, current_value_usd);
            return Some(100.0);
        }
        
        // Rule 3: FINAL TARGET - Exit everything at final multiplier
        if multiplier >= self.config.risk_management.final_target_multiplier {
            println!("🚀 FINAL target reached: {:.0}x (${:.2} → ${:.2}) - Taking ALL profit", 
                    multiplier, entry_usd, current_value_usd);
            return Some(100.0);
        }
        
        // Rule 4: MULTI-STAGE PROFIT TAKING
        // Third stage: 50x target
        if multiplier >= self.config.risk_management.profit_take_third_multiplier {
            println!("💎 Third target {:.0}x reached: Taking {:.0}% profit", 
                    self.config.risk_management.profit_take_third_multiplier,
                    self.config.risk_management.profit_take_third_percent);
            return Some(self.config.risk_management.profit_take_third_percent);
        }
        
        // Second stage: 10x target  
        if multiplier >= self.config.risk_management.profit_take_second_multiplier {
            println!("💰 Second target {:.0}x reached: Taking {:.0}% profit", 
                    self.config.risk_management.profit_take_second_multiplier,
                    self.config.risk_management.profit_take_second_percent);
            return Some(self.config.risk_management.profit_take_second_percent);
        }
        
        // First stage: 3x target
        if multiplier >= self.config.risk_management.profit_take_first_multiplier {
            println!("✅ First target {:.0}x reached: Taking {:.0}% profit", 
                    self.config.risk_management.profit_take_first_multiplier,
                    self.config.risk_management.profit_take_first_percent);
            return Some(self.config.risk_management.profit_take_first_percent);
        }
        
        // HOLD - Still building position
        None
    }
    
    pub fn should_force_exit(&self, position: &Position) -> bool {
        let age_hours = position.get_age_hours();
        let time_limit = self.config.strategy.time_horizon_hours;
        
        // Force exit if position exceeds time horizon or risk management limit
        age_hours >= time_limit || age_hours >= self.config.risk_management.force_exit_hours
    }
    
    pub fn get_position_size(&self, position_type: &str) -> f64 {
        self.config.get_position_size_usd(position_type)
    }
    
    pub fn can_open_position(&self, current_positions: usize) -> bool {
        current_positions < self.config.allocation.max_positions
    }
    
    pub fn validate_entry(&self, virality_score: f64, bonding_progress: f64, rug_score: f64) -> bool {
        let entry = &self.config.entry_criteria;
        
        // Check virality score if configured
        if let Some(min_virality) = entry.min_virality_score {
            if virality_score < min_virality {
                return false;
            }
        }
        
        // Check bonding curve progress if configured
        if let Some(max_bonding) = entry.max_bonding_curve_progress {
            if bonding_progress > max_bonding {
                return false;
            }
        }
        
        // Check rug score if configured
        if let Some(min_rug) = entry.min_rug_score {
            if rug_score < min_rug {
                return false;
            }
        }
        
        true
    }
    
    pub fn get_max_loss_per_position(&self) -> f64 {
        self.config.risk_management.max_loss_per_position_percent
    }
    
    pub fn should_emergency_exit(&self, position: &Position) -> bool {
        let max_loss = self.get_max_loss_per_position();
        let current_loss = -position.get_pnl_percent(); // Negative PnL = loss
        
        current_loss >= max_loss
    }
}