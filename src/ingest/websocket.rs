use anyhow::{Result, Context, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration, timeout, Instant};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tracing::{info, warn, error, debug, instrument};
use url::Url;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Configuration for Solana WebSocket connection
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    /// Primary RPC WebSocket URL (e.g., "wss://api.mainnet-beta.solana.com/")
    pub primary_url: String,
    /// Backup RPC WebSocket URLs for failover
    pub backup_urls: Vec<String>,
    /// Connection timeout in milliseconds
    pub connect_timeout_ms: u64,
    /// Maximum reconnection attempts before giving up
    pub max_reconnect_attempts: u32,
    /// Base delay between reconnection attempts in milliseconds
    pub reconnect_delay_ms: u64,
    /// Heartbeat interval to keep connection alive
    pub heartbeat_interval_ms: u64,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            primary_url: "wss://api.mainnet-beta.solana.com/".to_string(),
            backup_urls: vec![
                "wss://solana-api.projectserum.com/".to_string(),
                "wss://rpc.ankr.com/solana_ws".to_string(),
            ],
            connect_timeout_ms: 30000,
            max_reconnect_attempts: 10,
            reconnect_delay_ms: 1000,
            heartbeat_interval_ms: 30000,
        }
    }
}

/// JSON-RPC request for Solana WebSocket subscriptions
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Value,
}

/// JSON-RPC response from Solana WebSocket
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC error structure
#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

/// WebSocket notification from Solana RPC
#[derive(Debug, Deserialize)]
pub struct WebSocketNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: WebSocketNotificationParams,
}

/// Parameters for WebSocket notifications
#[derive(Debug, Deserialize)]
pub struct WebSocketNotificationParams {
    pub subscription: u64,
    pub result: Value,
}

/// Types of subscriptions supported
#[derive(Debug, Clone)]
pub enum SubscriptionType {
    /// Subscribe to account changes
    Account { pubkey: String, commitment: String },
    /// Subscribe to transaction signatures
    Signature { signature: String },
    /// Subscribe to program account changes
    ProgramAccount { program_id: String, commitment: String },
    /// Subscribe to slot updates
    Slot,
    /// Subscribe to block updates
    Block { commitment: String },
}

/// Event emitted by WebSocket client
#[derive(Debug, Clone)]
pub enum WebSocketEvent {
    /// Successfully connected to RPC
    Connected { url: String },
    /// Disconnected from RPC
    Disconnected { reason: String },
    /// Subscription confirmed
    SubscriptionConfirmed { subscription_id: u64, request_id: u64 },
    /// Account update received
    AccountUpdate { subscription_id: u64, data: Value },
    /// Transaction notification received
    TransactionNotification { subscription_id: u64, data: Value },
    /// Program account update received
    ProgramAccountUpdate { subscription_id: u64, data: Value },
    /// Slot update received
    SlotUpdate { subscription_id: u64, data: Value },
    /// Block update received
    BlockUpdate { subscription_id: u64, data: Value },
    /// Error occurred
    Error { error: String },
}

/// Connection state for monitoring
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    Failed,
}

/// Statistics for monitoring WebSocket performance
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub state: ConnectionState,
    pub current_url: String,
    pub connection_attempts: u32,
    pub successful_connections: u32,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub subscriptions_active: u32,
    pub last_message_time: Option<Instant>,
    pub uptime_seconds: u64,
}

/// Manages WebSocket connections to Solana RPC nodes
pub struct SolanaWebSocketClient {
    /// Configuration for the client
    config: WebSocketConfig,
    /// Channel for sending events to consumers
    event_sender: mpsc::UnboundedSender<WebSocketEvent>,
    /// Request ID counter for JSON-RPC requests
    request_id: Arc<AtomicU64>,
    /// Current connection state
    connection_state: Arc<tokio::sync::RwLock<ConnectionState>>,
    /// Statistics for monitoring
    stats: Arc<tokio::sync::RwLock<ConnectionStats>>,
    /// Active subscriptions mapping request_id -> subscription_id
    active_subscriptions: Arc<tokio::sync::RwLock<HashMap<u64, u64>>>,
    /// Channel for sending messages to WebSocket (populated when connected)
    message_sender: Arc<tokio::sync::RwLock<Option<mpsc::UnboundedSender<Message>>>>,
}

