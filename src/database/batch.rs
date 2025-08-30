use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::interval;
use tracing::{debug, info, warn, error, instrument};

use crate::core::{MarketEvent, TradingSignal};
use super::BadgerDatabase;
use super::DatabaseError;

/// High-performance batch processor for database operations
pub struct BatchProcessor<T> {
    batch: Arc<Mutex<VecDeque<T>>>,
    batch_size: usize,
    batch_timeout: Duration,
    last_flush: Arc<RwLock<Instant>>,
    pending_count: Arc<AtomicUsize>,
    flush_trigger: broadcast::Sender<()>,
    _flush_receiver: broadcast::Receiver<()>,
}

impl<T> BatchProcessor<T> 
where 
    T: Clone + Send + Sync + 'static,
{
    pub fn new(batch_size: usize, batch_timeout: Duration) -> Self {
        let (flush_trigger, flush_receiver) = broadcast::channel(100);
        
        Self {
            batch: Arc::new(Mutex::new(VecDeque::new())),
            batch_size,
            batch_timeout,
            last_flush: Arc::new(RwLock::new(Instant::now())),
            pending_count: Arc::new(AtomicUsize::new(0)),
            flush_trigger,
            _flush_receiver: flush_receiver,
        }
    }

    /// Add event to batch with backpressure handling
    pub async fn add(&self, event: T) -> Result<(), DatabaseError> {
        let current_count = self.pending_count.load(Ordering::Relaxed);
        
        // Backpressure: reject if queue is too full
        if current_count > self.batch_size * 10 {
            return Err(DatabaseError::InitializationError(
                "Batch queue overflow - backpressure activated".to_string()
            ));
        }

        {
            let mut batch = self.batch.lock().await;
            batch.push_back(event);
            self.pending_count.store(batch.len(), Ordering::Relaxed);
        }

        // Trigger flush if batch size reached
        if current_count >= self.batch_size {
            let _ = self.flush_trigger.send(());
        }

        Ok(())
    }

    /// Get current batch size
    pub async fn len(&self) -> usize {
        self.batch.lock().await.len()
    }

    /// Check if batch is empty
    pub async fn is_empty(&self) -> bool {
        self.batch.lock().await.is_empty()
    }

    /// Drain all events from batch for processing
    pub async fn drain(&self) -> Vec<T> {
        let mut batch = self.batch.lock().await;
        let events: Vec<T> = batch.drain(..).collect();
        self.pending_count.store(0, Ordering::Relaxed);
        
        // Update last flush time
        {
            let mut last_flush = self.last_flush.write().await;
            *last_flush = Instant::now();
        }
        
        events
    }

    /// Check if batch should be flushed due to timeout
    pub async fn should_flush_timeout(&self) -> bool {
        let last_flush = *self.last_flush.read().await;
        let elapsed = last_flush.elapsed();
        elapsed >= self.batch_timeout && !self.is_empty().await
    }

    /// Force flush trigger
    pub async fn force_flush(&self) {
        let _ = self.flush_trigger.send(());
    }
}

/// Enhanced batch-based persistence service
pub struct EnhancedPersistenceService {
    db: Arc<BadgerDatabase>,
    market_event_batcher: BatchProcessor<MarketEvent>,
    trading_signal_batcher: BatchProcessor<TradingSignal>,
    events_processed: Arc<AtomicUsize>,
    signals_processed: Arc<AtomicUsize>,
}

