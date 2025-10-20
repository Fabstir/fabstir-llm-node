// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::vector::{VectorDbClient, VectorDbConfig};
use futures::future::join_all;
use serde_json::json;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_with_delays() {
    println!("=== TEST WITH DELAYS BETWEEN INSERTIONS ===");

    let config = VectorDbConfig {
        api_url: "http://localhost:8080".to_string(),
        api_key: None,
        timeout_secs: 10,
    };

    let client = VectorDbClient::new(config).unwrap();
    let count = 100;

    let start = Instant::now();
    let mut successful = 0;

    for i in 0..count {
        let vector_data = json!({
            "id": format!("delayed_{}", i),
            "vector": vec![0.1, 0.2, 0.3],
            "metadata": {"index": i}
        });

        match client.insert_vector_json(vector_data).await {
            Ok(_) => {
                successful += 1;
                if i % 10 == 0 {
                    println!("Inserted {} vectors...", i);
                }

                // Add small delay after each insert
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(e) => {
                eprintln!("Failed at vector {}: {:?}", i, e);
                // Try to continue with delay
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    let duration = start.elapsed();
    let throughput = successful as f64 / duration.as_secs_f64();

    println!("\n=== RESULTS ===");
    println!("Successfully inserted {} vectors with delays", successful);
    println!("Total duration: {:?}", duration);
    println!("Throughput: {:.2} vectors/second", throughput);
}

#[tokio::test]
async fn test_concurrent_batches() {
    println!("=== TEST WITH CONCURRENT BATCH INSERTIONS ===");

    let batch_size = 10;
    let num_batches = 10;

    let start = Instant::now();
    let mut tasks = Vec::new();

    for batch in 0..num_batches {
        let task = tokio::spawn(async move {
            let config = VectorDbConfig {
                api_url: "http://localhost:8080".to_string(),
                api_key: None,
                timeout_secs: 10,
            };

            let client = match VectorDbClient::new(config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Batch {} failed to create client: {:?}", batch, e);
                    return 0;
                }
            };

            let mut successful = 0;

            for i in 0..batch_size {
                let id = batch * batch_size + i;
                let vector_data = json!({
                    "id": format!("concurrent_{}_{}", batch, i),
                    "vector": vec![0.1, 0.2, 0.3],
                    "metadata": {"batch": batch, "index": i}
                });

                match client.insert_vector_json(vector_data).await {
                    Ok(_) => successful += 1,
                    Err(e) => eprintln!("Batch {} vector {} failed: {:?}", batch, i, e),
                }

                // Small delay to avoid overwhelming
                tokio::time::sleep(Duration::from_millis(50)).await;
            }

            successful
        });

        tasks.push(task);
    }

    // Wait for all batches to complete
    let results = join_all(tasks).await;
    let total_successful: usize = results.iter().filter_map(|r| r.as_ref().ok()).sum();

    let duration = start.elapsed();
    let throughput = total_successful as f64 / duration.as_secs_f64();

    println!("\n=== RESULTS ===");
    println!(
        "Successfully inserted {} vectors concurrently",
        total_successful
    );
    println!("Total duration: {:?}", duration);
    println!("Throughput: {:.2} vectors/second", throughput);
}

#[tokio::test]
async fn test_raw_http_client() {
    println!("=== TEST WITH RAW HTTP CLIENT (BYPASSING VectorDbClient) ===");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let count = 50;
    let start = Instant::now();
    let mut successful = 0;

    for i in 0..count {
        let data = json!({
            "id": format!("raw_http_{}", i),
            "vector": vec![0.1, 0.2, 0.3],
            "metadata": {"index": i}
        });

        let response = client
            .post("http://localhost:8080/api/v1/vectors")
            .json(&data)
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                successful += 1;
                if i % 10 == 0 {
                    println!("Raw HTTP: Inserted {} vectors", i);
                }
            }
            Ok(resp) => {
                eprintln!(
                    "Raw HTTP: Vector {} failed with status: {}",
                    i,
                    resp.status()
                );
            }
            Err(e) => {
                eprintln!("Raw HTTP: Vector {} error: {:?}", i, e);
                if i >= 32 {
                    println!("Raw HTTP also fails at vector {}!", i);
                    break;
                }
            }
        }
    }

    let duration = start.elapsed();
    let throughput = successful as f64 / duration.as_secs_f64();

    println!("\n=== RESULTS ===");
    println!("Raw HTTP: Successfully inserted {} vectors", successful);
    println!("Total duration: {:?}", duration);
    println!("Throughput: {:.2} vectors/second", throughput);
}
