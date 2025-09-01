/// Transport Layer Handler
/// 
/// Manages the transport bus, service registry, and inter-component communication.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, debug};

use crate::core::TradingSignal;
use crate::transport::{EnhancedTransportBus, ServiceRegistry};

pub struct TransportHandler {
    transport_bus: Arc<EnhancedTransportBus>,
    service_registry: Arc<ServiceRegistry>,
    signal_sender: mpsc::UnboundedSender<TradingSignal>,
}

impl TransportHandler {
    /// Initialize transport layer
    pub async fn init() -> Result<Self> {
        info!("🚌 Initializing Transport Layer");
        
        // Create signal channel for trading signals
        let (signal_sender, _signal_receiver) = mpsc::unbounded_channel();
        
        // Initialize transport bus
        let transport_bus = Arc::new(EnhancedTransportBus::new());
        
        // Initialize service registry
        let service_registry = Arc::new(ServiceRegistry::new(Arc::clone(&transport_bus)));
        
        info!("✅ Transport Layer initialized");
        
        Ok(Self {
            transport_bus,
            service_registry,
            signal_sender,
        })
    }
    
    /// Start transport monitoring
    pub async fn start(&mut self) -> Result<()> {
        debug!("🔄 Starting transport monitoring");
        // Transport layer is passive, no background tasks needed
        Ok(())
    }
    
    /// Get signal sender for other components
    pub fn get_signal_sender(&self) -> mpsc::UnboundedSender<TradingSignal> {
        self.signal_sender.clone()
    }
    
    /// Get transport bus reference
    pub fn get_transport_bus(&self) -> Arc<EnhancedTransportBus> {
        Arc::clone(&self.transport_bus)
    }
    
    /// Get service registry reference
    pub fn get_service_registry(&self) -> Arc<ServiceRegistry> {
        Arc::clone(&self.service_registry)
    }
    
    /// Get number of registered services
    pub async fn get_service_count(&self) -> usize {
        // Simplified implementation - would query actual registry
        5 // Placeholder
    }
    
    /// Shutdown transport layer
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("🛑 Shutting down transport layer");
        // Cleanup any active connections or monitoring tasks
        Ok(())
    }
}