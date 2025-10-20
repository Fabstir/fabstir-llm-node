// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
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
async fn test_s5_api_patterns() {
    println!("\n=== S5 API PATTERN MONITORING ===");
    let client = VectorDbClient::new(perf_config()).unwrap();

    for i in 0..20 {
        let insert_start = Instant::now();
        let id = format!("monitor_{}", i);
        let vector = vec![0.1 * i as f32; 3];
        let metadata = json!({ "monitoring": true });

        let vector_data = json!({
            "id": id,
            "vector": vector,
            "metadata": metadata
        });

        client.insert_vector_json(vector_data).await.unwrap();
        let insert_duration = insert_start.elapsed();

        println!("Insert {}: {:?}", i, insert_duration);
    }

    println!("Monitoring test complete");
}
