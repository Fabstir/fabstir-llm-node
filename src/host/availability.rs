use chrono::{DateTime, Datelike, Duration, NaiveTime, Timelike, Utc, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AvailabilitySchedule {
    pub default_status: AvailabilityStatus,
    pub weekly_schedule: Vec<(Weekday, NaiveTime, NaiveTime)>,
    pub timezone: String,
    pub exceptions: Vec<(DateTime<Utc>, DateTime<Utc>, AvailabilityStatus, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AvailabilityStatus {
    Available,
    Unavailable,
    Maintenance,
    ShuttingDown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceWindow {
    pub id: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub description: String,
    pub affects_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityConfig {
    pub max_concurrent_jobs: u32,
    pub reserved_capacity: u32,
    pub max_queue_size: u32,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityUsage {
    pub active_jobs: u32,
    pub available_slots: u32,
    pub queued_jobs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailabilityPeriod {
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub status: AvailabilityStatus,
}

#[derive(Debug, Clone)]
pub struct AvailabilityChange {
    pub timestamp: DateTime<Utc>,
    pub old_status: AvailabilityStatus,
    pub new_status: AvailabilityStatus,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ShutdownHandle {
    pub shutdown_id: String,
    pub initiated_at: DateTime<Utc>,
    pub timeout: Duration,
}

#[derive(Debug, Error)]
pub enum ScheduleError {
    #[error("Invalid schedule configuration: {0}")]
    InvalidConfiguration(String),
    #[error("Capacity exceeded for model: {0}")]
    CapacityExceeded(String),
    #[error("Job not found: {0}")]
    JobNotFound(String),
    #[error("System is shutting down")]
    ShuttingDown,
    #[error("Maintenance window conflict: {0}")]
    MaintenanceConflict(String),
}

#[derive(Debug)]
pub struct AvailabilityManager {
    current_status: AvailabilityStatus,
    schedule: Option<AvailabilitySchedule>,
    maintenance_windows: HashMap<String, MaintenanceWindow>,
    capacity_configs: HashMap<String, CapacityConfig>,
    capacity_usage: HashMap<String, CapacityUsage>,
    job_allocations: HashMap<String, String>, // job_id -> model_id
    change_sender: broadcast::Sender<AvailabilityChange>,
    shutdown_handle: Option<ShutdownHandle>,
}

impl AvailabilityManager {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(100);

        Self {
            current_status: AvailabilityStatus::Available,
            schedule: None,
            maintenance_windows: HashMap::new(),
            capacity_configs: HashMap::new(),
            capacity_usage: HashMap::new(),
            job_allocations: HashMap::new(),
            change_sender: sender,
            shutdown_handle: None,
        }
    }

    pub async fn set_schedule(
        &mut self,
        schedule: AvailabilitySchedule,
    ) -> Result<(), ScheduleError> {
        self.validate_schedule(&schedule)?;
        self.schedule = Some(schedule);
        self.update_current_status().await;
        Ok(())
    }

    pub async fn get_current_status(&self) -> AvailabilityStatus {
        self.current_status.clone()
    }

    pub async fn check_availability_at(&self, time: DateTime<Utc>) -> AvailabilityStatus {
        // Check exceptions first
        if let Some(schedule) = &self.schedule {
            for (start, end, status, _) in &schedule.exceptions {
                if time >= *start && time <= *end {
                    return status.clone();
                }
            }

            // Check weekly schedule
            let weekday = time.weekday();
            let time_of_day = time.time();

            for (day, start_time, end_time) in &schedule.weekly_schedule {
                if weekday == *day && time_of_day >= *start_time && time_of_day <= *end_time {
                    return AvailabilityStatus::Available;
                }
            }

            return AvailabilityStatus::Unavailable;
        }

        AvailabilityStatus::Available
    }

    pub async fn schedule_maintenance(
        &mut self,
        maintenance: MaintenanceWindow,
    ) -> Result<(), ScheduleError> {
        // Check for conflicts
        for existing in self.maintenance_windows.values() {
            if self.windows_overlap(&maintenance, existing) {
                return Err(ScheduleError::MaintenanceConflict(existing.id.clone()));
            }
        }

        self.maintenance_windows
            .insert(maintenance.id.clone(), maintenance);
        Ok(())
    }

    pub async fn get_upcoming_maintenance(&self) -> Vec<MaintenanceWindow> {
        let now = Utc::now();
        self.maintenance_windows
            .values()
            .filter(|window| window.start_time > now)
            .cloned()
            .collect()
    }

    pub async fn set_capacity_config(
        &mut self,
        config: CapacityConfig,
    ) -> Result<(), ScheduleError> {
        for model_id in &config.models {
            self.capacity_usage.insert(
                model_id.clone(),
                CapacityUsage {
                    active_jobs: 0,
                    available_slots: config.max_concurrent_jobs - config.reserved_capacity,
                    queued_jobs: 0,
                },
            );
            self.capacity_configs
                .insert(model_id.clone(), config.clone());
        }
        Ok(())
    }

    pub async fn allocate_capacity(
        &mut self,
        model_id: &str,
        job_id: &str,
    ) -> Result<(), ScheduleError> {
        if self.shutdown_handle.is_some() {
            return Err(ScheduleError::ShuttingDown);
        }

        let config = self
            .capacity_configs
            .get(model_id)
            .ok_or_else(|| ScheduleError::CapacityExceeded(model_id.to_string()))?;

        let usage = self
            .capacity_usage
            .get_mut(model_id)
            .ok_or_else(|| ScheduleError::CapacityExceeded(model_id.to_string()))?;

        let max_usable = config.max_concurrent_jobs - config.reserved_capacity;

        if usage.active_jobs >= max_usable {
            return Err(ScheduleError::CapacityExceeded(model_id.to_string()));
        }

        usage.active_jobs += 1;
        usage.available_slots = max_usable - usage.active_jobs;

        self.job_allocations
            .insert(job_id.to_string(), model_id.to_string());
        Ok(())
    }

    pub async fn release_capacity(
        &mut self,
        model_id: &str,
        job_id: &str,
    ) -> Result<(), ScheduleError> {
        let config = self
            .capacity_configs
            .get(model_id)
            .ok_or_else(|| ScheduleError::JobNotFound(job_id.to_string()))?;

        let usage = self
            .capacity_usage
            .get_mut(model_id)
            .ok_or_else(|| ScheduleError::JobNotFound(job_id.to_string()))?;

        if usage.active_jobs > 0 {
            usage.active_jobs -= 1;
            let max_usable = config.max_concurrent_jobs - config.reserved_capacity;
            usage.available_slots = max_usable - usage.active_jobs;
        }

        self.job_allocations.remove(job_id);
        Ok(())
    }

    pub async fn get_capacity_usage(&self, model_id: &str) -> CapacityUsage {
        self.capacity_usage
            .get(model_id)
            .cloned()
            .unwrap_or(CapacityUsage {
                active_jobs: 0,
                available_slots: 0,
                queued_jobs: 0,
            })
    }

    pub async fn subscribe_to_changes(&self) -> broadcast::Receiver<AvailabilityChange> {
        self.change_sender.subscribe()
    }

    pub async fn set_status(&mut self, status: AvailabilityStatus) -> Result<(), ScheduleError> {
        let old_status = self.current_status.clone();
        self.current_status = status.clone();

        let change = AvailabilityChange {
            timestamp: Utc::now(),
            old_status,
            new_status: status,
            reason: "Manual status change".to_string(),
        };

        let _ = self.change_sender.send(change);
        Ok(())
    }

    pub async fn initiate_graceful_shutdown(
        &mut self,
        timeout: Duration,
    ) -> Result<ShutdownHandle, ScheduleError> {
        let handle = ShutdownHandle {
            shutdown_id: uuid::Uuid::new_v4().to_string(),
            initiated_at: Utc::now(),
            timeout,
        };

        self.shutdown_handle = Some(handle.clone());
        self.set_status(AvailabilityStatus::ShuttingDown).await?;

        Ok(handle)
    }

    pub async fn get_availability_forecast(&self, duration: Duration) -> Vec<AvailabilityPeriod> {
        let mut forecast = Vec::new();
        let start_time = Utc::now();
        let end_time = start_time + duration;

        // Current period
        let current_end = self.find_next_status_change(start_time).unwrap_or(end_time);
        forecast.push(AvailabilityPeriod {
            start_time,
            end_time: current_end.min(end_time),
            status: self.current_status.clone(),
        });

        // Add maintenance windows within the forecast period
        for window in self.maintenance_windows.values() {
            if window.start_time >= start_time && window.start_time <= end_time {
                forecast.push(AvailabilityPeriod {
                    start_time: window.start_time,
                    end_time: window.end_time.min(end_time),
                    status: AvailabilityStatus::Maintenance,
                });
            }
        }

        // Sort by start time
        forecast.sort_by(|a, b| a.start_time.cmp(&b.start_time));
        forecast
    }

    async fn update_current_status(&mut self) {
        let now = Utc::now();
        let new_status = self.check_availability_at(now).await;

        if new_status != self.current_status {
            let old_status = self.current_status.clone();
            self.current_status = new_status.clone();

            let change = AvailabilityChange {
                timestamp: now,
                old_status,
                new_status,
                reason: "Scheduled status change".to_string(),
            };

            let _ = self.change_sender.send(change);
        }
    }

    fn validate_schedule(&self, schedule: &AvailabilitySchedule) -> Result<(), ScheduleError> {
        // Validate weekly schedule
        for (_, start, end) in &schedule.weekly_schedule {
            if start >= end {
                return Err(ScheduleError::InvalidConfiguration(
                    "Start time must be before end time".to_string(),
                ));
            }
        }

        // Validate exceptions
        for (start, end, _, _) in &schedule.exceptions {
            if start >= end {
                return Err(ScheduleError::InvalidConfiguration(
                    "Exception start time must be before end time".to_string(),
                ));
            }
        }

        Ok(())
    }

    fn windows_overlap(&self, window1: &MaintenanceWindow, window2: &MaintenanceWindow) -> bool {
        window1.start_time < window2.end_time && window1.end_time > window2.start_time
    }

    fn find_next_status_change(&self, from_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut next_change: Option<DateTime<Utc>> = None;

        // Check maintenance windows
        for window in self.maintenance_windows.values() {
            if window.start_time > from_time {
                next_change = Some(match next_change {
                    Some(current) => current.min(window.start_time),
                    None => window.start_time,
                });
            }
        }

        next_change
    }
}

impl Default for AvailabilityManager {
    fn default() -> Self {
        Self::new()
    }
}
