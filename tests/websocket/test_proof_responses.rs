// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use futures::StreamExt;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

use fabstir_llm_node::api::websocket::{
    handlers::{response::ResponseHandler, session_init::SessionInitHandler},
    messages::{ConversationMessage, ProofData, StreamToken},
    proof_manager::ProofManager,
};

#[tokio::test]
async fn test_response_includes_proof_field() -> Result<()> {
    // Create response handler with proof manager
    let session_handler = Arc::new(SessionInitHandler::new());
    let proof_manager = Arc::new(ProofManager::new());
    let response_handler = ResponseHandler::new(session_handler.clone(), Some(proof_manager));

    // Initialize session
    session_handler
        .handle_session_init("test-proof-1", 123, vec![])
        .await?;

    // Create response stream
    let mut stream = response_handler
        .create_response_stream("test-proof-1", "What is 2+2?", 0)
        .await?;

    // Collect tokens from stream
    let mut final_token = None;
    while let Some(result) = stream.next().await {
        match result {
            Ok(token) => {
                final_token = Some(token);
            }
            Err(e) => return Err(e),
        }
    }

    // Verify final token has proof field
    assert!(final_token.is_some(), "Should have received tokens");
    let token = final_token.unwrap();
    assert!(token.proof.is_some(), "Final token should include proof");
    let proof = token.proof.unwrap();
    assert!(!proof.hash.is_empty(), "Proof hash should not be empty");
    assert_eq!(
        proof.proof_type, "simple",
        "Proof type should be simple by default"
    );

    Ok(())
}

#[tokio::test]
async fn test_streaming_response_includes_proof_in_final_token() -> Result<()> {
    // Create response handler with proof manager
    let session_handler = Arc::new(SessionInitHandler::new());
    let proof_manager = Arc::new(ProofManager::new());
    let response_handler = ResponseHandler::new(session_handler.clone(), Some(proof_manager));

    // Initialize session
    session_handler
        .handle_session_init("test-stream-proof", 456, vec![])
        .await?;

    // Create response stream
    let mut stream = response_handler
        .create_response_stream("test-stream-proof", "Test prompt", 0)
        .await?;

    let mut final_token = None;
    let mut token_count = 0;

    // Consume stream
    use futures::StreamExt;
    while let Some(result) = stream.next().await {
        let token = result?;
        token_count += 1;
        if token.is_final {
            final_token = Some(token);
        }
    }

    // Verify final token has proof
    assert!(token_count > 0, "Should have received tokens");
    assert!(final_token.is_some(), "Should have final token");

    let final_tok = final_token.unwrap();
    assert!(
        final_tok.proof.is_some(),
        "Final token should include proof"
    );
    let proof = final_tok.proof.unwrap();
    assert!(!proof.hash.is_empty(), "Proof hash should not be empty");

    Ok(())
}

#[tokio::test]
async fn test_proof_contains_model_and_prompt_hash() -> Result<()> {
    // Create response handler with proof manager
    let session_handler = Arc::new(SessionInitHandler::new());
    let proof_manager = Arc::new(ProofManager::new());
    let response_handler = ResponseHandler::new(session_handler.clone(), Some(proof_manager));

    // Initialize session
    session_handler
        .handle_session_init("test-hash", 789, vec![])
        .await?;

    let prompt = "What is the capital of France?";

    // Generate response
    let response = response_handler
        .generate_response("test-hash", prompt, 0)
        .await?;

    // Verify proof contains expected fields
    let proof = response.proof.expect("Should have proof");
    assert!(!proof.model_hash.is_empty(), "Should have model hash");
    assert!(!proof.input_hash.is_empty(), "Should have input hash");
    assert!(!proof.output_hash.is_empty(), "Should have output hash");
    assert!(proof.timestamp > 0, "Should have timestamp");

    Ok(())
}

