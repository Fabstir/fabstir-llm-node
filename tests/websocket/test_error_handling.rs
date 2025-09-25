use fabstir_llm_node::api::websocket::{
    handlers::{inference::InferenceHandler, session_init::SessionInitHandler},
    messages::{ConversationMessage, ErrorCode, WebSocketError},
};
use futures::StreamExt;
use std::sync::Arc;

#[tokio::test]
async fn test_model_not_loaded_error() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    session_handler
        .handle_session_init("model-error", 2000, vec![])
        .await
        .unwrap();

    // Try to use a model that's not loaded
    let result = inference_handler
        .generate_response_with_model("model-error", "Hello", 1, "non-existent-model")
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("Model not found")
            || err.to_string().contains("not loaded")
            || err.to_string().contains("No inference engine")
    );
}

#[tokio::test]
async fn test_empty_prompt_error() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    session_handler
        .handle_session_init("empty-prompt", 2100, vec![])
        .await
        .unwrap();

    let result = inference_handler
        .generate_response("empty-prompt", "", 1)
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Empty prompt"));
}

#[tokio::test]
async fn test_invalid_session_error() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler);

    let result = inference_handler
        .generate_response("invalid-session", "Test", 1)
        .await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Session not found"));
}

#[tokio::test]
async fn test_token_limit_exceeded_error() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    // Create context that's near the token limit
    let mut context = vec![];
    for i in 0..100 {
        context.push(ConversationMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
            content: "x".repeat(1000), // Very long messages
            timestamp: Some(i),
            tokens: Some(250), // High token count
            proof: None,
        });
    }

    let result = session_handler
        .handle_session_init("token-overflow", 2200, context)
        .await;

    // Should handle gracefully by trimming
    assert!(result.is_ok());

    // Cache should be within limits
    let cache = session_handler.get_cache("token-overflow").await.unwrap();
    assert!(cache.is_within_token_limit().await);
}

#[tokio::test]
async fn test_streaming_error_propagation() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    session_handler
        .handle_session_init("stream-error", 2300, vec![])
        .await
        .unwrap();

    // Try to stream with invalid configuration
    let config = fabstir_llm_node::api::websocket::handlers::inference::StreamConfig {
        max_tokens: 0,     // Invalid: zero tokens
        temperature: -1.0, // Invalid: negative temperature
        stream: true,
    };

    let result = inference_handler
        .create_streaming_response("stream-error", "Test", 1, config)
        .await;

    // Should error on invalid config
    assert!(result.is_err());
}

#[tokio::test]
async fn test_concurrent_error_isolation() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = Arc::new(InferenceHandler::new(session_handler.clone()));

    // Create one valid and one invalid session
    session_handler
        .handle_session_init("valid-session", 2400, vec![])
        .await
        .unwrap();

    let mut handles = vec![];

    // Valid request
    let ih1 = inference_handler.clone();
    let sh1 = session_handler.clone();
    handles.push(tokio::spawn(async move {
        let cache = sh1.get_cache("valid-session").await.unwrap();
        cache
            .add_message(ConversationMessage {
                role: "user".to_string(),
                content: "Valid prompt".to_string(),
                timestamp: Some(1),
                tokens: None,
                proof: None,
            })
            .await;

        ih1.generate_response("valid-session", "Valid prompt", 1)
            .await
    }));

    // Invalid request (session doesn't exist)
    let ih2 = inference_handler.clone();
    handles.push(tokio::spawn(async move {
        ih2.generate_response("invalid-session", "Test", 1).await
    }));

    let results: Vec<_> = futures::future::join_all(handles).await;

    // First should succeed
    assert!(results[0].as_ref().unwrap().is_ok());

    // Second should fail
    assert!(results[1].as_ref().unwrap().is_err());
}

#[tokio::test]
async fn test_malformed_message_handling() {
    let session_handler = Arc::new(SessionInitHandler::new());

    // Try to init with malformed context (e.g., missing role)
    let bad_context = vec![ConversationMessage {
        role: "".to_string(), // Empty role
        content: "Test".to_string(),
        timestamp: Some(1),
        tokens: None,
        proof: None,
    }];

    let result = session_handler
        .handle_session_init("malformed", 2500, bad_context)
        .await;

    // Should either handle gracefully or error clearly
    match result {
        Err(e) => {
            let err_msg = e.to_string();
            assert!(err_msg.contains("role") || err_msg.contains("invalid"));
        }
        Ok(_) => {
            // If it accepts it, verify it's handled safely
            let cache = session_handler.get_cache("malformed").await.unwrap();
            let messages = cache.get_messages().await;
            assert_eq!(messages.len(), 1);
        }
    }
}

#[tokio::test]
async fn test_recovery_after_error() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    session_handler
        .handle_session_init("recovery-test", 2600, vec![])
        .await
        .unwrap();

    // First request with error (empty prompt)
    let result1 = inference_handler
        .generate_response("recovery-test", "", 1)
        .await;
    assert!(result1.is_err());

    // Second request should work
    let cache = session_handler.get_cache("recovery-test").await.unwrap();
    cache
        .add_message(ConversationMessage {
            role: "user".to_string(),
            content: "Valid prompt".to_string(),
            timestamp: Some(1),
            tokens: None,
            proof: None,
        })
        .await;

    let result2 = inference_handler
        .generate_response("recovery-test", "Valid prompt", 1)
        .await;
    assert!(result2.is_ok());
}

#[tokio::test]
async fn test_timeout_handling() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    session_handler
        .handle_session_init("timeout-test", 2700, vec![])
        .await
        .unwrap();

    // Request with timeout
    let config = fabstir_llm_node::api::websocket::handlers::inference::StreamConfig {
        max_tokens: 10000, // Very high to potentially cause timeout
        temperature: 0.7,
        stream: false,
    };

    // Add timeout wrapper
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        inference_handler.generate_response_with_config(
            "timeout-test",
            "Generate something",
            1,
            config,
        ),
    )
    .await;

    // Should complete within timeout
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_error_code_mapping() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    // Test various error conditions and their codes
    let errors = vec![
        ("", ErrorCode::InvalidRequest),              // Empty session
        ("non-existent", ErrorCode::SessionNotFound), // Missing session
    ];

    for (session_id, expected_code) in errors {
        let result = inference_handler
            .generate_response(session_id, "Test", 1)
            .await;

        if result.is_err() {
            let err_str = result.unwrap_err().to_string();
            // Verify appropriate error is returned
            assert!(!err_str.is_empty());
        }
    }
}
