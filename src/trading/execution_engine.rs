/// Trading Execution Engine
/// 
/// This module orchestrates the execution of copy trading signals,
/// including risk management, position sizing, and performance tracking.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn, error, debug};
use chrono::Utc;

use crate::core::TradingSignal;
use crate::intelligence::{WalletIntelligenceEngine, CopyTradeResult, TradeResult, ExitReason};
use crate::intelligence::SignalUrgency;
use super::jupiter_client::{JupiterClient, TradeExecutionResult};

/// Trading execution engine that processes signals from intelligence system
pub struct TradingExecutionEngine {
    /// Jupiter client for swap execution
    jupiter_client: Arc<JupiterClient>,
    /// Wallet intelligence engine for feedback
    intelligence_engine: Arc<WalletIntelligenceEngine>,
    /// Signal receiver channel
    signal_receiver: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<TradingSignal>>>,
    /// Execution configuration
    config: ExecutionConfig,
    /// Active positions tracking
    active_positions: Arc<tokio::sync::RwLock<std::collections::HashMap<String, ActivePosition>>>,
}

/// Configuration for trade execution
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// Maximum position size in SOL
    pub max_position_size_sol: f64,
    /// Minimum position size in SOL  
    pub min_position_size_sol: f64,
    /// Maximum number of concurrent positions
    pub max_concurrent_positions: u32,
    /// Stop loss percentage (e.g., 0.1 = 10%)
    pub stop_loss_percentage: f64,
    /// Take profit percentage (e.g., 0.5 = 50%)
    pub take_profit_percentage: f64,
    /// Maximum position hold time in seconds
    pub max_hold_time_seconds: i64,
    /// Enable dry run mode (no real trades)
    pub dry_run_mode: bool,
}

/// Active trading position
#[derive(Debug, Clone)]
pub struct ActivePosition {
    /// Copy signal ID that created this position
    pub copy_signal_id: i64,
    /// Insider wallet being copied
    pub insider_wallet: String,
    /// Token mint being traded
    pub token_mint: String,
    /// Entry price in SOL
    pub entry_price: f64,
    /// Position size in tokens
    pub position_size: u64,
    /// Entry timestamp
    pub entry_timestamp: i64,
    /// Transaction signature
    pub entry_signature: String,
    /// Target stop loss price
    pub stop_loss_price: Option<f64>,
    /// Target take profit price
    pub take_profit_price: Option<f64>,
}

/// Execution statistics
#[derive(Debug, Clone)]
pub struct ExecutionStats {
    /// Total signals processed
    pub total_signals: u64,
    /// Successful executions
    pub successful_executions: u64,
    /// Failed executions
    pub failed_executions: u64,
    /// Total volume traded in SOL
    pub total_volume_sol: f64,
    /// Average execution time in milliseconds
    pub avg_execution_time_ms: f64,
    /// Current active positions
    pub active_positions_count: u32,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            max_position_size_sol: 1.0,          // 1 SOL max per trade
            min_position_size_sol: 0.01,         // 0.01 SOL minimum
            max_concurrent_positions: 10,        // Max 10 positions
            stop_loss_percentage: 0.15,          // 15% stop loss
            take_profit_percentage: 0.5,         // 50% take profit
            max_hold_time_seconds: 3600,         // 1 hour max hold
            dry_run_mode: true,                  // Start in dry run for safety
        }
    }
}

