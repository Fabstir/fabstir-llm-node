use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};
use cid::Cid;
use sha2::{Sha256, Digest};
use super::packager::PackagedResult;

#[derive(Debug, Clone)]
pub struct S5StorageConfig {
    pub portal_url: String,
    pub api_key: Option<String>,
    pub base_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMetadata {
    pub cid: String,
    pub size_bytes: usize,
    pub content_type: String,
    pub timestamp: DateTime<Utc>,
    pub node_id: String,
    pub job_id: String,
}

#[derive(Debug)]
pub struct StorageResult {
    pub cid: String,
    pub path: String,
    pub metadata: StorageMetadata,
}

#[derive(Clone)]
pub struct S5StorageClient {
    config: S5StorageConfig,
    cbor_encoder: CborEncoder,
    // In-memory storage for testing
    storage: std::sync::Arc<tokio::sync::Mutex<HashMap<String, Vec<u8>>>>,
    metadata_store: std::sync::Arc<tokio::sync::Mutex<HashMap<String, StorageMetadata>>>,
}

#[derive(Clone)]
pub struct CborEncoder;

impl CborEncoder {
    pub fn encode_deterministic<T: Serialize>(&self, value: &T) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        ciborium::into_writer(value, &mut buffer)
            .context("Failed to encode to CBOR")?;
        Ok(buffer)
    }
    
    pub fn decode<T: for<'de> Deserialize<'de>>(&self, data: &[u8]) -> Result<T> {
        ciborium::from_reader(data)
            .context("Failed to decode CBOR")
    }
}

impl S5StorageClient {
    pub fn new(config: S5StorageConfig) -> Self {
        Self {
            config,
            cbor_encoder: CborEncoder,
            storage: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            metadata_store: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }
    
    pub async fn store_result(&self, result: &PackagedResult) -> Result<StorageResult> {
        // Encode result as CBOR
        let cbor_data = self.cbor_encoder.encode_deterministic(result)?;
        let size_bytes = cbor_data.len();
        
        // Calculate CID
        let mut hasher = Sha256::new();
        hasher.update(&cbor_data);
        let hash = hasher.finalize();
        
        // Create multihash from the hash
        let mh = multihash::Multihash::wrap(0x12, &hash)
            .context("Failed to create multihash")?;
        
        // Create a CID (using SHA-256 and CBOR codec)
        let cid = Cid::new_v1(0x71, mh);
        let cid_str = cid.to_string();
        
        // Construct path
        let path = format!("{}/results/{}/result.cbor", self.config.base_path, result.result.job_id);
        
        // Store data (in-memory for testing)
        let mut storage = self.storage.lock().await;
        storage.insert(cid_str.clone(), cbor_data.clone());
        storage.insert(path.clone(), cbor_data);
        
        // Create metadata
        let metadata = StorageMetadata {
            cid: cid_str.clone(),
            size_bytes,
            content_type: "application/cbor".to_string(),
            timestamp: Utc::now(),
            node_id: result.result.node_id.clone(),
            job_id: result.result.job_id.clone(),
        };
        
        // Store metadata
        let mut metadata_store = self.metadata_store.lock().await;
        metadata_store.insert(cid_str.clone(), metadata.clone());
        metadata_store.insert(result.result.job_id.clone(), metadata.clone());
        
        Ok(StorageResult {
            cid: cid_str,
            path,
            metadata,
        })
    }
    
    pub async fn retrieve_result(&self, cid: &str) -> Result<PackagedResult> {
        let storage = self.storage.lock().await;
        let data = storage.get(cid)
            .ok_or_else(|| anyhow::anyhow!("CID not found"))?;
        
        self.cbor_encoder.decode(data)
    }
    
    pub async fn retrieve_by_path(&self, job_id: &str) -> Result<PackagedResult> {
        let path = format!("{}/results/{}/result.cbor", self.config.base_path, job_id);
        
        let storage = self.storage.lock().await;
        let data = storage.get(&path)
            .ok_or_else(|| anyhow::anyhow!("Result not found for job_id: {}", job_id))?;
        
        self.cbor_encoder.decode(data)
    }
    
    pub async fn store_with_metadata(
        &self,
        result: &PackagedResult,
        _metadata: HashMap<String, String>,
    ) -> Result<StorageResult> {
        // Store the result normally
        let storage_result = self.store_result(result).await?;
        
        // In a real implementation, we'd store the additional metadata
        // For now, just return the result
        Ok(storage_result)
    }
    
    pub async fn list_results(&self, prefix: &str) -> Result<Vec<StorageMetadata>> {
        let metadata_store = self.metadata_store.lock().await;
        let results: Vec<StorageMetadata> = metadata_store
            .values()
            .filter(|meta| prefix.is_empty() || meta.job_id.starts_with(prefix))
            .cloned()
            .collect();
        
        Ok(results)
    }
    
    pub async fn delete_result(&self, job_id: &str) -> Result<()> {
        let path = format!("{}/results/{}/result.cbor", self.config.base_path, job_id);
        
        let mut storage = self.storage.lock().await;
        storage.remove(&path);
        
        let mut metadata_store = self.metadata_store.lock().await;
        metadata_store.remove(job_id);
        
        Ok(())
    }
}