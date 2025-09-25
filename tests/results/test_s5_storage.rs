use chrono::Utc;
use fabstir_llm_node::results::{
    InferenceResult, PackagedResult, ResultMetadata, S5StorageClient, S5StorageConfig,
    StorageMetadata, StorageResult,
};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> S5StorageConfig {
        S5StorageConfig {
            portal_url: "https://s5.example.com".to_string(),
            api_key: Some("test_api_key".to_string()),
            base_path: "/fabstir-llm".to_string(),
        }
    }

    fn create_test_packaged_result() -> PackagedResult {
        let result = InferenceResult {
            job_id: "job_12345".to_string(),
            model_id: "llama2-7b".to_string(),
            prompt: "What is Rust?".to_string(),
            response: "Rust is a systems programming language.".to_string(),
            tokens_generated: 8,
            inference_time_ms: 750,
            timestamp: Utc::now(),
            node_id: "node_abc123".to_string(),
            metadata: ResultMetadata::default(),
        };

        PackagedResult {
            result,
            signature: vec![1, 2, 3, 4, 5],
            encoding: "cbor".to_string(),
            version: "1.0".to_string(),
            job_request: None,
        }
    }

    #[tokio::test]
    async fn test_store_result_returns_cid() {
        let config = create_test_config();
        let client = S5StorageClient::new(config);
        let packaged_result = create_test_packaged_result();

        let storage_result = client.store_result(&packaged_result).await.unwrap();

        // Should return a valid CID
        assert!(!storage_result.cid.is_empty());
        assert!(storage_result.cid.starts_with("b") || storage_result.cid.starts_with("z"));

        // Should store at correct path
        assert_eq!(
            storage_result.path,
            "/fabstir-llm/results/job_12345/result.cbor"
        );

        // Metadata should be correct
        assert_eq!(storage_result.metadata.job_id, "job_12345");
        assert_eq!(storage_result.metadata.node_id, "node_abc123");
        assert_eq!(storage_result.metadata.content_type, "application/cbor");
    }

    #[tokio::test]
    async fn test_retrieve_result_by_cid() {
        let config = create_test_config();
        let client = S5StorageClient::new(config);
        let original_result = create_test_packaged_result();

        // Store first
        let storage_result = client.store_result(&original_result).await.unwrap();

        // Retrieve by CID
        let retrieved_result = client.retrieve_result(&storage_result.cid).await.unwrap();

        // Should match original
        assert_eq!(
            retrieved_result.result.job_id,
            original_result.result.job_id
        );
        assert_eq!(
            retrieved_result.result.response,
            original_result.result.response
        );
        assert_eq!(retrieved_result.signature, original_result.signature);
    }

    #[tokio::test]
    async fn test_retrieve_result_by_path() {
        let config = create_test_config();
        let client = S5StorageClient::new(config);
        let original_result = create_test_packaged_result();

        // Store first
        client.store_result(&original_result).await.unwrap();

        // Retrieve by job ID (path-based)
        let retrieved_result = client.retrieve_by_path("job_12345").await.unwrap();

        assert_eq!(retrieved_result.result.job_id, "job_12345");
        assert_eq!(
            retrieved_result.result.response,
            original_result.result.response
        );
    }

    #[tokio::test]
    async fn test_cbor_encoding_deterministic() {
        let config = create_test_config();
        let client = S5StorageClient::new(config);
        let result = create_test_packaged_result();

        // Store multiple times
        let storage_result1 = client.store_result(&result).await.unwrap();
        let storage_result2 = client.store_result(&result).await.unwrap();

        // Should produce same CID (deterministic)
        assert_eq!(storage_result1.cid, storage_result2.cid);
    }

    #[tokio::test]
    async fn test_store_large_result() {
        let config = create_test_config();
        let client = S5StorageClient::new(config);
        let mut packaged_result = create_test_packaged_result();

        // Create large response (10MB)
        packaged_result.result.response = "x".repeat(10 * 1024 * 1024);

        let storage_result = client.store_result(&packaged_result).await.unwrap();

        assert!(!storage_result.cid.is_empty());
        assert!(storage_result.metadata.size_bytes >= 10 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_store_with_custom_metadata() {
        let config = create_test_config();
        let client = S5StorageClient::new(config);
        let packaged_result = create_test_packaged_result();

        let mut metadata = HashMap::new();
        metadata.insert("model_version".to_string(), "2.0".to_string());
        metadata.insert("gpu_type".to_string(), "RTX_4090".to_string());
        metadata.insert("region".to_string(), "us-east-1".to_string());

        let storage_result = client
            .store_with_metadata(&packaged_result, metadata)
            .await
            .unwrap();

        assert!(!storage_result.cid.is_empty());
        // Metadata should be stored (implementation specific)
    }

    #[tokio::test]
    async fn test_list_results_by_prefix() {
        let config = create_test_config();
        let client = S5StorageClient::new(config);

        // Store multiple results
        for i in 0..3 {
            let mut result = create_test_packaged_result();
            result.result.job_id = format!("job_{}", i);
            client.store_result(&result).await.unwrap();
        }

        // List all results
        let results = client.list_results("").await.unwrap();

        assert!(results.len() >= 3);
        for meta in results {
            assert!(meta.job_id.starts_with("job_"));
        }
    }

    #[tokio::test]
    async fn test_path_structure() {
        let config = create_test_config();
        let client = S5StorageClient::new(config);
        let packaged_result = create_test_packaged_result();

        let storage_result = client.store_result(&packaged_result).await.unwrap();

        // Verify path structure follows pattern
        assert!(storage_result.path.starts_with("/fabstir-llm/results/"));
        assert!(storage_result.path.contains(&packaged_result.result.job_id));
        assert!(storage_result.path.ends_with(".cbor"));
    }

    #[tokio::test]
    async fn test_error_on_invalid_cid() {
        let config = create_test_config();
        let client = S5StorageClient::new(config);

        let result = client.retrieve_result("invalid_cid_123").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_concurrent_storage_operations() {
        let config = create_test_config();
        let client = S5StorageClient::new(config);

        // Store multiple results concurrently
        let mut handles = vec![];
        for i in 0..5 {
            let client = client.clone();
            let mut result = create_test_packaged_result();
            result.result.job_id = format!("job_concurrent_{}", i);

            let handle = tokio::spawn(async move { client.store_result(&result).await });
            handles.push(handle);
        }

        // All should succeed
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }
    }
}
