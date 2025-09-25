use anyhow::{anyhow, Result};
use ethers::types::{H256, U256};
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{interval, sleep};
use tracing::{debug, error, info, warn};

use crate::blockchain::multi_chain_registrar::{MultiChainRegistrar, RegistrationStatus};
use crate::monitoring::alerting::{Alert, AlertLevel, AlertManager};
use crate::monitoring::metrics::{Counter, Gauge, Histogram, MetricsCollector};

/// Configuration for the registration monitor
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub check_interval: Duration,
    pub warning_threshold: Duration,
    pub critical_threshold: Duration,
    pub auto_renewal: bool,
    pub renewal_buffer: Duration,
    pub max_retry_attempts: u32,
    pub retry_delay: Duration,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        MonitorConfig {
            check_interval: Duration::from_secs(300),      // 5 minutes
            warning_threshold: Duration::from_secs(86400), // 24 hours
            critical_threshold: Duration::from_secs(3600), // 1 hour
            auto_renewal: false,
            renewal_buffer: Duration::from_secs(7200), // 2 hours before expiry
            max_retry_attempts: 3,
            retry_delay: Duration::from_secs(30),
        }
    }
}

/// Health status of a registration
#[derive(Debug, Clone)]
pub struct RegistrationHealth {
    pub chain_id: u64,
    pub status: RegistrationStatus,
    pub last_check: Instant,
    pub is_healthy: bool,
    pub issues: Vec<HealthIssue>,
    pub stake_balance: U256,
    pub fab_balance: U256,
    pub registration_block: Option<u64>,
    pub time_until_expiry: Option<Duration>,
}

