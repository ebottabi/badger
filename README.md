# 🦡 Badger - Solana Trading Bot

A high-performance Solana trading bot built in Rust for insider wallet tracking and automated token sniping.

## Architecture

Badger is built with a modular, single-binary architecture:

- **🔄 Badger Ingest**: Real-time Solana blockchain data ingestion
- **👁️ Badger Stalker**: Insider wallet tracking and pattern detection  
- **🔍 Badger Scout**: New token discovery and opportunity scanning
- **⚡ Badger Strike**: Lightning-fast trade execution and sniping
- **🚌 Transport Layer**: Ultra-fast inter-service communication

## Features

- **Single Binary**: All services run as coordinated async tasks
- **Structured Logging**: Comprehensive tracing with JSON and console output
- **Graceful Shutdown**: Coordinated service shutdown on Ctrl+C
- **High Performance**: CPU-native optimizations and async architecture
- **Configurable**: TOML-based configuration system

## Quick Start

### Build & Run

```bash
# Build optimized binary
cargo build --release

# Run with default settings
./target/release/badger

# Run with custom log level
RUST_LOG=debug ./target/release/badger
```

### Configuration

Edit configuration files in `config/`:

- `badger.toml` - Main application settings
- `wallets.json` - Insider wallets to track
- `triggers.toml` - Buy/sell trigger rules
- `logging.toml` - Logging configuration

### Logging

Badger provides comprehensive logging:

- **Console**: Formatted output with colors and structure
- **Files**: JSON logs in `logs/badger.log.YYYY-MM-DD`
- **Levels**: Configurable per-module log levels

```bash
# View live logs
tail -f logs/badger.log.$(date +%Y-%m-%d)

# Set custom log levels
export RUST_LOG="badger=debug,badger_transport=trace"
```

## Environment Variables

- `RUST_LOG`: Override log levels (e.g., `debug`, `badger=trace`)
- `BADGER_CONFIG`: Custom config directory (default: `config/`)

## Deployment

Use the provided deployment script:

```bash
# Deploy with system optimizations
./scripts/optimize.sh

# Deploy services
./scripts/deploy.sh
```

## Development

### Project Structure

```
badger/
├── src/main.rs              # Main orchestrator
├── crates/
│   ├── badger-core/          # Shared types & constants
│   ├── badger-transport/     # Inter-service communication
│   ├── badger-ingest/        # Blockchain data ingestion
│   ├── badger-stalker/       # Wallet tracking
│   ├── badger-scout/         # Token discovery
│   └── badger-strike/        # Trade execution
├── config/                   # Configuration files
└── logs/                     # Log files (auto-created)
```

### Building

```bash
# Check code
cargo check

# Run tests
cargo test

# Build release
cargo build --release

# Build with optimizations
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

## Monitoring

Monitor the bot's performance:

```bash
# Main process logs
tail -f logs/badger.log.$(date +%Y-%m-%d)

# Filter for specific service
tail -f logs/badger.log.$(date +%Y-%m-%d) | grep "badger_strike"

# Monitor JSON logs with jq
tail -f logs/badger.log.$(date +%Y-%m-%d) | jq -r 'select(.level == "ERROR")'
```

## Safety & Disclaimers

⚠️ **This is trading software - use at your own risk**

- Test thoroughly on devnet before mainnet use
- Start with small amounts
- Monitor positions actively
- Understand the risks of automated trading

## License

[Add your license here]

---

**🦡 Built for speed, precision, and profit**