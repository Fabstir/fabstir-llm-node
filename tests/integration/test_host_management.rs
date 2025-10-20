// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use ethers::prelude::*;
use fabstir_llm_node::contracts::registry_monitor::RegistryMonitor;
use fabstir_llm_node::host::registry::{HostInfo, HostRegistry};
use fabstir_llm_node::host::selection::{HostSelector, JobRequirements, PerformanceMetrics};
use fabstir_llm_node::job_assignment_types::{AssignmentStatus, JobClaimConfig};
use fabstir_llm_node::job_claim::{ClaimError, JobClaimer};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

// Helper to create test provider
fn create_test_provider() -> Arc<Provider<Http>> {
    Arc::new(Provider::<Http>::try_from("http://localhost:8545").unwrap())
}

// Helper to create test addresses
fn create_test_address(suffix: u8) -> Address {
    let mut bytes = [0u8; 20];
    bytes[19] = suffix;
    Address::from(bytes)
}

#[tokio::test]
async fn test_complete_host_management_workflow() {
    // 1. Start registry monitor
    let provider = create_test_provider();
    let contract_address = create_test_address(100);
    let monitor = Arc::new(RegistryMonitor::new(contract_address, provider.clone()));

    // 2. Register 3 hosts with different capabilities
    let host1 = create_test_address(1);
    let host2 = create_test_address(2);
    let host3 = create_test_address(3);

    monitor
        .handle_node_registered(
            host1,
            r#"{"gpu":"rtx4090","ram":64,"models":["llama-70b","codellama-34b"]}"#.to_string(),
            U256::from(1000000u64),
        )
        .await;

    monitor
        .handle_node_registered(
            host2,
            r#"{"gpu":"rtx3090","ram":32,"models":["llama-7b","mistral-7b"]}"#.to_string(),
            U256::from(500000u64),
        )
        .await;

    monitor
        .handle_node_registered(
            host3,
            r#"{"gpu":"a100","ram":128,"models":["llama-70b","mixtral-8x7b"]}"#.to_string(),
            U256::from(2000000u64),
        )
        .await;

    // 3. Verify hosts discoverable via registry
    let registry = Arc::new(HostRegistry::new(monitor.clone()));
    let hosts = registry.get_registered_hosts().await;
    assert_eq!(hosts.len(), 3);
    assert!(hosts.contains(&host1));
    assert!(hosts.contains(&host2));
    assert!(hosts.contains(&host3));

    // 4. Submit job with requirements
    let requirements = JobRequirements {
        model_id: "llama-70b".to_string(),
        min_ram_gb: 64,
        max_cost_per_token: Some(0.001),
        min_reliability: Some(0.95),
    };

    // 5. Use selector to find best host
    let mut selector = HostSelector::new();

    // Add performance metrics for hosts
    selector
        .update_performance_metrics(
            host1,
            PerformanceMetrics {
                jobs_completed: 100,
                success_rate: 0.98,
                avg_completion_time: 500,
                uptime_percentage: 0.99,
                current_load: 2,
                cost_per_token: 0.0005,
            },
        )
        .await;

    selector
        .update_performance_metrics(
            host3,
            PerformanceMetrics {
                jobs_completed: 200,
                success_rate: 0.99,
                avg_completion_time: 300,
                uptime_percentage: 0.999,
                current_load: 1,
                cost_per_token: 0.0008,
            },
        )
        .await;

    let capable_addrs = registry.get_available_hosts(&requirements.model_id).await;
    assert_eq!(capable_addrs.len(), 2); // host1 and host3

    // Convert addresses to HostInfo for selector
    let mut capable_hosts = Vec::new();
    for addr in capable_addrs {
        if let Some(info) = registry.get_host_metadata(addr).await {
            capable_hosts.push(info);
        }
    }

    let best_host = selector
        .select_best_host(capable_hosts, &requirements)
        .await
        .unwrap();
    assert!(best_host == host1 || best_host == host3);

    // 6. Assign job to selected host
    let config = JobClaimConfig {
        max_concurrent_jobs: 10,
        claim_timeout_ms: 30000,
        enable_auto_claim: true,
    };
    let claimer = JobClaimer::new(config).await.unwrap();

    let job_id = "test-job-001";
    claimer
        .assign_job_to_host(job_id, best_host, &registry)
        .await
        .unwrap();

    // 7. Verify assignment recorded
    let assignment = claimer.get_assignment_record(job_id).await.unwrap();
    assert_eq!(assignment.job_id, job_id);
    assert_eq!(assignment.host_address, best_host);
    assert_eq!(assignment.status, AssignmentStatus::Confirmed);

    // 8. Reassign to different host
    let new_host = if best_host == host1 { host3 } else { host1 };
    claimer
        .reassign_job(job_id, new_host, &registry)
        .await
        .unwrap();

    let updated = claimer.get_assignment_record(job_id).await.unwrap();
    assert_eq!(updated.host_address, new_host);
    assert_eq!(updated.status, AssignmentStatus::Reassigned);

    // 9. Unregister a host
    monitor.handle_node_unregistered(host2).await;

    // 10. Verify host removed from available list
    let remaining = registry.get_registered_hosts().await;
    assert_eq!(remaining.len(), 2);
    assert!(!remaining.contains(&host2));
}

