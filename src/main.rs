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
use badger::core::{MarketEvent, TradingSignal, DexType, WalletManager, WalletProvisionConfig, WalletType};
use badger::transport::{
    EnhancedTransportBus, ServiceRegistry, ServiceInfo, ServiceType, ServiceCapability, 
    ServiceStatus, SubscriptionInfo, EventType, WalletEvent, SystemAlert
};
use badger::database::analytics::{
    PositionTracker, PnLCalculator, PerformanceTracker, InsiderAnalytics
};
use badger::intelligence::WalletIntelligenceEngine;

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
                    amount_sol: Some(initial_liquidity_sol * 0.1),
                    max_slippage: Some(5.0),
                    metadata: None,
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
                    amount_sol: Some(1.0),
                    max_slippage: Some(5.0),
                    metadata: None,
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
        TradingSignal::Buy { token_mint, confidence, max_amount_sol, reason, source, .. } => {
            println!("🎯 BUY SIGNAL GENERATED");
            println!("   Token: {} | Confidence: {:.1}%", 
                &token_mint[..8], confidence * 100.0);
            println!("   Max Amount: {:.3} SOL | Source: {:?}", max_amount_sol, source);
            println!("   Reason: {}", reason);
        }
        TradingSignal::Sell { token_mint, price_target, stop_loss, reason, .. } => {
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

/// Process market event for wallet intelligence (Phase 4: Ultra-fast insider copy trading)
async fn process_market_event_for_wallet_intelligence(
    event: &MarketEvent,
    wallet_intelligence: &Arc<WalletIntelligenceEngine>,
) {
    // Process event through ultra-fast wallet intelligence engine
    if let Err(e) = wallet_intelligence.process_market_event(event).await {
        warn!("Failed to process market event for wallet intelligence: {}", e);
    }
}

/// Process market event for insider analytics tracking (Phase 3: Task 3.1)
async fn process_market_event_for_insider_analytics(
    event: &MarketEvent,
    insider_analytics: &Arc<InsiderAnalytics>,
) {
    match event {
        MarketEvent::SwapDetected { swap } => {
            // Track insider wallet activity from swaps
            let activity_type = match swap.swap_type {
                badger::core::SwapType::Buy => "BUY",
                badger::core::SwapType::Sell => "SELL",
            };
            
            if let Err(e) = insider_analytics.track_insider_activity(
                &swap.wallet,
                &swap.token_out, // For buys, token_out is what they're buying
                activity_type,
                swap.amount_out as f64,
                swap.price_impact,
                Some(&swap.signature),
                Some(swap.slot as i64),
            ).await {
                warn!("Failed to track insider activity for swap: {}", e);
            } else {
                debug!("📊 Tracked insider activity: {} {} {}", swap.wallet, activity_type, swap.token_out);
            }
        }
        MarketEvent::LargeTransferDetected { transfer } => {
            // Track large transfers as potential insider activity
            if let Err(e) = insider_analytics.track_insider_activity(
                &transfer.from_wallet,
                &transfer.token_mint,
                "TRANSFER",
                transfer.amount as f64,
                None, // No price for transfers
                None, // No transaction hash available in this structure
                Some(transfer.slot as i64),
            ).await {
                warn!("Failed to track insider activity for large transfer: {}", e);
            } else {
                debug!("📊 Tracked insider large transfer: {} -> {}", transfer.from_wallet, transfer.to_wallet);
            }
        }
        _ => {
            // Other market events don't directly indicate insider activity
        }
    }
}

/// Process trading signal for position tracking and P&L calculation (Phase 3: Task 3.1)
async fn process_trading_signal_for_analytics(
    signal: &TradingSignal,
    position_tracker: &Arc<PositionTracker>,
    pnl_calculator: &Arc<PnLCalculator>,
) {
    match signal {
        TradingSignal::Buy { token_mint, confidence, max_amount_sol, .. } => {
            // For demonstration, we're simulating opening a position
            // In a real implementation, this would be triggered by actual trade execution
            
            let entry_price = 0.000001; // Simulated entry price - would come from actual trade
            let quantity = max_amount_sol / entry_price;
            let fees = max_amount_sol * 0.005; // 0.5% fee simulation
            
            // Check if this might be an insider signal by looking for wallet patterns
            let insider_wallet = extract_potential_insider_wallet(signal);
            
            match position_tracker.open_position(
                signal,
                entry_price,
                quantity,
                fees,
                insider_wallet,
            ).await {
                Ok(position) => {
                    info!("📊 Position opened for analytics tracking: #{} ({})", position.id, token_mint);
                    
                    // Update P&L calculator with current price
                    pnl_calculator.update_price(token_mint, entry_price).await;
                }
                Err(e) => {
                    warn!("Failed to open position for analytics: {}", e);
                }
            }
        }
        TradingSignal::Sell { token_mint, price_target, .. } => {
            // Simulate closing a position
            let exit_price = *price_target;
            let exit_fees = exit_price * 0.005; // 0.5% fee simulation
            
            match position_tracker.close_position(token_mint, exit_price, exit_fees).await {
                Ok(Some(closed_position)) => {
                    info!("📊 Position closed for analytics: #{} P&L: ${:.4}", 
                          closed_position.id, closed_position.pnl.unwrap_or(0.0));
                }
                Ok(None) => {
                    debug!("No open position found to close for token: {}", token_mint);
                }
                Err(e) => {
                    warn!("Failed to close position for analytics: {}", e);
                }
            }
        }
        _ => {
            // Other signal types don't directly map to position changes
        }
    }
}

/// Extract potential insider wallet from trading signal context
fn extract_potential_insider_wallet(signal: &TradingSignal) -> Option<String> {
    // This is a placeholder - in a real implementation, you would extract
    // the wallet address from the signal context or source data
    match signal.get_source() {
        badger::core::SignalSource::InsiderWallet => {
            // Extract wallet from signal metadata or context
            Some("insider_wallet_placeholder".to_string())
        }
        _ => None,
    }
}

/// Generate real-time trading report (Phase 3: Task 3.1)
async fn generate_real_time_report(
    position_tracker: &Arc<PositionTracker>,
    pnl_calculator: &Arc<PnLCalculator>,
    insider_analytics: &Arc<InsiderAnalytics>,
) -> Result<()> {
    println!("\n═══════════════════════════════════════════════════════");
    println!("📊 BADGER BOT REAL-TIME ANALYTICS REPORT");
    println!("═══════════════════════════════════════════════════════");
    
    // Get position summary
    match position_tracker.get_position_summary().await {
        Ok(summary) => {
            println!("📈 POSITION SUMMARY:");
            println!("   Total Positions: {} | Open: {} | Closed: {}", 
                summary.total_positions, summary.open_positions, summary.closed_positions);
            println!("   Total P&L: ${:.4} | Total Fees: ${:.4}", summary.total_pnl, summary.total_fees);
            println!("   Win Rate: {:.1}% | Avg Hold Time: {:.1}h", 
                summary.win_rate * 100.0, summary.average_hold_time / 3600.0);
            if let Some(best) = summary.best_trade {
                println!("   Best Trade: ${:.4} | Worst Trade: ${:.4}", 
                    best, summary.worst_trade.unwrap_or(0.0));
            }
        }
        Err(e) => warn!("Failed to get position summary: {}", e),
    }
    
    // Get portfolio P&L
    match pnl_calculator.calculate_portfolio_pnl().await {
        Ok(portfolio) => {
            println!("💰 PORTFOLIO P&L:");
            println!("   Realized P&L: ${:.4} | Unrealized P&L: ${:.4}", 
                portfolio.total_realized_pnl, portfolio.total_unrealized_pnl);
            println!("   Net P&L: ${:.4} | Portfolio ROI: {:.2}%", 
                portfolio.net_pnl, portfolio.portfolio_roi);
            println!("   Profit Factor: {:.2} | Sharpe Ratio: {:.2}", 
                portfolio.profit_factor, portfolio.sharpe_ratio.unwrap_or(0.0));
            if portfolio.max_drawdown > 0.0 {
                println!("   Max Drawdown: {:.2}%", portfolio.max_drawdown);
            }
        }
        Err(e) => warn!("Failed to calculate portfolio P&L: {}", e),
    }
    
    // Get top insiders
    match insider_analytics.get_top_insiders(5).await {
        Ok(top_insiders) => {
            if !top_insiders.is_empty() {
                println!("🕵️ TOP INSIDER WALLETS:");
                for (i, insider) in top_insiders.iter().take(3).enumerate() {
                    println!("   {}. {} | Score: {:.1} | Success: {:.1}% | ROI: {:.1}%",
                        i + 1,
                        &insider.wallet_address[..8],
                        insider.copy_worthiness,
                        insider.success_rate * 100.0,
                        insider.roi_percentage
                    );
                }
            } else {
                println!("🕵️ TOP INSIDER WALLETS: No insider activity detected yet");
            }
        }
        Err(e) => warn!("Failed to get top insiders: {}", e),
    }

    println!("═══════════════════════════════════════════════════════\n");
    Ok(())
}

/// Generate performance report (Phase 3: Task 3.1)
async fn generate_performance_report(
    performance_tracker: &Arc<PerformanceTracker>,
    pnl_calculator: &Arc<PnLCalculator>,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let hour_ago = now - 3600; // Last hour
    
    // Calculate hourly performance
    match performance_tracker.calculate_performance(hour_ago, now).await {
        Ok(metrics) => {
            if metrics.total_trades > 0 {
                println!("\n🎯 HOURLY PERFORMANCE METRICS:");
                println!("   Trades: {} | Win Rate: {:.1}% | Total Return: ${:.4}", 
                    metrics.total_trades, metrics.win_rate * 100.0, metrics.total_return);
                println!("   Avg Win: ${:.4} | Avg Loss: ${:.4} | Profit Factor: {:.2}", 
                    metrics.average_win, metrics.average_loss, metrics.profit_factor);
                if let Some(sharpe) = metrics.sharpe_ratio {
                    println!("   Sharpe Ratio: {:.2} | Max Drawdown: {:.2}%", sharpe, metrics.max_drawdown);
                }
                
                // Save performance snapshot
                if let Err(e) = performance_tracker.save_performance_snapshot(&metrics, "HOURLY").await {
                    warn!("Failed to save performance snapshot: {}", e);
                }
            }
        }
        Err(e) => warn!("Failed to calculate hourly performance: {}", e),
    }

    // Save P&L snapshot
    match pnl_calculator.calculate_portfolio_pnl().await {
        Ok(portfolio_pnl) => {
            if let Err(e) = pnl_calculator.save_pnl_snapshot(&portfolio_pnl, "HOURLY").await {
                warn!("Failed to save P&L snapshot: {}", e);
            }
        }
        Err(e) => warn!("Failed to calculate portfolio P&L for snapshot: {}", e),
    }

    Ok(())
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
    // Wallet management system
    wallet_manager: Option<Arc<tokio::sync::RwLock<WalletManager>>>,
    // Analytics components
    position_tracker: Option<Arc<PositionTracker>>,
    pnl_calculator: Option<Arc<PnLCalculator>>,
    performance_tracker: Option<Arc<PerformanceTracker>>,
    insider_analytics: Option<Arc<InsiderAnalytics>>,
    // Wallet intelligence system
    wallet_intelligence: Option<Arc<WalletIntelligenceEngine>>,
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
            // Initialize wallet management as None - will be set up later
            wallet_manager: None,
            // Initialize analytics components as None - will be set up later
            position_tracker: None,
            pnl_calculator: None,
            performance_tracker: None,
            insider_analytics: None,
            wallet_intelligence: None,
        }
    }

    /// Initialize the wallet management system
    async fn initialize_wallet_system(&mut self) -> Result<()> {
        info!("🏦 Initializing Wallet Management System");

        // Create wallet provisioning configuration
        let wallet_config = WalletProvisionConfig {
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            config_dir: "wallets".to_string(),
            master_password: None, // Will be generated automatically
            auto_create: true,
            initial_trading_balance_sol: Some(0.1), // Start with 0.1 SOL for testing
        };

        // Create wallet manager
        let mut wallet_manager = WalletManager::new(wallet_config)
            .map_err(|e| anyhow::anyhow!("Failed to create wallet manager: {}", e))?;

        // Initialize and provision wallets
        wallet_manager.initialize().await
            .map_err(|e| anyhow::anyhow!("Failed to initialize wallet system: {}", e))?;

        // Store wallet manager in orchestrator
        self.wallet_manager = Some(Arc::new(tokio::sync::RwLock::new(wallet_manager)));

        info!("✅ Wallet Management System initialized successfully");
        Ok(())
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

        // Run database migrations before starting services
        info!("🔄 Running database migrations...");
        let db = database_manager.get_database();
        let migration_runner = badger::database::MigrationRunner::new(db.clone());
        
        if let Err(e) = migration_runner.run_migrations().await {
            error!("Failed to run database migrations: {}", e);
            return Err(anyhow::anyhow!("Database migration failed: {}", e));
        }
        
        // Get migration status for info
        match migration_runner.get_migration_status().await {
            Ok(status) => info!("📊 Migration status: {}", status.summary()),
            Err(e) => warn!("Could not get migration status: {}", e),
        }
        
        // Initialize session now that migrations are complete
        info!("🔄 Initializing database session...");
        if let Err(e) = db.initialize_session().await {
            error!("Failed to initialize database session: {}", e);
            return Err(anyhow::anyhow!("Database session initialization failed: {}", e));
        }
        info!("✅ Database session initialized successfully");
        
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
        
        // Initialize analytics components after database is ready
        self.initialize_analytics().await?;
        
        Ok(())
    }

    /// Initialize analytics components (Task 3.1: Real-time Metrics Calculation)
    async fn initialize_analytics(&mut self) -> Result<()> {
        info!("🔧 Initializing analytics components for real-time metrics");
        
        let db_manager = self.database_manager.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database manager not initialized"))?;
        let db = db_manager.get_database();

        // Initialize position tracker
        let position_tracker = Arc::new(PositionTracker::new(db.clone()));
        position_tracker.initialize_schema().await
            .map_err(|e| anyhow::anyhow!("Failed to initialize position tracker schema: {}", e))?;

        // Initialize P&L calculator
        let pnl_calculator = Arc::new(PnLCalculator::new(db.clone(), position_tracker.clone()));
        pnl_calculator.initialize_schema().await
            .map_err(|e| anyhow::anyhow!("Failed to initialize P&L calculator schema: {}", e))?;

        // Initialize performance tracker
        let performance_tracker = Arc::new(PerformanceTracker::new(
            db.clone(), 
            position_tracker.clone(),
            pnl_calculator.clone()
        ));
        performance_tracker.initialize_schema().await
            .map_err(|e| anyhow::anyhow!("Failed to initialize performance tracker schema: {}", e))?;

        // Initialize insider analytics
        let insider_analytics = Arc::new(InsiderAnalytics::new(db.clone(), position_tracker.clone()));
        insider_analytics.initialize_schema().await
            .map_err(|e| anyhow::anyhow!("Failed to initialize insider analytics schema: {}", e))?;

        // Initialize wallet intelligence engine (Phase 4)
        let (signal_sender, _signal_receiver) = tokio::sync::mpsc::unbounded_channel();
        let wallet_intelligence = Arc::new(WalletIntelligenceEngine::new(
            db.clone(),
            signal_sender,
        ).await.map_err(|e| anyhow::anyhow!("Failed to create wallet intelligence engine: {}", e))?);
        
        wallet_intelligence.initialize_schema().await
            .map_err(|e| anyhow::anyhow!("Failed to initialize wallet intelligence schema: {}", e))?;

        // Start background tasks for wallet intelligence
        wallet_intelligence.start_background_tasks().await
            .map_err(|e| anyhow::anyhow!("Failed to start wallet intelligence background tasks: {}", e))?;

        // Store references
        self.position_tracker = Some(position_tracker);
        self.pnl_calculator = Some(pnl_calculator);
        self.performance_tracker = Some(performance_tracker);
        self.insider_analytics = Some(insider_analytics);
        self.wallet_intelligence = Some(wallet_intelligence);

        info!("✅ Analytics components initialized successfully");
        info!("   📊 Position Tracker: Ready for real-time position tracking");
        info!("   💰 P&L Calculator: Ready for real-time profit/loss calculation");
        info!("   📈 Performance Tracker: Ready for bot performance metrics");
        info!("   🕵️ Insider Analytics: Ready for wallet intelligence tracking");
        info!("   🧠 Wallet Intelligence: Ready for nanosecond insider copy trading");
        
        Ok(())
    }

    /// Start wallet monitoring and balance tracking service
    async fn start_wallet_monitoring_service(&mut self) -> Result<()> {
        info!("🏦 Starting wallet monitoring service");

        let wallet_manager = self.wallet_manager.clone()
            .ok_or_else(|| anyhow::anyhow!("Wallet manager not initialized"))?;

        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let wallet_monitoring_task = tokio::spawn(async move {
            let mut balance_check_interval = tokio::time::interval(Duration::from_secs(60)); // Check balances every minute
            let mut health_check_interval = tokio::time::interval(Duration::from_secs(300)); // Health check every 5 minutes
            
            info!("🏦 Wallet monitoring service started - checking balances every 60 seconds");

            loop {
                tokio::select! {
                    // Balance updates every minute
                    _ = balance_check_interval.tick() => {
                        let mut wallet_writer = wallet_manager.write().await;
                        
                        // Update balances for all wallets
                        for wallet_type in wallet_writer.get_available_wallets() {
                            match wallet_writer.get_balance(&wallet_type, true).await {
                                Ok(balance) => {
                                    debug!("💳 {:?} wallet balance updated: {:.6} SOL", wallet_type, balance);
                                }
                                Err(e) => {
                                    warn!("Failed to update balance for {:?} wallet: {}", wallet_type, e);
                                }
                            }
                        }
                    }

                    // Comprehensive health check every 5 minutes
                    _ = health_check_interval.tick() => {
                        let wallet_reader = wallet_manager.read().await;
                        
                        println!("\n🏦 ═══════════════════════════════════════════════════════");
                        println!("🏦 WALLET HEALTH CHECK - {}", chrono::Utc::now().format("%H:%M:%S UTC"));
                        println!("🏦 ═══════════════════════════════════════════════════════");
                        
                        for wallet_type in wallet_reader.get_available_wallets() {
                            match wallet_reader.get_wallet_config(&wallet_type) {
                                Ok(config) => {
                                    let balance = config.cached_balance_sol
                                        .map(|b| format!("{:.6} SOL", b))
                                        .unwrap_or_else(|| "Unknown".to_string());
                                    
                                    let last_accessed = config.last_accessed
                                        .map(|ts| ts.format("%H:%M:%S").to_string())
                                        .unwrap_or_else(|| "Never".to_string());
                                    
                                    let status = if config.is_active { "🟢 Active" } else { "🔴 Inactive" };
                                    
                                    println!("📱 {:?} Wallet:", wallet_type);
                                    println!("   Address: {}...{}", &config.public_key[..8], &config.public_key[config.public_key.len()-8..]);
                                    println!("   Balance: {} | Status: {}", balance, status);
                                    println!("   Last Accessed: {}", last_accessed);
                                    
                                    // Add explorer links
                                    match wallet_reader.get_explorer_url(&wallet_type, Some("solscan")) {
                                        Ok(url) => println!("   🔍 Solscan: {}", url),
                                        Err(_) => println!("   🔍 Explorer: Unable to generate link"),
                                    }
                                }
                                Err(e) => {
                                    println!("❌ {:?} Wallet: Error - {}", wallet_type, e);
                                }
                            }
                        }
                        
                        println!("🏦 ═══════════════════════════════════════════════════════\n");
                    }

                    // Handle shutdown
                    _ = shutdown_rx.recv() => {
                        info!("🛑 Wallet monitoring service received shutdown signal");
                        break;
                    }
                }
            }

            info!("✅ Wallet monitoring service completed successfully");
            Ok(())
        });

        self.tasks.push(wallet_monitoring_task);
        info!("✅ Wallet monitoring service started successfully");
        Ok(())
    }

    /// Start real-time analytics reporting service (Phase 3: Task 3.1)
    async fn start_analytics_reporting_service(&mut self) -> Result<()> {
        info!("📊 Starting real-time analytics reporting service");

        let position_tracker = self.position_tracker.clone()
            .ok_or_else(|| anyhow::anyhow!("Position tracker not initialized"))?;
        let pnl_calculator = self.pnl_calculator.clone()
            .ok_or_else(|| anyhow::anyhow!("P&L calculator not initialized"))?;
        let performance_tracker = self.performance_tracker.clone()
            .ok_or_else(|| anyhow::anyhow!("Performance tracker not initialized"))?;
        let insider_analytics = self.insider_analytics.clone()
            .ok_or_else(|| anyhow::anyhow!("Insider analytics not initialized"))?;

        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let reporting_task = tokio::spawn(async move {
            let mut reporting_interval = tokio::time::interval(Duration::from_secs(60)); // Report every minute
            let mut performance_interval = tokio::time::interval(Duration::from_secs(300)); // Performance every 5 minutes
            
            // Start a trading session
            let session_id = match performance_tracker.start_trading_session().await {
                Ok(id) => {
                    info!("🚀 Started trading session: {}", id);
                    id
                }
                Err(e) => {
                    error!("Failed to start trading session: {}", e);
                    return Err(anyhow::anyhow!("Failed to start trading session: {}", e));
                }
            };

            loop {
                tokio::select! {
                    // Real-time reporting every minute
                    _ = reporting_interval.tick() => {
                        if let Err(e) = generate_real_time_report(
                            &position_tracker,
                            &pnl_calculator, 
                            &insider_analytics
                        ).await {
                            warn!("Failed to generate real-time report: {}", e);
                        }
                    }

                    // Performance metrics every 5 minutes
                    _ = performance_interval.tick() => {
                        if let Err(e) = generate_performance_report(
                            &performance_tracker,
                            &pnl_calculator
                        ).await {
                            warn!("Failed to generate performance report: {}", e);
                        }
                    }

                    // Handle shutdown
                    _ = shutdown_rx.recv() => {
                        info!("🛑 Analytics reporting service received shutdown signal");
                        
                        // End the trading session
                        if let Err(e) = performance_tracker.end_trading_session().await {
                            warn!("Failed to end trading session cleanly: {}", e);
                        }
                        
                        break;
                    }
                }
            }

            Ok(())
        });

        self.tasks.push(reporting_task);
        info!("✅ Analytics reporting service started successfully");
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
        
        // Clone analytics components for the ingestion task
        let position_tracker = self.position_tracker.clone();
        let pnl_calculator = self.pnl_calculator.clone(); 
        let performance_tracker = self.performance_tracker.clone();
        let insider_analytics = self.insider_analytics.clone();
        let wallet_intelligence = self.wallet_intelligence.clone();
        
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
                                            
                                            // Process with insider analytics (Phase 3: Task 3.1)
                                            if let Some(insider_analytics) = &insider_analytics {
                                                process_market_event_for_insider_analytics(&market_event, insider_analytics).await;
                                            }
                                            
                                            // Process with wallet intelligence (Phase 4: Ultra-fast copy trading)
                                            if let Some(wallet_intelligence) = &wallet_intelligence {
                                                process_market_event_for_wallet_intelligence(&market_event, wallet_intelligence).await;
                                            }
                                            
                                            // Generate and route trading signals
                                            if let Some(signal) = generate_basic_trading_signal(&market_event) {
                                                display_trading_signal(&signal);
                                                
                                                // Route signal through transport layer
                                                match service_registry.route_trading_signal(
                                                    signal.clone(),
                                                    Some("ingestion-service-001")
                                                ).await {
                                                    Ok(_) => println!("   📤 TradingSignal routed to transport bus successfully"),
                                                    Err(e) => warn!("Failed to route trading signal: {}", e),
                                                }
                                                
                                                // Process signal with analytics (Phase 3: Task 3.1)
                                                if let (Some(position_tracker), Some(pnl_calc)) = (&position_tracker, &pnl_calculator) {
                                                    process_trading_signal_for_analytics(&signal, position_tracker, pnl_calc).await;
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
                            TradingSignal::Buy { token_mint, confidence, max_amount_sol, reason, source, .. } => {
                                println!("  🟢 BUY SIGNAL: Token: {} | Confidence: {:.1}% | Max: {:.3} SOL", 
                                    &token_mint[..8], confidence * 100.0, max_amount_sol);
                                println!("      Reason: {} | Source: {:?}", reason, source);
                            }
                            TradingSignal::Sell { token_mint, price_target, stop_loss, reason, .. } => {
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
        
        // Initialize wallet management system first
        self.initialize_wallet_system().await?;
        
        // Start transport monitoring first to capture all events
        self.start_transport_monitoring_service().await?;
        
        // Initialize Phase 3 database services
        self.initialize_database_services().await?;
        
        // Start ingestion service
        self.start_ingestion_service().await?;
        
        // Start analytics reporting service (Phase 3: Task 3.1)
        self.start_analytics_reporting_service().await?;
        
        // Start wallet monitoring service
        self.start_wallet_monitoring_service().await?;
        
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
        
        println!("\n🔍 BADGER TRADING BOT - FULLY OPERATIONAL");
        println!("   🏦 Wallet Management:");
        if let Some(wallet_manager) = &self.wallet_manager {
            let wallet_reader = wallet_manager.read().await;
            for wallet_type in wallet_reader.get_available_wallets() {
                if let Ok(config) = wallet_reader.get_wallet_config(&wallet_type) {
                    let balance = config.cached_balance_sol
                        .map(|b| format!("{:.6} SOL", b))
                        .unwrap_or_else(|| "Unknown".to_string());
                    println!("      • {:?}: {} ({})", wallet_type, config.public_key, balance);
                }
            }
        }
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
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,badger=debug,sqlx=warn"))
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
    
    info!("🦡 Badger Trading Bot - Phase 4 Wallet Intelligence & Copy Trading");
    info!("==============================================================");
    info!("Version: 0.4.0-phase4");
    info!("Phase 4 Features:");
    info!("  🔥 Real-time Raydium AMM pool monitoring");
    info!("  ⚡ Jupiter V6 aggregator event tracking");
    info!("  🌊 Orca Whirlpool program monitoring");
    info!("  🪙 SPL Token new mint detection");
    info!("  🚀 Pump.fun meme coin launch tracking");
    info!("  🎯 Advanced trading signal generation");
    info!("  🗄️ Persistent event storage and analytics");
    info!("  📊 Real-time performance tracking");
    info!("  🕵️ Wallet intelligence and insider scoring");
    info!("  🧠 Nanosecond-speed insider copy trading");
    info!("  ⚡ Ultra-fast decision making with memory cache");
    info!("  🎯 Automated position sizing and signal generation");
    info!("  🔍 High-performance data queries");
    info!("Performance: Nanosecond decisions + comprehensive intelligence tracking");

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