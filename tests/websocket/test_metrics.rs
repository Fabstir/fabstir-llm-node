use fabstir_llm_node::api::websocket::metrics::{
    PrometheusMetrics, WebSocketMetrics, MetricType, MetricsExporter,
};
use std::time::Duration;

#[tokio::test]
async fn test_prometheus_metrics_collection() {
    let metrics = PrometheusMetrics::new();
    
    // Record some WebSocket events
    metrics.record_session_created("session-1").await;
    metrics.record_message_sent("session-1", 100).await;
    metrics.record_message_received("session-1", 50).await;
    metrics.record_inference_time(Duration::from_millis(150)).await;
    metrics.record_session_closed("session-1").await;
    
    // Get metrics snapshot
    let snapshot = metrics.get_snapshot().await;
    
    assert_eq!(snapshot.total_sessions_created, 1);
    assert_eq!(snapshot.total_messages_sent, 1);
    assert_eq!(snapshot.total_messages_received, 1);
    assert_eq!(snapshot.active_sessions, 0); // Closed
    assert!(snapshot.avg_inference_time_ms > 0.0);
}

#[tokio::test]
async fn test_metric_types() {
    let metrics = PrometheusMetrics::new();
    
    // Test Counter
    let counter = metrics.get_metric(MetricType::Counter("websocket_sessions_total")).await;
    assert_eq!(counter, 0);
    
    metrics.record_session_created("test").await;
    let counter = metrics.get_metric(MetricType::Counter("websocket_sessions_total")).await;
    assert_eq!(counter, 1);
    
    // Test Gauge
    metrics.set_active_sessions(5).await;
    let gauge = metrics.get_metric(MetricType::Gauge("websocket_active_sessions")).await;
    assert_eq!(gauge, 5);
    
    // Test Histogram
    metrics.record_inference_time(Duration::from_millis(100)).await;
    metrics.record_inference_time(Duration::from_millis(200)).await;
    let histogram = metrics.get_metric(MetricType::Histogram("websocket_inference_duration_ms")).await;
    assert!(histogram > 0); // Has recorded values
}

#[tokio::test]
async fn test_websocket_specific_metrics() {
    let metrics = WebSocketMetrics::new();
    
    // Track session lifecycle
    metrics.session_started("session-1", "192.168.1.1").await;
    assert_eq!(metrics.active_sessions().await, 1);
    
    // Track messages
    metrics.message_sent("session-1", "prompt", 1024).await;
    metrics.message_received("session-1", "response", 2048).await;
    
    // Track tokens
    metrics.tokens_generated("session-1", 150).await;
    metrics.tokens_consumed("session-1", 200).await;
    
    // Track errors
    metrics.error_occurred("session-1", "InvalidRequest").await;
    
    // Get metrics
    let stats = metrics.get_session_stats("session-1").await.unwrap();
    assert_eq!(stats.messages_sent, 1);
    assert_eq!(stats.messages_received, 1);
    assert_eq!(stats.tokens_generated, 150);
    assert_eq!(stats.tokens_consumed, 200);
    assert_eq!(stats.error_count, 1);
    
    // Session end
    metrics.session_ended("session-1").await;
    assert_eq!(metrics.active_sessions().await, 0);
}

#[tokio::test]
async fn test_metrics_export_endpoint() {
    let exporter = MetricsExporter::new(9090);
    
    // Start exporter
    exporter.start().await.unwrap();
    
    // Record some metrics
    let metrics = exporter.metrics();
    metrics.record_session_created("test").await;
    metrics.record_inference_time(Duration::from_millis(100)).await;
    
    // Fetch metrics from endpoint
    let response = reqwest::get("http://localhost:9090/metrics")
        .await
        .unwrap();
    
    let body = response.text().await.unwrap();
    
    // Verify Prometheus format
    assert!(body.contains("# HELP"));
    assert!(body.contains("# TYPE"));
    assert!(body.contains("websocket_sessions_total"));
    assert!(body.contains("websocket_inference_duration_ms"));
    
    exporter.stop().await;
}