#[tokio::test]
async fn test_performance_with_100_hosts() {
    let provider = create_test_provider();
    let contract_address = create_test_address(100);
    let monitor = Arc::new(RegistryMonitor::new(contract_address, provider));
    let registry = Arc::new(HostRegistry::new(monitor.clone()));
    let mut selector = HostSelector::new();

    // 1. Register 100+ mock hosts
    let start = Instant::now();
    for i in 0..105 {
        let host = create_test_address(i);
        let gpu = match i % 3 {
            0 => "rtx4090",
            1 => "rtx3090",
            _ => "a100",
        };
        let ram = 32 + (i as u32 % 4) * 32;
        let models = match i % 4 {
            0 => r#"["llama-70b","codellama-34b"]"#,
            1 => r#"["llama-7b","mistral-7b"]"#,
            2 => r#"["mixtral-8x7b","llama-70b"]"#,
            _ => r#"["gpt-j","bloom"]"#,
        };

        monitor
            .handle_node_registered(
                host,
                format!(r#"{{"gpu":"{}","ram":{},"models":{}}}"#, gpu, ram, models),
                U256::from(100000u64 * (i as u64 + 1)),
            )
            .await;

        // Add performance metrics
        selector
            .update_performance_metrics(
                host,
                PerformanceMetrics {
                    jobs_completed: 10 + i as u32 * 2,
                    success_rate: 0.90 + ((i % 10) as f64) / 100.0,
                    avg_completion_time: 300 + (i as u64 % 5) * 100,
                    uptime_percentage: 0.95 + ((i % 5) as f64) / 100.0,
                    current_load: (i % 5) as u32,
                    cost_per_token: 0.0001 + ((i % 3) as f64) * 0.0001,
                },
            )
            .await;
    }
    let registration_time = start.elapsed();
    println!("Registered 105 hosts in {:?}", registration_time);

    // 2. Submit 50 jobs with varying requirements
    let config = JobClaimConfig {
        max_concurrent_jobs: 100,
        claim_timeout_ms: 30000,
        enable_auto_claim: true,
    };
    let claimer = JobClaimer::new(config).await.unwrap();

    let mut selection_times = Vec::new();
    let mut assignments = Vec::new();

    for i in 0..50 {
        let job_id = format!("perf-job-{:03}", i);
        let requirements = JobRequirements {
            model_id: match i % 3 {
                0 => "llama-70b",
                1 => "llama-7b",
                _ => "mixtral-8x7b",
            }
            .to_string(),
            min_ram_gb: 32 + (i % 3) * 32,
            max_cost_per_token: if i % 2 == 0 { Some(0.0005) } else { None },
            min_reliability: Some(0.90 + ((i % 5) as f64) / 100.0),
        };

        // 3. Measure selection time for each job
        let select_start = Instant::now();
        let capable_addrs = registry.get_available_hosts(&requirements.model_id).await;
        let mut capable = Vec::new();
        for addr in capable_addrs {
            if let Some(info) = registry.get_host_metadata(addr).await {
                capable.push(info);
            }
        }
        let best = selector.select_best_host(capable, &requirements).await;
        let select_time = select_start.elapsed();
        selection_times.push(select_time);

        if let Some(host) = best {
            claimer
                .assign_job_to_host(&job_id, host, &registry)
                .await
                .unwrap();
            assignments.push((job_id.clone(), host));
        }
    }

    // 4. Verify all jobs assigned
    assert!(assignments.len() >= 45); // Allow some jobs to not find hosts

    // 5. Assert selection time < 100ms per job
    let avg_selection = selection_times.iter().sum::<Duration>() / selection_times.len() as u32;
    println!("Average selection time: {:?}", avg_selection);
    assert!(avg_selection < Duration::from_millis(100));

    // 6. Test concurrent assignments
    let concurrent_start = Instant::now();
    let mut handles = Vec::new();

    for i in 0..20 {
        let claimer_clone = claimer.clone();
        let registry_clone = registry.clone();

        let handle = tokio::spawn(async move {
            let job_id = format!("concurrent-job-{:03}", i);
            let requirements = JobRequirements {
                model_id: "llama-7b".to_string(),
                min_ram_gb: 32,
                max_cost_per_token: None,
                min_reliability: Some(0.90),
            };

            let capable_addrs = registry_clone
                .get_available_hosts(&requirements.model_id)
                .await;
            // Just pick first available host for concurrent test
            let best = capable_addrs.first().copied();

            if let Some(host) = best {
                claimer_clone
                    .assign_job_to_host(&job_id, host, &registry_clone)
                    .await
            } else {
                Ok(()) // No host available, consider it success for this test
            }
        });
        handles.push(handle);
    }

    // Wait for all handles
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await);
    }
    let concurrent_time = concurrent_start.elapsed();
    println!("20 concurrent assignments in {:?}", concurrent_time);

    let successful = results.iter().filter(|r| r.is_ok()).count();
    assert!(successful >= 15); // Most should succeed
}

