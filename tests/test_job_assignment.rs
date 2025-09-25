use ethers::prelude::*;
use fabstir_llm_node::contracts::registry_monitor::RegistryMonitor;
use fabstir_llm_node::host::registry::{HostInfo, HostRegistry};
use fabstir_llm_node::host::selection::{HostSelector, JobRequirements, PerformanceMetrics};
use fabstir_llm_node::job_assignment_types::{AssignmentRecord, AssignmentStatus, JobClaimConfig};
use fabstir_llm_node::job_claim::JobClaimer;
use std::sync::Arc;
use tokio::sync::RwLock;

// Helper to create mock JobClaimer
async fn create_mock_claimer() -> JobClaimer {
    let config = JobClaimConfig {
        max_concurrent_jobs: 5,
        claim_timeout_ms: 30000,
        enable_auto_claim: false,
    };

    JobClaimer::new(config).await.unwrap()
}

// Helper to create mock HostRegistry with hosts
async fn create_mock_registry() -> Arc<HostRegistry> {
    let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
    let contract_address = "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
        .parse::<Address>()
        .unwrap();

    let monitor = Arc::new(RegistryMonitor::new(contract_address, Arc::new(provider)));

    // Add some test hosts
    let host1 = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();
    let host2 = "0x2222222222222222222222222222222222222222"
        .parse::<Address>()
        .unwrap();

    monitor
        .handle_node_registered(
            host1,
            r#"{"gpu":"rtx4090","ram":64,"models":["llama-7b"]}"#.to_string(),
            U256::from(1000000u64),
        )
        .await;

    monitor
        .handle_node_registered(
            host2,
            r#"{"gpu":"rtx3090","ram":32,"models":["llama-7b"]}"#.to_string(),
            U256::from(500000u64),
        )
        .await;

    Arc::new(HostRegistry::new(monitor))
}

// Helper to create mock HostSelector with metrics
async fn create_mock_selector() -> Arc<HostSelector> {
    let mut selector = HostSelector::new();

    let host1 = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();
    let host2 = "0x2222222222222222222222222222222222222222"
        .parse::<Address>()
        .unwrap();

    selector
        .update_performance_metrics(
            host1,
            PerformanceMetrics {
                jobs_completed: 100,
                success_rate: 0.98,
                avg_completion_time: 500,
                uptime_percentage: 0.99,
                current_load: 2,
                cost_per_token: 0.0002,
            },
        )
        .await;

    selector
        .update_performance_metrics(
            host2,
            PerformanceMetrics {
                jobs_completed: 50,
                success_rate: 0.95,
                avg_completion_time: 750,
                uptime_percentage: 0.97,
                current_load: 1,
                cost_per_token: 0.0001,
            },
        )
        .await;

    Arc::new(selector)
}

#[tokio::test]
async fn test_assign_job_to_specific_host() {
    // Test single job assignment to specific host
    let claimer = create_mock_claimer().await;
    let registry = create_mock_registry().await;

    let job_id = "job-001";
    let host_address = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();

    let result = claimer
        .assign_job_to_host(job_id, host_address, &registry)
        .await;
    assert!(result.is_ok());

    // Verify assignment record exists
    let record = claimer.get_assignment_record(job_id).await;
    assert!(record.is_some());
    let record = record.unwrap();
    assert_eq!(record.job_id, job_id);
    assert_eq!(record.host_address, host_address);
    assert_eq!(record.status, AssignmentStatus::Confirmed);
}

#[tokio::test]
async fn test_batch_assign_multiple_jobs() {
    // Test batch assignment of multiple jobs
    let claimer = create_mock_claimer().await;
    let registry = create_mock_registry().await;

    let host1 = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();
    let host2 = "0x2222222222222222222222222222222222222222"
        .parse::<Address>()
        .unwrap();

    let assignments = vec![("job-001", host1), ("job-002", host2), ("job-003", host1)];

    let results = claimer
        .batch_assign_jobs(assignments, &registry)
        .await
        .unwrap();

    // All assignments should succeed
    assert_eq!(results.len(), 3);
    for result in results {
        assert!(result.is_ok());
    }

    // Verify all records exist
    assert!(claimer.get_assignment_record("job-001").await.is_some());
    assert!(claimer.get_assignment_record("job-002").await.is_some());
    assert!(claimer.get_assignment_record("job-003").await.is_some());
}

#[tokio::test]
async fn test_reassign_job_to_new_host() {
    // Test job reassignment to different host
    let claimer = create_mock_claimer().await;
    let registry = create_mock_registry().await;

    let job_id = "job-001";
    let original_host = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();
    let new_host = "0x2222222222222222222222222222222222222222"
        .parse::<Address>()
        .unwrap();

    // First assign to original host
    claimer
        .assign_job_to_host(job_id, original_host, &registry)
        .await
        .unwrap();

    // Then reassign to new host
    let result = claimer.reassign_job(job_id, new_host, &registry).await;
    assert!(result.is_ok());

    // Verify assignment updated
    let record = claimer.get_assignment_record(job_id).await.unwrap();
    assert_eq!(record.host_address, new_host);
    assert_eq!(record.status, AssignmentStatus::Reassigned);
}

