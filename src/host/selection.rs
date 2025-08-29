use ethers::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use tracing::{info, debug, warn};

use crate::host::registry::HostInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub jobs_completed: u32,
    pub success_rate: f64,      // 0.0 to 1.0
    pub avg_completion_time: u64, // milliseconds
    pub uptime_percentage: f64,  // 0.0 to 1.0
    pub current_load: u32,       // active jobs
    pub cost_per_token: f64,
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            jobs_completed: 0,
            success_rate: 0.5,  // Assume average performance for new hosts
            avg_completion_time: 1000,  // 1 second default
            uptime_percentage: 0.9,  // 90% default uptime
            current_load: 0,
            cost_per_token: 0.001,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScoringWeights {
    pub performance: f64,  // Default: 0.3
    pub cost: f64,        // Default: 0.2
    pub reliability: f64, // Default: 0.3
    pub load: f64,       // Default: 0.2
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            performance: 0.3,
            cost: 0.2,
            reliability: 0.3,
            load: 0.2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JobRequirements {
    pub model_id: String,
    pub min_ram_gb: u32,
    pub max_cost_per_token: Option<f64>,
    pub min_reliability: Option<f64>,
}

pub struct HostSelector {
    performance_tracker: Arc<RwLock<HashMap<Address, PerformanceMetrics>>>,
    weight_config: ScoringWeights,
}

impl HostSelector {
    pub fn new() -> Self {
        Self {
            performance_tracker: Arc::new(RwLock::new(HashMap::new())),
            weight_config: ScoringWeights::default(),
        }
    }
    
    pub fn with_weights(weights: ScoringWeights) -> Self {
        Self {
            performance_tracker: Arc::new(RwLock::new(HashMap::new())),
            weight_config: weights,
        }
    }
    
    pub fn calculate_host_score(&self, _host: &HostInfo, metrics: &PerformanceMetrics) -> f64 {
        // Normalize each factor to 0-1 range
        
        // Performance score (lower completion time is better)
        let perf_score = if metrics.avg_completion_time > 0 {
            1.0 / (1.0 + (metrics.avg_completion_time as f64 / 1000.0)) // Normalize to seconds
        } else {
            0.0
        };
        
        // Cost score (lower cost is better)
        let cost_score = if metrics.cost_per_token > 0.0 {
            1.0 / (1.0 + metrics.cost_per_token * 10000.0) // Scale for typical costs
        } else {
            0.0
        };
        
        // Reliability score (combination of success rate and uptime)
        let reliability_score = (metrics.success_rate + metrics.uptime_percentage) / 2.0;
        
        // Load score (lower load is better)
        let load_score = 1.0 / (1.0 + metrics.current_load as f64);
        
        // Apply weights and sum
        let total_score = 
            perf_score * self.weight_config.performance +
            cost_score * self.weight_config.cost +
            reliability_score * self.weight_config.reliability +
            load_score * self.weight_config.load;
        
        // Ensure score is between 0 and 1
        total_score.min(1.0).max(0.0)
    }
    
    pub async fn select_best_host(
        &self,
        hosts: Vec<HostInfo>,
        requirements: &JobRequirements
    ) -> Option<Address> {
        if hosts.is_empty() {
            return None;
        }
        
        // Filter hosts by requirements
        let filtered_hosts = self.filter_by_requirements(hosts, requirements).await;
        
        if filtered_hosts.is_empty() {
            warn!("No hosts meet the requirements");
            return None;
        }
        
        // Score each host
        let tracker = self.performance_tracker.read().await;
        let mut scored_hosts: Vec<(Address, f64)> = Vec::new();
        
        for host in &filtered_hosts {
            let metrics = tracker
                .get(&host.address)
                .cloned()
                .unwrap_or_default();
            
            let score = self.calculate_host_score(host, &metrics);
            scored_hosts.push((host.address, score));
        }
        
        // Sort by score (highest first)
        scored_hosts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        if let Some((addr, score)) = scored_hosts.first() {
            debug!("Selected best host {} with score {:.3}", addr, score);
            Some(*addr)
        } else {
            None
        }
    }
    
    pub async fn select_top_n_hosts(
        &self,
        hosts: Vec<HostInfo>,
        n: usize,
        requirements: &JobRequirements
    ) -> Vec<Address> {
        if hosts.is_empty() || n == 0 {
            return Vec::new();
        }
        
        // Filter hosts by requirements
        let filtered_hosts = self.filter_by_requirements(hosts, requirements).await;
        
        // Score each host
        let tracker = self.performance_tracker.read().await;
        let mut scored_hosts: Vec<(Address, f64)> = Vec::new();
        
        for host in &filtered_hosts {
            let metrics = tracker
                .get(&host.address)
                .cloned()
                .unwrap_or_default();
            
            let score = self.calculate_host_score(host, &metrics);
            scored_hosts.push((host.address, score));
        }
        
        // Sort by score (highest first)
        scored_hosts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        // Take top n
        scored_hosts
            .into_iter()
            .take(n)
            .map(|(addr, _)| addr)
            .collect()
    }
    
    pub async fn select_by_cost_optimization(&self, hosts: Vec<HostInfo>) -> Option<Address> {
        if hosts.is_empty() {
            return None;
        }
        
        let tracker = self.performance_tracker.read().await;
        let mut cheapest: Option<(Address, f64)> = None;
        
        for host in &hosts {
            if !host.is_online {
                continue;
            }
            
            let metrics = tracker
                .get(&host.address)
                .cloned()
                .unwrap_or_default();
            
            if let Some((_, min_cost)) = cheapest {
                if metrics.cost_per_token < min_cost {
                    cheapest = Some((host.address, metrics.cost_per_token));
                }
            } else {
                cheapest = Some((host.address, metrics.cost_per_token));
            }
        }
        
        if let Some((addr, cost)) = cheapest {
            debug!("Selected cheapest host {} with cost {:.6}", addr, cost);
            Some(addr)
        } else {
            None
        }
    }
    
    pub async fn select_by_performance(&self, hosts: Vec<HostInfo>) -> Option<Address> {
        if hosts.is_empty() {
            return None;
        }
        
        let tracker = self.performance_tracker.read().await;
        let mut fastest: Option<(Address, u64)> = None;
        
        for host in &hosts {
            if !host.is_online {
                continue;
            }
            
            let metrics = tracker
                .get(&host.address)
                .cloned()
                .unwrap_or_default();
            
            // Skip hosts with no performance data
            if metrics.avg_completion_time == 0 {
                continue;
            }
            
            if let Some((_, min_time)) = fastest {
                if metrics.avg_completion_time < min_time {
                    fastest = Some((host.address, metrics.avg_completion_time));
                }
            } else {
                fastest = Some((host.address, metrics.avg_completion_time));
            }
        }
        
        if let Some((addr, time)) = fastest {
            debug!("Selected fastest host {} with avg time {}ms", addr, time);
            Some(addr)
        } else {
            None
        }
    }
    
    pub async fn select_with_load_balancing(&self, hosts: Vec<HostInfo>) -> Option<Address> {
        if hosts.is_empty() {
            return None;
        }
        
        let tracker = self.performance_tracker.read().await;
        let mut least_loaded: Option<(Address, u32)> = None;
        
        for host in &hosts {
            if !host.is_online {
                continue;
            }
            
            // Only consider hosts with actual metrics (not default)
            if let Some(metrics) = tracker.get(&host.address) {
                if let Some((_, min_load)) = least_loaded {
                    if metrics.current_load < min_load {
                        least_loaded = Some((host.address, metrics.current_load));
                    }
                } else {
                    least_loaded = Some((host.address, metrics.current_load));
                }
            }
        }
        
        if let Some((addr, load)) = least_loaded {
            debug!("Selected least loaded host {} with {} active jobs", addr, load);
            Some(addr)
        } else {
            None
        }
    }
    
    pub async fn update_performance_metrics(&mut self, host: Address, metrics: PerformanceMetrics) {
        let mut tracker = self.performance_tracker.write().await;
        info!("Updating metrics for host {}: success_rate={:.2}, load={}", 
              host, metrics.success_rate, metrics.current_load);
        tracker.insert(host, metrics);
    }
    
    async fn filter_by_requirements(
        &self,
        hosts: Vec<HostInfo>,
        requirements: &JobRequirements
    ) -> Vec<HostInfo> {
        let tracker = self.performance_tracker.read().await;
        let mut filtered = Vec::new();
        
        for host in hosts {
            // Check if online
            if !host.is_online {
                continue;
            }
            
            // Check model support
            if !self.host_supports_model(&host.metadata, &requirements.model_id) {
                continue;
            }
            
            // Check RAM requirement
            if !self.host_meets_ram_requirement(&host.metadata, requirements.min_ram_gb) {
                continue;
            }
            
            // Check cost requirement
            if let Some(max_cost) = requirements.max_cost_per_token {
                let metrics = tracker
                    .get(&host.address)
                    .cloned()
                    .unwrap_or_default();
                
                if metrics.cost_per_token > max_cost {
                    continue;
                }
            }
            
            // Check reliability requirement
            if let Some(min_reliability) = requirements.min_reliability {
                let metrics = tracker
                    .get(&host.address)
                    .cloned()
                    .unwrap_or_default();
                
                let reliability = (metrics.success_rate + metrics.uptime_percentage) / 2.0;
                if reliability < min_reliability {
                    continue;
                }
            }
            
            filtered.push(host);
        }
        
        filtered
    }
    
    fn host_supports_model(&self, metadata: &str, model_id: &str) -> bool {
        // Parse metadata JSON to check for model support
        if let Ok(json) = serde_json::from_str::<Value>(metadata) {
            if let Some(models) = json.get("models") {
                if let Some(models_array) = models.as_array() {
                    for model in models_array {
                        if let Some(model_str) = model.as_str() {
                            if model_str == model_id {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        
        // Fallback: simple string search
        metadata.contains(model_id)
    }
    
    fn host_meets_ram_requirement(&self, metadata: &str, min_ram_gb: u32) -> bool {
        // Parse metadata JSON to check RAM
        if let Ok(json) = serde_json::from_str::<Value>(metadata) {
            if let Some(ram) = json.get("ram") {
                if let Some(ram_value) = ram.as_u64() {
                    return ram_value as u32 >= min_ram_gb;
                }
            }
        }
        
        true // If can't parse, assume it meets requirement
    }
    
    pub async fn get_metrics_count(&self) -> usize {
        let tracker = self.performance_tracker.read().await;
        tracker.len()
    }
}