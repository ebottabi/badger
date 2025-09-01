# Trading Orchestrator Usage Guide

The Trading Orchestrator is the core integration layer that coordinates all trading system components into a cohesive, automated trading platform.

## Overview

The `TradingOrchestrator` integrates:
- **Scout** (Token Scanner): Discovers new token opportunities
- **Stalker** (Wallet Monitor): Monitors insider wallet activities  
- **Strike** (Trade Executor): Executes trades on DEX
- **Memory-Mapped Database**: Ultra-fast wallet confidence lookups
- **Transport Bus**: Event routing and communication
- **Wallet Management**: Profit taking and fund management
- **Portfolio Tracking**: Real-time position monitoring

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                 Trading Orchestrator                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │    Scout     │  │   Stalker    │  │    Strike    │       │
│  │(Token Scan)  │  │(Wallet Mon)  │  │(Trade Exec)  │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│          │                  │                  │            │
│          └──────────────────┼──────────────────┘            │
│                             │                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │  Memory DB   │  │Transport Bus │  │Wallet Mgmt   │       │
│  │(Ultra-fast)  │  │(Event Route) │  │(Fund Mgmt)   │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│                    Core Trading Flows                       │
│  • Ingestion Flow    • Copy Trading    • Wallet Management │ 
│  • Portfolio Track   • Statistics      • Health Monitoring │
└─────────────────────────────────────────────────────────────┘
```

## Basic Usage

### 1. Initialize with Default Configuration

```rust
use badger::trading::{TradingOrchestrator, OrchestratorConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize with defaults
    let orchestrator = TradingOrchestrator::new(None).await?;
    
    // Start the orchestrator
    orchestrator.run().await?;
    
    Ok(())
}
```

### 2. Custom Configuration

```rust
use badger::trading::{TradingOrchestrator, OrchestratorConfig};
use badger::intelligence::mmap_db::MmapConfig;

let config = OrchestratorConfig {
    mmap_config: MmapConfig {
        file_path: "data/production_wallets.mmap".to_string(),
        capacity: 1048576, // 1M wallets
        ..Default::default()
    },
    min_copy_confidence: 0.85,       // High confidence trades only
    min_copy_amount_sol: 0.05,       // Minimum 0.05 SOL copy trades
    max_copy_amount_sol: 2.0,        // Maximum 2 SOL copy trades
    profit_take_threshold: 30.0,     // Take profits at 30%
    stop_loss_threshold: -15.0,      // Stop losses at -15%
    cold_wallet_threshold: 10.0,     // Transfer profits > 10 SOL
    enable_auto_trading: true,       // Enable automated trading
    enable_profit_taking: true,      // Enable profit taking
    enable_stop_loss: true,          // Enable stop losses
    max_positions: 25,               // Hold max 25 positions
    ..Default::default()
};

let orchestrator = TradingOrchestrator::new(Some(config)).await?;
```

## Trading Flows

### 1. Ingestion Flow

Coordinates data ingestion across all services:

```rust
// Automatically started by orchestrator.run()
// - Token scanner discovers new mints
// - Wallet monitor tracks insider activities
// - Events published to transport bus
// - Statistics updated in real-time
```

### 2. Copy Trading Flow

Monitors insider wallets and replicates profitable trades:

```rust
// Triggered by insider wallet activities
// 1. Wallet activity detected by Stalker
// 2. Confidence lookup in memory-mapped DB (1-5ns)
// 3. Signal generated if confidence >= threshold
// 4. Trade executed through Strike module
// 5. Position tracked in portfolio

// Example flow:
// Insider buys BONK -> Confidence 0.87 -> Copy trade 0.1 SOL -> Position opened
```

### 3. Wallet Management Flow

Manages trading funds and risk:

```rust
// Continuously running background flow
// - Monitors portfolio for profit/loss thresholds
// - Executes profit taking at configured levels  
// - Implements stop losses to limit downside
// - Transfers excess profits to cold storage
// - Maintains position size limits
```

### 4. Portfolio Tracking Flow

Real-time position and performance monitoring:

```rust
// Updates every rebalance_interval_secs
// - Fetches current token prices
// - Calculates unrealized P&L for all positions
// - Updates portfolio statistics
// - Triggers rebalancing if needed
// - Maintains position count within limits
```

## Monitoring and Statistics

### Real-time Statistics

```rust
let stats = orchestrator.get_statistics().await;

println!("Uptime: {}s", stats.uptime_seconds);
println!("Opportunities: {}", stats.opportunities_scanned);
println!("Copy Trades: {}", stats.copy_trades_executed); 
println!("Active Positions: {}", stats.active_positions);
println!("Portfolio Value: {:.4} SOL", stats.total_portfolio_value_sol);
println!("DB Hit Rate: {:.2}%", stats.mmap_hit_rate * 100.0);
```

### Portfolio Information

```rust
let portfolio = orchestrator.get_portfolio().await;

