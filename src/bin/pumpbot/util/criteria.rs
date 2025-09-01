/// Buy signal criteria and filtering logic

use crate::client::event_parser::UniversalPumpEvent;

pub struct InstantBuyCriteria {
    pub min_market_cap_sol: f64,
    pub min_initial_buy_sol: f64,
    pub max_token_age_minutes: i64,
    pub min_whale_buy_sol: f64,
}

impl Default for InstantBuyCriteria {
    fn default() -> Self {
        Self {
            min_market_cap_sol: 30.0,      // 30 SOL minimum market cap
            min_initial_buy_sol: 3.0,      // 3 SOL minimum initial buy
            max_token_age_minutes: 5,      // Only tokens <5 minutes old
            min_whale_buy_sol: 8.0,        // 8 SOL+ considered whale buy
        }
    }
}

impl InstantBuyCriteria {
    pub fn is_instant_buy_signal(&self, event: &UniversalPumpEvent) -> bool {
        let market_cap = event.market_cap_sol.unwrap_or(0.0);
        let initial_buy = event.sol_amount.unwrap_or(0.0);
        
        market_cap >= self.min_market_cap_sol &&
        initial_buy >= self.min_initial_buy_sol &&
        !self.is_suspicious_token(event)
    }
    
    pub fn is_whale_buy(&self, event: &UniversalPumpEvent) -> bool {
        event.sol_amount.unwrap_or(0.0) >= self.min_whale_buy_sol
    }
    
    fn is_suspicious_token(&self, event: &UniversalPumpEvent) -> bool {
        let name_lower = event.name.to_lowercase();
        let symbol_lower = event.symbol.to_lowercase();
        
        // Flag obvious scam patterns
        name_lower.contains("elon") || 
        name_lower.contains("trump") ||
        symbol_lower.len() > 10 ||
        event.name.chars().any(|c| !c.is_ascii())
    }
}