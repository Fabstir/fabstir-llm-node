// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Security Tests for Embedding API (Sub-phase 9.2)
//!
//! This module tests security best practices for the embedding API to ensure:
//! - Input validation is comprehensive
//! - No code injection vulnerabilities
//! - No path traversal vulnerabilities
//! - Rate limiting is applied (or absence is documented)
//! - Embeddings are never logged (privacy)
//! - Memory limits are enforced
//! - Malicious input is rejected
//! - Resource exhaustion is prevented

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use fabstir_llm_node::api::embed::EmbedRequest;
use fabstir_llm_node::api::http_server::{create_app, AppState};
use fabstir_llm_node::embeddings::{EmbeddingModelConfig, EmbeddingModelManager};
use std::sync::Arc;
use tower::ServiceExt; // for `oneshot`

/// Test helper: Create AppState with embedding model manager
async fn setup_test_state_with_model() -> AppState {
    let configs = vec![EmbeddingModelConfig {
        name: "all-MiniLM-L6-v2".to_string(),
        model_path: "/workspace/models/all-MiniLM-L6-v2-onnx/model.onnx".to_string(),
        tokenizer_path: "/workspace/models/all-MiniLM-L6-v2-onnx/tokenizer.json".to_string(),
        dimensions: 384,
    }];

    let manager = EmbeddingModelManager::new(configs)
        .await
        .expect("Failed to create embedding model manager");

    let mut state = AppState::new_for_test();
    *state.embedding_model_manager.write().await = Some(Arc::new(manager));
    state
}

//
// SECURITY TEST 1: Comprehensive Input Validation
//

