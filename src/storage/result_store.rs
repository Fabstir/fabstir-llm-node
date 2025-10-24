// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Inference Result Storage Module
//!
//! Provides in-memory storage for inference results associated with jobs.
//! Used to retrieve results for proof verification during settlement validation.

use crate::results::packager::InferenceResult;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Statistics about result storage
#[derive(Debug, Clone, Default)]
pub struct ResultStoreStats {
    pub total_results: usize,
    pub total_tokens: u32,
    pub hits: u64,
    pub misses: u64,
}

/// In-memory storage for inference results
#[derive(Clone)]
pub struct ResultStore {
    results: Arc<RwLock<HashMap<u64, InferenceResult>>>,
    stats: Arc<RwLock<ResultStoreStats>>,
}

impl ResultStore {
    /// Create a new result store
    pub fn new() -> Self {
        Self {
            results: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(ResultStoreStats::default())),
        }
    }

    /// Store a result for a job
    pub async fn store_result(&self, job_id: u64, result: InferenceResult) -> Result<()> {
        debug!(
            "📥 Storing result for job {} ({} tokens)",
            job_id, result.tokens_generated
        );

        let tokens = result.tokens_generated;

        let mut results = self.results.write().await;
        results.insert(job_id, result);

        // Update stats
        let mut stats = self.stats.write().await;
        stats.total_results = results.len();
        stats.total_tokens += tokens;

        info!("✅ Result stored for job {} ({} tokens)", job_id, tokens);
        Ok(())
    }

    /// Retrieve a result for a job
    pub async fn retrieve_result(&self, job_id: u64) -> Result<InferenceResult> {
        debug!("🔍 Retrieving result for job {}", job_id);

        let results = self.results.read().await;

        if let Some(result) = results.get(&job_id) {
            // Update stats - hit
            let mut stats = self.stats.write().await;
            stats.hits += 1;
            drop(stats);

            debug!("✅ Result found for job {}", job_id);
            Ok(result.clone())
        } else {
            // Update stats - miss
            let mut stats = self.stats.write().await;
            stats.misses += 1;
            drop(stats);

            warn!("❌ No result found for job {}", job_id);
            Err(anyhow!("No result found for job {}", job_id))
        }
    }

    /// Check if a result exists for a job
    pub async fn has_result(&self, job_id: u64) -> bool {
        let results = self.results.read().await;
        results.contains_key(&job_id)
    }

    /// Remove a result for a job
    pub async fn remove_result(&self, job_id: u64) -> Result<InferenceResult> {
        debug!("🗑️ Removing result for job {}", job_id);

        let mut results = self.results.write().await;

        if let Some(result) = results.remove(&job_id) {
            let tokens = result.tokens_generated;

            // Update stats
            let mut stats = self.stats.write().await;
            stats.total_results = results.len();
            stats.total_tokens = stats.total_tokens.saturating_sub(tokens);
            drop(stats);

            info!(
                "✅ Result removed for job {} ({} tokens freed)",
                job_id, tokens
            );
            Ok(result)
        } else {
            warn!("⚠️ No result to remove for job {}", job_id);
            Err(anyhow!("No result found for job {}", job_id))
        }
    }

    /// Get the number of stored results
    pub async fn len(&self) -> usize {
        self.results.read().await.len()
    }

    /// Check if store is empty
    pub async fn is_empty(&self) -> bool {
        self.results.read().await.is_empty()
    }

    /// Clear all results
    pub async fn clear(&self) {
        info!("🧹 Clearing all results from store");

        let mut results = self.results.write().await;
        results.clear();

        let mut stats = self.stats.write().await;
        stats.total_results = 0;
        stats.total_tokens = 0;
    }

    /// Get storage statistics
    pub async fn stats(&self) -> ResultStoreStats {
        self.stats.read().await.clone()
    }

    /// Get all job IDs with stored results
    pub async fn list_jobs(&self) -> Vec<u64> {
        self.results.read().await.keys().copied().collect()
    }
}

