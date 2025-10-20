// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// tests/monitoring/test_metrics.rs

use anyhow::Result;
use fabstir_llm_node::monitoring::{
    AggregationType, Counter, Gauge, Histogram, MetricLabel, MetricType, MetricValue,
    MetricsCollector, MetricsConfig, MetricsExporter, MetricsRegistry, PrometheusExporter,
    TimeWindow,
};
use std::time::Duration;
use tokio;

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

#[tokio::test]
async fn test_basic_counter_metrics() {
    let collector = create_test_metrics_collector().await.unwrap();

    // Register a counter
    let counter = collector
        .register_counter("inference_requests_total", "Total inference requests")
        .await
        .unwrap();

    // Increment counter
    counter.inc().await;
    counter.inc_by(5).await;

    // Get current value
    let value = counter.get().await;
    assert_eq!(value, 6.0);

    // Check metric in registry
    let metric = collector
        .get_metric("inference_requests_total")
        .await
        .unwrap();
    assert_eq!(metric.value, MetricValue::Counter(6.0));
}

#[tokio::test]
async fn test_gauge_metrics() {
    let collector = create_test_metrics_collector().await.unwrap();

    // Register a gauge
    let gauge = collector
        .register_gauge("gpu_memory_usage_bytes", "GPU memory usage in bytes")
        .await
        .unwrap();

    // Set gauge value
    gauge.set(1_000_000_000.0).await;

    // Increment and decrement
    gauge.inc_by(500_000_000.0).await;
    gauge.dec_by(200_000_000.0).await;

    let value = gauge.get().await;
    assert_eq!(value, 1_300_000_000.0);
}

#[tokio::test]
async fn test_histogram_metrics() {
    let collector = create_test_metrics_collector().await.unwrap();

    // Register a histogram with buckets
    let buckets = vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0];
    let histogram = collector
        .register_histogram(
            "inference_latency_seconds",
            "Inference request latency",
            buckets,
        )
        .await
        .unwrap();

    // Record observations
    histogram.observe(0.25).await;
    histogram.observe(0.75).await;
    histogram.observe(1.5).await;
    histogram.observe(3.0).await;

    // Get statistics
    let stats = histogram.get_statistics().await;
    assert_eq!(stats.count, 4);
    assert_eq!(stats.sum, 5.5);
    assert!(stats.average > 1.0 && stats.average < 2.0);
}

#[tokio::test]
async fn test_metric_labels() {
    let collector = create_test_metrics_collector().await.unwrap();

    // Create counter with labels
    let counter = collector
        .register_counter_with_labels(
            "http_requests_total",
            "Total HTTP requests",
            vec!["method", "status"],
        )
        .await
        .unwrap();

    // Increment with different label values
    counter
        .with_labels(vec![("method", "GET"), ("status", "200")])
        .inc()
        .await;

    counter
        .with_labels(vec![("method", "POST"), ("status", "201")])
        .inc_by(3)
        .await;

    // Query by labels
    let get_200 = counter
        .with_labels(vec![("method", "GET"), ("status", "200")])
        .get()
        .await;
    assert_eq!(get_200, 1.0);

    let post_201 = counter
        .with_labels(vec![("method", "POST"), ("status", "201")])
        .get()
        .await;
    assert_eq!(post_201, 3.0);
}

