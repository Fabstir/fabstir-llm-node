use fabstir_llm_node::contracts::{JobMonitor, JobMonitorConfig, JobEvent, JobStatus};
use ethers::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_job_monitor_creation() {
    let config = JobMonitorConfig {
        marketplace_address: "0x5FbDB2315678afecb367f032d93F642f64180aa3".parse().unwrap(),
        registry_address: "0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512".parse().unwrap(),
        polling_interval: Duration::from_millis(100),
        confirmation_blocks: 1,
        event_buffer_size: 100,
    };
    
    let web3_client = create_test_web3_client().await;
    let monitor = JobMonitor::new(config, web3_client)
        .await
        .expect("Failed to create job monitor");
    
    // Should be ready to monitor
    assert!(monitor.is_running());
}

#[tokio::test]
async fn test_job_posted_event_monitoring() {
    let config = JobMonitorConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let mut monitor = JobMonitor::new(config, web3_client.clone())
        .await
        .expect("Failed to create job monitor");
    
    // Start monitoring
    let mut event_receiver = monitor.start().await;
    
    // Post a job via contract
    let job_id = post_test_job(&web3_client).await;
    
    // Should receive JobPosted event
    let event = tokio::time::timeout(Duration::from_secs(2), event_receiver.recv())
        .await
        .expect("Timeout waiting for event")
        .expect("Channel closed");
    
    match event {
        JobEvent::JobPosted { 
            job_id: id, 
            client, 
            model_commitment, 
            max_price,
            deadline 
        } => {
            assert_eq!(id, job_id);
            assert!(!client.is_zero());
            assert!(!model_commitment.is_empty());
            assert!(max_price > U256::zero());
            assert!(deadline > 0);
        }
        _ => panic!("Expected JobPosted event, got {:?}", event),
    }
}

#[tokio::test]
async fn test_job_claimed_event_monitoring() {
    let config = JobMonitorConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let mut monitor = JobMonitor::new(config, web3_client.clone())
        .await
        .expect("Failed to create job monitor");
    
    let mut event_receiver = monitor.start().await;
    
    // Post and claim a job
    let job_id = post_test_job(&web3_client).await;
    claim_test_job(&web3_client, job_id).await;
    
    // Skip JobPosted event
    let _ = event_receiver.recv().await;
    
    // Should receive JobClaimed event
    let event = tokio::time::timeout(Duration::from_secs(2), event_receiver.recv())
        .await
        .expect("Timeout waiting for event")
        .expect("Channel closed");
    
    match event {
        JobEvent::JobClaimed { job_id: id, host } => {
            assert_eq!(id, job_id);
            assert!(!host.is_zero());
        }
        _ => panic!("Expected JobClaimed event, got {:?}", event),
    }
}

#[tokio::test]
async fn test_job_completed_event_monitoring() {
    let config = JobMonitorConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let mut monitor = JobMonitor::new(config, web3_client.clone())
        .await
        .expect("Failed to create job monitor");
    
    let mut event_receiver = monitor.start().await;
    
    // Complete job flow
    let job_id = post_test_job(&web3_client).await;
    claim_test_job(&web3_client, job_id).await;
    complete_test_job(&web3_client, job_id).await;
    
    // Skip earlier events
    let _ = event_receiver.recv().await; // JobPosted
    let _ = event_receiver.recv().await; // JobClaimed
    
    // Should receive JobCompleted event
    let event = tokio::time::timeout(Duration::from_secs(2), event_receiver.recv())
        .await
        .expect("Timeout waiting for event")
        .expect("Channel closed");
    
    match event {
        JobEvent::JobCompleted { job_id: id, output_hash } => {
            assert_eq!(id, job_id);
            assert!(!output_hash.is_empty());
        }
        _ => panic!("Expected JobCompleted event, got {:?}", event),
    }
}

