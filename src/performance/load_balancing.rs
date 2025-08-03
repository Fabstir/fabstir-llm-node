use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Mutex};
use thiserror::Error;
use serde::{Serialize, Deserialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct LoadBalancerConfig {
    pub strategy: LoadStrategy,
    pub health_check_interval_secs: u64,
    pub node_timeout_secs: u64,
    pub max_retries: usize,
    pub enable_session_affinity: bool,
    pub load_threshold: f64,
    pub rebalance_interval_secs: u64,
}

impl Default for LoadBalancerConfig {
    fn default() -> Self {
        Self {
            strategy: LoadStrategy::LeastConnections,
            health_check_interval_secs: 10,
            node_timeout_secs: 30,
            max_retries: 3,
            enable_session_affinity: false,
            load_threshold: 0.8,
            rebalance_interval_secs: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LoadStrategy {
    RoundRobin,
    LeastConnections,
    WeightedRoundRobin,
    Random,
    LeastResponseTime,
    ResourceBased,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeStatus {
    Healthy,
    Unhealthy,
    Draining,
    Drained,
    Maintenance,
    CircuitOpen,
    CircuitHalfOpen,
}

#[derive(Debug, Clone)]
pub struct WorkerNode {
    pub id: String,
    pub address: String,
    pub capabilities: NodeCapabilities,
    pub status: NodeStatus,
}

#[derive(Debug, Clone, Default)]
pub struct NodeCapabilities {
    pub models: Vec<String>,
    pub max_batch_size: usize,
    pub gpu_memory_gb: u64,
    pub supports_streaming: bool,
}

#[derive(Debug, Clone)]
pub struct WorkerMetrics {
    pub active_connections: usize,
    pub requests_per_second: f64,
    pub average_latency_ms: f64,
    pub cpu_usage_percent: f64,
    pub memory_usage_percent: f64,
    pub gpu_usage_percent: f64,
    pub error_rate: f64,
    pub last_health_check: Instant,
    // Additional fields expected by tests
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub gpu_usage: f64,
    pub queue_depth: usize,
    pub request_success_rate: f64,
}

#[derive(Debug, Clone)]
pub struct LoadDistribution {
    pub node_loads: HashMap<String, f64>,
    pub total_requests: u64,
    pub average_load: f64,
    pub peak_load: f64,
    pub distribution_efficiency: f64,
}

#[derive(Debug, Clone)]
pub struct LoadMetrics {
    pub requests_per_second: f64,
    pub average_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub error_rate: f64,
    pub active_nodes: usize,
    pub total_requests: u64,
    pub nodes: HashMap<String, WorkerMetrics>,
}

#[derive(Debug, Clone)]
pub struct HealthCheck {
    pub endpoint: String,
    pub interval_secs: u64,
    pub timeout_secs: u64,
    pub unhealthy_threshold: u32,
    pub healthy_threshold: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionAffinity {
    None,
    ClientIp,
    Cookie,
    Header(String),
}

#[derive(Error, Debug)]
pub enum LoadBalancerError {
    #[error("No healthy nodes available")]
    NoHealthyNodes,
    #[error("All nodes are overloaded")]
    AllNodesOverloaded,
    #[error("Node not found: {node_id}")]
    NodeNotFound { node_id: String },
    #[error("Model not supported by any node: {model_id}")]
    ModelNotSupported { model_id: String },
    #[error("Connection limit reached for node: {node_id}")]
    ConnectionLimitReached { node_id: String },
    #[error("Health check failed: {reason}")]
    HealthCheckFailed { reason: String },
}

struct NodeState {
    node: WorkerNode,
    metrics: WorkerMetrics,
    health_check_failures: u32,
    weight: f64,
    active_connections: HashMap<String, Instant>,
}

struct BalancerState {
    nodes: HashMap<String, NodeState>,
    round_robin_index: usize,
    session_affinity_map: HashMap<String, String>,
    total_requests: u64,
    latency_history: VecDeque<f64>,
    last_rebalance: Instant,
}

pub struct LoadBalancer {
    config: LoadBalancerConfig,
    state: Arc<RwLock<BalancerState>>,
}

impl LoadBalancer {
    pub async fn new(config: LoadBalancerConfig, nodes: Vec<WorkerNode>) -> Result<Self> {
        let mut node_states = HashMap::new();
        
        for node in nodes {
            let state = NodeState {
                metrics: WorkerMetrics {
                    active_connections: 0,
                    requests_per_second: 0.0,
                    average_latency_ms: 50.0,
                    cpu_usage_percent: 20.0,
                    memory_usage_percent: 30.0,
                    gpu_usage_percent: 0.0,
                    error_rate: 0.0,
                    last_health_check: Instant::now(),
                    cpu_usage: 0.20,
                    memory_usage: 0.30,
                    gpu_usage: 0.0,
                    queue_depth: 0,
                    request_success_rate: 0.95,
                },
                health_check_failures: 0,
                weight: 1.0,
                active_connections: HashMap::new(),
                node: node.clone(),
            };
            node_states.insert(node.id.clone(), state);
        }

        let state = BalancerState {
            nodes: node_states,
            round_robin_index: 0,
            session_affinity_map: HashMap::new(),
            total_requests: 0,
            latency_history: VecDeque::with_capacity(1000),
            last_rebalance: Instant::now(),
        };

        let balancer = Self {
            config,
            state: Arc::new(RwLock::new(state)),
        };

        // Start health check task
        let balancer_clone = balancer.clone();
        tokio::spawn(async move {
            balancer_clone.health_check_loop().await;
        });

        Ok(balancer)
    }

    pub async fn select_node(&self, model_id: &str, session_id: Option<&str>) -> Result<WorkerNode> {
        let mut state = self.state.write().await;
        state.total_requests += 1;

        // Check session affinity
        if self.config.enable_session_affinity {
            if let Some(session_id) = session_id {
                if let Some(node_id) = state.session_affinity_map.get(session_id) {
                    if let Some(node_state) = state.nodes.get(node_id) {
                        if node_state.node.status == NodeStatus::Healthy &&
                           node_state.node.capabilities.models.contains(&model_id.to_string()) {
                            return Ok(node_state.node.clone());
                        }
                    }
                }
            }
        }

        // Filter healthy nodes that support the model
        let eligible_nodes: Vec<String> = state.nodes.iter()
            .filter(|(_, node_state)| {
                node_state.node.status == NodeStatus::Healthy &&
                node_state.node.capabilities.models.contains(&model_id.to_string())
            })
            .map(|(id, _)| id.clone())
            .collect();

        if eligible_nodes.is_empty() {
            return Err(LoadBalancerError::NoHealthyNodes.into());
        }

        // Check if all nodes are overloaded
        let overloaded_count = eligible_nodes.iter()
            .filter(|id| {
                state.nodes.get(*id)
                    .map(|node| {
                        let load = (node.metrics.cpu_usage_percent + 
                                   node.metrics.memory_usage_percent) / 2.0;
                        load > self.config.load_threshold * 100.0
                    })
                    .unwrap_or(false)
            })
            .count();

        if overloaded_count == eligible_nodes.len() {
            return Err(LoadBalancerError::AllNodesOverloaded.into());
        }

        // Select node based on strategy
        let selected_node_id = match self.config.strategy {
            LoadStrategy::RoundRobin => {
                let idx = state.round_robin_index % eligible_nodes.len();
                state.round_robin_index = (state.round_robin_index + 1) % eligible_nodes.len();
                eligible_nodes[idx].clone()
            }
            LoadStrategy::LeastConnections => {
                eligible_nodes.iter()
                    .min_by_key(|id| {
                        state.nodes.get(*id)
                            .map(|n| n.metrics.active_connections)
                            .unwrap_or(usize::MAX)
                    })
                    .cloned()
                    .unwrap()
            }
            LoadStrategy::WeightedRoundRobin => {
                // Simple weighted selection based on available resources
                let weights: Vec<(String, f64)> = eligible_nodes.iter()
                    .map(|id| {
                        state.nodes.get(id)
                            .map(|node| {
                                let weight = node.weight * (1.0 - node.metrics.cpu_usage_percent / 100.0);
                                (id.clone(), weight)
                            })
                            .unwrap_or((id.clone(), 0.0))
                    })
                    .collect();
                
                // Select based on weights
                let total_weight: f64 = weights.iter().map(|(_, w)| w).sum();
                let mut random_point = rand::random::<f64>() * total_weight;
                
                let mut selected = eligible_nodes[0].clone();
                for (id, weight) in weights {
                    random_point -= weight;
                    if random_point <= 0.0 {
                        selected = id;
                        break;
                    }
                }
                selected
            }
            LoadStrategy::Random => {
                let idx = rand::random::<usize>() % eligible_nodes.len();
                eligible_nodes[idx].clone()
            }
            LoadStrategy::LeastResponseTime => {
                eligible_nodes.iter()
                    .min_by_key(|id| {
                        state.nodes.get(*id)
                            .map(|n| n.metrics.average_latency_ms as u64)
                            .unwrap_or(u64::MAX)
                    })
                    .cloned()
                    .unwrap()
            }
            LoadStrategy::ResourceBased => {
                // Select based on available resources
                eligible_nodes.iter()
                    .min_by_key(|id| {
                        state.nodes.get(*id)
                            .map(|node| {
                                let load_score = (
                                    node.metrics.cpu_usage_percent * 0.3 +
                                    node.metrics.memory_usage_percent * 0.3 +
                                    node.metrics.gpu_usage_percent * 0.4
                                ) as u64;
                                load_score
                            })
                            .unwrap_or(u64::MAX)
                    })
                    .cloned()
                    .unwrap()
            }
        };

        // Update session affinity if enabled
        if self.config.enable_session_affinity {
            if let Some(session_id) = session_id {
                state.session_affinity_map.insert(session_id.to_string(), selected_node_id.clone());
            }
        }

        if let Some(node_state) = state.nodes.get(&selected_node_id) {
            Ok(node_state.node.clone())
        } else {
            Err(LoadBalancerError::NoHealthyNodes.into())
        }
    }

    pub async fn acquire_connection(&self, node_id: &str) -> Result<String> {
        let mut state = self.state.write().await;
        
        if let Some(node_state) = state.nodes.get_mut(node_id) {
            let connection_id = Uuid::new_v4().to_string();
            node_state.active_connections.insert(connection_id.clone(), Instant::now());
            node_state.metrics.active_connections = node_state.active_connections.len();
            Ok(connection_id)
        } else {
            Err(LoadBalancerError::NodeNotFound {
                node_id: node_id.to_string(),
            }.into())
        }
    }

    pub async fn release_connection(&self, node_id: &str, connection_id: &str) -> Result<()> {
        let mut state = self.state.write().await;
        
        if let Some(node_state) = state.nodes.get_mut(node_id) {
            node_state.active_connections.remove(connection_id);
            node_state.metrics.active_connections = node_state.active_connections.len();
            Ok(())
        } else {
            Err(LoadBalancerError::NodeNotFound {
                node_id: node_id.to_string(),
            }.into())
        }
    }

    pub async fn add_node(&self, node: WorkerNode) -> Result<()> {
        let mut state = self.state.write().await;
        
        let node_state = NodeState {
            metrics: WorkerMetrics {
                active_connections: 0,
                requests_per_second: 0.0,
                average_latency_ms: 50.0,
                cpu_usage_percent: 20.0,
                memory_usage_percent: 30.0,
                gpu_usage_percent: 0.0,
                error_rate: 0.0,
                last_health_check: Instant::now(),
                cpu_usage: 0.20,
                memory_usage: 0.30,
                gpu_usage: 0.0,
                queue_depth: 0,
                request_success_rate: 0.95,
            },
            health_check_failures: 0,
            weight: 1.0,
            active_connections: HashMap::new(),
            node: node.clone(),
        };
        
        state.nodes.insert(node.id.clone(), node_state);
        Ok(())
    }

    pub async fn remove_node(&self, node_id: &str) -> Result<()> {
        let mut state = self.state.write().await;
        
        // Mark as draining first
        if let Some(node_state) = state.nodes.get_mut(node_id) {
            node_state.node.status = NodeStatus::Draining;
        }
        
        // Wait for connections to drain (mock)
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Remove node
        state.nodes.remove(node_id);
        
        // Clear session affinity entries
        state.session_affinity_map.retain(|_, v| v != node_id);
        
        Ok(())
    }

    pub async fn update_node_weight(&self, node_id: &str, weight: f64) -> Result<()> {
        let mut state = self.state.write().await;
        
        if let Some(node_state) = state.nodes.get_mut(node_id) {
            node_state.weight = weight;
            Ok(())
        } else {
            Err(LoadBalancerError::NodeNotFound {
                node_id: node_id.to_string(),
            }.into())
        }
    }

    pub async fn set_node_status(&self, node_id: &str, status: NodeStatus) -> Result<()> {
        let mut state = self.state.write().await;
        
        if let Some(node_state) = state.nodes.get_mut(node_id) {
            node_state.node.status = status;
            Ok(())
        } else {
            Err(LoadBalancerError::NodeNotFound {
                node_id: node_id.to_string(),
            }.into())
        }
    }

    pub async fn get_node_status(&self, node_id: &str) -> Result<NodeStatus> {
        let state = self.state.read().await;
        
        if let Some(node_state) = state.nodes.get(node_id) {
            Ok(node_state.node.status.clone())
        } else {
            Err(LoadBalancerError::NodeNotFound {
                node_id: node_id.to_string(),
            }.into())
        }
    }

    pub async fn start_node_drain(&self, node_id: &str) -> Result<()> {
        self.set_node_status(node_id, NodeStatus::Draining).await
    }

    pub async fn release_all_connections(&self, node_id: &str) -> Result<()> {
        let mut state = self.state.write().await;
        
        if let Some(node_state) = state.nodes.get_mut(node_id) {
            node_state.active_connections.clear();
            node_state.metrics.active_connections = 0;
            node_state.node.status = NodeStatus::Drained;
            Ok(())
        } else {
            Err(LoadBalancerError::NodeNotFound {
                node_id: node_id.to_string(),
            }.into())
        }
    }

    pub async fn update_node_metrics(&self, node_id: &str, metrics: WorkerMetrics) -> Result<()> {
        let mut state = self.state.write().await;
        
        if let Some(node_state) = state.nodes.get_mut(node_id) {
            node_state.metrics = metrics;
            Ok(())
        } else {
            Err(LoadBalancerError::NodeNotFound {
                node_id: node_id.to_string(),
            }.into())
        }
    }

    pub async fn enable_auto_rebalancing(&self, interval: Duration) -> Result<()> {
        // Mock implementation - in real implementation would start rebalancing task
        Ok(())
    }

    pub async fn get_node_metrics(&self, node_id: &str) -> Result<WorkerMetrics> {
        let state = self.state.read().await;
        
        if let Some(node_state) = state.nodes.get(node_id) {
            Ok(node_state.metrics.clone())
        } else {
            Err(LoadBalancerError::NodeNotFound {
                node_id: node_id.to_string(),
            }.into())
        }
    }

    pub async fn get_load_distribution(&self) -> LoadDistribution {
        let state = self.state.read().await;
        
        let mut node_loads = HashMap::new();
        let mut total_load = 0.0;
        let mut peak_load: f64 = 0.0;
        
        for (id, node_state) in &state.nodes {
            let load = (node_state.metrics.cpu_usage_percent + 
                       node_state.metrics.memory_usage_percent + 
                       node_state.metrics.gpu_usage_percent) / 3.0;
            node_loads.insert(id.clone(), load);
            total_load += load;
            peak_load = peak_load.max(load);
        }
        
        let node_count = state.nodes.len() as f64;
        let average_load = if node_count > 0.0 {
            total_load / node_count
        } else {
            0.0
        };
        
        // Calculate distribution efficiency (lower variance is better)
        let variance: f64 = node_loads.values()
            .map(|&load| (load - average_load).powi(2))
            .sum::<f64>() / node_count;
        let distribution_efficiency = 1.0 / (1.0 + variance);
        
        LoadDistribution {
            node_loads,
            total_requests: state.total_requests,
            average_load,
            peak_load,
            distribution_efficiency,
        }
    }

    pub async fn get_metrics(&self) -> LoadMetrics {
        let state = self.state.read().await;
        
        let active_nodes = state.nodes.values()
            .filter(|n| n.node.status == NodeStatus::Healthy)
            .count();
        
        let total_rps: f64 = state.nodes.values()
            .map(|n| n.metrics.requests_per_second)
            .sum();
        
        let avg_latency: f64 = if !state.latency_history.is_empty() {
            state.latency_history.iter().sum::<f64>() / state.latency_history.len() as f64
        } else {
            0.0
        };
        
        // Calculate percentiles (mock)
        let p95_latency = avg_latency * 1.5;
        let p99_latency = avg_latency * 2.0;
        
        let error_rate: f64 = state.nodes.values()
            .map(|n| n.metrics.error_rate)
            .sum::<f64>() / state.nodes.len().max(1) as f64;
        
        // Collect per-node metrics
        let nodes: HashMap<String, WorkerMetrics> = state.nodes.iter()
            .map(|(id, node_state)| (id.clone(), node_state.metrics.clone()))
            .collect();
        
        LoadMetrics {
            requests_per_second: total_rps,
            average_latency_ms: avg_latency,
            p95_latency_ms: p95_latency,
            p99_latency_ms: p99_latency,
            error_rate,
            active_nodes,
            total_requests: state.total_requests,
            nodes,
        }
    }

    pub async fn rebalance(&self) -> Result<()> {
        let mut state = self.state.write().await;
        
        // Check if rebalance is needed
        let now = Instant::now();
        if now.duration_since(state.last_rebalance) < Duration::from_secs(self.config.rebalance_interval_secs) {
            return Ok(());
        }
        
        // Rebalance logic (mock)
        // In real implementation, would migrate connections, update weights, etc.
        state.last_rebalance = now;
        
        Ok(())
    }

    pub async fn start_health_monitoring(&self) -> Result<()> {
        // Health monitoring is already started in new()
        // This method is for explicit control in tests
        Ok(())
    }
    
    async fn health_check_loop(&self) {
        loop {
            tokio::time::sleep(Duration::from_secs(self.config.health_check_interval_secs)).await;
            
            let node_ids: Vec<String> = {
                let state = self.state.read().await;
                state.nodes.keys().cloned().collect()
            };
            
            for node_id in node_ids {
                self.perform_health_check(&node_id).await.ok();
            }
        }
    }

    pub async fn record_request_failure(&self, node_id: &str, _reason: &str) -> Result<()> {
        let mut state = self.state.write().await;
        
        if let Some(node_state) = state.nodes.get_mut(node_id) {
            // Increment error rate
            node_state.metrics.error_rate = 
                (node_state.metrics.error_rate * 0.9) + 0.1; // Exponential moving average
            
            // Check if we should open circuit breaker
            if node_state.metrics.error_rate > 0.5 {
                node_state.node.status = NodeStatus::CircuitOpen;
            }
            
            Ok(())
        } else {
            Err(LoadBalancerError::NodeNotFound {
                node_id: node_id.to_string(),
            }.into())
        }
    }

    pub async fn mark_node_unhealthy(&self, node_id: &str, reason: &str) -> Result<()> {
        let mut state = self.state.write().await;
        
        if let Some(node_state) = state.nodes.get_mut(node_id) {
            node_state.node.status = NodeStatus::Unhealthy;
            // Log the reason (in real implementation)
            tracing::warn!("Node {} marked unhealthy: {}", node_id, reason);
            Ok(())
        } else {
            Err(LoadBalancerError::NodeNotFound {
                node_id: node_id.to_string(),
            }.into())
        }
    }
    
    pub async fn mock_health_check_result(&self, node_id: &str, is_healthy: bool) -> Result<()> {
        let mut state = self.state.write().await;
        
        if let Some(node_state) = state.nodes.get_mut(node_id) {
            if is_healthy {
                node_state.health_check_failures = 0;
                if node_state.node.status == NodeStatus::Unhealthy {
                    node_state.node.status = NodeStatus::Healthy;
                }
            } else {
                node_state.health_check_failures += 1;
                if node_state.health_check_failures >= 3 {
                    node_state.node.status = NodeStatus::Unhealthy;
                }
            }
            Ok(())
        } else {
            Err(LoadBalancerError::NodeNotFound {
                node_id: node_id.to_string(),
            }.into())
        }
    }

    async fn perform_health_check(&self, node_id: &str) -> Result<()> {
        let mut state = self.state.write().await;
        
        if let Some(node_state) = state.nodes.get_mut(node_id) {
            // Mock health check
            let is_healthy = rand::random::<f64>() > 0.05; // 95% success rate
            
            if is_healthy {
                node_state.health_check_failures = 0;
                if node_state.node.status == NodeStatus::Unhealthy {
                    node_state.node.status = NodeStatus::Healthy;
                }
            } else {
                node_state.health_check_failures += 1;
                if node_state.health_check_failures >= 3 {
                    node_state.node.status = NodeStatus::Unhealthy;
                }
            }
            
            node_state.metrics.last_health_check = Instant::now();
            
            // Update mock metrics
            node_state.metrics.cpu_usage_percent = 20.0 + rand::random::<f64>() * 60.0;
            node_state.metrics.memory_usage_percent = 30.0 + rand::random::<f64>() * 50.0;
            node_state.metrics.gpu_usage_percent = rand::random::<f64>() * 80.0;
            node_state.metrics.requests_per_second = rand::random::<f64>() * 100.0;
            node_state.metrics.average_latency_ms = 10.0 + rand::random::<f64>() * 90.0;
            
            // Sync duplicate fields (as percentages)
            node_state.metrics.cpu_usage = node_state.metrics.cpu_usage_percent / 100.0;
            node_state.metrics.memory_usage = node_state.metrics.memory_usage_percent / 100.0;
            node_state.metrics.gpu_usage = node_state.metrics.gpu_usage_percent / 100.0;
            node_state.metrics.queue_depth = (rand::random::<f64>() * 100.0) as usize;
            node_state.metrics.request_success_rate = 1.0 - node_state.metrics.error_rate;
        }
        
        Ok(())
    }

}

impl Clone for LoadBalancer {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: self.state.clone(),
        }
    }
}

// Request router for advanced routing logic
pub struct RequestRouter {
    balancer: Arc<LoadBalancer>,
}

impl RequestRouter {
    pub fn new(balancer: Arc<LoadBalancer>) -> Self {
        Self { balancer }
    }

    pub async fn route_request(
        &self,
        model_id: &str,
        session_id: Option<&str>,
        _metadata: Option<HashMap<String, String>>,
    ) -> Result<WorkerNode> {
        // Could implement more complex routing based on metadata
        self.balancer.select_node(model_id, session_id).await
    }

    pub async fn route_with_retry(
        &self,
        model_id: &str,
        session_id: Option<&str>,
    ) -> Result<WorkerNode> {
        let mut last_error = None;
        
        for _ in 0..self.balancer.config.max_retries {
            match self.balancer.select_node(model_id, session_id).await {
                Ok(node) => return Ok(node),
                Err(e) => {
                    last_error = Some(e);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| LoadBalancerError::NoHealthyNodes.into()))
    }
}

// For random number generation
mod rand {
    pub fn random<T>() -> T
    where
        T: RandomValue,
    {
        T::random()
    }

    pub trait RandomValue {
        fn random() -> Self;
    }

    impl RandomValue for f64 {
        fn random() -> Self {
            // Mock random value
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos();
            (nanos % 1000) as f64 / 1000.0
        }
    }

    impl RandomValue for usize {
        fn random() -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos();
            nanos as usize
        }
    }
}