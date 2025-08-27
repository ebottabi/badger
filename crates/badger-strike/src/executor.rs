use anyhow::Result;
use badger_core::types::{Signal, Token};
use badger_transport::signal_bus::SignalBus;
use tracing::{info, debug, warn, error, instrument};

#[derive(Debug)]
pub struct TradeExecutor {
    signal_bus: SignalBus,
}

impl TradeExecutor {
    #[instrument]
    pub async fn new() -> Result<Self> {
        info!("Initializing TradeExecutor");
        Ok(Self {
            signal_bus: SignalBus::new(),
        })
    }

    #[instrument(skip(self))]
    pub async fn run(&self) -> Result<()> {
        info!("TradeExecutor: Listening for buy/sell signals");
        
        let mut signal_receiver = self.signal_bus.subscribe();
        
        while let Ok(signal) = signal_receiver.recv().await {
            if let Err(e) = self.execute_signal(&signal).await {
                error!(
                    signal_type = ?signal.signal_type,
                    token_symbol = %signal.token.symbol,
                    amount_sol = signal.amount_sol,
                    error = %e,
                    "Failed to execute trading signal"
                );
            }
        }
        
        warn!("TradeExecutor signal receiver channel closed");
        Ok(())
    }

    #[instrument(skip(self))]
    async fn execute_signal(&self, signal: &Signal) -> Result<()> {
        debug!(
            signal_type = ?signal.signal_type,
            token_symbol = %signal.token.symbol,
            amount_sol = signal.amount_sol,
            timestamp = signal.timestamp,
            "Processing trading signal"
        );

        match signal.signal_type {
            badger_core::types::SignalType::Buy => {
                self.execute_buy(&signal.token, signal.amount_sol).await?;
            }
            badger_core::types::SignalType::Sell => {
                self.execute_sell(&signal.token, signal.amount_sol).await?;
            }
            badger_core::types::SignalType::Alert => {
                info!(
                    token_symbol = %signal.token.symbol,
                    "Received alert signal"
                );
            }
        }
        Ok(())
    }

    #[instrument(skip(self))]
    async fn execute_buy(&self, token: &Token, amount_sol: f64) -> Result<()> {
        info!(
            token_symbol = %token.symbol,
            token_mint = %token.mint,
            amount_sol = amount_sol,
            liquidity_sol = token.liquidity_sol,
            "⚡ Executing BUY order"
        );
        // TODO: Execute actual swap transaction
        Ok(())
    }

    #[instrument(skip(self))]
    async fn execute_sell(&self, token: &Token, amount_sol: f64) -> Result<()> {
        info!(
            token_symbol = %token.symbol,
            token_mint = %token.mint,
            amount_sol = amount_sol,
            liquidity_sol = token.liquidity_sol,
            "⚡ Executing SELL order"
        );
        // TODO: Execute actual swap transaction
        Ok(())
    }
}