impl std::fmt::Debug for SolanaWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaWebSocketClient")
            .field("config", &self.config)
            .field("request_id", &self.request_id)
            .finish_non_exhaustive()
    }
}

impl SolanaWebSocketClient {
    /// Creates a new WebSocket client with the given configuration
    /// 
    /// # Arguments
    /// * `config` - WebSocket connection configuration
    /// 
    /// # Returns
    /// * `Result<(Self, mpsc::UnboundedReceiver<WebSocketEvent>)>` - Client instance and event receiver
    #[instrument]
    pub fn new(config: WebSocketConfig) -> Result<(Self, mpsc::UnboundedReceiver<WebSocketEvent>)> {
        info!("Initializing Solana WebSocket client with primary URL: {}", config.primary_url);
        
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        
        let stats = ConnectionStats {
            state: ConnectionState::Disconnected,
            current_url: config.primary_url.clone(),
            connection_attempts: 0,
            successful_connections: 0,
            messages_sent: 0,
            messages_received: 0,
            subscriptions_active: 0,
            last_message_time: None,
            uptime_seconds: 0,
        };
        
        let client = Self {
            config,
            event_sender,
            request_id: Arc::new(AtomicU64::new(1)),
            connection_state: Arc::new(tokio::sync::RwLock::new(ConnectionState::Disconnected)),
            stats: Arc::new(tokio::sync::RwLock::new(stats)),
            active_subscriptions: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            message_sender: Arc::new(tokio::sync::RwLock::new(None)),
        };
        
        Ok((client, event_receiver))
    }
    
    /// Starts the WebSocket client connection loop
    /// This method runs indefinitely, handling connections, reconnections, and message processing
    #[instrument(skip(self))]
    pub async fn run(&self) -> Result<()> {
        info!("Starting Solana WebSocket client connection loop");
        
        let mut current_url_index = 0;
        let urls = std::iter::once(self.config.primary_url.clone())
            .chain(self.config.backup_urls.iter().cloned())
            .collect::<Vec<_>>();
        
        loop {
            let url = &urls[current_url_index % urls.len()];
            
            // Update connection state to connecting
            {
                let mut state = self.connection_state.write().await;
                *state = ConnectionState::Connecting;
                let mut stats = self.stats.write().await;
                stats.state = ConnectionState::Connecting;
                stats.current_url = url.clone();
                stats.connection_attempts += 1;
            }
            
            // Emit connecting event
            let _ = self.event_sender.send(WebSocketEvent::Connected { url: url.clone() });
            
            match self.connect_and_handle(url).await {
                Ok(()) => {
                    debug!("WebSocket connection closed normally");
                }
                Err(e) => {
                    error!(error = %e, url = %url, "WebSocket connection failed");
                    
                    // Update connection state to failed
                    {
                        let mut state = self.connection_state.write().await;
                        *state = ConnectionState::Failed;
                        let mut stats = self.stats.write().await;
                        stats.state = ConnectionState::Failed;
                    }
                    
                    // Emit error event
                    let _ = self.event_sender.send(WebSocketEvent::Error {
                        error: format!("Connection to {} failed: {}", url, e),
                    });
                    
                    // Try next URL
                    current_url_index += 1;
                    
                    // If we've tried all URLs, wait before retrying
                    if current_url_index % urls.len() == 0 {
                        let delay = Duration::from_millis(self.config.reconnect_delay_ms);
                        warn!("All URLs failed, waiting {:?} before retrying", delay);
                        sleep(delay).await;
                    }
                }
            }
            
            // Update state to reconnecting
            {
                let mut state = self.connection_state.write().await;
                *state = ConnectionState::Reconnecting;
                let mut stats = self.stats.write().await;
                stats.state = ConnectionState::Reconnecting;
            }
        }
    }
    