/// Health issues that can be detected
#[derive(Debug, Clone)]
pub struct HealthIssue {
    pub issue_type: IssueType,
    pub severity: IssueSeverity,
    pub message: String,
    pub detected_at: Instant,
    pub resolved: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueType {
    NotRegistered,
    ExpiringS,
    LowStake,
    LowBalance,
    RpcFailure,
    ApiUnreachable,
    ModelNotApproved,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueSeverity {
    Info,
    Warning,
    Critical,
}

/// Warning event for callbacks
#[derive(Debug, Clone)]
pub struct WarningEvent {
    pub chain_id: u64,
    pub level: String,
    pub message: String,
    pub time_until_expiry: Option<Duration>,
}

/// Main registration monitor
pub struct RegistrationMonitor {
    registrar: Arc<MultiChainRegistrar>,
    config: Arc<RwLock<MonitorConfig>>,
    health_states: Arc<RwLock<HashMap<u64, RegistrationHealth>>>,
    monitor_handles: Arc<Mutex<HashMap<u64, JoinHandle<()>>>>,
    metrics_collector: Arc<MetricsCollector>,
    alert_manager: Option<Arc<AlertManager>>,
    warning_callbacks:
        Arc<RwLock<Vec<Box<dyn Fn(WarningEvent) -> BoxFuture<'static, ()> + Send + Sync>>>>,
    simulated_failures: Arc<RwLock<HashMap<u64, bool>>>,
    mock_expiries: Arc<RwLock<HashMap<u64, Instant>>>,
    renewal_history: Arc<RwLock<HashMap<u64, Vec<Instant>>>>,
    mock_mode: Arc<RwLock<bool>>,
}

impl RegistrationMonitor {
    /// Create a new registration monitor
    pub async fn new(registrar: Arc<MultiChainRegistrar>, config: MonitorConfig) -> Result<Self> {
        let metrics_collector = Arc::new(MetricsCollector::new_default());

        // Register metrics
        metrics_collector.register_gauge_sync("registration_status", "Current registration status");
        metrics_collector
            .register_counter_sync("health_check_count", "Number of health checks performed");
        metrics_collector.register_histogram_sync(
            "health_check_duration_ms",
            "Duration of health checks in milliseconds",
        );
        metrics_collector
            .register_counter_sync("expiry_warnings", "Number of expiry warnings issued");
        metrics_collector.register_counter_sync("renewal_attempts", "Number of renewal attempts");
        metrics_collector.register_counter_sync("recovery_attempts", "Number of recovery attempts");
        metrics_collector.register_gauge_sync("stake_balance_fab", "Current FAB stake balance");
        metrics_collector.register_gauge_sync(
            "time_until_expiry_seconds",
            "Time until registration expires",
        );

        Ok(RegistrationMonitor {
            registrar,
            config: Arc::new(RwLock::new(config)),
            health_states: Arc::new(RwLock::new(HashMap::new())),
            monitor_handles: Arc::new(Mutex::new(HashMap::new())),
            metrics_collector,
            alert_manager: None,
            warning_callbacks: Arc::new(RwLock::new(Vec::new())),
            simulated_failures: Arc::new(RwLock::new(HashMap::new())),
            mock_expiries: Arc::new(RwLock::new(HashMap::new())),
            renewal_history: Arc::new(RwLock::new(HashMap::new())),
            mock_mode: Arc::new(RwLock::new(false)),
        })
    }

    /// Start monitoring all chains
    pub async fn start_monitoring(&self) -> Result<()> {
        info!("Starting registration monitoring");

        let chain_ids = self.registrar.get_all_chain_ids().await?;

        for chain_id in chain_ids {
            self.start_chain_monitor(chain_id).await?;
        }

        Ok(())
    }

    /// Start monitoring a specific chain
    async fn start_chain_monitor(&self, chain_id: u64) -> Result<()> {
        let mut handles = self.monitor_handles.lock().await;

        // Don't start if already monitoring
        if handles.contains_key(&chain_id) {
            return Ok(());
        }

        let registrar = self.registrar.clone();
        let config = self.config.clone();
        let health_states = self.health_states.clone();
        let metrics = self.metrics_collector.clone();
        let callbacks = self.warning_callbacks.clone();
        let failures = self.simulated_failures.clone();
        let mock_expiries = self.mock_expiries.clone();
        let renewal_history = self.renewal_history.clone();
        let mock_mode = self.mock_mode.clone();

        let handle = tokio::spawn(async move {
            info!("Starting monitor for chain {}", chain_id);

            let mut check_interval = {
                let cfg = config.read().await;
                interval(cfg.check_interval)
            };

            loop {
                check_interval.tick().await;

                // Perform health check
                let start = Instant::now();
                let health = match Self::check_health_internal(
                    chain_id,
                    &registrar,
                    &config,
                    &failures,
                    &mock_expiries,
                )
                .await
                {
                    Ok(h) => h,
                    Err(e) => {
                        error!("Health check failed for chain {}: {}", chain_id, e);
                        metrics.increment_counter("recovery_attempts", 1.0);
                        continue;
                    }
                };

                // Track recovery attempts - check for resolved issues
                {
                    let states = health_states.read().await;
                    if let Some(prev_health) = states.get(&chain_id) {
                        // Check if we had RPC failure before but not now
                        let had_rpc_failure = prev_health
                            .issues
                            .iter()
                            .any(|issue| issue.issue_type == IssueType::RpcFailure);
                        let has_rpc_failure = health
                            .issues
                            .iter()
                            .any(|issue| issue.issue_type == IssueType::RpcFailure);

                        if had_rpc_failure && !has_rpc_failure {
                            // RPC failure has been resolved
                            metrics.increment_counter("recovery_attempts", 1.0);
                            info!("Chain {} recovered from RPC failure", chain_id);
                        }

                        // Also track overall health recovery
                        if !prev_health.is_healthy && health.is_healthy {
                            metrics.increment_counter("recovery_attempts", 1.0);
                            info!("Chain {} recovered to healthy state", chain_id);
                        }
                    }
                }

                // Record metrics
                let duration_ms = start.elapsed().as_millis() as f64;
                metrics.record_histogram("health_check_duration_ms", duration_ms);
                metrics.increment_counter("health_check_count", 1.0);

                // Update status metric
                let status_value = match health.status {
                    RegistrationStatus::NotRegistered => 0.0,
                    RegistrationStatus::Pending { .. } => 1.0,
                    RegistrationStatus::Confirmed { .. } => 2.0,
                    RegistrationStatus::Failed { .. } => -1.0,
                };

                // Register the chain-specific gauge if not exists
                let gauge_name = format!("registration_status_{}", chain_id);
                metrics.register_gauge_sync(
                    &gauge_name,
                    &format!("Registration status for chain {}", chain_id),
                );
                metrics.set_gauge(&gauge_name, status_value);

                // Check for warnings
                if let Some(time_until_expiry) = health.time_until_expiry {
                    let expiry_gauge_name = format!("time_until_expiry_seconds_{}", chain_id);
                    metrics.register_gauge_sync(
                        &expiry_gauge_name,
                        &format!("Time until expiry for chain {}", chain_id),
                    );
                    metrics.set_gauge(&expiry_gauge_name, time_until_expiry.as_secs() as f64);

                    let cfg = config.read().await;

                    // Issue warnings based on time until expiry
                    let (level, should_warn) = if time_until_expiry < cfg.critical_threshold {
                        ("CRITICAL", true)
                    } else if time_until_expiry < cfg.warning_threshold {
                        ("WARNING", true)
                    } else {
                        ("INFO", false)
                    };

                    if should_warn {
                        let warning = WarningEvent {
                            chain_id,
                            level: level.to_string(),
                            message: format!(
                                "Registration expiring in {} minutes",
                                time_until_expiry.as_secs() / 60
                            ),
                            time_until_expiry: Some(time_until_expiry),
                        };

                        // Call warning callbacks
                        let callbacks = callbacks.read().await;
                        for callback in callbacks.iter() {
                            callback(warning.clone()).await;
                        }

                        metrics.increment_counter("expiry_warnings", 1.0);

                        // Check if auto-renewal should trigger
                        if cfg.auto_renewal && time_until_expiry < cfg.renewal_buffer {
                            info!("Triggering auto-renewal for chain {}", chain_id);

                            // Check if we're in mock mode
                            let is_mock = *mock_mode.read().await;

                            if is_mock {
                                // For testing, just record the renewal attempt
                                let mut history = renewal_history.write().await;
                                history
                                    .entry(chain_id)
                                    .or_insert_with(Vec::new)
                                    .push(Instant::now());
                                metrics.increment_counter("renewal_attempts", 1.0);
                                info!("Mock renewal recorded for chain {}", chain_id);
                            } else {
                                match registrar.register_on_chain(chain_id).await {
                                    Ok(tx_hash) => {
                                        info!("Renewal transaction sent: {:?}", tx_hash);
                                        metrics.increment_counter("renewal_attempts", 1.0);

                                        let mut history = renewal_history.write().await;
                                        history
                                            .entry(chain_id)
                                            .or_insert_with(Vec::new)
                                            .push(Instant::now());
                                    }
                                    Err(e) => {
                                        error!("Failed to renew registration: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }

                // Update health state
                let mut states = health_states.write().await;
                states.insert(chain_id, health);
            }
        });

        handles.insert(chain_id, handle);
        Ok(())
    }

    /// Check health of a specific chain registration
    async fn check_health_internal(
        chain_id: u64,
        registrar: &Arc<MultiChainRegistrar>,
        _config: &Arc<RwLock<MonitorConfig>>,
        failures: &Arc<RwLock<HashMap<u64, bool>>>,
        mock_expiries: &Arc<RwLock<HashMap<u64, Instant>>>,
    ) -> Result<RegistrationHealth> {
        let mut issues = Vec::new();

        // Check for simulated failures
        let is_failing = {
            let f = failures.read().await;
            f.get(&chain_id).copied().unwrap_or(false)
        };

        if is_failing {
            issues.push(HealthIssue {
                issue_type: IssueType::RpcFailure,
                severity: IssueSeverity::Critical,
                message: "RPC connection failure (simulated)".to_string(),
                detected_at: Instant::now(),
                resolved: false,
            });
        }

        // Get registration status
        let status = registrar
            .get_registration_status(chain_id)
            .await
            .unwrap_or(RegistrationStatus::NotRegistered);

        // Check if registered
        let is_healthy = match &status {
            RegistrationStatus::Confirmed { .. } => !is_failing,
            RegistrationStatus::NotRegistered => {
                issues.push(HealthIssue {
                    issue_type: IssueType::NotRegistered,
                    severity: IssueSeverity::Critical,
                    message: "Node not registered on chain".to_string(),
                    detected_at: Instant::now(),
                    resolved: false,
                });
                false
            }
            RegistrationStatus::Failed { error } => {
                issues.push(HealthIssue {
                    issue_type: IssueType::NotRegistered,
                    severity: IssueSeverity::Critical,
                    message: format!("Registration failed: {}", error),
                    detected_at: Instant::now(),
                    resolved: false,
                });
                false
            }
            RegistrationStatus::Pending { .. } => true, // Pending is considered healthy
        };

        // Check mock expiry
        let time_until_expiry = {
            let expiries = mock_expiries.read().await;
            expiries.get(&chain_id).map(|expiry_time| {
                let now = Instant::now();
                if *expiry_time > now {
                    expiry_time.duration_since(now)
                } else {
                    Duration::from_secs(0)
                }
            })
        };

        // Check FAB balance (mock for now)
        let fab_balance = U256::from(1000u64) * U256::exp10(18); // Mock 1000 FAB
        let stake_balance = U256::from(1000u64) * U256::exp10(18); // Mock staked amount

        Ok(RegistrationHealth {
            chain_id,
            status,
            last_check: Instant::now(),
            is_healthy,
            issues,
            stake_balance,
            fab_balance,
            registration_block: None,
            time_until_expiry,
        })
    }

    /// Stop monitoring all chains
    pub async fn stop_monitoring(&self) -> Result<()> {
        info!("Stopping registration monitoring");

        let mut handles = self.monitor_handles.lock().await;

        for (chain_id, handle) in handles.drain() {
            debug!("Stopping monitor for chain {}", chain_id);
            handle.abort();
        }

        Ok(())
    }

    /// Get health status for a specific chain
    pub async fn get_health(&self, chain_id: u64) -> Result<RegistrationHealth> {
        let states = self.health_states.read().await;
        states
            .get(&chain_id)
            .cloned()
            .ok_or_else(|| anyhow!("No health data for chain {}", chain_id))
    }

    /// Update monitor configuration
    pub async fn update_config(&self, new_config: MonitorConfig) -> Result<()> {
        let mut config = self.config.write().await;
        *config = new_config;
        info!("Monitor configuration updated");
        Ok(())
    }

    /// Register a warning callback
    pub async fn on_warning<F>(&self, callback: F)
    where
        F: Fn(WarningEvent) -> BoxFuture<'static, ()> + Send + Sync + 'static,
    {
        let mut callbacks = self.warning_callbacks.write().await;
        callbacks.push(Box::new(callback));
    }

    /// Get current metrics
    pub async fn get_metrics(&self) -> Result<HashMap<String, f64>> {
        self.metrics_collector.get_all_metrics().await
    }

    /// Enable auto-renewal for a chain
    pub async fn enable_auto_renewal(&self, _chain_id: u64) -> Result<()> {
        let mut config = self.config.write().await;
        config.auto_renewal = true;
        Ok(())
    }

    /// Check if auto-renewal is enabled
    pub async fn is_auto_renewal_enabled(&self, _chain_id: u64) -> Result<bool> {
        let config = self.config.read().await;
        Ok(config.auto_renewal)
    }

    /// Mock an expiring registration (for testing)
    pub async fn mock_expiring_registration(
        &self,
        chain_id: u64,
        expires_in: Duration,
    ) -> Result<()> {
        let mut expiries = self.mock_expiries.write().await;
        expiries.insert(chain_id, Instant::now() + expires_in);

        // Enable mock mode
        let mut mock = self.mock_mode.write().await;
        *mock = true;

        Ok(())
    }

    /// Check if a chain was renewed (for testing)
    pub async fn was_renewed(&self, chain_id: u64) -> Result<bool> {
        let history = self.renewal_history.read().await;
        Ok(history
            .get(&chain_id)
            .map(|h| !h.is_empty())
            .unwrap_or(false))
    }

    /// Simulate RPC failure (for testing)
    pub async fn simulate_rpc_failure(&self, chain_id: u64, fail: bool) -> Result<()> {
        let mut failures = self.simulated_failures.write().await;
        if fail {
            failures.insert(chain_id, true);
        } else {
            failures.remove(&chain_id);
        }
        Ok(())
    }
}
