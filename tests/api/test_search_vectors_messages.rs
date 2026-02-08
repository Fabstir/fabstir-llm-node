// TDD Tests for Search Vectors Messages (Sub-phase 2.2)
// Written FIRST before implementation

use fabstir_llm_node::api::websocket::message_types::{
    SearchVectorsRequest, SearchVectorsResponse, VectorSearchResult,
};
use serde_json::json;

#[test]
fn test_search_request_serialization() {
    let request = SearchVectorsRequest {
        request_id: Some("search-123".to_string()),
        query_vector: vec![0.5; 384],
        k: 10,
        threshold: Some(0.7),
        metadata_filter: Some(json!({"category": {"$eq": "science"}})),
    };

    // Serialize to JSON
    let json_str = serde_json::to_string(&request).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    // Verify structure (camelCase)
    assert_eq!(parsed["requestId"], "search-123");
    assert_eq!(parsed["queryVector"].as_array().unwrap().len(), 384);
    assert_eq!(parsed["k"], 10);
    assert_eq!(parsed["threshold"], 0.7);
    assert!(parsed["metadataFilter"].is_object());
}

#[test]
fn test_search_response_deserialization() {
    let json_str = r#"{
        "requestId": "search-123",
        "results": [
            {
                "id": "doc1",
                "score": 0.95,
                "metadata": {"title": "Test Doc"}
            },
            {
                "id": "doc2",
                "score": 0.87,
                "metadata": {"title": "Another Doc"}
            }
        ],
        "totalVectors": 1000,
        "searchTimeMs": 12.5
    }"#;

    let response: SearchVectorsResponse = serde_json::from_str(json_str).unwrap();

    assert_eq!(response.request_id, Some("search-123".to_string()));
    assert_eq!(response.results.len(), 2);
    assert_eq!(response.results[0].id, "doc1");
    assert_eq!(response.results[0].score, 0.95);
    assert_eq!(response.results[0].metadata["title"], "Test Doc");
    assert_eq!(response.total_vectors, 1000);
    assert_eq!(response.search_time_ms, 12.5);
}

#[test]
fn test_search_validates_k_limit() {
    use fabstir_llm_node::api::websocket::message_types::MAX_SEARCH_K;

    // Verify constant exists
    assert_eq!(MAX_SEARCH_K, 100);

    // Test validation with k too large
    let request = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 150, // Too large
        threshold: None,
        metadata_filter: None,
    };

    let validation_result = request.validate();
    assert!(validation_result.is_err());
    assert!(validation_result.unwrap_err().to_string().contains("100"));
}

#[test]
fn test_search_validates_query_dimensions() {
    let request = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 256], // Wrong dimensions
        k: 10,
        threshold: None,
        metadata_filter: None,
    };

    let validation_result = request.validate();
    assert!(validation_result.is_err());
    assert!(validation_result.unwrap_err().to_string().contains("384"));
}

#[test]
fn test_search_with_threshold() {
    // Test with threshold
    let request_with_threshold = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 5,
        threshold: Some(0.8),
        metadata_filter: None,
    };

    let json_str = serde_json::to_string(&request_with_threshold).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["threshold"], 0.8);

    // Test without threshold (should be null or omitted)
    let request_no_threshold = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 5,
        threshold: None,
        metadata_filter: None,
    };

    let json_str = serde_json::to_string(&request_no_threshold).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(parsed.get("threshold").is_none() || parsed["threshold"].is_null());
}

#[test]
fn test_search_with_metadata_filter() {
    // Test with complex filter
    let filter = json!({
        "category": {"$eq": "science"},
        "author": {"$in": ["John", "Jane"]}
    });

    let request = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 10,
        threshold: None,
        metadata_filter: Some(filter.clone()),
    };

    let json_str = serde_json::to_string(&request).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed["metadataFilter"]["category"]["$eq"], "science");
    assert_eq!(parsed["metadataFilter"]["author"]["$in"][0], "John");

    // Test without filter
    let request_no_filter = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 10,
        threshold: None,
        metadata_filter: None,
    };

    let json_str = serde_json::to_string(&request_no_filter).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(parsed.get("metadataFilter").is_none() || parsed["metadataFilter"].is_null());
}

#[test]
fn test_search_timing_included() {
    let response = SearchVectorsResponse {
        msg_type: "searchVectorsResponse".to_string(),
        request_id: Some("req-456".to_string()),
        results: vec![VectorSearchResult {
            id: "doc1".to_string(),
            score: 0.95,
            metadata: json!({"title": "Test"}),
        }],
        total_vectors: 500,
        search_time_ms: 8.3,
    };

    // Serialize
    let json_str = serde_json::to_string(&response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    // Verify timing is included
    assert!(parsed["searchTimeMs"].is_number());
    assert_eq!(parsed["searchTimeMs"], 8.3);
    assert_eq!(parsed["totalVectors"], 500);
}

#[test]
fn test_search_empty_results() {
    // Test response with no results (e.g., all filtered out by threshold)
    let response = SearchVectorsResponse {
        msg_type: "searchVectorsResponse".to_string(),
        request_id: Some("req-789".to_string()),
        results: vec![], // Empty results
        total_vectors: 100,
        search_time_ms: 5.2,
    };

    let json_str = serde_json::to_string(&response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed["results"].as_array().unwrap().len(), 0);
    assert_eq!(parsed["totalVectors"], 100);
    assert!(parsed["searchTimeMs"].is_number());

    // Should still deserialize correctly
    let deserialized: SearchVectorsResponse = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.results.len(), 0);
    assert_eq!(deserialized.total_vectors, 100);
}

#[test]
fn test_search_result_structure() {
    let result = VectorSearchResult {
        id: "doc-42".to_string(),
        score: 0.923,
        metadata: json!({
            "title": "Machine Learning Tutorial",
            "author": "John Doe",
            "page": 15,
            "tags": ["ml", "ai"]
        }),
    };

    // Serialize and verify structure
    let json_str = serde_json::to_string(&result).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed["id"], "doc-42");
    assert_eq!(parsed["score"], 0.923);
    assert_eq!(parsed["metadata"]["title"], "Machine Learning Tutorial");
    assert_eq!(parsed["metadata"]["page"], 15);
    assert_eq!(parsed["metadata"]["tags"][0], "ml");
}
