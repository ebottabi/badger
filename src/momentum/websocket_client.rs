/// PumpPortal WebSocket client for real-time momentum tracking

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use anyhow::Result;

const PUMPPORTAL_WEBSOCKET: &str = "wss://pumpportal.fun/api/data";

#[derive(Debug, Clone, Deserialize)]
pub struct TokenTrade {
    pub signature: String,
    pub mint: String,
    pub sol_amount: f64,
    pub token_amount: f64,
    pub is_buy: bool,
    pub user: String,
    pub timestamp: u64,
    pub virtual_sol_reserves: Option<f64>,
    pub virtual_token_reserves: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenCreation {
    pub signature: String,
    pub mint: String,
    #[serde(rename = "traderPublicKey")]
    pub trader_public_key: String,
    #[serde(rename = "txType")]
    pub tx_type: String,
    #[serde(rename = "initialBuy")]
    pub initial_buy: f64,
    #[serde(rename = "solAmount")]
    pub sol_amount: f64,
    #[serde(rename = "bondingCurveKey")]
    pub bonding_curve_key: String,
    #[serde(rename = "vTokensInBondingCurve")]
    pub v_tokens_in_bonding_curve: f64,
    #[serde(rename = "vSolInBondingCurve")]
    pub v_sol_in_bonding_curve: f64,
    #[serde(rename = "marketCapSol")]
    pub market_cap_sol: f64,
    pub name: String,
    pub symbol: String,
    pub uri: String,
    pub pool: String,
}

#[derive(Debug, Clone)]
pub struct VolumeMetrics {
    pub trades_1h: Vec<TokenTrade>,
    pub volume_sol_1h: f64,
    pub unique_traders_1h: usize,
    pub buy_volume_sol_1h: f64,
    pub sell_volume_sol_1h: f64,
    pub last_price_sol: f64,
    pub price_change_1h_percent: f64,
    pub last_updated: SystemTime,
}

impl VolumeMetrics {
    pub fn new() -> Self {
        Self {
            trades_1h: Vec::new(),
            volume_sol_1h: 0.0,
            unique_traders_1h: 0,
            buy_volume_sol_1h: 0.0,
            sell_volume_sol_1h: 0.0,
            last_price_sol: 0.0,
            price_change_1h_percent: 0.0,
            last_updated: SystemTime::now(),
        }
    }
}

pub struct MomentumTracker {
    token_metrics: Arc<Mutex<HashMap<String, VolumeMetrics>>>,
    momentum_watchlist: Arc<Mutex<HashMap<String, TokenCreation>>>, // Track promising tokens
    ws_sender: Option<futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>>,
}

impl MomentumTracker {
    pub fn new() -> Self {
        Self {
            token_metrics: Arc::new(Mutex::new(HashMap::new())),
            momentum_watchlist: Arc::new(Mutex::new(HashMap::new())),
            ws_sender: None,
        }
    }
    
