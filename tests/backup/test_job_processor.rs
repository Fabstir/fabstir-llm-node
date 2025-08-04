// tests/test_job_processor.rs

use ethers::prelude::*;
use ethers::types::{Address, H256, U256};
use libp2p::{PeerId, Multiaddr};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{sleep, Duration};

use fabstir_llm_node::{
    JobProcessor, 
    JobStatus,
    JobRequest,
    JobResult,
    LLMService,
    JobNodeConfig,
    ContractClientTrait,
    JobEvent,
};

#[derive(Debug, Clone)]
struct MockContractClient {
    jobs: Arc<RwLock<Vec<JobRequest>>>,
    events: mpsc::Sender<JobEvent>,
}


impl MockContractClient {
    fn new() -> (Self, mpsc::Receiver<JobEvent>) {
        let (tx, rx) = mpsc::channel(100);
        (
            Self {
                jobs: Arc::new(RwLock::new(Vec::new())),
                events: tx,
            },
            rx,
        )
    }
    
    async fn emit_job_event(&self, job: JobEvent) {
        self.events.send(job).await.unwrap();
    }
}

#[async_trait::async_trait]
impl ContractClientTrait for MockContractClient {
    async fn emit_job_event(&self, job_event: JobEvent) -> anyhow::Result<()> {
        self.events.send(job_event).await.unwrap();
        Ok(())
    }

