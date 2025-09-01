/// Main pump realtime analyzer orchestrating all components

use std::collections::{HashMap, HashSet};
use tokio::time::Duration;
use colored::Colorize;
use chrono::{DateTime, Utc};

use crate::client::websocket::PumpWebSocketClient;
use crate::client::event_parser::{parse_pump_event, is_subscription_success};
use crate::util::time_series::SlidingWindow;
use crate::algo::signal_processor::SignalProcessor;
use crate::algo::trend_analysis::{calculate_trend_analysis, TrendStrength};
use crate::algo::mathematical_engine::MathematicalEngine;

pub struct PumpRealtimeAnalyzer {
    seen_tokens: HashSet<String>,
    pub token_windows: HashMap<String, SlidingWindow>,
    pub signal_processor: SignalProcessor,
    pub mathematical_engine: MathematicalEngine,
    websocket_client: PumpWebSocketClient,
}

impl PumpRealtimeAnalyzer {
    pub fn new() -> Self {
        Self {
            seen_tokens: HashSet::new(),
            token_windows: HashMap::new(),
            signal_processor: SignalProcessor::new(),
            mathematical_engine: MathematicalEngine::new(),
            websocket_client: PumpWebSocketClient::new(),
        }
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