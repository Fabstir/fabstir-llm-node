// src/monitoring/health_checks.rs - Health monitoring and checks

use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use std::time::{Duration, Instant};
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthConfig {
    pub enable_health_checks: bool,
    pub check_interval_seconds: u64,
    pub timeout_seconds: u64,
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub components: Vec<String>,
    pub resource_thresholds: ThresholdConfig,
}

impl Default for HealthConfig {
    fn default() -> Self {
        HealthConfig {
            enable_health_checks: true,
            check_interval_seconds: 30,
            timeout_seconds: 10,
            failure_threshold: 3,
            success_threshold: 2,
            components: vec![],
            resource_thresholds: ThresholdConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub disk_percent: f64,
    pub gpu_memory_percent: f64,
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        ThresholdConfig {
            cpu_percent: 90.0,
            memory_percent: 85.0,
            disk_percent: 95.0,
            gpu_memory_percent: 90.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Terminating,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckType {
    Liveness,
    Readiness,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub name: String,
    pub status: HealthStatus,
    pub message: Option<String>,
    pub last_check: u64,
    pub response_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub status: HealthStatus,
    pub timestamp: u64,
    pub components: HashMap<String, ComponentHealth>,
    pub overall_score: f64,
    pub resources: HashMap<String, ResourceStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceStatus {
    pub name: String,
    pub current_value: f64,
    pub max_value: f64,
    pub status: HealthStatus,
    pub threshold_warning: f64,
    pub threshold_critical: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivenessResult {
    pub is_alive: bool,
    pub uptime_seconds: u64,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessResult {
    pub is_ready: bool,
    pub components_ready: HashMap<String, bool>,
    pub dependencies_ready: HashMap<String, bool>,
}

type CheckFunction = Box<dyn Fn() -> Pin<Box<dyn Future<Output = Result<ComponentHealth>> + Send>> + Send + Sync>;
type ResourceCheckFunction = Box<dyn Fn() -> Pin<Box<dyn Future<Output = Result<(f64, f64)>> + Send>> + Send + Sync>;

pub struct HealthCheck {
    pub name: String,
    pub check_type: CheckType,
    pub check_fn: CheckFunction,
}

impl HealthCheck {
    pub fn new(name: &str, check_type: CheckType, check_fn: CheckFunction) -> Self {
        HealthCheck {
            name: name.to_string(),
            check_type,
            check_fn,
        }
    }
}

pub struct ResourceCheck {
    pub name: String,
    pub check_fn: ResourceCheckFunction,
    pub warning_threshold: f64,
    pub critical_threshold: f64,
}

impl ResourceCheck {
    pub fn new(
        name: &str,
        check_fn: ResourceCheckFunction,
        warning_threshold: f64,
        critical_threshold: f64,
    ) -> Self {
        ResourceCheck {
            name: name.to_string(),
            check_fn,
            warning_threshold,
            critical_threshold,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyCheck {
    pub name: String,
    pub url: String,
    pub check_type: CheckType,
    pub timeout: Duration,
}

impl DependencyCheck {
    pub fn new(name: &str, url: &str, check_type: CheckType, timeout: Duration) -> Self {
        DependencyCheck {
            name: name.to_string(),
            url: url.to_string(),
            check_type,
            timeout,
        }
    }
}

pub struct HealthEndpoint {
    checker: Arc<HealthChecker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointResponse {
    pub status_code: u16,
    pub body: String,
}

impl HealthEndpoint {
    pub fn new(checker: Arc<HealthChecker>) -> Self {
        HealthEndpoint { checker }
    }

    pub async fn handle_request(&self, path: &str) -> Result<EndpointResponse> {
        match path {
            "/health/live" => {
                let liveness = self.checker.liveness_probe().await?;
                let status_code = if liveness.is_alive { 200 } else { 503 };
                Ok(EndpointResponse {
                    status_code,
                    body: serde_json::to_string(&liveness)?,
                })
            }
            "/health/ready" => {
                let readiness = self.checker.readiness_probe().await?;
                let status_code = if readiness.is_ready { 200 } else { 503 };
                Ok(EndpointResponse {
                    status_code,
                    body: serde_json::to_string(&readiness)?,
                })
            }
            "/health" => {
                let report = self.checker.check_health().await?;
                let status_code = match report.status {
                    HealthStatus::Healthy => 200,
                    HealthStatus::Degraded => 200,
                    _ => 503,
                };
                Ok(EndpointResponse {
                    status_code,
                    body: serde_json::to_string(&report)?,
                })
            }
            _ => Err(anyhow!("Unknown health endpoint: {}", path)),
        }
    }
}

pub struct HealthChecker {
    config: HealthConfig,
    state: Arc<RwLock<HealthState>>,
    start_time: Instant,
}

struct HealthState {
    component_health: HashMap<String, ComponentHealth>,
    health_checks: HashMap<String, HealthCheck>,
    resource_checks: HashMap<String, ResourceCheck>,
    dependency_checks: HashMap<String, DependencyCheck>,
    health_history: Vec<HealthReport>,
    component_ready: HashMap<String, bool>,
    metrics: HashMap<String, f64>,
    is_shutting_down: bool,
}

impl HealthChecker {
    pub async fn new(config: HealthConfig) -> Result<Self> {
        let mut component_ready = HashMap::new();
        for component in &config.components {
            component_ready.insert(component.clone(), false);
        }

        let state = Arc::new(RwLock::new(HealthState {
            component_health: HashMap::new(),
            health_checks: HashMap::new(),
            resource_checks: HashMap::new(),
            dependency_checks: HashMap::new(),
            health_history: Vec::new(),
            component_ready,
            metrics: HashMap::new(),
            is_shutting_down: false,
        }));

        Ok(HealthChecker {
            config,
            state,
            start_time: Instant::now(),
        })
    }

    pub async fn check_health(&self) -> Result<HealthReport> {
        let mut components = HashMap::new();
        let mut resources = HashMap::new();
        let mut overall_score = 1.0;
        let mut worst_status = HealthStatus::Healthy;

        // Check if shutting down
        {
            let state = self.state.read().await;
            if state.is_shutting_down {
                return Ok(HealthReport {
                    status: HealthStatus::Terminating,
                    timestamp: Utc::now().timestamp() as u64,
                    components,
                    overall_score: 0.0,
                    resources,
                });
            }
        }

        // Run all health checks with timeout
        let check_names: Vec<String> = {
            let state = self.state.read().await;
            state.health_checks.keys().cloned().collect()
        };

        for check_name in check_names {
            let state = self.state.read().await;
            let check = match state.health_checks.get(&check_name) {
                Some(c) => c,
                None => continue,
            };
            let check_future = (check.check_fn)();
            let check_name_clone = check.name.clone();
            drop(state);
            let result = tokio::time::timeout(
                Duration::from_secs(self.config.timeout_seconds),
                check_future,
            )
            .await;

            let component_health = match result {
                Ok(Ok(health)) => health,
                Ok(Err(e)) => ComponentHealth {
                    name: check_name_clone.clone(),
                    status: HealthStatus::Unhealthy,
                    message: Some(format!("Check failed: {}", e)),
                    last_check: Utc::now().timestamp() as u64,
                    response_time_ms: 0,
                },
                Err(_) => ComponentHealth {
                    name: check_name_clone.clone(),
                    status: HealthStatus::Unhealthy,
                    message: Some("Health check timeout".to_string()),
                    last_check: Utc::now().timestamp() as u64,
                    response_time_ms: self.config.timeout_seconds * 1000,
                },
            };

            // Update worst status
            match component_health.status {
                HealthStatus::Unhealthy => {
                    worst_status = HealthStatus::Unhealthy;
                    overall_score *= 0.0;
                }
                HealthStatus::Degraded => {
                    if worst_status != HealthStatus::Unhealthy {
                        worst_status = HealthStatus::Degraded;
                    }
                    overall_score *= 0.5;
                }
                _ => {}
            }

            components.insert(check_name_clone.clone(), component_health.clone());

            // Update component health in state
            let mut state = self.state.write().await;
            state.component_health.insert(check_name_clone, component_health);
        }

        // Include existing component health
        let state = self.state.read().await;
        for (name, health) in &state.component_health {
            if !components.contains_key(name) {
                components.insert(name.clone(), health.clone());
                
                // Update worst status
                match health.status {
                    HealthStatus::Unhealthy => {
                        worst_status = HealthStatus::Unhealthy;
                        overall_score *= 0.0;
                    }
                    HealthStatus::Degraded => {
                        if worst_status != HealthStatus::Unhealthy {
                            worst_status = HealthStatus::Degraded;
                        }
                        overall_score *= 0.5;
                    }
                    _ => {}
                }
            }
        }

        // Run resource checks
        let resource_check_names: Vec<String> = {
            let state = self.state.read().await;
            state.resource_checks.keys().cloned().collect()
        };

        for name in resource_check_names {
            let (warning, critical, check_result) = {
                let state = self.state.read().await;
                let resource_check = match state.resource_checks.get(&name) {
                    Some(rc) => rc,
                    None => continue,
                };
                let warning = resource_check.warning_threshold;
                let critical = resource_check.critical_threshold;
                let result = (resource_check.check_fn)().await;
                (warning, critical, result)
            };
            
            if let Ok((current, max)) = check_result {
                let percentage = (current / max) * 100.0;
                let status = if percentage >= critical {
                    HealthStatus::Unhealthy
                } else if percentage >= warning {
                    HealthStatus::Degraded
                } else {
                    HealthStatus::Healthy
                };

                resources.insert(name.clone(), ResourceStatus {
                    name: name.clone(),
                    current_value: current,
                    max_value: max,
                    status,
                    threshold_warning: warning,
                    threshold_critical: critical,
                });
            }
        }

        let report = HealthReport {
            status: worst_status,
            timestamp: Utc::now().timestamp() as u64,
            components,
            overall_score,
            resources,
        };

        // Store in history
        let mut state = self.state.write().await;
        state.health_history.push(report.clone());
        
        // Keep only recent history (last 1000 entries)
        if state.health_history.len() > 1000 {
            state.health_history.remove(0);
        }

        Ok(report)
    }

    pub async fn register_check(&self, check: HealthCheck) -> Result<()> {
        let mut state = self.state.write().await;
        state.health_checks.insert(check.name.clone(), check);
        Ok(())
    }

    pub async fn liveness_probe(&self) -> Result<LivenessResult> {
        let state = self.state.read().await;
        
        let status = if state.is_shutting_down {
            HealthStatus::Terminating
        } else {
            HealthStatus::Healthy
        };

        Ok(LivenessResult {
            is_alive: !state.is_shutting_down,
            uptime_seconds: self.start_time.elapsed().as_secs(),
            status,
        })
    }

    pub async fn readiness_probe(&self) -> Result<ReadinessResult> {
        let state = self.state.read().await;
        
        if state.is_shutting_down {
            return Ok(ReadinessResult {
                is_ready: false,
                components_ready: state.component_ready.clone(),
                dependencies_ready: HashMap::new(),
            });
        }

        let all_ready = state.component_ready.values().all(|&ready| ready);
        
        // Check dependencies
        let mut dependencies_ready = HashMap::new();
        for (name, _dep) in &state.dependency_checks {
            // Mock dependency check - in real implementation would make HTTP request
            dependencies_ready.insert(name.clone(), true);
        }

        Ok(ReadinessResult {
            is_ready: all_ready,
            components_ready: state.component_ready.clone(),
            dependencies_ready,
        })
    }

    pub async fn set_component_ready(&self, component: &str, ready: bool) {
        let mut state = self.state.write().await;
        state.component_ready.insert(component.to_string(), ready);
    }

    pub async fn register_resource_check(&self, check: ResourceCheck) -> Result<()> {
        let mut state = self.state.write().await;
        state.resource_checks.insert(check.name.clone(), check);
        Ok(())
    }

    pub async fn check_resources(&self) -> Result<HealthReport> {
        // Run check_health which includes resource checks
        self.check_health().await
    }

    pub async fn add_dependency_check(&self, check: DependencyCheck) {
        let mut state = self.state.write().await;
        state.dependency_checks.insert(check.name.clone(), check);
    }

    pub async fn check_dependencies(&self) -> HashMap<String, ComponentHealth> {
        let state = self.state.read().await;
        let mut results = HashMap::new();

        for (name, _dep) in &state.dependency_checks {
            // Mock dependency check
            results.insert(name.clone(), ComponentHealth {
                name: name.clone(),
                status: HealthStatus::Healthy,
                message: Some(format!("Checking {}", _dep.url)),
                last_check: Utc::now().timestamp() as u64,
                response_time_ms: 50,
            });
        }

        results
    }

    pub async fn update_component_health(&self, health: ComponentHealth) {
        let mut state = self.state.write().await;
        state.component_health.insert(health.name.clone(), health);
    }

    pub async fn get_health_history(&self, duration: Duration) -> Result<Vec<HealthReport>> {
        let state = self.state.read().await;
        let cutoff = Utc::now().timestamp() as u64 - duration.as_secs();
        
        let history: Vec<HealthReport> = state.health_history.iter()
            .filter(|report| report.timestamp >= cutoff)
            .cloned()
            .collect();

        Ok(history)
    }

    pub async fn calculate_uptime_percentage(&self, duration: Duration) -> f64 {
        let history = self.get_health_history(duration).await.unwrap_or_default();
        
        if history.is_empty() {
            return 100.0;
        }

        let healthy_count = history.iter()
            .filter(|report| report.status == HealthStatus::Healthy)
            .count();

        (healthy_count as f64 / history.len() as f64) * 100.0
    }

    pub async fn begin_shutdown(&self) {
        let mut state = self.state.write().await;
        state.is_shutting_down = true;
    }

    pub async fn record_metric(&self, name: &str, value: f64) {
        let mut state = self.state.write().await;
        state.metrics.insert(name.to_string(), value);
    }

    pub async fn get_metric(&self, name: &str) -> Option<f64> {
        let state = self.state.read().await;
        state.metrics.get(name).copied()
    }
}

impl Clone for HealthChecker {
    fn clone(&self) -> Self {
        HealthChecker {
            config: self.config.clone(),
            state: self.state.clone(),
            start_time: self.start_time,
        }
    }
}

// Re-export types that might be needed but not explicitly defined
pub type HealthHistory = Vec<HealthReport>;
pub type ResourceType = String;
pub type HealthError = anyhow::Error;
pub type LivenessProbe = LivenessResult;
pub type ReadinessProbe = ReadinessResult;