#[tokio::test]
async fn test_job_status_tracking() {
    let config = JobMonitorConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let monitor = JobMonitor::new(config, web3_client.clone())
        .await
        .expect("Failed to create job monitor");
    
    // Post a job
    let job_id = post_test_job(&web3_client).await;
    
    // Check job status
    let status = monitor.get_job_status(job_id)
        .await
        .expect("Failed to get job status");
    
    assert_eq!(status, JobStatus::Posted);
    
    // Claim the job
    claim_test_job(&web3_client, job_id).await;
    
    // Status should update
    let status = monitor.get_job_status(job_id)
        .await
        .expect("Failed to get job status");
    
    assert_eq!(status, JobStatus::Claimed);
}

#[tokio::test]
async fn test_eligible_jobs_discovery() {
    let config = JobMonitorConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let monitor = JobMonitor::new(config, web3_client.clone())
        .await
        .expect("Failed to create job monitor");
    
    // Register as a host with specific capabilities
    register_test_host(&web3_client, vec!["llama-7b", "mistral-7b"]).await;
    
    // Post jobs with different requirements
    let job1 = post_test_job_with_model(&web3_client, "llama-7b").await;
    let job2 = post_test_job_with_model(&web3_client, "gpt-4").await;
    let job3 = post_test_job_with_model(&web3_client, "mistral-7b").await;
    
    // Find eligible jobs
    let eligible_jobs = monitor.find_eligible_jobs()
        .await
        .expect("Failed to find eligible jobs");
    
    // Should only find jobs matching our capabilities
    assert_eq!(eligible_jobs.len(), 2);
    assert!(eligible_jobs.contains(&job1));
    assert!(eligible_jobs.contains(&job3));
    assert!(!eligible_jobs.contains(&job2));
}

#[tokio::test]
async fn test_event_filtering_by_block_range() {
    let config = JobMonitorConfig {
        start_block: Some(100),
        end_block: Some(200),
        ..Default::default()
    };
    
    let web3_client = create_test_web3_client().await;
    let monitor = JobMonitor::new(config, web3_client)
        .await
        .expect("Failed to create job monitor");
    
    // Should only monitor events in specified range
    let filter = monitor.get_event_filter();
    assert_eq!(filter.from_block, Some(BlockNumber::Number(100.into())));
    assert_eq!(filter.to_block, Some(BlockNumber::Number(200.into())));
}

#[tokio::test]
async fn test_monitor_restart_from_checkpoint() {
    let config = JobMonitorConfig {
        checkpoint_interval: 10,
        ..Default::default()
    };
    
    let web3_client = create_test_web3_client().await;
    let mut monitor = JobMonitor::new(config.clone(), web3_client.clone())
        .await
        .expect("Failed to create job monitor");
    
    // Start monitoring
    let mut event_receiver = monitor.start().await;
    
    // Post some jobs
    for _ in 0..5 {
        post_test_job(&web3_client).await;
    }
    
    // Collect events
    let mut event_count = 0;
    while let Ok(Some(_)) = tokio::time::timeout(Duration::from_millis(500), event_receiver.recv()).await {
        event_count += 1;
    }
    
    assert_eq!(event_count, 5);
    
    // Get checkpoint
    let checkpoint = monitor.get_checkpoint();
    assert!(checkpoint > 0);
    
    // Stop monitor
    monitor.stop().await;
    
    // Create new monitor from checkpoint
    let mut config_with_checkpoint = config;
    config_with_checkpoint.start_block = Some(checkpoint);
    
    let new_monitor = JobMonitor::new(config_with_checkpoint, web3_client.clone())
        .await
        .expect("Failed to create monitor from checkpoint");
    
    // Should start from checkpoint
    assert_eq!(new_monitor.get_last_processed_block(), checkpoint);
}