impl TradingExecutionEngine {
    /// Create new trading execution engine
    pub fn new(
        jupiter_client: Arc<JupiterClient>,
        intelligence_engine: Arc<WalletIntelligenceEngine>,
        signal_receiver: mpsc::UnboundedReceiver<TradingSignal>,
        config: Option<ExecutionConfig>,
    ) -> Self {
        let config = config.unwrap_or_default();
        
        info!("🎯 Trading execution engine created with config:");
        info!("   • Max position size: {} SOL", config.max_position_size_sol);
        info!("   • Max concurrent positions: {}", config.max_concurrent_positions);
        info!("   • Stop loss: {}%", config.stop_loss_percentage * 100.0);
        info!("   • Take profit: {}%", config.take_profit_percentage * 100.0);
        info!("   • Dry run mode: {}", config.dry_run_mode);

        Self {
            jupiter_client,
            intelligence_engine,
            signal_receiver: Arc::new(tokio::sync::Mutex::new(signal_receiver)),
            config,
            active_positions: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Start processing trading signals
    pub async fn start_signal_processing(&self) -> Result<()> {
        info!("🚀 Starting trading signal processing");

        loop {
            // Get next trading signal
            let signal = {
                let mut receiver = self.signal_receiver.lock().await;
                match receiver.recv().await {
                    Some(signal) => signal,
                    None => {
                        warn!("Signal channel closed, stopping execution engine");
                        break;
                    }
                }
            };

            // Process the signal
            self.process_trading_signal(signal).await;
        }

        Ok(())
    }

    /// Process a single trading signal
    async fn process_trading_signal(&self, signal: TradingSignal) {
        debug!("📥 Processing trading signal: {:?}", signal);

        // Check if we can execute this signal
        if !self.can_execute_signal(&signal).await {
            return;
        }

        // Execute the trade
        match self.execute_signal(signal).await {
            Ok(result) => {
                info!("✅ Signal execution completed: success={}", result.success);
                if let Some(ref signature) = result.signature {
                    info!("   📜 Transaction: {}", signature);
                }
            }
            Err(e) => {
                error!("❌ Signal execution failed: {}", e);
            }
        }
    }

    /// Check if we can execute a trading signal
    async fn can_execute_signal(&self, signal: &TradingSignal) -> bool {
        // Check concurrent position limits
        let positions = self.active_positions.read().await;
        if positions.len() >= self.config.max_concurrent_positions as usize {
            debug!("⏸️ Max concurrent positions reached ({}/{})", 
                   positions.len(), self.config.max_concurrent_positions);
            return false;
        }

        // Check position size limits
        match signal {
            TradingSignal::Buy { max_amount_sol, .. } => {
                if *max_amount_sol > self.config.max_position_size_sol {
                    debug!("⏸️ Position size too large: {} SOL (max: {} SOL)", 
                           max_amount_sol, self.config.max_position_size_sol);
                    return false;
                }
                if *max_amount_sol < self.config.min_position_size_sol {
                    debug!("⏸️ Position size too small: {} SOL (min: {} SOL)", 
                           max_amount_sol, self.config.min_position_size_sol);
                    return false;
                }
            }
            TradingSignal::Sell { .. } => {
                // For sell signals, check if we have an active position
                // This will be implemented when we have position tracking
            }
            _ => {
                debug!("⏸️ Unsupported signal type for execution");
                return false;
            }
        }

        // Check if market conditions are suitable
        match signal {
            TradingSignal::Buy { token_mint, max_amount_sol, .. } => {
                let sol_lamports = (max_amount_sol * 1_000_000_000.0) as u64;
                match self.jupiter_client.is_safe_to_trade(
                    "So11111111111111111111111111111111111112", // SOL
                    token_mint,
                    sol_lamports,
                ).await {
                    Ok(safe) => {
                        if !safe {
                            debug!("⏸️ Market conditions not safe for trading");
                            return false;
                        }
                    }
                    Err(e) => {
                        debug!("⏸️ Could not check market safety: {}", e);
                        return false;
                    }
                }
            }
            _ => {}
        }

        true
    }

    /// Execute a trading signal
    async fn execute_signal(&self, signal: TradingSignal) -> Result<TradeExecutionResult> {
        if self.config.dry_run_mode {
            info!("🔄 DRY RUN: Would execute signal: {:?}", signal);
            
            // Simulate successful execution
            return Ok(TradeExecutionResult {
                success: true,
                signature: Some("DRY_RUN_SIMULATION".to_string()),
                error: None,
                input_amount: Some(1_000_000), // 0.001 SOL
                output_amount: Some(1000000),  // 1M tokens
                price_impact: Some(0.5),       // 0.5% impact
                gas_fee_lamports: Some(10000), // 0.00001 SOL
                execution_time_ms: 100,        // 100ms
            });
        }

        // Execute real trade through Jupiter
        let result = self.jupiter_client.execute_trade(&signal).await?;

        // If successful, track the position
        if result.success {
            self.track_new_position(&signal, &result).await?;
        }

        // Provide feedback to intelligence engine
        self.provide_execution_feedback(&signal, &result).await?;

        Ok(result)
    }

    /// Track a new position after successful execution
    async fn track_new_position(
        &self,
        signal: &TradingSignal,
        result: &TradeExecutionResult,
    ) -> Result<()> {
        if let TradingSignal::Buy { token_mint, max_amount_sol, metadata, .. } = signal {
            let copy_signal_id = metadata.as_ref()
                .and_then(|m| m.split(',').find(|s| s.starts_with("copy_signal_id:")))
                .and_then(|s| s.split(':').nth(1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            
            let insider_wallet = metadata.as_ref()
                .and_then(|m| m.split(',').find(|s| s.starts_with("insider_wallet:")))
                .and_then(|s| s.split(':').nth(1))
                .unwrap_or("unknown").to_string();
                
            let position = ActivePosition {
                copy_signal_id,
                insider_wallet,
                token_mint: token_mint.clone(),
                entry_price: *max_amount_sol,
                position_size: result.output_amount.unwrap_or(0),
                entry_timestamp: Utc::now().timestamp(),
                entry_signature: result.signature.clone().unwrap_or_default(),
                stop_loss_price: Some(max_amount_sol * (1.0 - self.config.stop_loss_percentage)),
                take_profit_price: Some(max_amount_sol * (1.0 + self.config.take_profit_percentage)),
            };

            let mut positions = self.active_positions.write().await;
            positions.insert(token_mint.clone(), position);

            info!("📊 New position tracked: {} tokens of {}", 
                  result.output_amount.unwrap_or(0), token_mint);
        }

        Ok(())
    }

    /// Provide execution feedback to intelligence engine
    async fn provide_execution_feedback(
        &self,
        signal: &TradingSignal,
        result: &TradeExecutionResult,
    ) -> Result<()> {
        // Create copy trade result for intelligence engine
        let copy_result = CopyTradeResult {
            insider_wallet: match signal {
                TradingSignal::Buy { metadata, .. } => {
                    // Extract insider wallet from metadata
                    metadata.as_ref()
                        .and_then(|m| m.split(',').find(|s| s.starts_with("insider_wallet:")))
                        .and_then(|s| s.split(':').nth(1))
                        .unwrap_or("unknown").to_string()
                }
                TradingSignal::Sell { metadata, .. } => {
                    // Extract insider wallet from metadata
                    metadata.as_ref()
                        .and_then(|m| m.split(',').find(|s| s.starts_with("insider_wallet:")))
                        .and_then(|s| s.split(':').nth(1))
                        .unwrap_or("unknown").to_string()
                }
                _ => "unknown".to_string(),
            },
            token_mint: match signal {
                TradingSignal::Buy { token_mint, .. } => token_mint.clone(),
                TradingSignal::Sell { token_mint, .. } => token_mint.clone(),
                _ => "unknown".to_string(),
            },
            our_entry_price: result.input_amount.map(|a| a as f64 / 1_000_000_000.0),
            our_exit_price: None, // Will be set when position is closed
            profit_loss_sol: None, // Will be calculated when closed
            profit_percentage: None,
            hold_duration_seconds: None,
            result: if result.success { TradeResult::Pending } else { TradeResult::Loss },
            exit_reason: ExitReason::Manual, // Default for now
        };

        // Update intelligence engine with execution result  
        if let Some(copy_signal_id) = match signal {
            TradingSignal::Buy { metadata, .. } => {
                // Extract copy signal ID from metadata
                metadata.as_ref()
                    .and_then(|m| m.split(',').find(|s| s.starts_with("copy_signal_id:")))
                    .and_then(|s| s.split(':').nth(1))
                    .and_then(|s| s.parse().ok())
            }
            TradingSignal::Sell { metadata, .. } => {
                // Extract copy signal ID from metadata
                metadata.as_ref()
                    .and_then(|m| m.split(',').find(|s| s.starts_with("copy_signal_id:")))
                    .and_then(|s| s.split(':').nth(1))
                    .and_then(|s| s.parse().ok())
            }
            _ => None,
        } {
            self.intelligence_engine
                .update_copy_performance(copy_signal_id, copy_result)
                .await?;
        }

        Ok(())
    }

    /// Monitor active positions for stop loss/take profit
    pub async fn monitor_positions(&self) -> Result<()> {
        info!("👁️ Starting position monitoring");

        loop {
            {
                let positions = self.active_positions.read().await;
                for (token_mint, position) in positions.iter() {
                    self.check_position_exit_conditions(token_mint, position).await;
                }
            }

            // Check every 30 seconds
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        }
    }

    /// Check if a position should be closed
    async fn check_position_exit_conditions(&self, token_mint: &str, position: &ActivePosition) {
        // Check maximum hold time
        let current_time = Utc::now().timestamp();
        if current_time - position.entry_timestamp > self.config.max_hold_time_seconds {
            info!("⏰ Position {} exceeded max hold time, closing", token_mint);
            self.close_position(token_mint, ExitReason::TimeDecay).await;
            return;
        }

        // Get current market price
        let current_price = match self.jupiter_client.get_market_price(
            token_mint,
            "So11111111111111111111111111111111111112", // SOL
        ).await {
            Ok(price) => price * position.position_size as f64,
            Err(e) => {
                debug!("Could not get market price for {}: {}", token_mint, e);
                return;
            }
        };

        // Check stop loss
        if let Some(stop_loss) = position.stop_loss_price {
            if current_price <= stop_loss {
                info!("📉 Stop loss triggered for {}: {} <= {}", 
                      token_mint, current_price, stop_loss);
                self.close_position(token_mint, ExitReason::StopLoss).await;
                return;
            }
        }

        // Check take profit
        if let Some(take_profit) = position.take_profit_price {
            if current_price >= take_profit {
                info!("📈 Take profit triggered for {}: {} >= {}", 
                      token_mint, current_price, take_profit);
                self.close_position(token_mint, ExitReason::TakeProfit).await;
                return;
            }
        }
    }

    /// Close an active position
    async fn close_position(&self, token_mint: &str, exit_reason: ExitReason) {
        // Remove from active positions
        let position = {
            let mut positions = self.active_positions.write().await;
            positions.remove(token_mint)
        };

        if let Some(position) = position {
            info!("🚪 Closing position: {} (reason: {:?})", token_mint, exit_reason);

            // Create sell signal with correct structure
            let sell_signal = TradingSignal::Sell {
                token_mint: token_mint.to_string(),
                price_target: 0.0, // Market sell
                stop_loss: 0.0,    // Market sell
                reason: format!("Position exit: {:?}", exit_reason),
                amount_tokens: Some(position.position_size as f64),
                min_price: Some(0.0), // Market sell
                source: Some(crate::core::SignalSource::InsiderCopy),
                metadata: Some(format!("insider_wallet:{},copy_signal_id:{}", 
                                       position.insider_wallet, position.copy_signal_id)),
            };

            // Execute the sell
            match self.execute_signal(sell_signal).await {
                Ok(result) => {
                    let signature = result.signature.clone().unwrap_or_else(|| "DRY_RUN".to_string());
                    info!("✅ Position closed successfully: {}", signature);
                    
                    // Calculate final P&L and update intelligence engine
                    self.finalize_position_pnl(&position, &result, exit_reason).await;
                }
                Err(e) => {
                    error!("❌ Failed to close position {}: {}", token_mint, e);
                }
            }
        }
    }

    /// Finalize position P&L calculation and update intelligence
    async fn finalize_position_pnl(
        &self,
        position: &ActivePosition,
        exit_result: &TradeExecutionResult,
        exit_reason: ExitReason,
    ) {
        let exit_amount_sol = exit_result.output_amount
            .map(|a| a as f64 / 1_000_000_000.0) // Convert lamports to SOL
            .unwrap_or(0.0);

        let profit_loss = exit_amount_sol - position.entry_price;
        let profit_percentage = if position.entry_price > 0.0 {
            profit_loss / position.entry_price
        } else {
            0.0
        };

        let final_result = CopyTradeResult {
            insider_wallet: position.insider_wallet.clone(),
            token_mint: position.token_mint.clone(),
            our_entry_price: Some(position.entry_price),
            our_exit_price: Some(exit_amount_sol),
            profit_loss_sol: Some(profit_loss),
            profit_percentage: Some(profit_percentage),
            hold_duration_seconds: Some(Utc::now().timestamp() - position.entry_timestamp),
            result: if profit_loss > 0.0 { TradeResult::Win } else { TradeResult::Loss },
            exit_reason,
        };

        // Update intelligence engine
        if let Err(e) = self.intelligence_engine
            .update_copy_performance(position.copy_signal_id, final_result)
            .await
        {
            error!("Failed to update position P&L: {}", e);
        }

        info!("📊 Position finalized: {} SOL P&L ({:.2}%)", 
              profit_loss, profit_percentage * 100.0);
    }

    /// Get current execution statistics
    pub async fn get_execution_stats(&self) -> ExecutionStats {
        let positions = self.active_positions.read().await;
        
        ExecutionStats {
            total_signals: 0,           // Would track in real implementation
            successful_executions: 0,   // Would track in real implementation  
            failed_executions: 0,       // Would track in real implementation
            total_volume_sol: 0.0,      // Would track in real implementation
            avg_execution_time_ms: 0.0, // Would track in real implementation
            active_positions_count: positions.len() as u32,
        }
    }

    /// Enable or disable dry run mode
    pub async fn set_dry_run_mode(&mut self, enabled: bool) {
        self.config.dry_run_mode = enabled;
        info!("🔄 Dry run mode: {}", if enabled { "ENABLED" } else { "DISABLED" });
    }
}