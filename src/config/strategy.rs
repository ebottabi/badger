/// Strategy configuration structures

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub strategy: Strategy,
    pub allocation: Allocation,
    pub entry_criteria: Entry,
    pub risk_management: Risk,
    pub trading: Trading,
    pub wallet: Wallet,
    pub verification_links: Links,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Strategy {
    pub total_capital_usd: f64,
    pub target_multiplier: f64,
    pub time_horizon_hours: f64,
    pub allocation_strategy: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Allocation {
    pub main_position_percent: f64,
    pub secondary_position_percent: f64,
    pub reserve_percent: f64,
    pub max_positions: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Entry {
    // Legacy criteria (for backward compatibility)
    pub min_virality_score: Option<f64>,
    pub max_bonding_curve_progress: Option<f64>,
    pub min_rug_score: Option<f64>,
    pub min_progress_velocity: Option<f64>,
    pub signal_timeout_seconds: Option<u64>,
    pub max_token_age_minutes: Option<i64>,
    
    // Momentum-based criteria
    pub min_volume_spike_percent: Option<f64>,
    pub min_price_momentum_percent: Option<f64>,
    pub min_trade_count_hour: Option<f64>,
    pub min_unique_buyers_hour: Option<f64>,
    pub min_token_age_hours: Option<u64>,
    pub max_token_age_hours: Option<u64>,
    pub momentum_window_minutes: Option<u64>,
    
    // Common criteria
    pub min_market_cap_usd: f64,
    pub max_market_cap_usd: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Risk {
    pub max_loss_per_position_percent: f64,
    pub trailing_stop_percent: f64,
    pub max_loss_usd: f64,
    pub profit_take_first_percent: f64,
    pub profit_take_first_multiplier: f64,
    pub profit_take_second_percent: f64,
    pub profit_take_second_multiplier: f64,
    pub profit_take_third_percent: f64,
    pub profit_take_third_multiplier: f64,
    pub final_target_multiplier: f64,
    pub force_exit_hours: f64,
    pub min_hold_minutes: f64,
    pub momentum_exit_threshold: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Trading {
    pub slippage_tolerance_percent: f64,
    pub max_retry_attempts: u32,
    pub execution_delay_ms: u64,
    pub priority_fee_sol: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Wallet {
    pub public_key: String,
    pub pump_api_key: String,
    pub birdeye_api_key: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Links {
    pub dex_screener_base: String,
    pub rug_check_base: String,
    pub pump_fun_base: String,
}

impl Config {
    pub fn load_from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }
    
    pub fn get_position_size_usd(&self, position_type: &str) -> f64 {
        let percent = match position_type {
            "main" => self.allocation.main_position_percent,
            "secondary" => self.allocation.secondary_position_percent,
            "reserve" => self.allocation.reserve_percent,
            _ => 10.0,
        };
        self.strategy.total_capital_usd * percent / 100.0
    }
}