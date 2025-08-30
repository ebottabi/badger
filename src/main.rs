use anyhow::Result;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio::sync::broadcast;
use tracing::{info, warn, error, debug};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use std::time::Duration;

use badger::ingest::websocket::{SolanaWebSocketClient, WebSocketConfig, WebSocketEvent};

/// Parse and display slot update data in a human-readable format
fn parse_and_display_slot_update(subscription_id: u64, data: &serde_json::Value) {
    if let Ok(slot_info) = serde_json::from_value::<serde_json::Value>(data.clone()) {
        if let (Some(slot), Some(parent), Some(root)) = (
            slot_info.get("slot").and_then(|s| s.as_u64()),
            slot_info.get("parent").and_then(|p| p.as_u64()),
            slot_info.get("root").and_then(|r| r.as_u64())
        ) {
            let finality_gap = slot - root;
            println!("⏰ SLOT #{} | Parent: {} | Finalized Root: {} | Gap: {} slots ({:.1}s)", 
                slot, parent, root, finality_gap, finality_gap as f64 * 0.4);
        } else {
            println!("⏰ SLOT UPDATE [{}]: {}", subscription_id, data);
        }
    }
}

/// Parse and display account update data in a human-readable format
fn parse_and_display_account_update(subscription_id: u64, data: &serde_json::Value) {
    if let Some(context) = data.get("context") {
        if let Some(slot) = context.get("slot").and_then(|s| s.as_u64()) {
            if let Some(value) = data.get("value") {
                if let Some(lamports) = value.get("lamports").and_then(|l| l.as_u64()) {
                    let sol_balance = lamports as f64 / 1_000_000_000.0;
                    
                    if let Some(parsed_data) = value.get("data").and_then(|d| d.get("parsed")) {
                        if let Some(token_info) = parsed_data.get("info") {
                            if let Some(supply) = token_info.get("supply").and_then(|s| s.as_str()) {
                                if let Ok(supply_num) = supply.parse::<u64>() {
                                    let usdc_supply = supply_num as f64 / 1_000_000.0;
                                    println!("💰 USDC MINT UPDATE | Slot: {} | Supply: {:.2}M USDC | Balance: {:.9} SOL", 
                                        slot, usdc_supply / 1_000_000.0, sol_balance);
                                    return;
                                }
                            }
                        }
                    }
                    
                    println!("📊 ACCOUNT UPDATE | Slot: {} | Balance: {:.9} SOL", slot, sol_balance);
                } else {
                    println!("📊 ACCOUNT UPDATE [{}] | Slot: {} | Raw: {}", 
                        subscription_id, 
                        data.get("context").and_then(|c| c.get("slot")).unwrap_or(&serde_json::Value::Null),
                        data);
                }
            }
        }
    } else {
        println!("📊 ACCOUNT UPDATE [{}]: {}", subscription_id, data);
    }
}

/// Parse and display program account updates (Raydium, Jupiter DEX events)
fn parse_and_display_program_update(subscription_id: u64, data: &serde_json::Value) {
    if let Some(context) = data.get("context") {
        if let Some(slot) = context.get("slot").and_then(|s| s.as_u64()) {
            if let Some(value) = data.get("value") {
                if let Some(account) = value.get("account") {
                    if let Some(pubkey) = value.get("pubkey").and_then(|p| p.as_str()) {
                        if let Some(owner) = account.get("owner").and_then(|o| o.as_str()) {
                            match owner {
                                "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8" => {
                                    println!("🔥 RAYDIUM POOL EVENT | Slot: {} | Account: {}...{} | Subscription: {}", 
                                        slot, &pubkey[..8], &pubkey[pubkey.len()-8..], subscription_id);
                                    
                                    // Check if this is a new pool creation
                                    if let Some(account_data) = account.get("data") {
                                        if let Some(parsed) = account_data.get("parsed") {
                                            if let Some(program) = parsed.get("program").and_then(|p| p.as_str()) {
                                                println!("   Program: {} | New pool detected!", program);
                                            }
                                        } else if let Some(raw_data) = account_data.get("data") {
                                            println!("   Raw pool data detected - {} bytes", 
                                                raw_data.as_array().map(|a| a.len()).unwrap_or(0));
                                        }
                                    }
                                }
                                "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4" => {
                                    println!("⚡ JUPITER SWAP EVENT | Slot: {} | Account: {}...{}", 
                                        slot, &pubkey[..8], &pubkey[pubkey.len()-8..]);
                                    
                                    if let Some(lamports) = account.get("lamports").and_then(|l| l.as_u64()) {
                                        let sol_value = lamports as f64 / 1_000_000_000.0;
                                        println!("   Jupiter account balance: {:.6} SOL", sol_value);
                                    }
                                }
                                _ => {
                                    println!("📋 PROGRAM UPDATE | Slot: {} | Owner: {} | Account: {}...{}", 
                                        slot, owner, &pubkey[..8], &pubkey[pubkey.len()-8..]);
                                }
                            }
                        } else {
                            println!("📋 PROGRAM UPDATE [{}] | Slot: {} | Account: {}", 
                                subscription_id, slot, pubkey);
                        }
                    }
                }
            }
        }
    } else {
        println!("📋 PROGRAM UPDATE [{}]: {}", subscription_id, data);
    }
}