#[tokio::test]
async fn test_auto_assign_picks_best_host() {
    // Test that auto-assignment uses HostSelector to pick best host
    let claimer = create_mock_claimer().await;
    let registry = create_mock_registry().await;
    let selector = create_mock_selector().await;

    let job_id = "job-001";
    let requirements = JobRequirements {
        model_id: "llama-7b".to_string(),
        min_ram_gb: 32,
        max_cost_per_token: None,
        min_reliability: Some(0.9),
    };

    let result = claimer
        .auto_assign_job(job_id, &registry, &selector, &requirements)
        .await;
    assert!(result.is_ok());

    let assigned_host = result.unwrap();
    // Should pick host2 (better overall score due to lower cost and load)
    assert_eq!(
        assigned_host,
        "0x2222222222222222222222222222222222222222"
            .parse::<Address>()
            .unwrap()
    );

    // Verify assignment record
    let record = claimer.get_assignment_record(job_id).await.unwrap();
    assert_eq!(record.host_address, assigned_host);
}

#[tokio::test]
async fn test_assignment_failure_handling() {
    // Test handling of assignment failures
    let claimer = create_mock_claimer().await;
    let registry = create_mock_registry().await;

    let job_id = "job-001";
    let invalid_host = "0x9999999999999999999999999999999999999999"
        .parse::<Address>()
        .unwrap(); // Not registered

    let result = claimer
        .assign_job_to_host(job_id, invalid_host, &registry)
        .await;
    assert!(result.is_err());

    // Should not create assignment record for failed assignment
    let record = claimer.get_assignment_record(job_id).await;
    assert!(record.is_none());
}

#[tokio::test]
async fn test_priority_jobs_assigned_first() {
    // Test that priority jobs are assigned before regular jobs
    let claimer = create_mock_claimer().await;
    let registry = create_mock_registry().await;

    let host = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();

    // Add jobs with different priorities
    claimer.add_priority_job("urgent-job", 10).await;
    claimer.add_priority_job("normal-job", 5).await;
    claimer.add_priority_job("low-priority", 1).await;

    // Process assignments
    let processed = claimer
        .process_priority_assignments(host, &registry, 3)
        .await;
    assert_eq!(processed.len(), 3);

    // Verify order (highest priority first)
    assert_eq!(processed[0], "urgent-job");
    assert_eq!(processed[1], "normal-job");
    assert_eq!(processed[2], "low-priority");
}

#[tokio::test]
async fn test_concurrent_assignment_safety() {
    // Test thread-safe concurrent assignments
    let claimer = Arc::new(create_mock_claimer().await);
    let registry = create_mock_registry().await;
    let registry = Arc::new(registry);

    let mut handles = vec![];

    for i in 0..10 {
        let claimer_clone = claimer.clone();
        let registry_clone = registry.clone();

        let handle = tokio::spawn(async move {
            let job_id = format!("job-{:03}", i);
            let host = if i % 2 == 0 {
                "0x1111111111111111111111111111111111111111"
            } else {
                "0x2222222222222222222222222222222222222222"
            }
            .parse::<Address>()
            .unwrap();

            claimer_clone
                .assign_job_to_host(&job_id, host, &registry_clone)
                .await
        });
        handles.push(handle);
    }

    // Wait for all assignments
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    // Verify all assignments recorded
    for i in 0..10 {
        let job_id = format!("job-{:03}", i);
        assert!(claimer.get_assignment_record(&job_id).await.is_some());
    }
}

#[tokio::test]
async fn test_get_host_assignments() {
    // Test getting all assignments for a specific host
    let claimer = create_mock_claimer().await;
    let registry = create_mock_registry().await;

    let host1 = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();
    let host2 = "0x2222222222222222222222222222222222222222"
        .parse::<Address>()
        .unwrap();

    // Assign multiple jobs
    claimer
        .assign_job_to_host("job-001", host1, &registry)
        .await
        .unwrap();
    claimer
        .assign_job_to_host("job-002", host1, &registry)
        .await
        .unwrap();
    claimer
        .assign_job_to_host("job-003", host2, &registry)
        .await
        .unwrap();

    // Get assignments for host1
    let host1_jobs = claimer.get_host_assignments(host1).await;
    assert_eq!(host1_jobs.len(), 2);
    assert!(host1_jobs.contains(&"job-001".to_string()));
    assert!(host1_jobs.contains(&"job-002".to_string()));

    // Get assignments for host2
    let host2_jobs = claimer.get_host_assignments(host2).await;
    assert_eq!(host2_jobs.len(), 1);
    assert!(host2_jobs.contains(&"job-003".to_string()));
}
