/// Configuration manager with hot-reload capability

use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};
use std::path::Path;
use tokio::time::interval;
use super::Config;

pub struct ConfigManager {
    config_path: String,
    current_config: Arc<RwLock<Config>>,
    last_modified: RwLock<SystemTime>,
}

impl ConfigManager {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let config = Config::load_from_file(path)?;
        let last_modified = std::fs::metadata(path)?.modified()?;
        
        Ok(Self {
            config_path: path.to_string(),
            current_config: Arc::new(RwLock::new(config)),
            last_modified: RwLock::new(last_modified),
        })
    }
    
    pub fn get_config(&self) -> Config {
        self.current_config.read().unwrap().clone()
    }
    
    pub async fn start_hot_reload(&self) {
        let mut reload_timer = interval(Duration::from_secs(5));
        let config_path = self.config_path.clone();
        let current_config = Arc::clone(&self.current_config);
        let last_modified = Arc::new(RwLock::new(*self.last_modified.read().unwrap()));
        
        tokio::spawn(async move {
            loop {
                reload_timer.tick().await;
                
                if let Ok(metadata) = std::fs::metadata(&config_path) {
                    if let Ok(modified) = metadata.modified() {
                        let last_mod = *last_modified.read().unwrap();
                        
                        if modified > last_mod {
                            if let Ok(new_config) = Config::load_from_file(&config_path) {
                                *current_config.write().unwrap() = new_config;
                                *last_modified.write().unwrap() = modified;
                                println!("🔄 Configuration reloaded successfully");
                            } else {
                                println!("⚠️ Failed to reload configuration - keeping current");
                            }
                        }
                    }
                }
            }
        });
    }
    
    pub fn should_execute(&self, virality_score: f64, bonding_progress: f64, rug_score: f64, velocity: f64) -> bool {
        let config = self.get_config();
        let entry = &config.entry_criteria;
        
        // Check virality score if configured
        if let Some(min_virality) = entry.min_virality_score {
            if virality_score < min_virality {
                return false;
            }
        }
        
        // Check bonding curve progress if configured
        if let Some(max_bonding) = entry.max_bonding_curve_progress {
            if bonding_progress > max_bonding {
                return false;
            }
        }
        
        // Check rug score if configured
        if let Some(min_rug) = entry.min_rug_score {
            if rug_score < min_rug {
                return false;
            }
        }
        
        // Check progress velocity if configured
        if let Some(min_velocity) = entry.min_progress_velocity {
            if velocity < min_velocity {
                return false;
            }
        }
        
        true
    }
}