println!("Total Value: {:.4} SOL", portfolio.total_value_sol);
println!("Realized P&L: {:.4} SOL", portfolio.realized_pnl_sol);
println!("Unrealized P&L: {:.4} SOL", portfolio.unrealized_pnl_sol);
println!("Total Trades: {}", portfolio.total_trades);
println!("Win Rate: {:.1}%", 
         (portfolio.winning_trades as f64 / portfolio.total_trades as f64) * 100.0);

// Active positions
for position in &portfolio.positions {
    println!("{}: {:.2}% P&L", position.token_symbol, position.unrealized_pnl_percent);
}
```

## Configuration Reference

### OrchestratorConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `mmap_config` | `MmapConfig` | Default | Memory-mapped DB configuration |
| `min_copy_confidence` | `f32` | `0.75` | Minimum confidence for copy trades |
| `min_copy_amount_sol` | `f64` | `0.01` | Minimum SOL amount to copy |
| `max_copy_amount_sol` | `f64` | `1.0` | Maximum SOL amount to copy |
| `profit_take_threshold` | `f64` | `50.0` | Profit taking threshold (%) |
| `stop_loss_threshold` | `f64` | `-20.0` | Stop loss threshold (%) |
| `cold_wallet_threshold` | `f64` | `5.0` | Cold storage transfer threshold |
| `enable_auto_trading` | `bool` | `true` | Enable automatic trading |
| `enable_profit_taking` | `bool` | `true` | Enable profit taking |
| `enable_stop_loss` | `bool` | `true` | Enable stop losses |
| `max_positions` | `usize` | `20` | Maximum concurrent positions |

### MmapConfig (Memory Database)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `file_path` | `String` | `"data/wallets.mmap"` | Database file path |
| `capacity` | `usize` | `1048576` | Max wallet entries (power of 2) |
| `max_probe_distance` | `usize` | `8` | Hash collision resolution |
| `enable_checksums` | `bool` | `true` | Data integrity checks |

## Performance Characteristics

### Memory-Mapped Database
- **Lookup Speed**: 1-5 nanoseconds per wallet confidence check
- **Capacity**: Up to 16M+ wallets (configurable)
- **Memory Usage**: ~100MB per 1M wallets
- **Persistence**: Automatic file-backed storage

### Trading Performance
- **Signal Processing**: Sub-millisecond insider signal detection
- **Order Execution**: Direct DEX integration via Jupiter
- **Portfolio Updates**: Real-time position tracking
- **Risk Management**: Automatic profit/loss management

### Event Processing
- **Transport Bus**: High-throughput event routing
- **Module Integration**: Seamless cross-module communication
- **Statistics**: Real-time performance metrics
- **Health Monitoring**: Automatic system health tracking

## Production Deployment

### 1. System Requirements

```bash
# Minimum requirements
CPU: 4 cores
RAM: 8GB
Disk: 50GB SSD
Network: Stable internet connection

# Recommended for high-frequency trading
CPU: 8+ cores  
RAM: 16GB+
Disk: 100GB+ NVMe SSD
Network: Low-latency connection (<50ms to Solana RPC)
```

### 2. Configuration

```rust
// Production configuration
let config = OrchestratorConfig {
    mmap_config: MmapConfig {
        file_path: "/data/production/wallets.mmap".to_string(),
        capacity: 4194304, // 4M wallets
        enable_checksums: true,
        backup_on_close: true,
        ..Default::default()
    },
    min_copy_confidence: 0.90,       // Very high confidence only
    max_copy_amount_sol: 5.0,        // Larger position sizes
    profit_take_threshold: 40.0,     // Conservative profit taking
    stop_loss_threshold: -10.0,      // Tight stop losses
    cold_wallet_threshold: 50.0,     // Large cold storage threshold
    max_positions: 50,               // More concurrent positions
    rebalance_interval_secs: 60,     // Frequent rebalancing
    ..Default::default()
};
```

### 3. Monitoring Setup

```rust
// Background monitoring task
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(300));
    
    loop {
        interval.tick().await;
        
        let stats = orchestrator.get_statistics().await;
        
        // Log key metrics
        tracing::info!(
            "Portfolio: {:.4} SOL | Positions: {} | Copy Trades: {} | Hit Rate: {:.2}%",
            stats.total_portfolio_value_sol,
            stats.active_positions,
            stats.copy_trades_executed,
            stats.mmap_hit_rate * 100.0
        );
        
        // Alert on issues
        if stats.mmap_hit_rate < 0.95 {
            tracing::warn!("Low memory DB hit rate: {:.2}%", stats.mmap_hit_rate * 100.0);
        }
    }
});
```

## Error Handling

The orchestrator implements comprehensive error handling:

```rust
// Graceful shutdown on errors
match orchestrator.run().await {
    Ok(_) => println!("Orchestrator completed successfully"),
    Err(e) => {
        eprintln!("Orchestrator failed: {}", e);
        // Implement recovery logic
    }
}

// Manual shutdown
orchestrator.shutdown().await;
```

## Examples

See the complete examples in:
- `examples/orchestrator_demo.rs` - Full demonstration
- `tests/integration_orchestrator.rs` - Integration tests

Run the demo:
```bash
cargo run --example orchestrator_demo
```

Run integration tests:
```bash
cargo test integration_orchestrator
```