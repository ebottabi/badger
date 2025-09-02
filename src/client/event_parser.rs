/// Event parsing for universal pump.fun formats

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct UniversalPumpEvent {
    // Core fields (all formats)
    pub mint: String,
    pub name: String,
    pub symbol: String,
    
    #[serde(alias = "traderPublicKey", alias = "trader_public_key")]
    pub trader_public_key: String,
    
    #[serde(alias = "txType", alias = "tx_type")]
    pub tx_type: String,
    
    // Optional fields (format-dependent)
    pub pool: Option<String>,
    
    #[serde(alias = "solAmount", alias = "sol_amount")]
    pub sol_amount: Option<f64>,
    
    #[serde(alias = "marketCapSol", alias = "market_cap_sol")]
    pub market_cap_sol: Option<f64>,
    
    #[serde(alias = "initialBuy", alias = "initial_buy")]
    pub initial_buy: Option<f64>,
    
    pub signature: Option<String>,
    pub uri: Option<String>,
    
    // Pump format specific
    #[serde(alias = "vSolInBondingCurve", alias = "v_sol_in_bonding_curve")]
    pub v_sol_in_bonding_curve: Option<f64>,
    
    #[serde(alias = "vTokensInBondingCurve", alias = "v_tokens_in_bonding_curve")]
    pub v_tokens_in_bonding_curve: Option<f64>,
    
    // Bonk format specific
    #[serde(alias = "solInPool", alias = "sol_in_pool")]
    pub sol_in_pool: Option<f64>,
    
    #[serde(alias = "tokensInPool", alias = "tokens_in_pool")]
    pub tokens_in_pool: Option<f64>,
    
    #[serde(alias = "newTokenBalance", alias = "new_token_balance")]
    pub new_token_balance: Option<f64>,
    
    // Additional fields for other formats
    #[serde(alias = "bondingCurveKey", alias = "bonding_curve_key")]
    pub bonding_curve_key: Option<String>,
}

pub fn parse_pump_event(message: &str) -> Result<UniversalPumpEvent, serde_json::Error> {
    serde_json::from_str::<UniversalPumpEvent>(message)
}

pub fn is_subscription_success(message: &str) -> Option<String> {
    if message.contains("Successfully subscribed") {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(message) {
            return json.get("message").and_then(|m| m.as_str()).map(|s| s.to_string());
        }
    }
    None
}