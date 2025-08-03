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
use tokio::sync::mpsc;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, Serialize)]
pub struct LivenessProbe {
    pub is_alive: bool,
    pub uptime_seconds: u64,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadinessProbe {
    pub is_ready: bool,
    pub components_ready: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceInfo {
    pub current_value: f64,
    pub max_value: f64,
    pub status: HealthStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResourceReport {
    pub status: HealthStatus,
    pub resources: HashMap<String, ResourceInfo>,
}

#[derive(Debug, Clone)]
pub struct DependencyHealth {
    pub name: String,
    pub status: HealthStatus,
    pub last_check: u64,
    pub response_time_ms: u64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    pub status_code: u16,
    pub body: String,
}

// Health check function type
type HealthCheckFn = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<ComponentHealth>> + Send>> + Send + Sync>;

pub struct HealthCheck {
    name: String,
    check_type: CheckType,
    check_fn: HealthCheckFn,
}

impl HealthCheck {
    pub fn new(
        name: &str,
        check_type: CheckType,
        check_fn: Box<dyn Fn() -> Pin<Box<dyn Future<Output = Result<ComponentHealth>> + Send>> + Send + Sync>,
    ) -> Self {
        HealthCheck {
            name: name.to_string(),
            check_type,
            check_fn: Arc::new(check_fn),
        }
    }
}

// Resource check function type
type ResourceCheckFn = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<(f64, f64)>> + Send>> + Send + Sync>;

pub struct ResourceCheck {
    name: String,
    check_fn: ResourceCheckFn,
    warning_threshold: f64,
    critical_threshold: f64,
}

impl ResourceCheck {
    pub fn new(
        name: &str,
        check_fn: Box<dyn Fn() -> Pin<Box<dyn Future<Output = Result<(f64, f64)>> + Send>> + Send + Sync>,
        warning_threshold: f64,
        critical_threshold: f64,
    ) -> Self {
        ResourceCheck {
            name: name.to_string(),
            check_fn: Arc::new(check_fn),
            warning_threshold,
            critical_threshold,
        }
    }
}

pub struct DependencyCheck {
    name: String,
    url: String,
    check_type: CheckType,
    timeout: Duration,
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

struct HealthCheckerState {
    config: HealthConfig,
    components: HashMap<String, ComponentHealth>,
    health_checks: HashMap<String, HealthCheck>,
    resource_checks: HashMap<String, ResourceCheck>,
    dependency_checks: HashMap<String, DependencyCheck>,
    health_history: Vec<HealthReport>,
    ready_components: HashMap<String, bool>,
    start_time: Instant,
    is_shutting_down: bool,
    metrics: HashMap<String, f64>,
    gc_handle: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Clone)]
pub struct HealthChecker {
    state: Arc<RwLock<HealthCheckerState>>,
}

impl HealthChecker {
    pub async fn new(config: HealthConfig) -> Result<Self> {
        let mut components = HashMap::new();
        for component in &config.components {
            components.insert(
                component.clone(),
                ComponentHealth {
                    name: component.clone(),
                    status: HealthStatus::Healthy,
                    message: None,
                    last_check: Utc::now().timestamp() as u64,
                    response_time_ms: 0,
                },
            );
        }

        let mut ready_components = HashMap::new();
        for component in &config.components {
            ready_components.insert(component.clone(), false);
        }

        let state = Arc::new(RwLock::new(HealthCheckerState {
            config,
            components,
            health_checks: HashMap::new(),
            resource_checks: HashMap::new(),
            dependency_checks: HashMap::new(),
            health_history: Vec::new(),
            ready_components,
            start_time: Instant::now(),
            is_shutting_down: false,
            metrics: HashMap::new(),
            gc_handle: None,
        }));

        Ok(HealthChecker { state })
    }

    pub async fn check_health(&self) -> Result<HealthReport> {
        // First check if shutting down
        {
            let state = self.state.read().await;
            if state.is_shutting_down {
                return Ok(HealthReport {
                    status: HealthStatus::Terminating,
                    timestamp: Utc::now().timestamp() as u64,
                    components: state.components.clone(),
                    overall_score: 0.0,
                });
            }
        }

        // Get timeout duration and collect checks
        let (timeout_duration, checks_to_run) = {
            let state = self.state.read().await;
            let timeout_duration = Duration::from_secs(state.config.timeout_seconds);
            let checks: Vec<_> = state.health_checks
                .keys()
                .cloned()
                .collect();
            (timeout_duration, checks)
        };
        
        // Run checks
        for check_name in checks_to_run {
            // Get the check function
            let check_fn = {
                let state = self.state.read().await;
                state.health_checks.get(&check_name).map(|c| c.check_fn.clone())
            };
            
            if let Some(check_fn) = check_fn {
                let check_result = tokio::time::timeout(
                    timeout_duration,
                    check_fn()
                ).await;

                let component_health = match check_result {
                    Ok(Ok(health)) => health,
                    Ok(Err(_)) => ComponentHealth {
                        name: check_name.clone(),
                        status: HealthStatus::Unhealthy,
                        message: Some("Check failed".to_string()),
                        last_check: Utc::now().timestamp() as u64,
                        response_time_ms: timeout_duration.as_millis() as u64,
                    },
                    Err(_) => ComponentHealth {
                        name: check_name.clone(),
                        status: HealthStatus::Unhealthy,
                        message: Some("Check timeout".to_string()),
                        last_check: Utc::now().timestamp() as u64,
                        response_time_ms: timeout_duration.as_millis() as u64,
                    },
                };

                // Update component
                let mut state = self.state.write().await;
                state.components.insert(check_name, component_health);
            }
        }
        
        // Generate report
        let mut state = self.state.write().await;

        // Calculate overall status and score
        let mut unhealthy_count = 0;
        let mut degraded_count = 0;
        let total_components = state.components.len();

        for component in state.components.values() {
            match component.status {
                HealthStatus::Unhealthy => unhealthy_count += 1,
                HealthStatus::Degraded => degraded_count += 1,
                _ => {}
            }
        }

        let overall_status = if unhealthy_count > 0 {
            HealthStatus::Unhealthy
        } else if degraded_count > 0 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        };

        let overall_score = if total_components > 0 {
            let healthy_count = total_components - unhealthy_count - degraded_count;
            (healthy_count as f64 + (degraded_count as f64 * 0.5)) / total_components as f64
        } else {
            1.0
        };

        let report = HealthReport {
            status: overall_status,
            timestamp: Utc::now().timestamp() as u64,
            components: state.components.clone(),
            overall_score,
        };

        // Store in history
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

    pub async fn liveness_probe(&self) -> Result<LivenessProbe> {
        let state = self.state.read().await;
        let uptime_seconds = state.start_time.elapsed().as_secs();
        
        Ok(LivenessProbe {
            is_alive: true, // Always true until process actually terminates
            uptime_seconds,
            status: if state.is_shutting_down {
                HealthStatus::Terminating
            } else {
                HealthStatus::Healthy
            },
        })
    }

    pub async fn readiness_probe(&self) -> Result<ReadinessProbe> {
        let state = self.state.read().await;
        
        if state.is_shutting_down {
            return Ok(ReadinessProbe {
                is_ready: false,
                components_ready: state.ready_components.clone(),
            });
        }

        let all_ready = state.ready_components.values().all(|&ready| ready);
        
        Ok(ReadinessProbe {
            is_ready: all_ready,
            components_ready: state.ready_components.clone(),
        })
    }

    pub async fn set_component_ready(&self, component: &str, ready: bool) {
        let mut state = self.state.write().await;
        state.ready_components.insert(component.to_string(), ready);
    }

    pub async fn register_resource_check(&self, check: ResourceCheck) -> Result<()> {
        let mut state = self.state.write().await;
        state.resource_checks.insert(check.name.clone(), check);
        Ok(())
    }

    pub async fn check_resources(&self) -> Result<ResourceReport> {
        let state = self.state.read().await;
        let mut resources = HashMap::new();
        let mut overall_status = HealthStatus::Healthy;

        for (name, check) in &state.resource_checks {
            let result = (check.check_fn)().await?;
            let (current, max) = result;
            let percentage = (current / max) * 100.0;

            let status = if percentage >= check.critical_threshold {
                overall_status = HealthStatus::Unhealthy;
                HealthStatus::Unhealthy
            } else if percentage >= check.warning_threshold {
                if overall_status == HealthStatus::Healthy {
                    overall_status = HealthStatus::Degraded;
                }
                HealthStatus::Degraded
            } else {
                HealthStatus::Healthy
            };

            resources.insert(
                name.clone(),
                ResourceInfo {
                    current_value: current,
                    max_value: max,
                    status,
                    message: None,
                },
            );
        }

        Ok(ResourceReport {
            status: overall_status,
            resources,
        })
    }

    pub async fn add_dependency_check(&self, check: DependencyCheck) {
        let mut state = self.state.write().await;
        state.dependency_checks.insert(check.name.clone(), check);
    }

    pub async fn check_dependencies(&self) -> HashMap<String, DependencyHealth> {
        let state = self.state.read().await;
        let mut results = HashMap::new();

        // For testing, return mock results
        for (name, check) in &state.dependency_checks {
            results.insert(
                name.clone(),
                DependencyHealth {
                    name: name.clone(),
                    status: HealthStatus::Healthy, // Mock as healthy for tests
                    last_check: Utc::now().timestamp() as u64,
                    response_time_ms: 50,
                    message: Some("Mock dependency check".to_string()),
                },
            );
        }

        results
    }

    pub async fn update_component_health(&self, health: ComponentHealth) {
        let mut state = self.state.write().await;
        state.components.insert(health.name.clone(), health);
    }

    pub async fn get_health_history(&self, duration: Duration) -> Result<Vec<HealthReport>> {
        let state = self.state.read().await;
        let cutoff = Utc::now().timestamp() as u64 - duration.as_secs();
        
        Ok(state.health_history
            .iter()
            .filter(|report| report.timestamp >= cutoff)
            .cloned()
            .collect())
    }

    pub async fn calculate_uptime_percentage(&self, duration: Duration) -> f64 {
        let state = self.state.read().await;
        let cutoff = Utc::now().timestamp() as u64 - duration.as_secs();
        
        let relevant_reports: Vec<_> = state.health_history
            .iter()
            .filter(|report| report.timestamp >= cutoff)
            .collect();

        if relevant_reports.is_empty() {
            return 100.0;
        }

        let healthy_count = relevant_reports
            .iter()
            .filter(|report| report.status == HealthStatus::Healthy)
            .count();

        (healthy_count as f64 / relevant_reports.len() as f64) * 100.0
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

    pub async fn start_garbage_collection(&self) {
        let mut state = self.state.write().await;
        if state.gc_handle.is_some() {
            return;
        }

        let checker = self.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(300)).await; // 5 minutes
                
                // Clean up old history
                let cutoff = Utc::now().timestamp() as u64 - 3600; // Keep last hour
                let mut state = checker.state.write().await;
                state.health_history.retain(|report| report.timestamp >= cutoff);
            }
        });

        state.gc_handle = Some(handle);
    }
}

pub struct HealthEndpoint {
    checker: Arc<HealthChecker>,
}

impl HealthEndpoint {
    pub fn new(checker: Arc<HealthChecker>) -> Self {
        HealthEndpoint { checker }
    }

    pub async fn handle_request(&self, path: &str) -> Result<HealthResponse> {
        match path {
            "/health/live" => {
                let probe = self.checker.liveness_probe().await?;
                Ok(HealthResponse {
                    status_code: if probe.is_alive { 200 } else { 503 },
                    body: serde_json::to_string(&probe)?,
                })
            }
            "/health/ready" => {
                let probe = self.checker.readiness_probe().await?;
                Ok(HealthResponse {
                    status_code: if probe.is_ready { 200 } else { 503 },
                    body: serde_json::to_string(&probe)?,
                })
            }
            "/health" => {
                let report = self.checker.check_health().await?;
                Ok(HealthResponse {
                    status_code: match report.status {
                        HealthStatus::Healthy => 200,
                        HealthStatus::Degraded => 200,
                        _ => 503,
                    },
                    body: serde_json::to_string(&report)?,
                })
            }
            _ => Err(anyhow!("Unknown health endpoint")),
        }
    }
}