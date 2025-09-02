/// Time series data structures and calculations

use chrono::{DateTime, Utc, Duration as ChronoDuration};
use std::collections::VecDeque;

#[derive(Clone)]
pub struct SlidingWindow {
    pub events: VecDeque<TimeSeriesPoint>,
    pub window_duration: ChronoDuration,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct TimeSeriesPoint {
    pub timestamp: DateTime<Utc>,
    pub price_sol: f64,
    pub volume_sol: f64,
    pub market_cap_sol: f64,
    pub tx_type: String,
    pub trader: String,
    // Enhanced fields for mathematical analysis
    pub bonding_curve_progress: f64,        // % of bonding curve filled (0-100)
    pub v_sol_in_bonding_curve: f64,        // SOL in bonding curve
    pub v_tokens_in_bonding_curve: f64,     // Tokens in bonding curve
    pub holder_count: Option<u64>,          // Number of holders (if available)
    pub initial_buy: Option<f64>,           // Initial buy amount for create events
}

impl SlidingWindow {
    pub fn new(window_duration_minutes: i64) -> Self {
        Self {
            events: VecDeque::new(),
            window_duration: ChronoDuration::minutes(window_duration_minutes),
            created_at: Utc::now(),
        }
    }
    
    pub fn add_point(&mut self, point: TimeSeriesPoint) {
        self.events.push_back(point);
        self.cleanup_old_events();
    }
    
    pub fn cleanup_old_events(&mut self) {
        let cutoff_time = Utc::now() - self.window_duration;
        while let Some(front) = self.events.front() {
            if front.timestamp < cutoff_time {
                self.events.pop_front();
            } else {
                break;
            }
        }
    }
    
    pub fn has_sufficient_data(&self) -> bool {
        self.events.len() >= 3
    }
    
    pub fn age_minutes(&self) -> i64 {
        let now = Utc::now();
        now.signed_duration_since(self.created_at).num_minutes()
    }
}