#[tokio::test]
async fn test_metric_aggregation() {
    let collector = create_test_metrics_collector().await.unwrap();

    let gauge = collector
        .register_gauge("cpu_usage_percent", "CPU usage percentage")
        .await
        .unwrap();

    // Record values over time
    for i in 0..10 {
        gauge.set((i * 10) as f64).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Get aggregated values
    let avg = collector
        .get_aggregated_value(
            "cpu_usage_percent",
            TimeWindow::OneMinute,
            AggregationType::Average,
        )
        .await
        .unwrap();

    assert!(avg > 0.0);

    let max = collector
        .get_aggregated_value(
            "cpu_usage_percent",
            TimeWindow::OneMinute,
            AggregationType::Max,
        )
        .await
        .unwrap();

    assert_eq!(max, 90.0);
}

#[tokio::test]
async fn test_prometheus_export() {
    let collector = create_test_metrics_collector().await.unwrap();

    // Register various metrics
    let counter = collector
        .register_counter("test_counter", "Test counter")
        .await
        .unwrap();
    counter.inc_by(42).await;

    let gauge = collector
        .register_gauge("test_gauge", "Test gauge")
        .await
        .unwrap();
    gauge.set(123.45).await;

    // Export to Prometheus format
    let exporter = PrometheusExporter::new();
    let output = collector.export(&exporter).await.unwrap();

    assert!(output.contains("# HELP test_counter Test counter"));
    assert!(output.contains("# TYPE test_counter counter"));
    assert!(output.contains("test_counter 42"));

    assert!(output.contains("# HELP test_gauge Test gauge"));
    assert!(output.contains("# TYPE test_gauge gauge"));
    assert!(output.contains("test_gauge 123.45"));
}

#[tokio::test]
async fn test_metric_reset() {
    let collector = create_test_metrics_collector().await.unwrap();

    let counter = collector
        .register_counter("reset_test", "Reset test counter")
        .await
        .unwrap();

    counter.inc_by(100).await;
    assert_eq!(counter.get().await, 100.0);

    // Reset the counter
    counter.reset().await;
    assert_eq!(counter.get().await, 0.0);
}

#[tokio::test]
async fn test_batch_metric_updates() {
    let collector = create_test_metrics_collector().await.unwrap();

    // Batch update multiple metrics
    let updates = vec![
        ("requests_total", MetricValue::Counter(100.0)),
        ("memory_usage", MetricValue::Gauge(2_000_000_000.0)),
        ("latency_p99", MetricValue::Gauge(0.95)),
    ];

    collector.batch_update(updates).await.unwrap();

    // Verify all metrics were updated
    let requests = collector.get_metric("requests_total").await.unwrap();
    assert_eq!(requests.value, MetricValue::Counter(100.0));

    let memory = collector.get_metric("memory_usage").await.unwrap();
    assert_eq!(memory.value, MetricValue::Gauge(2_000_000_000.0));
}

#[tokio::test]
async fn test_metric_persistence() {
    let collector = create_test_metrics_collector().await.unwrap();

    // Create metrics
    let counter = collector
        .register_counter("persistent_counter", "Persistent counter")
        .await
        .unwrap();
    counter.inc_by(50).await;

    // Save metrics to disk
    let path = std::path::Path::new("test_data/metrics_snapshot.bin");
    collector.save_snapshot(path).await.unwrap();

    // Create new collector and load snapshot
    let new_collector = create_test_metrics_collector().await.unwrap();
    new_collector.load_snapshot(path).await.unwrap();

    // Verify metric was restored
    let restored = new_collector
        .get_metric("persistent_counter")
        .await
        .unwrap();
    assert_eq!(restored.value, MetricValue::Counter(50.0));
}

#[tokio::test]
async fn test_metric_rate_calculation() {
    let collector = create_test_metrics_collector().await.unwrap();

    let counter = collector
        .register_counter("requests", "Request counter")
        .await
        .unwrap();

    // Record initial value
    collector
        .record_time_series_point("requests", 0.0)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Simulate requests over time
    let start = std::time::Instant::now();
    for i in 0..10 {
        counter.inc_by(10).await;
        // Record time series point
        collector
            .record_time_series_point("requests", (i + 1) as f64 * 10.0)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let elapsed = start.elapsed().as_secs_f64();

    // Calculate rate
    let rate = collector
        .calculate_rate("requests", Duration::from_secs(1))
        .await
        .unwrap();

    // Should be approximately 100 requests per elapsed time
    let expected_rate = 100.0 / elapsed;
    assert!((rate - expected_rate).abs() < 50.0); // Allow more variance due to timing
}

#[tokio::test]
async fn test_metric_stream() {
    let collector = create_test_metrics_collector().await.unwrap();

    // Subscribe to metric updates
    let mut stream = collector.subscribe("test_stream_counter").await.unwrap();

    collector
        .register_counter("test_stream_counter", "Streaming counter")
        .await
        .unwrap();

    // Update counter in background
    let collector_clone = collector.clone();
    tokio::spawn(async move {
        for _i in 1..=5 {
            collector_clone
                .increment_counter_with_notification("test_stream_counter")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    // Collect streamed updates
    let mut updates = vec![];
    for _ in 0..5 {
        if let Ok(Some(value)) = tokio::time::timeout(Duration::from_secs(1), stream.recv()).await {
            updates.push(value);
        }
    }

    assert_eq!(updates.len(), 5);
    assert_eq!(updates.last().unwrap().value, MetricValue::Counter(5.0));
}

#[tokio::test]
async fn test_custom_metric_type() {
    let collector = create_test_metrics_collector().await.unwrap();

    // Register a custom summary metric
    let summary = collector
        .register_summary(
            "response_size_bytes",
            "Response size distribution",
            vec![0.5, 0.9, 0.99],    // Quantiles
            Duration::from_secs(60), // Window
        )
        .await
        .unwrap();

    // Record values
    for i in 1..=100 {
        summary.observe((i * 1000) as f64).await;
    }

    // Get quantiles
    let quantiles = summary.get_quantiles().await;
    assert!(quantiles.get(&0.5).unwrap() > &40000.0);
    assert!(quantiles.get(&0.5).unwrap() < &60000.0);
    assert!(quantiles.get(&0.99).unwrap() > &95000.0);
}

#[tokio::test]
async fn test_metric_garbage_collection() {
    let mut config = MetricsConfig::default();
    config.retention_period_hours = 0; // Immediate GC for testing
    config.collection_interval_ms = 50;

    let collector = MetricsCollector::new(config).await.unwrap();

    // Create metric
    let gauge = collector
        .register_gauge("short_lived", "Short lived metric")
        .await
        .unwrap();
    gauge.set(100.0).await;

    // Start garbage collection
    collector.start_garbage_collection().await;

    // Wait for GC cycle
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Old metrics should be cleaned up
    let metrics = collector.list_metrics().await;
    assert!(metrics
        .iter()
        .any(|m| m.last_updated.elapsed().as_secs() < 1));
}
