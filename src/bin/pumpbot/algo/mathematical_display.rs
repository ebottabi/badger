/// Display methods for mathematical analysis results

use chrono::{DateTime, Utc};
use colored::Colorize;
use crate::algo::mathematical_engine::{MathematicalAnalysis, BuySignalStrength};

impl super::analyzer::PumpRealtimeAnalyzer {
    pub fn print_mathematical_analysis(&self, mint: &str, analysis: &MathematicalAnalysis, timestamp: DateTime<Utc>) {
        let score_color = if analysis.composite_virality_score > 0.8 {
            "bright_green"
        } else if analysis.composite_virality_score > 0.6 {
            "yellow"
        } else {
            "red"
        };
        
        println!("\n{} {} MATHEMATICAL ANALYSIS", "🧮", "ADVANCED".bold());
        println!("{}", "=".repeat(80));
        println!("🪙 Token: {}", mint);
        println!("⏰ Analysis Time: {}", timestamp.format("%H:%M:%S"));
        println!();
        
        // Core velocities
        println!("{}", "📊 VELOCITY METRICS:".bold());
        println!("   📈 Progress Velocity: {:.2}%/min (bonding curve momentum)", analysis.progress_velocity);
        println!("   💧 Volume Velocity: {:.4} SOL/sec (surge detection)", analysis.volume_velocity);
        println!("   🚀 Price Velocity: {:.6}/sec (exponential growth)", analysis.price_velocity);
        println!();
        
        // Risk and prediction
        println!("{}", "🔍 RISK & PREDICTION:".bold());
        println!("   🛡️  Rug Risk Score: {:.3} (1.0=safe, 0.0=risky)", analysis.holder_distribution_score);
        println!("   🔮 Growth Score: {:.3} (5-min projection)", analysis.predictive_growth_score);
        println!();
        
        // Composite score with color
        let score_str = format!("{:.3}", analysis.composite_virality_score);
        let colored_score = match score_color {
            "bright_green" => score_str.bright_green(),
            "yellow" => score_str.yellow(),
            _ => score_str.red(),
        };
        println!("{} {}", "🎯 VIRALITY SCORE:".bold(), colored_score.bold());
        
        // Interpretation guide
        println!("   Scale: 0.0-0.3=📉Low, 0.3-0.6=📊Medium, 0.6-0.8=📈High, 0.8-1.0=🚀Viral");
        
        println!("{}", "=".repeat(80));
    }
    
    pub fn print_automated_buy_signal(&self, mint: &str, analysis: &MathematicalAnalysis, signal_type: &str) {
        let icon = match signal_type {
            "STRONG BUY" => "🚨💎",
            "BUY" => "✅💰",
            _ => "📊",
        };
        
        println!("\n{} {} AUTOMATED {} SIGNAL", icon, "🤖", signal_type.bold());
        println!("{}", "=".repeat(80));
        println!("🪙 Token: {}", mint);
        
        // Signal validation details
        println!("\n{}", "🔬 SIGNAL VALIDATION:".bold());
        println!("   🎯 Virality Score: {:.3} (threshold: 0.7+)", analysis.composite_virality_score);
        println!("   📈 Progress Velocity: {:.2}%/min (momentum check)", analysis.progress_velocity);
        println!("   🛡️  Rug Risk Score: {:.3} (safety threshold: 0.6+)", analysis.holder_distribution_score);
        println!("   🔮 Growth Projection: {:.3} (5-min potential)", analysis.predictive_growth_score);
        
        // Position sizing recommendation
        let position_size = self.mathematical_engine.calculate_position_size(
            analysis.composite_virality_score,
            analysis.holder_distribution_score,
        );
        
        println!("\n{}", "💰 POSITION RECOMMENDATION:".bold());
        println!("   💎 Suggested Size: {:.2} SOL", position_size);
        println!("   ⚖️  Risk Level: {}", self.assess_risk_level(analysis));
        println!("   ⏱️  Time Horizon: 5-30 minutes (momentum play)");
        
        // Quick links for execution
        self.print_execution_links(mint);
        
        println!("{}", "=".repeat(80));
    }
    
    pub fn print_automated_sell_signal(&self, mint: &str, analysis: &MathematicalAnalysis, signal_type: &str) {
        let icon = match signal_type {
            "STRONG SELL" => "🚨📉",
            "SELL" => "⚠️💸",
            _ => "📊",
        };
        
        println!("\n{} {} AUTOMATED {} SIGNAL", icon, "🤖", signal_type.bold());
        println!("{}", "=".repeat(80));
        println!("🪙 Token: {}", mint);
        
        println!("\n{}", "⚠️ SELL TRIGGERS ACTIVATED:".bold());
        if analysis.price_velocity < -0.01 {
            println!("   📉 Momentum Reversal: Price dropping {:.4}/sec", analysis.price_velocity);
        }
        if analysis.composite_virality_score < 0.3 {
            println!("   💥 Virality Collapse: Score dropped to {:.3}", analysis.composite_virality_score);
        }
        if analysis.progress_velocity < -2.0 {
            println!("   🐌 Progress Stagnation: {:.2}%/min bonding momentum", analysis.progress_velocity);
        }
        
        println!("\n{}", "🎯 EXIT RECOMMENDATION: IMMEDIATE".bold().red());
        println!("   ⏱️  Execute within: <30 seconds for best price");
        
        self.print_execution_links(mint);
        
        println!("{}", "=".repeat(80));
    }
    
    pub fn print_hold_signal(&self, mint: &str, analysis: &MathematicalAnalysis) {
        println!("\n{} {} HOLD SIGNAL - High Potential", "⏸️", "🤖".bold());
        println!("{}", "-".repeat(60));
        println!("🪙 Token: {}", mint);
        println!("🎯 Virality Score: {:.3} (good, but conditions not met)", analysis.composite_virality_score);
        
        // Show what's missing
        println!("\n{}", "⏳ WAITING FOR:".bold());
        if analysis.progress_velocity < 2.0 {
            println!("   📈 Higher momentum (need >2%/min, have {:.2}%/min)", analysis.progress_velocity);
        }
        if analysis.holder_distribution_score < 0.6 {
            println!("   🛡️  Better risk profile (need >0.6, have {:.3})", analysis.holder_distribution_score);
        }
        
        println!("{}", "-".repeat(60));
    }
    
    fn assess_risk_level(&self, analysis: &MathematicalAnalysis) -> &str {
        if analysis.holder_distribution_score > 0.8 && analysis.composite_virality_score > 0.8 {
            "LOW 🟢"
        } else if analysis.holder_distribution_score > 0.6 && analysis.composite_virality_score > 0.7 {
            "MEDIUM 🟡"
        } else {
            "HIGH 🔴"
        }
    }
    
    fn print_execution_links(&self, mint: &str) {
        println!("\n{}", "⚡ QUICK EXECUTION LINKS:".bold());
        println!("   🚀 Pump.fun Trade: {}", 
            format!("https://pump.fun/{}", mint));
        println!("   📊 Live Chart: {}", 
            format!("https://dexscreener.com/solana/{}", mint));
        println!("   🔍 Token Safety: {}", 
            format!("https://rugcheck.xyz/tokens/{}", mint));
    }
}