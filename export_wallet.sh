#!/bin/bash

# Export Badger Wallet to Phantom-compatible formats
echo "🔑 Exporting Badger Wallets for Phantom Import"
echo "=============================================="

# Check if wallet files exist
if [ ! -d "wallets" ]; then
    echo "❌ No wallets directory found. Run 'cargo run' first to create wallets."
    exit 1
fi

if [ ! -f "wallets/master.key" ]; then
    echo "❌ Master key not found. Run 'cargo run' first to initialize wallets."
    exit 1
fi

echo "📖 Found wallet directory, extracting private keys..."

# Use the simple export tool that doesn't need complex dependencies
cargo run --bin simple_export --quiet 2>/dev/null || {
    echo "🔄 Compiling simple export tool..."
    
    # Compile the standalone extractor
    rustc simple_export.rs \
        --extern serde_json \
        --extern base64 \
        --extern bs58 \
        -L target/debug/deps \
        -o simple_extract
    
    if [ $? -eq 0 ]; then
        echo "✅ Compiled successfully, extracting keys..."
        ./simple_extract
        rm -f simple_extract
    else
        echo "❌ Failed to compile. Make sure you have run 'cargo build' first."
        echo "💡 Try: cargo build && ./export_wallet.sh"
        exit 1
    fi
}

echo ""
echo "✅ Export complete!"
echo ""
echo "🦄 PHANTOM IMPORT STEPS:"
echo "1. Copy the Base58 private key from above"
echo "2. Open Phantom Wallet app"  
echo "3. Settings → Add/Import Wallet → Import Private Key"
echo "4. Paste the Base58 key"
echo "5. Name your wallet and you're done!"
echo ""
echo "📱 ALTERNATIVE WALLETS:"
echo "• Solflare: Also supports Base58 import"
echo "• Backpack: Paste same Base58 key"
echo "• Any Solana wallet supporting private key import"
echo ""
echo "🔒 SECURITY REMINDER:"
echo "• Clear terminal history: history -c"
echo "• Never share private keys with anyone"  
echo "• Private keys = complete wallet control"