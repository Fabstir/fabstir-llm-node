// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use chrono::Utc;
use fabstir_llm_node::storage::{
    CompressionType, ModelFormat, ModelMetadata, ModelStorage, ModelStorageConfig, ModelVersion,
    S5Backend, S5Client, S5Storage, S5StorageConfig, StorageError,
};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_storage() -> Result<ModelStorage, StorageError> {
        let s5_config = match std::env::var("TEST_S5_BACKEND").as_deref() {
            Ok("real") => S5StorageConfig {
                backend: S5Backend::Real {
                    portal_url: std::env::var("S5_PORTAL_URL")
                        .unwrap_or_else(|_| "https://s5.vup.cx".to_string()),
                },
                api_key: std::env::var("S5_API_KEY").ok(),
                cache_ttl_seconds: 300,
                max_retries: 3,
            },
            _ => S5StorageConfig {
                backend: S5Backend::Mock,
                api_key: None,
                cache_ttl_seconds: 300,
                max_retries: 3,
            },
        };

        let s5_client = S5Client::create(s5_config).await?;
        let config = ModelStorageConfig {
            base_path: "home/models".to_string(),
            enable_compression: true,
            chunk_size_mb: 100,
        };

        Ok(ModelStorage::new(s5_client, config))
    }

    fn create_test_metadata() -> ModelMetadata {
        ModelMetadata {
            model_id: "llama-3.2-1b-instruct".to_string(),
            name: "LLaMA 3.2 1B Instruct".to_string(),
            format: ModelFormat::GGUF,
            size_bytes: 1_073_741_824, // 1GB
            parameters: 1_000_000_000,
            quantization: Some("Q4_K_M".to_string()),
            created_at: Utc::now(),
            sha256_hash: "abcdef1234567890".to_string(),
            compression: Some(CompressionType::Zstd),
            tags: vec!["instruct".to_string(), "efficient".to_string()],
            metadata: HashMap::from([
                ("license".to_string(), "apache-2.0".to_string()),
                ("context_length".to_string(), "4096".to_string()),
            ]),
        }
    }

    #[tokio::test]
    async fn test_store_model_basic() {
        let storage = create_test_storage().await.unwrap();

        let metadata = create_test_metadata();
        let model_data = vec![0u8; 1000]; // Small test model

        let version = storage
            .store_model(&metadata.model_id, model_data.clone(), metadata.clone())
            .await
            .unwrap();

        assert!(!version.version_id.is_empty());
        assert!(!version.cid.is_empty());
        assert_eq!(version.model_id, metadata.model_id);
        assert!(version.is_latest);
    }

    #[tokio::test]
    async fn test_retrieve_model() {
        let storage = create_test_storage().await.unwrap();

        let metadata = create_test_metadata();
        let model_data = vec![42u8; 1000];

        // Store model
        let version = storage
            .store_model(&metadata.model_id, model_data.clone(), metadata.clone())
            .await
            .unwrap();

        // Retrieve by model ID (latest version)
        let (retrieved_data, retrieved_metadata) =
            storage.get_model(&metadata.model_id).await.unwrap();

        assert_eq!(retrieved_data, model_data);
        assert_eq!(retrieved_metadata.model_id, metadata.model_id);
        assert_eq!(retrieved_metadata.name, metadata.name);

        // Retrieve by specific version
        let (version_data, version_metadata) = storage
            .get_model_version(&metadata.model_id, &version.version_id)
            .await
            .unwrap();

        assert_eq!(version_data, model_data);
        assert_eq!(version_metadata.model_id, metadata.model_id);
    }

    #[tokio::test]
    async fn test_model_versioning() {
        let storage = create_test_storage().await.unwrap();

        let mut metadata = create_test_metadata();

        // Store v1
        let v1_data = vec![1u8; 1000];
        let v1 = storage
            .store_model(&metadata.model_id, v1_data.clone(), metadata.clone())
            .await
            .unwrap();

        // Store v2
        let v2_data = vec![2u8; 1000];
        metadata
            .metadata
            .insert("version_note".to_string(), "Improved accuracy".to_string());
        let v2 = storage
            .store_model(&metadata.model_id, v2_data.clone(), metadata.clone())
            .await
            .unwrap();

        // Store v3
        let v3_data = vec![3u8; 1000];
        metadata
            .metadata
            .insert("version_note".to_string(), "Bug fixes".to_string());
        let v3 = storage
            .store_model(&metadata.model_id, v3_data.clone(), metadata.clone())
            .await
            .unwrap();

        // List versions
        let versions = storage
            .list_model_versions(&metadata.model_id)
            .await
            .unwrap();
        assert_eq!(versions.len(), 3);

        // Verify latest flag
        assert!(
            !versions
                .iter()
                .find(|v| v.version_id == v1.version_id)
                .unwrap()
                .is_latest
        );
        assert!(
            !versions
                .iter()
                .find(|v| v.version_id == v2.version_id)
                .unwrap()
                .is_latest
        );
        assert!(
            versions
                .iter()
                .find(|v| v.version_id == v3.version_id)
                .unwrap()
                .is_latest
        );

        // Get latest should return v3
        let (latest_data, _) = storage.get_model(&metadata.model_id).await.unwrap();
        assert_eq!(latest_data, v3_data);

        // Get specific versions
        let (v1_retrieved, _) = storage
            .get_model_version(&metadata.model_id, &v1.version_id)
            .await
            .unwrap();
        assert_eq!(v1_retrieved, v1_data);
    }

    #[tokio::test]
    async fn test_list_models() {
        let storage = create_test_storage().await.unwrap();

        // Store multiple models
        let models = vec![
            ("llama-3.2-1b", "LLaMA 3.2 1B", ModelFormat::GGUF),
            ("mistral-7b", "Mistral 7B", ModelFormat::GGUF),
            ("phi-2", "Phi-2", ModelFormat::SafeTensors),
        ];

        for (id, name, format) in models {
            let mut metadata = create_test_metadata();
            metadata.model_id = id.to_string();
            metadata.name = name.to_string();
            metadata.format = format;

            let data = vec![0u8; 100];
            storage.store_model(id, data, metadata).await.unwrap();
        }

        // List all models
        let all_models = storage.list_models().await.unwrap();
        assert_eq!(all_models.len(), 3);

        let model_ids: Vec<String> = all_models.iter().map(|m| m.model_id.clone()).collect();
        assert!(model_ids.contains(&"llama-3.2-1b".to_string()));
        assert!(model_ids.contains(&"mistral-7b".to_string()));
        assert!(model_ids.contains(&"phi-2".to_string()));
    }

    #[tokio::test]
    async fn test_model_compression() {
        let storage = create_test_storage().await.unwrap();

        let metadata = create_test_metadata();

        // Create compressible data (repeated pattern)
        let uncompressed_size = 100_000;
        let model_data: Vec<u8> = (0..uncompressed_size).map(|i| (i % 10) as u8).collect();

        let model_id = metadata.model_id.clone();
        let version = storage
            .store_model(&model_id, model_data.clone(), metadata)
            .await
            .unwrap();

        // Check that compression was applied
        let storage_stats = storage.get_model_stats(&model_id).await.unwrap();
        assert!(storage_stats.compressed_size < uncompressed_size);
        assert!(storage_stats.compression_ratio > 1.0);

        // Retrieve and verify decompression
        let (retrieved_data, _) = storage.get_model(&model_id).await.unwrap();
        assert_eq!(retrieved_data, model_data);
    }

    #[tokio::test]
    async fn test_large_model_chunking() {
        let storage = create_test_storage().await.unwrap();

        let mut metadata = create_test_metadata();
        metadata.model_id = "large-model".to_string();

        // Create 250MB model (will be chunked at 100MB)
        let large_size = 250 * 1024 * 1024;
        let model_data: Vec<u8> = (0..large_size).map(|i| (i % 256) as u8).collect();

        metadata.size_bytes = large_size as u64;

        let version = storage
            .store_model(&metadata.model_id, model_data.clone(), metadata.clone())
            .await
            .unwrap();

        // Verify chunking occurred
        let chunk_info = storage
            .get_chunk_info(&metadata.model_id, &version.version_id)
            .await
            .unwrap();
        assert_eq!(chunk_info.total_chunks, 3); // 250MB / 100MB = 3 chunks
        assert_eq!(chunk_info.chunk_size_mb, 100);

        // Retrieve and verify
        let (retrieved_data, _) = storage.get_model(&metadata.model_id).await.unwrap();
        assert_eq!(retrieved_data.len(), model_data.len());
        assert_eq!(retrieved_data[0..1000], model_data[0..1000]);
        assert_eq!(
            retrieved_data[retrieved_data.len() - 1000..],
            model_data[model_data.len() - 1000..]
        );
    }

    #[tokio::test]
    async fn test_delete_model() {
        let storage = create_test_storage().await.unwrap();

        let metadata = create_test_metadata();
        let model_data = vec![0u8; 1000];

        // Store model
        storage
            .store_model(&metadata.model_id, model_data, metadata.clone())
            .await
            .unwrap();

        // Verify it exists
        assert!(storage.model_exists(&metadata.model_id).await.unwrap());

        // Delete model
        storage.delete_model(&metadata.model_id).await.unwrap();

        // Verify it's gone
        assert!(!storage.model_exists(&metadata.model_id).await.unwrap());

        // Getting deleted model should fail
        let result = storage.get_model(&metadata.model_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_model_metadata_search() {
        let storage = create_test_storage().await.unwrap();

        // Store models with different tags
        let models = vec![
            ("model1", vec!["instruct", "fast"]),
            ("model2", vec!["chat", "fast"]),
            ("model3", vec!["instruct", "accurate"]),
        ];

        for (id, tags) in models {
            let mut metadata = create_test_metadata();
            metadata.model_id = id.to_string();
            metadata.tags = tags.iter().map(|s| s.to_string()).collect();

            storage
                .store_model(id, vec![0u8; 100], metadata)
                .await
                .unwrap();
        }

        // Search by tag
        let fast_models = storage.search_models_by_tag("fast").await.unwrap();
        assert_eq!(fast_models.len(), 2);

        let instruct_models = storage.search_models_by_tag("instruct").await.unwrap();
        assert_eq!(instruct_models.len(), 2);

        // Search by format
        let gguf_models = storage
            .search_models_by_format(ModelFormat::GGUF)
            .await
            .unwrap();
        assert_eq!(gguf_models.len(), 3);
    }

    #[tokio::test]
    async fn test_model_integrity_verification() {
        let storage = create_test_storage().await.unwrap();

        let mut metadata = create_test_metadata();
        let model_data = b"test model data".to_vec();

        // Calculate actual hash
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&model_data);
        metadata.sha256_hash = format!("{:x}", hasher.finalize());

        // Store model
        storage
            .store_model(&metadata.model_id, model_data.clone(), metadata.clone())
            .await
            .unwrap();

        // Verify integrity
        let is_valid = storage
            .verify_model_integrity(&metadata.model_id)
            .await
            .unwrap();
        assert!(is_valid);

        // Test with corrupted data (mock-specific)
        #[cfg(not(feature = "integration"))]
        {
            storage.corrupt_model_data(&metadata.model_id).await;
            let is_valid = storage
                .verify_model_integrity(&metadata.model_id)
                .await
                .unwrap();
            assert!(!is_valid);
        }
    }
}
