use fabstir_llm_node::vector::{VectorDbClient, VectorDbConfig};
use serde_json::json;
use std::time::Instant;

fn perf_config() -> VectorDbConfig {
    VectorDbConfig {
        api_url: "http://localhost:8080".to_string(),
        api_key: None,
        timeout_secs: 60,
    }
}

#[tokio::test]
async fn test_baseline_throughput() {
    println!("\n=== BASELINE THROUGHPUT TEST ===");
    let client = VectorDbClient::new(perf_config()).unwrap();

    let count = 100;
    let start = Instant::now();

    for i in 0..count {
        let id = format!("perf_baseline_{}", i);
        let vector = vec![0.1 * (i as f32 / count as f32); 3];
        let metadata = json!({
            "test": "baseline",
            "index": i
        });

        let vector_data = json!({
            "id": id,
            "vector": vector,
            "metadata": metadata
        });

        match client.insert_vector_json(vector_data).await {
            Ok(_) => {
                if i % 10 == 0 {
                    println!("Inserted {} vectors...", i);
                }
            }
            Err(e) => {
                eprintln!("Failed at vector {}: {:?}", i, e);
                panic!("Test failed");
            }
        }
    }

    let duration = start.elapsed();
    let throughput = count as f64 / duration.as_secs_f64();

    println!("\n=== Results ===");
    println!("Vectors inserted: {}", count);
    println!("Total duration: {:?}", duration);
    println!("Throughput: {:.2} vectors/second", throughput);

    assert!(throughput > 1.0, "Throughput too low");
}
