#!/bin/bash

# System-level optimizations for Badger

set -e

echo "🚀 Applying system optimizations for Badger..."

# CPU optimizations
echo "Applying CPU optimizations..."
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    # Linux optimizations
    echo "Detected Linux - applying optimizations..."
    
    # Set CPU governor to performance
    sudo cpupower frequency-set -g performance 2>/dev/null || echo "⚠️  Could not set CPU governor (requires cpupower)"
    
    # Increase network buffers
    sudo sysctl -w net.core.rmem_max=536870912 2>/dev/null || echo "⚠️  Could not set network buffer size"
    sudo sysctl -w net.core.wmem_max=536870912 2>/dev/null || echo "⚠️  Could not set network buffer size"
    
    # Optimize TCP settings
    sudo sysctl -w net.ipv4.tcp_congestion_control=bbr 2>/dev/null || echo "⚠️  Could not set TCP congestion control"
    
elif [[ "$OSTYPE" == "darwin"* ]]; then
    # macOS optimizations
    echo "Detected macOS - applying optimizations..."
    
    # Increase network buffers
    sudo sysctl -w kern.ipc.maxsockbuf=16777216 2>/dev/null || echo "⚠️  Could not set socket buffer size"
    sudo sysctl -w net.inet.tcp.sendspace=1048576 2>/dev/null || echo "⚠️  Could not set TCP send buffer"
    sudo sysctl -w net.inet.tcp.recvspace=1048576 2>/dev/null || echo "⚠️  Could not set TCP receive buffer"
fi

# Set high priority for current user processes
echo "Setting process priorities..."
ulimit -n 65536 2>/dev/null || echo "⚠️  Could not increase file descriptor limit"

# Rust-specific optimizations
echo "Applying Rust optimizations..."
export RUSTFLAGS="-C target-cpu=native -C opt-level=3"

# Build with optimizations
echo "Building with maximum optimizations..."
cargo build --release

echo "✅ System optimizations applied!"
echo "🏃 Your system is now optimized for high-frequency trading"
echo "⚡ CPU set to performance mode (Linux only)"
echo "📡 Network buffers increased for lower latency"
echo "🦀 Rust compiled with native CPU optimizations"