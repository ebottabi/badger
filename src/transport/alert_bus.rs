use anyhow::Result;
use crate::core::types::{Token, Wallet};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{info, debug, warn, instrument};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub alert_type: AlertType,
    pub token: Option<Token>,
    pub wallet: Option<Wallet>,
    pub message: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertType {
    WalletActivity,
    TokenAlert,
    SystemAlert,
}

#[derive(Debug, Clone)]
pub struct AlertBus {
    tx: broadcast::Sender<Alert>,
}

impl AlertBus {
    #[instrument]
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1000);
        debug!("AlertBus initialized with capacity: 1000");
        Self { tx }
    }

    #[instrument(skip(self, alert))]
    pub fn publish(&self, alert: Alert) -> Result<()> {
        match self.tx.send(alert.clone()) {
            Ok(subscriber_count) => {
                match alert.alert_type {
                    AlertType::SystemAlert => warn!(
                        alert_type = ?alert.alert_type,
                        message = %alert.message,
                        timestamp = alert.timestamp,
                        subscriber_count = subscriber_count,
                        "Published system alert"
                    ),
                    _ => info!(
                        alert_type = ?alert.alert_type,
                        message = %alert.message,
                        timestamp = alert.timestamp,
                        subscriber_count = subscriber_count,
                        "Published alert"
                    )
                }
                Ok(())
            }
            Err(e) => {
                warn!(
                    alert_type = ?alert.alert_type,
                    message = %alert.message,
                    error = %e,
                    "Failed to publish alert"
                );
                Err(e.into())
            }
        }
    }

    #[instrument(skip(self))]
    pub fn subscribe(&self) -> broadcast::Receiver<Alert> {
        let receiver = self.tx.subscribe();
        debug!("New subscriber added to alert bus");
        receiver
    }
}