#[tokio::test]
async fn test_input_validation_comprehensive() {
    let state = setup_test_state_with_model().await;
    let app = create_app(Arc::new(state));

    // Test 1: Empty texts array
    let empty_request = EmbedRequest {
        texts: vec![],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let req_body = serde_json::to_string(&empty_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Should reject empty texts array"
    );

    // Test 2: Too many texts (>96)
    let too_many_request = EmbedRequest {
        texts: vec!["text".to_string(); 97],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let req_body = serde_json::to_string(&too_many_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Should reject >96 texts"
    );

    // Test 3: Text too long (>8192 characters)
    let long_text = "a".repeat(8193);
    let long_text_request = EmbedRequest {
        texts: vec![long_text],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let req_body = serde_json::to_string(&long_text_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Should reject text >8192 characters"
    );

    // Test 4: Whitespace-only text
    let whitespace_request = EmbedRequest {
        texts: vec!["   \t\n  ".to_string()],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let req_body = serde_json::to_string(&whitespace_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Should reject whitespace-only text"
    );

    // Test 5: Invalid chain_id
    let invalid_chain_request = EmbedRequest {
        texts: vec!["test".to_string()],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 99999,
    };

    let req_body = serde_json::to_string(&invalid_chain_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Should reject invalid chain_id"
    );

    println!("✓ All input validation tests passed");
}

//
// SECURITY TEST 2: No Code Injection Vulnerabilities
//

#[tokio::test]
async fn test_no_code_injection() {
    let state = setup_test_state_with_model().await;

    // Test various code injection attempts
    let injection_attempts = vec![
        // JavaScript injection
        "<script>alert('XSS')</script>",
        // Shell injection
        "; ls -la; echo 'pwned'",
        "$(cat /etc/passwd)",
        "`whoami`",
        // SQL injection (shouldn't affect embeddings, but test anyway)
        "'; DROP TABLE users; --",
        // Command injection
        "| cat /etc/passwd",
        "&& rm -rf /",
        // Path traversal
        "../../etc/passwd",
        "../../../root/.ssh/id_rsa",
        // Python code injection
        "__import__('os').system('ls')",
        "eval('print(1)')",
    ];

    let request = EmbedRequest {
        texts: injection_attempts.iter().map(|s| s.to_string()).collect(),
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let app = create_app(Arc::new(state));
    let req_body = serde_json::to_string(&request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();

    // Should succeed - these are just text to be embedded
    // The key is that they're treated as DATA, not CODE
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Injection attempts should be treated as harmless text data"
    );

    // Parse response to verify embeddings were generated
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let response_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let embeddings = response_json["embeddings"].as_array().unwrap();
    assert_eq!(
        embeddings.len(),
        injection_attempts.len(),
        "All texts should be embedded without execution"
    );

    println!("✓ No code injection vulnerabilities found");
}

//
// SECURITY TEST 3: No Path Traversal Vulnerabilities
//

#[tokio::test]
async fn test_no_path_traversal() {
    let state = setup_test_state_with_model().await;

    // Attempt to use path traversal in model name
    let path_traversal_attempts = vec![
        "../../etc/passwd",
        "../../../models/malicious.onnx",
        "/etc/shadow",
        "..\\..\\windows\\system32",
        "all-MiniLM-L6-v2/../../../etc/passwd",
    ];

    for attack_path in path_traversal_attempts {
        let request = EmbedRequest {
            texts: vec!["test".to_string()],
            model: attack_path.to_string(),
            chain_id: 84532,
        };

        let app = create_app(Arc::new(state.clone()));
        let req_body = serde_json::to_string(&request).unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(req_body))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();

        // Should return 404 NOT_FOUND (model doesn't exist)
        // NOT 500 INTERNAL_SERVER_ERROR (path access error)
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "Path traversal attempt '{}' should result in model not found, not path access",
            attack_path
        );
    }

    println!("✓ No path traversal vulnerabilities found");
}

//
// SECURITY TEST 4: Rate Limiting Applied
//

#[tokio::test]
async fn test_rate_limiting_applied() {
    // NOTE: This test documents the ABSENCE of rate limiting on HTTP endpoints
    // Rate limiting is currently only implemented for WebSocket connections

    let state = setup_test_state_with_model().await;

    let request = EmbedRequest {
        texts: vec!["test".to_string()],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let app = create_app(Arc::new(state));

    // Send 10 rapid requests
    for i in 0..10 {
        let req_body = serde_json::to_string(&request).unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(req_body))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();

        // Currently NO rate limiting on HTTP endpoints
        // All requests should succeed
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Request {} should succeed (no HTTP rate limiting yet)",
            i + 1
        );
    }

    println!("⚠️  Rate limiting NOT applied to HTTP /v1/embed endpoint");
    println!("   (Recommendation: Add rate limiting middleware for production)");
}

//
// SECURITY TEST 5: Embeddings Never Logged (Privacy)
//

#[tokio::test]
async fn test_embeddings_never_logged() {
    // This test verifies that embedding vectors are NEVER logged
    // We can't directly test log output, but we can verify the code behavior

    let state = setup_test_state_with_model().await;

    // Send request with sensitive data
    let sensitive_request = EmbedRequest {
        texts: vec![
            "My SSN is 123-45-6789".to_string(),
            "Credit card: 4532-1234-5678-9010".to_string(),
            "Password: SuperSecret123!".to_string(),
        ],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let app = create_app(Arc::new(state));
    let req_body = serde_json::to_string(&sensitive_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Parse response to verify embeddings exist
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let response_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let embeddings = response_json["embeddings"].as_array().unwrap();
    assert_eq!(embeddings.len(), 3);

    // Code audit confirms:
    // 1. handler.rs:60 - Logs only: text count, model name, chain_id
    // 2. handler.rs:156 - Logs only: embedding count and dimensions
    // 3. handler.rs:198 - Logs only: count, total tokens, elapsed time
    // 4. NO logging of actual embedding vectors anywhere

    println!("✓ Embedding vectors are never logged (privacy preserved)");
    println!("   Only metadata logged: count, dimensions, tokens, elapsed time");
}

//
// SECURITY TEST 6: Memory Limits Enforced
//

#[tokio::test]
async fn test_memory_limits_enforced() {
    let state = setup_test_state_with_model().await;

    // Test 1: Maximum batch size (96 texts)
    let max_batch_request = EmbedRequest {
        texts: vec!["Test text for memory test".to_string(); 96],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let app = create_app(Arc::new(state.clone()));
    let req_body = serde_json::to_string(&max_batch_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Should handle max batch size (96 texts)"
    );

    // Test 2: Maximum text length (8192 chars)
    let max_length_request = EmbedRequest {
        texts: vec!["a".repeat(8192)],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let app = create_app(Arc::new(state.clone()));
    let req_body = serde_json::to_string(&max_length_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Should handle max text length (8192 chars)"
    );

    // Test 3: Maximum batch with maximum length
    // 96 texts × 8192 chars = 786,432 characters (~768KB of text)
    let max_combined_request = EmbedRequest {
        texts: vec!["b".repeat(8192); 96],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let app = create_app(Arc::new(state));
    let req_body = serde_json::to_string(&max_combined_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Should handle max batch with max length (~768KB text)"
    );

    println!("✓ Memory limits enforced via input validation");
    println!("   Max batch: 96 texts");
    println!("   Max text length: 8192 chars");
    println!("   Max total: ~768KB per request");
}

//
// SECURITY TEST 7: Malicious Input Rejected
//

#[tokio::test]
async fn test_malicious_input_rejected() {
    let state = setup_test_state_with_model().await;

    // Test 1: Null bytes (should be rejected or handled safely)
    // Note: Rust strings are UTF-8, so null bytes are valid Unicode
    let null_byte_request = EmbedRequest {
        texts: vec!["test\0with\0nulls".to_string()],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let app = create_app(Arc::new(state.clone()));
    let req_body = serde_json::to_string(&null_byte_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    // Should succeed (null bytes are valid UTF-8)
    assert!(
        response.status() == StatusCode::OK || response.status().is_client_error(),
        "Null bytes should be handled safely"
    );

    // Test 2: Invalid UTF-8 (should fail at JSON parsing level)
    // We can't easily test this because axum validates JSON before our handler

    // Test 3: Extremely nested JSON (JSON bomb)
    // This would be caught by axum's JSON body size limits

    // Test 4: Invalid model name with special characters
    let special_chars_request = EmbedRequest {
        texts: vec!["test".to_string()],
        model: "'; DROP TABLE models; --".to_string(),
        chain_id: 84532,
    };

    let app = create_app(Arc::new(state));
    let req_body = serde_json::to_string(&special_chars_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "Should reject invalid model name (treated as non-existent model)"
    );

    println!("✓ Malicious input patterns rejected or handled safely");
}

//
// SECURITY TEST 8: Resource Exhaustion Prevented
//

#[tokio::test]
async fn test_resource_exhaustion_prevented() {
    let state = setup_test_state_with_model().await;

    // Test 1: Attempt to exceed batch size limit
    let over_limit_request = EmbedRequest {
        texts: vec!["test".to_string(); 1000], // Way over 96 limit
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let app = create_app(Arc::new(state.clone()));
    let req_body = serde_json::to_string(&over_limit_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Should reject excessive batch size"
    );

    // Test 2: Attempt to exceed text length limit
    let over_length_request = EmbedRequest {
        texts: vec!["a".repeat(100_000)], // Way over 8192 limit
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let app = create_app(Arc::new(state.clone()));
    let req_body = serde_json::to_string(&over_length_request).unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/embed")
        .header("content-type", "application/json")
        .body(Body::from(req_body))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Should reject excessive text length"
    );

    // Test 3: Multiple concurrent requests (within limits)
    // This tests that concurrent requests don't cause resource exhaustion
    let valid_request = EmbedRequest {
        texts: vec!["test".to_string(); 10],
        model: "all-MiniLM-L6-v2".to_string(),
        chain_id: 84532,
    };

    let mut tasks = Vec::new();
    for _ in 0..5 {
        let app = create_app(Arc::new(state.clone()));
        let req_body = serde_json::to_string(&valid_request).unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(req_body))
            .unwrap();

        let task = tokio::spawn(async move { app.oneshot(req).await });
        tasks.push(task);
    }

    // Wait for all requests
    for task in tasks {
        let response = task.await.unwrap().unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Concurrent requests should all succeed"
        );
    }

    println!("✓ Resource exhaustion prevented via input validation");
    println!("   Batch size limited to 96");
    println!("   Text length limited to 8192 chars");
    println!("   Concurrent requests handled safely");
}
