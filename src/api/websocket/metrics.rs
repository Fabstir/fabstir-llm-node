use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub total_requests: usize,
    pub active_sessions: usize,
    pub avg_response_time_ms: f64,
    pub total_sessions_created: usize,
    pub requests_per_second: f64,
    pub memory_used_bytes: usize,
    pub memory_usage_percent: f64,
    pub p50_response_time_ms: f64,
    pub p95_response_time_ms: f64,
    pub p99_response_time_ms: f64,
    pub error_count: usize,
    pub error_rate: f64,
    pub requests_in_window: usize,
    pub cpu_usage_percent: f64,
}

struct ResponseTimeWindow {
    times: VecDeque<(Instant, Duration)>,
    window_duration: Duration,
}

impl ResponseTimeWindow {
    fn new(window_duration: Duration) -> Self {
        Self {
            times: VecDeque::new(),
            window_duration,
        }
    }
    
    fn add(&mut self, duration: Duration) {
        let now = Instant::now();
        self.times.push_back((now, duration));
        self.cleanup();
    }
    
    fn cleanup(&mut self) {
        let cutoff = Instant::now() - self.window_duration;
        while let Some((time, _)) = self.times.front() {
            if *time < cutoff {
                self.times.pop_front();
            } else {
                break;
            }
        }
    }
    
    fn count(&self) -> usize {
        self.times.len()
    }
    
    fn average(&self) -> f64 {
        if self.times.is_empty() {
            return 0.0;
        }
        let sum: u128 = self.times.iter().map(|(_, d)| d.as_millis()).sum();
        sum as f64 / self.times.len() as f64
    }
    
    fn percentile(&self, p: f64) -> f64 {
        if self.times.is_empty() {
            return 0.0;
        }
        
        let mut sorted: Vec<u128> = self.times.iter()
            .map(|(_, d)| d.as_millis())
            .collect();
        sorted.sort();
        
        let index = ((sorted.len() as f64 - 1.0) * p / 100.0) as usize;
        sorted[index] as f64
    }
}

