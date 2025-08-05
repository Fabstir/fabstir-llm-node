// src/storage/enhanced_s5_client.rs
// Phase 4.1.1: Enhanced S5.js with Internal Mock - HTTP Client Implementation

use anyhow::{Result, anyhow};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, warn, error};

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
}

impl EnhancedS5Client {
    pub fn new(base_url: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        
        Ok(Self {
            client,
            base_url,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_client_creation() {
        let client = EnhancedS5Client::new("http://localhost:5524".to_string());
        assert!(client.is_ok());
    }
    
    #[tokio::test]
    async fn test_path_formatting() {
        let client = EnhancedS5Client::new("http://localhost:5524".to_string()).unwrap();
        
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