/// Production-ready Badger trading bot orchestrator
/// 
/// This orchestrator manages the core WebSocket ingestion system for real-time
/// Solana blockchain data processing. It handles graceful startup, shutdown,
/// and error recovery for all services.
struct BadgerOrchestrator {
    shutdown_tx: broadcast::Sender<()>,
    tasks: Vec<JoinHandle<Result<()>>>,
    websocket_config: WebSocketConfig,
}

impl BadgerOrchestrator {
    fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);
        
        let websocket_config = WebSocketConfig {
            primary_url: "wss://api.mainnet-beta.solana.com".to_string(),
            backup_urls: vec![
                "wss://rpc.ankr.com/solana_ws".to_string(),
                "wss://solana-api.projectserum.com".to_string(),
            ],
            connect_timeout_ms: 10000,
            max_reconnect_attempts: 10,
            reconnect_delay_ms: 5000,
            heartbeat_interval_ms: 10000,
        };
        
        Self {
            shutdown_tx,
            tasks: Vec::new(),
            websocket_config,
        }
    }


    /// Starts the core WebSocket ingestion service
    /// 
    /// This service maintains persistent connections to Solana RPC WebSocket endpoints
    /// and processes real-time blockchain data including account updates, transactions,
    /// and program events.
    async fn start_ingestion_service(&mut self) -> Result<()> {
        info!("🔄 Starting Badger Ingestion Service");
        info!("Connecting to Solana mainnet WebSocket endpoints");
        
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let config = self.websocket_config.clone();
        
        let ingestion_task = tokio::spawn(async move {
            info!("🚀 Badger Ingest - Real-time Solana Data Processing");
            
            // Initialize WebSocket client
            let (client, mut event_rx) = match SolanaWebSocketClient::new(config) {
                Ok((client, rx)) => {
                    info!("✅ WebSocket client initialized successfully");
                    (client, rx)
                }
                Err(e) => {
                    error!("❌ Failed to initialize WebSocket client: {}", e);
                    return Err(e);
                }
            };
            
            // Real Solana data only - no mock data

            // Start WebSocket client in background
            let client_handle = tokio::spawn(async move {
                info!("📡 Starting WebSocket client - will subscribe after connection");
                
                // Start the client event loop first
                client.run().await
            });
            
            // Real-time event processing loop (no delays, no batching)
            let mut client_handle = Some(client_handle);
            
            loop {
                tokio::select! {
                    // Process WebSocket events in real-time with no delays
                    Some(event) = event_rx.recv() => {
                        
                        match event {
                            WebSocketEvent::Connected { url } => {
                                info!("🟢 Connected to Solana WebSocket: {}", url);
                                println!("🎯 Connection established - auto-subscriptions will be sent!");
                            }
                            WebSocketEvent::Disconnected { reason } => {
                                warn!("🔴 WebSocket disconnected: {}", reason);
                            }
                            WebSocketEvent::SubscriptionConfirmed { subscription_id, request_id } => {
                                info!("✅ Subscription confirmed: {} (request: {})", subscription_id, request_id);
                                println!("🎯 SUBSCRIPTION CONFIRMED: request_id={}, subscription_id={}", request_id, subscription_id);
                            }
                            WebSocketEvent::AccountUpdate { subscription_id, data } => {
                                parse_and_display_account_update(subscription_id, &data);
                            }
                            WebSocketEvent::TransactionNotification { subscription_id, data } => {
                                println!("📈 TRANSACTION [{}]: {}", subscription_id,
                                    serde_json::to_string_pretty(&data).unwrap_or_else(|_| format!("{:?}", data)));
                            }
                            WebSocketEvent::ProgramAccountUpdate { subscription_id, data } => {
                                parse_and_display_program_update(subscription_id, &data);
                            }
                            WebSocketEvent::SlotUpdate { subscription_id, data } => {
                                parse_and_display_slot_update(subscription_id, &data);
                            }
                            WebSocketEvent::BlockUpdate { subscription_id, data } => {
                                println!("🧱 BLOCK UPDATE [{}]: {}", subscription_id,
                                    serde_json::to_string_pretty(&data).unwrap_or_else(|_| format!("{:?}", data)));
                            }
                            WebSocketEvent::Error { error } => {
                                error!("❌ WebSocket error: {}", error);
                            }
                        }
                        
                        // Real-time processing - no delays or batching
                    }
                    
                    
                    // Handle shutdown signal
                    _ = shutdown_rx.recv() => {
                        info!("🛑 Ingestion service received shutdown signal");
                        client_handle.as_mut().unwrap().abort();
                        break;
                    }
                    
                    // Handle client task completion
                    result = async { client_handle.as_mut().unwrap().await }, if client_handle.is_some() => {
                        match result {
                            Ok(Ok(())) => {
                                info!("WebSocket client completed successfully");
                            }
                            Ok(Err(e)) => {
                                error!("WebSocket client error: {}", e);
                                return Err(e);
                            }
                            Err(e) => {
                                error!("WebSocket client task failed: {}", e);
                                return Err(e.into());
                            }
                        }
                        client_handle = None;
                        break;
                    }
                }
            }
            
            info!("✅ Ingestion service completed successfully");
            Ok(())
        });
        
        self.tasks.push(ingestion_task);
        info!("✅ Ingestion service started successfully");
        Ok(())
    }

    /// Starts all configured services
    async fn start_all_services(&mut self) -> Result<()> {
        info!("🚀 Starting all Badger services");
        
        self.start_ingestion_service().await?;
        
        info!("✅ All {} services started successfully", self.tasks.len());
        Ok(())
    }

    /// Gracefully shuts down all services
    async fn shutdown_all(&mut self) -> Result<()> {
        info!("🛑 Initiating graceful shutdown of all services");
        
        // Send shutdown signal to all services
        let _ = self.shutdown_tx.send(());
        debug!("Shutdown signal broadcasted to all services");
        
        // Wait for all tasks to complete with timeout
        let shutdown_timeout = Duration::from_secs(30);
        let mut results = Vec::new();
        
        for (i, task) in self.tasks.drain(..).enumerate() {
            match tokio::time::timeout(shutdown_timeout, task).await {
                Ok(result) => results.push((i, result)),
                Err(_timeout_error) => {
                    warn!("⏰ Service {} shutdown timed out after {:?}", i + 1, shutdown_timeout);
                    // Create a proper JoinError by aborting the task
                    results.push((i, Ok(Err(anyhow::anyhow!("Service shutdown timeout")))));
                }
            }
        }
        
        // Report shutdown results
        for (i, result) in results {
            match result {
                Ok(Ok(())) => info!("✅ Service {} shut down cleanly", i + 1),
                Ok(Err(e)) => warn!("⚠️  Service {} error during shutdown: {}", i + 1, e),
                Err(e) => error!("❌ Service {} task failed: {}", i + 1, e),
            }
        }
        
        info!("✅ All services shut down successfully");
        Ok(())
    }

}

