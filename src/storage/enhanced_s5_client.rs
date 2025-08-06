// src/storage/enhanced_s5_client.rs
// Phase 4.1.1: Enhanced S5.js with Internal Mock - HTTP Client Implementation

use anyhow::{Result, anyhow};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tracing::{info, warn, error};
use sha2::{Sha256, Digest};

#[derive(Debug, Clone)]
pub struct S5Config {
    pub api_url: String,
    pub api_key: Option<String>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S5File {
    pub name: String,
    pub size: u64,
    #[serde(rename = "type")]
    pub file_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(rename = "mockStorage")] 
    pub mock_storage: bool,
    pub server: String,
    pub version: String,
}

#[derive(Clone)]
pub struct EnhancedS5Client {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    // Mock storage for testing
    mock_storage: std::sync::Arc<Mutex<HashMap<String, (Vec<u8>, Option<JsonValue>)>>>,
}

impl EnhancedS5Client {
    pub fn new(config: S5Config) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;
        
        Ok(Self {
            client,
            base_url: config.api_url,
            api_key: config.api_key,
            mock_storage: std::sync::Arc::new(Mutex::new(HashMap::new())),
        })
    }
    
    // Legacy constructor for backward compatibility
    pub fn new_legacy(base_url: String) -> Result<Self> {
        Self::new(S5Config {
            api_url: base_url,
            api_key: None,
            timeout_secs: 30,
        })
    }
    
    pub async fn health_check(&self) -> Result<HealthResponse> {
        let url = format!("{}/health", self.base_url);
        
        let response = self.client
            .get(&url)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow!("Health check failed with status: {}", response.status()));
        }
        
        let health: HealthResponse = response.json().await?;
        Ok(health)
    }
    
    pub async fn put_file(&self, path: &str, content: Vec<u8>) -> Result<()> {
        let url = if path.starts_with("/s5/fs") {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/s5/fs/{}", self.base_url, path.trim_start_matches('/'))
        };
        
        info!("PUT file to: {}", url);
        
        let response = self.client
            .put(&url)
            .body(content)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Failed to PUT file: {} - {}", status, error_text));
        }
        
        Ok(())
    }
    
    pub async fn get_file(&self, path: &str) -> Result<Vec<u8>> {
        let url = if path.starts_with("/s5/fs") {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/s5/fs/{}", self.base_url, path.trim_start_matches('/'))
        };
        
        info!("GET file from: {}", url);
        
        let response = self.client
            .get(&url)
            .send()
            .await?;
        
        if response.status() == 404 {
            return Err(anyhow!("File not found: {}", path));
        }
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Failed to GET file: {} - {}", status, error_text));
        }
        
        let content = response.bytes().await?;
        Ok(content.to_vec())
    }
    
    pub async fn list_directory(&self, path: &str) -> Result<Vec<S5File>> {
        // Ensure path ends with / for directory listing
        let formatted_path = if path.starts_with("/s5/fs") {
            if !path.ends_with('/') {
                format!("{}/", path)
            } else {
                path.to_string()
            }
        } else {
            let clean_path = path.trim_start_matches('/').trim_end_matches('/');
            format!("/s5/fs/{}/", clean_path)
        };
        
        let url = format!("{}{}", self.base_url, formatted_path);
        
        info!("LIST directory: {}", url);
        
        let response = self.client
            .get(&url)
            .send()
            .await?;
        
        if response.status() == 404 {
            // Directory doesn't exist, return empty list
            return Ok(Vec::new());
        }
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Failed to list directory: {} - {}", status, error_text));
        }
        
        let files: Vec<S5File> = response.json().await?;
        Ok(files)
    }
    
    pub async fn delete_file(&self, path: &str) -> Result<()> {
        let url = if path.starts_with("/s5/fs") {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/s5/fs/{}", self.base_url, path.trim_start_matches('/'))
        };
        
        info!("DELETE file: {}", url);
        
        let response = self.client
            .delete(&url)
            .send()
            .await?;
        
        // Delete should be idempotent - 404 is okay
        if response.status() == 404 {
            warn!("File not found for deletion (idempotent): {}", path);
            return Ok(());
        }
        
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow!("Failed to DELETE file: {} - {}", status, error_text));
        }
        
        Ok(())
    }
    
    pub async fn exists(&self, path: &str) -> Result<bool> {
        let url = if path.starts_with("/s5/fs") {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/s5/fs/{}", self.base_url, path.trim_start_matches('/'))
        };
        
        let response = self.client
            .head(&url)
            .send()
            .await?;
        
        Ok(response.status().is_success())
    }
    
    // New methods for E2E workflow tests
    pub async fn put(&self, path: &str, data: Vec<u8>, metadata: Option<JsonValue>) -> Result<String> {
        // Generate a mock CID using BLAKE3-like hash
        let mut hasher = Sha256::new();
        hasher.update(&data);
        hasher.update(path.as_bytes());
        let hash_result = hasher.finalize();
        let cid = format!("bafybei{}", hex::encode(&hash_result[..16]));
        
        // Store in mock storage
        let mut storage = self.mock_storage.lock().unwrap();
        storage.insert(path.to_string(), (data, metadata));
        
        info!("Stored data at path: {} with CID: {}", path, cid);
        Ok(cid)
    }
    
    pub async fn get(&self, path: &str) -> Result<(Vec<u8>, Option<JsonValue>)> {
        // Retrieve from mock storage
        let storage = self.mock_storage.lock().unwrap();
        
        if let Some(entry) = storage.get(path) {
            Ok(entry.clone())
        } else {
            Err(anyhow!("File not found at path: {}", path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_client_creation() {
        let config = S5Config {
            api_url: "http://localhost:5524".to_string(),
            api_key: None,
            timeout_secs: 30,
        };
        let client = EnhancedS5Client::new(config);
        assert!(client.is_ok());
    }
    
    #[tokio::test]
    async fn test_legacy_client_creation() {
        let client = EnhancedS5Client::new_legacy("http://localhost:5524".to_string());
        assert!(client.is_ok());
    }
    
    #[tokio::test]
    async fn test_path_formatting() {
        let client = EnhancedS5Client::new_legacy("http://localhost:5524".to_string()).unwrap();
        
        // Test various path formats are handled correctly
        let test_paths = vec![
            "/s5/fs/test/file.txt",
            "test/file.txt",
            "/test/file.txt",
            "s5/fs/test/file.txt",
        ];
        
        for path in test_paths {
            // Just ensure no panic occurs
            let _ = client.exists(path).await;
        }
    }
}