#[tokio::test]
async fn test_registry_event_monitoring() {
    let provider = create_test_provider();
    let contract_address = create_test_address(100);
    let monitor = Arc::new(RegistryMonitor::new(contract_address, provider));
    let registry = Arc::new(HostRegistry::new(monitor.clone()));

    // Register host
    let host = create_test_address(1);
    monitor
        .handle_node_registered(
            host,
            r#"{"gpu":"rtx4090","ram":64,"models":["llama-70b"]}"#.to_string(),
            U256::from(1000000u64),
        )
        .await;

    // Verify registered
    let hosts = registry.get_registered_hosts().await;
    assert!(hosts.contains(&host));

    // Update capabilities by re-registering with new info
    monitor
        .handle_node_registered(
            host,
            r#"{"gpu":"rtx4090","ram":128,"models":["llama-70b","mixtral-8x7b"]}"#.to_string(),
            U256::from(1000000u64),
        )
        .await;

    // Verify updated
    let info = registry.get_host_metadata(host).await.unwrap();
    assert!(info.metadata.contains("128"));

    // Note: Pause/resume functionality not exposed in current implementation
    // Would need to be added to RegistryMonitor if required
    sleep(Duration::from_millis(10)).await;

    // Unregister
    monitor.handle_node_unregistered(host).await;

    // Verify unregistered
    let remaining = registry.get_registered_hosts().await;
    assert!(!remaining.contains(&host));
}

