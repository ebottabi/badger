use anyhow::Result;
use crate::core::types::Signal;
use tokio::sync::broadcast;
use tracing::{info, debug, warn, instrument};

#[derive(Debug, Clone)]
pub struct SignalBus {
    tx: broadcast::Sender<Signal>,
}

impl SignalBus {
    #[instrument]
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1000);
        debug!("SignalBus initialized with capacity: 1000");
        Self { tx }
    }

    #[instrument(skip(self, signal))]
    pub fn publish(&self, signal: Signal) -> Result<()> {
        match self.tx.send(signal.clone()) {
            Ok(subscriber_count) => {
                info!(
                    signal_type = ?signal.signal_type,
                    token_symbol = %signal.token.symbol,
                    amount_sol = signal.amount_sol,
                    subscriber_count = subscriber_count,
                    "Published signal to signal bus"
                );
                Ok(())
            }
            Err(e) => {
                warn!(
                    signal_type = ?signal.signal_type,
                    token_symbol = %signal.token.symbol,
                    error = %e,
                    "Failed to publish signal to signal bus"
                );
                Err(e.into())
            }
        }
    }

    #[instrument(skip(self))]
    pub fn subscribe(&self) -> broadcast::Receiver<Signal> {
        let receiver = self.tx.subscribe();
        debug!("New subscriber added to signal bus");
        receiver
    }
}