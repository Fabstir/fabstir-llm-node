use ethers::prelude::*;
use fabstir_llm_node::host::registry::HostInfo;
use fabstir_llm_node::host::selection::{
    HostSelector, JobRequirements, PerformanceMetrics, ScoringWeights,
};
use std::collections::HashMap;
use std::sync::Arc;

// Helper function to create mock hosts with performance data
fn create_mock_hosts_with_metrics() -> (Vec<HostInfo>, HashMap<Address, PerformanceMetrics>) {
    let mut hosts = Vec::new();
    let mut metrics = HashMap::new();

    // Host 1: High performance, high cost
    let addr1 = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();
    hosts.push(HostInfo {
        address: addr1,
        metadata: r#"{"gpu":"A100","ram":128,"models":["llama-70b","gpt-j"]}"#.to_string(),
        stake: U256::from(2000000u64),
        is_online: true,
    });
    metrics.insert(
        addr1,
        PerformanceMetrics {
            jobs_completed: 1000,
            success_rate: 0.99,
            avg_completion_time: 500, // Fast
            uptime_percentage: 0.999,
            current_load: 2,
            cost_per_token: 0.001, // Expensive
        },
    );

    // Host 2: Good balance of performance and cost
    let addr2 = "0x2222222222222222222222222222222222222222"
        .parse::<Address>()
        .unwrap();
    hosts.push(HostInfo {
        address: addr2,
        metadata: r#"{"gpu":"RTX 4090","ram":64,"models":["llama-7b","mistral-7b"]}"#.to_string(),
        stake: U256::from(1000000u64),
        is_online: true,
    });
    metrics.insert(
        addr2,
        PerformanceMetrics {
            jobs_completed: 500,
            success_rate: 0.95,
            avg_completion_time: 1000, // Medium speed
            uptime_percentage: 0.98,
            current_load: 3,
            cost_per_token: 0.0002, // Reasonable
        },
    );

    // Host 3: Low cost, lower performance
    let addr3 = "0x3333333333333333333333333333333333333333"
        .parse::<Address>()
        .unwrap();
    hosts.push(HostInfo {
        address: addr3,
        metadata: r#"{"gpu":"RTX 3090","ram":32,"models":["llama-7b"]}"#.to_string(),
        stake: U256::from(500000u64),
        is_online: true,
    });
    metrics.insert(
        addr3,
        PerformanceMetrics {
            jobs_completed: 200,
            success_rate: 0.90,
            avg_completion_time: 2000, // Slower
            uptime_percentage: 0.95,
            current_load: 1,
            cost_per_token: 0.00005, // Cheapest
        },
    );

    // Host 4: Overloaded host
    let addr4 = "0x4444444444444444444444444444444444444444"
        .parse::<Address>()
        .unwrap();
    hosts.push(HostInfo {
        address: addr4,
        metadata: r#"{"gpu":"RTX 4090","ram":64,"models":["llama-7b"]}"#.to_string(),
        stake: U256::from(1000000u64),
        is_online: true,
    });
    metrics.insert(
        addr4,
        PerformanceMetrics {
            jobs_completed: 800,
            success_rate: 0.96,
            avg_completion_time: 900,
            uptime_percentage: 0.97,
            current_load: 10, // Heavily loaded
            cost_per_token: 0.00025,
        },
    );

    (hosts, metrics)
}

#[tokio::test]
async fn test_best_host_selection() {
    // Test that best host is selected based on composite score
    let (hosts, metrics) = create_mock_hosts_with_metrics();
    let mut selector = HostSelector::new();

    // Update metrics for all hosts
    for (addr, metric) in metrics {
        selector.update_performance_metrics(addr, metric).await;
    }

    let requirements = JobRequirements {
        model_id: "llama-7b".to_string(),
        min_ram_gb: 32,
        max_cost_per_token: Some(0.001),
        min_reliability: Some(0.9),
    };

    let best = selector.select_best_host(hosts, &requirements).await;
    assert!(best.is_some());

    // Host 3 should be selected (lowest cost and load)
    let selected = best.unwrap();
    assert_eq!(
        selected,
        "0x3333333333333333333333333333333333333333"
            .parse::<Address>()
            .unwrap()
    );
}

#[tokio::test]
async fn test_top_n_hosts_ranking() {
    // Test that top N hosts are ranked correctly
    let (hosts, metrics) = create_mock_hosts_with_metrics();
    let mut selector = HostSelector::new();

    for (addr, metric) in metrics {
        selector.update_performance_metrics(addr, metric).await;
    }

    let requirements = JobRequirements {
        model_id: "llama-7b".to_string(),
        min_ram_gb: 32,
        max_cost_per_token: None,
        min_reliability: None,
    };

    let top_3 = selector.select_top_n_hosts(hosts, 3, &requirements).await;
    assert_eq!(top_3.len(), 3);

    // Verify ordering makes sense (not necessarily exact due to weights)
    assert!(top_3.contains(
        &"0x2222222222222222222222222222222222222222"
            .parse::<Address>()
            .unwrap()
    ));
}

#[tokio::test]
async fn test_cost_optimization_selection() {
    // Test that cost optimization selects cheapest viable host
    let (hosts, metrics) = create_mock_hosts_with_metrics();
    let mut selector = HostSelector::new();

    for (addr, metric) in metrics {
        selector.update_performance_metrics(addr, metric).await;
    }

    let cheapest = selector.select_by_cost_optimization(hosts).await;
    assert!(cheapest.is_some());

    // Host 3 should be selected (cheapest)
    assert_eq!(
        cheapest.unwrap(),
        "0x3333333333333333333333333333333333333333"
            .parse::<Address>()
            .unwrap()
    );
}

