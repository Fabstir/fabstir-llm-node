use fabstir_llm_node::vector::{VectorDbClient, VectorDbConfig};
use serde_json::json;
use std::time::Instant;

fn perf_config() -> VectorDbConfig {
    VectorDbConfig {
        api_url: "http://localhost:8080".to_string(),
        api_key: None,
        timeout_secs: 300,
    }
}

#[tokio::test]
async fn test_1k_vectors() {
    println!("\n=== 1K VECTOR SCALE TEST ===");
    let client = VectorDbClient::new(perf_config()).unwrap();
    
    let batch_size = 100;
    let total_batches = 10;
    
    for batch in 0..total_batches {
        let batch_start = Instant::now();
        
        for i in 0..batch_size {
            let id = format!("scale_1k_{}_{}", batch, i);
            let vector = vec![0.01 * (batch * batch_size + i) as f32; 3];
            let metadata = json!({
                "batch": batch,
                "test": "1k_scale"
            });
            
            let vector_data = json!({
                "id": id,
                "vector": vector,
                "metadata": metadata
            });
            
            client.insert_vector_json(vector_data).await.unwrap();
        }
        
        let batch_duration = batch_start.elapsed();
        println!("Batch {}/{}: {:?}", batch + 1, total_batches, batch_duration);
    }
    
    println!("1K vectors inserted successfully");
}

#[tokio::test]
#[ignore]
async fn test_10k_scale() {
    use std::time::Duration;
    
    println!("\n=== 10K VECTOR SCALE TEST ===");
    
    let client = VectorDbClient::new(perf_config()).unwrap();
    let total = 10000;
    let batch_size = 500;
    let start = Instant::now();
    
    for batch_num in 0..(total / batch_size) {
        let batch_start = Instant::now();
        
        for i in 0..batch_size {
            let id = batch_num * batch_size + i;
            let vector_data = json!({
                "id": format!("scale_10k_{}", id),
                "vector": vec![0.1, 0.2, 0.3],
                "metadata": {"batch": batch_num}
            });
            
            if let Err(e) = client.insert_vector_json(vector_data).await {
                eprintln!("Failed at vector {}: {:?}", id, e);
                break;
            }
        }
        
        let batch_elapsed = batch_start.elapsed();
        let total_elapsed = start.elapsed();
        let inserted = (batch_num + 1) * batch_size;
        let rate = inserted as f64 / total_elapsed.as_secs_f64();
        
        println!("Batch {}/20: {} vectors in {:.2}s (overall: {:.1} vec/s)", 
                 batch_num + 1, inserted, total_elapsed.as_secs_f64(), rate);
        
        // Brief pause between batches
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    
    let duration = start.elapsed();
    println!("\n=== 10K SCALE RESULTS ===");
    println!("Total: 10000 vectors in {:.2}s", duration.as_secs_f64());
    println!("Throughput: {:.2} vectors/second", 10000.0 / duration.as_secs_f64());
}
