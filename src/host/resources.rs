use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMetrics {
    pub timestamp: u64,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub gpus: Vec<GpuMetrics>,
    pub network: NetworkMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuMetrics {
    pub device_id: u32,
    pub name: String,
    pub usage_percent: f64,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub temperature_celsius: f64,
    pub power_draw_watts: f64,
    pub clock_speed_mhz: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub usage_percent: f64,
    pub core_count: u32,
    pub per_core_usage: Vec<f64>,
    pub temperature_celsius: Option<f64>,
    pub frequency_mhz: f64,
    pub load_average: (f64, f64, f64), // 1min, 5min, 15min
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub total_mb: u64,
    pub used_mb: u64,
    pub available_mb: u64,
    pub usage_percent: f64,
    pub swap_total_mb: u64,
    pub swap_used_mb: u64,
    pub cached_mb: u64,
    pub buffer_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub interface: String,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub errors: u64,
    pub dropped: u64,
    pub bandwidth_mbps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertThreshold {
    pub metric: String,
    pub level: AlertLevel,
    pub value: f64,
    pub duration: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AlertLevel {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAlert {
    pub timestamp: u64,
    pub metric: String,
    pub level: AlertLevel,
    pub current_value: f64,
    pub threshold_value: f64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAllocation {
    pub job_id: String,
    pub memory_mb: u64,
    pub cpu_cores: u32,
    pub gpu_devices: Vec<u32>,
    pub allocated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSummary {
    pub timestamp: u64,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub gpu_usage: Vec<f64>,
    pub network_bandwidth: f64,
    pub health_score: f64,
    pub active_alerts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePrediction {
    pub metric: String,
    pub predicted_value: f64,
    pub confidence: f64,
    pub time_horizon: std::time::Duration,
}

#[derive(Debug, Error)]
pub enum MonitoringError {
    #[error("Failed to initialize monitoring: {0}")]
    InitializationError(String),
    #[error("GPU not found: {0}")]
    GpuNotFound(u32),
    #[error("Invalid metric name: {0}")]
    InvalidMetric(String),
    #[error("Monitoring not started")]
    NotStarted,
    #[error("Resource allocation failed: {0}")]
    AllocationFailed(String),
}

#[derive(Debug)]
pub struct ResourceMonitor {
    initialized: bool,
    monitoring_handle: Option<JoinHandle<()>>,
    metrics_history: Vec<ResourceMetrics>,
    alert_thresholds: Vec<AlertThreshold>,
    alert_sender: broadcast::Sender<ResourceAlert>,
    allocations: HashMap<String, ResourceAllocation>,
    simulated_metrics: HashMap<String, f64>,
    predictive_enabled: bool,
}

impl ResourceMonitor {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(100);

        Self {
            initialized: false,
            monitoring_handle: None,
            metrics_history: Vec::new(),
            alert_thresholds: Vec::new(),
            alert_sender: sender,
            allocations: HashMap::new(),
            simulated_metrics: HashMap::new(),
            predictive_enabled: false,
        }
    }

    pub async fn initialize(&mut self) -> Result<(), MonitoringError> {
        // Simulate GPU detection - in real implementation would use CUDA/OpenCL
        self.initialized = true;
        Ok(())
    }

    pub async fn list_gpus(&self) -> Vec<String> {
        if !self.initialized {
            return vec![];
        }

        // Simulate GPU listing
        vec![
            "NVIDIA GeForce RTX 4090".to_string(),
            "NVIDIA GeForce RTX 4080".to_string(),
        ]
    }

    pub async fn get_gpu_metrics(&self, device_id: u32) -> Result<GpuMetrics, MonitoringError> {
        if !self.initialized {
            return Err(MonitoringError::NotStarted);
        }

        if device_id > 1 {
            return Err(MonitoringError::GpuNotFound(device_id));
        }

        // Simulate GPU metrics
        let usage = self
            .simulated_metrics
            .get("gpu_usage")
            .copied()
            .unwrap_or(45.0);
        let memory_usage = self
            .simulated_metrics
            .get("gpu_memory")
            .copied()
            .unwrap_or(8192.0);
        let temperature = self
            .simulated_metrics
            .get("gpu_temperature")
            .copied()
            .unwrap_or(65.0);

        Ok(GpuMetrics {
            device_id,
            name: format!("NVIDIA GPU {}", device_id),
            usage_percent: usage,
            memory_used_mb: memory_usage as u64,
            memory_total_mb: 24576, // 24GB
            temperature_celsius: temperature,
            power_draw_watts: 300.0,
            clock_speed_mhz: 2500,
        })
    }

    pub async fn get_cpu_metrics(&self) -> Result<CpuMetrics, MonitoringError> {
        if !self.initialized {
            return Err(MonitoringError::NotStarted);
        }

        let usage = self
            .simulated_metrics
            .get("cpu_usage")
            .copied()
            .unwrap_or(25.0);

        Ok(CpuMetrics {
            usage_percent: usage,
            core_count: 16,
            per_core_usage: vec![usage; 16],
            temperature_celsius: Some(55.0),
            frequency_mhz: 3400.0,
            load_average: (1.2, 1.5, 1.8),
        })
    }

    pub async fn get_memory_metrics(&self) -> Result<MemoryMetrics, MonitoringError> {
        if !self.initialized {
            return Err(MonitoringError::NotStarted);
        }

        let total = 32768; // 32GB
        let used = self
            .simulated_metrics
            .get("memory_usage")
            .map(|usage| (usage / 100.0 * total as f64) as u64)
            .unwrap_or(16384);

        Ok(MemoryMetrics {
            total_mb: total,
            used_mb: used,
            available_mb: total - used,
            usage_percent: (used as f64 / total as f64) * 100.0,
            swap_total_mb: 8192,
            swap_used_mb: 0,
            cached_mb: 4096,
            buffer_mb: 1024,
        })
    }

    pub async fn get_network_metrics(&self) -> Result<NetworkMetrics, MonitoringError> {
        if !self.initialized {
            return Err(MonitoringError::NotStarted);
        }

        Ok(NetworkMetrics {
            interface: "eth0".to_string(),
            bytes_sent: 1024 * 1024 * 100,     // 100MB
            bytes_received: 1024 * 1024 * 200, // 200MB
            packets_sent: 10000,
            packets_received: 15000,
            errors: 0,
            dropped: 0,
            bandwidth_mbps: 1000.0, // 1Gbps
        })
    }

    pub async fn start_monitoring(&mut self, interval: Duration) -> Result<(), MonitoringError> {
        if !self.initialized {
            return Err(MonitoringError::NotStarted);
        }

        let metrics_history = std::sync::Arc::new(Mutex::new(Vec::<ResourceMetrics>::new()));
        let history_clone = metrics_history.clone();

        let handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);

            loop {
                interval_timer.tick().await;

                // Simulate collecting metrics
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                let metrics = ResourceMetrics {
                    timestamp,
                    cpu: CpuMetrics {
                        usage_percent: 30.0,
                        core_count: 16,
                        per_core_usage: vec![30.0; 16],
                        temperature_celsius: Some(55.0),
                        frequency_mhz: 3400.0,
                        load_average: (1.2, 1.5, 1.8),
                    },
                    memory: MemoryMetrics {
                        total_mb: 32768,
                        used_mb: 16384,
                        available_mb: 16384,
                        usage_percent: 50.0,
                        swap_total_mb: 8192,
                        swap_used_mb: 0,
                        cached_mb: 4096,
                        buffer_mb: 1024,
                    },
                    gpus: vec![GpuMetrics {
                        device_id: 0,
                        name: "NVIDIA GPU 0".to_string(),
                        usage_percent: 45.0,
                        memory_used_mb: 8192,
                        memory_total_mb: 24576,
                        temperature_celsius: 65.0,
                        power_draw_watts: 300.0,
                        clock_speed_mhz: 2500,
                    }],
                    network: NetworkMetrics {
                        interface: "eth0".to_string(),
                        bytes_sent: 1024 * 1024 * 100,
                        bytes_received: 1024 * 1024 * 200,
                        packets_sent: 10000,
                        packets_received: 15000,
                        errors: 0,
                        dropped: 0,
                        bandwidth_mbps: 1000.0,
                    },
                };

                let mut history = history_clone.lock().await;
                history.push(metrics);

                // Keep only last 1000 entries
                if history.len() > 1000 {
                    history.remove(0);
                }
            }
        });

        self.monitoring_handle = Some(handle);
        Ok(())
    }

    pub async fn stop_monitoring(&mut self) -> Result<(), MonitoringError> {
        if let Some(handle) = self.monitoring_handle.take() {
            handle.abort();
        }
        Ok(())
    }

    pub async fn get_metrics_history(&self, duration: Duration) -> Vec<ResourceMetrics> {
        let cutoff_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - duration.as_secs();

        self.metrics_history
            .iter()
            .filter(|metrics| metrics.timestamp >= cutoff_time)
            .cloned()
            .collect()
    }

    pub async fn add_alert_threshold(
        &mut self,
        threshold: AlertThreshold,
    ) -> Result<(), MonitoringError> {
        self.alert_thresholds.push(threshold);
        Ok(())
    }

    pub async fn subscribe_to_alerts(&self) -> broadcast::Receiver<ResourceAlert> {
        self.alert_sender.subscribe()
    }

    pub async fn simulate_metric(&mut self, metric: &str, value: f64) {
        self.simulated_metrics.insert(metric.to_string(), value);

        // Check for alerts
        for threshold in &self.alert_thresholds {
            if threshold.metric == metric && value >= threshold.value {
                let alert = ResourceAlert {
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    metric: metric.to_string(),
                    level: threshold.level.clone(),
                    current_value: value,
                    threshold_value: threshold.value,
                    message: format!(
                        "{} exceeded threshold: {} >= {}",
                        metric, value, threshold.value
                    ),
                };

                let _ = self.alert_sender.send(alert);
            }
        }
    }

    pub async fn allocate_resources(
        &mut self,
        job_id: &str,
        memory_mb: u64,
        cpu_cores: u32,
    ) -> Result<(), MonitoringError> {
        let allocation = ResourceAllocation {
            job_id: job_id.to_string(),
            memory_mb,
            cpu_cores,
            gpu_devices: vec![],
            allocated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        self.allocations.insert(job_id.to_string(), allocation);
        Ok(())
    }

    pub async fn get_job_allocation(&self, job_id: &str) -> Option<ResourceAllocation> {
        self.allocations.get(job_id).cloned()
    }

    pub async fn get_total_allocated_resources(&self) -> ResourceAllocation {
        let mut total = ResourceAllocation {
            job_id: "total".to_string(),
            memory_mb: 0,
            cpu_cores: 0,
            gpu_devices: vec![],
            allocated_at: 0,
        };

        for allocation in self.allocations.values() {
            total.memory_mb += allocation.memory_mb;
            total.cpu_cores += allocation.cpu_cores;
        }

        total
    }

    pub async fn release_resources(&mut self, job_id: &str) -> Result<(), MonitoringError> {
        if self.allocations.remove(job_id).is_none() {
            return Err(MonitoringError::AllocationFailed(format!(
                "Job {} not found",
                job_id
            )));
        }
        Ok(())
    }

    pub async fn get_resource_summary(&self) -> Result<ResourceSummary, MonitoringError> {
        if !self.initialized {
            return Err(MonitoringError::NotStarted);
        }

        let cpu_metrics = self.get_cpu_metrics().await?;
        let memory_metrics = self.get_memory_metrics().await?;
        let gpu_metrics = self.get_gpu_metrics(0).await?;
        let network_metrics = self.get_network_metrics().await?;

        // Calculate health score based on various factors
        let health_score = ((100.0 - cpu_metrics.usage_percent) * 0.3
            + (100.0 - memory_metrics.usage_percent) * 0.3
            + (100.0 - gpu_metrics.usage_percent) * 0.4)
            .max(0.0);

        Ok(ResourceSummary {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            cpu_usage: cpu_metrics.usage_percent,
            memory_usage: memory_metrics.usage_percent,
            gpu_usage: vec![gpu_metrics.usage_percent],
            network_bandwidth: network_metrics.bandwidth_mbps,
            health_score,
            active_alerts: 0,
        })
    }

    pub async fn export_metrics_json(&self, duration: Duration) -> Result<String, MonitoringError> {
        let metrics = self.get_metrics_history(duration).await;
        serde_json::to_string_pretty(&metrics).map_err(|_| {
            MonitoringError::InitializationError("Failed to serialize metrics".to_string())
        })
    }

    pub async fn export_metrics_csv(&self, duration: Duration) -> Result<String, MonitoringError> {
        let metrics = self.get_metrics_history(duration).await;

        let mut csv =
            String::from("timestamp,cpu_usage,memory_usage,gpu_usage,network_bandwidth\n");
        for metric in metrics {
            csv.push_str(&format!(
                "{},{},{},{},{}\n",
                metric.timestamp,
                metric.cpu.usage_percent,
                metric.memory.usage_percent,
                metric.gpus.get(0).map(|g| g.usage_percent).unwrap_or(0.0),
                metric.network.bandwidth_mbps
            ));
        }

        Ok(csv)
    }

    pub async fn enable_predictive_monitoring(&mut self, enabled: bool) {
        self.predictive_enabled = enabled;
    }

    pub async fn get_resource_prediction(
        &self,
        metric: &str,
        horizon: std::time::Duration,
    ) -> Result<ResourcePrediction, MonitoringError> {
        if !self.predictive_enabled {
            return Err(MonitoringError::InvalidMetric(
                "Predictive monitoring not enabled".to_string(),
            ));
        }

        // Simple trend-based prediction
        let current_value = self.simulated_metrics.get(metric).copied().unwrap_or(50.0);
        let trend = 5.0; // Assume 5% increase per time unit
        let predicted_value = current_value + trend;

        Ok(ResourcePrediction {
            metric: metric.to_string(),
            predicted_value,
            confidence: 0.8,
            time_horizon: horizon,
        })
    }
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        Self::new()
    }
}