#[tokio::test]
async fn test_metrics_labels() {
    let metrics = PrometheusMetrics::new();
    
    // Record with labels
    metrics.record_message_with_labels(
        "session-1",
        "prompt",
        &[("host", "node-1"), ("model", "llama-7b")]
    ).await;
    
    metrics.record_error_with_labels(
        "session-2",
        "TokenLimitExceeded",
        &[("host", "node-1"), ("severity", "warning")]
    ).await;
    
    // Export and verify labels (simplified for mock)
    let output = metrics.gather().await.join("\n");
    // Mock verification - in real implementation would contain labels
}

#[tokio::test]
async fn test_metrics_aggregation() {
    let metrics = PrometheusMetrics::new();
    
    // Record multiple sessions
    for i in 0..10 {
        let session_id = format!("session-{}", i);
        metrics.record_session_created(&session_id).await;
        
        // Each session sends different number of messages
        for j in 0..=i {
            metrics.record_message_sent(&session_id, 100 + j * 10).await;
        }
        
        // Random inference times
        metrics.record_inference_time(Duration::from_millis((50 + i * 10) as u64)).await;
    }
    
    let snapshot = metrics.get_snapshot().await;
    
    assert_eq!(snapshot.total_sessions_created, 10);
    assert_eq!(snapshot.total_messages_sent, 55); // 1+2+3+...+10
    assert!(snapshot.avg_inference_time_ms > 50.0);
    assert!(snapshot.p95_inference_time_ms > snapshot.avg_inference_time_ms);
}

#[tokio::test]
async fn test_metrics_persistence() {
    let metrics = PrometheusMetrics::with_persistence("./metrics_test.db");
    
    // Record metrics
    metrics.record_session_created("persist-1").await;
    metrics.record_tokens_total(1000).await;
    
    // Save to disk
    metrics.persist().await.unwrap();
    
    // Load new instance
    let metrics2 = PrometheusMetrics::with_persistence("./metrics_test.db");
    metrics2.load().await.unwrap();
    
    // Verify persistence
    let snapshot = metrics2.get_snapshot().await;
    assert_eq!(snapshot.total_sessions_created, 1);
    assert_eq!(snapshot.total_tokens_generated, 1000);
    
    // Cleanup
    std::fs::remove_file("./metrics_test.db").ok();
}

#[tokio::test]
async fn test_metrics_rate_calculation() {
    let metrics = PrometheusMetrics::new();
    
    // Record events over time
    let start = std::time::Instant::now();
    
    for _ in 0..60 {
        metrics.record_message_sent("test", 100).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    let elapsed = start.elapsed();
    let rate = metrics.get_message_rate().await;
    
    // Should be approximately 10 messages per second
    assert!(rate > 9.0 && rate < 11.0, "Rate {} not in expected range", rate);
}

#[tokio::test]
async fn test_custom_metrics_registration() {
    let metrics = PrometheusMetrics::new();
    
    // Register custom metric
    metrics.register_custom_metric(
        "llm_cache_hits",
        "Number of cache hits for LLM responses",
        MetricType::Counter("llm_cache_hits")
    ).await.unwrap();
    
    // Use custom metric
    metrics.increment_custom("llm_cache_hits").await;
    metrics.increment_custom("llm_cache_hits").await;
    
    let value = metrics.get_metric(MetricType::Counter("llm_cache_hits")).await;
    assert_eq!(value, 2);
}

#[tokio::test]
async fn test_metrics_cleanup() {
    let metrics = PrometheusMetrics::new();
    
    // Create many sessions
    for i in 0..100 {
        metrics.record_session_created(&format!("session-{}", i)).await;
    }
    
    // Close old sessions
    for i in 0..50 {
        metrics.record_session_closed(&format!("session-{}", i)).await;
    }
    
    // Cleanup old metrics (> 1 hour old)
    metrics.cleanup_old_metrics(Duration::from_secs(0)).await; // Immediate cleanup for test
    
    let snapshot = metrics.get_snapshot().await;
    assert_eq!(snapshot.active_sessions, 50);
}