// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! S5 metrics monitoring tests
//!
//! Tests for S5 vector loading metrics collection and export

use anyhow::Result;
use fabstir_llm_node::monitoring::{MetricsCollector, MetricsConfig, S5Metrics, TimeWindow};
use std::sync::Arc;
use std::time::Duration;

async fn create_test_metrics_collector() -> Result<MetricsCollector> {
    let config = MetricsConfig {
        enable_metrics: true,
        collection_interval_ms: 100,
        retention_period_hours: 24,
        aggregation_windows: vec![
            TimeWindow::OneMinute,
            TimeWindow::FiveMinutes,
            TimeWindow::OneHour,
        ],
        export_format: "prometheus".to_string(),
        export_endpoint: "http://localhost:9090".to_string(),
        buffer_size: 10000,
    };

    MetricsCollector::new(config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_s5_metrics_initialization() {
        let collector = create_test_metrics_collector().await.unwrap();
        let metrics = S5Metrics::new(&collector).await.unwrap();

        // Verify all metrics are initialized with zero values
        assert_eq!(metrics.download_errors.get().await, 0.0);
        assert_eq!(metrics.vectors_loaded.get().await, 0.0);
        assert_eq!(metrics.cache_hits.get().await, 0.0);
        assert_eq!(metrics.cache_misses.get().await, 0.0);
    }

    #[tokio::test]
    async fn test_download_duration_recording() {
        let collector = create_test_metrics_collector().await.unwrap();
        let metrics = S5Metrics::new(&collector).await.unwrap();

        // Record various download durations
        metrics.record_download(Duration::from_millis(150)).await;
        metrics.record_download(Duration::from_millis(750)).await;
        metrics.record_download(Duration::from_secs(2)).await;

        let stats = metrics.download_duration.get_statistics().await;
        assert_eq!(stats.count, 3);
        assert!(stats.sum > 2.0); // At least 2 seconds total
        assert!(stats.average > 0.6); // Average should be > 600ms
    }

    #[tokio::test]
    async fn test_download_error_counting() {
        let collector = create_test_metrics_collector().await.unwrap();
        let metrics = S5Metrics::new(&collector).await.unwrap();

        // Record multiple errors
        metrics.record_download_error().await;
        metrics.record_download_error().await;
        metrics.record_download_error().await;

        assert_eq!(metrics.download_errors.get().await, 3.0);
    }

    #[tokio::test]
    async fn test_vectors_loaded_counting() {
        let collector = create_test_metrics_collector().await.unwrap();
        let metrics = S5Metrics::new(&collector).await.unwrap();

        // Load vectors in batches
        metrics.record_vectors_loaded(100).await;
        metrics.record_vectors_loaded(250).await;
        metrics.record_vectors_loaded(500).await;

        assert_eq!(metrics.vectors_loaded.get().await, 850.0);
    }

    #[tokio::test]
    async fn test_index_build_duration_recording() {
        let collector = create_test_metrics_collector().await.unwrap();
        let metrics = S5Metrics::new(&collector).await.unwrap();

        // Record index build times
        metrics.record_index_build(Duration::from_millis(25)).await;
        metrics.record_index_build(Duration::from_millis(100)).await;
        metrics.record_index_build(Duration::from_millis(500)).await;

        let stats = metrics.index_build_duration.get_statistics().await;
        assert_eq!(stats.count, 3);
        assert!(stats.sum > 0.6); // At least 625ms total
    }

    #[tokio::test]
    async fn test_cache_hit_counting() {
        let collector = create_test_metrics_collector().await.unwrap();
        let metrics = S5Metrics::new(&collector).await.unwrap();

        // Record cache hits
        metrics.record_cache_hit().await;
        metrics.record_cache_hit().await;
        metrics.record_cache_hit().await;
        metrics.record_cache_hit().await;

        assert_eq!(metrics.cache_hits.get().await, 4.0);
    }

    #[tokio::test]
    async fn test_cache_miss_counting() {
        let collector = create_test_metrics_collector().await.unwrap();
        let metrics = S5Metrics::new(&collector).await.unwrap();

        // Record cache misses
        metrics.record_cache_miss().await;
        metrics.record_cache_miss().await;

        assert_eq!(metrics.cache_misses.get().await, 2.0);
    }

    #[tokio::test]
    async fn test_cache_hit_rate_calculation() {
        let collector = create_test_metrics_collector().await.unwrap();
        let metrics = S5Metrics::new(&collector).await.unwrap();

        // Simulate cache access pattern: 80% hit rate
        for _ in 0..8 {
            metrics.record_cache_hit().await;
        }
        for _ in 0..2 {
            metrics.record_cache_miss().await;
        }

        let hits = metrics.cache_hits.get().await;
        let misses = metrics.cache_misses.get().await;
        let total = hits + misses;
        let hit_rate = hits / total;

        assert_eq!(hits, 8.0);
        assert_eq!(misses, 2.0);
        assert!((hit_rate - 0.8).abs() < 0.01); // 80% hit rate
    }

    #[tokio::test]
    async fn test_metrics_collector_integration() {
        let collector = create_test_metrics_collector().await.unwrap();
        let metrics = S5Metrics::new(&collector).await.unwrap();

        // Record some data
        metrics.record_download(Duration::from_secs(1)).await;
        metrics.record_download_error().await;
        metrics.record_vectors_loaded(100).await;

        // Verify metrics are accessible through collector
        let download_metric = collector
            .get_metric("s5_download_duration_seconds")
            .await
            .unwrap();
        assert_eq!(
            download_metric.metric_type,
            fabstir_llm_node::monitoring::MetricType::Histogram
        );

        let error_metric = collector
            .get_metric("s5_download_errors_total")
            .await
            .unwrap();
        assert_eq!(
            error_metric.metric_type,
            fabstir_llm_node::monitoring::MetricType::Counter
        );
    }

    #[tokio::test]
    async fn test_concurrent_metric_updates() {
        use tokio::task;

        let collector = create_test_metrics_collector().await.unwrap();
        let metrics = Arc::new(S5Metrics::new(&collector).await.unwrap());
        let mut handles = vec![];

        // Spawn 10 tasks, each recording 100 operations
        for _ in 0..10 {
            let metrics_clone = Arc::clone(&metrics);
            let handle = task::spawn(async move {
                for _ in 0..100 {
                    metrics_clone.record_vectors_loaded(1).await;
                    metrics_clone.record_cache_hit().await;
                }
            });
            handles.push(handle);
        }

        // Wait for all tasks
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify totals
        assert_eq!(metrics.vectors_loaded.get().await, 1000.0);
        assert_eq!(metrics.cache_hits.get().await, 1000.0);
    }
}
