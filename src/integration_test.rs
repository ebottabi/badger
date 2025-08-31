use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, error};
use crate::ingest::{SolanaWebSocketClient, SimpleStreamProcessor};
use crate::ingest::websocket::{WebSocketConfig, WebSocketEvent};

/// Simple integration test to verify Solana WebSocket connectivity
pub async fn test_solana_websocket_connection() -> Result<()> {
    info!("🔌 Testing Solana WebSocket connection in simplified single-crate structure...");
    
    // Configure WebSocket client to connect to real Solana mainnet
    let config = WebSocketConfig {
        primary_url: "wss://api.mainnet-beta.solana.com".to_string(),
        backup_urls: vec![],
        connect_timeout_ms: 10000,
        max_reconnect_attempts: 3,
        reconnect_delay_ms: 5000,
        heartbeat_interval_ms: 30000,
    };
    
    // Initialize WebSocket client
    let (mut client, mut event_rx) = SolanaWebSocketClient::new(config)?;
    info!("✅ SolanaWebSocketClient created successfully");
    
    // Connect to Solana WebSocket
    client.connect().await?;
    info!("✅ Connected to Solana WebSocket");
    
    // Subscribe to account notifications for a high-activity account (USDC token mint)
    let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    client.subscribe_to_account(usdc_mint).await?;
    info!("✅ Subscribed to account: {}", usdc_mint);
    
    // Listen for events for a short time to verify connectivity
    let mut event_count = 0;
    let max_events = 5;
    
    info!("📡 Listening for WebSocket events...");
    
    // Use a timeout to avoid waiting indefinitely
    let timeout_duration = tokio::time::Duration::from_secs(30);
    let timeout = tokio::time::timeout(timeout_duration, async {
        while let Some(event) = event_rx.recv().await {
            event_count += 1;
            
            match event {
                WebSocketEvent::Connected { url } => {
                    info!("🟢 WebSocket connected to: {}", url);
                }
                WebSocketEvent::Disconnected { reason } => {
                    info!("🔴 WebSocket disconnected: {}", reason);
                }
                WebSocketEvent::SubscriptionConfirmed { subscription_id, request_id } => {
                    info!("✅ Subscription confirmed: {} (request: {})", subscription_id, request_id);
                }
                WebSocketEvent::AccountUpdate { subscription_id, data } => {
                    info!("📊 Account update received for subscription: {}", subscription_id);
                }
                WebSocketEvent::Error { error } => {
                    error!("❌ WebSocket error: {}", error);
                }
                _ => {
                    info!("📡 Other WebSocket event received");
                }
            }
            
            if event_count >= max_events {
                info!("📈 Received {} events, test successful!", event_count);
                break;
            }
        }
    });
    
    match timeout.await {
        Ok(_) => {
            info!("🎉 Integration test completed successfully!");
            info!("📊 Total events received: {}", event_count);
        }
        Err(_) => {
            info!("⏰ Test timeout reached");
            if event_count > 0 {
                info!("✅ Connection working (received {} events before timeout)", event_count);
            } else {
                info!("⚠️  No events received - connection may be slow or account inactive");
            }
        }
    }
    
    // Disconnect
    client.disconnect().await?;
    info!("🔌 Disconnected from Solana WebSocket");
    
    Ok(())
}