    pub async fn connect(&mut self) -> Result<()> {
        println!("🔌 Connecting to PumpPortal WebSocket: {}", PUMPPORTAL_WEBSOCKET);
        
        let (ws_stream, _response) = connect_async(PUMPPORTAL_WEBSOCKET).await?;
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        
        // Subscribe to new token creation to discover tokens, then monitor them for momentum
        // (We use creation events to build our watchlist, not to trade immediately)
        let subscribe_new_tokens = serde_json::json!({
            "method": "subscribeNewToken"
        });
        
        ws_sender.send(Message::Text(subscribe_new_tokens.to_string())).await?;
        println!("✅ Subscribed to new tokens (for building momentum watchlist)");
        
        // Store sender for future use
        self.ws_sender = Some(ws_sender);
        
        // Spawn background task to handle incoming messages
        let metrics_clone = Arc::clone(&self.token_metrics);
        tokio::spawn(async move {
            while let Some(msg) = ws_receiver.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        // RAW DATA DUMP - Show everything we receive
                        println!("🔍 RAW WebSocket Data: {}", text);
                        
                        // Skip subscription confirmation and method messages
                        if text.contains("Successfully subscribed") || text.starts_with("{\"method\"") {
                            println!("⏩ Skipping system message");
                            continue;
                        }
                        
                        // Parse token creation events to build our momentum watchlist
                        if let Ok(token_creation) = serde_json::from_str::<TokenCreation>(&text) {
                            // Only add tokens with decent initial buy and market cap to watchlist
                            if token_creation.sol_amount >= 1.0 && token_creation.market_cap_sol >= 30.0 {
                                println!("👀 Adding to watchlist: {} ({}) - {:.1} SOL initial, {:.1} SOL mcap", 
                                        token_creation.name, token_creation.symbol, 
                                        token_creation.sol_amount, token_creation.market_cap_sol);
                                Self::add_to_momentum_watchlist(metrics_clone.clone(), token_creation).await;
                            }
                        } else if let Ok(trade) = serde_json::from_str::<TokenTrade>(&text) {
                            println!("✅ Successfully parsed trade for token: {}", trade.mint);
                            Self::process_trade_update(metrics_clone.clone(), trade).await;
                        } else {
                            println!("❌ Failed to parse as TokenCreation or TokenTrade: {}", text);
                        }
                    }
                    Ok(Message::Ping(ping)) => {
                        // Handle ping frames - respond with pong
                        println!("📡 Received ping, responding with pong");
                    }
                    Ok(Message::Close(_)) => {
                        println!("🔌 WebSocket connection closed");
                        break;
                    }
                    Err(e) => {
                        println!("❌ WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });
        
        // Spawn cleanup task to remove old trades every minute
        let metrics_cleanup = Arc::clone(&self.token_metrics);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                Self::cleanup_old_trades(metrics_cleanup.clone()).await;
            }
        });
        
        Ok(())
    }
    
    async fn add_to_momentum_watchlist(metrics: Arc<Mutex<HashMap<String, VolumeMetrics>>>, token_creation: TokenCreation) {
        // Add token to watchlist for momentum monitoring (separate from metrics)
        // This builds our list of tokens to monitor via external APIs for trading activity
        
        // Initialize basic metrics for the token
        let mut metrics_map = metrics.lock().unwrap();
        if !metrics_map.contains_key(&token_creation.mint) {
            let mut initial_metrics = VolumeMetrics::new();
            initial_metrics.last_price_sol = token_creation.market_cap_sol / 1_000_000_000.0; // Rough price estimate
            metrics_map.insert(token_creation.mint.clone(), initial_metrics);
        }
    }
    
    async fn process_trade_update(metrics: Arc<Mutex<HashMap<String, VolumeMetrics>>>, trade: TokenTrade) {
        let mut metrics_map = metrics.lock().unwrap();
        let token_metrics = metrics_map.entry(trade.mint.clone()).or_insert_with(VolumeMetrics::new);
        
        // Add trade to 1-hour window
        token_metrics.trades_1h.push(trade.clone());
        
        // Update volume metrics
        token_metrics.volume_sol_1h += trade.sol_amount;
        if trade.is_buy {
            token_metrics.buy_volume_sol_1h += trade.sol_amount;
        } else {
            token_metrics.sell_volume_sol_1h += trade.sol_amount;
        }
        
        // Update price tracking
        if let Some(virtual_sol) = trade.virtual_sol_reserves {
            if let Some(virtual_tokens) = trade.virtual_token_reserves {
                if virtual_tokens > 0.0 {
                    let old_price = token_metrics.last_price_sol;
                    token_metrics.last_price_sol = virtual_sol / virtual_tokens;
                    
                    // Calculate 1-hour price change
                    if old_price > 0.0 {
                        token_metrics.price_change_1h_percent = 
                            ((token_metrics.last_price_sol - old_price) / old_price) * 100.0;
                    }
                }
            }
        }
        
        // Update unique trader count
        let unique_traders: std::collections::HashSet<String> = 
            token_metrics.trades_1h.iter().map(|t| t.user.clone()).collect();
        token_metrics.unique_traders_1h = unique_traders.len();
        
        token_metrics.last_updated = SystemTime::now();
    }
    
