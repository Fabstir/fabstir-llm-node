use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Metrics specific to registration monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationMetrics {
    pub chain_id: u64,
    pub registration_uptime: Duration,
    pub last_health_check: DateTime<Utc>,
    pub health_check_count: u64,
    pub renewal_count: u64,
    pub failed_renewals: u64,
    pub warning_count: u64,
    pub critical_alerts: u64,
    pub rpc_failures: u64,
    pub api_check_failures: u64,
    pub average_check_duration: Duration,
    pub last_renewal_attempt: Option<DateTime<Utc>>,
    pub consecutive_failures: u32,
}

impl RegistrationMetrics {
    pub fn new(chain_id: u64) -> Self {
        RegistrationMetrics {
            chain_id,
            registration_uptime: Duration::from_secs(0),
            last_health_check: Utc::now(),
            health_check_count: 0,
            renewal_count: 0,
            failed_renewals: 0,
            warning_count: 0,
            critical_alerts: 0,
            rpc_failures: 0,
            api_check_failures: 0,
            average_check_duration: Duration::from_secs(0),
            last_renewal_attempt: None,
            consecutive_failures: 0,
        }
    }

    /// Update metrics after a health check
    pub fn record_health_check(&mut self, duration: Duration, success: bool) {
        self.health_check_count += 1;
        self.last_health_check = Utc::now();

        // Update average duration
        let total_duration = self.average_check_duration * (self.health_check_count - 1) as u32;
        self.average_check_duration = (total_duration + duration) / self.health_check_count as u32;

        if success {
            self.consecutive_failures = 0;
        } else {
            self.consecutive_failures += 1;
        }
    }

    /// Record a renewal attempt
    pub fn record_renewal_attempt(&mut self, success: bool) {
        self.last_renewal_attempt = Some(Utc::now());

        if success {
            self.renewal_count += 1;
        } else {
            self.failed_renewals += 1;
        }
    }

    /// Record a warning
    pub fn record_warning(&mut self, is_critical: bool) {
        if is_critical {
            self.critical_alerts += 1;
        } else {
            self.warning_count += 1;
        }
    }

    /// Get health score (0-100)
    pub fn health_score(&self) -> f64 {
        let mut score = 100.0;

        // Deduct for failures
        score -= (self.consecutive_failures as f64 * 10.0).min(30.0);
        score -= (self.rpc_failures as f64 * 2.0).min(20.0);
        score -= (self.api_check_failures as f64 * 1.0).min(10.0);
        score -= (self.failed_renewals as f64 * 5.0).min(20.0);

        // Deduct for alerts
        score -= (self.critical_alerts as f64 * 5.0).min(15.0);
        score -= (self.warning_count as f64 * 1.0).min(5.0);

        score.max(0.0)
    }
}

/// Aggregated metrics across all chains
pub struct AggregatedMetrics {
    metrics_by_chain: Arc<RwLock<HashMap<u64, RegistrationMetrics>>>,
    global_stats: Arc<RwLock<GlobalStats>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalStats {
    pub total_chains_monitored: usize,
    pub healthy_chains: usize,
    pub chains_with_warnings: usize,
    pub chains_with_critical_issues: usize,
    pub total_health_checks: u64,
    pub total_renewals: u64,
    pub total_failures: u64,
    pub overall_health_score: f64,
    pub last_updated: DateTime<Utc>,
}

impl AggregatedMetrics {
    pub fn new() -> Self {
        AggregatedMetrics {
            metrics_by_chain: Arc::new(RwLock::new(HashMap::new())),
            global_stats: Arc::new(RwLock::new(GlobalStats {
                total_chains_monitored: 0,
                healthy_chains: 0,
                chains_with_warnings: 0,
                chains_with_critical_issues: 0,
                total_health_checks: 0,
                total_renewals: 0,
                total_failures: 0,
                overall_health_score: 100.0,
                last_updated: Utc::now(),
            })),
        }
    }

