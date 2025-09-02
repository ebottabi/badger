/// Main pump realtime analyzer orchestrating all components

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::time::Duration;
use colored::Colorize;
use chrono::{DateTime, Utc};

use crate::client::websocket::PumpWebSocketClient;
use crate::client::event_parser::{parse_pump_event, is_subscription_success};
use crate::util::time_series::SlidingWindow;
use crate::algo::signal_processor::SignalProcessor;
use crate::algo::trend_analysis::{calculate_trend_analysis, TrendStrength};
use crate::algo::mathematical_engine::MathematicalEngine;
use crate::config::ConfigManager;
use crate::execution::{PositionManager, TradingClient, StrategyExecutor, RiskManager, PortfolioTracker, PositionMonitor};

pub struct PumpRealtimeAnalyzer {
    seen_tokens: HashSet<String>,
    pub token_windows: HashMap<String, SlidingWindow>,
    pub signal_processor: SignalProcessor,
    pub mathematical_engine: MathematicalEngine,
    websocket_client: PumpWebSocketClient,
    pub config_manager: Option<Arc<ConfigManager>>,
    pub strategy_executor: Option<Arc<StrategyExecutor>>,
}

impl PumpRealtimeAnalyzer {
    pub fn new() -> Self {
        // Create default signal processor without config for basic operation
        Self {
            seen_tokens: HashSet::new(),
            token_windows: HashMap::new(),
            signal_processor: SignalProcessor::new_default(),
            mathematical_engine: MathematicalEngine::new(),
            websocket_client: PumpWebSocketClient::new(),
            config_manager: None,
            strategy_executor: None,
        }
    }
    
    pub async fn new_with_execution(config_manager: Arc<ConfigManager>) -> Result<Self, Box<dyn std::error::Error>> {
        let config = config_manager.get_config();
        
        // Initialize execution components
        let position_manager = Arc::new(PositionManager::new());
        let trading_client = Arc::new(TradingClient::new(
            config.wallet.public_key.clone(),
            config.wallet.pump_api_key.clone(),
            config.trading.slippage_tolerance_percent,
            config.trading.max_retry_attempts,
            config.trading.priority_fee_sol,
        ));
        let risk_manager = Arc::new(RiskManager::new(config.clone()));
        let portfolio_tracker = Arc::new(std::sync::Mutex::new(
            PortfolioTracker::new("data/portfolio.json")
        ));
        
        let strategy_executor = Arc::new(StrategyExecutor::new(
            config.clone(),
            Arc::clone(&position_manager),
            Arc::clone(&trading_client),
            Arc::clone(&risk_manager),
        ));
        
        // Start position monitoring in background
        let position_monitor = Arc::new(PositionMonitor::new(
            config.clone(),
            position_manager,
            trading_client,
            risk_manager,
            Arc::clone(&portfolio_tracker),
        ));
        
        let monitor_clone = Arc::clone(&position_monitor);
        tokio::spawn(async move {
            monitor_clone.start_monitoring().await;
        });
        
        Ok(Self {
            seen_tokens: HashSet::new(),
            token_windows: HashMap::new(),
            signal_processor: SignalProcessor::new(&config).with_executor(Arc::clone(&strategy_executor)),
            mathematical_engine: MathematicalEngine::new(),
            websocket_client: PumpWebSocketClient::new(),
            config_manager: Some(config_manager),
            strategy_executor: Some(strategy_executor),
        })
    }
    
    pub async fn start_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (mut write, mut read) = self.websocket_client.connect().await?;
        
        // Subscribe to all event types
        self.websocket_client.send_subscriptions(&mut write).await?;
        
        // Start sliding window analysis timer
        let mut analysis_timer = tokio::time::interval(Duration::from_secs(15));
        
        println!("\n{}", "🎯 Starting real-time monitoring + trend analysis...".bright_green());
        println!("{}", "=".repeat(80).dimmed());
        
        loop {
            tokio::select! {
                // Real-time: Process WebSocket events instantly
                Some(message) = futures_util::StreamExt::next(&mut read) => {
                    if let Ok(tokio_tungstenite::tungstenite::protocol::Message::Text(text)) = message {
                        self.handle_realtime_message(&text).await;
                    }
                }
                
                // Sliding window: Analyze trends every 15 seconds  
                _ = analysis_timer.tick() => {
                    self.analyze_sliding_windows().await;
                }
            }
        }
    }
    
    async fn handle_realtime_message(&mut self, message: &str) {
        // Parse multi-format events (pump, bonk, raydium, etc.)
        if let Ok(event) = parse_pump_event(message) {
            // Check if token is already seen for creation events
            if event.tx_type == "create" && self.seen_tokens.contains(&event.mint) {
                return;
            }
            
            if event.tx_type == "create" {
                self.seen_tokens.insert(event.mint.clone());
            }
            
            // Instant decision making
            self.signal_processor.process_instant_signals(&event).await;
            
            // Add to sliding window for trend analysis
            self.add_to_sliding_window(&event);
        } else if let Some(success_msg) = is_subscription_success(message) {
            println!("✅ {}", success_msg.green());
        }
    }
}