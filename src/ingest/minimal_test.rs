use anyhow::{Result, Context};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_util::{StreamExt, SinkExt};
use serde_json::{json, Value};
use tracing::{info, debug, error, warn};

/// Minimal Solana WebSocket test - no complex dependencies
pub struct MinimalSolanaTest;

impl MinimalSolanaTest {
    /// Simple test that connects to Solana and shows real live data
    pub async fn run() -> Result<()> {
        info!("🚀 MINIMAL SOLANA WEBSOCKET TEST");
        info!("================================");
        
        // Connect to Solana mainnet WebSocket
        let url = "wss://api.mainnet-beta.solana.com/";
        info!("📡 Connecting to Solana WebSocket: {}", url);
        
        let (ws_stream, response) = connect_async(url).await
            .context("Failed to connect to Solana WebSocket")?;
        
        info!("✅ CONNECTED TO SOLANA MAINNET!");
        info!("HTTP Status: {}", response.status());
        info!("");
        
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        
        // Subscribe to slot updates (new blocks being created)
        let slot_subscription = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "slotSubscribe",
            "params": []
        });
        
        info!("📋 Subscribing to live Solana slot updates...");
        ws_sender.send(Message::Text(slot_subscription.to_string())).await
            .context("Failed to send slot subscription")?;
        
        info!("🔥 STREAMING LIVE SOLANA DATA:");
        info!("------------------------------");
        
        let mut message_count = 0;
        let start_time = std::time::Instant::now();
        
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    message_count += 1;
                    
                    // Parse the JSON message
                    if let Ok(json_msg) = serde_json::from_str::<Value>(&text) {
                        // Check if this is a subscription confirmation
                        if let Some(result) = json_msg.get("result") {
                            if let Some(subscription_id) = result.as_u64() {
                                info!("🎯 Subscription active! ID: {}", subscription_id);
                                continue;
                            }
                        }
                        
                        // Check if this is a slot notification (new block)
                        if let Some(method) = json_msg.get("method") {
                            if method.as_str() == Some("slotNotification") {
                                if let Some(params) = json_msg.get("params") {
                                    if let Some(result) = params.get("result") {
                                        if let Some(slot) = result.get("slot") {
                                            let slot_num = slot.as_u64().unwrap_or(0);
                                            let elapsed = start_time.elapsed();
                                            
                                            // Show live block creation
                                            info!(
                                                "🟢 LIVE BLOCK #{} | Messages: {} | Time: {:.1}s",
                                                slot_num,
                                                message_count,
                                                elapsed.as_secs_f32()
                                            );
                                            
                                            // Stop after showing 10 blocks for demo
                                            if message_count >= 10 {
                                                info!("");
                                                info!("✅ SUCCESS! Received {} live messages from Solana", message_count);
                                                info!("📊 Average rate: {:.1} messages/second", 
                                                    message_count as f64 / elapsed.as_secs_f64());
                                                info!("🎉 REAL SOLANA DATA STREAMING WORKS!");
                                                return Ok(());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Message::Close(close_frame)) => {
                    warn!("WebSocket closed: {:?}", close_frame);
                    break;
                }
                Ok(Message::Ping(data)) => {
                    debug!("Ping received, sending pong");
                    let _ = ws_sender.send(Message::Pong(data)).await;
                }
                Err(e) => {
                    error!(error = %e, "WebSocket error");
                    break;
                }
                _ => {
                    debug!("Other message type received");
                }
            }
        }
        
        Ok(())
    }
}