use anyhow::Result;
use tokio::signal;
use tokio::task::JoinHandle;
use tokio::sync::broadcast;
use tracing::{info, warn, error, debug, instrument};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// Import service modules
use badger_ingest::stream::StreamProcessor;
use badger_stalker::monitor::WalletMonitor;
use badger_scout::scanner::TokenScanner;
use badger_strike::executor::TradeExecutor;

struct ServiceOrchestrator {
    shutdown_tx: broadcast::Sender<()>,
    tasks: Vec<JoinHandle<Result<()>>>,
}

impl ServiceOrchestrator {
    fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);
        Self {
            shutdown_tx,
            tasks: Vec::new(),
        }
    }

    #[instrument(skip(self))]
    async fn start_all_services(&mut self) -> Result<()> {
        info!("🚀 Starting all Badger services programmatically");

        // Start Ingest Service
        let mut ingest_shutdown = self.shutdown_tx.subscribe();
        let ingest_task = tokio::spawn(async move {
            info!("🔄 Badger Ingest - Data Ingestion Service starting");
            info!("Monitoring Solana blockchain for DEX transactions");
            
            match StreamProcessor::new().await {
                Ok(stream_processor) => {
                    tokio::select! {
                        result = stream_processor.run() => {
                            match &result {
                                Ok(()) => info!("Badger Ingest completed successfully"),
                                Err(e) => error!("Badger Ingest error: {}", e)
                            }
                            result
                        }
                        _ = ingest_shutdown.recv() => {
                            info!("🛑 Badger Ingest shutting down gracefully");
                            Ok(())
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to initialize Badger Ingest: {}", e);
                    Err(e)
                }
            }
        });
        self.tasks.push(ingest_task);

        // Start Stalker Service
        let mut stalker_shutdown = self.shutdown_tx.subscribe();
        let stalker_task = tokio::spawn(async move {
            info!("👁️  Badger Stalker - Wallet Tracking Service starting");
            info!("Monitoring insider wallets for trading activity");
            
            match WalletMonitor::new().await {
                Ok(wallet_monitor) => {
                    tokio::select! {
                        result = wallet_monitor.run() => {
                            match &result {
                                Ok(()) => info!("Badger Stalker completed successfully"),
                                Err(e) => error!("Badger Stalker error: {}", e)
                            }
                            result
                        }
                        _ = stalker_shutdown.recv() => {
                            info!("🛑 Badger Stalker shutting down gracefully");
                            Ok(())
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to initialize Badger Stalker: {}", e);
                    Err(e)
                }
            }
        });
        self.tasks.push(stalker_task);

        // Start Scout Service
        let mut scout_shutdown = self.shutdown_tx.subscribe();
        let scout_task = tokio::spawn(async move {
            info!("🔍 Badger Scout - Token Discovery Service starting");
            info!("Scanning for new token launches and opportunities");
            
            match TokenScanner::new().await {
                Ok(token_scanner) => {
                    tokio::select! {
                        result = token_scanner.run() => {
                            match &result {
                                Ok(()) => info!("Badger Scout completed successfully"),
                                Err(e) => error!("Badger Scout error: {}", e)
                            }
                            result
                        }
                        _ = scout_shutdown.recv() => {
                            info!("🛑 Badger Scout shutting down gracefully");
                            Ok(())
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to initialize Badger Scout: {}", e);
                    Err(e)
                }
            }
        });
        self.tasks.push(scout_task);

        // Start Strike Service
        let mut strike_shutdown = self.shutdown_tx.subscribe();
        let strike_task = tokio::spawn(async move {
            info!("⚡ Badger Strike - Trade Execution Service starting");
            info!("Ready to execute buy/sell orders with precision");
            
            match TradeExecutor::new().await {
                Ok(trade_executor) => {
                    tokio::select! {
                        result = trade_executor.run() => {
                            match &result {
                                Ok(()) => info!("Badger Strike completed successfully"),
                                Err(e) => error!("Badger Strike error: {}", e)
                            }
                            result
                        }
                        _ = strike_shutdown.recv() => {
                            info!("🛑 Badger Strike shutting down gracefully");
                            Ok(())
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to initialize Badger Strike: {}", e);
                    Err(e)
                }
            }
        });
        self.tasks.push(strike_task);

        info!("✅ All {} services started successfully", self.tasks.len());
        Ok(())
    }

    #[instrument(skip(self))]
    async fn shutdown_all(&mut self) -> Result<()> {
        info!("🛑 Shutting down all services");
        
        // Send shutdown signal to all services
        let _ = self.shutdown_tx.send(());
        debug!("Shutdown signal sent to all services");
        
        // Wait for all tasks to complete
        let mut results = Vec::new();
        for task in self.tasks.drain(..) {
            results.push(task.await);
        }
        
        // Check for any errors
        for (i, result) in results.into_iter().enumerate() {
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

fn init_tracing() -> Result<()> {
    // Create logs directory if it doesn't exist
    std::fs::create_dir_all("logs")?;
    
    // Create file appender for logs
    let file_appender = tracing_appender::rolling::daily("logs", "badger.log");
    let (non_blocking_file, _guard) = tracing_appender::non_blocking(file_appender);
    
    // Create console layer with formatting
    let console_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_level(true)
        .compact();
    
    // Create file layer with JSON formatting
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_file)
        .json()
        .with_current_span(false)
        .with_span_list(true);
    
    // Initialize subscriber with both console and file layers
    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();
    
    // Leak the guard to prevent the file appender from being dropped
    std::mem::forget(_guard);
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    init_tracing()?;
    
    info!("🦡 Badger Trading Bot - Main Controller");
    info!("======================================");

    let mut orchestrator = ServiceOrchestrator::new();
    
    // Start all services
    match orchestrator.start_all_services().await {
        Ok(()) => {
            info!("🎯 Badger is now actively monitoring and trading");
            info!("📊 All services running in coordinated tasks");
            info!("Press Ctrl+C to shutdown all services");
        }
        Err(e) => {
            error!("Failed to start services: {}", e);
            return Err(e);
        }
    }

    // Wait for shutdown signal
    match signal::ctrl_c().await {
        Ok(()) => {
            info!("🛑 Shutdown signal received");
        }
        Err(e) => {
            error!("Failed to listen for shutdown signal: {}", e);
        }
    }
    
    orchestrator.shutdown_all().await?;
    
    info!("👋 Badger shutdown complete");
    Ok(())
}