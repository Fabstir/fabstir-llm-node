// TDD Tests for Upload Vectors Messages (Sub-phase 2.1)
// Written FIRST before implementation

use fabstir_llm_node::api::websocket::message_types::{
    UploadVectorsRequest, UploadVectorsResponse, VectorUpload,
};
use serde_json::json;

#[test]
fn test_upload_vectors_request_serialization() {
    let request = UploadVectorsRequest {
        request_id: Some("req-123".to_string()),
        vectors: vec![
            VectorUpload {
                id: "doc1".to_string(),
                vector: vec![0.1; 384],
                metadata: json!({"title": "Test Document"}),
            },
            VectorUpload {
                id: "doc2".to_string(),
                vector: vec![0.2; 384],
                metadata: json!({"title": "Another Doc"}),
            },
        ],
        replace: false,
    };

    // Serialize to JSON
    let json_str = serde_json::to_string(&request).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    // Verify structure (camelCase for JavaScript compatibility)
    assert!(parsed["requestId"].is_string());
    assert_eq!(parsed["requestId"], "req-123");
    assert!(parsed["vectors"].is_array());
    assert_eq!(parsed["vectors"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["replace"], false);

    // Verify vector structure
    let vec0 = &parsed["vectors"][0];
    assert_eq!(vec0["id"], "doc1");
    assert_eq!(vec0["vector"].as_array().unwrap().len(), 384);
    assert_eq!(vec0["metadata"]["title"], "Test Document");
}

#[test]
fn test_upload_vectors_response_deserialization() {
    let json_str = r#"{
        "requestId": "req-123",
        "uploaded": 5,
        "rejected": 2,
        "errors": ["Vector doc3: invalid dimensions", "Vector doc7: NaN value"]
    }"#;

    let response: UploadVectorsResponse = serde_json::from_str(json_str).unwrap();

    assert_eq!(response.request_id, Some("req-123".to_string()));
    assert_eq!(response.uploaded, 5);
    assert_eq!(response.rejected, 2);
    assert_eq!(response.errors.len(), 2);
    assert!(response.errors[0].contains("invalid dimensions"));
    assert!(response.errors[1].contains("NaN value"));
}

#[test]
fn test_upload_validates_batch_size() {
    // Test max batch size constant exists
    use fabstir_llm_node::api::websocket::message_types::MAX_UPLOAD_BATCH_SIZE;

    assert_eq!(MAX_UPLOAD_BATCH_SIZE, 1000);

    // Test validation function
    let too_many_vectors: Vec<VectorUpload> = (0..1001)
        .map(|i| VectorUpload {
            id: format!("doc{}", i),
            vector: vec![0.1; 384],
            metadata: json!({}),
        })
        .collect();

    let request = UploadVectorsRequest {
        request_id: None,
        vectors: too_many_vectors,
        replace: false,
    };

    let validation_result = request.validate();
    assert!(validation_result.is_err());
    assert!(validation_result
        .unwrap_err()
        .to_string()
        .contains("batch size"));
}

#[test]
fn test_upload_validates_dimensions() {
    let wrong_dimensions = VectorUpload {
        id: "doc1".to_string(),
        vector: vec![0.1; 256], // Wrong: should be 384
        metadata: json!({}),
    };

    let request = UploadVectorsRequest {
        request_id: None,
        vectors: vec![wrong_dimensions],
        replace: false,
    };

    let validation_result = request.validate();
    assert!(validation_result.is_err());
    assert!(validation_result.unwrap_err().to_string().contains("384"));
}

#[test]
fn test_upload_replace_flag() {
    // Test replace=true serialization
    let request_replace = UploadVectorsRequest {
        request_id: None,
        vectors: vec![VectorUpload {
            id: "doc1".to_string(),
            vector: vec![0.1; 384],
            metadata: json!({}),
        }],
        replace: true, // Should clear existing vectors
    };

    let json_str = serde_json::to_string(&request_replace).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["replace"], true);

    // Test replace=false serialization
    let request_append = UploadVectorsRequest {
        request_id: None,
        vectors: vec![VectorUpload {
            id: "doc1".to_string(),
            vector: vec![0.1; 384],
            metadata: json!({}),
        }],
        replace: false, // Should append to existing
    };

    let json_str = serde_json::to_string(&request_append).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["replace"], false);
}

#[test]
fn test_upload_with_metadata() {
    let complex_metadata = json!({
        "title": "Machine Learning Tutorial",
        "author": "John Doe",
        "page": 42,
        "tags": ["ml", "ai", "tutorial"],
        "published": "2024-01-15"
    });

    let upload = VectorUpload {
        id: "doc1".to_string(),
        vector: vec![0.5; 384],
        metadata: complex_metadata.clone(),
    };

    // Serialize and deserialize
    let json_str = serde_json::to_string(&upload).unwrap();
    let parsed: VectorUpload = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed.id, "doc1");
    assert_eq!(parsed.vector.len(), 384);
    assert_eq!(parsed.metadata["title"], "Machine Learning Tutorial");
    assert_eq!(parsed.metadata["page"], 42);
    assert_eq!(parsed.metadata["tags"][0], "ml");
}

#[test]
fn test_upload_error_messages_clear() {
    let response = UploadVectorsResponse {
        msg_type: "uploadVectorsResponse".to_string(),
        request_id: Some("req-456".to_string()),
        uploaded: 3,
        rejected: 2,
        errors: vec![
            "doc5: Invalid vector dimensions: expected 384, got 256".to_string(),
            "doc7: Metadata too large: 15000 bytes (max: 10240 bytes / ~10KB)".to_string(),
        ],
    };

    // Serialize to JSON
    let json_str = serde_json::to_string(&response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    // Verify error messages are clear and descriptive
    assert_eq!(parsed["errors"].as_array().unwrap().len(), 2);
    assert!(parsed["errors"][0].as_str().unwrap().contains("384"));
    assert!(parsed["errors"][1].as_str().unwrap().contains("10KB"));
}

#[test]
fn test_upload_request_id_preserved() {
    // Test with request_id
    let request_with_id = UploadVectorsRequest {
        request_id: Some("custom-req-789".to_string()),
        vectors: vec![VectorUpload {
            id: "doc1".to_string(),
            vector: vec![0.1; 384],
            metadata: json!({}),
        }],
        replace: false,
    };

    let json_str = serde_json::to_string(&request_with_id).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["requestId"], "custom-req-789");

    // Test without request_id (should be optional)
    let request_without_id = UploadVectorsRequest {
        request_id: None,
        vectors: vec![VectorUpload {
            id: "doc1".to_string(),
            vector: vec![0.1; 384],
            metadata: json!({}),
        }],
        replace: false,
    };

    let json_str = serde_json::to_string(&request_without_id).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(parsed.get("requestId").is_none() || parsed["requestId"].is_null());

    // Test response preserves request_id
    let response = UploadVectorsResponse {
        msg_type: "uploadVectorsResponse".to_string(),
        request_id: Some("custom-req-789".to_string()),
        uploaded: 1,
        rejected: 0,
        errors: vec![],
    };

    let json_str = serde_json::to_string(&response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["requestId"], "custom-req-789");
}
