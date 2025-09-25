use anyhow::Result;
use fabstir_llm_node::api::websocket::{integration::*, manager::SessionManager, session::*};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_inference_pipeline_integration() -> Result<()> {
    // Test that sessions integrate with inference pipeline
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    // Create a session with context
    let mut session = WebSocketSession::new("test-session");
    manager.register_session(session.clone()).await?;

    // Add some context
    session.add_message_async("user", "What is 2+2?").await?;
    session
        .add_message_async("assistant", "2+2 equals 4")
        .await?;

    // Process inference request with session context
    let request = InferenceRequest {
        prompt: "What did I just ask about?".to_string(),
        session_id: Some(session.id.clone()),
        temperature: 0.7,
        max_tokens: 50,
    };

    let response = integration.process_with_context(request).await?;

    // Response should reference the context
    assert!(response.context_used);
    assert_eq!(response.session_id, Some(session.id.clone()));
    assert!(response.messages_included > 0);

    Ok(())
}

#[tokio::test]
async fn test_job_processor_session_support() -> Result<()> {
    // Test job processor with session persistence
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    // Create session for a job
    let job_id = "job-123";
    let session = integration.create_job_session(job_id).await?;

    assert_eq!(
        session.metadata.read().await.get("job_id"),
        Some(&job_id.to_string())
    );
    assert_eq!(session.state, SessionState::Active);

    // Process job stages with session
    integration
        .update_job_stage(&session, "preprocessing")
        .await?;
    integration.update_job_stage(&session, "inference").await?;
    integration
        .update_job_stage(&session, "postprocessing")
        .await?;

    let stages = integration.get_job_stages(&session).await?;
    assert_eq!(stages.len(), 3);

    Ok(())
}

#[tokio::test]
async fn test_session_persistence_hooks() -> Result<()> {
    // Test persistence hooks for saving/loading sessions
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    // Enable persistence
    integration.enable_persistence(true).await?;

    // Create and save session
    let mut session = WebSocketSession::new("persist-test");
    session.add_message_async("user", "Remember this").await?;

    let saved = integration.save_session(&session).await?;
    assert!(saved);

    // Load session
    let loaded = integration.load_session(&session.id).await?;
    assert_eq!(loaded.id, session.id);
    assert_eq!(loaded.messages.read().await.len(), 1);

    Ok(())
}

#[tokio::test]
async fn test_concurrent_session_handling() -> Result<()> {
    // Test handling multiple concurrent sessions
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    let mut handles = vec![];

    for i in 0..10 {
        let int = integration.clone();
        let handle = tokio::spawn(async move {
            let mut session = WebSocketSession::new(&format!("concurrent-{}", i));
            int.process_session_request(&session, "test request").await
        });
        handles.push(handle);
    }

    // All should complete successfully
    for handle in handles {
        let result = handle.await?;
        assert!(result.is_ok());
    }

    // Check session count
    let stats = integration.get_statistics().await?;
    assert_eq!(stats.sessions_processed, 10);

    Ok(())
}

#[tokio::test]
async fn test_session_recovery_mechanism() -> Result<()> {
    // Test session recovery after failures
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    // Create session and simulate failure
    let mut session = WebSocketSession::new("recovery-test");
    session.set_state(SessionState::Failed).await?;

    // Attempt recovery
    let recovered = integration.recover_session(&session.id).await?;
    assert_eq!(recovered.state, SessionState::Active);
    assert_eq!(recovered.id, session.id);

    // Recovery should preserve context
    assert_eq!(
        recovered.metadata.read().await.get("recovered"),
        Some(&"true".to_string())
    );

    Ok(())
}

#[tokio::test]
async fn test_pipeline_error_handling() -> Result<()> {
    // Test error handling in integrated pipeline
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    // Create session with invalid request
    let mut session = WebSocketSession::new("error-test");

    let request = InferenceRequest {
        prompt: "".to_string(), // Invalid empty prompt
        session_id: Some(session.id.clone()),
        temperature: 2.0, // Invalid temperature
        max_tokens: -1,   // Invalid tokens
    };

    let result = integration.process_with_context(request).await;
    assert!(result.is_err());

    // Session should track error
    let error_count = integration.get_session_errors(&session.id).await?;
    assert!(error_count > 0);

    Ok(())
}

