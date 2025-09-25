use anyhow::Result;
use fabstir_llm_node::api::websocket::{integration::*, manager::*, session::*};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_e2e_session_lifecycle() -> Result<()> {
    // Simulate complete session lifecycle
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());
    
    // Create session
    let session = WebSocketSession::new("e2e-test-1");
    manager.register_session(session.clone()).await?;
    
    // Process inference request
    let request = InferenceRequest {
        prompt: "Hello, world!".to_string(),
        session_id: Some(session.id.clone()),
        temperature: 0.7,
        max_tokens: 100,
    };
    
    let response = integration.process_with_context(request).await?;
    assert_eq!(response.session_id, Some(session.id.clone()));
    
    // Close session
    manager.remove_session(&session.id).await;
    assert!(!manager.has_session(&session.id).await);
    
    Ok(())
}

#[tokio::test]
async fn test_e2e_multi_turn_conversation() -> Result<()> {
    // Test multi-turn conversation with context
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());
    
    // Initialize session
    let mut session = WebSocketSession::new("conversation-test");
    manager.register_session(session.clone()).await?;
    
    // First turn
    session.add_message_async("user", "My name is Alice").await?;
    
    let request1 = InferenceRequest {
        prompt: "My name is Alice".to_string(),
        session_id: Some(session.id.clone()),
        temperature: 0.7,
        max_tokens: 50,
    };
    integration.process_with_context(request1).await?;
    
    // Second turn - should remember context
    let request2 = InferenceRequest {
        prompt: "What is my name?".to_string(),
        session_id: Some(session.id.clone()),
        temperature: 0.7,
        max_tokens: 50,
    };
    
    let response = integration.process_with_context(request2).await?;
    assert!(response.context_used);
    assert!(response.messages_included > 0);
    
    Ok(())
}

#[tokio::test]
async fn test_e2e_concurrent_clients() -> Result<()> {
    // Test multiple concurrent client sessions
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());
    
    let mut handles = vec![];
    
    for i in 0..5 {
        let int = integration.clone();
        let mgr = manager.clone();
        
        let handle = tokio::spawn(async move {
            // Each client has its own session
            let session = WebSocketSession::new(format!("client-{}", i));
            mgr.register_session(session.clone()).await?;
            
            // Send request
            let request = InferenceRequest {
                prompt: format!("Request from client {}", i),
                session_id: Some(session.id.clone()),
                temperature: 0.7,
                max_tokens: 50,
            };
            
            int.process_with_context(request).await?;
            Ok::<_, anyhow::Error>(())
        });
        handles.push(handle);
    }
    
    // All clients should succeed
    for handle in handles {
        handle.await??;
    }
    
    assert_eq!(manager.session_count().await, 5);
    
    Ok(())
}

#[tokio::test]
async fn test_e2e_session_recovery() -> Result<()> {
    // Test session recovery after simulated disconnect
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());
    
    // First connection
    let mut session = WebSocketSession::new("recovery-test");
    manager.register_session(session.clone()).await?;
    
    // Add some context
    session.add_message_async("user", "Remember the number 42").await?;
    
    // Simulate disconnect by changing state
    session.set_state(SessionState::Failed).await?;
    
    // Recover session
    let recovered = integration.recover_session(&session.id).await?;
    assert_eq!(recovered.id, session.id);
    
    // Context should be preserved
    assert_eq!(recovered.metadata.read().await.get("recovered"), Some(&"true".to_string()));
    
    Ok(())
}

#[tokio::test]
async fn test_e2e_load_testing() -> Result<()> {
    // Load test with many requests
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());
    
    // Initialize session
    let session = WebSocketSession::new("load-test");
    manager.register_session(session.clone()).await?;
    
    // Send many requests rapidly
    for i in 0..50 {
        integration.process_session_request(&session, &format!("Request {}", i)).await?;
    }
    
    // Check metrics
    let stats = integration.get_statistics().await?;
    assert_eq!(stats.sessions_processed, 50);
    
    Ok(())
}

