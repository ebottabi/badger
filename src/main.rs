/// Main Entry Point for Badger Trading System
/// 
/// Clean handler-based architecture with init() methods as requested

use anyhow::Result;
use tokio::signal;
use tokio::time::{Duration, interval};
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use badger::handlers::SystemOrchestrator;

/// Main entry point
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_file(false)
                .with_line_number(false)
        )
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "badger=info,warn".into())
        )
        .init();
    
    info!("🦡 Starting Badger - High-Performance Solana Copy Trading Bot");
    
    // Initialize the complete system using handlers
    let mut orchestrator = SystemOrchestrator::init().await?;
    
    // Start all services
    orchestrator.start().await?;
    
    // Display system status
    let status = orchestrator.get_status().await;
    info!("📋 System Status:\n{}", status);
    
    // Start periodic status reporting
    let mut status_interval = interval(Duration::from_secs(300)); // Every 5 minutes
    
    // Run until shutdown signal
    tokio::select! {
        // Periodic status updates
        _ = async {
            loop {
                status_interval.tick().await;
                let status = orchestrator.get_status().await;
                info!("📊 Status Update:\n{}", status);
            }
        } => {},
        
        // Graceful shutdown on CTRL+C
        _ = signal::ctrl_c() => {
            info!("🛑 Shutdown signal received");
        }
    }
    
    // Shutdown all services
    orchestrator.shutdown().await?;
    
    info!("👋 Badger shutdown complete - All wallets loaded from folder successfully");
    Ok(())
}