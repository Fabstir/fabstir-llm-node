use fabstir_llm_node::qa::{
    UptimeTracker, UptimeMetrics, DowntimeEvent, UptimeAlert,
    ServiceStatus, UptimeConfig, UptimeError, HistoricalUptime
};
use chrono::{DateTime, Utc, Duration};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_config() -> UptimeConfig {
        UptimeConfig {
            check_interval_ms: 1000, // 1 second
            downtime_threshold_ms: 5000, // 5 seconds
            alert_thresholds: vec![
                (99.9, "Critical: Below 99.9% uptime".to_string()),
                (99.5, "Warning: Below 99.5% uptime".to_string()),
                (99.0, "Notice: Below 99% uptime".to_string()),
            ],
            rolling_window_hours: 24,
            persist_metrics: true,
            persistence_path: "/tmp/uptime_metrics".to_string(),
        }
    }

    #[tokio::test]
    async fn test_start_uptime_tracking() {
        let config = create_test_config();
        let tracker = UptimeTracker::new(config);
        
        let result = tracker.start_tracking().await;
        assert!(result.is_ok());
        
        let status = tracker.get_current_status().await;
        assert_eq!(status, ServiceStatus::Online);
        
        // Should have initial timestamp
        let start_time = tracker.get_tracking_start_time().await;
        assert!(start_time.is_some());
    }

    #[tokio::test]
    async fn test_record_heartbeat() {
        let config = create_test_config();
        let tracker = UptimeTracker::new(config);
        
        tracker.start_tracking().await.unwrap();
        
        // Record multiple heartbeats
        for _ in 0..5 {
            let result = tracker.record_heartbeat().await;
            assert!(result.is_ok());
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        
        let last_heartbeat = tracker.get_last_heartbeat().await;
        assert!(last_heartbeat.is_some());
        assert!(Utc::now().signed_duration_since(last_heartbeat.unwrap()).num_seconds() < 1);
    }

    #[tokio::test]
    async fn test_detect_downtime() {
        let mut config = create_test_config();
        config.downtime_threshold_ms = 100; // Very short for testing
        
        let tracker = UptimeTracker::new(config);
        tracker.start_tracking().await.unwrap();
        
        // Record initial heartbeat
        tracker.record_heartbeat().await.unwrap();
        
        // Wait longer than threshold
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        
        // Check should detect downtime
        let status = tracker.check_status().await;
        assert_eq!(status, ServiceStatus::Offline);
        
        let downtime_events = tracker.get_downtime_events(Duration::hours(1)).await;
        assert!(!downtime_events.is_empty());
    }

    #[tokio::test]
    async fn test_calculate_uptime_percentage() {
        let config = create_test_config();
        let tracker = UptimeTracker::new(config);
        
        tracker.start_tracking().await.unwrap();
        
        // Simulate 1 hour of operation with 5 minutes downtime
        let start = Utc::now() - Duration::hours(1);
        tracker.set_tracking_start_time(start).await;
        
        // Add downtime event
        let downtime = DowntimeEvent {
            start_time: start + Duration::minutes(30),
            end_time: Some(start + Duration::minutes(35)),
            duration_ms: 300_000, // 5 minutes
            reason: "Network interruption".to_string(),
        };
        
        tracker.record_downtime_event(downtime).await.unwrap();
        
        let uptime_pct = tracker.calculate_uptime_percentage(Duration::hours(1)).await;
        assert!((uptime_pct - 91.67).abs() < 0.01); // ~91.67% uptime
    }

    #[tokio::test]
    async fn test_uptime_alerts() {
        let config = create_test_config();
        let tracker = UptimeTracker::new(config);
        
        tracker.start_tracking().await.unwrap();
        
        // Subscribe to alerts
        let mut alert_receiver = tracker.subscribe_to_alerts().await;
        
        // Simulate poor uptime (90%)
        tracker.set_uptime_percentage(90.0).await;
        
        // Should trigger alerts
        let alert = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            alert_receiver.recv()
        ).await;
        
        assert!(alert.is_ok());
        let alert_data = alert.unwrap().unwrap();
        assert!(alert_data.message.contains("Below"));
        assert_eq!(alert_data.current_uptime, 90.0);
    }

    #[tokio::test]
    async fn test_rolling_window_metrics() {
        let config = create_test_config();
        let tracker = UptimeTracker::new(config);
        
        tracker.start_tracking().await.unwrap();
        
        // Simulate metrics over time
        let windows = vec![
            (Duration::hours(1), 99.9),
            (Duration::hours(6), 99.5),
            (Duration::hours(12), 99.0),
            (Duration::hours(24), 98.5),
            (Duration::days(7), 98.0),
            (Duration::days(30), 97.5),
        ];
        
        for (window, expected_uptime) in windows {
            tracker.set_uptime_for_window(window, expected_uptime).await;
            
            let metrics = tracker.get_uptime_metrics(window).await;
            assert!((metrics.uptime_percentage - expected_uptime).abs() < 0.01);
            assert_eq!(metrics.window_duration, window);
        }
    }

    #[tokio::test]
    async fn test_downtime_aggregation() {
        let config = create_test_config();
        let tracker = UptimeTracker::new(config);
        
        tracker.start_tracking().await.unwrap();
        
        // Add multiple downtime events
        let events = vec![
            ("Network failure", 120_000),      // 2 minutes
            ("Hardware maintenance", 300_000), // 5 minutes
            ("Software update", 180_000),      // 3 minutes
        ];
        
        for (reason, duration_ms) in events {
            let event = DowntimeEvent {
                start_time: Utc::now() - Duration::milliseconds(duration_ms as i64),
                end_time: Some(Utc::now()),
                duration_ms,
                reason: reason.to_string(),
            };
            tracker.record_downtime_event(event).await.unwrap();
        }
        
        let stats = tracker.get_downtime_statistics(Duration::hours(24)).await;
        assert_eq!(stats.total_events, 3);
        assert_eq!(stats.total_downtime_ms, 600_000); // 10 minutes total
        assert!(stats.reasons.contains_key("Network failure"));
        assert_eq!(stats.average_downtime_ms, 200_000); // 3.33 minutes average
    }

    #[tokio::test]
    async fn test_service_recovery_detection() {
        let mut config = create_test_config();
        config.downtime_threshold_ms = 100;
        
        let tracker = UptimeTracker::new(config);
        tracker.start_tracking().await.unwrap();
        
        // Go offline
        tracker.record_heartbeat().await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        assert_eq!(tracker.check_status().await, ServiceStatus::Offline);
        
        // Come back online
        tracker.record_heartbeat().await.unwrap();
        assert_eq!(tracker.check_status().await, ServiceStatus::Online);
        
        // Check recovery was recorded
        let events = tracker.get_recovery_events(Duration::hours(1)).await;
        assert_eq!(events.len(), 1);
        assert!(events[0].recovery_time_ms > 0);
    }

    #[tokio::test]
    async fn test_uptime_persistence() {
        let config = create_test_config();
        let tracker = UptimeTracker::new(config.clone());
        
        tracker.start_tracking().await.unwrap();
        
        // Add some metrics
        for i in 0..5 {
            tracker.record_heartbeat().await.unwrap();
            if i == 2 {
                // Simulate downtime
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
        
        // Save metrics
        let result = tracker.save_metrics().await;
        assert!(result.is_ok());
        
        // Create new tracker and load metrics
        let new_tracker = UptimeTracker::new(config);
        let result = new_tracker.load_metrics().await;
        assert!(result.is_ok());
        
        // Verify metrics were restored
        let historical = new_tracker.get_historical_uptime(Duration::hours(24)).await;
        assert!(!historical.data_points.is_empty());
    }

    #[tokio::test]
    async fn test_multi_service_tracking() {
        let config = create_test_config();
        let tracker = UptimeTracker::new(config);
        
        // Track multiple services
        let services = vec!["inference", "p2p", "contracts"];
        
        for service in &services {
            tracker.start_tracking_service(service).await.unwrap();
        }
        
        // Record different uptime for each
        tracker.record_service_heartbeat("inference").await.unwrap();
        tracker.record_service_heartbeat("p2p").await.unwrap();
        // Don't record for contracts (simulate it being down)
        
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        
        let report = tracker.get_multi_service_report().await;
        assert_eq!(report.services.len(), 3);
        assert_eq!(report.services["inference"].status, ServiceStatus::Online);
        assert_eq!(report.services["p2p"].status, ServiceStatus::Online);
        assert_eq!(report.services["contracts"].status, ServiceStatus::Offline);
    }
}