    async fn is_connected(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn test_job_processor_initialization() {
    let config = JobNodeConfig {
        peer_id: PeerId::random(),
        listen_addr: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
        contract_address: Address::random(),
        private_key: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        rpc_url: "http://localhost:8545".to_string(),
        models_dir: "./models".to_string(),
        supported_models: vec![],
        min_payment: U256::zero(),
        enable_priority_queue: false,
        event_poll_interval: Duration::from_secs(5),
        max_reconnect_attempts: 3,
        node_address: Address::random(),
        ..Default::default()
    };
    
    let (contract_client, _) = MockContractClient::new();
    let llm_service = LLMService::new(&config.models_dir).await.unwrap();
    
    let processor = JobProcessor::new(
        config,
        Arc::new(contract_client),
        Arc::new(llm_service),
    );
    
    assert!(processor.is_running());
    assert_eq!(processor.get_active_jobs().await, 0);
    assert_eq!(processor.get_completed_jobs().await, 0);
}

#[tokio::test]
async fn test_monitor_job_events() {
    let config = JobNodeConfig::default();
    let (contract_client, mut event_rx) = MockContractClient::new();
    let llm_service = LLMService::new("./models").await.unwrap();
    
    let processor = JobProcessor::new(
        config,
        Arc::new(contract_client.clone()),
        Arc::new(llm_service),
    );
    
    // Start monitoring  
    let processor_clone = processor.clone();
    let monitor_handle = tokio::spawn(async move {
        processor_clone.start_monitoring().await
    });
    
    // Emit test events
    let job1 = JobEvent {
        job_id: H256::random(),
        requester: Address::random(),
        model_id: "llama3-70b".to_string(),
        max_tokens: 1000,
        parameters: r#"{"temperature": 0.7, "top_p": 0.9}"#.to_string(),
        payment_amount: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
    };
    
    let job2 = JobEvent {
        job_id: H256::random(),
        requester: Address::random(),
        model_id: "mistral-7b".to_string(),
        max_tokens: 500,
        parameters: r#"{"temperature": 0.5}"#.to_string(),
        payment_amount: U256::from(500_000_000_000_000_000u64), // 0.5 ETH
    };
    
    // Process the events directly on the processor (since it implements ContractClientTrait)
    processor.emit_job_event(job1.clone()).await.unwrap();
    processor.emit_job_event(job2.clone()).await.unwrap();
    
    // Verify events were received
    let pending_jobs = processor.get_pending_jobs().await;
    assert_eq!(pending_jobs.len(), 2);
    assert!(pending_jobs.iter().any(|j| j.job_id == job1.job_id));
    assert!(pending_jobs.iter().any(|j| j.job_id == job2.job_id));
}

#[tokio::test]
async fn test_job_filtering_by_model() {
    let mut config = JobNodeConfig::default();
    config.supported_models = vec!["llama3-70b".to_string(), "llama3-13b".to_string()];
    
    let (contract_client, _) = MockContractClient::new();
    let llm_service = LLMService::new("./models").await.unwrap();
    
    let processor = JobProcessor::new(
        config,
        Arc::new(contract_client.clone()),
        Arc::new(llm_service),
    );
    
    // Emit jobs for different models
    let supported_job = JobEvent {
        job_id: H256::random(),
        requester: Address::random(),
        model_id: "llama3-70b".to_string(),
        max_tokens: 100,
        parameters: "{}".to_string(),
        payment_amount: U256::from(1_000_000_000_000_000_000u64),
    };
    
    let unsupported_job = JobEvent {
        job_id: H256::random(),
        model_id: "gpt-4".to_string(),
        requester: Address::random(),
        max_tokens: 100,
        parameters: "{}".to_string(),
        payment_amount: U256::from(1_000_000_000_000_000_000u64),
    };
    
    processor.emit_job_event(supported_job.clone()).await.unwrap();
    processor.emit_job_event(unsupported_job.clone()).await.unwrap();
    
    sleep(Duration::from_millis(100)).await;
    
    // Only supported model job should be pending
    let pending_jobs = processor.get_pending_jobs().await;
    assert_eq!(pending_jobs.len(), 1);
    assert_eq!(pending_jobs[0].model_id, "llama3-70b");
}

#[tokio::test]
async fn test_job_filtering_by_payment() {
    let mut config = JobNodeConfig::default();
    config.min_payment = U256::from(100_000_000_000_000_000u64); // 0.1 ETH minimum
    
    let (contract_client, _) = MockContractClient::new();
    let llm_service = LLMService::new("./models").await.unwrap();
    
    let processor = JobProcessor::new(
        config,
        Arc::new(contract_client.clone()),
        Arc::new(llm_service),
    );
    
    // Emit jobs with different payments
    let high_payment_job = JobEvent {
        job_id: H256::random(),
        requester: Address::random(),
        model_id: "llama3-70b".to_string(),
        max_tokens: 100,
        parameters: "{}".to_string(),
        payment_amount: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
    };
    
    let low_payment_job = JobEvent {
        job_id: H256::random(),
        requester: Address::random(),
        model_id: "llama3-70b".to_string(),
        max_tokens: 100,
        parameters: "{}".to_string(),
        payment_amount: U256::from(10_000_000_000_000_000u64), // 0.01 ETH
    };
    
    processor.emit_job_event(high_payment_job.clone()).await.unwrap();
    processor.emit_job_event(low_payment_job.clone()).await.unwrap();
    
    sleep(Duration::from_millis(100)).await;
    
    // Only high payment job should be pending
    let pending_jobs = processor.get_pending_jobs().await;
    assert_eq!(pending_jobs.len(), 1);
    assert_eq!(pending_jobs[0].payment_amount, high_payment_job.payment_amount);
}

#[tokio::test]
async fn test_concurrent_job_monitoring() {
    let config = JobNodeConfig::default();
    let (contract_client, _) = MockContractClient::new();
    let llm_service = LLMService::new("./models").await.unwrap();
    
    let processor = JobProcessor::new(
        config,
        Arc::new(contract_client.clone()),
        Arc::new(llm_service),
    );
    
    // Start multiple monitoring tasks
    let handles: Vec<_> = (0..3)
        .map(|_| {
            let proc = processor.clone();
            tokio::spawn(async move {
                proc.start_monitoring().await
            })
        })
        .collect();
    
    // Emit many jobs concurrently
    let jobs: Vec<_> = (0..10)
        .map(|i| JobEvent {
            job_id: H256::from_low_u64_be(i),
            requester: Address::random(),
            model_id: format!("model-{}", i % 3),
            max_tokens: 100 * (i + 1) as u32,
            parameters: "{}".to_string(),
            payment_amount: U256::from(i + 1) * U256::from(100_000_000_000_000_000u64),
        })
        .collect();
    
    for job in &jobs {
        processor.emit_job_event(job.clone()).await.unwrap();
    }
    
    sleep(Duration::from_millis(200)).await;
    
    // All jobs should be received (no duplicates from multiple monitors)
    let pending_jobs = processor.get_pending_jobs().await;
    assert_eq!(pending_jobs.len(), jobs.len());
}

#[tokio::test]
async fn test_job_priority_queue() {
    let mut config = JobNodeConfig::default();
    config.enable_priority_queue = true;
    
    let (contract_client, _) = MockContractClient::new();
    let llm_service = LLMService::new("./models").await.unwrap();
    
    let processor = JobProcessor::new(
        config,
        Arc::new(contract_client.clone()),
        Arc::new(llm_service),
    );
    
    // Emit jobs with different payments (priority)
    let low_priority = JobEvent {
        job_id: H256::from_low_u64_be(1),
        requester: Address::random(),
        model_id: "llama3-70b".to_string(),
        max_tokens: 100,
        parameters: "{}".to_string(),
        payment_amount: U256::from(100_000_000_000_000_000u64), // 0.1 ETH
    };
    
    let high_priority = JobEvent {
        job_id: H256::from_low_u64_be(2),
        requester: Address::random(),
        model_id: "llama3-70b".to_string(),
        max_tokens: 100,
        parameters: "{}".to_string(),
        payment_amount: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
    };
    
    let medium_priority = JobEvent {
        job_id: H256::from_low_u64_be(3),
        requester: Address::random(),
        model_id: "llama3-70b".to_string(),
        max_tokens: 100,
        parameters: "{}".to_string(),
        payment_amount: U256::from(500_000_000_000_000_000u64), // 0.5 ETH
    };
    
    // Emit in order: low, high, medium
    processor.emit_job_event(low_priority.clone()).await.unwrap();
    processor.emit_job_event(high_priority.clone()).await.unwrap();
    processor.emit_job_event(medium_priority.clone()).await.unwrap();
    
    sleep(Duration::from_millis(100)).await;
    
    // Get next job should return highest payment first
    let next_job = processor.get_next_job().await.unwrap();
    assert_eq!(next_job.job_id, high_priority.job_id);
    
    let next_job = processor.get_next_job().await.unwrap();
    assert_eq!(next_job.job_id, medium_priority.job_id);
    
    let next_job = processor.get_next_job().await.unwrap();
    assert_eq!(next_job.job_id, low_priority.job_id);
}

#[tokio::test]
async fn test_job_status_tracking() {
    let config = JobNodeConfig::default();
    let (contract_client, _) = MockContractClient::new();
    let llm_service = LLMService::new("./models").await.unwrap();
    
    let processor = JobProcessor::new(
        config,
        Arc::new(contract_client.clone()),
        Arc::new(llm_service),
    );
    
    let job = JobEvent {
        job_id: H256::random(),
        requester: Address::random(),
        model_id: "llama3-70b".to_string(),
        max_tokens: 100,
        parameters: "{}".to_string(),
        payment_amount: U256::from(1_000_000_000_000_000_000u64),
    };
    
    processor.emit_job_event(job.clone()).await.unwrap();
    sleep(Duration::from_millis(100)).await;
    
    // Check initial status
    let status = processor.get_job_status(job.job_id).await;
    assert_eq!(status, Some(JobStatus::Pending));
    
    // Simulate claiming the job
    processor.update_job_status(job.job_id, JobStatus::Claimed).await;
    let status = processor.get_job_status(job.job_id).await;
    assert_eq!(status, Some(JobStatus::Claimed));
    
    // Simulate processing
    processor.update_job_status(job.job_id, JobStatus::Processing).await;
    let status = processor.get_job_status(job.job_id).await;
    assert_eq!(status, Some(JobStatus::Processing));
    
    // Simulate completion
    processor.update_job_status(job.job_id, JobStatus::Completed).await;
    let status = processor.get_job_status(job.job_id).await;
    assert_eq!(status, Some(JobStatus::Completed));
}

#[tokio::test]
async fn test_event_reconnection() {
    let mut config = JobNodeConfig::default();
    config.event_poll_interval = Duration::from_millis(100);
    config.max_reconnect_attempts = 3;
    
    let (contract_client, _) = MockContractClient::new();
    let llm_service = LLMService::new("./models").await.unwrap();
    
    let processor = JobProcessor::new(
        config,
        Arc::new(contract_client.clone()),
        Arc::new(llm_service),
    );
    
    // Start monitoring to enable reconnection logic
    processor.start_monitoring().await.unwrap();
    
    // Simulate connection failure and recovery
    processor.simulate_disconnect().await;
    assert!(!processor.is_connected().await);
    
    // Should attempt reconnection
    sleep(Duration::from_millis(300)).await;
    
    // Verify reconnection
    assert!(processor.is_connected().await);
    assert_eq!(processor.get_reconnect_count().await, 1);
}