#[tokio::test]
async fn test_concurrent_event_processing() {
    let config = JobMonitorConfig {
        max_concurrent_events: 5,
        ..Default::default()
    };
    
    let web3_client = create_test_web3_client().await;
    let mut monitor = JobMonitor::new(config, web3_client.clone())
        .await
        .expect("Failed to create job monitor");
    
    let mut event_receiver = monitor.start().await;
    
    // Post multiple jobs quickly
    let mut job_ids = Vec::new();
    for _ in 0..10 {
        job_ids.push(post_test_job(&web3_client).await);
    }
    
    // Should process events concurrently
    let start = std::time::Instant::now();
    let mut received_count = 0;
    
    while received_count < 10 {
        if let Ok(Some(_)) = tokio::time::timeout(Duration::from_millis(100), event_receiver.recv()).await {
            received_count += 1;
        } else {
            break;
        }
    }
    
    let duration = start.elapsed();
    
    // Should process faster than sequential (assuming some processing delay)
    assert!(duration < Duration::from_secs(2));
    assert_eq!(received_count, 10);
}

#[tokio::test]
async fn test_monitor_error_recovery() {
    let config = JobMonitorConfig {
        max_retries: 3,
        retry_delay: Duration::from_millis(100),
        ..Default::default()
    };
    
    let web3_client = create_test_web3_client().await;
    
    // Create monitor with faulty RPC that recovers
    let mut monitor = JobMonitor::new(config, web3_client)
        .await
        .expect("Failed to create job monitor");
    
    // Simulate RPC errors
    monitor.inject_error_rate(0.5); // 50% error rate
    
    let mut event_receiver = monitor.start().await;
    
    // Post a job
    let job_id = post_test_job(&monitor.web3_client()).await;
    
    // Should eventually receive event despite errors
    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(event) = event_receiver.recv().await {
                if matches!(event, JobEvent::JobPosted { .. }) {
                    return Some(event);
                }
            }
        }
    })
    .await
    .expect("Timeout waiting for event")
    .expect("No event received");
    
    // Verify error metrics
    let metrics = monitor.get_metrics();
    assert!(metrics.error_count > 0);
    assert!(metrics.retry_count > 0);
    assert_eq!(metrics.events_processed, 1);
}

#[tokio::test]
async fn test_job_metadata_retrieval() {
    let config = JobMonitorConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let monitor = JobMonitor::new(config, web3_client.clone())
        .await
        .expect("Failed to create job monitor");
    
    // Post a job with metadata
    let job_id = post_test_job_with_metadata(
        &web3_client,
        "llama-7b",
        "Generate a poem about rust",
        json!({
            "temperature": 0.7,
            "max_tokens": 100,
            "top_p": 0.9
        })
    ).await;
    
    // Retrieve job metadata
    let metadata = monitor.get_job_metadata(job_id)
        .await
        .expect("Failed to get job metadata");
    
    assert_eq!(metadata.model, "llama-7b");
    assert_eq!(metadata.prompt, "Generate a poem about rust");
    assert_eq!(metadata.parameters["temperature"], 0.7);
    assert_eq!(metadata.parameters["max_tokens"], 100);
}

// Helper functions
async fn create_test_web3_client() -> Arc<Web3Client> {
    let config = Web3Config::default();
    Arc::new(Web3Client::new(config).await.expect("Failed to create Web3 client"))
}

async fn post_test_job(client: &Web3Client) -> U256 {
    // Implementation would interact with JobMarketplace contract
    U256::from(1)
}

async fn post_test_job_with_model(client: &Web3Client, model: &str) -> U256 {
    // Implementation would post job with specific model requirement
    U256::from(2)
}

async fn post_test_job_with_metadata(
    client: &Web3Client,
    model: &str,
    prompt: &str,
    params: serde_json::Value
) -> U256 {
    // Implementation would post job with full metadata
    U256::from(3)
}

async fn claim_test_job(client: &Web3Client, job_id: U256) {
    // Implementation would claim job via contract
}

async fn complete_test_job(client: &Web3Client, job_id: U256) {
    // Implementation would complete job via contract
}

async fn register_test_host(client: &Web3Client, capabilities: Vec<&str>) {
    // Implementation would register host with capabilities
}