    /// Connects to a specific URL and handles the WebSocket communication
    /// 
    /// # Arguments
    /// * `url` - The WebSocket URL to connect to
    /// 
    /// # Returns
    /// * `Result<()>` - Ok if connection was successful and closed normally
    #[instrument(skip(self))]
    async fn connect_and_handle(&self, url: &str) -> Result<()> {
        info!("Attempting to connect to Solana RPC WebSocket: {}", url);
        
        // Parse and validate URL
        let parsed_url = Url::parse(url).context("Failed to parse WebSocket URL")?;
        
        // Establish WebSocket connection with timeout
        let (ws_stream, response) = timeout(
            Duration::from_millis(self.config.connect_timeout_ms),
            connect_async(parsed_url)
        ).await
            .context("Connection timeout")?
            .context("Failed to connect to WebSocket")?;
        
        info!("Successfully connected to {} (HTTP {})", url, response.status());
        
        // Update connection state to connected
        {
            let mut state = self.connection_state.write().await;
            *state = ConnectionState::Connected;
            let mut stats = self.stats.write().await;
            stats.state = ConnectionState::Connected;
            stats.successful_connections += 1;
            stats.last_message_time = Some(Instant::now());
        }
        
        // Split WebSocket into sender and receiver
        let (ws_sender, mut ws_receiver) = ws_stream.split();
        
        // Create channel for sending messages to WebSocket
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        
        // Store message sender for external use
        {
            let mut sender = self.message_sender.write().await;
            *sender = Some(tx.clone());
        }
        
        // Auto-subscribe to key data streams after connection and channel setup
        info!("🔧 Auto-subscribing to Solana data streams...");
        
        // Subscribe to slot updates (most reliable)
        let slot_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 999,
            method: "slotSubscribe".to_string(),
            params: serde_json::json!([]),
        };
        
        if let Ok(slot_msg) = serde_json::to_string(&slot_request) {
            match tx.send(Message::Text(slot_msg)) {
                Ok(_) => info!("📡 Sent slot subscription request"),
                Err(e) => error!("❌ Failed to send slot subscription: {}", e),
            }
        } else {
            error!("❌ Failed to serialize slot subscription request");
        }
        
