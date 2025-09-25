use reqwest::Client;
use serde_json::json;
use std::time::{Duration, Instant};

/// Create a properly configured HTTP client with connection pooling settings
fn create_optimized_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(10)
        .tcp_keepalive(Duration::from_secs(30))
        .build()
        .unwrap()
}

#[tokio::test]
async fn test_optimized_client() {
    println!("=== TEST WITH OPTIMIZED HTTP CLIENT ===");

    let client = create_optimized_client();
    let count = 100;
    let start = Instant::now();
    let mut successful = 0;
    let mut failures = Vec::new();

    for i in 0..count {
        let data = json!({
            "id": format!("optimized_{}", i),
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
                    println!("Inserted {} vectors...", i);
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let error_text = resp.text().await.unwrap_or_default();
                failures.push((i, format!("Status {}: {}", status, error_text)));
                eprintln!("Vector {} failed: Status {}", i, status);
            }
            Err(e) => {
                failures.push((i, format!("{:?}", e)));
                eprintln!("Vector {} error: {:?}", i, e);
            }
        }
    }

    let duration = start.elapsed();
    let throughput = successful as f64 / duration.as_secs_f64();

    println!("\n=== RESULTS ===");
    println!("Successfully inserted {} vectors", successful);
    println!("Failed: {} vectors", failures.len());
    if !failures.is_empty() {
        println!(
            "First failure at vector {}: {}",
            failures[0].0, failures[0].1
        );
    }
    println!("Total duration: {:?}", duration);
    println!("Throughput: {:.2} vectors/second", throughput);

    assert!(successful > 50, "Should insert at least 50 vectors");
}

#[tokio::test]
async fn test_with_new_client_per_request() {
    println!("=== TEST WITH NEW CLIENT PER REQUEST ===");

    let count = 50;
    let start = Instant::now();
    let mut successful = 0;

    for i in 0..count {
        // Create new client for each request (inefficient but might work)
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        let data = json!({
            "id": format!("new_client_{}", i),
            "vector": vec![0.1, 0.2, 0.3],
            "metadata": {"index": i}
        });

        match client
            .post("http://localhost:8080/api/v1/vectors")
            .json(&data)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                successful += 1;
                if i % 10 == 0 {
                    println!("Inserted {} vectors...", i);
                }
            }
            Ok(resp) => {
                eprintln!("Vector {} failed with status: {}", i, resp.status());
            }
            Err(e) => {
                eprintln!("Vector {} error: {:?}", i, e);
            }
        }
    }

    let duration = start.elapsed();
    let throughput = successful as f64 / duration.as_secs_f64();

    println!("\n=== RESULTS ===");
    println!(
        "Successfully inserted {} vectors (new client per request)",
        successful
    );
    println!("Total duration: {:?}", duration);
    println!("Throughput: {:.2} vectors/second", throughput);
}

#[tokio::test]
async fn test_batch_api() {
    println!("=== TEST USING BATCH API ===");

    let client = create_optimized_client();
    let total_vectors = 100;
    let batch_size = 20;

    let start = Instant::now();
    let mut total_successful = 0;

    for batch_num in 0..(total_vectors / batch_size) {
        let mut vectors = Vec::new();

        for i in 0..batch_size {
            let id = batch_num * batch_size + i;
            vectors.push(json!({
                "id": format!("batch_api_{}", id),
                "vector": vec![0.1, 0.2, 0.3],
                "metadata": {"batch": batch_num, "index": i}
            }));
        }

        let batch_data = json!({
            "vectors": vectors
        });

        println!("Sending batch {} with {} vectors...", batch_num, batch_size);

        match client
            .post("http://localhost:8080/api/v1/vectors/batch")
            .json(&batch_data)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<serde_json::Value>().await {
                    Ok(result) => {
                        let successful = result["successful"]
                            .as_u64()
                            .or_else(|| result["inserted_count"].as_u64())
                            .unwrap_or(0);
                        total_successful += successful as usize;
                        println!(
                            "  Batch {} succeeded: {} vectors inserted",
                            batch_num, successful
                        );
                    }
                    Err(e) => {
                        println!(
                            "  Batch {} succeeded but couldn't parse response: {:?}",
                            batch_num, e
                        );
                        total_successful += batch_size;
                    }
                }
            }
            Ok(resp) => {
                eprintln!(
                    "  Batch {} failed with status: {}",
                    batch_num,
                    resp.status()
                );
                if let Ok(text) = resp.text().await {
                    eprintln!("  Error: {}", text);
                }
            }
            Err(e) => {
                eprintln!("  Batch {} error: {:?}", batch_num, e);
            }
        }

        // Small delay between batches
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let duration = start.elapsed();
    let throughput = total_successful as f64 / duration.as_secs_f64();

    println!("\n=== RESULTS ===");
    println!(
        "Successfully inserted {} vectors using batch API",
        total_successful
    );
    println!("Total duration: {:?}", duration);
    println!("Throughput: {:.2} vectors/second", throughput);
}