#[tokio::test]
async fn test_e2e_error_recovery() -> Result<()> {
    // Test error handling and recovery
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());
    
    // Send invalid request
    let invalid_request = InferenceRequest {
        prompt: "".to_string(), // Invalid empty prompt
        session_id: Some("error-test".to_string()),
        temperature: 2.0, // Invalid temperature
        max_tokens: -1, // Invalid tokens
    };
    
    // Should get error
    let result = integration.process_with_context(invalid_request).await;
    assert!(result.is_err());
    
    // Connection should still work with valid request
    let valid_request = InferenceRequest {
        prompt: "Valid request".to_string(),
        session_id: Some("error-recovery".to_string()),
        temperature: 0.7,
        max_tokens: 50,
    };
    
    let response = integration.process_with_context(valid_request).await?;
    assert!(response.session_id.is_some());
    
    Ok(())
}

#[tokio::test]
async fn test_e2e_streaming_response() -> Result<()> {
    // Test streaming response simulation
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());
    
    let session = WebSocketSession::new("stream-test");
    manager.register_session(session.clone()).await?;
    
    // Request streaming response
    let request = InferenceRequest {
        prompt: "Tell me a story".to_string(),
        session_id: Some(session.id.clone()),
        temperature: 0.7,
        max_tokens: 100,
    };
    
    // Process request (simulated streaming)
    let response = integration.process_with_context(request).await?;
    assert!(response.session_id.is_some());
    
    Ok(())
}

#[tokio::test]
async fn test_e2e_heartbeat_mechanism() -> Result<()> {
    // Test heartbeat keepalive simulation
    let manager = Arc::new(SessionManager::new());
    
    // Initialize session
    let session = WebSocketSession::new("heartbeat-test");
    manager.register_session(session.clone()).await?;
    
    // Simulate heartbeats
    for _ in 0..3 {
        // Update last activity
        session.metadata.write().await.insert("last_heartbeat".to_string(), 
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs()
                .to_string());
        
        sleep(Duration::from_millis(100)).await;
    }
    
    // Session should still be active
    assert!(manager.has_session(&session.id).await);
    
    Ok(())
}

#[tokio::test]
async fn test_e2e_metadata_sync() -> Result<()> {
    // Test metadata synchronization
    let manager = Arc::new(SessionManager::new());
    
    let session = WebSocketSession::new("metadata-test");
    
    // Set metadata
    session.metadata.write().await.insert("user_id".to_string(), "user123".to_string());
    session.metadata.write().await.insert("model".to_string(), "llama2".to_string());
    session.metadata.write().await.insert("temperature".to_string(), "0.7".to_string());
    
    manager.register_session(session.clone()).await?;
    
    // Retrieve and verify metadata
    if let Some(retrieved) = manager.get_session(&session.id).await {
        let metadata = retrieved.metadata.read().await;
        assert_eq!(metadata.get("user_id"), Some(&"user123".to_string()));
        assert_eq!(metadata.get("model"), Some(&"llama2".to_string()));
        assert_eq!(metadata.get("temperature"), Some(&"0.7".to_string()));
    }
    
    Ok(())
}

#[tokio::test]
async fn test_e2e_graceful_shutdown() -> Result<()> {
    // Test graceful shutdown scenario
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());
    
    // Connect multiple sessions
    for i in 0..3 {
        let session = WebSocketSession::new(format!("shutdown-{}", i));
        manager.register_session(session).await?;
    }
    
    assert_eq!(manager.session_count().await, 3);
    
    // Trigger shutdown
    integration.shutdown().await?;
    
    // All sessions should be closed
    assert_eq!(manager.session_count().await, 0);
    
    // Check cleanup statistics
    let stats = integration.get_statistics().await?;
    assert_eq!(stats.resources_freed, 3);
    
    Ok(())
}