    /// Update metrics for a specific chain
    pub async fn update_chain_metrics(&self, chain_id: u64, metrics: RegistrationMetrics) {
        let mut chain_metrics = self.metrics_by_chain.write().await;
        chain_metrics.insert(chain_id, metrics);

        // Recalculate global stats
        self.recalculate_global_stats().await;
    }

    /// Get metrics for a specific chain
    pub async fn get_chain_metrics(&self, chain_id: u64) -> Option<RegistrationMetrics> {
        let metrics = self.metrics_by_chain.read().await;
        metrics.get(&chain_id).cloned()
    }

    /// Get all metrics
    pub async fn get_all_metrics(&self) -> HashMap<u64, RegistrationMetrics> {
        self.metrics_by_chain.read().await.clone()
    }

    /// Get global statistics
    pub async fn get_global_stats(&self) -> GlobalStats {
        self.global_stats.read().await.clone()
    }

    /// Recalculate global statistics
    async fn recalculate_global_stats(&self) {
        let chain_metrics = self.metrics_by_chain.read().await;
        let mut stats = self.global_stats.write().await;

        stats.total_chains_monitored = chain_metrics.len();
        stats.healthy_chains = 0;
        stats.chains_with_warnings = 0;
        stats.chains_with_critical_issues = 0;
        stats.total_health_checks = 0;
        stats.total_renewals = 0;
        stats.total_failures = 0;

        let mut total_score = 0.0;

        for (_, metrics) in chain_metrics.iter() {
            let score = metrics.health_score();
            total_score += score;

            if score >= 90.0 {
                stats.healthy_chains += 1;
            } else if score >= 70.0 {
                stats.chains_with_warnings += 1;
            } else {
                stats.chains_with_critical_issues += 1;
            }

            stats.total_health_checks += metrics.health_check_count;
            stats.total_renewals += metrics.renewal_count;
            stats.total_failures += metrics.failed_renewals + metrics.rpc_failures;
        }

        stats.overall_health_score = if chain_metrics.is_empty() {
            100.0
        } else {
            total_score / chain_metrics.len() as f64
        };

        stats.last_updated = Utc::now();
    }

    /// Export metrics in Prometheus format
    pub async fn export_prometheus(&self) -> String {
        let mut output = String::new();
        let chain_metrics = self.metrics_by_chain.read().await;
        let global_stats = self.global_stats.read().await;

        // Global metrics
        output.push_str(&format!(
            "# HELP registration_health_score Overall health score (0-100)\n"
        ));
        output.push_str(&format!("# TYPE registration_health_score gauge\n"));
        output.push_str(&format!(
            "registration_health_score {}\n",
            global_stats.overall_health_score
        ));

        output.push_str(&format!(
            "# HELP registration_healthy_chains Number of healthy chains\n"
        ));
        output.push_str(&format!("# TYPE registration_healthy_chains gauge\n"));
        output.push_str(&format!(
            "registration_healthy_chains {}\n",
            global_stats.healthy_chains
        ));

        output.push_str(&format!(
            "# HELP registration_total_renewals Total renewal count\n"
        ));
        output.push_str(&format!("# TYPE registration_total_renewals counter\n"));
        output.push_str(&format!(
            "registration_total_renewals {}\n",
            global_stats.total_renewals
        ));

        // Per-chain metrics
        for (chain_id, metrics) in chain_metrics.iter() {
            output.push_str(&format!(
                "registration_chain_health_score{{chain=\"{}\"}} {}\n",
                chain_id,
                metrics.health_score()
            ));

            output.push_str(&format!(
                "registration_chain_checks{{chain=\"{}\"}} {}\n",
                chain_id, metrics.health_check_count
            ));

            output.push_str(&format!(
                "registration_chain_renewals{{chain=\"{}\"}} {}\n",
                chain_id, metrics.renewal_count
            ));

            output.push_str(&format!(
                "registration_chain_failures{{chain=\"{}\"}} {}\n",
                chain_id, metrics.consecutive_failures
            ));
        }

        output
    }
}

impl Default for AggregatedMetrics {
    fn default() -> Self {
        Self::new()
    }
}
