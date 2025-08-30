use anyhow::Result;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio::sync::broadcast;
use tracing::{info, warn, error, debug};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use std::time::Duration;
use std::sync::Arc;

use badger::ingest::websocket::{SolanaWebSocketClient, WebSocketConfig, WebSocketEvent};
use badger::ingest::DexEventParser;
use badger::core::{MarketEvent, TradingSignal, DexType};
use badger::transport::{
    EnhancedTransportBus, ServiceRegistry, ServiceInfo, ServiceType, ServiceCapability, 
    ServiceStatus, SubscriptionInfo, EventType, WalletEvent, SystemAlert
};

use chrono::Utc;
use std::collections::HashMap;

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

/// Parse and display program account updates using production DEX parser
fn parse_and_display_program_update(subscription_id: u64, data: &serde_json::Value) {
    // Use production DEX parser to extract real market events
    match DexEventParser::parse_program_update(subscription_id, data) {
        Ok(events) => {
            for event in events {
                display_market_event(&event);
                
                // Generate trading signals based on events (Phase 1 basic implementation)
                if let Some(signal) = generate_basic_trading_signal(&event) {
                    display_trading_signal(&signal);
                }
            }
        }
        Err(e) => {
            debug!("Failed to parse program update: {}", e);
            // Fallback to basic display for debugging
            if let Some(context) = data.get("context") {
                if let Some(slot) = context.get("slot").and_then(|s| s.as_u64()) {
                    if let Some(value) = data.get("value") {
                        if let Some(pubkey) = value.get("pubkey").and_then(|p| p.as_str()) {
                            if let Some(account) = value.get("account") {
                                if let Some(owner) = account.get("owner").and_then(|o| o.as_str()) {
                                    let dex_type = DexType::from_program_id(owner);
                                    println!("📋 UNKNOWN {:?} EVENT | Slot: {} | Account: {}...{}", 
                                        dex_type, slot, &pubkey[..8], &pubkey[pubkey.len()-8..]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Display market events in production format
fn display_market_event(event: &MarketEvent) {
    match event {
        MarketEvent::PoolCreated { pool, creator, initial_liquidity_sol } => {
            println!("🔥 NEW POOL CREATED");
            println!("   Pool: {} | DEX: {:?}", 
                &pool.address[..8], pool.dex);
            println!("   Tokens: {} ↔ {}", 
                &pool.base_mint[..8], &pool.quote_mint[..8]);
            println!("   Creator: {} | Liquidity: {:.3} SOL", 
                &creator[..8], initial_liquidity_sol);
            println!("   Slot: {} | Time: {}", pool.slot, pool.created_at.format("%H:%M:%S"));
        }
        MarketEvent::TokenLaunched { token } => {
            println!("🪙 NEW TOKEN LAUNCHED");
            println!("   Mint: {} | Symbol: {}", 
                &token.mint[..8], token.symbol);
            println!("   Supply: {} | Decimals: {}", token.supply, token.decimals);
            println!("   Mint Auth: {} | Freeze Auth: {}", 
                token.mint_authority.as_ref().map(|s| &s[..8]).unwrap_or("None"),
                token.freeze_authority.as_ref().map(|s| &s[..8]).unwrap_or("None"));
            println!("   Slot: {} | Time: {}", token.slot, token.created_at.format("%H:%M:%S"));
        }
        MarketEvent::SwapDetected { swap } => {
            println!("💱 SWAP DETECTED");
            println!("   Type: {:?} | DEX: {:?}", swap.swap_type, swap.dex);
            println!("   {} -> {} | Wallet: {}", 
                &swap.token_in[..8], &swap.token_out[..8], &swap.wallet[..8]);
            println!("   Signature: {} | Slot: {}", &swap.signature[..8], swap.slot);
        }
        MarketEvent::LargeTransferDetected { transfer } => {
            println!("💸 LARGE TRANSFER DETECTED");
            println!("   Token: {} | Amount: {}", &transfer.token_mint[..8], transfer.amount);
            println!("   From: {} -> To: {}", 
                &transfer.from_wallet[..8], &transfer.to_wallet[..8]);
            println!("   USD Value: ${:.2}", transfer.amount_usd.unwrap_or(0.0));
        }
        _ => {
            println!("📊 MARKET EVENT: {:?}", event);
        }
    }
}

/// Generate basic trading signals from market events (Phase 1 implementation)
fn generate_basic_trading_signal(event: &MarketEvent) -> Option<TradingSignal> {
    match event {
        MarketEvent::PoolCreated { pool, initial_liquidity_sol, .. } => {
            // Basic pool creation signal
            if *initial_liquidity_sol > 5.0 && pool.dex != DexType::Unknown {
                Some(TradingSignal::Buy {
                    token_mint: pool.base_mint.clone(),
                    confidence: 0.6, // Medium confidence for new pools
                    max_amount_sol: initial_liquidity_sol * 0.1, // Max 10% of pool liquidity
                    reason: format!("New pool on {:?} with {:.1} SOL liquidity", pool.dex, initial_liquidity_sol),
                    source: badger::core::SignalSource::NewPool,
                })
            } else {
                None
            }
        }
        MarketEvent::TokenLaunched { token } => {
            // Basic new token signal
            if token.mint_authority.is_none() && token.freeze_authority.is_none() {
                Some(TradingSignal::Buy {
                    token_mint: token.mint.clone(),
                    confidence: 0.8, // High confidence for renounced tokens
                    max_amount_sol: 1.0, // Conservative 1 SOL max
                    reason: "New token with renounced mint and freeze authority".to_string(),
                    source: badger::core::SignalSource::NewPool,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Display trading signals in production format
fn display_trading_signal(signal: &TradingSignal) {
    match signal {
        TradingSignal::Buy { token_mint, confidence, max_amount_sol, reason, source } => {
            println!("🎯 BUY SIGNAL GENERATED");
            println!("   Token: {} | Confidence: {:.1}%", 
                &token_mint[..8], confidence * 100.0);
            println!("   Max Amount: {:.3} SOL | Source: {:?}", max_amount_sol, source);
            println!("   Reason: {}", reason);
        }
        TradingSignal::Sell { token_mint, price_target, stop_loss, reason } => {
            println!("💰 SELL SIGNAL GENERATED");
            println!("   Token: {} | Target: {:.6} | Stop: {:.6}", 
                &token_mint[..8], price_target, stop_loss);
            println!("   Reason: {}", reason);
        }
        TradingSignal::SwapActivity { token_mint, volume_increase, whale_activity } => {
            println!("📈 SWAP ACTIVITY DETECTED");
            println!("   Token: {} | Volume +{:.1}% | Whale: {}", 
                &token_mint[..8], volume_increase * 100.0, whale_activity);
        }
    }
}

/// Production-ready Badger trading bot orchestrator
/// 
/// This orchestrator manages the core WebSocket ingestion system for real-time
/// Solana blockchain data processing with the enhanced transport layer for
/// service communication. It handles graceful startup, shutdown,
/// and error recovery for all services.
struct BadgerOrchestrator {
    shutdown_tx: broadcast::Sender<()>,
    tasks: Vec<JoinHandle<Result<()>>>,
    websocket_config: WebSocketConfig,
    transport_bus: Arc<EnhancedTransportBus>,
    service_registry: Arc<ServiceRegistry>,
    database_manager: Option<badger::DatabaseManager>,
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
        
        // Initialize the enhanced transport bus
        let transport_bus = Arc::new(EnhancedTransportBus::new());
        
        // Initialize the service registry
        let service_registry = Arc::new(ServiceRegistry::new(transport_bus.clone()));
        
        Self {
            shutdown_tx,
            tasks: Vec::new(),
            websocket_config,
            transport_bus,
            service_registry,
            database_manager: None,
        }
    }

    /// Initialize the database services (Phase 3)
    async fn initialize_database_services(&mut self) -> Result<()> {
        info!("🗄️ Initializing Phase 3 Database Services");
        
        // Initialize database manager (directory creation handled in database layer)
        let mut database_manager = badger::DatabaseManager::new();
        
        // Initialize with transport bus and service registry
        if let Err(e) = database_manager.initialize(
            self.transport_bus.clone(),
            self.service_registry.clone(),
        ).await {
            error!("Failed to initialize database manager: {}", e);
            return Err(anyhow::anyhow!("Database initialization failed: {}", e));
        }
        
        // Start all database services
        let db_handles = database_manager.start_all_services().await
            .map_err(|e| anyhow::anyhow!("Failed to start database services: {}", e))?;
        
        // Convert database service handles to our handle type
        for handle in db_handles {
            let converted_handle = tokio::spawn(async move {
                match handle.await {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(e)) => Err(anyhow::anyhow!("Database service error: {}", e)),
                    Err(e) => Err(anyhow::anyhow!("Database service join error: {}", e)),
                }
            });
            self.tasks.push(converted_handle);
        }
        
        self.database_manager = Some(database_manager);
        
        info!("✅ Phase 3 Database Services initialized successfully");
        Ok(())
    }

    /// Starts the core WebSocket ingestion service with enhanced transport integration
    /// 
    /// This service maintains persistent connections to Solana RPC WebSocket endpoints
    /// and processes real-time blockchain data including account updates, transactions,
    /// and program events. All events are routed through the enhanced transport bus.
    async fn start_ingestion_service(&mut self) -> Result<()> {
        info!("🔄 Starting Enhanced Badger Ingestion Service with Transport Layer");
        info!("Connecting to Solana mainnet WebSocket endpoints");
        
        // Register the ingestion service
        let ingestion_service = ServiceInfo {
            id: "ingestion-service-001".to_string(),
            name: "Solana WebSocket Ingestion".to_string(),
            service_type: ServiceType::Ingestion,
            version: "1.0.0".to_string(),
            capabilities: vec![
                ServiceCapability::MarketEventProducer,
                ServiceCapability::TradingSignalProducer,
            ],
            subscriptions: vec![], // Ingestion service doesn't subscribe, it produces
            status: ServiceStatus::Starting,
            registered_at: Utc::now(),
            last_heartbeat: Utc::now(),
            metadata: HashMap::new(),
        };
        
        self.service_registry.register_service(ingestion_service).await?;
        
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let config = self.websocket_config.clone();
        let service_registry = self.service_registry.clone();
        
        let ingestion_task = tokio::spawn(async move {
            info!("🚀 Badger Ingest - Real-time Solana Data Processing");
            
            // Initialize WebSocket client
            let (client, mut event_rx) = match SolanaWebSocketClient::new(config) {
                Ok((client, rx)) => {
                    info!("✅ WebSocket client initialized successfully");
                    
                    // Send a system startup alert through transport bus
                    if let Err(e) = service_registry.route_system_alert(
                        badger::transport::SystemAlert::ServiceStartup {
                            service: "Solana WebSocket Ingestion".to_string(),
                            version: "1.0.0".to_string(),
                        },
                        Some("ingestion-service-001")
                    ).await {
                        warn!("Failed to send startup alert: {}", e);
                    } else {
                        println!("📤 Sent service startup alert to transport bus");
                    }
                    
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
                                
                                // Update service status to healthy
                                if let Err(e) = service_registry.update_service_status(
                                    "ingestion-service-001", 
                                    ServiceStatus::Healthy
                                ).await {
                                    warn!("Failed to update service status: {}", e);
                                }
                            }
                            WebSocketEvent::Disconnected { reason } => {
                                warn!("🔴 WebSocket disconnected: {}", reason);
                            }
                            WebSocketEvent::SubscriptionConfirmed { subscription_id, request_id } => {
                                info!("✅ Subscription confirmed: {} (request: {})", subscription_id, request_id);
                                let sub_type = match request_id {
                                    999 => "Slot Updates",
                                    998 => "USDC Account", 
                                    997 => "Raydium Program",
                                    996 => "Jupiter Program",
                                    995 => "Orca Program", 
                                    994 => "SPL Token Program",
                                    993 => "Pump.fun Program",
                                    _ => "Unknown"
                                };
                                println!("🎯 SUBSCRIPTION CONFIRMED: {} (sub: {}, req: {})", sub_type, subscription_id, request_id);
                                println!("   📡 This subscription will send program account updates for DEX analysis");
                            }
                            WebSocketEvent::AccountUpdate { subscription_id, data } => {
                                parse_and_display_account_update(subscription_id, &data);
                            }
                            WebSocketEvent::TransactionNotification { subscription_id, data } => {
                                println!("📈 TRANSACTION [{}]: {}", subscription_id,
                                    serde_json::to_string_pretty(&data).unwrap_or_else(|_| format!("{:?}", data)));
                            }
                            WebSocketEvent::ProgramAccountUpdate { subscription_id, data } => {
                                println!("🔍 PROGRAM UPDATE [sub: {}] - analyzing for DEX events", subscription_id);
                                
                                // Show some context about the update
                                if let Some(context) = data.get("context") {
                                    if let Some(slot) = context.get("slot") {
                                        println!("   📍 Slot: {}", slot);
                                    }
                                }
                                if let Some(value) = data.get("value") {
                                    if let Some(pubkey) = value.get("pubkey") {
                                        println!("   🔑 Account: {}", pubkey.as_str().unwrap_or("unknown")[..std::cmp::min(16, pubkey.as_str().unwrap_or("").len())].to_string() + "...");
                                    }
                                    if let Some(account) = value.get("account") {
                                        if let Some(owner) = account.get("owner") {
                                            println!("   👤 Owner: {}", owner.as_str().unwrap_or("unknown"));
                                        }
                                    }
                                }
                                
                                // Parse DEX events and route through transport layer
                                match DexEventParser::parse_program_update(subscription_id, &data) {
                                    Ok(market_events) => {
                                        if market_events.is_empty() {
                                            println!("   ⚪ No market events parsed from this update (normal - most updates aren't DEX events)");
                                        } else {
                                            println!("   ✅ Parsed {} market events - routing through transport bus", market_events.len());
                                        }
                                        
                                        for market_event in market_events {
                                            // Display the event (for Phase 1 compatibility)
                                            display_market_event(&market_event);
                                            
                                            // Route through transport layer (Phase 2 enhancement)
                                            match service_registry.route_market_event(
                                                market_event.clone(), 
                                                Some("ingestion-service-001")
                                            ).await {
                                                Ok(_) => println!("   📤 MarketEvent routed to transport bus successfully"),
                                                Err(e) => warn!("Failed to route market event: {}", e),
                                            }
                                            
                                            // Generate and route trading signals
                                            if let Some(signal) = generate_basic_trading_signal(&market_event) {
                                                display_trading_signal(&signal);
                                                
                                                // Route signal through transport layer
                                                match service_registry.route_trading_signal(
                                                    signal,
                                                    Some("ingestion-service-001")
                                                ).await {
                                                    Ok(_) => println!("   📤 TradingSignal routed to transport bus successfully"),
                                                    Err(e) => warn!("Failed to route trading signal: {}", e),
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        println!("   ❌ DEX Parser failed: {} (this is normal for non-DEX account updates)", e);
                                        // Show basic account info for debugging
                                        if let Some(value) = data.get("value") {
                                            if let Some(account) = value.get("account") {
                                                if let Some(owner) = account.get("owner").and_then(|o| o.as_str()) {
                                                    let dex_type = badger::core::DexType::from_program_id(owner);
                                                    if dex_type != badger::core::DexType::Unknown {
                                                        println!("   🤔 This was a {:?} program update but parsing failed - might need parser improvement", dex_type);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
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
                        info!("🛑 Ingestion service received shutdown signal - aborting immediately");
                        if let Some(handle) = client_handle.as_mut() {
                            handle.abort();
                        }
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

    /// Starts a transport monitoring service to demonstrate Phase 2 functionality
    async fn start_transport_monitoring_service(&mut self) -> Result<()> {
        info!("🔄 Starting Transport Monitoring Service");
        
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let transport_bus = self.transport_bus.clone();
        let service_registry = self.service_registry.clone();
        
        // Use a one-shot channel to synchronize subscription completion
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        
        let monitor_task = tokio::spawn(async move {
            info!("🚀 Transport Monitor - Subscribing to all event channels");
            
            // Subscribe to all event types FIRST
            let mut market_events = transport_bus.subscribe_market_events().await;
            let mut trading_signals = transport_bus.subscribe_trading_signals().await;
            let mut wallet_events = transport_bus.subscribe_wallet_events().await;
            let mut system_alerts = transport_bus.subscribe_system_alerts().await;
            
            info!("📊 Transport Monitor subscriptions complete - signaling ready");
            
            // Signal that subscriptions are ready
            let _ = ready_tx.send(());
            
            // Update service to healthy
            if let Err(e) = service_registry.update_service_status(
                "transport-monitor-001", 
                ServiceStatus::Healthy
            ).await {
                warn!("Failed to update monitor service status: {}", e);
            }
            
            info!("📊 Transport Monitor active - listening for events");
            println!("🎧 TRANSPORT MONITOR: Ready to receive events on all channels");
            
            loop {
                tokio::select! {
                    Ok(market_event) = market_events.recv() => {
                        println!("📈 TRANSPORT BUS - MarketEvent received:");
                        info!("📈 TRANSPORT BUS - MarketEvent received:");
                        match &market_event {
                            MarketEvent::PoolCreated { pool, creator, initial_liquidity_sol } => {
                                println!("  🔥 Pool Created: {} | DEX: {:?} | Creator: {}...{} | Liquidity: {:.3} SOL", 
                                    &pool.address[..8], pool.dex, &creator[..4], &creator[creator.len()-4..], initial_liquidity_sol);
                            }
                            MarketEvent::TokenLaunched { token } => {
                                println!("  🪙 Token Launched: {} | Symbol: {} | Supply: {}", 
                                    &token.mint[..8], token.symbol, token.supply);
                                println!("      Mint Auth: {} | Freeze Auth: {}", 
                                    token.mint_authority.as_ref().map(|s| &s[..8]).unwrap_or("None"),
                                    token.freeze_authority.as_ref().map(|s| &s[..8]).unwrap_or("None"));
                            }
                            MarketEvent::SwapDetected { swap } => {
                                println!("  💱 Swap: {} | {} -> {} | Wallet: {}...{} | DEX: {:?}", 
                                    &swap.signature[..8], &swap.token_in[..8], &swap.token_out[..8], 
                                    &swap.wallet[..4], &swap.wallet[swap.wallet.len()-4..], swap.dex);
                            }
                            MarketEvent::LargeTransferDetected { transfer } => {
                                println!("  💸 Large Transfer: {} | Token: {} | Amount: {} | USD: ${:.2}", 
                                    &transfer.signature[..8], &transfer.token_mint[..8], 
                                    transfer.amount, transfer.amount_usd.unwrap_or(0.0));
                            }
                            _ => {
                                println!("  📊 Other MarketEvent: {:?}", std::mem::discriminant(&market_event));
                            }
                        }
                    }
                    Ok(trading_signal) = trading_signals.recv() => {
                        println!("🎯 TRANSPORT BUS - TradingSignal received:");
                        info!("🎯 TRANSPORT BUS - TradingSignal received:");
                        match &trading_signal {
                            TradingSignal::Buy { token_mint, confidence, max_amount_sol, reason, source } => {
                                println!("  🟢 BUY SIGNAL: Token: {} | Confidence: {:.1}% | Max: {:.3} SOL", 
                                    &token_mint[..8], confidence * 100.0, max_amount_sol);
                                println!("      Reason: {} | Source: {:?}", reason, source);
                            }
                            TradingSignal::Sell { token_mint, price_target, stop_loss, reason } => {
                                println!("  🔴 SELL SIGNAL: Token: {} | Target: {:.6} | Stop: {:.6}", 
                                    &token_mint[..8], price_target, stop_loss);
                                println!("      Reason: {}", reason);
                            }
                            TradingSignal::SwapActivity { token_mint, volume_increase, whale_activity } => {
                                println!("  📈 SWAP ACTIVITY: Token: {} | Volume +{:.1}% | Whale: {}", 
                                    &token_mint[..8], volume_increase * 100.0, whale_activity);
                            }
                        }
                    }
                    Ok(wallet_event) = wallet_events.recv() => {
                        println!("👛 TRANSPORT BUS - WalletEvent received:");
                        info!("👛 TRANSPORT BUS - WalletEvent received:");
                        match &wallet_event {
                            WalletEvent::InsiderActivity { wallet, action, token_mint, amount_sol, confidence, .. } => {
                                println!("  🕵️ Insider Activity: Wallet: {}...{} | Action: {:?}", 
                                    &wallet[..4], &wallet[wallet.len()-4..], action);
                                println!("      Token: {} | Amount: {:.3} SOL | Confidence: {:.1}%", 
                                    &token_mint[..8], amount_sol, confidence * 100.0);
                            }
                            WalletEvent::NewInsiderDetected { wallet, success_rate, total_trades, .. } => {
                                println!("  🎯 New Insider: {}...{} | Success: {:.1}% | Trades: {}", 
                                    &wallet[..4], &wallet[wallet.len()-4..], success_rate * 100.0, total_trades);
                            }
                            _ => {
                                println!("  👛 Other WalletEvent: {:?}", std::mem::discriminant(&wallet_event));
                            }
                        }
                    }
                    Ok(system_alert) = system_alerts.recv() => {
                        println!("🚨 TRANSPORT BUS - SystemAlert received:");
                        info!("🚨 TRANSPORT BUS - SystemAlert received:");
                        match &system_alert {
                            SystemAlert::ServiceStartup { service, version } => {
                                println!("  🟢 Service Started: {} v{}", service, version);
                            }
                            SystemAlert::ServiceShutdown { service, reason, uptime_seconds } => {
                                println!("  🔴 Service Stopped: {} | Reason: {} | Uptime: {}s", 
                                    service, reason, uptime_seconds);
                            }
                            SystemAlert::ConnectionIssue { service, error, .. } => {
                                println!("  ⚠️ Connection Issue: {} | Error: {}", service, error);
                            }
                            SystemAlert::HighTrafficDetected { events_per_minute, threshold, service } => {
                                println!("  🔥 High Traffic: {} | {}/min (threshold: {})", 
                                    service, events_per_minute, threshold);
                            }
                            _ => {
                                println!("  🚨 Other SystemAlert: {:?}", std::mem::discriminant(&system_alert));
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("🛑 Transport Monitor received shutdown signal");
                        break;
                    }
                }
            }
            
            info!("✅ Transport Monitor completed successfully");
            Ok(())
        });
        
        // Wait for subscriptions to be ready before proceeding
        info!("⏳ Waiting for monitoring service subscriptions to complete...");
        ready_rx.await.map_err(|_| anyhow::anyhow!("Monitor service failed to start"))?;
        info!("✅ Monitoring service subscriptions ready");
        
        // Now register the service (after subscriptions are active)
        let monitoring_service = ServiceInfo {
            id: "transport-monitor-001".to_string(),
            name: "Transport Layer Monitor".to_string(),
            service_type: ServiceType::Utility,
            version: "1.0.0".to_string(),
            capabilities: vec![
                ServiceCapability::MarketEventConsumer,
                ServiceCapability::TradingSignalConsumer,
            ],
            subscriptions: vec![
                SubscriptionInfo {
                    event_type: EventType::MarketEvent,
                    filters: vec![],
                    subscribed_at: Utc::now(),
                },
                SubscriptionInfo {
                    event_type: EventType::TradingSignal,
                    filters: vec![],
                    subscribed_at: Utc::now(),
                },
            ],
            status: ServiceStatus::Starting,
            registered_at: Utc::now(),
            last_heartbeat: Utc::now(),
            metadata: HashMap::new(),
        };
        
        self.service_registry.register_service(monitoring_service).await?;
        
        self.tasks.push(monitor_task);
        info!("✅ Transport monitoring service started and registered");
        Ok(())
    }

    /// Starts all configured services
    async fn start_all_services(&mut self) -> Result<()> {
        info!("🚀 Starting all Badger services with Enhanced Transport Layer + Phase 3 Database");
        
        // Start transport monitoring first to capture all events
        self.start_transport_monitoring_service().await?;
        
        // Initialize Phase 3 database services
        self.initialize_database_services().await?;
        
        // Start ingestion service
        self.start_ingestion_service().await?;
        
        // Display transport bus statistics and start periodic monitoring
        let stats = self.transport_bus.get_statistics().await;
        info!("📊 Initial Transport Bus Statistics:");
        info!("  - Market Event Subscribers: {}", stats.market_subscribers);
        info!("  - Trading Signal Subscribers: {}", stats.signal_subscribers);  
        info!("  - Wallet Event Subscribers: {}", stats.wallet_subscribers);
        info!("  - System Alert Subscribers: {}", stats.alert_subscribers);
        
        // Start periodic transport statistics reporting
        let transport_stats_bus = self.transport_bus.clone();
        let stats_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                let stats = transport_stats_bus.get_statistics().await;
                
                if stats.market_events_sent > 0 || stats.trading_signals_sent > 0 || 
                   stats.wallet_events_sent > 0 || stats.system_alerts_sent > 0 {
                    println!("\n📈 TRANSPORT BUS ACTIVITY (Last 30s):");
                    println!("  🔥 Market Events: {} sent | {} subscribers", 
                        stats.market_events_sent, stats.market_subscribers);
                    println!("  🎯 Trading Signals: {} sent | {} subscribers", 
                        stats.trading_signals_sent, stats.signal_subscribers);
                    println!("  👛 Wallet Events: {} sent | {} subscribers", 
                        stats.wallet_events_sent, stats.wallet_subscribers);
                    println!("  🚨 System Alerts: {} sent | {} subscribers", 
                        stats.system_alerts_sent, stats.alert_subscribers);
                }
            }
        });
        
        self.tasks.push(stats_task);
        
        info!("✅ All {} services started successfully", self.tasks.len());
        
        println!("\n🔍 PHASE 3: ENHANCED DATA PERSISTENCE & ANALYTICS");
        println!("   📊 Listening for real-time Solana DEX activity");
        println!("   🗄️ Database Services Active:");
        println!("      • PersistenceService - Storing all events");
        println!("      • AnalyticsService - Real-time performance tracking");
        println!("      • WalletTrackerService - Insider scoring system");
        println!("      • QueryService - High-performance data queries");
        println!("   🎯 Market events will appear when DEX transactions occur:");
        println!("      • New Raydium AMM pools created");
        println!("      • Jupiter aggregator swaps executed"); 
        println!("      • Orca Whirlpool activity detected");
        println!("      • New tokens launched on Pump.fun");
        println!("      • Large SPL token transfers");
        println!("   ⏳ Note: Real DEX events may be infrequent - this is normal");
        println!("   📈 Analytics and database stats will update periodically\n");
        
        Ok(())
    }

    /// Gracefully shuts down all services
    async fn shutdown_all(&mut self) -> Result<()> {
        info!("🛑 Initiating graceful shutdown of all services");
        
        // Send shutdown signal to all services
        let _ = self.shutdown_tx.send(());
        debug!("Shutdown signal broadcasted to all services");
        
        // Wait for all tasks to complete with shorter timeout for faster shutdown
        let shutdown_timeout = Duration::from_secs(5);
        let mut results = Vec::new();
        
        for (i, task) in self.tasks.drain(..).enumerate() {
            match tokio::time::timeout(shutdown_timeout, task).await {
                Ok(result) => results.push((i, result)),
                Err(_timeout_error) => {
                    warn!("⏰ Service {} shutdown timed out after {:?} - was force terminated", i + 1, shutdown_timeout);
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
    // let file_appender = tracing_appender::rolling::daily("logs", "badger.log");
    // let (non_blocking_file, _guard) = tracing_appender::non_blocking(file_appender);
    
    // Create console layer with colored output for development
    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_level(true)
        .compact();
    
    // // Create file layer with structured JSON logging for production analysis
    // let file_layer = tracing_subscriber::fmt::layer()
    //     .with_writer(non_blocking_file)
    //     .json()
    //     .with_current_span(false)
    //     .with_span_list(true);
    
    // Initialize subscriber with environment-based filtering
    tracing_subscriber::registry()
        .with(console_layer)
        //.with(file_layer)
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,badger=debug"))
        )
        .init();
    
    // Keep the guard alive for the entire program duration
    //std::mem::forget(_guard);
    
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
    
    info!("🦡 Badger Trading Bot - Phase 3 Data Persistence & Analytics");
    info!("==============================================================");
    info!("Version: 0.3.0-phase3");
    info!("Phase 3 Features:");
    info!("  🔥 Real-time Raydium AMM pool monitoring");
    info!("  ⚡ Jupiter V6 aggregator event tracking");
    info!("  🌊 Orca Whirlpool program monitoring");
    info!("  🪙 SPL Token new mint detection");
    info!("  🚀 Pump.fun meme coin launch tracking");
    info!("  🎯 Advanced trading signal generation");
    info!("  🗄️ Persistent event storage and analytics");
    info!("  📊 Real-time performance tracking");
    info!("  🕵️ Wallet intelligence and insider scoring");
    info!("  🔍 High-performance data queries");
    info!("Performance: Zero-delay processing + comprehensive data persistence");

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
            info!("🛑 Shutdown signal received (Ctrl+C) - initiating immediate shutdown");
            println!("🛑 Shutting down Badger...");
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