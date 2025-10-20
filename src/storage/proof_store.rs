// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Proof Storage Module
//!
//! Provides in-memory storage for EZKL proofs associated with jobs.
//! Used to store proofs after checkpoint submission and retrieve them
//! for validation before settlement.

use crate::results::proofs::InferenceProof;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Statistics about proof storage
#[derive(Debug, Clone, Default)]
pub struct ProofStoreStats {
    pub total_proofs: usize,
    pub total_size_bytes: usize,
    pub hits: u64,
    pub misses: u64,
}

/// In-memory storage for inference proofs
#[derive(Clone)]
pub struct ProofStore {
    proofs: Arc<RwLock<HashMap<u64, InferenceProof>>>,
    stats: Arc<RwLock<ProofStoreStats>>,
}

impl ProofStore {
    /// Create a new proof store
    pub fn new() -> Self {
        Self {
            proofs: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(ProofStoreStats::default())),
        }
    }

    /// Store a proof for a job
    pub async fn store_proof(&self, job_id: u64, proof: InferenceProof) -> Result<()> {
        let proof_size = proof.proof_data.len();

        debug!("📥 Storing proof for job {} ({} bytes)", job_id, proof_size);

        let mut proofs = self.proofs.write().await;
        proofs.insert(job_id, proof);

        // Update stats
        let mut stats = self.stats.write().await;
        stats.total_proofs = proofs.len();
        stats.total_size_bytes += proof_size;

        info!("✅ Proof stored for job {} ({} bytes)", job_id, proof_size);
        Ok(())
    }

    /// Retrieve a proof for a job
    pub async fn retrieve_proof(&self, job_id: u64) -> Result<InferenceProof> {
        debug!("🔍 Retrieving proof for job {}", job_id);

        let proofs = self.proofs.read().await;

        if let Some(proof) = proofs.get(&job_id) {
            // Update stats - hit
            let mut stats = self.stats.write().await;
            stats.hits += 1;
            drop(stats);

            debug!("✅ Proof found for job {}", job_id);
            Ok(proof.clone())
        } else {
            // Update stats - miss
            let mut stats = self.stats.write().await;
            stats.misses += 1;
            drop(stats);

            warn!("❌ No proof found for job {}", job_id);
            Err(anyhow!("No proof found for job {}", job_id))
        }
    }

    /// Check if a proof exists for a job
    pub async fn has_proof(&self, job_id: u64) -> bool {
        let proofs = self.proofs.read().await;
        proofs.contains_key(&job_id)
    }

    /// Remove a proof for a job
    pub async fn remove_proof(&self, job_id: u64) -> Result<InferenceProof> {
        debug!("🗑️ Removing proof for job {}", job_id);

        let mut proofs = self.proofs.write().await;

        if let Some(proof) = proofs.remove(&job_id) {
            let proof_size = proof.proof_data.len();

            // Update stats
            let mut stats = self.stats.write().await;
            stats.total_proofs = proofs.len();
            stats.total_size_bytes = stats.total_size_bytes.saturating_sub(proof_size);
            drop(stats);

            info!("✅ Proof removed for job {} ({} bytes freed)", job_id, proof_size);
            Ok(proof)
        } else {
            warn!("⚠️ No proof to remove for job {}", job_id);
            Err(anyhow!("No proof found for job {}", job_id))
        }
    }

    /// Get the number of stored proofs
    pub async fn len(&self) -> usize {
        self.proofs.read().await.len()
    }

    /// Check if store is empty
    pub async fn is_empty(&self) -> bool {
        self.proofs.read().await.is_empty()
    }

    /// Clear all proofs
    pub async fn clear(&self) {
        info!("🧹 Clearing all proofs from store");

        let mut proofs = self.proofs.write().await;
        proofs.clear();

        let mut stats = self.stats.write().await;
        stats.total_proofs = 0;
        stats.total_size_bytes = 0;
    }

    /// Get storage statistics
    pub async fn stats(&self) -> ProofStoreStats {
        self.stats.read().await.clone()
    }

    /// Get all job IDs with stored proofs
    pub async fn list_jobs(&self) -> Vec<u64> {
        self.proofs.read().await.keys().copied().collect()
    }
}