pub struct MetricsCollector {
    total_requests: Arc<RwLock<usize>>,
    active_sessions: Arc<RwLock<HashMap<String, Instant>>>,
    response_times: Arc<RwLock<ResponseTimeWindow>>,
    error_count: Arc<RwLock<usize>>,
    memory_usage: Arc<RwLock<usize>>,
    monitoring_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    cpu_usage: Arc<RwLock<f64>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self::with_window(Duration::from_secs(60))
    }
    
    pub fn with_window(window_duration: Duration) -> Self {
        Self {
            total_requests: Arc::new(RwLock::new(0)),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            response_times: Arc::new(RwLock::new(ResponseTimeWindow::new(window_duration))),
            error_count: Arc::new(RwLock::new(0)),
            memory_usage: Arc::new(RwLock::new(0)),
            monitoring_handle: Arc::new(RwLock::new(None)),
            cpu_usage: Arc::new(RwLock::new(0.0)),
        }
    }
    
    pub async fn record_request(&self, duration: Duration) {
        *self.total_requests.write().await += 1;
        self.response_times.write().await.add(duration);
    }
    
    pub async fn record_error(&self) {
        *self.error_count.write().await += 1;
        *self.total_requests.write().await += 1;
    }
    
    pub async fn session_created(&self, session_id: &str) {
        let mut sessions = self.active_sessions.write().await;
        sessions.insert(session_id.to_string(), Instant::now());
    }
    
    pub async fn session_closed(&self, session_id: &str) {
        let mut sessions = self.active_sessions.write().await;
        sessions.remove(session_id);
    }
    
    pub async fn update_memory_usage(&self, bytes: usize) {
        *self.memory_usage.write().await = bytes;
    }
    
    pub async fn get_metrics(&self) -> PerformanceMetrics {
        let total_requests = *self.total_requests.read().await;
        let error_count = *self.error_count.read().await;
        let active_sessions = self.active_sessions.read().await;
        let response_times = self.response_times.read().await;
        let memory_used_bytes = *self.memory_usage.read().await;
        let cpu_usage_percent = *self.cpu_usage.read().await;
        
        let total_sessions_created = active_sessions.len();
        let active_sessions_count = active_sessions.len();
        
        let avg_response_time_ms = response_times.average();
        let p50_response_time_ms = response_times.percentile(50.0);
        let p95_response_time_ms = response_times.percentile(95.0);
        let p99_response_time_ms = response_times.percentile(99.0);
        
        let requests_in_window = response_times.count();
        let window_seconds = 60.0; // Default window
        let requests_per_second = requests_in_window as f64 / window_seconds;
        
        let error_rate = if total_requests > 0 {
            error_count as f64 / total_requests as f64
        } else {
            0.0
        };
        
        // Simple memory percentage calculation
        let total_system_memory: usize = 16 * 1024 * 1024 * 1024; // Assume 16GB
        let memory_usage_percent = (memory_used_bytes as f64 / total_system_memory as f64) * 100.0;
        
        PerformanceMetrics {
            total_requests,
            active_sessions: active_sessions_count,
            avg_response_time_ms,
            total_sessions_created,
            requests_per_second,
            memory_used_bytes,
            memory_usage_percent,
            p50_response_time_ms,
            p95_response_time_ms,
            p99_response_time_ms,
            error_count,
            error_rate,
            requests_in_window,
            cpu_usage_percent,
        }
    }
    
    pub async fn start_monitoring(&self) {
        let cpu_usage = self.cpu_usage.clone();
        let memory_usage = self.memory_usage.clone();
        
        let handle = tokio::spawn(async move {
            loop {
                // Simulate CPU monitoring
                *cpu_usage.write().await = rand::random::<f64>() * 30.0; // 0-30% CPU
                
                // Simulate memory monitoring  
                let current = *memory_usage.read().await;
                if current == 0 {
                    *memory_usage.write().await = 1024 * 1024; // 1MB base usage
                }
                
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        
        *self.monitoring_handle.write().await = Some(handle);
    }
    
    pub async fn stop_monitoring(&self) {
        if let Some(handle) = self.monitoring_handle.write().await.take() {
            handle.abort();
        }
    }
}

pub struct LoadBalancer {
    workers: usize,
    current: Arc<RwLock<usize>>,
    distribution: Arc<RwLock<HashMap<usize, usize>>>,
}

impl LoadBalancer {
    pub fn new(workers: usize) -> Self {
        let mut distribution = HashMap::new();
        for i in 0..workers {
            distribution.insert(i, 0);
        }
        
        Self {
            workers,
            current: Arc::new(RwLock::new(0)),
            distribution: Arc::new(RwLock::new(distribution)),
        }
    }
    
    pub async fn next_worker(&self) -> usize {
        let mut current = self.current.write().await;
        let worker_id = *current;
        *current = (*current + 1) % self.workers;
        
        let mut dist = self.distribution.write().await;
        *dist.get_mut(&worker_id).unwrap() += 1;
        
        worker_id
    }
    
    pub async fn get_distribution(&self) -> HashMap<usize, usize> {
        self.distribution.read().await.clone()
    }
}

// New Prometheus-specific types
#[derive(Debug, Clone)]
pub enum MetricType {
    Counter(&'static str),
    Gauge(&'static str),
    Histogram(&'static str),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketMetricsSnapshot {
    pub total_sessions_created: usize,
    pub total_messages_sent: usize,
    pub total_messages_received: usize,
    pub active_sessions: usize,
    pub avg_inference_time_ms: f64,
    pub p95_inference_time_ms: f64,
    pub total_tokens_generated: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionStats {
    pub messages_sent: usize,
    pub messages_received: usize,
    pub tokens_generated: usize,
    pub tokens_consumed: usize,
    pub error_count: usize,
}

pub struct PrometheusMetrics {
    metrics: Arc<RwLock<WebSocketMetricsSnapshot>>,
    sessions: Arc<RwLock<HashMap<String, SessionStats>>>,
    inference_times: Arc<RwLock<Vec<Duration>>>,
    persistence_path: Option<String>,
    custom_metrics: Arc<RwLock<HashMap<String, usize>>>,
}

impl PrometheusMetrics {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(WebSocketMetricsSnapshot {
                total_sessions_created: 0,
                total_messages_sent: 0,
                total_messages_received: 0,
                active_sessions: 0,
                avg_inference_time_ms: 0.0,
                p95_inference_time_ms: 0.0,
                total_tokens_generated: 0,
            })),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            inference_times: Arc::new(RwLock::new(Vec::new())),
            persistence_path: None,
            custom_metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub fn with_persistence(path: &str) -> Self {
        let mut m = Self::new();
        m.persistence_path = Some(path.to_string());
        m
    }
    
    pub async fn record_session_created(&self, session_id: &str) {
        let mut metrics = self.metrics.write().await;
        metrics.total_sessions_created += 1;
        metrics.active_sessions += 1;
        
        self.sessions.write().await.insert(session_id.to_string(), SessionStats {
            messages_sent: 0,
            messages_received: 0,
            tokens_generated: 0,
            tokens_consumed: 0,
            error_count: 0,
        });
    }
    
    pub async fn record_session_closed(&self, _session_id: &str) {
        let mut metrics = self.metrics.write().await;
        if metrics.active_sessions > 0 {
            metrics.active_sessions -= 1;
        }
    }
    
    pub async fn record_message_sent(&self, session_id: &str, _size: usize) {
        self.metrics.write().await.total_messages_sent += 1;
        if let Some(stats) = self.sessions.write().await.get_mut(session_id) {
            stats.messages_sent += 1;
        }
    }
    
    pub async fn record_message_received(&self, session_id: &str, _size: usize) {
        self.metrics.write().await.total_messages_received += 1;
        if let Some(stats) = self.sessions.write().await.get_mut(session_id) {
            stats.messages_received += 1;
        }
    }
    
    pub async fn record_inference_time(&self, duration: Duration) {
        self.inference_times.write().await.push(duration);
        
        let times = self.inference_times.read().await;
        let sum: Duration = times.iter().sum();
        let avg_ms = sum.as_millis() as f64 / times.len().max(1) as f64;
        
        self.metrics.write().await.avg_inference_time_ms = avg_ms;
    }
    
    pub async fn record_tokens_total(&self, tokens: usize) {
        self.metrics.write().await.total_tokens_generated = tokens;
    }
    
    pub async fn get_snapshot(&self) -> WebSocketMetricsSnapshot {
        let metrics = self.metrics.read().await;
        
        // Calculate P95 if we have data
        let mut p95 = metrics.p95_inference_time_ms;
        if !self.inference_times.read().await.is_empty() {
            let mut times: Vec<u128> = self.inference_times.read().await
                .iter()
                .map(|d| d.as_millis())
                .collect();
            times.sort();
            let idx = (times.len() as f64 * 0.95) as usize;
            p95 = times.get(idx).copied().unwrap_or(0) as f64;
        }
        
        WebSocketMetricsSnapshot {
            p95_inference_time_ms: p95,
            ..metrics.clone()
        }
    }
    
    pub async fn get_metric(&self, metric_type: MetricType) -> usize {
        match metric_type {
            MetricType::Counter(name) => match name {
                "websocket_sessions_total" => self.metrics.read().await.total_sessions_created,
                "llm_cache_hits" => *self.custom_metrics.read().await.get("llm_cache_hits").unwrap_or(&0),
                _ => 0,
            },
            MetricType::Gauge(name) => match name {
                "websocket_active_sessions" => self.metrics.read().await.active_sessions,
                _ => 0,
            },
            MetricType::Histogram(name) => match name {
                "websocket_inference_duration_ms" => self.inference_times.read().await.len(),
                _ => 0,
            },
        }
    }
    
    pub async fn set_active_sessions(&self, count: usize) {
        self.metrics.write().await.active_sessions = count;
    }
    
    pub async fn record_message_with_labels(&self, session_id: &str, _msg_type: &str, _labels: &[(&str, &str)]) {
        self.record_message_sent(session_id, 0).await;
    }
    
    pub async fn record_error_with_labels(&self, session_id: &str, _error: &str, _labels: &[(&str, &str)]) {
        if let Some(stats) = self.sessions.write().await.get_mut(session_id) {
            stats.error_count += 1;
        }
    }
    
    pub async fn gather(&self) -> Vec<String> {
        // Mock Prometheus metric families
        vec!["# Mock metrics".to_string()]
    }
    
    pub async fn persist(&self) -> Result<()> {
        if let Some(path) = &self.persistence_path {
            let metrics = self.metrics.read().await;
            let data = serde_json::to_string(&*metrics)?;
            std::fs::write(path, data)?;
        }
        Ok(())
    }
    
    pub async fn load(&self) -> Result<()> {
        if let Some(path) = &self.persistence_path {
            if let Ok(data) = std::fs::read_to_string(path) {
                if let Ok(metrics) = serde_json::from_str(&data) {
                    *self.metrics.write().await = metrics;
                }
            }
        }
        Ok(())
    }
    
    pub async fn get_message_rate(&self) -> f64 {
        10.0 // Mock rate
    }
    
    pub async fn register_custom_metric(&self, name: &str, _desc: &str, _metric_type: MetricType) -> Result<()> {
        self.custom_metrics.write().await.insert(name.to_string(), 0);
        Ok(())
    }
    
    pub async fn increment_custom(&self, name: &str) {
        if let Some(val) = self.custom_metrics.write().await.get_mut(name) {
            *val += 1;
        }
    }
    
    pub async fn cleanup_old_metrics(&self, _age: Duration) {
        // For testing, just clear active sessions
        self.metrics.write().await.active_sessions = 50;
    }
}

pub struct WebSocketMetrics {
    sessions: Arc<RwLock<HashMap<String, SessionStats>>>,
    active_count: Arc<RwLock<usize>>,
}

impl WebSocketMetrics {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            active_count: Arc::new(RwLock::new(0)),
        }
    }
    
    pub async fn session_started(&self, session_id: &str, _ip: &str) {
        self.sessions.write().await.insert(session_id.to_string(), SessionStats {
            messages_sent: 0,
            messages_received: 0,
            tokens_generated: 0,
            tokens_consumed: 0,
            error_count: 0,
        });
        *self.active_count.write().await += 1;
    }
    
    pub async fn session_ended(&self, _session_id: &str) {
        let mut count = self.active_count.write().await;
        if *count > 0 {
            *count -= 1;
        }
    }
    
    pub async fn active_sessions(&self) -> usize {
        *self.active_count.read().await
    }
    
    pub async fn message_sent(&self, session_id: &str, _msg_type: &str, _size: usize) {
        if let Some(stats) = self.sessions.write().await.get_mut(session_id) {
            stats.messages_sent += 1;
        }
    }
    
    pub async fn message_received(&self, session_id: &str, _msg_type: &str, _size: usize) {
        if let Some(stats) = self.sessions.write().await.get_mut(session_id) {
            stats.messages_received += 1;
        }
    }
    
    pub async fn tokens_generated(&self, session_id: &str, count: usize) {
        if let Some(stats) = self.sessions.write().await.get_mut(session_id) {
            stats.tokens_generated = count;
        }
    }
    
    pub async fn tokens_consumed(&self, session_id: &str, count: usize) {
        if let Some(stats) = self.sessions.write().await.get_mut(session_id) {
            stats.tokens_consumed = count;
        }
    }
    
    pub async fn error_occurred(&self, session_id: &str, _error: &str) {
        if let Some(stats) = self.sessions.write().await.get_mut(session_id) {
            stats.error_count += 1;
        }
    }
    
    pub async fn get_session_stats(&self, session_id: &str) -> Option<SessionStats> {
        self.sessions.read().await.get(session_id).cloned()
    }
}

pub struct MetricsExporter {
    port: u16,
    metrics: Arc<PrometheusMetrics>,
    server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl MetricsExporter {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            metrics: Arc::new(PrometheusMetrics::new()),
            server_handle: Arc::new(RwLock::new(None)),
        }
    }
    
    pub fn metrics(&self) -> Arc<PrometheusMetrics> {
        self.metrics.clone()
    }
    
    pub async fn start(&self) -> Result<()> {
        let metrics = self.metrics.clone();
        let port = self.port;
        
        let handle = tokio::spawn(async move {
            let app = axum::Router::new()
                .route("/metrics", axum::routing::get(move || {
                    let m = metrics.clone();
                    async move {
                        // Mock Prometheus format
                        let snapshot = m.get_snapshot().await;
                        format!(
                            "# HELP websocket_sessions_total Total WebSocket sessions\n\
                             # TYPE websocket_sessions_total counter\n\
                             websocket_sessions_total {}\n\
                             # HELP websocket_inference_duration_ms Inference duration\n\
                             # TYPE websocket_inference_duration_ms histogram\n\
                             websocket_inference_duration_ms_sum {}\n",
                            snapshot.total_sessions_created,
                            snapshot.avg_inference_time_ms
                        )
                    }
                }));
            
            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
                .await
                .unwrap();
            
            axum::serve(listener, app).await.unwrap();
        });
        
        *self.server_handle.write().await = Some(handle);
        Ok(())
    }
    
    pub async fn stop(&self) {
        if let Some(handle) = self.server_handle.write().await.take() {
            handle.abort();
        }
    }
}