impl EnhancedPersistenceService {
    pub fn new(db: Arc<BadgerDatabase>) -> Self {
        Self {
            db,
            market_event_batcher: BatchProcessor::new(500, Duration::from_secs(5)),
            trading_signal_batcher: BatchProcessor::new(100, Duration::from_secs(3)),
            events_processed: Arc::new(AtomicUsize::new(0)),
            signals_processed: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Add market event to batch
    pub async fn store_market_event(&self, event: MarketEvent) -> Result<(), DatabaseError> {
        self.market_event_batcher.add(event).await?;
        debug!("📦 Market event added to batch");
        Ok(())
    }

    /// Add trading signal to batch
    pub async fn store_trading_signal(&self, signal: TradingSignal) -> Result<(), DatabaseError> {
        self.trading_signal_batcher.add(signal).await?;
        debug!("📦 Trading signal added to batch");
        Ok(())
    }

    /// Start the batch processing service
    #[instrument(skip(self))]
    pub async fn run(self) -> Result<(), DatabaseError> {
        info!("🚀 Enhanced Persistence Service starting with batching");

        let db_clone = self.db.clone();
        let market_batcher = self.market_event_batcher;
        let signal_batcher = self.trading_signal_batcher;
        let events_processed = self.events_processed.clone();
        let signals_processed = self.signals_processed.clone();

        // Market events batch processor
        let market_processor = {
            let db = db_clone.clone();
            let batcher = market_batcher;
            let counter = events_processed.clone();
            
            tokio::spawn(async move {
                let mut flush_receiver = batcher.flush_trigger.subscribe();
                let mut timer = interval(Duration::from_millis(1000)); // Check every second
                
                info!("📦 Market events batch processor started");
                
                loop {
                    tokio::select! {
                        // Flush trigger received
                        _ = flush_receiver.recv() => {
                            if let Err(e) = Self::flush_market_events(&db, &batcher, &counter).await {
                                error!("Failed to flush market events batch: {}", e);
                            }
                        }
                        
                        // Periodic timeout check
                        _ = timer.tick() => {
                            if batcher.should_flush_timeout().await {
                                if let Err(e) = Self::flush_market_events(&db, &batcher, &counter).await {
                                    error!("Failed to flush market events batch (timeout): {}", e);
                                }
                            }
                        }
                    }
                }
            })
        };

        // Trading signals batch processor
        let signal_processor = {
            let db = db_clone.clone();
            let batcher = signal_batcher;
            let counter = signals_processed.clone();
            
            tokio::spawn(async move {
                let mut flush_receiver = batcher.flush_trigger.subscribe();
                let mut timer = interval(Duration::from_millis(500)); // Check more frequently
                
                info!("📦 Trading signals batch processor started");
                
                loop {
                    tokio::select! {
                        // Flush trigger received
                        _ = flush_receiver.recv() => {
                            if let Err(e) = Self::flush_trading_signals(&db, &batcher, &counter).await {
                                error!("Failed to flush trading signals batch: {}", e);
                            }
                        }
                        
                        // Periodic timeout check
                        _ = timer.tick() => {
                            if batcher.should_flush_timeout().await {
                                if let Err(e) = Self::flush_trading_signals(&db, &batcher, &counter).await {
                                    error!("Failed to flush trading signals batch (timeout): {}", e);
                                }
                            }
                        }
                    }
                }
            })
        };

        // Statistics reporter
        let stats_reporter = {
            let events_processed = events_processed.clone();
            let signals_processed = signals_processed.clone();
            
            tokio::spawn(async move {
                let mut timer = interval(Duration::from_secs(30));
                
                loop {
                    timer.tick().await;
                    let events = events_processed.load(Ordering::Relaxed);
                    let signals = signals_processed.load(Ordering::Relaxed);
                    
                    info!("📊 BATCH PROCESSING STATS:");
                    info!("   📦 Market Events Processed: {}", events);
                    info!("   📶 Trading Signals Processed: {}", signals);
                    info!("   ⚡ Total Throughput: {} events", events + signals);
                }
            })
        };

        // Wait for processors to complete (they run indefinitely)
        tokio::select! {
            result = market_processor => {
                error!("Market events processor exited: {:?}", result);
            }
            result = signal_processor => {
                error!("Trading signals processor exited: {:?}", result);
            }
            result = stats_reporter => {
                error!("Stats reporter exited: {:?}", result);
            }
        }

        Ok(())
    }

    /// Flush market events batch with transaction
    async fn flush_market_events(
        db: &BadgerDatabase, 
        batcher: &BatchProcessor<MarketEvent>,
        counter: &AtomicUsize
    ) -> Result<(), DatabaseError> {
        let events = batcher.drain().await;
        if events.is_empty() {
            return Ok(());
        }

        let batch_size = events.len();
        debug!("🔄 Flushing {} market events", batch_size);

        // Start transaction for batch insert
        let mut tx = db.begin_transaction().await?;

        // Batch insert all events in single transaction
        for event in &events {
            let event_data = serde_json::to_string(event)
                .map_err(|e| DatabaseError::SerializationError(format!("Failed to serialize event: {}", e)))?;

            sqlx::query(r#"
                INSERT INTO market_events (event_id, event_type, timestamp, slot, data, processed_at)
                VALUES (?, ?, ?, ?, ?, strftime('%s', 'now'))
            "#)
            .bind(&event.get_event_id())
            .bind(event.get_event_type())
            .bind(event.get_timestamp())
            .bind(event.get_slot().unwrap_or(0))
            .bind(event_data)
            .execute(&mut *tx)
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to insert market event: {}", e)))?;
        }

        // Commit transaction
        tx.commit().await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to commit market events: {}", e)))?;

        counter.fetch_add(batch_size, Ordering::Relaxed);
        info!("✅ Batch inserted {} market events", batch_size);
        
        Ok(())
    }

    /// Flush trading signals batch with transaction
    async fn flush_trading_signals(
        db: &BadgerDatabase, 
        batcher: &BatchProcessor<TradingSignal>,
        counter: &AtomicUsize
    ) -> Result<(), DatabaseError> {
        let signals = batcher.drain().await;
        if signals.is_empty() {
            return Ok(());
        }

        let batch_size = signals.len();
        debug!("🔄 Flushing {} trading signals", batch_size);

        // Start transaction for batch insert
        let mut tx = db.begin_transaction().await?;

        // Batch insert all signals in single transaction
        for signal in &signals {
            let signal_data = serde_json::to_string(signal)
                .map_err(|e| DatabaseError::SerializationError(format!("Failed to serialize signal: {}", e)))?;

            sqlx::query(r#"
                INSERT INTO trading_signals (signal_id, signal_type, timestamp, confidence, data, processed_at)
                VALUES (?, ?, ?, ?, ?, strftime('%s', 'now'))
            "#)
            .bind(&signal.get_signal_id())
            .bind(&signal.get_signal_type())
            .bind(signal.get_timestamp())
            .bind(signal.get_confidence())
            .bind(signal_data)
            .execute(&mut *tx)
            .await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to insert trading signal: {}", e)))?;
        }

        // Commit transaction
        tx.commit().await
            .map_err(|e| DatabaseError::QueryError(format!("Failed to commit trading signals: {}", e)))?;

        counter.fetch_add(batch_size, Ordering::Relaxed);
        info!("✅ Batch inserted {} trading signals", batch_size);
        
        Ok(())
    }
}