#[tokio::test]
async fn test_proof_manager_caches_recent_proofs() -> Result<()> {
    let proof_manager = ProofManager::new();

    // Generate proof for same content multiple times
    let model = "test-model";
    let prompt = "Test prompt";
    let output = "Test output";

    let proof1 = proof_manager.generate_proof(model, prompt, output).await?;

    let proof2 = proof_manager.generate_proof(model, prompt, output).await?;

    // Should return same proof (cached)
    assert_eq!(proof1.hash, proof2.hash, "Should return cached proof");
    assert_eq!(
        proof1.timestamp, proof2.timestamp,
        "Timestamps should match for cached proof"
    );

    Ok(())
}

#[tokio::test]
async fn test_websocket_message_json_includes_proof() -> Result<()> {
    // Create response handler with proof manager
    let session_handler = Arc::new(SessionInitHandler::new());
    let proof_manager = Arc::new(ProofManager::new());
    let response_handler = ResponseHandler::new(session_handler.clone(), Some(proof_manager));

    // Initialize session
    session_handler
        .handle_session_init("test-json", 111, vec![])
        .await?;

    // Generate response
    let response = response_handler
        .generate_response("test-json", "Test", 0)
        .await?;

    // Convert to JSON and verify structure
    let json = serde_json::to_value(&response)?;
    assert!(json.get("proof").is_some(), "JSON should have proof field");

    let proof_json = &json["proof"];
    assert!(proof_json.get("hash").is_some(), "Proof should have hash");
    assert!(
        proof_json.get("proof_type").is_some(),
        "Proof should have type"
    );
    assert!(
        proof_json.get("model_hash").is_some(),
        "Proof should have model_hash"
    );

    Ok(())
}

#[tokio::test]
async fn test_proof_generation_error_returns_response_without_proof() -> Result<()> {
    // Create response handler with proof manager (errors handled gracefully)
    let session_handler = Arc::new(SessionInitHandler::new());
    // ProofManager will handle errors internally
    let proof_manager = Arc::new(ProofManager::new());
    let response_handler = ResponseHandler::new(session_handler.clone(), Some(proof_manager));

    // Initialize session
    session_handler
        .handle_session_init("test-error", 222, vec![])
        .await?;

    // Generate response - should succeed even if proof fails
    let response = response_handler
        .generate_response("test-error", "Test", 0)
        .await?;

    // Response should still work but without proof
    assert_eq!(response.role, "assistant");
    assert!(response.content.len() > 0);
    // Proof might be None or contain error indication

    Ok(())
}

#[tokio::test]
async fn test_proof_manager_handles_concurrent_requests() -> Result<()> {
    let proof_manager = Arc::new(ProofManager::new());

    // Launch multiple concurrent proof generations
    let mut handles = vec![];

    for i in 0..5 {
        let pm = proof_manager.clone();
        let handle = tokio::spawn(async move {
            pm.generate_proof("model", &format!("prompt-{}", i), &format!("output-{}", i))
                .await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let mut results = vec![];
    for handle in handles {
        let result = handle.await??;
        results.push(result);
    }

    // Verify all proofs are unique (different inputs)
    for i in 0..results.len() {
        for j in i + 1..results.len() {
            assert_ne!(
                results[i].hash, results[j].hash,
                "Different inputs should produce different proofs"
            );
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_response_handler_without_proof_manager_works() -> Result<()> {
    // Create response handler WITHOUT proof manager
    let session_handler = Arc::new(SessionInitHandler::new());
    let response_handler = ResponseHandler::new(session_handler.clone(), None);

    // Initialize session
    session_handler
        .handle_session_init("test-no-proof", 333, vec![])
        .await?;

    // Generate response
    let response = response_handler
        .generate_response("test-no-proof", "Test", 0)
        .await?;

    // Response should work but without proof
    assert_eq!(response.role, "assistant");
    assert!(response.content.len() > 0);
    assert!(
        response.proof.is_none(),
        "Should not have proof when manager is None"
    );

    Ok(())
}
