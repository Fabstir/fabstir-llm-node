// src/monitoring/metrics.rs - Metrics collection and export

use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use std::time::{Duration, Instant};
use async_trait::async_trait;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub enable_metrics: bool,
    pub collection_interval_ms: u64,
    pub retention_period_hours: u64,
    pub aggregation_windows: Vec<TimeWindow>,
    pub export_format: String,
    pub export_endpoint: String,
    pub buffer_size: usize,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        MetricsConfig {
            enable_metrics: true,
            collection_interval_ms: 1000,
            retention_period_hours: 24,
            aggregation_windows: vec![TimeWindow::OneMinute],
            export_format: "prometheus".to_string(),
            export_endpoint: "http://localhost:9090".to_string(),
            buffer_size: 10000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeWindow {
    OneMinute,
    FiveMinutes,
    FifteenMinutes,
    OneHour,
    OneDay,
}

impl TimeWindow {
    pub fn as_duration(&self) -> Duration {
        match self {
            TimeWindow::OneMinute => Duration::from_secs(60),
            TimeWindow::FiveMinutes => Duration::from_secs(300),
            TimeWindow::FifteenMinutes => Duration::from_secs(900),
            TimeWindow::OneHour => Duration::from_secs(3600),
            TimeWindow::OneDay => Duration::from_secs(86400),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AggregationType {
    Sum,
    Average,
    Min,
    Max,
    Count,
    P50,
    P90,
    P95,
    P99,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
    Summary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MetricValue {
    Counter(f64),
    Gauge(f64),
    Histogram { buckets: Vec<(f64, u64)>, sum: f64, count: u64 },
    Summary { quantiles: Vec<(f64, f64)>, sum: f64, count: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricLabel {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct Metric {
    pub name: String,
    pub help: String,
    pub metric_type: MetricType,
    pub value: MetricValue,
    pub labels: Vec<MetricLabel>,
    pub timestamp: DateTime<Utc>,
    pub last_updated: Instant,
}

#[derive(Debug, Clone)]
pub struct HistogramStatistics {
    pub count: u64,
    pub sum: f64,
    pub average: f64,
    pub min: f64,
    pub max: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
}

#[derive(Debug, Clone)]
pub struct SummaryStatistics {
    pub count: u64,
    pub sum: f64,
    pub average: f64,
    pub quantiles: Vec<(f64, f64)>,
}

// Time series data for aggregation
#[derive(Debug, Clone)]
struct DataPoint {
    value: f64,
    timestamp: Instant,
}

#[derive(Debug)]
struct MetricData {
    metric: Metric,
    time_series: Vec<DataPoint>,
    label_values: HashMap<Vec<(String, String)>, f64>,
}

pub struct MetricsCollector {
    config: MetricsConfig,
    state: Arc<RwLock<MetricsState>>,
    gc_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
}

impl Clone for MetricsCollector {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: self.state.clone(),
            gc_handle: None, // Don't clone the gc_handle
        }
    }
}

struct MetricsState {
    metrics: HashMap<String, MetricData>,
    counters: HashMap<String, Arc<Counter>>,
    gauges: HashMap<String, Arc<Gauge>>,
    histograms: HashMap<String, Arc<Histogram>>,
    summaries: HashMap<String, Arc<Summary>>,
    snapshots: HashMap<String, Vec<u8>>,
    subscriptions: HashMap<String, Vec<tokio::sync::mpsc::Sender<Metric>>>,
}

pub struct MetricsRegistry {
    collectors: HashMap<String, Arc<dyn MetricCollector>>,
}

#[async_trait]
pub trait MetricCollector: Send + Sync {
    fn name(&self) -> &str;
    fn help(&self) -> &str;
    fn metric_type(&self) -> MetricType;
    async fn collect(&self) -> Result<MetricValue>;
}

// Counter implementation
#[derive(Clone)]
pub struct Counter {
    name: String,
    help: String,
    value: Arc<RwLock<f64>>,
    labels: Vec<String>,
    label_values: Arc<RwLock<HashMap<Vec<(String, String)>, f64>>>,
    collector: Option<Arc<MetricsCollector>>,
}

impl Counter {
    pub async fn inc(&self) {
        let mut value = self.value.write().await;
        *value += 1.0;
    }
    

    pub async fn inc_by<T: Into<f64>>(&self, v: T) {
        let mut value = self.value.write().await;
        *value += v.into();
    }

    pub async fn get(&self) -> f64 {
        *self.value.read().await
    }

    pub async fn reset(&self) {
        let mut value = self.value.write().await;
        *value = 0.0;
    }

    pub fn with_labels(&self, labels: Vec<(&str, &str)>) -> LabeledCounter {
        LabeledCounter {
            counter: self,
            labels: labels.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }
}

pub struct LabeledCounter<'a> {
    counter: &'a Counter,
    labels: Vec<(String, String)>,
}

impl<'a> LabeledCounter<'a> {
    pub async fn inc(&self) {
        let mut label_values = self.counter.label_values.write().await;
        let value = label_values.entry(self.labels.clone()).or_insert(0.0);
        *value += 1.0;
    }

    pub async fn inc_by<T: Into<f64>>(&self, v: T) {
        let mut label_values = self.counter.label_values.write().await;
        let value = label_values.entry(self.labels.clone()).or_insert(0.0);
        *value += v.into();
    }

    pub async fn get(&self) -> f64 {
        let label_values = self.counter.label_values.read().await;
        label_values.get(&self.labels).copied().unwrap_or(0.0)
    }
}

// Gauge implementation
#[derive(Clone)]
pub struct Gauge {
    name: String,
    help: String,
    value: Arc<RwLock<f64>>,
    time_series: Arc<RwLock<Vec<DataPoint>>>,
    collector: Option<Arc<MetricsCollector>>,
}

impl Gauge {
    pub async fn set<T: Into<f64>>(&self, v: T) {
        let mut value = self.value.write().await;
        *value = v.into();
        
        // Add to time series
        let mut series = self.time_series.write().await;
        series.push(DataPoint {
            value: *value,
            timestamp: Instant::now(),
        });
        
        // Keep only recent data (last hour)
        let cutoff = Instant::now() - Duration::from_secs(3600);
        series.retain(|dp| dp.timestamp > cutoff);
        
        // Update metric in collector
        if let Some(collector) = &self.collector {
            let _ = collector.update_metric_value(&self.name, MetricValue::Gauge(*value)).await;
        }
    }

    pub async fn inc_by<T: Into<f64>>(&self, v: T) {
        let mut value = self.value.write().await;
        *value += v.into();
        let new_value = *value;
        
        let mut series = self.time_series.write().await;
        series.push(DataPoint {
            value: new_value,
            timestamp: Instant::now(),
        });
        
        // Update metric in collector
        if let Some(collector) = &self.collector {
            let _ = collector.update_metric_value(&self.name, MetricValue::Gauge(new_value)).await;
        }
    }

    pub async fn dec_by<T: Into<f64>>(&self, v: T) {
        let mut value = self.value.write().await;
        *value -= v.into();
        let new_value = *value;
        
        let mut series = self.time_series.write().await;
        series.push(DataPoint {
            value: new_value,
            timestamp: Instant::now(),
        });
        
        // Update metric in collector
        if let Some(collector) = &self.collector {
            let _ = collector.update_metric_value(&self.name, MetricValue::Gauge(new_value)).await;
        }
    }

    pub async fn get(&self) -> f64 {
        *self.value.read().await
    }
}

// Histogram implementation
#[derive(Clone)]
pub struct Histogram {
    name: String,
    help: String,
    buckets: Vec<f64>,
    bucket_counts: Arc<RwLock<Vec<u64>>>,
    sum: Arc<RwLock<f64>>,
    count: Arc<RwLock<u64>>,
    observations: Arc<RwLock<Vec<f64>>>,
    collector: Option<Arc<MetricsCollector>>,
}

impl Histogram {
    pub async fn observe(&self, v: f64) {
        // Update buckets
        let mut bucket_counts = self.bucket_counts.write().await;
        for (i, &bucket) in self.buckets.iter().enumerate() {
            if v <= bucket {
                bucket_counts[i] += 1;
            }
        }
        
        // Update sum and count
        let mut sum = self.sum.write().await;
        *sum += v;
        
        let mut count = self.count.write().await;
        *count += 1;
        
        // Store observation for statistics
        let mut observations = self.observations.write().await;
        observations.push(v);
        
        // Keep only last 10000 observations
        if observations.len() > 10000 {
            observations.remove(0);
        }
    }

    pub async fn get_statistics(&self) -> HistogramStatistics {
        let observations = self.observations.read().await;
        let count = *self.count.read().await;
        let sum = *self.sum.read().await;
        
        if observations.is_empty() {
            return HistogramStatistics {
                count: 0,
                sum: 0.0,
                average: 0.0,
                min: 0.0,
                max: 0.0,
                p50: 0.0,
                p90: 0.0,
                p95: 0.0,
                p99: 0.0,
            };
        }
        
        let mut sorted = observations.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let min = sorted[0];
        let max = sorted[sorted.len() - 1];
        let average = sum / count as f64;
        
        let p50 = percentile(&sorted, 0.5);
        let p90 = percentile(&sorted, 0.9);
        let p95 = percentile(&sorted, 0.95);
        let p99 = percentile(&sorted, 0.99);
        
        HistogramStatistics {
            count,
            sum,
            average,
            min,
            max,
            p50,
            p90,
            p95,
            p99,
        }
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    
    let index = ((sorted.len() - 1) as f64 * p) as usize;
    sorted[index]
}

// Summary implementation
#[derive(Clone)]
pub struct Summary {
    name: String,
    help: String,
    quantiles: Vec<f64>,
    observations: Arc<RwLock<Vec<(f64, Instant)>>>,
    window: Duration,
    collector: Option<Arc<MetricsCollector>>,
}

impl Summary {
    pub async fn observe(&self, v: f64) {
        let mut observations = self.observations.write().await;
        observations.push((v, Instant::now()));
        
        // Remove old observations outside window
        let cutoff = Instant::now() - self.window;
        observations.retain(|(_, timestamp)| *timestamp > cutoff);
        
        // Update metric in collector
        if let Some(collector) = &self.collector {
            let values: Vec<f64> = observations.iter().map(|(v, _)| *v).collect();
            if !values.is_empty() {
                let mut sorted = values.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                
                let quantiles: Vec<(f64, f64)> = self.quantiles.iter()
                    .map(|&q| (q, percentile(&sorted, q)))
                    .collect();
                
                let sum: f64 = values.iter().sum();
                let count = values.len() as u64;
                
                let _ = collector.update_metric_value(&self.name, MetricValue::Summary {
                    quantiles,
                    sum,
                    count,
                }).await;
            }
        }
    }

    pub async fn get_statistics(&self) -> SummaryStatistics {
        let observations = self.observations.read().await;
        let values: Vec<f64> = observations.iter().map(|(v, _)| *v).collect();
        
        if values.is_empty() {
            return SummaryStatistics {
                count: 0,
                sum: 0.0,
                average: 0.0,
                quantiles: vec![],
            };
        }
        
        let count = values.len() as u64;
        let sum: f64 = values.iter().sum();
        let average = sum / count as f64;
        
        let mut sorted = values;
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let quantiles: Vec<(f64, f64)> = self.quantiles.iter()
            .map(|&q| (q, percentile(&sorted, q)))
            .collect();
        
        SummaryStatistics {
            count,
            sum,
            average,
            quantiles,
        }
    }

    pub async fn get_quantiles(&self) -> QuantileMap {
        let stats = self.get_statistics().await;
        QuantileMap {
            quantiles: stats.quantiles,
        }
    }
}

// MetricsExporter trait
#[async_trait]
pub trait MetricsExporter: Send + Sync {
    async fn export(&self, metrics: Vec<Metric>) -> Result<String>;
}

// PrometheusExporter implementation
pub struct PrometheusExporter;

impl PrometheusExporter {
    pub fn new() -> Self {
        PrometheusExporter
    }
}

#[async_trait]
impl MetricsExporter for PrometheusExporter {
    async fn export(&self, metrics: Vec<Metric>) -> Result<String> {
        let mut output = String::new();
        
        for metric in metrics {
            // Write HELP and TYPE
            output.push_str(&format!("# HELP {} {}\n", metric.name, metric.help));
            output.push_str(&format!("# TYPE {} {}\n", metric.name, 
                match metric.metric_type {
                    MetricType::Counter => "counter",
                    MetricType::Gauge => "gauge",
                    MetricType::Histogram => "histogram",
                    MetricType::Summary => "summary",
                }
            ));
            
            // Write metric value
            match &metric.value {
                MetricValue::Counter(v) => {
                    // Format counters as integers if they're whole numbers
                    if v.fract() == 0.0 {
                        output.push_str(&format!("{} {}\n", metric.name, *v as i64));
                    } else {
                        output.push_str(&format!("{} {}\n", metric.name, v));
                    }
                },
                MetricValue::Gauge(v) => {
                    output.push_str(&format!("{} {}\n", metric.name, v));
                },
                MetricValue::Histogram { buckets, sum, count } => {
                    // Write bucket values
                    for (bucket_bound, bucket_count) in buckets {
                        output.push_str(&format!("{}_bucket{{le=\"{}\"}} {}\n", 
                            metric.name, bucket_bound, bucket_count));
                    }
                    output.push_str(&format!("{}_bucket{{le=\"+Inf\"}} {}\n", 
                        metric.name, count));
                    output.push_str(&format!("{}_sum {}\n", metric.name, sum));
                    output.push_str(&format!("{}_count {}\n", metric.name, count));
                },
                MetricValue::Summary { quantiles, sum, count } => {
                    // Write quantile values
                    for (quantile, value) in quantiles {
                        output.push_str(&format!("{}{{quantile=\"{}\"}} {}\n", 
                            metric.name, quantile, value));
                    }
                    output.push_str(&format!("{}_sum {}\n", metric.name, sum));
                    output.push_str(&format!("{}_count {}\n", metric.name, count));
                },
            }
        }
        
        Ok(output)
    }
}

impl MetricsCollector {
    pub async fn new(config: MetricsConfig) -> Result<Self> {
        let state = Arc::new(RwLock::new(MetricsState {
            metrics: HashMap::new(),
            counters: HashMap::new(),
            gauges: HashMap::new(),
            histograms: HashMap::new(),
            summaries: HashMap::new(),
            snapshots: HashMap::new(),
            subscriptions: HashMap::new(),
        }));
        
        Ok(MetricsCollector {
            config,
            state,
            gc_handle: None,
        })
    }

    pub async fn register_counter(&self, name: &str, help: &str) -> Result<Arc<Counter>> {
        let counter = Arc::new(Counter {
            name: name.to_string(),
            help: help.to_string(),
            value: Arc::new(RwLock::new(0.0)),
            labels: vec![],
            label_values: Arc::new(RwLock::new(HashMap::new())),
            collector: None,
        });
        
        let mut state = self.state.write().await;
        state.counters.insert(name.to_string(), counter.clone());
        state.metrics.insert(name.to_string(), MetricData {
            metric: Metric {
                name: name.to_string(),
                help: help.to_string(),
                metric_type: MetricType::Counter,
                value: MetricValue::Counter(0.0),
                labels: vec![],
                timestamp: Utc::now(),
                last_updated: Instant::now(),
            },
            time_series: vec![],
            label_values: HashMap::new(),
        });
        
        Ok(counter)
    }

    pub async fn register_counter_with_labels(
        &self, 
        name: &str, 
        help: &str,
        labels: Vec<&str>
    ) -> Result<Arc<Counter>> {
        let counter = Arc::new(Counter {
            name: name.to_string(),
            help: help.to_string(),
            value: Arc::new(RwLock::new(0.0)),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            label_values: Arc::new(RwLock::new(HashMap::new())),
            collector: None,
        });
        
        let mut state = self.state.write().await;
        state.counters.insert(name.to_string(), counter.clone());
        state.metrics.insert(name.to_string(), MetricData {
            metric: Metric {
                name: name.to_string(),
                help: help.to_string(),
                metric_type: MetricType::Counter,
                value: MetricValue::Counter(0.0),
                labels: vec![],
                timestamp: Utc::now(),
                last_updated: Instant::now(),
            },
            time_series: vec![],
            label_values: HashMap::new(),
        });
        
        Ok(counter)
    }

    pub async fn register_gauge(&self, name: &str, help: &str) -> Result<Arc<Gauge>> {
        let gauge = Arc::new(Gauge {
            name: name.to_string(),
            help: help.to_string(),
            value: Arc::new(RwLock::new(0.0)),
            time_series: Arc::new(RwLock::new(vec![])),
            collector: None,
        });
        
        let mut state = self.state.write().await;
        state.gauges.insert(name.to_string(), gauge.clone());
        state.metrics.insert(name.to_string(), MetricData {
            metric: Metric {
                name: name.to_string(),
                help: help.to_string(),
                metric_type: MetricType::Gauge,
                value: MetricValue::Gauge(0.0),
                labels: vec![],
                timestamp: Utc::now(),
                last_updated: Instant::now(),
            },
            time_series: vec![],
            label_values: HashMap::new(),
        });
        
        Ok(gauge)
    }

    pub async fn register_histogram(
        &self, 
        name: &str, 
        help: &str,
        buckets: Vec<f64>
    ) -> Result<Arc<Histogram>> {
        let histogram = Arc::new(Histogram {
            name: name.to_string(),
            help: help.to_string(),
            buckets: buckets.clone(),
            bucket_counts: Arc::new(RwLock::new(vec![0; buckets.len()])),
            sum: Arc::new(RwLock::new(0.0)),
            count: Arc::new(RwLock::new(0)),
            observations: Arc::new(RwLock::new(vec![])),
            collector: None,
        });
        
        let mut state = self.state.write().await;
        state.histograms.insert(name.to_string(), histogram.clone());
        state.metrics.insert(name.to_string(), MetricData {
            metric: Metric {
                name: name.to_string(),
                help: help.to_string(),
                metric_type: MetricType::Histogram,
                value: MetricValue::Histogram {
                    buckets: buckets.iter().map(|&b| (b, 0)).collect(),
                    sum: 0.0,
                    count: 0,
                },
                labels: vec![],
                timestamp: Utc::now(),
                last_updated: Instant::now(),
            },
            time_series: vec![],
            label_values: HashMap::new(),
        });
        
        Ok(histogram)
    }

    pub async fn register_summary(
        &self,
        name: &str,
        help: &str,
        quantiles: Vec<f64>,
        window: Duration,
    ) -> Result<Arc<Summary>> {
        let summary = Arc::new(Summary {
            name: name.to_string(),
            help: help.to_string(),
            quantiles: quantiles.clone(),
            observations: Arc::new(RwLock::new(vec![])),
            window,
            collector: None,
        });
        
        let mut state = self.state.write().await;
        state.summaries.insert(name.to_string(), summary.clone());
        state.metrics.insert(name.to_string(), MetricData {
            metric: Metric {
                name: name.to_string(),
                help: help.to_string(),
                metric_type: MetricType::Summary,
                value: MetricValue::Summary {
                    quantiles: quantiles.iter().map(|&q| (q, 0.0)).collect(),
                    sum: 0.0,
                    count: 0,
                },
                labels: vec![],
                timestamp: Utc::now(),
                last_updated: Instant::now(),
            },
            time_series: vec![],
            label_values: HashMap::new(),
        });
        
        Ok(summary)
    }

    pub async fn get_metric(&self, name: &str) -> Result<Metric> {
        let state = self.state.read().await;
        
        // Get current value from the actual metric object
        let mut metric = state.metrics.get(name)
            .map(|data| data.metric.clone())
            .ok_or_else(|| anyhow!("Metric not found: {}", name))?;
        
        // Update with current value
        if let Some(counter) = state.counters.get(name) {
            metric.value = MetricValue::Counter(counter.get().await);
        } else if let Some(gauge) = state.gauges.get(name) {
            metric.value = MetricValue::Gauge(gauge.get().await);
        } else if let Some(histogram) = state.histograms.get(name) {
            let stats = histogram.get_statistics().await;
            let bucket_counts = histogram.bucket_counts.read().await;
            let buckets: Vec<(f64, u64)> = histogram.buckets.iter()
                .zip(bucket_counts.iter())
                .map(|(b, c)| (*b, *c))
                .collect();
            metric.value = MetricValue::Histogram {
                buckets,
                sum: stats.sum,
                count: stats.count,
            };
        } else if let Some(summary) = state.summaries.get(name) {
            let stats = summary.get_statistics().await;
            let quantiles = summary.get_quantiles().await;
            metric.value = MetricValue::Summary {
                quantiles: quantiles.quantiles.clone(),
                sum: stats.sum,
                count: stats.count,
            };
        }
        
        metric.timestamp = Utc::now();
        metric.last_updated = Instant::now();
        
        Ok(metric)
    }

    pub async fn get_aggregated_value(
        &self,
        name: &str,
        window: TimeWindow,
        aggregation: AggregationType,
    ) -> Result<f64> {
        let state = self.state.read().await;
        
        // Try to get values from gauge time series first, then counter, then generic metrics
        let values: Vec<f64> = if let Some(gauge) = state.gauges.get(name) {
            let series = gauge.time_series.read().await;
            let cutoff = Instant::now() - window.as_duration();
            series.iter()
                .filter(|dp| dp.timestamp > cutoff)
                .map(|dp| dp.value)
                .collect()
        } else if let Some(counter) = state.counters.get(name) {
            // For counters, we need to build time series from metrics data
            if let Some(data) = state.metrics.get(name) {
                let cutoff = Instant::now() - window.as_duration();
                data.time_series.iter()
                    .filter(|dp| dp.timestamp > cutoff)
                    .map(|dp| dp.value)
                    .collect()
            } else {
                vec![]
            }
        } else if let Some(data) = state.metrics.get(name) {
            // Fallback to metrics data time series
            let cutoff = Instant::now() - window.as_duration();
            data.time_series.iter()
                .filter(|dp| dp.timestamp > cutoff)
                .map(|dp| dp.value)
                .collect()
        } else {
            return Err(anyhow!("Metric not found: {}", name));
        };
        
        if values.is_empty() {
            return Ok(0.0);
        }
        
        match aggregation {
            AggregationType::Sum => Ok(values.iter().sum()),
            AggregationType::Average => Ok(values.iter().sum::<f64>() / values.len() as f64),
            AggregationType::Min => Ok(values.iter().cloned().fold(f64::INFINITY, f64::min)),
            AggregationType::Max => Ok(values.iter().cloned().fold(f64::NEG_INFINITY, f64::max)),
            AggregationType::Count => Ok(values.len() as f64),
            AggregationType::P50 => {
                let mut sorted = values.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                Ok(percentile(&sorted, 0.5))
            },
            AggregationType::P90 => {
                let mut sorted = values.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                Ok(percentile(&sorted, 0.9))
            },
            AggregationType::P95 => {
                let mut sorted = values.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                Ok(percentile(&sorted, 0.95))
            },
            AggregationType::P99 => {
                let mut sorted = values.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                Ok(percentile(&sorted, 0.99))
            },
        }
    }

    pub async fn export(&self, exporter: &dyn MetricsExporter) -> Result<String> {
        let mut metrics = Vec::new();
        
        for name in self.list_metrics().await.iter().map(|m| m.name.clone()) {
            if let Ok(metric) = self.get_metric(&name).await {
                metrics.push(metric);
            }
        }
        
        exporter.export(metrics).await
    }

    pub async fn batch_update(&self, updates: Vec<(&str, MetricValue)>) -> Result<()> {
        let mut state = self.state.write().await;
        
        for (name, value) in updates {
            if let Some(data) = state.metrics.get_mut(name) {
                data.metric.value = value;
                data.metric.timestamp = Utc::now();
                data.metric.last_updated = Instant::now();
            } else {
                // Create new metric if it doesn't exist
                let metric_type = match &value {
                    MetricValue::Counter(_) => MetricType::Counter,
                    MetricValue::Gauge(_) => MetricType::Gauge,
                    MetricValue::Histogram { .. } => MetricType::Histogram,
                    MetricValue::Summary { .. } => MetricType::Summary,
                };
                
                state.metrics.insert(name.to_string(), MetricData {
                    metric: Metric {
                        name: name.to_string(),
                        help: format!("Auto-created metric: {}", name),
                        metric_type,
                        value,
                        labels: vec![],
                        timestamp: Utc::now(),
                        last_updated: Instant::now(),
                    },
                    time_series: vec![],
                    label_values: HashMap::new(),
                });
            }
        }
        
        Ok(())
    }

    pub async fn save_snapshot(&self, path: &std::path::Path) -> Result<()> {
        let state = self.state.read().await;
        
        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        // Serialize all metrics with current values
        let mut snapshot_data: HashMap<String, Metric> = HashMap::new();
        
        for (name, data) in state.metrics.iter() {
            let mut metric = data.metric.clone();
            
            // Update with current value from actual counter/gauge/etc
            if let Some(counter) = state.counters.get(name) {
                metric.value = MetricValue::Counter(counter.get().await);
            } else if let Some(gauge) = state.gauges.get(name) {
                metric.value = MetricValue::Gauge(gauge.get().await);
            }
            // Add other metric types if needed
            
            snapshot_data.insert(name.clone(), metric);
        }
        
        let snapshot = serde_json::to_vec(&snapshot_data)?;
        std::fs::write(path, snapshot)?;
        
        Ok(())
    }

    pub async fn load_snapshot(&self, path: &std::path::Path) -> Result<()> {
        let data = std::fs::read(path)?;
        let snapshot_data: HashMap<String, Metric> = serde_json::from_slice(&data)?;
        
        let mut state = self.state.write().await;
        
        for (name, metric) in snapshot_data {
            // Restore the metric data
            state.metrics.insert(name.clone(), MetricData {
                metric: metric.clone(),
                time_series: vec![],
                label_values: HashMap::new(),
            });
            
            // Also recreate the actual counter/gauge/etc with the saved value
            match &metric.value {
                MetricValue::Counter(value) => {
                    let counter = Arc::new(Counter {
                        name: name.clone(),
                        help: metric.help.clone(),
                        value: Arc::new(RwLock::new(*value)),
                        labels: vec![],
                        label_values: Arc::new(RwLock::new(HashMap::new())),
                        collector: None,
                    });
                    state.counters.insert(name.clone(), counter);
                }
                MetricValue::Gauge(value) => {
                    let gauge = Arc::new(Gauge {
                        name: name.clone(),
                        help: metric.help.clone(),
                        value: Arc::new(RwLock::new(*value)),
                        time_series: Arc::new(RwLock::new(vec![])),
                        collector: None,
                    });
                    state.gauges.insert(name.clone(), gauge);
                }
                _ => {} // Handle other types if needed
            }
        }
        
        Ok(())
    }

    pub async fn calculate_rate(&self, metric: &str, window: Duration) -> Result<f64> {
        // First, get the current metric value to ensure time series is updated
        let current_metric = self.get_metric(metric).await?;
        let current_value = match current_metric.value {
            MetricValue::Counter(v) => v,
            MetricValue::Gauge(v) => v,
            _ => return Err(anyhow!("Cannot calculate rate for this metric type")),
        };
        
        // Record current value in time series
        self.record_time_series_point(metric, current_value).await?;
        
        let state = self.state.read().await;
        let data = state.metrics.get(metric)
            .ok_or_else(|| anyhow!("Metric not found: {}", metric))?;
        
        let cutoff = Instant::now() - window;
        let points: Vec<&DataPoint> = data.time_series.iter()
            .filter(|dp| dp.timestamp > cutoff)
            .collect();
        
        if points.len() < 2 {
            return Ok(0.0);
        }
        
        // Calculate rate as change per second
        let first = points.first().unwrap();
        let last = points.last().unwrap();
        let value_change = last.value - first.value;
        let time_diff = last.timestamp.duration_since(first.timestamp).as_secs_f64();
        
        if time_diff > 0.0 {
            Ok(value_change / time_diff)
        } else {
            Ok(0.0)
        }
    }

    pub async fn subscribe(&self, metric_name: &str) -> Result<tokio::sync::mpsc::Receiver<Metric>> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        
        let mut state = self.state.write().await;
        
        // Send current value if metric exists
        if let Some(data) = state.metrics.get(metric_name) {
            let _ = tx.send(data.metric.clone()).await;
        }
        
        state.subscriptions
            .entry(metric_name.to_string())
            .or_insert_with(Vec::new)
            .push(tx);
        
        Ok(rx)
    }

    pub async fn list_metrics(&self) -> Vec<Metric> {
        let state = self.state.read().await;
        state.metrics.values()
            .map(|data| data.metric.clone())
            .collect()
    }

    pub async fn start_garbage_collection(&self) {
        // For now, just do a one-time cleanup instead of spawning a thread
        let mut state = self.state.write().await;
        let retention_period = Duration::from_secs(self.config.retention_period_hours * 3600);
        let cutoff = Instant::now() - retention_period;
        
        // Clean up old time series data
        for data in state.metrics.values_mut() {
            data.time_series.retain(|dp| dp.timestamp > cutoff);
        }
    }

    pub async fn record_time_series_point(&self, name: &str, value: f64) -> Result<()> {
        let mut state = self.state.write().await;
        if let Some(data) = state.metrics.get_mut(name) {
            data.time_series.push(DataPoint {
                value,
                timestamp: Instant::now(),
            });
            // Keep only last hour
            let cutoff = Instant::now() - Duration::from_secs(3600);
            data.time_series.retain(|dp| dp.timestamp > cutoff);
            Ok(())
        } else {
            Err(anyhow!("Metric not found: {}", name))
        }
    }
    
    /// Helper method for tests to increment counter with notifications
    pub async fn increment_counter_with_notification(&self, name: &str) -> Result<()> {
        // First increment the counter
        {
            let state = self.state.read().await;
            if let Some(counter) = state.counters.get(name) {
                counter.inc().await;
            } else {
                return Err(anyhow!("Counter not found: {}", name));
            }
        }
        
        // Get the updated metric
        let metric = self.get_metric(name).await?;
        
        // Notify subscribers
        let state = self.state.read().await;
        if let Some(subscribers) = state.subscriptions.get(name) {
            for tx in subscribers {
                let _ = tx.send(metric.clone()).await;
            }
        }
        
        Ok(())
    }
    
    async fn update_metric_value(&self, name: &str, value: MetricValue) -> Result<()> {
        let mut state = self.state.write().await;
        if let Some(data) = state.metrics.get_mut(name) {
            data.metric.value = value.clone();
            data.metric.timestamp = Utc::now();
            data.metric.last_updated = Instant::now();
            
            // Add to time series for rate calculation
            if let MetricValue::Counter(v) | MetricValue::Gauge(v) = &value {
                data.time_series.push(DataPoint {
                    value: *v,
                    timestamp: Instant::now(),
                });
                
                // Keep only last hour of data
                let cutoff = Instant::now() - Duration::from_secs(3600);
                data.time_series.retain(|dp| dp.timestamp > cutoff);
            }
            
            // Send update to subscribers
        }
        
        // Get subscribers and metric outside the write lock
        let (metric_to_send, subscribers) = {
            let state = self.state.read().await;
            if let Some(data) = state.metrics.get(name) {
                (Some(data.metric.clone()), state.subscriptions.get(name).cloned())
            } else {
                (None, None)
            }
        };
        
        // Send updates
        if let (Some(metric), Some(subscribers)) = (metric_to_send, subscribers) {
            for tx in subscribers {
                let _ = tx.send(metric.clone()).await;
            }
        }
        
        Ok(())
    }

    // Helper method to create with default config
    pub fn new_default() -> Self {
        let state = Arc::new(RwLock::new(MetricsState {
            metrics: HashMap::new(),
            counters: HashMap::new(),
            gauges: HashMap::new(),
            histograms: HashMap::new(),
            summaries: HashMap::new(),
            snapshots: HashMap::new(),
            subscriptions: HashMap::new(),
        }));

        MetricsCollector {
            config: MetricsConfig::default(),
            state,
            gc_handle: None,
        }
    }

    // Register gauge without async (sync version)
    pub fn register_gauge_sync(&self, name: &str, help: &str) {
        let name = name.to_string();
        let help = help.to_string();
        let state = self.state.clone();

        tokio::spawn(async move {
            let gauge = Arc::new(Gauge {
                name: name.clone(),
                help: help.clone(),
                value: Arc::new(RwLock::new(0.0)),
                time_series: Arc::new(RwLock::new(vec![])),
                collector: None,
            });

            let mut s = state.write().await;
            s.gauges.insert(name.clone(), gauge);
        });
    }

    // Register counter without async (sync version)
    pub fn register_counter_sync(&self, name: &str, help: &str) {
        let name = name.to_string();
        let help = help.to_string();
        let state = self.state.clone();

        tokio::spawn(async move {
            let counter = Arc::new(Counter {
                name: name.clone(),
                help: help.clone(),
                value: Arc::new(RwLock::new(0.0)),
                labels: vec![],
                label_values: Arc::new(RwLock::new(HashMap::new())),
                collector: None,
            });

            let mut s = state.write().await;
            s.counters.insert(name.clone(), counter);
        });
    }

    // Register histogram without async (sync version)
    pub fn register_histogram_sync(&self, name: &str, help: &str) {
        let name = name.to_string();
        let help = help.to_string();
        let state = self.state.clone();
        let buckets = vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0];

        tokio::spawn(async move {
            let histogram = Arc::new(Histogram {
                name: name.clone(),
                help: help.clone(),
                buckets: buckets.clone(),
                bucket_counts: Arc::new(RwLock::new(vec![0; buckets.len()])),
                sum: Arc::new(RwLock::new(0.0)),
                count: Arc::new(RwLock::new(0)),
                observations: Arc::new(RwLock::new(vec![])),
                collector: None,
            });

            let mut s = state.write().await;
            s.histograms.insert(name.clone(), histogram);
        });
    }

    // Increment counter
    pub fn increment_counter(&self, name: &str, value: f64) {
        let name = name.to_string();
        let state = self.state.clone();

        tokio::spawn(async move {
            let s = state.read().await;
            if let Some(counter) = s.counters.get(&name) {
                counter.inc_by(value).await;
            }
        });
    }

    // Set gauge value
    pub fn set_gauge(&self, name: &str, value: f64) {
        let name = name.to_string();
        let state = self.state.clone();

        tokio::spawn(async move {
            let s = state.read().await;
            if let Some(gauge) = s.gauges.get(&name) {
                gauge.set(value).await;
            }
        });
    }

    // Record histogram observation
    pub fn record_histogram(&self, name: &str, value: f64) {
        let name = name.to_string();
        let state = self.state.clone();

        tokio::spawn(async move {
            let s = state.read().await;
            if let Some(histogram) = s.histograms.get(&name) {
                histogram.observe(value).await;
            }
        });
    }

    // Get all metrics as a simple map
    pub async fn get_all_metrics(&self) -> Result<HashMap<String, f64>> {
        let mut result = HashMap::new();
        let state = self.state.read().await;

        for (name, counter) in &state.counters {
            let value = counter.value.read().await;
            result.insert(name.clone(), *value);
        }

        for (name, gauge) in &state.gauges {
            let value = gauge.value.read().await;
            result.insert(name.clone(), *value);
        }

        for (name, histogram) in &state.histograms {
            let count = histogram.count.read().await;
            result.insert(format!("{}_count", name), *count as f64);
            let sum = histogram.sum.read().await;
            result.insert(format!("{}_sum", name), *sum);
        }

        Ok(result)
    }
}

// Custom HashMap wrapper for quantiles that supports f64 keys
pub struct QuantileMap {
    quantiles: Vec<(f64, f64)>,
}

impl QuantileMap {
    pub fn get(&self, key: &f64) -> Option<&f64> {
        self.quantiles.iter()
            .find(|(k, _)| (k - key).abs() < f64::EPSILON)
            .map(|(_, v)| v)
    }
}

// Make MetricData serializable
impl Serialize for MetricData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("MetricData", 2)?;
        state.serialize_field("metric", &self.metric)?;
        state.serialize_field("label_values", &self.label_values)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for MetricData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct MetricDataHelper {
            metric: Metric,
            label_values: HashMap<Vec<(String, String)>, f64>,
        }
        
        let helper = MetricDataHelper::deserialize(deserializer)?;
        Ok(MetricData {
            metric: helper.metric,
            time_series: vec![],
            label_values: helper.label_values,
        })
    }
}

// Serialize Instant as elapsed seconds
impl Serialize for Metric {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Metric", 7)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("help", &self.help)?;
        state.serialize_field("metric_type", &self.metric_type)?;
        state.serialize_field("value", &self.value)?;
        state.serialize_field("labels", &self.labels)?;
        state.serialize_field("timestamp", &self.timestamp)?;
        state.serialize_field("last_updated_secs", &self.last_updated.elapsed().as_secs())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Metric {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct MetricHelper {
            name: String,
            help: String,
            metric_type: MetricType,
            value: MetricValue,
            labels: Vec<MetricLabel>,
            timestamp: DateTime<Utc>,
            last_updated_secs: u64,
        }
        
        let helper = MetricHelper::deserialize(deserializer)?;
        Ok(Metric {
            name: helper.name,
            help: helper.help,
            metric_type: helper.metric_type,
            value: helper.value,
            labels: helper.labels,
            timestamp: helper.timestamp,
            last_updated: Instant::now() - Duration::from_secs(helper.last_updated_secs),
        })
    }
}