impl Default for ResultStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::packager::ResultMetadata;
    use chrono::Utc;

    fn create_test_result(job_id: &str, tokens: u32) -> InferenceResult {
        InferenceResult {
            job_id: job_id.to_string(),
            model_id: "test-model".to_string(),
            prompt: "test prompt".to_string(),
            response: "test response".to_string(),
            tokens_generated: tokens,
            inference_time_ms: 100,
            timestamp: Utc::now(),
            node_id: "test-node".to_string(),
            metadata: ResultMetadata::default(),
        }
    }

    #[tokio::test]
    async fn test_store_and_retrieve_result() {
        let store = ResultStore::new();
        let result = create_test_result("123", 50);

        // Store result
        store.store_result(123, result.clone()).await.unwrap();

        // Retrieve result
        let retrieved = store.retrieve_result(123).await.unwrap();
        assert_eq!(retrieved.job_id, result.job_id);
        assert_eq!(retrieved.tokens_generated, 50);
    }

    #[tokio::test]
    async fn test_has_result() {
        let store = ResultStore::new();
        let result = create_test_result("456", 75);

        assert!(!store.has_result(456).await);

        store.store_result(456, result).await.unwrap();
        assert!(store.has_result(456).await);
    }

    #[tokio::test]
    async fn test_remove_result() {
        let store = ResultStore::new();
        let result = create_test_result("789", 100);

        store.store_result(789, result).await.unwrap();
        assert!(store.has_result(789).await);

        let removed = store.remove_result(789).await.unwrap();
        assert_eq!(removed.job_id, "789");
        assert!(!store.has_result(789).await);
    }

    #[tokio::test]
    async fn test_retrieve_nonexistent_result() {
        let store = ResultStore::new();
        let result = store.retrieve_result(999).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stats() {
        let store = ResultStore::new();

        let result1 = create_test_result("100", 50);
        let result2 = create_test_result("200", 75);

        store.store_result(100, result1).await.unwrap();
        store.store_result(200, result2).await.unwrap();

        let stats = store.stats().await;
        assert_eq!(stats.total_results, 2);
        assert_eq!(stats.total_tokens, 125);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);

        // Test hit/miss tracking
        store.retrieve_result(100).await.unwrap();
        let stats = store.stats().await;
        assert_eq!(stats.hits, 1);

        let _ = store.retrieve_result(999).await;
        let stats = store.stats().await;
        assert_eq!(stats.misses, 1);
    }

    #[tokio::test]
    async fn test_len_and_is_empty() {
        let store = ResultStore::new();

        assert_eq!(store.len().await, 0);
        assert!(store.is_empty().await);

        let result = create_test_result("111", 25);
        store.store_result(111, result).await.unwrap();

        assert_eq!(store.len().await, 1);
        assert!(!store.is_empty().await);
    }

    #[tokio::test]
    async fn test_clear() {
        let store = ResultStore::new();

        let result1 = create_test_result("222", 30);
        let result2 = create_test_result("333", 40);

        store.store_result(222, result1).await.unwrap();
        store.store_result(333, result2).await.unwrap();

        assert_eq!(store.len().await, 2);

        store.clear().await;

        assert_eq!(store.len().await, 0);
        assert!(store.is_empty().await);

        let stats = store.stats().await;
        assert_eq!(stats.total_results, 0);
        assert_eq!(stats.total_tokens, 0);
    }

    #[tokio::test]
    async fn test_list_jobs() {
        let store = ResultStore::new();

        let result1 = create_test_result("444", 10);
        let result2 = create_test_result("555", 20);
        let result3 = create_test_result("666", 30);

        store.store_result(444, result1).await.unwrap();
        store.store_result(555, result2).await.unwrap();
        store.store_result(666, result3).await.unwrap();

        let mut jobs = store.list_jobs().await;
        jobs.sort();

        assert_eq!(jobs, vec![444, 555, 666]);
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let store = Arc::new(ResultStore::new());

        let handles: Vec<_> = (0..10u64)
            .map(|i| {
                let store = store.clone();
                tokio::spawn(async move {
                    let result = create_test_result(&i.to_string(), (i * 10) as u32);
                    store.store_result(i, result).await.unwrap();
                })
            })
            .collect();

        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(store.len().await, 10);
    }
}
