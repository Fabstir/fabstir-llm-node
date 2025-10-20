// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::qa::{
    LatencyBucket, MetricsAggregation, ModelPerformance, PerformanceAlert, ResponseMetrics,
    ResponseTimeConfig, ResponseTimeError, ResponseTimeTracker,
};
use std::collections::HashMap;
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> ResponseTimeConfig {
        ResponseTimeConfig {
            buckets_ms: vec![10, 50, 100, 250, 500, 1000, 2500, 5000],
            percentiles: vec![50.0, 90.0, 95.0, 99.0, 99.9],
            sliding_window_size: 1000,
            alert_threshold_p99_ms: 1000,
            track_by_model: true,
            track_by_operation: true,
            export_interval_sec: 60,
        }
    }

    #[tokio::test]
    async fn test_record_response_time() {
        let config = create_test_config();
        let tracker = ResponseTimeTracker::new(config);

        // Record various response times
        let times = vec![45, 120, 67, 230, 98, 450, 88, 156, 203, 99];

        for time_ms in times {
            let result = tracker
                .record_response_time("llama-3.2-1b", "inference", time_ms)
                .await;
            assert!(result.is_ok());
        }

        let metrics = tracker.get_current_metrics().await;
        assert_eq!(metrics.count, 10);
        assert!(metrics.average_ms > 0.0);
        assert!(metrics.min_ms > 0);
        assert!(metrics.max_ms <= 450);
    }

    #[tokio::test]
    async fn test_calculate_percentiles() {
        let config = create_test_config();
        let tracker = ResponseTimeTracker::new(config);

        // Generate 1000 response times with known distribution
        for i in 0..1000 {
            let time_ms = match i {
                0..=500 => 50,    // 50% at 50ms
                501..=900 => 100, // 40% at 100ms
                901..=950 => 500, // 5% at 500ms
                _ => 1000,        // 5% at 1000ms
            };
            tracker
                .record_response_time("model", "op", time_ms)
                .await
                .unwrap();
        }

        let percentiles = tracker.calculate_percentiles().await;

        assert!((percentiles.p50 - 50.0).abs() < 10.0);
        assert!((percentiles.p90 - 100.0).abs() < 10.0);
        assert!((percentiles.p95 - 500.0).abs() < 50.0);
        assert!((percentiles.p99 - 1000.0).abs() < 100.0);
    }

    #[tokio::test]
    async fn test_latency_buckets() {
        let config = create_test_config();
        let tracker = ResponseTimeTracker::new(config);

        // Record times that fall into different buckets
        let test_times = vec![
            (5, "0-10ms"),
            (25, "10-50ms"),
            (75, "50-100ms"),
            (150, "100-250ms"),
            (350, "250-500ms"),
            (750, "500-1000ms"),
            (1500, "1000-2500ms"),
            (3000, "2500-5000ms"),
            (6000, ">5000ms"),
        ];

        for (time_ms, _bucket) in test_times {
            tracker
                .record_response_time("model", "op", time_ms)
                .await
                .unwrap();
        }

        let distribution = tracker.get_latency_distribution().await;

        // Verify each bucket has expected count
        assert_eq!(distribution.buckets.len(), 9);
        for bucket in &distribution.buckets {
            if bucket.max_ms.is_some() {
                assert_eq!(bucket.count, 1);
            }
        }
        assert_eq!(distribution.total_count, 9);
    }

    #[tokio::test]
    async fn test_model_specific_metrics() {
        let config = create_test_config();
        let tracker = ResponseTimeTracker::new(config);

        // Record times for different models
        let models = vec![
            ("llama-3.2-1b", vec![50, 60, 55, 65, 70]),
            ("mistral-7b", vec![100, 120, 110, 130, 125]),
            ("llama-70b", vec![500, 550, 520, 580, 560]),
        ];

        for (model, times) in models {
            for time_ms in times {
                tracker
                    .record_response_time(model, "inference", time_ms)
                    .await
                    .unwrap();
            }
        }

        // Get model-specific metrics
        let llama_small = tracker.get_model_metrics("llama-3.2-1b").await;
        let mistral = tracker.get_model_metrics("mistral-7b").await;
        let llama_large = tracker.get_model_metrics("llama-70b").await;

        assert!(llama_small.average_ms < 70.0);
        assert!(mistral.average_ms > 100.0 && mistral.average_ms < 130.0);
        assert!(llama_large.average_ms > 500.0);
    }

    #[tokio::test]
    async fn test_operation_breakdown() {
        let config = create_test_config();
        let tracker = ResponseTimeTracker::new(config);

        // Record times for different operations
        let operations = vec![
            ("tokenization", vec![10, 12, 11, 13, 9]),
            ("inference", vec![200, 220, 210, 230, 215]),
            ("detokenization", vec![5, 6, 4, 7, 5]),
        ];

        for (op, times) in operations {
            for time_ms in times {
                tracker
                    .record_response_time("model", op, time_ms)
                    .await
                    .unwrap();
            }
        }

        let breakdown = tracker.get_operation_breakdown("model").await;

        assert_eq!(breakdown.operations.len(), 3);
        assert!(breakdown.operations["tokenization"].average_ms < 15.0);
        assert!(breakdown.operations["inference"].average_ms > 200.0);
        assert!(breakdown.operations["detokenization"].average_ms < 10.0);
    }

    #[tokio::test]
    async fn test_performance_alerts() {
        let mut config = create_test_config();
        config.alert_threshold_p99_ms = 500;

        let tracker = ResponseTimeTracker::new(config);

        // Subscribe to alerts
        let mut alert_receiver = tracker.subscribe_to_alerts().await;

        // Record mostly fast responses
        for _ in 0..90 {
            tracker
                .record_response_time("model", "op", 100)
                .await
                .unwrap();
        }

        // Record some slow responses that push p99 over threshold
        for _ in 0..10 {
            tracker
                .record_response_time("model", "op", 600)
                .await
                .unwrap();
        }

        // Should receive performance alert
        let alert = tokio::time::timeout(Duration::from_secs(1), alert_receiver.recv()).await;

        assert!(alert.is_ok());
        let alert_data = alert.unwrap().unwrap();
        assert!(alert_data.metric_value > 500.0);
        assert_eq!(alert_data.threshold, 500.0);
    }

    #[tokio::test]
    async fn test_sliding_window() {
        let mut config = create_test_config();
        config.sliding_window_size = 10;

        let tracker = ResponseTimeTracker::new(config);

        // Fill window with slow responses
        for _ in 0..10 {
            tracker
                .record_response_time("model", "op", 1000)
                .await
                .unwrap();
        }

        let metrics = tracker.get_current_metrics().await;
        assert!(metrics.average_ms > 900.0);

        // Add fast responses, pushing out slow ones
        for _ in 0..10 {
            tracker
                .record_response_time("model", "op", 50)
                .await
                .unwrap();
        }

        let metrics = tracker.get_current_metrics().await;
        assert!(metrics.average_ms < 100.0);
    }

    #[tokio::test]
    async fn test_time_series_data() {
        let config = create_test_config();
        let tracker = ResponseTimeTracker::new(config);

        // Record data over time
        for minute in 0..5 {
            let base_time = 50 + (minute * 20);
            for _ in 0..10 {
                tracker
                    .record_response_time("model", "op", base_time + rand::random::<u64>() % 20)
                    .await
                    .unwrap();
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let time_series = tracker
            .get_time_series_data(Duration::from_secs(1), 5)
            .await;

        assert_eq!(time_series.len(), 5);

        // Verify increasing trend
        for i in 1..time_series.len() {
            assert!(time_series[i].average_ms >= time_series[i - 1].average_ms);
        }
    }

    #[tokio::test]
    async fn test_export_metrics() {
        let config = create_test_config();
        let tracker = ResponseTimeTracker::new(config);

        // Generate some data
        for _ in 0..100 {
            tracker
                .record_response_time("model", "inference", 50 + rand::random::<u64>() % 100)
                .await
                .unwrap();
        }

        // Export in different formats
        let prometheus = tracker.export_prometheus_format().await;
        assert!(prometheus.contains("response_time_milliseconds"));
        assert!(prometheus.contains("quantile=\"0.5\""));

        let json = tracker.export_json_format().await;
        assert!(json.is_ok());
        let parsed: serde_json::Value = serde_json::from_str(&json.unwrap()).unwrap();
        assert!(parsed["metrics"]["count"].as_u64().unwrap() == 100);
    }

    #[tokio::test]
    async fn test_comparison_metrics() {
        let config = create_test_config();
        let tracker = ResponseTimeTracker::new(config);

        // Record baseline period
        for _ in 0..100 {
            tracker
                .record_response_time("model", "op", 100)
                .await
                .unwrap();
        }

        let baseline = tracker.capture_baseline("v1.0").await;

        // Record new version with improved performance
        for _ in 0..100 {
            tracker
                .record_response_time("model", "op", 80)
                .await
                .unwrap();
        }

        let comparison = tracker.compare_with_baseline("v1.0").await;

        assert!(comparison.is_ok());
        let comp_data = comparison.unwrap();
        assert!(comp_data.improvement_percentage > 15.0);
        assert!(comp_data.p50_improvement > 0.0);
        assert!(comp_data.p99_improvement > 0.0);
    }
}
