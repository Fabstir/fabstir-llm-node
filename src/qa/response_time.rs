use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseTimeConfig {
    pub buckets_ms: Vec<u64>,
    pub percentiles: Vec<f64>,
    pub sliding_window_size: usize,
    pub alert_threshold_p99_ms: u64,
    pub track_by_model: bool,
    pub track_by_operation: bool,
    pub export_interval_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetrics {
    pub count: u64,
    pub average_ms: f64,
    pub min_ms: u64,
    pub max_ms: u64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
    pub p99_9: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyBucket {
    pub min_ms: u64,
    pub max_ms: Option<u64>,
    pub count: u64,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyDistribution {
    pub buckets: Vec<LatencyBucket>,
    pub total_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceAlert {
    pub timestamp: u64,
    pub metric_name: String,
    pub metric_value: f64,
    pub threshold: f64,
    pub model: Option<String>,
    pub operation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPerformance {
    pub model_id: String,
    pub average_ms: f64,
    pub count: u64,
    pub percentiles: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsAggregation {
    pub operations: HashMap<String, ResponseMetrics>,
    pub total_requests: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    pub timestamp: u64,
    pub average_ms: f64,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceComparison {
    pub baseline_name: String,
    pub improvement_percentage: f64,
    pub p50_improvement: f64,
    pub p99_improvement: f64,
    pub average_improvement: f64,
}

#[derive(Debug, Error)]
pub enum ResponseTimeError {
    #[error("Invalid model: {0}")]
    InvalidModel(String),
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
    #[error("Export error: {0}")]
    ExportError(String),
    #[error("Baseline not found: {0}")]
    BaselineNotFound(String),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
struct ResponseRecord {
    pub timestamp: u64,
    pub model: String,
    pub operation: String,
    pub duration_ms: u64,
}

#[derive(Debug)]
pub struct ResponseTimeTracker {
    config: ResponseTimeConfig,
    responses: Arc<Mutex<VecDeque<ResponseRecord>>>,
    alert_sender: broadcast::Sender<PerformanceAlert>,
    baselines: Arc<Mutex<HashMap<String, ResponseMetrics>>>,
}

impl ResponseTimeTracker {
    pub fn new(config: ResponseTimeConfig) -> Self {
        let (alert_sender, _) = broadcast::channel(100);

        Self {
            config,
            responses: Arc::new(Mutex::new(VecDeque::new())),
            alert_sender,
            baselines: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn record_response_time(
        &self,
        model: &str,
        operation: &str,
        duration_ms: u64,
    ) -> Result<(), ResponseTimeError> {
        let record = ResponseRecord {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            model: model.to_string(),
            operation: operation.to_string(),
            duration_ms,
        };

        let mut responses = self.responses.lock().await;
        responses.push_back(record);

        // Maintain sliding window
        while responses.len() > self.config.sliding_window_size {
            responses.pop_front();
        }

        // Check for alerts
        self.check_performance_alerts().await;

        Ok(())
    }

    pub async fn get_current_metrics(&self) -> ResponseMetrics {
        let responses = self.responses.lock().await;
        self.calculate_metrics_from_responses(&responses)
    }

    pub async fn calculate_percentiles(&self) -> ResponseMetrics {
        self.get_current_metrics().await
    }

    pub async fn get_latency_distribution(&self) -> LatencyDistribution {
        let responses = self.responses.lock().await;
        let mut buckets = Vec::new();
        let total_count = responses.len() as u64;

        for (i, &bucket_max) in self.config.buckets_ms.iter().enumerate() {
            let bucket_min = if i == 0 { 0 } else { self.config.buckets_ms[i - 1] };
            
            let count = responses
                .iter()
                .filter(|r| r.duration_ms > bucket_min && r.duration_ms <= bucket_max)
                .count() as u64;

            buckets.push(LatencyBucket {
                min_ms: bucket_min,
                max_ms: Some(bucket_max),
                count,
                percentage: if total_count > 0 { count as f64 / total_count as f64 * 100.0 } else { 0.0 },
            });
        }

        // Add overflow bucket for times > max bucket
        let max_bucket = *self.config.buckets_ms.last().unwrap_or(&0);
        let overflow_count = responses
            .iter()
            .filter(|r| r.duration_ms > max_bucket)
            .count() as u64;

        buckets.push(LatencyBucket {
            min_ms: max_bucket,
            max_ms: None,
            count: overflow_count,
            percentage: if total_count > 0 { overflow_count as f64 / total_count as f64 * 100.0 } else { 0.0 },
        });

        LatencyDistribution {
            buckets,
            total_count,
        }
    }

    pub async fn get_model_metrics(&self, model: &str) -> ModelPerformance {
        let responses = self.responses.lock().await;
        let model_responses: VecDeque<_> = responses
            .iter()
            .filter(|r| r.model == model)
            .cloned()
            .collect();

        let metrics = self.calculate_metrics_from_responses(&model_responses);
        
        let mut percentiles = HashMap::new();
        percentiles.insert("p50".to_string(), metrics.p50);
        percentiles.insert("p90".to_string(), metrics.p90);
        percentiles.insert("p95".to_string(), metrics.p95);
        percentiles.insert("p99".to_string(), metrics.p99);

        ModelPerformance {
            model_id: model.to_string(),
            average_ms: metrics.average_ms,
            count: metrics.count,
            percentiles,
        }
    }

    pub async fn get_operation_breakdown(&self, model: &str) -> MetricsAggregation {
        let responses = self.responses.lock().await;
        let mut operations = HashMap::new();
        let mut total_requests = 0;

        // Group by operation
        let mut operation_responses: HashMap<String, VecDeque<ResponseRecord>> = HashMap::new();
        for response in responses.iter() {
            if response.model == model {
                operation_responses
                    .entry(response.operation.clone())
                    .or_insert_with(VecDeque::new)
                    .push_back(response.clone());
                total_requests += 1;
            }
        }

        // Calculate metrics for each operation
        for (operation, operation_data) in operation_responses {
            let metrics = self.calculate_metrics_from_responses(&operation_data);
            operations.insert(operation, metrics);
        }

        MetricsAggregation {
            operations,
            total_requests,
        }
    }

    pub async fn subscribe_to_alerts(&self) -> broadcast::Receiver<PerformanceAlert> {
        self.alert_sender.subscribe()
    }

    pub async fn get_time_series_data(
        &self,
        interval: Duration,
        points: usize,
    ) -> Vec<TimeSeriesPoint> {
        let responses = self.responses.lock().await;
        let mut time_series = Vec::new();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let interval_secs = interval.as_secs();

        for i in 0..points {
            let start_time = now - (interval_secs * (points - i) as u64);
            let end_time = start_time + interval_secs;

            let period_responses: Vec<_> = responses
                .iter()
                .filter(|r| r.timestamp >= start_time && r.timestamp < end_time)
                .collect();

            let count = period_responses.len() as u64;
            let average_ms = if count > 0 {
                period_responses.iter().map(|r| r.duration_ms).sum::<u64>() as f64 / count as f64
            } else {
                0.0
            };

            time_series.push(TimeSeriesPoint {
                timestamp: start_time,
                average_ms,
                count,
            });
        }

        time_series
    }

    pub async fn export_prometheus_format(&self) -> String {
        let metrics = self.get_current_metrics().await;
        let mut output = String::new();

        output.push_str("# HELP response_time_milliseconds Response time distribution\n");
        output.push_str("# TYPE response_time_milliseconds summary\n");
        
        for &percentile in &self.config.percentiles {
            let value = match percentile {
                50.0 => metrics.p50,
                90.0 => metrics.p90,
                95.0 => metrics.p95,
                99.0 => metrics.p99,
                99.9 => metrics.p99_9,
                _ => 0.0,
            };
            
            output.push_str(&format!(
                "response_time_milliseconds{{quantile=\"{}\"}} {}\n",
                percentile / 100.0,
                value
            ));
        }

        output.push_str(&format!("response_time_milliseconds_count {}\n", metrics.count));
        output.push_str(&format!("response_time_milliseconds_sum {}\n", metrics.average_ms * metrics.count as f64));

        output
    }

    pub async fn export_json_format(&self) -> Result<String, ResponseTimeError> {
        let metrics = self.get_current_metrics().await;
        let distribution = self.get_latency_distribution().await;

        let export_data = serde_json::json!({
            "metrics": metrics,
            "distribution": distribution,
            "timestamp": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        Ok(serde_json::to_string_pretty(&export_data)?)
    }

    pub async fn capture_baseline(&self, name: &str) -> ResponseMetrics {
        let metrics = self.get_current_metrics().await;
        let mut baselines = self.baselines.lock().await;
        baselines.insert(name.to_string(), metrics.clone());
        metrics
    }

    pub async fn compare_with_baseline(&self, baseline_name: &str) -> Result<PerformanceComparison, ResponseTimeError> {
        let baselines = self.baselines.lock().await;
        let baseline = baselines
            .get(baseline_name)
            .ok_or_else(|| ResponseTimeError::BaselineNotFound(baseline_name.to_string()))?;

        let current = self.get_current_metrics().await;

        let p50_improvement = ((baseline.p50 - current.p50) / baseline.p50) * 100.0;
        let p99_improvement = ((baseline.p99 - current.p99) / baseline.p99) * 100.0;
        let average_improvement = ((baseline.average_ms - current.average_ms) / baseline.average_ms) * 100.0;
        let improvement_percentage = (p50_improvement + p99_improvement + average_improvement) / 3.0;

        Ok(PerformanceComparison {
            baseline_name: baseline_name.to_string(),
            improvement_percentage,
            p50_improvement,
            p99_improvement,
            average_improvement,
        })
    }

    async fn check_performance_alerts(&self) {
        let metrics = self.get_current_metrics().await;
        
        if metrics.p99 > self.config.alert_threshold_p99_ms as f64 {
            let alert = PerformanceAlert {
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                metric_name: "p99_latency".to_string(),
                metric_value: metrics.p99,
                threshold: self.config.alert_threshold_p99_ms as f64,
                model: None,
                operation: None,
            };
            
            let _ = self.alert_sender.send(alert);
        }
    }

    fn calculate_metrics_from_responses(&self, responses: &VecDeque<ResponseRecord>) -> ResponseMetrics {
        if responses.is_empty() {
            return ResponseMetrics {
                count: 0,
                average_ms: 0.0,
                min_ms: 0,
                max_ms: 0,
                p50: 0.0,
                p90: 0.0,
                p95: 0.0,
                p99: 0.0,
                p99_9: 0.0,
            };
        }

        let mut durations: Vec<u64> = responses.iter().map(|r| r.duration_ms).collect();
        durations.sort_unstable();

        let count = durations.len() as u64;
        let sum: u64 = durations.iter().sum();
        let average_ms = sum as f64 / count as f64;
        let min_ms = durations[0];
        let max_ms = durations[durations.len() - 1];

        let p50 = self.calculate_percentile(&durations, 50.0);
        let p90 = self.calculate_percentile(&durations, 90.0);
        let p95 = self.calculate_percentile(&durations, 95.0);
        let p99 = self.calculate_percentile(&durations, 99.0);
        let p99_9 = self.calculate_percentile(&durations, 99.9);

        ResponseMetrics {
            count,
            average_ms,
            min_ms,
            max_ms,
            p50,
            p90,
            p95,
            p99,
            p99_9,
        }
    }

    fn calculate_percentile(&self, sorted_durations: &[u64], percentile: f64) -> f64 {
        if sorted_durations.is_empty() {
            return 0.0;
        }

        let index = (percentile / 100.0) * (sorted_durations.len() - 1) as f64;
        let lower_index = index.floor() as usize;
        let upper_index = index.ceil() as usize;

        if lower_index == upper_index {
            sorted_durations[lower_index] as f64
        } else {
            let lower_value = sorted_durations[lower_index] as f64;
            let upper_value = sorted_durations[upper_index] as f64;
            let weight = index - lower_index as f64;
            lower_value + weight * (upper_value - lower_value)
        }
    }
}