    async fn cleanup_old_trades(metrics: Arc<Mutex<HashMap<String, VolumeMetrics>>>) {
        let one_hour_ago = SystemTime::now() - Duration::from_secs(3600);
        
        let mut metrics_map = metrics.lock().unwrap();
        for (mint, token_metrics) in metrics_map.iter_mut() {
            // Remove trades older than 1 hour
            token_metrics.trades_1h.retain(|trade| {
                let trade_time = UNIX_EPOCH + Duration::from_secs(trade.timestamp);
                trade_time > one_hour_ago
            });
            
            // Recalculate metrics after cleanup
            token_metrics.volume_sol_1h = token_metrics.trades_1h.iter().map(|t| t.sol_amount).sum();
            token_metrics.buy_volume_sol_1h = token_metrics.trades_1h.iter()
                .filter(|t| t.is_buy)
                .map(|t| t.sol_amount)
                .sum();
            token_metrics.sell_volume_sol_1h = token_metrics.trades_1h.iter()
                .filter(|t| !t.is_buy)
                .map(|t| t.sol_amount)
                .sum();
            
            let unique_traders: std::collections::HashSet<String> = 
                token_metrics.trades_1h.iter().map(|t| t.user.clone()).collect();
            token_metrics.unique_traders_1h = unique_traders.len();
        }
        
        // Remove tokens with no recent activity
        metrics_map.retain(|mint, token_metrics| {
            if token_metrics.trades_1h.is_empty() {
                println!("🧹 Removing inactive token from tracking: {}", mint);
                false
            } else {
                true
            }
        });
    }
    
    pub fn get_momentum_candidates(&self, min_volume_spike: f64, min_trades: usize, min_unique_buyers: usize) -> Vec<(String, VolumeMetrics)> {
        let metrics_map = self.token_metrics.lock().unwrap();
        let mut candidates = Vec::new();
        
        for (mint, metrics) in metrics_map.iter() {
            // Check volume spike criteria
            if metrics.volume_sol_1h < min_volume_spike {
                continue;
            }
            
            // Check trade count
            if metrics.trades_1h.len() < min_trades {
                continue;
            }
            
            // Check unique buyer count
            if metrics.unique_traders_1h < min_unique_buyers {
                continue;
            }
            
            // Check price momentum (positive)
            if metrics.price_change_1h_percent < 15.0 {
                continue;
            }
            
            candidates.push((mint.clone(), metrics.clone()));
        }
        
        // Sort by volume (highest first)
        candidates.sort_by(|a, b| b.1.volume_sol_1h.partial_cmp(&a.1.volume_sol_1h).unwrap());
        
        candidates
    }
    
    pub fn get_token_metrics(&self, mint: &str) -> Option<VolumeMetrics> {
        let metrics_map = self.token_metrics.lock().unwrap();
        metrics_map.get(mint).cloned()
    }
    
    pub fn print_momentum_summary(&self) {
        let metrics_map = self.token_metrics.lock().unwrap();
        let active_tokens = metrics_map.len();
        
        println!("\n📊 MOMENTUM TRACKER SUMMARY");
        println!("═══════════════════════════════════════");
        println!("🎯 Active tokens tracked: {}", active_tokens);
        
        if active_tokens > 0 {
            let total_volume: f64 = metrics_map.values().map(|m| m.volume_sol_1h).sum();
            let total_trades: usize = metrics_map.values().map(|m| m.trades_1h.len()).sum();
            
            println!("💰 Total volume (1h): {:.2} SOL", total_volume);
            println!("📈 Total trades (1h): {}", total_trades);
            
            // Show top 5 by volume
            let mut sorted_tokens: Vec<_> = metrics_map.iter().collect();
            sorted_tokens.sort_by(|a, b| b.1.volume_sol_1h.partial_cmp(&a.1.volume_sol_1h).unwrap());
            
            println!("\n🔥 TOP VOLUME TOKENS:");
            for (i, (mint, metrics)) in sorted_tokens.iter().take(5).enumerate() {
                println!("  {}. {} - {:.2} SOL vol, {} trades, {:.1}% price change", 
                         i+1, 
                         &mint[..8], 
                         metrics.volume_sol_1h,
                         metrics.trades_1h.len(),
                         metrics.price_change_1h_percent);
            }
        }
        
        println!("═══════════════════════════════════════\n");
    }
}