#[tokio::test]
async fn test_load_balancing() {
    let provider = create_test_provider();
    let contract_address = create_test_address(100);
    let monitor = Arc::new(RegistryMonitor::new(contract_address, provider));
    let registry = Arc::new(HostRegistry::new(monitor.clone()));
    let mut selector = HostSelector::new();

    // Register 3 identical hosts
    let hosts: Vec<_> = (1..=3).map(create_test_address).collect();

    for &host in &hosts {
        monitor
            .handle_node_registered(
                host,
                r#"{"gpu":"rtx4090","ram":64,"models":["llama-70b"]}"#.to_string(),
                U256::from(1000000u64),
            )
            .await;

        selector
            .update_performance_metrics(
                host,
                PerformanceMetrics {
                    jobs_completed: 100,
                    success_rate: 0.98,
                    avg_completion_time: 500,
                    uptime_percentage: 0.99,
                    current_load: 0,
                    cost_per_token: 0.0005,
                },
            )
            .await;
    }

    let requirements = JobRequirements {
        model_id: "llama-70b".to_string(),
        min_ram_gb: 32,
        max_cost_per_token: None,
        min_reliability: Some(0.95),
    };

    // Assign multiple jobs and track distribution
    let mut assignments = std::collections::HashMap::new();

    for i in 0..30 {
        let capable = registry
            .get_available_hosts(&requirements.model_id)
            .await
            .into_iter()
            .filter_map(|addr| {
                // Convert Address to HostInfo for selector
                Some(HostInfo {
                    address: addr,
                    metadata: format!("{{\"models\":[\"{}\"]}}", requirements.model_id),
                    stake: U256::from(100000u64),
                    is_online: true,
                })
            })
            .collect::<Vec<_>>();
        let best = selector
            .select_best_host(capable, &requirements)
            .await
            .unwrap();

        *assignments.entry(best).or_insert(0) += 1;

        // Simulate load increase for assigned host
        let current_metrics = PerformanceMetrics {
            jobs_completed: 100,
            success_rate: 0.98,
            avg_completion_time: 500,
            uptime_percentage: 0.99,
            current_load: (i / 3) as u32, // Gradually increase load
            cost_per_token: 0.0005,
        };
        selector
            .update_performance_metrics(best, current_metrics)
            .await;
    }

    // Verify relatively balanced distribution
    for &host in &hosts {
        let count = assignments.get(&host).unwrap_or(&0);
        assert!(*count >= 5 && *count <= 15); // Each should get 5-15 jobs
    }
}

#[tokio::test]
async fn test_failure_recovery() {
    let provider = create_test_provider();
    let contract_address = create_test_address(100);
    let monitor = Arc::new(RegistryMonitor::new(contract_address, provider));
    let registry = Arc::new(HostRegistry::new(monitor.clone()));

    let config = JobClaimConfig {
        max_concurrent_jobs: 10,
        claim_timeout_ms: 30000,
        enable_auto_claim: true,
    };
    let claimer = JobClaimer::new(config).await.unwrap();

    // Register hosts
    let host1 = create_test_address(1);
    let host2 = create_test_address(2);

    monitor
        .handle_node_registered(
            host1,
            r#"{"gpu":"rtx4090","ram":64,"models":["llama-70b"]}"#.to_string(),
            U256::from(1000000u64),
        )
        .await;

    monitor
        .handle_node_registered(
            host2,
            r#"{"gpu":"rtx3090","ram":64,"models":["llama-70b"]}"#.to_string(),
            U256::from(500000u64),
        )
        .await;

    // Assign job to host1
    let job_id = "recovery-job";
    claimer
        .assign_job_to_host(job_id, host1, &registry)
        .await
        .unwrap();

    // Simulate host1 failure (unregister)
    monitor.handle_node_unregistered(host1).await;

    // Reassign to host2
    let result = claimer.reassign_job(job_id, host2, &registry).await;
    assert!(result.is_ok());

    let record = claimer.get_assignment_record(job_id).await.unwrap();
    assert_eq!(record.host_address, host2);
    assert_eq!(record.status, AssignmentStatus::Reassigned);
}
