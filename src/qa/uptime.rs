use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use chrono::{DateTime, Utc, Duration};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ServiceStatus {
    Online,
    Offline,
    Maintenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UptimeConfig {
    pub check_interval_ms: u64,
    pub downtime_threshold_ms: u64,
    pub alert_thresholds: Vec<(f64, String)>, // (percentage, message)
    pub rolling_window_hours: u32,
    pub persist_metrics: bool,
    pub persistence_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DowntimeEvent {
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub duration_ms: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UptimeMetrics {
    pub uptime_percentage: f64,
    pub window_duration: Duration,
    pub total_downtime_ms: u64,
    pub downtime_events: u32,
    pub last_downtime: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UptimeAlert {
    pub timestamp: DateTime<Utc>,
    pub threshold: f64,
    pub current_uptime: f64,
    pub message: String,
    pub service: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalUptime {
    pub data_points: Vec<UptimeDataPoint>,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UptimeDataPoint {
    pub timestamp: DateTime<Utc>,
    pub uptime_percentage: f64,
    pub window_hours: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryEvent {
    pub timestamp: DateTime<Utc>,
    pub recovery_time_ms: u64,
    pub downtime_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DowntimeStatistics {
    pub total_events: u32,
    pub total_downtime_ms: u64,
    pub average_downtime_ms: u64,
    pub reasons: HashMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiServiceReport {
    pub services: HashMap<String, ServiceReport>,
    pub overall_health: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceReport {
    pub status: ServiceStatus,
    pub uptime_percentage: f64,
    pub last_heartbeat: Option<DateTime<Utc>>,
}

#[derive(Debug, Error)]
pub enum UptimeError {
    #[error("Service not found: {0}")]
    ServiceNotFound(String),
    #[error("Tracking not started")]
    TrackingNotStarted,
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
    #[error("Persistence error: {0}")]
    PersistenceError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug)]
pub struct UptimeTracker {
    config: UptimeConfig,
    start_time: Arc<Mutex<Option<DateTime<Utc>>>>,
    last_heartbeat: Arc<Mutex<Option<DateTime<Utc>>>>,
    downtime_events: Arc<Mutex<Vec<DowntimeEvent>>>,
    recovery_events: Arc<Mutex<Vec<RecoveryEvent>>>,
    alert_sender: broadcast::Sender<UptimeAlert>,
    current_status: Arc<Mutex<ServiceStatus>>,
    service_heartbeats: Arc<Mutex<HashMap<String, DateTime<Utc>>>>,
    service_status: Arc<Mutex<HashMap<String, ServiceStatus>>>,
    uptime_percentages: Arc<Mutex<HashMap<Duration, f64>>>,
}

impl UptimeTracker {
    pub fn new(config: UptimeConfig) -> Self {
        let (alert_sender, _) = broadcast::channel(100);

        Self {
            config,
            start_time: Arc::new(Mutex::new(None)),
            last_heartbeat: Arc::new(Mutex::new(None)),
            downtime_events: Arc::new(Mutex::new(Vec::new())),
            recovery_events: Arc::new(Mutex::new(Vec::new())),
            alert_sender,
            current_status: Arc::new(Mutex::new(ServiceStatus::Offline)),
            service_heartbeats: Arc::new(Mutex::new(HashMap::new())),
            service_status: Arc::new(Mutex::new(HashMap::new())),
            uptime_percentages: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn start_tracking(&self) -> Result<(), UptimeError> {
        let mut start_time = self.start_time.lock().await;
        *start_time = Some(Utc::now());
        
        let mut status = self.current_status.lock().await;
        *status = ServiceStatus::Online;
        
        Ok(())
    }

    pub async fn get_current_status(&self) -> ServiceStatus {
        self.current_status.lock().await.clone()
    }

    pub async fn get_tracking_start_time(&self) -> Option<DateTime<Utc>> {
        *self.start_time.lock().await
    }

    pub async fn record_heartbeat(&self) -> Result<(), UptimeError> {
        let now = Utc::now();
        let mut last_heartbeat = self.last_heartbeat.lock().await;
        *last_heartbeat = Some(now);
        
        let mut status = self.current_status.lock().await;
        *status = ServiceStatus::Online;
        
        Ok(())
    }

    pub async fn get_last_heartbeat(&self) -> Option<DateTime<Utc>> {
        *self.last_heartbeat.lock().await
    }

    pub async fn check_status(&self) -> ServiceStatus {
        let last_heartbeat = self.last_heartbeat.lock().await;
        
        if let Some(last_beat) = *last_heartbeat {
            let elapsed = Utc::now().signed_duration_since(last_beat);
            if elapsed.num_milliseconds() > self.config.downtime_threshold_ms as i64 {
                let mut status = self.current_status.lock().await;
                *status = ServiceStatus::Offline;
                return ServiceStatus::Offline;
            }
        }
        
        self.current_status.lock().await.clone()
    }

    pub async fn get_downtime_events(&self, duration: Duration) -> Vec<DowntimeEvent> {
        let events = self.downtime_events.lock().await;
        let cutoff = Utc::now() - duration;
        
        events
            .iter()
            .filter(|event| event.start_time >= cutoff)
            .cloned()
            .collect()
    }

    pub async fn set_tracking_start_time(&self, start: DateTime<Utc>) {
        let mut start_time = self.start_time.lock().await;
        *start_time = Some(start);
    }

    pub async fn record_downtime_event(&self, event: DowntimeEvent) -> Result<(), UptimeError> {
        let mut events = self.downtime_events.lock().await;
        events.push(event);
        Ok(())
    }

    pub async fn calculate_uptime_percentage(&self, window: Duration) -> f64 {
        let start_time = self.start_time.lock().await;
        let events = self.downtime_events.lock().await;
        
        let window_start = Utc::now() - window;
        let effective_start = start_time.unwrap_or(window_start).max(window_start);
        
        let total_window_ms = Utc::now().signed_duration_since(effective_start).num_milliseconds() as u64;
        
        let total_downtime_ms: u64 = events
            .iter()
            .filter(|event| event.start_time >= window_start)
            .map(|event| event.duration_ms)
            .sum();
        
        if total_window_ms == 0 {
            100.0
        } else {
            ((total_window_ms - total_downtime_ms) as f64 / total_window_ms as f64) * 100.0
        }
    }

    pub async fn subscribe_to_alerts(&self) -> broadcast::Receiver<UptimeAlert> {
        self.alert_sender.subscribe()
    }

    pub async fn set_uptime_percentage(&self, percentage: f64) {
        // Check thresholds and trigger alerts
        for (threshold, message) in &self.config.alert_thresholds {
            if percentage <= *threshold {
                let alert = UptimeAlert {
                    timestamp: Utc::now(),
                    threshold: *threshold,
                    current_uptime: percentage,
                    message: message.clone(),
                    service: None,
                };
                let _ = self.alert_sender.send(alert);
                break;
            }
        }
    }

    pub async fn set_uptime_for_window(&self, window: Duration, uptime: f64) {
        let mut percentages = self.uptime_percentages.lock().await;
        percentages.insert(window, uptime);
    }

    pub async fn get_uptime_metrics(&self, window: Duration) -> UptimeMetrics {
        let percentages = self.uptime_percentages.lock().await;
        let uptime_percentage = percentages.get(&window).copied()
            .unwrap_or_else(|| {
                // Calculate if not cached
                drop(percentages);
                futures::executor::block_on(self.calculate_uptime_percentage(window))
            });
        
        let events = self.downtime_events.lock().await;
        let window_start = Utc::now() - window;
        
        let window_events: Vec<_> = events
            .iter()
            .filter(|event| event.start_time >= window_start)
            .collect();
        
        let total_downtime_ms = window_events.iter().map(|e| e.duration_ms).sum();
        let last_downtime = window_events.iter().map(|e| e.start_time).max();
        
        UptimeMetrics {
            uptime_percentage,
            window_duration: window,
            total_downtime_ms,
            downtime_events: window_events.len() as u32,
            last_downtime,
        }
    }

    pub async fn get_downtime_statistics(&self, window: Duration) -> DowntimeStatistics {
        let events = self.downtime_events.lock().await;
        let window_start = Utc::now() - window;
        
        let window_events: Vec<_> = events
            .iter()
            .filter(|event| event.start_time >= window_start)
            .collect();
        
        let total_events = window_events.len() as u32;
        let total_downtime_ms = window_events.iter().map(|e| e.duration_ms).sum();
        let average_downtime_ms = if total_events > 0 {
            total_downtime_ms / total_events as u64
        } else {
            0
        };
        
        let mut reasons = HashMap::new();
        for event in &window_events {
            *reasons.entry(event.reason.clone()).or_insert(0) += 1;
        }
        
        DowntimeStatistics {
            total_events,
            total_downtime_ms,
            average_downtime_ms,
            reasons,
        }
    }

    pub async fn get_recovery_events(&self, window: Duration) -> Vec<RecoveryEvent> {
        let events = self.recovery_events.lock().await;
        let cutoff = Utc::now() - window;
        
        events
            .iter()
            .filter(|event| event.timestamp >= cutoff)
            .cloned()
            .collect()
    }

    pub async fn save_metrics(&self) -> Result<(), UptimeError> {
        if !self.config.persist_metrics {
            return Ok(());
        }
        
        let events = self.downtime_events.lock().await;
        let data = serde_json::to_string_pretty(&*events)?;
        tokio::fs::write(&self.config.persistence_path, data).await?;
        
        Ok(())
    }

    pub async fn load_metrics(&self) -> Result<(), UptimeError> {
        if !self.config.persist_metrics {
            return Ok(());
        }
        
        match tokio::fs::read_to_string(&self.config.persistence_path).await {
            Ok(data) => {
                let events: Vec<DowntimeEvent> = serde_json::from_str(&data)?;
                let mut stored_events = self.downtime_events.lock().await;
                *stored_events = events;
                Ok(())
            }
            Err(_) => Ok(()), // File doesn't exist yet
        }
    }

    pub async fn get_historical_uptime(&self, window: Duration) -> HistoricalUptime {
        let end_time = Utc::now();
        let start_time = end_time - window;
        
        // Generate data points every hour
        let mut data_points = Vec::new();
        let mut current = start_time;
        
        while current < end_time {
            let window_duration = Duration::hours(1);
            let uptime = self.calculate_uptime_percentage(window_duration).await;
            
            data_points.push(UptimeDataPoint {
                timestamp: current,
                uptime_percentage: uptime,
                window_hours: 1,
            });
            
            current = current + Duration::hours(1);
        }
        
        HistoricalUptime {
            data_points,
            period_start: start_time,
            period_end: end_time,
        }
    }

    pub async fn start_tracking_service(&self, service: &str) -> Result<(), UptimeError> {
        let mut service_status = self.service_status.lock().await;
        service_status.insert(service.to_string(), ServiceStatus::Online);
        
        let mut service_heartbeats = self.service_heartbeats.lock().await;
        service_heartbeats.insert(service.to_string(), Utc::now());
        
        Ok(())
    }

    pub async fn record_service_heartbeat(&self, service: &str) -> Result<(), UptimeError> {
        let mut service_heartbeats = self.service_heartbeats.lock().await;
        service_heartbeats.insert(service.to_string(), Utc::now());
        
        let mut service_status = self.service_status.lock().await;
        service_status.insert(service.to_string(), ServiceStatus::Online);
        
        Ok(())
    }

    pub async fn get_multi_service_report(&self) -> MultiServiceReport {
        let service_heartbeats = self.service_heartbeats.lock().await;
        let service_status = self.service_status.lock().await;
        
        let mut services = HashMap::new();
        let now = Utc::now();
        
        for (service, last_heartbeat) in service_heartbeats.iter() {
            let elapsed = now.signed_duration_since(*last_heartbeat);
            let status = if elapsed.num_milliseconds() > self.config.downtime_threshold_ms as i64 {
                ServiceStatus::Offline
            } else {
                service_status.get(service).cloned().unwrap_or(ServiceStatus::Online)
            };
            
            let uptime_percentage = if status == ServiceStatus::Online { 100.0 } else { 0.0 };
            
            services.insert(service.clone(), ServiceReport {
                status,
                uptime_percentage,
                last_heartbeat: Some(*last_heartbeat),
            });
        }
        
        let overall_health = if services.is_empty() {
            0.0
        } else {
            let online_count = services.values()
                .filter(|report| report.status == ServiceStatus::Online)
                .count();
            (online_count as f64 / services.len() as f64) * 100.0
        };
        
        MultiServiceReport {
            services,
            overall_health,
        }
    }
}