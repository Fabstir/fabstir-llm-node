use fabstir_llm_node::vector::vector_db_client::VectorDbClient;
use serde_json::json;

#[tokio::test]
async fn test_vector_db_health_check() {
    let client = VectorDbClient::new("http://host.docker.internal:7530").unwrap();
    
    let result = client.health_check().await;
    assert!(result.is_ok(), "Health check should succeed");
    
    let health = result.unwrap();
    assert_eq!(health["status"], "healthy");
}

#[tokio::test]
async fn test_insert_single_vector() {
    let client = VectorDbClient::new("http://host.docker.internal:7530").unwrap();
    
    let vector_data = json!({
        "vector": [0.1, 0.2, 0.3, 0.4, 0.5],
        "metadata": {
            "source": "test",
            "timestamp": "2024-01-01T00:00:00Z"
        }
    });
    
    let result = client.insert_vector(vector_data).await;
    assert!(result.is_ok(), "Insert should succeed");
    
    let response = result.unwrap();
    assert!(response["id"].is_string());
    assert_eq!(response["status"], "inserted");
}

#[tokio::test]
async fn test_get_vector_by_id() {
    let client = VectorDbClient::new("http://host.docker.internal:7530").unwrap();
    
    // First insert a vector
    let vector_data = json!({
        "vector": [0.6, 0.7, 0.8, 0.9, 1.0],
        "metadata": {
            "test": "get_by_id"
        }
    });
    
    let insert_result = client.insert_vector(vector_data.clone()).await.unwrap();
    let vector_id = insert_result["id"].as_str().unwrap();
    
    // Now get it back
    let result = client.get_vector(vector_id).await;
    assert!(result.is_ok(), "Get should succeed");
    
    let retrieved = result.unwrap();
    assert_eq!(retrieved["id"], vector_id);
    assert!(retrieved["vector"].is_array());
}

#[tokio::test]
async fn test_search_similar_vectors() {
    let client = VectorDbClient::new("http://host.docker.internal:7530").unwrap();
    
    // Insert some vectors first
    for i in 0..3 {
        let vector_data = json!({
            "vector": vec![0.1 * i as f64; 5],
            "metadata": {
                "index": i
            }
        });
        client.insert_vector(vector_data).await.unwrap();
    }
    
    // Search for similar vectors
    let search_query = json!({
        "query_vector": [0.15, 0.15, 0.15, 0.15, 0.15],
        "top_k": 2
    });
    
    let result = client.search_vectors(search_query).await;
    assert!(result.is_ok(), "Search should succeed");
    
    let results = result.unwrap();
    assert!(results["results"].is_array());
    let results_array = results["results"].as_array().unwrap();
    assert!(results_array.len() <= 2);
}

#[tokio::test]
async fn test_delete_vector() {
    let client = VectorDbClient::new("http://host.docker.internal:7530").unwrap();
    
    // Insert a vector
    let vector_data = json!({
        "vector": [1.0, 2.0, 3.0, 4.0, 5.0],
        "metadata": {
            "test": "delete"
        }
    });
    
    let insert_result = client.insert_vector(vector_data).await.unwrap();
    let vector_id = insert_result["id"].as_str().unwrap();
    
    // Delete it
    let result = client.delete_vector(vector_id).await;
    assert!(result.is_ok(), "Delete should succeed");
    
    let response = result.unwrap();
    assert_eq!(response["status"], "deleted");
    
    // Verify it's gone
    let get_result = client.get_vector(vector_id).await;
    assert!(get_result.is_err() || get_result.unwrap()["error"].is_string());
}

#[tokio::test]
async fn test_batch_insert_vectors() {
    let client = VectorDbClient::new("http://host.docker.internal:7530").unwrap();
    
    let batch_data = json!({
        "vectors": [
            {
                "vector": [0.1, 0.2, 0.3, 0.4, 0.5],
                "metadata": {"batch": 1}
            },
            {
                "vector": [0.6, 0.7, 0.8, 0.9, 1.0],
                "metadata": {"batch": 2}
            },
            {
                "vector": [1.1, 1.2, 1.3, 1.4, 1.5],
                "metadata": {"batch": 3}
            }
        ]
    });
    
    let result = client.batch_insert(batch_data).await;
    assert!(result.is_ok(), "Batch insert should succeed");
    
    let response = result.unwrap();
    assert!(response["inserted_count"].is_number());
    assert_eq!(response["inserted_count"], 3);
}

#[tokio::test]
async fn test_error_handling_invalid_url() {
    let result = VectorDbClient::new("not-a-valid-url");
    assert!(result.is_err(), "Should fail with invalid URL");
}

#[tokio::test]
async fn test_error_handling_connection_refused() {
    let client = VectorDbClient::new("http://localhost:9999").unwrap();
    
    let result = client.health_check().await;
    assert!(result.is_err(), "Should fail when server is not running");
}

#[tokio::test]
async fn test_search_with_filters() {
    let client = VectorDbClient::new("http://host.docker.internal:7530").unwrap();
    
    // Insert vectors with different metadata
    for i in 0..5 {
        let vector_data = json!({
            "vector": vec![0.1 * i as f64; 5],
            "metadata": {
                "category": if i % 2 == 0 { "even" } else { "odd" },
                "value": i
            }
        });
        client.insert_vector(vector_data).await.unwrap();
    }
    
    // Search with filters
    let search_query = json!({
        "query_vector": [0.25, 0.25, 0.25, 0.25, 0.25],
        "top_k": 3,
        "filters": {
            "category": "even"
        }
    });
    
    let result = client.search_vectors(search_query).await;
    assert!(result.is_ok(), "Filtered search should succeed");
    
    let results = result.unwrap();
    let results_array = results["results"].as_array().unwrap();
    
    // Verify all results have "even" category
    for item in results_array {
        if let Some(metadata) = item.get("metadata") {
            assert_eq!(metadata["category"], "even");
        }
    }
}