#[tokio::test]
async fn test_session_handoff_between_workers() -> Result<()> {
    // Test session handoff between different workers
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    // Create session on worker 1
    let mut session = integration.create_worker_session("worker-1").await?;
    session.add_message_async("user", "Initial message").await?;

    // Handoff to worker 2
    let handed_off = integration.handoff_session(&session.id, "worker-2").await?;
    assert!(handed_off);

    // Verify session on worker 2
    let worker_2_session = integration
        .get_worker_session("worker-2", &session.id)
        .await?;
    assert_eq!(worker_2_session.messages.read().await.len(), 1);

    Ok(())
}

#[tokio::test]
async fn test_batch_processing_with_sessions() -> Result<()> {
    // Test batch processing while maintaining session context
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    // Create multiple sessions for batch
    let mut sessions = vec![];
    for i in 0..5 {
        let mut session = WebSocketSession::new(&format!("batch-{}", i));
        session
            .add_message_async("user", &format!("Question {}", i))
            .await?;
        sessions.push(session);
    }

    // Process batch
    let results = integration.process_batch(sessions.clone()).await?;

    assert_eq!(results.len(), 5);
    for (i, result) in results.iter().enumerate() {
        assert_eq!(result.session_id, sessions[i].id);
        assert!(result.success);
    }

    Ok(())
}

#[tokio::test]
async fn test_session_timeout_handling() -> Result<()> {
    // Test session timeout and cleanup
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    // Configure short timeout
    integration
        .set_session_timeout(Duration::from_millis(100))
        .await?;

    // Create session
    let mut session = WebSocketSession::new("timeout-test");
    manager.register_session(session.clone()).await?;

    // Wait for timeout
    sleep(Duration::from_millis(150)).await;

    // Session should be cleaned up
    let exists = manager.has_session(&session.id).await;
    assert!(!exists);

    // Cleanup callback should have been called
    let stats = integration.get_statistics().await?;
    assert_eq!(stats.sessions_timed_out, 1);

    Ok(())
}

#[tokio::test]
async fn test_resource_cleanup_on_shutdown() -> Result<()> {
    // Test proper resource cleanup during shutdown
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    // Create multiple sessions
    for i in 0..5 {
        let mut session = WebSocketSession::new(&format!("cleanup-{}", i));
        manager.register_session(session).await?;
    }

    // Initiate shutdown
    integration.shutdown().await?;

    // All sessions should be closed
    let active = manager.get_active_sessions().await;
    assert_eq!(active.len(), 0);

    // Resources should be freed
    let stats = integration.get_statistics().await?;
    assert_eq!(stats.resources_freed, 5);

    Ok(())
}

#[tokio::test]
async fn test_metrics_integration() -> Result<()> {
    // Test metrics collection during integration
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    // Process some requests
    for i in 0..10 {
        let mut session = WebSocketSession::new(&format!("metrics-{}", i));
        integration
            .process_session_request(&session, "test")
            .await?;
    }

    // Check metrics
    let metrics = integration.get_metrics().await?;

    assert_eq!(metrics.total_requests, 10);
    assert!(metrics.avg_response_time_ms > 0.0);
    assert_eq!(metrics.error_rate, 0.0);

    Ok(())
}

#[tokio::test]
async fn test_load_balancing_integration() -> Result<()> {
    // Test load balancing across workers with sessions
    let manager = Arc::new(SessionManager::new());
    let integration = SessionIntegration::new(manager.clone());

    // Configure workers
    integration
        .configure_workers(vec!["worker-1", "worker-2", "worker-3"])
        .await?;

    // Create sessions and let load balancer assign
    let mut assignments = vec![];
    for i in 0..9 {
        let mut session = WebSocketSession::new(&format!("lb-{}", i));
        let worker = integration.assign_session_to_worker(session).await?;
        assignments.push(worker);
    }

    // Should be evenly distributed (3 each)
    let worker_1_count = assignments.iter().filter(|w| *w == "worker-1").count();
    let worker_2_count = assignments.iter().filter(|w| *w == "worker-2").count();
    let worker_3_count = assignments.iter().filter(|w| *w == "worker-3").count();

    assert_eq!(worker_1_count, 3);
    assert_eq!(worker_2_count, 3);
    assert_eq!(worker_3_count, 3);

    Ok(())
}
