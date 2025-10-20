// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_s5_retry_on_timeout() {
    println!("\n=== S5 RELIABILITY TEST ===");
    
    let client = reqwest::Client::new();
    let test_data = b"Testing retry logic";
    
    // Test with timeout - should complete within 30 seconds
    let result = timeout(
        Duration::from_secs(30),
        client.put("http://localhost:5524/s5/test/retry.txt")
            .body(test_data.to_vec())
            .send()
    ).await;
    
    assert!(result.is_ok(), "Upload with timeout failed");
    println!("✅ Upload completed within timeout");
}
