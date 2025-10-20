// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::host::{
    AlertLevel, AlertThreshold, CpuMetrics, GpuMetrics, MemoryMetrics, MonitoringError,
    NetworkMetrics, ResourceAlert, ResourceMetrics, ResourceMonitor,
};
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_thresholds() -> Vec<AlertThreshold> {
        vec![
            AlertThreshold {
                metric: "gpu_usage".to_string(),
                level: AlertLevel::Warning,
                value: 80.0,
                duration: Duration::from_secs(60),
            },
            AlertThreshold {
                metric: "gpu_usage".to_string(),
                level: AlertLevel::Critical,
                value: 95.0,
                duration: Duration::from_secs(30),
            },
            AlertThreshold {
                metric: "gpu_memory".to_string(),
                level: AlertLevel::Warning,
                value: 90.0,
                duration: Duration::from_secs(60),
            },
            AlertThreshold {
                metric: "gpu_temperature".to_string(),
                level: AlertLevel::Critical,
                value: 85.0,
                duration: Duration::from_secs(10),
            },
        ]
    }

    #[tokio::test]
    async fn test_initialize_resource_monitor() {
        let mut monitor = ResourceMonitor::new();

        let result = monitor.initialize().await;
        assert!(result.is_ok());

        let gpus = monitor.list_gpus().await;
        assert!(!gpus.is_empty());
    }

    #[tokio::test]
    async fn test_get_gpu_metrics() {
        let mut monitor = ResourceMonitor::new();
        monitor.initialize().await.unwrap();

        let metrics = monitor.get_gpu_metrics(0).await; // GPU 0

        assert!(metrics.is_ok());
        let gpu_metrics = metrics.unwrap();

        assert!(gpu_metrics.usage_percent >= 0.0 && gpu_metrics.usage_percent <= 100.0);
        assert!(gpu_metrics.memory_used_mb <= gpu_metrics.memory_total_mb);
        assert!(gpu_metrics.temperature_celsius > 0.0 && gpu_metrics.temperature_celsius < 100.0);
        assert!(gpu_metrics.power_draw_watts >= 0.0);
        assert!(!gpu_metrics.name.is_empty());
    }

    #[tokio::test]
    async fn test_get_cpu_metrics() {
        let mut monitor = ResourceMonitor::new();
        monitor.initialize().await.unwrap();

        let metrics = monitor.get_cpu_metrics().await;

        assert!(metrics.is_ok());
        let cpu_metrics = metrics.unwrap();

        assert!(cpu_metrics.usage_percent >= 0.0 && cpu_metrics.usage_percent <= 100.0);
        assert!(cpu_metrics.core_count > 0);
        assert!(!cpu_metrics.per_core_usage.is_empty());
        assert!(cpu_metrics.temperature_celsius.is_some());
        assert!(cpu_metrics.frequency_mhz > 0.0);
    }

    #[tokio::test]
    async fn test_get_memory_metrics() {
        let mut monitor = ResourceMonitor::new();
        monitor.initialize().await.unwrap();

        let metrics = monitor.get_memory_metrics().await;

        assert!(metrics.is_ok());
        let mem_metrics = metrics.unwrap();

        assert!(mem_metrics.used_mb <= mem_metrics.total_mb);
        assert!(mem_metrics.available_mb <= mem_metrics.total_mb);
        assert!(mem_metrics.usage_percent >= 0.0 && mem_metrics.usage_percent <= 100.0);
        assert!(mem_metrics.swap_used_mb <= mem_metrics.swap_total_mb);
    }

    #[tokio::test]
    async fn test_get_network_metrics() {
        let mut monitor = ResourceMonitor::new();
        monitor.initialize().await.unwrap();

        // Record initial state
        tokio::time::sleep(Duration::from_secs(1)).await;

        let metrics = monitor.get_network_metrics().await;

        assert!(metrics.is_ok());
        let net_metrics = metrics.unwrap();

        assert!(net_metrics.bytes_sent >= 0);
        assert!(net_metrics.bytes_received >= 0);
        assert!(net_metrics.packets_sent >= 0);
        assert!(net_metrics.packets_received >= 0);
        assert!(net_metrics.bandwidth_mbps >= 0.0);
    }

    #[tokio::test]
    async fn test_continuous_monitoring() {
        let mut monitor = ResourceMonitor::new();
        monitor.initialize().await.unwrap();

        // Start monitoring with 100ms interval
        let handle = monitor.start_monitoring(Duration::from_millis(100)).await;
        assert!(handle.is_ok());

        // Let it collect some data
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Get historical data
        let history = monitor.get_metrics_history(Duration::from_secs(1)).await;
        assert!(history.len() >= 4); // Should have at least 4 data points

        // Stop monitoring
        monitor.stop_monitoring().await.unwrap();
    }

    #[tokio::test]
    async fn test_alert_thresholds() {
        let mut monitor = ResourceMonitor::new();
        monitor.initialize().await.unwrap();

        let thresholds = create_test_thresholds();
        for threshold in thresholds {
            monitor.add_alert_threshold(threshold).await.unwrap();
        }

        // Subscribe to alerts
        let mut alert_receiver = monitor.subscribe_to_alerts().await;

        // Simulate high GPU usage
        monitor.simulate_metric("gpu_usage", 96.0).await;

        // Should receive critical alert
        let alert = tokio::time::timeout(Duration::from_secs(1), alert_receiver.recv()).await;

        assert!(alert.is_ok());
        let alert_data = alert.unwrap().unwrap();
        assert_eq!(alert_data.level, AlertLevel::Critical);
        assert_eq!(alert_data.metric, "gpu_usage");
    }

    #[tokio::test]
    async fn test_resource_allocation_tracking() {
        let mut monitor = ResourceMonitor::new();
        monitor.initialize().await.unwrap();

        // Track job resource allocation
        let job_id = "job-123";
        monitor.allocate_resources(job_id, 4096, 4).await.unwrap(); // 4GB RAM, 4 CPU cores

        let allocation = monitor.get_job_allocation(job_id).await;
        assert!(allocation.is_some());
        let allocation = allocation.unwrap();
        assert_eq!(allocation.memory_mb, 4096);
        assert_eq!(allocation.cpu_cores, 4);

        // Get total allocated resources
        let total = monitor.get_total_allocated_resources().await;
        assert_eq!(total.memory_mb, 4096);
        assert_eq!(total.cpu_cores, 4);

        // Release resources
        monitor.release_resources(job_id).await.unwrap();

        let total = monitor.get_total_allocated_resources().await;
        assert_eq!(total.memory_mb, 0);
        assert_eq!(total.cpu_cores, 0);
    }

    #[tokio::test]
    async fn test_resource_usage_summary() {
        let mut monitor = ResourceMonitor::new();
        monitor.initialize().await.unwrap();

        let summary = monitor.get_resource_summary().await;

        assert!(summary.is_ok());
        let summary_data = summary.unwrap();

        assert!(summary_data.timestamp > 0);
        assert!(summary_data.cpu_usage >= 0.0);
        assert!(summary_data.memory_usage >= 0.0);
        assert!(!summary_data.gpu_usage.is_empty());
        assert!(summary_data.network_bandwidth >= 0.0);
        assert!(summary_data.health_score >= 0.0 && summary_data.health_score <= 100.0);
    }

    #[tokio::test]
    async fn test_export_metrics() {
        let mut monitor = ResourceMonitor::new();
        monitor.initialize().await.unwrap();

        // Collect some metrics
        monitor
            .start_monitoring(Duration::from_millis(100))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        monitor.stop_monitoring().await.unwrap();

        // Export to different formats
        let json_export = monitor.export_metrics_json(Duration::from_secs(1)).await;
        assert!(json_export.is_ok());
        assert!(!json_export.unwrap().is_empty());

        let csv_export = monitor.export_metrics_csv(Duration::from_secs(1)).await;
        assert!(csv_export.is_ok());
        assert!(!csv_export.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_predictive_alerts() {
        let mut monitor = ResourceMonitor::new();
        monitor.initialize().await.unwrap();

        // Enable predictive monitoring
        monitor.enable_predictive_monitoring(true).await;

        // Simulate increasing memory usage pattern
        for i in 0..10 {
            monitor
                .simulate_metric("memory_usage", 50.0 + (i as f64) * 4.0)
                .await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Check for predictive alert
        let prediction = monitor
            .get_resource_prediction("memory_usage", Duration::from_secs(300))
            .await;

        assert!(prediction.is_ok());
        let pred_data = prediction.unwrap();
        assert!(pred_data.predicted_value > 85.0); // Should predict high usage
        assert!(pred_data.confidence > 0.7); // Should have reasonable confidence
    }
}
