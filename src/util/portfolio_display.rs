/// Real-time portfolio display utilities

use std::sync::Arc;
use colored::Colorize;
use crate::execution::{PositionManager, PortfolioTracker};
use crate::util::price_feed::PriceFeed;

pub struct PortfolioDisplayer {
    position_manager: Arc<PositionManager>,
    portfolio_tracker: Arc<std::sync::Mutex<PortfolioTracker>>,
    price_feed: Arc<PriceFeed>,
}

impl PortfolioDisplayer {
    pub fn new(
        position_manager: Arc<PositionManager>,
        portfolio_tracker: Arc<std::sync::Mutex<PortfolioTracker>>,
        price_feed: Arc<PriceFeed>,
    ) -> Self {
        Self {
            position_manager,
            portfolio_tracker,
            price_feed,
        }
    }
    
    pub async fn print_portfolio_status(&self) {
        println!("\n{}", "📊 PORTFOLIO STATUS".black().bold());
        println!("{}", "═".repeat(60).black());
        
        let positions = self.position_manager.get_open_positions();
        let sol_rate = match self.price_feed.get_sol_usd_rate().await {
            Ok(rate) => rate,
            Err(e) => {
                println!("⚠️ Failed to get SOL/USD rate for portfolio display: {}", e);
                return;
            }
        };
        
        if positions.is_empty() {
            println!("{}", "💤 No active positions".bright_yellow());
            return;
        }
        
        let mut total_invested_sol = 0.0;
        let mut total_current_value_sol = 0.0;
        
        for (i, position) in positions.iter().enumerate() {
            // Use the USD values that are properly maintained and convert to SOL
            let current_value_sol = position.current_value_usd / sol_rate;
            let pnl_percent = if position.entry_usd > 0.0 {
                ((position.current_value_usd - position.entry_usd) / position.entry_usd) * 100.0
            } else {
                0.0
            };
            let age_hours = position.get_age_hours();
            
            total_invested_sol += position.sol_invested;
            total_current_value_sol += current_value_sol;
            
            let pnl_color = if pnl_percent > 0.0 { "green" } else { "red" };
            let status_emoji = match position.status {
                crate::execution::PositionStatus::Open => "🟢",
                crate::execution::PositionStatus::PartialExit => "🟡",
                crate::execution::PositionStatus::Closed => "🔴",
            };
            
            println!(
                "{} {} ({}) | {:.6} | P&L: {} | Age: {:.1}h",
                status_emoji,
                position.mint.chars().take(8).collect::<String>(),
                position.symbol,
                position.current_price,
                format!("{:+.1}%", pnl_percent).color(pnl_color),
                age_hours
            );
            
            println!(
                "   💰 Invested: {:.3} SOL (${:.2}) | Current: {:.3} SOL (${:.2})",
                position.sol_invested,
                position.entry_usd,
                current_value_sol,
                position.current_value_usd
            );
            
            if i < positions.len() - 1 {
                println!("{}", "─".repeat(60).bright_black());
            }
        }
        
        let total_pnl_sol = total_current_value_sol - total_invested_sol;
        let total_pnl_percent = if total_invested_sol > 0.0 {
            (total_pnl_sol / total_invested_sol) * 100.0
        } else {
            0.0
        };
        
        println!("{}", "═".repeat(60).black());
        println!(
            "{} Total: {:.3} SOL (${:.2}) | P&L: {} (${:.2})",
            "📈".bright_green(),
            total_current_value_sol,
            total_current_value_sol * sol_rate,
            format!("{:+.1}%", total_pnl_percent).color(if total_pnl_percent > 0.0 { "green" } else { "red" }),
            total_pnl_sol * sol_rate
        );
        println!("{}", "═".repeat(60).black());
    }
    
    pub fn print_position_summary(&self) {
        let positions = self.position_manager.get_open_positions();
        let active_count = positions.iter()
            .filter(|p| matches!(p.status, crate::execution::PositionStatus::Open))
            .count();
        
        println!("📊 {} active positions", active_count);
    }
}