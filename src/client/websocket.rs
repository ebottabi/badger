/// WebSocket client for pump.fun real-time data

use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_util::{SinkExt, StreamExt};
use colored::Colorize;

pub struct PumpWebSocketClient {
    url: String,
}

impl PumpWebSocketClient {
    pub fn new() -> Self {
        Self {
            url: "wss://pumpportal.fun/api/data".to_string(),
        }
    }
    
    pub async fn connect(&self) -> Result<
        (
            futures_util::stream::SplitSink<
                tokio_tungstenite::WebSocketStream<
                    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
                >,
                Message
            >,
            futures_util::stream::SplitStream<
                tokio_tungstenite::WebSocketStream<
                    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
                >
            >
        ),
        Box<dyn std::error::Error>
    > {
        println!("🔗 Connecting to: {}", self.url.cyan());
        
        let (ws_stream, _) = connect_async(&self.url).await?;
        println!("✅ {}", "WebSocket connected successfully!".green().bold());
        
        let (write, read) = ws_stream.split();
        Ok((write, read))
    }
    
    pub async fn send_subscriptions(
        &self,
        write: &mut futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>
            >,
            Message
        >
    ) -> Result<(), Box<dyn std::error::Error>> {
        let subscriptions = vec![
            serde_json::json!({"method": "subscribeNewToken"}),
            serde_json::json!({"method": "subscribeTokenTrade", "keys": ["*"]}),
            serde_json::json!({"method": "subscribeAccountTrade", "keys": ["*"]}),
        ];
        
        for subscription in subscriptions {
            write.send(Message::Text(subscription.to_string())).await?;
        }
        
        Ok(())
    }
}