#[tokio::test]
async fn test_performance_selection() {
    // Test that performance selection picks fastest host
    let (hosts, metrics) = create_mock_hosts_with_metrics();
    let mut selector = HostSelector::new();

    for (addr, metric) in metrics {
        selector.update_performance_metrics(addr, metric).await;
    }

    let fastest = selector.select_by_performance(hosts).await;
    assert!(fastest.is_some());

    // Host 1 should be selected (fastest completion time)
    assert_eq!(
        fastest.unwrap(),
        "0x1111111111111111111111111111111111111111"
            .parse::<Address>()
            .unwrap()
    );
}

#[tokio::test]
async fn test_load_balancing_selection() {
    // Test that load balancing distributes evenly
    let (hosts, metrics) = create_mock_hosts_with_metrics();
    let mut selector = HostSelector::new();

    for (addr, metric) in metrics {
        selector.update_performance_metrics(addr, metric).await;
    }

    let least_loaded = selector.select_with_load_balancing(hosts).await;
    assert!(least_loaded.is_some());

    // Host 3 should be selected (lowest load of 1)
    assert_eq!(
        least_loaded.unwrap(),
        "0x3333333333333333333333333333333333333333"
            .parse::<Address>()
            .unwrap()
    );
}

#[tokio::test]
async fn test_empty_host_list() {
    // Test that empty host list is handled gracefully
    let selector = HostSelector::new();
    let empty_hosts = Vec::new();

    let requirements = JobRequirements {
        model_id: "llama-7b".to_string(),
        min_ram_gb: 32,
        max_cost_per_token: None,
        min_reliability: None,
    };

    let result = selector.select_best_host(empty_hosts, &requirements).await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_filtering_by_requirements() {
    // Test that filtering by requirements works correctly
    let (hosts, metrics) = create_mock_hosts_with_metrics();
    let mut selector = HostSelector::new();

    for (addr, metric) in metrics {
        selector.update_performance_metrics(addr, metric).await;
    }

    // Strict requirements that only host 1 meets
    let strict_requirements = JobRequirements {
        model_id: "gpt-j".to_string(), // Only host 1 has this
        min_ram_gb: 100,
        max_cost_per_token: Some(0.002),
        min_reliability: Some(0.98),
    };

    let result = selector.select_best_host(hosts, &strict_requirements).await;
    assert!(result.is_some());
    assert_eq!(
        result.unwrap(),
        "0x1111111111111111111111111111111111111111"
            .parse::<Address>()
            .unwrap()
    );
}

#[tokio::test]
async fn test_score_calculation() {
    // Test that score calculation produces expected results
    let selector = HostSelector::new();

    let host = HostInfo {
        address: "0x1111111111111111111111111111111111111111"
            .parse::<Address>()
            .unwrap(),
        metadata: "{}".to_string(),
        stake: U256::from(1000000u64),
        is_online: true,
    };

    let metrics = PerformanceMetrics {
        jobs_completed: 1000,
        success_rate: 1.0,        // Perfect
        avg_completion_time: 100, // Very fast
        uptime_percentage: 1.0,   // Perfect
        current_load: 0,          // No load
        cost_per_token: 0.0001,
    };

    let score = selector.calculate_host_score(&host, &metrics);

    // Should be high score (close to maximum)
    assert!(score > 0.8);
    assert!(score <= 1.0);
}

#[tokio::test]
async fn test_custom_scoring_weights() {
    // Test that custom scoring weights affect selection
    let (hosts, metrics) = create_mock_hosts_with_metrics();

    // Create selector with cost-focused weights
    let weights = ScoringWeights {
        performance: 0.1,
        cost: 0.7, // Heavily weight cost
        reliability: 0.1,
        load: 0.1,
    };

    let mut selector = HostSelector::with_weights(weights);

    for (addr, metric) in metrics {
        selector.update_performance_metrics(addr, metric).await;
    }

    let requirements = JobRequirements {
        model_id: "llama-7b".to_string(),
        min_ram_gb: 32,
        max_cost_per_token: None,
        min_reliability: None,
    };

    let best = selector.select_best_host(hosts, &requirements).await;
    assert!(best.is_some());

    // With cost-focused weights, host 3 (cheapest) should be selected
    assert_eq!(
        best.unwrap(),
        "0x3333333333333333333333333333333333333333"
            .parse::<Address>()
            .unwrap()
    );
}

#[tokio::test]
async fn test_concurrent_metric_updates() {
    // Test thread-safe concurrent metric updates
    let selector = Arc::new(tokio::sync::RwLock::new(HostSelector::new()));
    let mut handles = vec![];

    for i in 0..10 {
        let selector_clone = selector.clone();
        let handle = tokio::spawn(async move {
            let addr = format!("0x{:040x}", i + 1).parse::<Address>().unwrap();

            let metrics = PerformanceMetrics {
                jobs_completed: i * 100,
                success_rate: 0.9 + (i as f64 * 0.01),
                avg_completion_time: 1000u64.saturating_sub(i as u64 * 50),
                uptime_percentage: 0.95,
                current_load: i,
                cost_per_token: 0.0001 * (i as f64 + 1.0),
            };

            let mut selector = selector_clone.write().await;
            selector.update_performance_metrics(addr, metrics).await;
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify all metrics were updated
    let selector = selector.read().await;
    let count = selector.get_metrics_count().await;
    assert!(count >= 10);
}
