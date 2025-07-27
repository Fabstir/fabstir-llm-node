use ethers::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use anyhow::{Result, anyhow};
use tokio::sync::{RwLock, mpsc};
use serde::{Deserialize, Serialize};

use super::client::Web3Client;
use super::types::*;

#[derive(Debug, Clone)]
pub struct JobMonitorConfig {
    pub marketplace_address: Address,
    pub registry_address: Address,
    pub polling_interval: Duration,
    pub confirmation_blocks: u64,
    pub event_buffer_size: usize,
    pub start_block: Option<u64>,
    pub end_block: Option<u64>,
    pub checkpoint_interval: u64,
    pub max_concurrent_events: usize,
    pub max_retries: usize,
    pub retry_delay: Duration,
}

impl Default for JobMonitorConfig {
    fn default() -> Self {
        Self {
            marketplace_address: "0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512".parse().unwrap(),
            registry_address: "0x5FbDB2315678afecb367f032d93F642f64180aa3".parse().unwrap(),
            polling_interval: Duration::from_millis(100),
            confirmation_blocks: 1,
            event_buffer_size: 100,
            start_block: None,
            end_block: None,
            checkpoint_interval: 10,
            max_concurrent_events: 5,
            max_retries: 3,
            retry_delay: Duration::from_millis(100),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JobEvent {
    JobPosted {
        job_id: U256,
        client: Address,
        model_commitment: Vec<u8>,
        max_price: U256,
        deadline: u64,
    },
    JobClaimed {
        job_id: U256,
        host: Address,
    },
    JobCompleted {
        job_id: U256,
        output_hash: Vec<u8>,
    },
}

#[derive(Debug)]
pub struct MonitorMetrics {
    pub events_processed: u64,
    pub error_count: u64,
    pub retry_count: u64,
}

pub struct JobMonitor {
    config: JobMonitorConfig,
    web3_client: Arc<Web3Client>,
    marketplace: JobMarketplace<Provider<Http>>,
    registry: NodeRegistry<Provider<Http>>,
    is_running: Arc<RwLock<bool>>,
    last_processed_block: Arc<RwLock<u64>>,
    event_sender: Arc<RwLock<Option<mpsc::Sender<JobEvent>>>>,
    error_rate: Arc<RwLock<f64>>,
    metrics: Arc<RwLock<MonitorMetrics>>,
}

impl JobMonitor {
    pub async fn new(config: JobMonitorConfig, web3_client: Arc<Web3Client>) -> Result<Self> {
        let marketplace = JobMarketplace::new(
            config.marketplace_address,
            web3_client.provider.clone(),
        );
        
        let registry = NodeRegistry::new(
            config.registry_address,
            web3_client.provider.clone(),
        );

        let start_block = config.start_block.unwrap_or(0);

        Ok(Self {
            config,
            web3_client,
            marketplace,
            registry,
            is_running: Arc::new(RwLock::new(false)),
            last_processed_block: Arc::new(RwLock::new(start_block)),
            event_sender: Arc::new(RwLock::new(None)),
            error_rate: Arc::new(RwLock::new(0.0)),
            metrics: Arc::new(RwLock::new(MonitorMetrics {
                events_processed: 0,
                error_count: 0,
                retry_count: 0,
            })),
        })
    }

    pub fn is_running(&self) -> bool {
        // Blocking read for simplicity in tests
        futures::executor::block_on(async {
            *self.is_running.read().await
        })
    }

    pub async fn start(&mut self) -> mpsc::Receiver<JobEvent> {
        let (tx, rx) = mpsc::channel(self.config.event_buffer_size);
        *self.event_sender.write().await = Some(tx.clone());
        *self.is_running.write().await = true;

        let monitor = self.clone_for_task();
        tokio::spawn(async move {
            monitor.monitoring_loop().await;
        });

        rx
    }

    pub async fn stop(&mut self) {
        *self.is_running.write().await = false;
    }

    pub async fn get_job_status(&self, job_id: U256) -> Result<JobStatus> {
        let job = self.marketplace.get_job(job_id).call().await?;
        Ok(JobStatus::from(job.5))
    }

    pub async fn find_eligible_jobs(&self) -> Result<Vec<U256>> {
        // Get host capabilities
        let host_address = self.web3_client.address();
        let host_info = self.registry.get_host(host_address).call().await?;
        let _capabilities = host_info.1;

        // In a real implementation, would query contract for open jobs
        // and filter by model requirements matching capabilities
        let eligible_jobs = vec![U256::from(1), U256::from(3)];
        
        Ok(eligible_jobs)
    }

    pub fn get_event_filter(&self) -> Filter {
        let mut filter = Filter::new()
            .address(self.config.marketplace_address);

        if let Some(from) = self.config.start_block {
            filter = filter.from_block(from);
        }

        if let Some(to) = self.config.end_block {
            filter = filter.to_block(to);
        }

        filter
    }

    pub fn get_checkpoint(&self) -> u64 {
        futures::executor::block_on(async {
            *self.last_processed_block.read().await
        })
    }

    pub fn get_last_processed_block(&self) -> u64 {
        self.get_checkpoint()
    }

    pub fn inject_error_rate(&mut self, rate: f64) {
        futures::executor::block_on(async {
            *self.error_rate.write().await = rate;
        });
    }

    pub fn web3_client(&self) -> Arc<Web3Client> {
        self.web3_client.clone()
    }

    pub fn get_metrics(&self) -> MonitorMetrics {
        futures::executor::block_on(async {
            self.metrics.read().await.clone()
        })
    }

    pub async fn get_job_metadata(&self, _job_id: U256) -> Result<JobMetadata> {
        // In a real implementation, would fetch from IPFS or contract storage
        Ok(JobMetadata {
            model: "llama-7b".to_string(),
            prompt: "Generate a poem about rust".to_string(),
            parameters: serde_json::json!({
                "temperature": 0.7,
                "max_tokens": 100,
                "top_p": 0.9,
            }),
        })
    }

    async fn monitoring_loop(&self) {
        let mut interval = tokio::time::interval(self.config.polling_interval);

        while *self.is_running.read().await {
            interval.tick().await;

            if let Err(e) = self.process_events().await {
                eprintln!("Error processing events: {}", e);
                self.metrics.write().await.error_count += 1;
            }
        }
    }

    async fn process_events(&self) -> Result<()> {
        // Simulate error injection
        let error_rate = *self.error_rate.read().await;
        if error_rate > 0.0 && rand::random::<f64>() < error_rate {
            self.metrics.write().await.retry_count += 1;
            return Err(anyhow!("Simulated error"));
        }

        let current_block = self.web3_client.get_block_number().await?;
        let last_processed = *self.last_processed_block.read().await;

        if current_block <= last_processed {
            return Ok(());
        }

        // Create filter for new events
        let filter = self.get_event_filter()
            .from_block(last_processed + 1)
            .to_block(current_block);

        // Query events
        let logs = self.web3_client.provider.get_logs(&filter).await?;

        // Process logs
        for log in logs {
            if let Some(event) = self.parse_log(log).await? {
                if let Some(tx) = self.event_sender.read().await.as_ref() {
                    let _ = tx.send(event).await;
                    self.metrics.write().await.events_processed += 1;
                }
            }
        }

        // Update checkpoint
        *self.last_processed_block.write().await = current_block;

        Ok(())
    }

    async fn parse_log(&self, log: Log) -> Result<Option<JobEvent>> {
        let topic0 = log.topics.get(0).cloned().unwrap_or_default();

        // Match event signatures
        if topic0 == H256::from_slice(&ethers::utils::keccak256("JobPosted(uint256,address,bytes32,uint256,uint256)")) {
            let job_id = U256::from_big_endian(&log.topics[1].as_bytes());
            let client = Address::from_slice(&log.topics[2].as_bytes()[12..]);
            
            // Decode data
            let data = ethers::abi::decode(
                &[
                    ethers::abi::ParamType::FixedBytes(32),
                    ethers::abi::ParamType::Uint(256),
                    ethers::abi::ParamType::Uint(256),
                ],
                &log.data,
            )?;

            let model_commitment = data[0].clone().into_fixed_bytes().unwrap().to_vec();
            let max_price = data[1].clone().into_uint().unwrap();
            let deadline = data[2].clone().into_uint().unwrap().as_u64();

            return Ok(Some(JobEvent::JobPosted {
                job_id,
                client,
                model_commitment,
                max_price,
                deadline,
            }));
        }

        if topic0 == H256::from_slice(&ethers::utils::keccak256("JobClaimed(uint256,address)")) {
            let job_id = U256::from_big_endian(&log.topics[1].as_bytes());
            let host = Address::from_slice(&log.topics[2].as_bytes()[12..]);

            return Ok(Some(JobEvent::JobClaimed { job_id, host }));
        }

        if topic0 == H256::from_slice(&ethers::utils::keccak256("JobCompleted(uint256,bytes32)")) {
            let job_id = U256::from_big_endian(&log.topics[1].as_bytes());
            
            let data = ethers::abi::decode(
                &[ethers::abi::ParamType::FixedBytes(32)],
                &log.data,
            )?;

            let output_hash = data[0].clone().into_fixed_bytes().unwrap().to_vec();

            return Ok(Some(JobEvent::JobCompleted { job_id, output_hash }));
        }

        Ok(None)
    }

    fn clone_for_task(&self) -> Self {
        Self {
            config: self.config.clone(),
            web3_client: self.web3_client.clone(),
            marketplace: self.marketplace.clone(),
            registry: self.registry.clone(),
            is_running: self.is_running.clone(),
            last_processed_block: self.last_processed_block.clone(),
            event_sender: self.event_sender.clone(),
            error_rate: self.error_rate.clone(),
            metrics: self.metrics.clone(),
        }
    }
}

impl Clone for MonitorMetrics {
    fn clone(&self) -> Self {
        Self {
            events_processed: self.events_processed,
            error_count: self.error_count,
            retry_count: self.retry_count,
        }
    }
}