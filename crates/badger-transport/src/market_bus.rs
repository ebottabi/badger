use anyhow::Result;
use badger_core::types::Token;
use tokio::sync::broadcast;
use tracing::{info, debug, warn, instrument};

#[derive(Debug)]
pub struct MarketBus {
    tx: broadcast::Sender<Token>,
}

impl MarketBus {
    #[instrument]
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(10000);
        debug!("MarketBus initialized with capacity: 10000");
        Self { tx }
    }

    #[instrument(skip(self, token))]
    pub fn publish(&self, token: Token) -> Result<()> {
        match self.tx.send(token.clone()) {
            Ok(subscriber_count) => {
                debug!(
                    token_symbol = %token.symbol,
                    token_mint = %token.mint,
                    subscriber_count = subscriber_count,
                    "Published token to market bus"
                );
                Ok(())
            }
            Err(e) => {
                warn!(
                    token_symbol = %token.symbol,
                    error = %e,
                    "Failed to publish token to market bus"
                );
                Err(e.into())
            }
        }
    }

    #[instrument(skip(self))]
    pub fn subscribe(&self) -> broadcast::Receiver<Token> {
        let receiver = self.tx.subscribe();
        debug!("New subscriber added to market bus");
        receiver
    }
}