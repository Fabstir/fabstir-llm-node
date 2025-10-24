// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use ethers::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};

use super::client::Web3Client;
use super::types::*;

#[derive(Debug, Clone)]
pub struct ProofConfig {
    pub proof_system_address: Address,
    pub ezkl_verifier_address: Address,
    pub proof_generation_timeout: Duration,
    pub max_proof_size: usize,
    pub challenge_period: Duration,
    pub enable_batch_submission: bool,
    pub batch_size: usize,
    pub use_proof_compression: bool,
    pub store_proofs_on_ipfs: bool,
    pub max_proof_delay: Duration,
    pub max_resubmission_attempts: usize,
    pub resubmission_delay: Duration,
}

impl Default for ProofConfig {
    fn default() -> Self {
        // Load from environment variable - REQUIRED, NO FALLBACK
        let proof_system_address = std::env::var("PROOF_SYSTEM_ADDRESS")
            .expect("❌ FATAL: PROOF_SYSTEM_ADDRESS environment variable MUST be set")
            .parse()
            .expect("❌ FATAL: Invalid PROOF_SYSTEM_ADDRESS format");

        // EZKL verifier is optional (blank means not deployed)
        let ezkl_verifier_address = std::env::var("EZKL_VERIFIER_ADDRESS")
            .ok()
            .and_then(|s| if s.is_empty() { None } else { s.parse().ok() })
            .unwrap_or_else(Address::zero);

        Self {
            proof_system_address,
            ezkl_verifier_address,
            proof_generation_timeout: Duration::from_secs(300),
            max_proof_size: 10 * 1024,
            challenge_period: Duration::from_secs(86400),
            enable_batch_submission: false,
            batch_size: 5,
            use_proof_compression: false,
            store_proofs_on_ipfs: false,
            max_proof_delay: Duration::from_secs(3600),
            max_resubmission_attempts: 3,
            resubmission_delay: Duration::from_millis(100),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofData {
    pub job_id: U256,
    pub proof: Vec<u8>,
    pub public_inputs: Vec<u8>,
    pub verification_key: Vec<u8>,
    pub model_commitment: Vec<u8>,
    pub input_hash: Vec<u8>,
    pub output_hash: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProofEvent {
    ProofSubmitted {
        job_id: U256,
        submitter: Address,
        proof_hash: Vec<u8>,
    },
    ProofVerified {
        job_id: U256,
        is_valid: bool,
    },
    ProofChallenged {
        job_id: U256,
        challenger: Address,
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct StoredProof {
    pub size: usize,
    pub ipfs_hash: Option<String>,
    pub on_chain_data: Vec<u8>,
}

#[derive(Debug)]
pub struct ProofMetrics {
    pub retry_count: u64,
    pub successful_submissions: u64,
}

pub struct ProofSubmitter {
    config: ProofConfig,
    web3_client: Arc<Web3Client>,
    proof_system: ProofSystem<Provider<Http>>,
    event_sender: Arc<RwLock<Option<mpsc::Sender<ProofEvent>>>>,
    wallet: Arc<RwLock<Option<SignerMiddleware<Arc<Provider<Http>>, LocalWallet>>>>,
    error_rate: Arc<RwLock<f64>>,
    metrics: Arc<RwLock<ProofMetrics>>,
}

impl ProofSubmitter {
    pub async fn new(config: ProofConfig, web3_client: Arc<Web3Client>) -> Result<Self> {
        let proof_system =
            ProofSystem::new(config.proof_system_address, web3_client.provider.clone());

        Ok(Self {
            config,
            web3_client,
            proof_system,
            event_sender: Arc::new(RwLock::new(None)),
            wallet: Arc::new(RwLock::new(None)),
            error_rate: Arc::new(RwLock::new(0.0)),
            metrics: Arc::new(RwLock::new(ProofMetrics {
                retry_count: 0,
                successful_submissions: 0,
            })),
        })
    }

    pub fn is_ready(&self) -> bool {
        true
    }

    pub async fn generate_proof(
        &self,
        job_id: U256,
        model_commitment: Vec<u8>,
        input_hash: Vec<u8>,
        output_hash: Vec<u8>,
    ) -> Result<ProofData> {
        // In a real implementation, would generate EZKL proof
        // For testing, return mock proof
        Ok(ProofData {
            job_id,
            proof: vec![1u8; 256],
            public_inputs: vec![2u8; 64],
            verification_key: vec![3u8; 128],
            model_commitment,
            input_hash,
            output_hash,
        })
    }

    pub fn set_wallet(&mut self, private_key: &str) -> Result<()> {
        let wallet = private_key
            .parse::<LocalWallet>()
            .map_err(|e| anyhow!("Invalid private key: {}", e))?
            .with_chain_id(31337u64); // Default chain ID

        let signer = SignerMiddleware::new(self.web3_client.provider.clone(), wallet);

        futures::executor::block_on(async {
            *self.wallet.write().await = Some(signer);
        });

        Ok(())
    }

    pub async fn submit_proof(&self, _proof_data: ProofData) -> Result<H256> {
        // Simulate error injection
        let error_rate = *self.error_rate.read().await;
        if error_rate > 0.0 && rand::random::<f64>() < error_rate {
            self.metrics.write().await.retry_count += 1;
            return Err(anyhow!("Simulated submission error"));
        }

        // In a real implementation, would submit proof to contract
        self.metrics.write().await.successful_submissions += 1;
        Ok(H256::random())
    }

    pub async fn get_proof_status(&self, job_id: U256) -> Result<ProofStatus> {
        let proof = self.proof_system.get_proof(job_id).call().await?;
        Ok(ProofStatus::from(proof.3))
    }

    pub async fn start_monitoring(&mut self) -> mpsc::Receiver<ProofEvent> {
        let (tx, rx) = mpsc::channel(100);
        *self.event_sender.write().await = Some(tx.clone());

        let submitter = self.clone_for_monitoring();
        tokio::spawn(async move {
            submitter.monitoring_loop().await;
        });

        rx
    }

    pub async fn submit_proof_batch(&self, _proofs: Vec<ProofData>) -> Result<H256> {
        // In a real implementation, would use multicall for batch submission
        Ok(H256::random())
    }

    pub async fn prepare_proof_for_submission(&self, proof: ProofData) -> Result<StoredProof> {
        let size = proof.proof.len();

        let stored_proof = if self.config.use_proof_compression {
            // Compress proof
            StoredProof {
                size: size / 2, // Mock compression
                ipfs_hash: if self.config.store_proofs_on_ipfs {
                    Some("QmXxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".to_string())
                } else {
                    None
                },
                on_chain_data: vec![4u8; 256], // Compressed data
            }
        } else {
            StoredProof {
                size,
                ipfs_hash: None,
                on_chain_data: proof.proof,
            }
        };

        Ok(stored_proof)
    }

    pub async fn check_proof_deadline(&self, _job_id: U256) -> Result<bool> {
        // In a real implementation, would check job timestamp
        Ok(true)
    }

    pub fn inject_error_rate(&mut self, rate: f64) {
        futures::executor::block_on(async {
            *self.error_rate.write().await = rate;
        });
    }

    pub async fn submit_proof_with_retry(&self, proof: ProofData) -> Result<H256> {
        let mut attempts = 0;
        let max_attempts = self.config.max_resubmission_attempts;

        loop {
            match self.submit_proof(proof.clone()).await {
                Ok(tx_hash) => return Ok(tx_hash),
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        return Err(e);
                    }
                    tokio::time::sleep(self.config.resubmission_delay).await;
                }
            }
        }
    }

    pub fn get_metrics(&self) -> ProofMetrics {
        futures::executor::block_on(async { self.metrics.read().await.clone() })
    }

    pub async fn validate_proof(&self, proof: &ProofData) -> Result<bool> {
        // Basic validation
        if proof.proof.is_empty()
            || proof.public_inputs.is_empty()
            || proof.verification_key.is_empty()
        {
            return Ok(false);
        }

        // In a real implementation, would validate proof structure
        Ok(true)
    }

    pub async fn submit_challenge_response(
        &self,
        _challenge_id: U256,
        _response_data: Vec<u8>,
    ) -> Result<H256> {
        // In a real implementation, would submit response to contract
        Ok(H256::random())
    }

    pub async fn calculate_verification_fee(&self, proof_size: usize) -> Result<U256> {
        // Fee scales with proof size
        let base_fee = U256::from(100_000_000_000_000u64); // 0.0001 ETH
        let size_factor = U256::from(proof_size / 1024); // Per KB
        Ok(base_fee + (base_fee * size_factor / U256::from(10)))
    }

    async fn monitoring_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_millis(100));

        loop {
            interval.tick().await;

            // In a real implementation, would monitor for proof events
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    fn clone_for_monitoring(&self) -> Self {
        Self {
            config: self.config.clone(),
            web3_client: self.web3_client.clone(),
            proof_system: self.proof_system.clone(),
            event_sender: self.event_sender.clone(),
            wallet: self.wallet.clone(),
            error_rate: self.error_rate.clone(),
            metrics: self.metrics.clone(),
        }
    }
}

impl Clone for ProofMetrics {
    fn clone(&self) -> Self {
        Self {
            retry_count: self.retry_count,
            successful_submissions: self.successful_submissions,
        }
    }
}
