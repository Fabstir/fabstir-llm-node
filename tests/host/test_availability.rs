use chrono::{DateTime, Duration, NaiveTime, Utc, Weekday};
use fabstir_llm_node::host::{
    AvailabilityManager, AvailabilitySchedule, AvailabilityStatus, CapacityConfig,
    MaintenanceWindow, ScheduleError,
};
use std::collections::HashSet;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_schedule() -> AvailabilitySchedule {
        AvailabilitySchedule {
            default_status: AvailabilityStatus::Available,
            weekly_schedule: vec![
                (
                    Weekday::Mon,
                    NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                    NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
                ),
                (
                    Weekday::Tue,
                    NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                    NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
                ),
                (
                    Weekday::Wed,
                    NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                    NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
                ),
                (
                    Weekday::Thu,
                    NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                    NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
                ),
                (
                    Weekday::Fri,
                    NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                    NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
                ),
            ],
            timezone: "UTC".to_string(),
            exceptions: vec![],
        }
    }

    #[tokio::test]
    async fn test_set_availability_schedule() {
        let mut manager = AvailabilityManager::new();
        let schedule = create_test_schedule();

        let result = manager.set_schedule(schedule).await;
        assert!(result.is_ok());

        let current_status = manager.get_current_status().await;
        assert!(matches!(
            current_status,
            AvailabilityStatus::Available | AvailabilityStatus::Unavailable
        ));
    }

    #[tokio::test]
    async fn test_check_availability_at_time() {
        let mut manager = AvailabilityManager::new();
        let schedule = create_test_schedule();

        manager.set_schedule(schedule).await.unwrap();

        // Test during business hours (Monday 10:00 UTC)
        let monday_10am = DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let status = manager.check_availability_at(monday_10am).await;
        assert_eq!(status, AvailabilityStatus::Available);

        // Test outside business hours (Monday 20:00 UTC)
        let monday_8pm = DateTime::parse_from_rfc3339("2024-01-15T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let status = manager.check_availability_at(monday_8pm).await;
        assert_eq!(status, AvailabilityStatus::Unavailable);

        // Test weekend (Saturday)
        let saturday = DateTime::parse_from_rfc3339("2024-01-13T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let status = manager.check_availability_at(saturday).await;
        assert_eq!(status, AvailabilityStatus::Unavailable);
    }

    #[tokio::test]
    async fn test_schedule_maintenance_window() {
        let mut manager = AvailabilityManager::new();
        let schedule = create_test_schedule();

        manager.set_schedule(schedule).await.unwrap();

        let maintenance = MaintenanceWindow {
            id: "maint-001".to_string(),
            start_time: Utc::now() + Duration::hours(1),
            end_time: Utc::now() + Duration::hours(3),
            description: "GPU driver update".to_string(),
            affects_models: vec!["llama-3.2-1b".to_string()],
        };

        let result = manager.schedule_maintenance(maintenance).await;
        assert!(result.is_ok());

        let upcoming = manager.get_upcoming_maintenance().await;
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].id, "maint-001");
    }

    #[tokio::test]
    async fn test_capacity_management() {
        let mut manager = AvailabilityManager::new();

        let capacity = CapacityConfig {
            max_concurrent_jobs: 10,
            reserved_capacity: 2, // Always keep 2 slots free
            max_queue_size: 50,
            models: vec!["llama-3.2-1b".to_string()],
        };

        manager.set_capacity_config(capacity).await.unwrap();

        // Simulate job allocation
        for i in 0..8 {
            let result = manager
                .allocate_capacity("llama-3.2-1b", &format!("job-{}", i))
                .await;
            assert!(result.is_ok());
        }

        // Should fail after reaching limit (10 - 2 reserved = 8)
        let result = manager.allocate_capacity("llama-3.2-1b", "job-9").await;
        assert!(result.is_err());

        // Check current usage
        let usage = manager.get_capacity_usage("llama-3.2-1b").await;
        assert_eq!(usage.active_jobs, 8);
        assert_eq!(usage.available_slots, 0);

        // Release capacity
        manager
            .release_capacity("llama-3.2-1b", "job-0")
            .await
            .unwrap();

        let usage = manager.get_capacity_usage("llama-3.2-1b").await;
        assert_eq!(usage.active_jobs, 7);
        assert_eq!(usage.available_slots, 1);
    }

    #[tokio::test]
    async fn test_availability_notifications() {
        let mut manager = AvailabilityManager::new();
        let schedule = create_test_schedule();

        manager.set_schedule(schedule).await.unwrap();

        // Subscribe to availability changes
        let mut receiver = manager.subscribe_to_changes().await;

        // Trigger a change
        manager
            .set_status(AvailabilityStatus::Maintenance)
            .await
            .unwrap();

        let notification = receiver.recv().await;
        assert!(notification.is_ok());
        assert_eq!(
            notification.unwrap().new_status,
            AvailabilityStatus::Maintenance
        );
    }

    #[tokio::test]
    async fn test_availability_exceptions() {
        let mut manager = AvailabilityManager::new();
        let mut schedule = create_test_schedule();

        // Add holiday exception
        schedule.exceptions.push((
            DateTime::parse_from_rfc3339("2024-12-25T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            DateTime::parse_from_rfc3339("2024-12-25T23:59:59Z")
                .unwrap()
                .with_timezone(&Utc),
            AvailabilityStatus::Unavailable,
            "Christmas Holiday".to_string(),
        ));

        manager.set_schedule(schedule).await.unwrap();

        let christmas = DateTime::parse_from_rfc3339("2024-12-25T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let status = manager.check_availability_at(christmas).await;
        assert_eq!(status, AvailabilityStatus::Unavailable);
    }

    #[tokio::test]
    async fn test_graceful_shutdown() {
        let mut manager = AvailabilityManager::new();

        // Simulate active jobs
        manager
            .set_capacity_config(CapacityConfig {
                max_concurrent_jobs: 10,
                reserved_capacity: 0,
                max_queue_size: 0,
                models: vec!["llama-3.2-1b".to_string()],
            })
            .await
            .unwrap();

        manager
            .allocate_capacity("llama-3.2-1b", "job-1")
            .await
            .unwrap();
        manager
            .allocate_capacity("llama-3.2-1b", "job-2")
            .await
            .unwrap();

        // Initiate graceful shutdown
        let shutdown_handle = manager
            .initiate_graceful_shutdown(Duration::minutes(5))
            .await;

        assert!(shutdown_handle.is_ok());

        // Should not accept new jobs
        let result = manager.allocate_capacity("llama-3.2-1b", "job-3").await;
        assert!(matches!(result, Err(ScheduleError::ShuttingDown)));

        // Check shutdown status
        let status = manager.get_current_status().await;
        assert_eq!(status, AvailabilityStatus::ShuttingDown);
    }

    #[tokio::test]
    async fn test_availability_forecast() {
        let mut manager = AvailabilityManager::new();
        let schedule = create_test_schedule();

        manager.set_schedule(schedule).await.unwrap();

        // Schedule maintenance
        let maintenance = MaintenanceWindow {
            id: "maint-002".to_string(),
            start_time: Utc::now() + Duration::hours(24),
            end_time: Utc::now() + Duration::hours(26),
            description: "Scheduled update".to_string(),
            affects_models: vec!["llama-3.2-1b".to_string()],
        };

        manager.schedule_maintenance(maintenance).await.unwrap();

        // Get forecast for next 48 hours
        let forecast = manager.get_availability_forecast(Duration::hours(48)).await;

        assert!(!forecast.is_empty());

        // Should include maintenance window
        let has_maintenance = forecast
            .iter()
            .any(|period| matches!(period.status, AvailabilityStatus::Maintenance));
        assert!(has_maintenance);
    }
}
