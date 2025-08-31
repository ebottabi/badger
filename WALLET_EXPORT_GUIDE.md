# 📱 Badger Wallet Export Guide

## 🔑 Master Key Purpose

The **master key** in Badger is an encryption password that:
- **Encrypts your private keys** before storing them on disk
- **Protects wallet security** if someone accesses your computer files
- **Enables automatic wallet loading** when Badger restarts
- **Persists across sessions** - same key decrypts same wallets

### File Structure:
```
wallets/
├── master.key              # Your encryption password
├── wallet.salt             # Encryption salt (32 bytes)
├── trading_wallet.json     # Encrypted trading wallet
└── cold_wallet.json        # Encrypted cold storage wallet
```

---

## 📤 Export to Phantom Wallet

### Step 1: Create Wallets
```bash
# Run Badger to create wallets first
cargo run
# Let it initialize, then Ctrl+C to stop
```

### Step 2: Export Private Keys
```bash
# Use the export script
./export_wallet.sh
```

**Example Output:**
```
🔍 Processing trading_wallet.json:
✅ Wallet Type: Trading
📍 Public Key: 7xKXtAB...XYZ123

🔐 PRIVATE KEY (Base58 - for Phantom):
   5K8pN9m2vR...L6XzA8N5mP

🔗 View on Solscan: https://solscan.io/account/7xKXtAB...XYZ123

📱 TO IMPORT INTO PHANTOM:
   1. Open Phantom Wallet app
   2. Settings → Import Wallet → Private Key
   3. Paste this key: 5K8pN9m2vR...L6XzA8N5mP
   4. Your wallet will appear with your SOL balance!
```

### Step 3: Import to Phantom
1. **Open Phantom** wallet app or browser extension
2. **Settings** → **Add/Import Wallet** → **Import Private Key**
3. **Paste the Base58 key** from the export output
4. **Name your wallet** (e.g., "Badger Trading")
5. **Done!** Your wallet appears with current SOL balance

---

## 🦄 Supported Wallets

The Base58 private key format works with:

| Wallet | Import Method |
|--------|---------------|
| **Phantom** | Settings → Import Wallet → Private Key |
| **Solflare** | Import → Private Key |
| **Backpack** | Settings → Import Wallet |
| **Sollet** | Import Account → Private Key |
| **Exodus** | Settings → Private Keys |

---

## 🔒 Security Best Practices

### ✅ Safe Practices:
- **Only import into trusted wallets** (Phantom, Solflare, etc.)
- **Never share private keys** with anyone online
- **Clear terminal history** after viewing keys: `history -c`
- **Keep master key safe** - needed to decrypt wallets

### ❌ Never Do:
- **Don't paste private keys** into websites or DMs
- **Don't screenshot keys** - they can be recovered
- **Don't store keys** in plain text files
- **Don't share master key** - it decrypts all wallets

---

## 🛠️ Troubleshooting

### "No wallets directory found"
```bash
# Run Badger first to create wallets
cargo run
# Let it initialize, then try export again
```

### "Master key not found"
```bash
# Make sure Badger completed initialization
ls wallets/
# Should show: master.key, wallet.salt, *.json files
```

### "Failed to compile export tool"
```bash
# Build dependencies first
cargo build
./export_wallet.sh
```

### Export tool errors:
```bash
# Try manual compilation
cargo build --bin simple_export
cargo run --bin simple_export
```

---

## 🔍 Understanding Your Wallet

### Public Key (Address):
- **Safe to share** - this is where people send you SOL/tokens
- **View on explorers** - check balance and transaction history
- **Like your bank account number** - receive only

### Private Key:
- **NEVER share** - complete control of funds
- **64 bytes / Base58 encoded** - used to sign transactions  
- **Like your bank PIN** - spend/control funds

### Master Key:
- **Encryption password** - protects private keys on disk
- **Badger-specific** - not needed in Phantom
- **Keep safe** - needed to re-export if you lose private key

---

## 📊 Example Workflow

```bash
# 1. Create Badger wallets
cargo run              # Initialize wallets
# Ctrl+C after "Wallet Management System initialized"

# 2. Export for Phantom
./export_wallet.sh     # Get private keys

# 3. Import to Phantom
# Use Base58 key in Phantom app

# 4. Fund your wallet
# Send SOL to the public address

# 5. Start trading
cargo run              # Run Badger with funded wallet
```

**Your Badger wallet becomes a Phantom wallet - same address, same funds, different interface!** 🔄💰