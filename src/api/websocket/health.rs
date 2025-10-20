// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub uptime_seconds: u64,
    pub version: String,
}

/// Chain-specific health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainHealthStatus {
    pub chain_id: u64,
    pub chain_name: String,
    pub is_healthy: bool,
    pub rpc_responsive: bool,
    pub last_block_time: u64,
    pub connection_count: usize,
    pub error_rate: f64,
    pub average_latency_ms: u64,
}

/// Readiness check
pub struct ReadinessCheck {
    websocket_ready: Arc<RwLock<bool>>,
    inference_ready: Arc<RwLock<bool>>,
    blockchain_ready: Arc<RwLock<bool>>,
    chain_ready: Arc<RwLock<HashMap<u64, bool>>>,
}

impl ReadinessCheck {
    pub fn new() -> Self {
        Self {
            websocket_ready: Arc::new(RwLock::new(false)),
            inference_ready: Arc::new(RwLock::new(false)),
            blockchain_ready: Arc::new(RwLock::new(false)),
            chain_ready: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn is_ready(&self) -> bool {
        *self.websocket_ready.read().await
            && *self.inference_ready.read().await
            && *self.blockchain_ready.read().await
    }

    pub async fn set_websocket_ready(&self, ready: bool) {
        *self.websocket_ready.write().await = ready;
    }

    pub async fn set_inference_ready(&self, ready: bool) {
        *self.inference_ready.write().await = ready;
    }

    pub async fn set_blockchain_ready(&self, ready: bool) {
        *self.blockchain_ready.write().await = ready;
    }

    pub async fn get_status(&self) -> ReadinessStatus {
        ReadinessStatus {
            websocket_ready: *self.websocket_ready.read().await,
            inference_ready: *self.inference_ready.read().await,
            blockchain_ready: *self.blockchain_ready.read().await,
        }
    }

    // Chain-specific readiness methods
    pub async fn set_chain_ready(&self, chain_id: u64, ready: bool) {
        self.chain_ready.write().await.insert(chain_id, ready);
    }

    pub async fn is_chain_ready(&self, chain_id: u64) -> bool {
        self.chain_ready
            .read()
            .await
            .get(&chain_id)
            .copied()
            .unwrap_or(false)
    }

    pub async fn all_chains_ready(&self) -> bool {
        let chains = self.chain_ready.read().await;
        if chains.is_empty() {
            return false;
        }
        chains.values().all(|&ready| ready)
    }

    pub async fn get_chain_status(&self) -> HashMap<u64, bool> {
        self.chain_ready.read().await.clone()
    }
}

#[derive(Debug, Serialize)]
pub struct ReadinessStatus {
    pub websocket_ready: bool,
    pub inference_ready: bool,
    pub blockchain_ready: bool,
}

/// Liveness check
pub struct LivenessCheck {
    last_heartbeats: Arc<RwLock<HashMap<String, Instant>>>,
    max_heartbeat_age: Duration,
}

impl LivenessCheck {
    pub fn new() -> Self {
        Self {
            last_heartbeats: Arc::new(RwLock::new(HashMap::new())),
            max_heartbeat_age: Duration::from_secs(30),
        }
    }

    pub async fn is_alive(&self) -> bool {
        let heartbeats = self.last_heartbeats.read().await;

        // If no heartbeats recorded, consider alive (startup phase)
        if heartbeats.is_empty() {
            return true;
        }

        // Check all components have recent heartbeats
        for (_, last_beat) in heartbeats.iter() {
            if last_beat.elapsed() > self.max_heartbeat_age {
                return false;
            }
        }

        true
    }

    pub async fn record_heartbeat(&self, component: &str) {
        self.last_heartbeats
            .write()
            .await
            .insert(component.to_string(), Instant::now());
    }

    pub async fn simulate_hang(&self, component: &str, age: Duration) {
        self.last_heartbeats
            .write()
            .await
            .insert(component.to_string(), Instant::now() - age);
    }

    pub async fn get_status(&self) -> LivenessStatus {
        let heartbeats = self.last_heartbeats.read().await;

        LivenessStatus {
            last_websocket_heartbeat: heartbeats
                .get("websocket")
                .map(|t| t.elapsed())
                .unwrap_or(Duration::ZERO),
            last_inference_heartbeat: heartbeats
                .get("inference")
                .map(|t| t.elapsed())
                .unwrap_or(Duration::ZERO),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LivenessStatus {
    pub last_websocket_heartbeat: Duration,
    pub last_inference_heartbeat: Duration,
}

/// System resources
#[derive(Debug, Serialize)]
pub struct SystemResources {
    pub cpu_usage_percent: f64,
    pub memory_used_mb: usize,
    pub memory_available_mb: usize,
    pub disk_used_gb: usize,
    pub network_connections: usize,
}

/// Health metrics
#[derive(Debug, Serialize)]
pub struct HealthMetrics {
    pub total_requests: usize,
    pub successful_requests: usize,
    pub failed_requests: usize,
    pub success_rate: f64,
    pub uptime_seconds: u64,
}

/// Main health checker
pub struct HealthChecker {
    start_time: Instant,
    readiness: ReadinessCheck,
    liveness: LivenessCheck,
    request_stats: Arc<RwLock<RequestStats>>,
    circuit_breaker: Option<CircuitBreaker>,
    server_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
}

#[derive(Default)]
struct RequestStats {
    total: usize,
    successful: usize,
    failed: usize,
}

struct CircuitBreaker {
    failure_threshold: usize,
    reset_timeout: Duration,
    failures: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
    state: Arc<RwLock<HashMap<String, CircuitState>>>,
}

#[derive(Debug, Clone, PartialEq)]
enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl HealthChecker {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            readiness: ReadinessCheck::new(),
            liveness: LivenessCheck::new(),
            request_stats: Arc::new(RwLock::new(RequestStats::default())),
            circuit_breaker: None,
            server_handle: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_circuit_breaker(failure_threshold: usize, reset_timeout: Duration) -> Self {
        let mut checker = Self::new();
        checker.circuit_breaker = Some(CircuitBreaker {
            failure_threshold,
            reset_timeout,
            failures: Arc::new(RwLock::new(HashMap::new())),
            state: Arc::new(RwLock::new(HashMap::new())),
        });
        checker
    }

    pub async fn start_server(&self, port: u16) -> Result<()> {
        let start_time = self.start_time;

        let handle = tokio::spawn(async move {
            // Mock health server
            let app = axum::Router::new().route(
                "/health",
                axum::routing::get(move || async move {
                    let status = HealthStatus {
                        status: "healthy".to_string(),
                        uptime_seconds: start_time.elapsed().as_secs(),
                        version: "1.0.0".to_string(),
                    };
                    axum::Json(status)
                }),
            );

            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
                .await
                .unwrap();

            axum::serve(listener, app).await.unwrap();
        });

        *self.server_handle.write().await = Some(handle);
        Ok(())
    }

    pub async fn stop_server(&self) {
        if let Some(handle) = self.server_handle.write().await.take() {
            handle.abort();
        }
    }

    pub async fn get_system_resources(&self) -> SystemResources {
        // Mock system resources
        SystemResources {
            cpu_usage_percent: 25.5,
            memory_used_mb: 512,
            memory_available_mb: 16384,
            disk_used_gb: 100,
            network_connections: 42,
        }
    }

    pub async fn check_resources_healthy(&self) -> bool {
        let resources = self.get_system_resources().await;
        resources.cpu_usage_percent < 80.0 && resources.memory_available_mb > 1024
    }

    pub async fn circuit_state(&self) -> String {
        if let Some(breaker) = &self.circuit_breaker {
            let states = breaker.state.read().await;
            states
                .get("inference")
                .map(|s| match s {
                    CircuitState::Closed => "closed",
                    CircuitState::Open => "open",
                    CircuitState::HalfOpen => "half-open",
                })
                .unwrap_or("closed")
                .to_string()
        } else {
            "closed".to_string()
        }
    }

    pub async fn record_failure(&self, component: &str) {
        if let Some(breaker) = &self.circuit_breaker {
            let mut failures = breaker.failures.write().await;
            let component_failures = failures
                .entry(component.to_string())
                .or_insert_with(Vec::new);
            component_failures.push(Instant::now());

            // Check if we should open circuit
            if component_failures.len() >= breaker.failure_threshold {
                let mut states = breaker.state.write().await;
                states.insert(component.to_string(), CircuitState::Open);

                // Schedule half-open transition
                let state_clone = breaker.state.clone();
                let component_clone = component.to_string();
                let timeout = breaker.reset_timeout;
                tokio::spawn(async move {
                    tokio::time::sleep(timeout).await;
                    state_clone
                        .write()
                        .await
                        .insert(component_clone, CircuitState::HalfOpen);
                });
            }
        }
    }

    pub async fn record_success(&self, component: &str) {
        if let Some(breaker) = &self.circuit_breaker {
            let mut states = breaker.state.write().await;
            let current_state = states
                .get(component)
                .cloned()
                .unwrap_or(CircuitState::Closed);

            if current_state == CircuitState::HalfOpen {
                states.insert(component.to_string(), CircuitState::Closed);
                breaker.failures.write().await.remove(component);
            }
        }
    }

    pub async fn allow_request(&self, component: &str) -> bool {
        if let Some(breaker) = &self.circuit_breaker {
            let states = breaker.state.read().await;
            match states.get(component) {
                Some(CircuitState::Open) => false,
                _ => true,
            }
        } else {
            true
        }
    }

    pub async fn record_request_success(&self) {
        let mut stats = self.request_stats.write().await;
        stats.total += 1;
        stats.successful += 1;
    }

    pub async fn record_request_failure(&self) {
        let mut stats = self.request_stats.write().await;
        stats.total += 1;
        stats.failed += 1;
    }

    pub async fn get_health_metrics(&self) -> HealthMetrics {
        let stats = self.request_stats.read().await;
        let success_rate = if stats.total > 0 {
            stats.successful as f64 / stats.total as f64
        } else {
            1.0
        };

        HealthMetrics {
            total_requests: stats.total,
            successful_requests: stats.successful,
            failed_requests: stats.failed,
            success_rate,
            uptime_seconds: self.start_time.elapsed().as_secs(),
        }
    }

    pub async fn check_dependencies(&self) -> HashMap<String, DependencyStatus> {
        let mut deps = HashMap::new();

        // Mock dependency checks
        deps.insert("s5_storage".to_string(), DependencyStatus::Healthy);
        deps.insert("vector_db".to_string(), DependencyStatus::Healthy);
        deps.insert("blockchain".to_string(), DependencyStatus::Healthy);

        deps
    }
}

#[derive(Debug, Serialize)]
pub enum DependencyStatus {
    Healthy,
    Degraded,
    Unhealthy,
}
