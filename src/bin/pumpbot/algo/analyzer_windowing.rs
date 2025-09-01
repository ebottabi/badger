/// Sliding window management for analyzer

use std::collections::HashMap;
use chrono::{DateTime, Utc, Duration as ChronoDuration};
use colored::Colorize;

use crate::client::event_parser::UniversalPumpEvent;
use crate::util::time_series::SlidingWindow;
use crate::algo::signal_processor::SignalProcessor;
use crate::algo::trend_analysis::{calculate_trend_analysis, TrendStrength};
use crate::algo::mathematical_engine::{MathematicalEngine, BuySignalStrength};

impl super::analyzer::PumpRealtimeAnalyzer {
    pub fn add_to_sliding_window(&mut self, event: &UniversalPumpEvent) {
        let mint = event.mint.clone();
        
        // Create window if doesn't exist
        if !self.token_windows.contains_key(&mint) {
            self.token_windows.insert(mint.clone(), SlidingWindow::new(5)); // 5 minute window
        }
        
        // Calculate data point before borrowing window mutably
        let data_point = self.signal_processor.create_time_series_point(event);
        
        let window = self.token_windows.get_mut(&mint).unwrap();
        window.add_point(data_point);
    }
    
    pub async fn analyze_sliding_windows(&mut self) {
        let now = Utc::now();
        
        // Only analyze tokens that have enough data points
        for (mint, window) in &self.token_windows {
            if window.has_sufficient_data() {
                // Traditional trend analysis
                if let Some(basic_analysis) = calculate_trend_analysis(window) {
                    if basic_analysis.trend_strength != TrendStrength::Neutral {
                        self.print_trend_analysis(mint, &basic_analysis, now);
                    }
                }
                
                // Advanced mathematical analysis
                if let Some(math_analysis) = self.mathematical_engine.analyze(window) {
                    self.print_mathematical_analysis(mint, &math_analysis, now);
                    
                    // Check for automated buy signals
                    match math_analysis.buy_signal_strength {
                        BuySignalStrength::StrongBuy => {
                            self.print_automated_buy_signal(mint, &math_analysis, "STRONG BUY");
                        }
                        BuySignalStrength::Buy => {
                            self.print_automated_buy_signal(mint, &math_analysis, "BUY");
                        }
                        BuySignalStrength::StrongSell => {
                            self.print_automated_sell_signal(mint, &math_analysis, "STRONG SELL");
                        }
                        BuySignalStrength::Sell => {
                            self.print_automated_sell_signal(mint, &math_analysis, "SELL");
                        }
                        BuySignalStrength::Hold => {
                            // Only print if score is high but other conditions not met
                            if math_analysis.composite_virality_score > 0.6 {
                                self.print_hold_signal(mint, &math_analysis);
                            }
                        }
                    }
                }
            }
        }
        
        // Clean up old token windows (>10 minutes old)
        self.token_windows.retain(|_, window| {
            window.age_minutes() < 10
        });
    }
    
    fn print_trend_analysis(&self, mint: &str, analysis: &crate::algo::trend_analysis::TrendAnalysis, timestamp: DateTime<Utc>) {
        let trend_icon = match analysis.trend_strength {
            TrendStrength::StrongBullish => "🚀",
            TrendStrength::Bullish => "📈", 
            TrendStrength::Neutral => "➡️",
            TrendStrength::Bearish => "📉",
            TrendStrength::StrongBearish => "💥",
        };
        
        println!("\n{} {} TREND ANALYSIS", trend_icon, analysis.trend_strength.to_string().bold());
        println!("{}", "-".repeat(60));
        println!("🪙 Token: {}", mint);
        println!("📊 Price Momentum: {:.1}%/min", analysis.price_momentum_percent_per_min);
        println!("⚖️  Buy/Sell Ratio: {:.1}", analysis.buy_sell_ratio);
        println!("👥 Unique Traders: {}", analysis.unique_traders);
        println!("⚡ Trade Frequency: {:.1}/min", analysis.trade_frequency_per_min);
        println!("⏰ Analysis Time: {}", timestamp.format("%H:%M:%S"));
        
        // Decision recommendations
        match analysis.trend_strength {
            TrendStrength::StrongBullish => {
                println!("🎯 Recommendation: {}", "STRONG BUY - High momentum detected!".bold());
            }
            TrendStrength::Bullish => {
                println!("✅ Recommendation: {}", "BUY - Positive trend building".bold());
            }
            TrendStrength::StrongBearish => {
                println!("🚨 Recommendation: {}", "STRONG SELL - Major decline!".bold());
            }
            TrendStrength::Bearish => {
                println!("⚠️  Recommendation: {}", "SELL - Negative momentum".bold());
            }
            TrendStrength::Neutral => {
                println!("⏸️  Recommendation: {}", "HOLD - Wait for clearer signals".bold());
            }
        }
        
        println!("{}", "-".repeat(60));
    }
}