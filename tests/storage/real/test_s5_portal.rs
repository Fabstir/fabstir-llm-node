// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use std::time::Instant;

#[tokio::test]
async fn test_real_s5_connectivity() {
    println!("\n=== REAL S5 PORTAL CONNECTIVITY TEST ===");
    
    // For now, just test that we can reach the portal
    let client = reqwest::Client::new();
    let response = client.get("https://s5.vup.cx/s5/health")
        .send()
        .await
        .expect("Failed to connect to S5 portal");
    
    assert!(response.status().is_success(), "S5 portal not healthy");
    println!("✅ Connected to S5 portal");
}

#[tokio::test]
async fn test_real_s5_upload() {
    println!("\n=== REAL S5 UPLOAD TEST ===");
    
    let test_data = b"Hello from fabstir-llm-node Phase 4.3.1!";
    let client = reqwest::Client::new();
    
    let start = Instant::now();
    
    // Use Enhanced S5.js endpoint which handles the real S5 connection
    let response = client.put("http://localhost:5524/s5/test/phase_4_3_1.txt")
        .body(test_data.to_vec())
        .send()
        .await
        .expect("Failed to upload");
    
    let duration = start.elapsed();
    
    assert!(response.status().is_success(), "Upload failed");
    println!("✅ Uploaded to S5 via Enhanced S5.js in {:?}", duration);
}
