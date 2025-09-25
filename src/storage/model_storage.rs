use crate::storage::{CborCompat, CompressionType, S5Storage, StorageError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;
use zstd;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModelFormat {
    GGUF,
    SafeTensors,
    PyTorch,
    ONNX,
    TensorFlowLite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub model_id: String,
    pub name: String,
    pub format: ModelFormat,
    pub size_bytes: u64,
    pub parameters: u64,
    pub quantization: Option<String>,
    pub created_at: DateTime<Utc>,
    pub sha256_hash: String,
    pub compression: Option<CompressionType>,
    pub tags: Vec<String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelVersion {
    pub version_id: String,
    pub model_id: String,
    pub cid: String,
    pub metadata: ModelMetadata,
    pub is_latest: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ModelStorageConfig {
    pub base_path: String,
    pub enable_compression: bool,
    pub chunk_size_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    pub total_chunks: u32,
    pub chunk_size_mb: u64,
    pub chunk_cids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStats {
    pub total_size: u64,
    pub compressed_size: u64,
    pub compression_ratio: f64,
    pub chunk_count: u32,
}

pub struct ModelStorage {
    storage: Box<dyn S5Storage>,
    config: ModelStorageConfig,
    cbor: CborCompat,
}

impl Clone for ModelStorage {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            config: self.config.clone(),
            cbor: self.cbor.clone(),
        }
    }
}

impl ModelStorage {
    pub fn new(storage: Box<dyn S5Storage>, config: ModelStorageConfig) -> Self {
        Self {
            storage,
            config,
            cbor: CborCompat::new(),
        }
    }

    pub async fn store_model(
        &self,
        model_id: &str,
        data: Vec<u8>,
        metadata: ModelMetadata,
    ) -> Result<ModelVersion, StorageError> {
        // Generate version ID
        let version_id = Uuid::new_v4().to_string();
        let version_path = format!(
            "{}/{}/versions/{}",
            self.config.base_path, model_id, version_id
        );

        // Calculate actual hash if not provided
        let actual_hash = self.calculate_hash(&data);
        let mut final_metadata = metadata.clone();
        if final_metadata.sha256_hash.is_empty() {
            final_metadata.sha256_hash = actual_hash.clone();
        }

        // Store model data (with chunking if necessary)
        let model_cid = if data.len() > (self.config.chunk_size_mb * 1024 * 1024) as usize {
            self.store_chunked_model(&version_path, &data).await?
        } else {
            let processed_data = if self.config.enable_compression {
                self.compress_data(&data)?
            } else {
                data
            };

            self.storage
                .put(&format!("{}/data", version_path), processed_data)
                .await?
        };

        // Store metadata
        let metadata_path = format!("{}/metadata.cbor", version_path);
        let metadata_data = self
            .cbor
            .encode(&final_metadata)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;
        self.storage.put(&metadata_path, metadata_data).await?;

        // Create version record
        let version = ModelVersion {
            version_id: version_id.clone(),
            model_id: model_id.to_string(),
            cid: model_cid,
            metadata: final_metadata,
            is_latest: true,
            created_at: Utc::now(),
        };

        // Store version info
        let version_info_path = format!("{}/version.cbor", version_path);
        let version_data = self
            .cbor
            .encode(&version)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;
        self.storage.put(&version_info_path, version_data).await?;

        // Update latest pointer and mark other versions as not latest
        self.update_latest_version(model_id, &version_id).await?;

        Ok(version)
    }

    pub async fn get_model(
        &self,
        model_id: &str,
    ) -> Result<(Vec<u8>, ModelMetadata), StorageError> {
        let latest_version = self.get_latest_version(model_id).await?;
        self.get_model_version(model_id, &latest_version.version_id)
            .await
    }

    pub async fn get_model_version(
        &self,
        model_id: &str,
        version_id: &str,
    ) -> Result<(Vec<u8>, ModelMetadata), StorageError> {
        let version_path = format!(
            "{}/{}/versions/{}",
            self.config.base_path, model_id, version_id
        );

        // Load metadata
        let metadata_path = format!("{}/metadata.cbor", version_path);
        let metadata_data = self.storage.get(&metadata_path).await?;
        let metadata: ModelMetadata = self
            .cbor
            .decode(&metadata_data)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        // Load model data
        let data = if self.is_chunked(model_id, version_id).await? {
            self.load_chunked_model(&version_path).await?
        } else {
            let data_path = format!("{}/data", version_path);
            let mut raw_data = self.storage.get(&data_path).await?;

            if self.config.enable_compression {
                raw_data = self.decompress_data(&raw_data)?;
            }

            raw_data
        };

        Ok((data, metadata))
    }

    pub async fn list_models(&self) -> Result<Vec<ModelMetadata>, StorageError> {
        let entries = self.storage.list(&self.config.base_path).await?;
        let mut models = Vec::new();

        for entry in entries {
            if entry.entry_type == crate::storage::S5EntryType::Directory {
                if let Ok(latest_version) = self.get_latest_version(&entry.name).await {
                    models.push(latest_version.metadata);
                }
            }
        }

        Ok(models)
    }

    pub async fn list_model_versions(
        &self,
        model_id: &str,
    ) -> Result<Vec<ModelVersion>, StorageError> {
        let versions_path = format!("{}/{}/versions", self.config.base_path, model_id);
        let entries = self.storage.list(&versions_path).await?;
        let mut versions = Vec::new();

        for entry in entries {
            if entry.entry_type == crate::storage::S5EntryType::Directory {
                let version_info_path = format!("{}/{}/version.cbor", versions_path, entry.name);
                if let Ok(version_data) = self.storage.get(&version_info_path).await {
                    if let Ok(version) = self.cbor.decode::<ModelVersion>(&version_data) {
                        versions.push(version);
                    }
                }
            }
        }

        // Sort by creation time (newest first)
        versions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(versions)
    }

    pub async fn delete_model(&self, model_id: &str) -> Result<(), StorageError> {
        let model_path = format!("{}/{}", self.config.base_path, model_id);

        // List all versions and delete each one
        let versions = self.list_model_versions(model_id).await?;
        for version in versions {
            let version_path = format!("{}/versions/{}", model_path, version.version_id);
            self.delete_version_data(&version_path).await?;
        }

        // Delete the entire model directory structure
        // Note: This would require recursive delete in a real implementation
        // For now, we'll delete key files
        self.storage.delete(&model_path).await?;
        Ok(())
    }

    pub async fn model_exists(&self, model_id: &str) -> Result<bool, StorageError> {
        let model_path = format!("{}/{}", self.config.base_path, model_id);
        self.storage.exists(&model_path).await
    }

    pub async fn search_models_by_tag(
        &self,
        tag: &str,
    ) -> Result<Vec<ModelMetadata>, StorageError> {
        let all_models = self.list_models().await?;
        let filtered: Vec<ModelMetadata> = all_models
            .into_iter()
            .filter(|model| model.tags.contains(&tag.to_string()))
            .collect();
        Ok(filtered)
    }

    pub async fn search_models_by_format(
        &self,
        format: ModelFormat,
    ) -> Result<Vec<ModelMetadata>, StorageError> {
        let all_models = self.list_models().await?;
        let filtered: Vec<ModelMetadata> = all_models
            .into_iter()
            .filter(|model| model.format == format)
            .collect();
        Ok(filtered)
    }

    pub async fn get_model_stats(&self, model_id: &str) -> Result<ModelStats, StorageError> {
        let latest_version = self.get_latest_version(model_id).await?;
        let version_path = format!(
            "{}/{}/versions/{}",
            self.config.base_path, model_id, latest_version.version_id
        );

        let total_size = latest_version.metadata.size_bytes;
        let mut compressed_size = total_size;

        if self
            .is_chunked(model_id, &latest_version.version_id)
            .await?
        {
            let chunk_info = self
                .get_chunk_info(model_id, &latest_version.version_id)
                .await?;
            // Estimate compressed size from actual storage
            compressed_size = self
                .calculate_compressed_size_chunked(&version_path, &chunk_info)
                .await?;
        } else if self.config.enable_compression {
            let data_path = format!("{}/data", version_path);
            let stored_data = self.storage.get(&data_path).await?;
            compressed_size = stored_data.len() as u64;
        }

        let compression_ratio = if compressed_size > 0 {
            total_size as f64 / compressed_size as f64
        } else {
            1.0
        };

        let chunk_count = if self
            .is_chunked(model_id, &latest_version.version_id)
            .await?
        {
            let chunk_info = self
                .get_chunk_info(model_id, &latest_version.version_id)
                .await?;
            chunk_info.total_chunks
        } else {
            1
        };

        Ok(ModelStats {
            total_size,
            compressed_size,
            compression_ratio,
            chunk_count,
        })
    }

    pub async fn verify_model_integrity(&self, model_id: &str) -> Result<bool, StorageError> {
        let (data, metadata) = self.get_model(model_id).await?;
        let calculated_hash = self.calculate_hash(&data);
        Ok(calculated_hash == metadata.sha256_hash)
    }

    pub async fn get_chunk_info(
        &self,
        model_id: &str,
        version_id: &str,
    ) -> Result<ChunkInfo, StorageError> {
        let chunk_info_path = format!(
            "{}/{}/versions/{}/chunks.cbor",
            self.config.base_path, model_id, version_id
        );
        let chunk_data = self.storage.get(&chunk_info_path).await?;
        let chunk_info: ChunkInfo = self
            .cbor
            .decode(&chunk_data)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;
        Ok(chunk_info)
    }

    // Mock-specific method for testing
    pub async fn corrupt_model_data(&self, model_id: &str) {
        // This would only work with mock backend
        // In a real implementation, this would be a no-op
    }

    // Private helper methods

    async fn store_chunked_model(
        &self,
        version_path: &str,
        data: &[u8],
    ) -> Result<String, StorageError> {
        let chunk_size = (self.config.chunk_size_mb * 1024 * 1024) as usize;
        let total_chunks = (data.len() + chunk_size - 1) / chunk_size;
        let mut chunk_cids = Vec::new();

        for (i, chunk) in data.chunks(chunk_size).enumerate() {
            let chunk_data = if self.config.enable_compression {
                self.compress_data(chunk)?
            } else {
                chunk.to_vec()
            };

            let chunk_path = format!("{}/chunk_{:04}", version_path, i);
            let chunk_cid = self.storage.put(&chunk_path, chunk_data).await?;
            chunk_cids.push(chunk_cid);
        }

        let chunk_info = ChunkInfo {
            total_chunks: total_chunks as u32,
            chunk_size_mb: self.config.chunk_size_mb,
            chunk_cids: chunk_cids.clone(),
        };

        // Store chunk info
        let chunk_info_path = format!("{}/chunks.cbor", version_path);
        let chunk_info_data = self
            .cbor
            .encode(&chunk_info)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;
        self.storage.put(&chunk_info_path, chunk_info_data).await?;

        // Return a combined CID representing the whole model
        Ok(format!("chunked-{}", chunk_cids.join("-")))
    }

    async fn load_chunked_model(&self, version_path: &str) -> Result<Vec<u8>, StorageError> {
        let chunk_info_path = format!("{}/chunks.cbor", version_path);
        let chunk_info_data = self.storage.get(&chunk_info_path).await?;
        let chunk_info: ChunkInfo = self
            .cbor
            .decode(&chunk_info_data)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        let mut combined_data = Vec::new();

        for i in 0..chunk_info.total_chunks {
            let chunk_path = format!("{}/chunk_{:04}", version_path, i);
            let mut chunk_data = self.storage.get(&chunk_path).await?;

            if self.config.enable_compression {
                chunk_data = self.decompress_data(&chunk_data)?;
            }

            combined_data.extend(chunk_data);
        }

        Ok(combined_data)
    }

    async fn is_chunked(&self, model_id: &str, version_id: &str) -> Result<bool, StorageError> {
        let chunk_info_path = format!(
            "{}/{}/versions/{}/chunks.cbor",
            self.config.base_path, model_id, version_id
        );
        self.storage.exists(&chunk_info_path).await
    }

    async fn get_latest_version(&self, model_id: &str) -> Result<ModelVersion, StorageError> {
        let latest_path = format!("{}/{}/latest.cbor", self.config.base_path, model_id);
        let latest_data = self.storage.get(&latest_path).await?;
        let latest_version: ModelVersion = self
            .cbor
            .decode(&latest_data)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;
        Ok(latest_version)
    }

    async fn update_latest_version(
        &self,
        model_id: &str,
        version_id: &str,
    ) -> Result<(), StorageError> {
        // Load the new version
        let version_path = format!(
            "{}/{}/versions/{}/version.cbor",
            self.config.base_path, model_id, version_id
        );
        let version_data = self.storage.get(&version_path).await?;
        let mut new_version: ModelVersion = self
            .cbor
            .decode(&version_data)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        // Mark all other versions as not latest
        if let Ok(versions) = self.list_model_versions(model_id).await {
            for mut version in versions {
                if version.version_id != version_id {
                    version.is_latest = false;
                    let version_update_path = format!(
                        "{}/{}/versions/{}/version.cbor",
                        self.config.base_path, model_id, version.version_id
                    );
                    let version_update_data = self
                        .cbor
                        .encode(&version)
                        .map_err(|e| StorageError::SerializationError(e.to_string()))?;
                    self.storage
                        .put(&version_update_path, version_update_data)
                        .await?;
                }
            }
        }

        // Mark new version as latest
        new_version.is_latest = true;
        let updated_version_data = self
            .cbor
            .encode(&new_version)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;
        self.storage
            .put(&version_path, updated_version_data)
            .await?;

        // Update latest pointer
        let latest_path = format!("{}/{}/latest.cbor", self.config.base_path, model_id);
        self.storage
            .put(
                &latest_path,
                self.cbor
                    .encode(&new_version)
                    .map_err(|e| StorageError::SerializationError(e.to_string()))?,
            )
            .await?;

        Ok(())
    }

    async fn delete_version_data(&self, version_path: &str) -> Result<(), StorageError> {
        // Delete all files in the version directory
        let paths_to_delete = vec![
            format!("{}/data", version_path),
            format!("{}/metadata.cbor", version_path),
            format!("{}/version.cbor", version_path),
            format!("{}/chunks.cbor", version_path),
        ];

        for path in paths_to_delete {
            let _ = self.storage.delete(&path).await; // Ignore errors for files that don't exist
        }

        Ok(())
    }

    async fn calculate_compressed_size_chunked(
        &self,
        version_path: &str,
        chunk_info: &ChunkInfo,
    ) -> Result<u64, StorageError> {
        let mut total_size = 0u64;

        for i in 0..chunk_info.total_chunks {
            let chunk_path = format!("{}/chunk_{:04}", version_path, i);
            if let Ok(chunk_data) = self.storage.get(&chunk_path).await {
                total_size += chunk_data.len() as u64;
            }
        }

        Ok(total_size)
    }

    fn compress_data(&self, data: &[u8]) -> Result<Vec<u8>, StorageError> {
        zstd::stream::encode_all(data, 3)
            .map_err(|e| StorageError::CompressionError(format!("Compression failed: {}", e)))
    }

    fn decompress_data(&self, compressed: &[u8]) -> Result<Vec<u8>, StorageError> {
        zstd::stream::decode_all(compressed)
            .map_err(|e| StorageError::CompressionError(format!("Decompression failed: {}", e)))
    }

    fn calculate_hash(&self, data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }
}
