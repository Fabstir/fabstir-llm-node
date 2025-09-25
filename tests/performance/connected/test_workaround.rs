use fabstir_llm_node::vector::{VectorDbClient, VectorDbConfig};
use serde_json::json;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_with_connection_reset() {
    println!("=== WORKAROUND TEST: Reset Connection Every 30 Vectors ===");

    let mut total_inserted = 0;
    let target = 100;
    let batch_size = 30; // Reset connection every 30 vectors

    let overall_start = Instant::now();

    while total_inserted < target {
        // Create new client every 30 vectors
        let config = VectorDbConfig {
            api_url: "http://localhost:8080".to_string(),
            api_key: None,
            timeout_secs: 10,
        };

        let client = VectorDbClient::new(config).unwrap();
        println!(
            "\n[Batch {}] Creating new connection...",
            total_inserted / batch_size
        );

        // Insert batch
        let batch_start = Instant::now();
        let mut batch_count = 0;

        for i in 0..batch_size {
            let id = total_inserted + i;
            if id >= target {
                break;
            }

            let vector_data = json!({
                "id": format!("batch_{}", id),
                "vector": vec![0.1, 0.2, 0.3],
                "metadata": {"batch": id / batch_size, "index": id}
            });

            match client.insert_vector_json(vector_data).await {
                Ok(_) => {
                    batch_count += 1;
                    if id % 10 == 0 {
                        println!("  Inserted {} vectors total", id);
                    }
                }
                Err(e) => {
                    eprintln!("  Failed at vector {}: {:?}", id, e);
                    break;
                }
            }
        }

        let batch_duration = batch_start.elapsed();
        println!(
            "  Batch completed: {} vectors in {:?}",
            batch_count, batch_duration
        );

        total_inserted += batch_count;

        // Drop client to reset connection
        drop(client);

        // Small delay between batches
        if total_inserted < target {
            println!("  Waiting 100ms before next batch...");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    let total_duration = overall_start.elapsed();
    let throughput = total_inserted as f64 / total_duration.as_secs_f64();

    println!("\n=== RESULTS ===");
    println!(
        "Successfully inserted {} vectors with connection resets",
        total_inserted
    );
    println!("Total duration: {:?}", total_duration);
    println!("Overall throughput: {:.2} vectors/second", throughput);

    assert_eq!(total_inserted, target, "Should insert all vectors");
}

#[tokio::test]
async fn test_with_small_batches() {
    println!("=== WORKAROUND TEST: Small Batches (10 vectors) ===");

    let config = VectorDbConfig {
        api_url: "http://localhost:8080".to_string(),
        api_key: None,
        timeout_secs: 10,
    };

    let client = VectorDbClient::new(config).unwrap();
    let target = 100;
    let batch_size = 10;

    let start = Instant::now();
    let mut total_inserted = 0;

    for batch in 0..(target / batch_size) {
        println!(
            "\nBatch {}: inserting vectors {}..{}",
            batch,
            batch * batch_size,
            (batch + 1) * batch_size - 1
        );

        for i in 0..batch_size {
            let id = batch * batch_size + i;

            let vector_data = json!({
                "id": format!("small_batch_{}", id),
                "vector": vec![0.1, 0.2, 0.3],
                "metadata": {"batch": batch, "index": i}
            });

            match client.insert_vector_json(vector_data).await {
                Ok(_) => {
                    total_inserted += 1;
                }
                Err(e) => {
                    eprintln!("Failed at vector {}: {:?}", id, e);
                    break;
                }
            }
        }

        // Small delay between batches
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let duration = start.elapsed();
    let throughput = total_inserted as f64 / duration.as_secs_f64();

    println!("\n=== RESULTS ===");
    println!("Inserted {} vectors in small batches", total_inserted);
    println!("Total duration: {:?}", duration);
    println!("Throughput: {:.2} vectors/second", throughput);
}
