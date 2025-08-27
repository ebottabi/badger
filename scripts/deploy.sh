#!/bin/bash

# Badger deployment script

set -e

echo "🦡 Deploying Badger trading bot..."

# Build the main binary in release mode
echo "Building Badger main controller in release mode..."
cargo build --release

# Create runtime directories
echo "Creating runtime directories..."
mkdir -p logs
mkdir -p data

# Copy configuration files if they don't exist
echo "Setting up configuration..."
if [ ! -f "config/badger.toml" ]; then
    echo "⚠️  Please configure config/badger.toml before deployment"
    exit 1
fi

if [ ! -f "config/wallets.json" ]; then
    echo "⚠️  Please configure config/wallets.json before deployment"
    exit 1
fi

# Start the main controller which manages all services
echo "Starting Badger main controller (manages all services)..."

nohup ./target/release/badger > logs/badger.log 2>&1 &
MAIN_PID=$!
echo $MAIN_PID > logs/badger.pid

echo "✅ Badger deployed successfully!"
echo "🎯 Main controller PID: $MAIN_PID"
echo "📊 All services are managed by the main controller"
echo "📋 Check logs in logs/badger.log"
echo "🛑 Run 'kill $MAIN_PID' or 'pkill -f badger' to stop all services"
echo ""
echo "To monitor:"
echo "  tail -f logs/badger.log"