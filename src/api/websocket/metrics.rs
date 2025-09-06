use anyhow::Result;
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