        // Subscribe to USDC account (high activity)
        let account_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 998,
            method: "accountSubscribe".to_string(),
            params: serde_json::json!([
                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                {"commitment": "processed", "encoding": "jsonParsed"}
            ]),
        };
        
        if let Ok(account_msg) = serde_json::to_string(&account_request) {
            match tx.send(Message::Text(account_msg)) {
                Ok(_) => info!("📡 Sent USDC account subscription request"),
                Err(e) => error!("❌ Failed to send account subscription: {}", e),
            }
        } else {
            error!("❌ Failed to serialize account subscription request");
        }
        
        // Subscribe to Raydium AMM program for pool discovery
        let raydium_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 997,
            method: "programSubscribe".to_string(),
            params: serde_json::json!([
                "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
                {"commitment": "confirmed", "encoding": "jsonParsed", "filters": []}
            ]),
        };
        
        if let Ok(program_msg) = serde_json::to_string(&raydium_request) {
            match tx.send(Message::Text(program_msg)) {
                Ok(_) => info!("📡 Sent Raydium program subscription request"),
                Err(e) => error!("❌ Failed to send Raydium program subscription: {}", e),
            }
        } else {
            error!("❌ Failed to serialize Raydium program subscription request");
        }
        
        // Subscribe to Jupiter V6 program for aggregator trades
        let jupiter_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 996,
            method: "programSubscribe".to_string(),
            params: serde_json::json!([
                "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",
                {"commitment": "confirmed", "encoding": "jsonParsed", "filters": []}
            ]),
        };
        
        if let Ok(jupiter_msg) = serde_json::to_string(&jupiter_request) {
            match tx.send(Message::Text(jupiter_msg)) {
                Ok(_) => info!("📡 Sent Jupiter V6 program subscription request"),
                Err(e) => error!("❌ Failed to send Jupiter program subscription: {}", e),
            }
        } else {
            error!("❌ Failed to serialize Jupiter program subscription request");
        }
        
        // Subscribe to Orca Whirlpool program
        let orca_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 995,
            method: "programSubscribe".to_string(),
            params: serde_json::json!([
                "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc",
                {"commitment": "confirmed", "encoding": "jsonParsed", "filters": []}
            ]),
        };
        
        if let Ok(orca_msg) = serde_json::to_string(&orca_request) {
            match tx.send(Message::Text(orca_msg)) {
                Ok(_) => info!("📡 Sent Orca Whirlpool program subscription request"),
                Err(e) => error!("❌ Failed to send Orca program subscription: {}", e),
            }
        } else {
            error!("❌ Failed to serialize Orca program subscription request");
        }
        
        // Subscribe to SPL Token program for new token mints
        let spl_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 994,
            method: "programSubscribe".to_string(),
            params: serde_json::json!([
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                {"commitment": "confirmed", "encoding": "jsonParsed", "filters": [
                    {"dataSize": 82} // Filter for mint accounts only
                ]}
            ]),
        };
        
        if let Ok(spl_msg) = serde_json::to_string(&spl_request) {
            match tx.send(Message::Text(spl_msg)) {
                Ok(_) => info!("📡 Sent SPL Token program subscription request"),
                Err(e) => error!("❌ Failed to send SPL Token program subscription: {}", e),
            }
        } else {
            error!("❌ Failed to serialize SPL Token program subscription request");
        }
        
        // Subscribe to Pump.fun program for meme coin launches
        let pump_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 993,
            method: "programSubscribe".to_string(),
            params: serde_json::json!([
                "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
                {"commitment": "confirmed", "encoding": "jsonParsed", "filters": []}
            ]),
        };
        
        if let Ok(pump_msg) = serde_json::to_string(&pump_request) {
            match tx.send(Message::Text(pump_msg)) {
                Ok(_) => info!("📡 Sent Pump.fun program subscription request"),
                Err(e) => error!("❌ Failed to send Pump.fun program subscription: {}", e),
            }
        } else {
            error!("❌ Failed to serialize Pump.fun program subscription request");
        }
        
        // Spawn task to handle outgoing messages
        let tx_task = {
            let event_sender = self.event_sender.clone();
            let stats = self.stats.clone();
            let mut ws_sender = ws_sender;
            
            tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    if let Err(e) = ws_sender.send(msg).await {
                        error!(error = %e, "Failed to send WebSocket message");
                        let _ = event_sender.send(WebSocketEvent::Error {
                            error: format!("Send error: {}", e),
                        });
                        break;
                    }
                    
                    // Update stats
                    {
                        let mut stats = stats.write().await;
                        stats.messages_sent += 1;
                    }
                }
            })
        };
        
        // Spawn task to handle incoming messages
        let rx_task = {
            let event_sender = self.event_sender.clone();
            let stats = self.stats.clone();
            let active_subscriptions = self.active_subscriptions.clone();
            
            tokio::spawn(async move {
                while let Some(msg) = ws_receiver.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            // Update stats
                            {
                                let mut stats = stats.write().await;
                                stats.messages_received += 1;
                                stats.last_message_time = Some(Instant::now());
                            }
                            
                           // debug!("Received WebSocket message: {}", text);
                            
                            // Print all non-ping/pong messages to see what's actually coming through
                            if !text.is_empty() && text != "pong" {
                               //println!("🔗 RAW WEBSOCKET MESSAGE: {}", text);
                            }
                            
                            // Parse and handle JSON-RPC message
                            if let Err(e) = Self::handle_message(&text, &event_sender, &active_subscriptions).await {
                                warn!(error = %e, message = %text, "Failed to handle WebSocket message");
                                println!("❌ FAILED TO PARSE MESSAGE: {} - {}", e, text);
                            }
                        }
                        Ok(Message::Close(close_frame)) => {
                            info!("WebSocket closed: {:?}", close_frame);
                            break;
                        }
                        Ok(Message::Ping(_data)) => {
                            //debug!("Received ping, sending pong");
                            // WebSocket will automatically handle pong response
                        }
                        Ok(Message::Pong(_)) => {
                            //debug!("Received pong");
                        }
                        Ok(Message::Binary(data)) => {
                            warn!("Received unexpected binary message: {} bytes", data.len());
                        }
                        Ok(Message::Frame(_)) => {
                            debug!("Received raw frame message (ignored)");
                        }
                        Err(e) => {
                            error!(error = %e, "WebSocket receive error");
                            let _ = event_sender.send(WebSocketEvent::Error {
                                error: format!("Receive error: {}", e),
                            });
                            break;
                        }
                    }
                }
                
                debug!("WebSocket receive loop ended");
            })
        };
        
        // Spawn heartbeat task to keep connection alive
        let heartbeat_task = {
            let tx = tx.clone();
            let heartbeat_interval = Duration::from_millis(self.config.heartbeat_interval_ms);
            
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(heartbeat_interval);
                loop {
                    interval.tick().await;
                    
                    // Send ping to keep connection alive
                    if tx.send(Message::Ping(vec![])).is_err() {
                        debug!("Heartbeat channel closed");
                        break;
                    }
                }
            })
        };
        
        // Wait for tasks to complete (indicates connection closed)
        tokio::select! {
            _ = tx_task => debug!("WebSocket sender task completed"),
            _ = rx_task => debug!("WebSocket receiver task completed"),
            _ = heartbeat_task => debug!("Heartbeat task completed"),
        }
        
        Ok(())
    }
    
    /// Handles an incoming WebSocket message
    /// 
    /// # Arguments
    /// * `message` - Raw JSON message text
    /// * `event_sender` - Channel to send events to consumers
    /// * `active_subscriptions` - Map of active subscriptions
    #[instrument(skip(event_sender, active_subscriptions))]
    async fn handle_message(
        message: &str,
        event_sender: &mpsc::UnboundedSender<WebSocketEvent>,
        active_subscriptions: &Arc<tokio::sync::RwLock<HashMap<u64, u64>>>,
    ) -> Result<()> {
        // Try to parse as JSON-RPC response first
        if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(message) {
            if let Some(id) = response.id {
                if let Some(result) = response.result {
                    // This is a subscription confirmation
                    if let Ok(subscription_id) = serde_json::from_value::<u64>(result.clone()) {
                        // Store the subscription mapping
                        {
                            let mut subs = active_subscriptions.write().await;
                            subs.insert(id, subscription_id);
                        }
                        
                        let _ = event_sender.send(WebSocketEvent::SubscriptionConfirmed {
                            subscription_id,
                            request_id: id,
                        });
                        
                        info!("Subscription confirmed: request_id={}, subscription_id={}", id, subscription_id);
                        return Ok(());
                    }
                }
                
                if let Some(error) = response.error {
                    error!("JSON-RPC error for request {}: {} - {}", id, error.code, error.message);
                    let _ = event_sender.send(WebSocketEvent::Error {
                        error: format!("RPC error {}: {}", error.code, error.message),
                    });
                }
            }
            return Ok(());
        }
        
        // Try to parse as WebSocket notification
        if let Ok(notification) = serde_json::from_str::<WebSocketNotification>(message) {
            let subscription_id = notification.params.subscription;
            let data = notification.params.result;
            
            // Determine event type based on the method
            let event = match notification.method.as_str() {
                "accountNotification" => WebSocketEvent::AccountUpdate { subscription_id, data },
                "signatureNotification" => WebSocketEvent::TransactionNotification { subscription_id, data },
                "programNotification" => WebSocketEvent::ProgramAccountUpdate { subscription_id, data },
                "slotNotification" => WebSocketEvent::SlotUpdate { subscription_id, data },
                "blockNotification" => WebSocketEvent::BlockUpdate { subscription_id, data },
                method => {
                    warn!("Unknown notification method: {}", method);
                    return Ok(());
                }
            };
            
            debug!("Received {} for subscription {}", notification.method, subscription_id);
            let _ = event_sender.send(event);
            return Ok(());
        }
        
        warn!("Failed to parse WebSocket message as JSON-RPC response or notification");
        Ok(())
    }
    
    /// Subscribes to account changes for a specific public key
    /// 
    /// # Arguments
    /// * `pubkey` - The account public key to monitor
    /// * `commitment` - Commitment level ("finalized", "confirmed", "processed")
    /// 
    /// # Returns
    /// * `Result<u64>` - Request ID for tracking the subscription
    #[instrument(skip(self))]
    pub async fn subscribe_account(&self, pubkey: &str, commitment: &str) -> Result<u64> {
        let request_id = self.request_id.fetch_add(1, Ordering::Relaxed);
        
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: request_id,
            method: "accountSubscribe".to_string(),
            params: serde_json::json!([
                pubkey,
                {
                    "commitment": commitment,
                    "encoding": "jsonParsed"
                }
            ]),
        };
        
        self.send_request(request).await?;
        info!("Subscribed to account {} with commitment {}", pubkey, commitment);
        
        Ok(request_id)
    }
    
    /// Subscribes to program account changes for a specific program ID
    /// 
    /// # Arguments
    /// * `program_id` - The program ID to monitor
    /// * `commitment` - Commitment level ("finalized", "confirmed", "processed")
    /// 
    /// # Returns
    /// * `Result<u64>` - Request ID for tracking the subscription
    #[instrument(skip(self))]
    pub async fn subscribe_program(&self, program_id: &str, commitment: &str) -> Result<u64> {
        let request_id = self.request_id.fetch_add(1, Ordering::Relaxed);
        
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: request_id,
            method: "programSubscribe".to_string(),
            params: serde_json::json!([
                program_id,
                {
                    "commitment": commitment,
                    "encoding": "jsonParsed"
                }
            ]),
        };
        
        self.send_request(request).await?;
        info!("Subscribed to program {} with commitment {}", program_id, commitment);
        
        Ok(request_id)
    }
    
    /// Subscribes to slot updates (new blocks)
    /// 
    /// # Returns
    /// * `Result<u64>` - Request ID for tracking the subscription
    #[instrument(skip(self))]
    pub async fn subscribe_slot(&self) -> Result<u64> {
        let request_id = self.request_id.fetch_add(1, Ordering::Relaxed);
        
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: request_id,
            method: "slotSubscribe".to_string(),
            params: serde_json::json!([]),
        };
        
        self.send_request(request).await?;
        info!("Subscribed to slot updates (new blocks)");
        
        Ok(request_id)
    }
    
    /// Sends a JSON-RPC request over the WebSocket
    /// 
    /// # Arguments
    /// * `request` - The JSON-RPC request to send
    #[instrument(skip(self))]
    async fn send_request(&self, request: JsonRpcRequest) -> Result<()> {
        let message = serde_json::to_string(&request)
            .context("Failed to serialize JSON-RPC request")?;
        
        debug!("Sending WebSocket request: {}", message);
        
        // Get the message sender
        let sender = {
            let sender_lock = self.message_sender.read().await;
            sender_lock.clone()
        };
        
        match sender {
            Some(tx) => {
                tx.send(Message::Text(message))
                    .map_err(|_| anyhow::anyhow!("Failed to send message - WebSocket sender channel closed"))?;
                Ok(())
            }
            None => {
                bail!("WebSocket connection not established - call run() first");
            }
        }
    }
    
    /// Returns current connection statistics
    /// 
    /// # Returns
    /// * `ConnectionStats` - Current connection statistics
    pub async fn get_stats(&self) -> ConnectionStats {
        let stats = self.stats.read().await;
        stats.clone()
    }
    
    /// Returns current connection state
    /// 
    /// # Returns
    /// * `ConnectionState` - Current connection state
    pub async fn get_connection_state(&self) -> ConnectionState {
        let state = self.connection_state.read().await;
        state.clone()
    }
}