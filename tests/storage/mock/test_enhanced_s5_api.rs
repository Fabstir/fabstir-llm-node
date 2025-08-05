// tests/storage/mock/test_enhanced_s5_api.rs
// Phase 4.1.1: Enhanced S5.js with Internal Mock - API Integration Tests
// Updated to match actual Enhanced S5.js API

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

// Enhanced S5.js API types (based on actual implementation)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct S5File {
    name: String,
    size: u64,
    #[serde(rename = "type")]
    file_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    #[serde(rename = "mockStorage")] 
    mock_storage: bool,
    server: String,
    version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DirectoryListing {
    path: String,
    entries: Vec<S5File>,
}

// Test configuration
struct TestConfig {
    enhanced_s5_url: String,
    client: Client,
}

impl TestConfig {
    fn new() -> Self {
        let enhanced_s5_url = std::env::var("ENHANCED_S5_URL")
            .unwrap_or_else(|_| "http://localhost:5524".to_string());
        
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            enhanced_s5_url,
            client,
        }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.enhanced_s5_url, path)
    }
}

#[tokio::test]
async fn test_enhanced_s5_health_check() -> Result<()> {
    let config = TestConfig::new();
    
    // Wait for service to be ready
    let mut retries = 5;
    while retries > 0 {
        match config.client
            .get(config.api_url("/health"))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                let health: HealthResponse = response.json().await?;
                assert_eq!(health.status, "ok");
                assert!(health.mock_storage);
                println!("Health check passed: {} v{}", health.server, health.version);
                return Ok(());
            }
            _ => {
                retries -= 1;
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
    
    panic!("Enhanced S5.js health check failed after retries");
}

#[tokio::test]
async fn test_enhanced_s5_basic_put_get() -> Result<()> {
    let config = TestConfig::new();
    let test_path = "/s5/fs/test/basic/file.txt";
    let test_content = b"Hello from Enhanced S5.js mock!";
    
    // PUT file
    let response = config.client
        .put(config.api_url(test_path))
        .header("Content-Type", "application/octet-stream")
        .body(test_content.to_vec())
        .send()
        .await?;
    
    assert!(response.status().is_success(), 
            "PUT failed with status: {}", response.status());
    
    // GET file
    let response = config.client
        .get(config.api_url(test_path))
        .send()
        .await?;
    
    assert_eq!(response.status(), 200);
    let content = response.bytes().await?;
    assert_eq!(content.as_ref(), test_content);
    
    Ok(())
}

#[tokio::test]
async fn test_enhanced_s5_path_structure() -> Result<()> {
    let config = TestConfig::new();
    
    // Test different path structures
    let test_cases = vec![
        ("/s5/fs/home/user/document.pdf", "home directory"),
        ("/s5/fs/archive/2024/report.doc", "archive directory"),
        ("/s5/fs/shared/project/code.rs", "shared directory"),
    ];
    
    for (path, description) in test_cases {
        let content = format!("Test content for {}", description).into_bytes();
        
        // PUT file with structured path
        let response = config.client
            .put(config.api_url(path))
            .header("Content-Type", "application/octet-stream")
            .body(content.clone())
            .send()
            .await?;
        
        assert!(response.status().is_success(), 
                "Failed to PUT {}", description);
        
        // Verify file can be retrieved
        let response = config.client
            .get(config.api_url(path))
            .send()
            .await?;
        
        assert_eq!(response.status(), 200);
        let retrieved = response.bytes().await?;
        assert_eq!(retrieved.as_ref(), content.as_slice());
    }
    
    Ok(())
}

#[tokio::test]
async fn test_enhanced_s5_directory_listing() -> Result<()> {
    let config = TestConfig::new();
    let test_dir = "/s5/fs/test/listing";
    
    // Create some files
    for i in 0..5 {
        let file_path = format!("{}/file_{}.txt", test_dir, i);
        let response = config.client
            .put(config.api_url(&file_path))
            .header("Content-Type", "application/octet-stream")
            .body(format!("Content {}", i).into_bytes())
            .send()
            .await?;
        
        assert!(response.status().is_success());
    }
    
    // List directory (note trailing slash)
    let response = config.client
        .get(config.api_url(&format!("{}/", test_dir)))
        .send()
        .await?;
    
    assert_eq!(response.status(), 200);
    let listing: DirectoryListing = response.json().await?;
    
    // Should have 5 files
    assert_eq!(listing.entries.len(), 5, "Expected 5 files in directory");
    
    // Verify file properties
    for file in listing.entries.iter() {
        assert!(file.name.starts_with("file_"));
        assert_eq!(file.file_type, "file");
        assert!(file.size > 0);
    }
    
    Ok(())
}

#[tokio::test]
async fn test_enhanced_s5_content_types() -> Result<()> {
    let config = TestConfig::new();
    
    // Test CBOR content type
    let cbor_path = "/s5/fs/test/data.cbor";
    let cbor_data = vec![0x82, 0x01, 0x02]; // Simple CBOR array [1, 2]
    
    let response = config.client
        .put(config.api_url(cbor_path))
        .header("Content-Type", "application/cbor")
        .body(cbor_data.clone())
        .send()
        .await?;
    
    assert!(response.status().is_success());
    
    // Retrieve and verify
    let response = config.client
        .get(config.api_url(cbor_path))
        .send()
        .await?;
    
    assert_eq!(response.status(), 200);
    let content = response.bytes().await?;
    assert_eq!(content.as_ref(), cbor_data.as_slice());
    
    Ok(())
}

#[tokio::test]
async fn test_enhanced_s5_delete_operations() -> Result<()> {
    let config = TestConfig::new();
    let test_path = "/s5/fs/test/delete/file.txt";
    let test_content = b"File to be deleted";
    
    // First, create a file
    let response = config.client
        .put(config.api_url(test_path))
        .header("Content-Type", "application/octet-stream")
        .body(test_content.to_vec())
        .send()
        .await?;
    
    assert!(response.status().is_success());
    
    // Delete the file
    let response = config.client
        .delete(config.api_url(test_path))
        .send()
        .await?;
    
    assert!(response.status().is_success());
    
    // Verify file is gone
    let response = config.client
        .get(config.api_url(test_path))
        .send()
        .await?;
    
    assert_eq!(response.status(), 404);
    
    Ok(())
}

#[tokio::test]
async fn test_enhanced_s5_large_files() -> Result<()> {
    let config = TestConfig::new();
    let test_path = "/s5/fs/test/large/file.bin";
    
    // Create a 5MB file (well under the 50MB limit)
    let large_content = vec![0xAB; 5 * 1024 * 1024];
    
    let response = config.client
        .put(config.api_url(test_path))
        .header("Content-Type", "application/octet-stream")
        .body(large_content.clone())
        .send()
        .await?;
    
    assert!(response.status().is_success());
    
    // Retrieve and verify size
    let response = config.client
        .get(config.api_url(test_path))
        .send()
        .await?;
    
    assert_eq!(response.status(), 200);
    let content = response.bytes().await?;
    assert_eq!(content.len(), large_content.len());
    
    Ok(())
}

#[tokio::test]
async fn test_enhanced_s5_concurrent_operations() -> Result<()> {
    let config = TestConfig::new();
    let base_path = "/s5/fs/test/concurrent";
    
    // Spawn multiple concurrent operations
    let mut handles = vec![];
    
    for i in 0..10 {
        let client = config.client.clone();
        let url = config.enhanced_s5_url.clone();
        let path = format!("{}/file_{}.txt", base_path, i);
        let content = format!("Concurrent file {}", i).into_bytes();
        
        let handle = tokio::spawn(async move {
            let response = client
                .put(format!("{}{}", url, path))
                .header("Content-Type", "application/octet-stream")
                .body(content)
                .send()
                .await
                .expect("Request failed");
            
            assert!(response.status().is_success());
        });
        
        handles.push(handle);
    }
    
    // Wait for all operations to complete
    for handle in handles {
        handle.await?;
    }
    
    // Verify all files exist by listing directory
    let response = config.client
        .get(config.api_url(&format!("{}/", base_path)))
        .send()
        .await?;
    
    assert_eq!(response.status(), 200);
    let listing: DirectoryListing = response.json().await?;
    assert_eq!(listing.entries.len(), 10, "Not all concurrent files were created");
    
    Ok(())
}

#[tokio::test]
async fn test_enhanced_s5_error_handling() -> Result<()> {
    let config = TestConfig::new();
    
    // Test various error scenarios
    
    // 1. Get non-existent file
    let response = config.client
        .get(config.api_url("/s5/fs/nonexistent/path.txt"))
        .send()
        .await?;
    
    assert_eq!(response.status(), 404);
    
    // 2. Delete non-existent file
    let response = config.client
        .delete(config.api_url("/s5/fs/nonexistent/file.txt"))
        .send()
        .await?;
    
    // Should succeed (idempotent) or return 404
    assert!(response.status() == 200 || response.status() == 404);
    
    // 3. Invalid method
    let response = config.client
        .patch(config.api_url("/s5/fs/test.txt"))
        .send()
        .await?;
    
    assert_eq!(response.status(), 404); // Enhanced S5.js returns 404 for unsupported methods
    
    Ok(())
}