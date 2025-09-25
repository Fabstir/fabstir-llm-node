use fabstir_llm_node::api::websocket::{
    memory_manager::MemoryManager,
    metrics::{LoadBalancer, MetricsCollector, PerformanceMetrics},
    session::{SessionConfig, WebSocketSession},
};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[tokio::test]
async fn test_metrics_collector_creation() {
    let collector = MetricsCollector::new();

    let metrics = collector.get_metrics().await;
    assert_eq!(metrics.total_requests, 0);
    assert_eq!(metrics.active_sessions, 0);
    assert_eq!(metrics.avg_response_time_ms, 0.0);
}

#[tokio::test]
async fn test_request_tracking() {
    let collector = MetricsCollector::new();

    // Track some requests
    for i in 0..10 {
        let duration = Duration::from_millis(100 + i * 10);
        collector.record_request(duration).await;
    }

    let metrics = collector.get_metrics().await;
    assert_eq!(metrics.total_requests, 10);
    assert!(metrics.avg_response_time_ms > 100.0);
    assert!(metrics.avg_response_time_ms < 200.0);
}

#[tokio::test]
async fn test_session_metrics() {
    let collector = MetricsCollector::new();

    collector.session_created("session-1").await;
    collector.session_created("session-2").await;

    let metrics = collector.get_metrics().await;
    assert_eq!(metrics.active_sessions, 2);
    assert_eq!(metrics.total_sessions_created, 2);

    collector.session_closed("session-1").await;

    let metrics = collector.get_metrics().await;
    assert_eq!(metrics.active_sessions, 1);
}

#[tokio::test]
async fn test_throughput_calculation() {
    let collector = MetricsCollector::new();

    // Simulate high throughput
    let start = Instant::now();
    for _ in 0..100 {
        collector.record_request(Duration::from_millis(10)).await;
    }
    let elapsed = start.elapsed();

    let metrics = collector.get_metrics().await;
    let throughput = metrics.requests_per_second;

    // Should be roughly 100 / elapsed_seconds
    let expected = 100.0 / elapsed.as_secs_f64();
    assert!((throughput - expected).abs() < 10.0);
}

#[tokio::test]
async fn test_memory_metrics() {
    let collector = MetricsCollector::new();

    collector.update_memory_usage(1024 * 1024).await; // 1MB

    let metrics = collector.get_metrics().await;
    assert_eq!(metrics.memory_used_bytes, 1024 * 1024);
    assert!(metrics.memory_usage_percent > 0.0);
    assert!(metrics.memory_usage_percent < 100.0);
}

#[tokio::test]
async fn test_load_balancer() {
    let balancer = LoadBalancer::new(4); // 4 workers

    // Distribute load
    for _ in 0..100 {
        let worker_id = balancer.next_worker().await;
        assert!(worker_id < 4);
    }

    // Check distribution is relatively even
    let distribution = balancer.get_distribution().await;
    for count in distribution.values() {
        assert!(*count >= 20); // Each should get at least 20
        assert!(*count <= 30); // But not more than 30
    }
}

#[tokio::test]
async fn test_percentile_calculations() {
    let collector = MetricsCollector::new();

    // Add response times
    let times = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
    for time in times {
        collector.record_request(Duration::from_millis(time)).await;
    }

    let metrics = collector.get_metrics().await;
    assert_eq!(metrics.p50_response_time_ms, 50.0);
    assert_eq!(metrics.p95_response_time_ms, 95.0);
    assert_eq!(metrics.p99_response_time_ms, 99.0);
}

#[tokio::test]
async fn test_error_rate_tracking() {
    let collector = MetricsCollector::new();

    // Record mix of success and errors
    for i in 0..100 {
        if i % 10 == 0 {
            collector.record_error().await;
        } else {
            collector.record_request(Duration::from_millis(10)).await;
        }
    }

    let metrics = collector.get_metrics().await;
    assert_eq!(metrics.error_count, 10);
    assert!((metrics.error_rate - 0.1).abs() < 0.01);
}

#[tokio::test]
async fn test_concurrent_metric_updates() {
    let collector = Arc::new(MetricsCollector::new());

    let mut handles = vec![];

    // Spawn concurrent tasks
    for i in 0..10 {
        let collector_clone = collector.clone();
        let handle = tokio::spawn(async move {
            for j in 0..100 {
                collector_clone
                    .record_request(Duration::from_millis(i * 10 + j))
                    .await;
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let metrics = collector.get_metrics().await;
    assert_eq!(metrics.total_requests, 1000);
}

#[tokio::test]
async fn test_sliding_window_metrics() {
    let collector = MetricsCollector::with_window(Duration::from_secs(1));

    // Add requests
    for _ in 0..50 {
        collector.record_request(Duration::from_millis(10)).await;
    }

    let metrics1 = collector.get_metrics().await;
    assert_eq!(metrics1.total_requests, 50);

    // Wait for window to expire
    tokio::time::sleep(Duration::from_millis(1100)).await;

    // Add more requests
    for _ in 0..25 {
        collector.record_request(Duration::from_millis(10)).await;
    }

    let metrics2 = collector.get_metrics().await;
    // Should only show recent requests
    assert_eq!(metrics2.requests_in_window, 25);
}

#[tokio::test]
async fn test_performance_benchmarks() {
    let collector = MetricsCollector::new();

    let start = Instant::now();

    // Benchmark metric collection
    for _ in 0..10000 {
        collector.record_request(Duration::from_millis(1)).await;
    }

    let elapsed = start.elapsed();

    // Should be very fast
    assert!(elapsed < Duration::from_secs(1));

    // Getting metrics should also be fast
    let metrics_start = Instant::now();
    let _metrics = collector.get_metrics().await;
    let metrics_elapsed = metrics_start.elapsed();

    assert!(metrics_elapsed < Duration::from_millis(10));
}

#[tokio::test]
async fn test_resource_monitoring() {
    let collector = MetricsCollector::new();

    // Start resource monitoring
    collector.start_monitoring().await;

    // Let it collect some data
    tokio::time::sleep(Duration::from_millis(100)).await;

    let metrics = collector.get_metrics().await;

    // Should have CPU and memory metrics
    assert!(metrics.cpu_usage_percent >= 0.0);
    assert!(metrics.cpu_usage_percent <= 100.0);
    assert!(metrics.memory_used_bytes > 0);

    collector.stop_monitoring().await;
}