impl Default for ProofStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use crate::results::proofs::ProofType;

    fn create_test_proof(job_id: &str, proof_size: usize) -> InferenceProof {
        InferenceProof {
            job_id: job_id.to_string(),
            model_hash: "test_model_hash".to_string(),
            input_hash: "test_input_hash".to_string(),
            output_hash: "test_output_hash".to_string(),
            proof_data: vec![0xEF; proof_size],
            proof_type: ProofType::EZKL,
            timestamp: Utc::now(),
            prover_id: "test_prover".to_string(),
        }
    }

    #[tokio::test]
    async fn test_store_and_retrieve_proof() {
        let store = ProofStore::new();
        let proof = create_test_proof("123", 200);

        // Store proof
        store.store_proof(123, proof.clone()).await.unwrap();

        // Retrieve proof
        let retrieved = store.retrieve_proof(123).await.unwrap();
        assert_eq!(retrieved.job_id, proof.job_id);
        assert_eq!(retrieved.proof_data.len(), 200);
    }

    #[tokio::test]
    async fn test_has_proof() {
        let store = ProofStore::new();
        let proof = create_test_proof("456", 150);

        assert!(!store.has_proof(456).await);

        store.store_proof(456, proof).await.unwrap();
        assert!(store.has_proof(456).await);
    }

    #[tokio::test]
    async fn test_remove_proof() {
        let store = ProofStore::new();
        let proof = create_test_proof("789", 300);

        store.store_proof(789, proof).await.unwrap();
        assert!(store.has_proof(789).await);

        let removed = store.remove_proof(789).await.unwrap();
        assert_eq!(removed.job_id, "789");
        assert!(!store.has_proof(789).await);
    }

    #[tokio::test]
    async fn test_retrieve_nonexistent_proof() {
        let store = ProofStore::new();
        let result = store.retrieve_proof(999).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stats() {
        let store = ProofStore::new();

        let proof1 = create_test_proof("100", 200);
        let proof2 = create_test_proof("200", 300);

        store.store_proof(100, proof1).await.unwrap();
        store.store_proof(200, proof2).await.unwrap();

        let stats = store.stats().await;
        assert_eq!(stats.total_proofs, 2);
        assert_eq!(stats.total_size_bytes, 500);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);

        // Test hit/miss tracking
        store.retrieve_proof(100).await.unwrap();
        let stats = store.stats().await;
        assert_eq!(stats.hits, 1);

        let _ = store.retrieve_proof(999).await;
        let stats = store.stats().await;
        assert_eq!(stats.misses, 1);
    }

    #[tokio::test]
    async fn test_len_and_is_empty() {
        let store = ProofStore::new();

        assert_eq!(store.len().await, 0);
        assert!(store.is_empty().await);

        let proof = create_test_proof("111", 100);
        store.store_proof(111, proof).await.unwrap();

        assert_eq!(store.len().await, 1);
        assert!(!store.is_empty().await);
    }

    #[tokio::test]
    async fn test_clear() {
        let store = ProofStore::new();

        let proof1 = create_test_proof("222", 200);
        let proof2 = create_test_proof("333", 300);

        store.store_proof(222, proof1).await.unwrap();
        store.store_proof(333, proof2).await.unwrap();

        assert_eq!(store.len().await, 2);

        store.clear().await;

        assert_eq!(store.len().await, 0);
        assert!(store.is_empty().await);

        let stats = store.stats().await;
        assert_eq!(stats.total_proofs, 0);
        assert_eq!(stats.total_size_bytes, 0);
    }

    #[tokio::test]
    async fn test_list_jobs() {
        let store = ProofStore::new();

        let proof1 = create_test_proof("444", 100);
        let proof2 = create_test_proof("555", 100);
        let proof3 = create_test_proof("666", 100);

        store.store_proof(444, proof1).await.unwrap();
        store.store_proof(555, proof2).await.unwrap();
        store.store_proof(666, proof3).await.unwrap();

        let mut jobs = store.list_jobs().await;
        jobs.sort();

        assert_eq!(jobs, vec![444, 555, 666]);
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let store = Arc::new(ProofStore::new());

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let store = store.clone();
                tokio::spawn(async move {
                    let proof = create_test_proof(&i.to_string(), 100);
                    store.store_proof(i, proof).await.unwrap();
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(store.len().await, 10);
    }
}
