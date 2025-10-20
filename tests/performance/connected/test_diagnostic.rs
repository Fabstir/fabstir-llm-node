// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::vector::{VectorDbClient, VectorDbConfig};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::timeout;

#[tokio::test]
async fn test_diagnose_32_limit() {
    println!("=== DIAGNOSTIC TEST: 32 Vector Issue ===");

    let config = VectorDbConfig {
        api_url: "http://localhost:8080".to_string(),
        api_key: None,
        timeout_secs: 5, // Short timeout per request
    };

    let client = VectorDbClient::new(config).unwrap();

    // Test exactly around the problem area
    for i in 30..35 {
        println!("\nAttempting to insert vector {}", i);
        let start = Instant::now();

        let vector_data = json!({
            "id": format!("diag_{}", i),
            "vector": vec![0.1, 0.2, 0.3],
            "metadata": {"index": i}
        });

        // Use timeout for each request
        match timeout(
            Duration::from_secs(5),
            client.insert_vector_json(vector_data),
        )
        .await
        {
            Ok(Ok(_)) => {
                println!("  ✓ Vector {} inserted in {:?}", i, start.elapsed());
            }
            Ok(Err(e)) => {
                println!("  ✗ Vector {} failed: {:?}", i, e);
            }
            Err(_) => {
                println!("  ✗ Vector {} TIMED OUT after 5 seconds", i);
                println!("  Connection appears to be stuck at vector {}", i);
                break;
            }
        }
    }

    println!("\n=== Testing if new connection works ===");
    // Try with a fresh client
    let config2 = VectorDbConfig {
        api_url: "http://localhost:8080".to_string(),
        api_key: None,
        timeout_secs: 5,
    };

    let client2 = VectorDbClient::new(config2).unwrap();
    let vector_data = json!({
        "id": "diag_fresh_connection",
        "vector": vec![0.1, 0.2, 0.3],
        "metadata": {"test": "fresh"}
    });

    match timeout(
        Duration::from_secs(5),
        client2.insert_vector_json(vector_data),
    )
    .await
    {
        Ok(Ok(_)) => {
            println!("  ✓ Fresh connection works!");
        }
        Ok(Err(e)) => {
            println!("  ✗ Fresh connection failed: {:?}", e);
        }
        Err(_) => {
            println!("  ✗ Fresh connection also timed out");
        }
    }
}

#[tokio::test]
async fn test_exact_32_boundary() {
    println!("=== TESTING EXACT 32 BOUNDARY ===");

    let config = VectorDbConfig {
        api_url: "http://localhost:8080".to_string(),
        api_key: None,
        timeout_secs: 3,
    };

    let client = VectorDbClient::new(config).unwrap();

    // Insert exactly 32 vectors
    for i in 0..33 {
        let vector_data = json!({
            "id": format!("boundary_{}", i),
            "vector": vec![0.1, 0.2, 0.3],
            "metadata": {"index": i}
        });

        match timeout(
            Duration::from_secs(3),
            client.insert_vector_json(vector_data),
        )
        .await
        {
            Ok(Ok(_)) => {
                if i == 31 {
                    println!("✓ Vector 31 (32nd vector) inserted successfully");
                } else if i == 32 {
                    println!("✓ Vector 32 (33rd vector) inserted successfully");
                }
            }
            Ok(Err(e)) => {
                println!("✗ Vector {} failed: {:?}", i, e);
                break;
            }
            Err(_) => {
                println!("✗ Vector {} timed out - confirming issue at index {}", i, i);
                break;
            }
        }
    }
}
