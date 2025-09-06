use anyhow::Result;
use fabstir_llm_node::api::websocket::{connection::*, session::WebSocketSession};
use tokio::sync::mpsc;
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;

#[tokio::test]
async fn test_connection_handler_creation() -> Result<()> {
    let (tx, mut rx) = mpsc::channel(10);
    
    let handler = ConnectionHandler::new(tx);
    assert_eq!(handler.active_connections().await, 0);
    
    // Should be able to handle messages
    handler.handle_message("test", "hello").await?;
    
    let msg = rx.recv().await;
    assert!(msg.is_some());
    
    Ok(())
}

#[tokio::test]
async fn test_connection_registration() -> Result<()> {
    let handler = ConnectionHandler::new_standalone();
    
    let conn_id = "conn-1";
    let session = WebSocketSession::new("session-1");
    
    handler.register_connection(conn_id, session.clone()).await?;
    
    assert_eq!(handler.active_connections().await, 1);
    assert!(handler.has_connection(conn_id).await);
    
    Ok(())
}

#[tokio::test]
async fn test_connection_removal() -> Result<()> {
    let handler = ConnectionHandler::new_standalone();
    
    let conn_id = "conn-1";
    let session = WebSocketSession::new("session-1");
    
    handler.register_connection(conn_id, session).await?;
    assert_eq!(handler.active_connections().await, 1);
    
    handler.remove_connection(conn_id).await?;
    assert_eq!(handler.active_connections().await, 0);
    
    Ok(())
}

#[tokio::test]
async fn test_message_routing() -> Result<()> {
    let handler = ConnectionHandler::new_standalone();
    
    let conn_id = "conn-1";
    let session = WebSocketSession::new("session-1");
    handler.register_connection(conn_id, session).await?;
    
    // Route message to connection
    let response = handler.route_message(conn_id, "test message").await?;
    assert!(response.contains("processed"));
    
    Ok(())
}

#[tokio::test]
async fn test_broadcast_message() -> Result<()> {
    let handler = ConnectionHandler::new_standalone();
    
    // Register multiple connections
    for i in 0..3 {
        let conn_id = format!("conn-{}", i);
        let session = WebSocketSession::new(format!("session-{}", i));
        handler.register_connection(&conn_id, session).await?;
    }
    
    // Broadcast message
    let results = handler.broadcast("announcement").await?;
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.success));
    
    Ok(())
}

#[tokio::test]
async fn test_connection_health_check() -> Result<()> {
    let handler = ConnectionHandler::new_standalone();
    
    let conn_id = "conn-1";
    let session = WebSocketSession::new("session-1");
    handler.register_connection(conn_id, session).await?;
    
    // Check health
    let healthy = handler.check_connection_health(conn_id).await?;
    assert!(healthy);
    
    // Mark as unhealthy
    handler.mark_unhealthy(conn_id).await?;
    let healthy = handler.check_connection_health(conn_id).await?;
    assert!(!healthy);
    
    Ok(())
}

#[tokio::test]
async fn test_connection_metrics() -> Result<()> {
    let handler = ConnectionHandler::new_standalone();
    
    let conn_id = "conn-1";
    let session = WebSocketSession::new("session-1");
    handler.register_connection(conn_id, session).await?;
    
    // Send some messages
    for _ in 0..5 {
        handler.handle_message(conn_id, "test").await?;
    }
    
    let metrics = handler.get_connection_metrics(conn_id).await?;
    assert_eq!(metrics.messages_received, 5);
    assert!(metrics.bytes_received > 0);
    
    Ok(())
}

#[tokio::test]
async fn test_connection_timeout() -> Result<()> {
    let mut handler = ConnectionHandler::new_standalone();
    handler.set_timeout(Duration::from_millis(100));
    
    let conn_id = "conn-1";
    let session = WebSocketSession::new("session-1");
    handler.register_connection(conn_id, session).await?;
    
    // Wait for timeout
    tokio::time::sleep(Duration::from_millis(150)).await;
    
    // Connection should be marked for cleanup
    handler.cleanup_stale_connections().await?;
    assert_eq!(handler.active_connections().await, 0);
    
    Ok(())
}

#[tokio::test]
async fn test_session_binding() -> Result<()> {
    let handler = ConnectionHandler::new_standalone();
    
    let conn_id = "conn-1";
    let session = WebSocketSession::new("session-1");
    
    handler.register_connection(conn_id, session.clone()).await?;
    
    // Get session for connection
    let retrieved = handler.get_session(conn_id).await?;
    assert_eq!(retrieved.id, session.id);
    
    Ok(())
}

#[tokio::test]
async fn test_connection_state_transitions() -> Result<()> {
    let handler = ConnectionHandler::new_standalone();
    
    let conn_id = "conn-1";
    let session = WebSocketSession::new("session-1");
    handler.register_connection(conn_id, session).await?;
    
    // Check initial state
    let state = handler.get_connection_state(conn_id).await?;
    assert_eq!(state, ConnectionState::Connected);
    
    // Transition to authenticated
    handler.authenticate_connection(conn_id, "user-123").await?;
    let state = handler.get_connection_state(conn_id).await?;
    assert_eq!(state, ConnectionState::Authenticated);
    
    // Disconnect
    handler.disconnect(conn_id).await?;
    let state = handler.get_connection_state(conn_id).await?;
    assert_eq!(state, ConnectionState::Disconnected);
    
    Ok(())
}

#[tokio::test]
async fn test_concurrent_message_handling() -> Result<()> {
    let handler = ConnectionHandler::new_standalone();
    
    let conn_id = "conn-1";
    let session = WebSocketSession::new("session-1");
    handler.register_connection(conn_id, session).await?;
    
    let mut handles = vec![];
    
    // Send concurrent messages
    for i in 0..10 {
        let h = handler.clone();
        let cid = conn_id.to_string();
        let handle = tokio::spawn(async move {
            h.handle_message(&cid, &format!("message {}", i)).await
        });
        handles.push(handle);
    }
    
    // All should succeed
    for handle in handles {
        let result = handle.await?;
        assert!(result.is_ok());
    }
    
    Ok(())
}

#[tokio::test]
async fn test_connection_cleanup_callbacks() -> Result<()> {
    let handler = ConnectionHandler::new_standalone();
    
    let conn_id = "conn-1";
    let session = WebSocketSession::new("session-1");
    
    // Register with cleanup callback
    handler.register_connection_with_callback(
        conn_id,
        session,
        Box::new(|id| {
            println!("Cleaning up connection: {}", id);
        })
    ).await?;
    
    // Remove should trigger callback
    handler.remove_connection(conn_id).await?;
    
    // Verify cleanup was called (check logs or state)
    assert_eq!(handler.active_connections().await, 0);
    
    Ok(())
}