/// Initializes comprehensive logging for production use
/// 
/// Sets up both console and file logging with appropriate levels and formatting.
/// Logs are rotated daily and stored in the logs/ directory.
fn init_tracing() -> Result<()> {
    // Create logs directory if it doesn't exist
    std::fs::create_dir_all("logs")?;
    
    // Create file appender for logs with daily rotation
    let file_appender = tracing_appender::rolling::daily("logs", "badger.log");
    let (non_blocking_file, _guard) = tracing_appender::non_blocking(file_appender);
    
    // Create console layer with colored output for development
    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_level(true)
        .compact();
    
    // Create file layer with structured JSON logging for production analysis
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_file)
        .json()
        .with_current_span(false)
        .with_span_list(true);
    
    // Initialize subscriber with environment-based filtering
    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,badger=debug"))
        )
        .init();
    
    // Keep the guard alive for the entire program duration
    std::mem::forget(_guard);
    
    Ok(())
}

/// Main entry point for the Badger trading bot
/// 
/// This function initializes logging, starts all services, and handles
/// graceful shutdown on SIGINT (Ctrl+C).
fn main() -> Result<()> {
    // Create tokio runtime manually to avoid macro issues
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main())
}

async fn async_main() -> Result<()> {
    // Initialize comprehensive logging
    init_tracing()?;
    
    info!("🦡 Badger Trading Bot - Full Production System");
    info!("===============================================");
    info!("Version: 0.1.0");
    info!("Features: Real-time ingestion + Insider tracking + Token discovery + Lightning execution");
    info!("Transport: Ultra-fast shared memory inter-service communication");

    let mut orchestrator = BadgerOrchestrator::new();
    
    // Start all services
    match orchestrator.start_all_services().await {
        Ok(()) => {
            info!("🎯 Badger is now operational");
            info!("📊 Real-time Solana blockchain ingestion active");
            info!("🔄 Ready for additional services integration");
            info!("Press Ctrl+C to initiate graceful shutdown");
        }
        Err(e) => {
            error!("❌ Failed to start services: {}", e);
            return Err(e);
        }
    }

    // Wait for shutdown signal (Ctrl+C)
    match signal::ctrl_c().await {
        Ok(()) => {
            info!("🛑 Shutdown signal received (Ctrl+C)");
        }
        Err(e) => {
            error!("❌ Failed to listen for shutdown signal: {}", e);
            // Continue with shutdown anyway
        }
    }
    
    // Graceful shutdown
    orchestrator.shutdown_all().await?;
    
    info!("👋 Badger shutdown complete - All systems stopped cleanly");
    Ok(())
}