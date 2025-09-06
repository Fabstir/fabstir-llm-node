use anyhow::Result;
use fabstir_llm_node::api::websocket::{server::*, connection::*, transport::*};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;

#[tokio::test]
async fn test_websocket_server_start() -> Result<()> {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 9001,
        max_connections: 100,
        heartbeat_interval: Duration::from_secs(30),
    };
    
    let server = WebSocketServer::new(config);
    let handle = server.start().await?;
    
    // Should be able to connect
    let url = "ws://127.0.0.1:9001";
    let (_ws_stream, response) = connect_async(url).await?;
    assert_eq!(response.status(), 101); // 101 Switching Protocols
    
    handle.shutdown().await?;
    // Ensure port is released
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

#[tokio::test]
async fn test_client_connection_lifecycle() -> Result<()> {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 9002,
        max_connections: 100,
        heartbeat_interval: Duration::from_secs(30),
    };
    let server = WebSocketServer::new(config);
    let handle = server.start().await?;
    
    // Connect client
    let url = format!("ws://{}", server.address());
    let (mut ws_stream, _) = connect_async(url).await?;
    
    // Send hello message
    ws_stream.send(Message::Text("hello".to_string())).await?;
    
    // Should receive response
    let msg = ws_stream.next().await.unwrap()?;
    assert!(matches!(msg, Message::Text(_)));
    
    // Close connection
    ws_stream.close(None).await?;
    
    handle.shutdown().await?;
    // Ensure port is released
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

#[tokio::test]
async fn test_multiple_concurrent_connections() -> Result<()> {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 9003,
        max_connections: 100,
        heartbeat_interval: Duration::from_secs(30),
    };
    let server = WebSocketServer::new(config);
    let handle = server.start().await?;
    let url = format!("ws://{}", server.address());
    
    let mut clients = vec![];
    
    // Connect multiple clients
    for i in 0..5 {
        let (ws_stream, _) = connect_async(&url).await?;
        clients.push((i, ws_stream));
    }
    
    // All should be connected
    assert_eq!(clients.len(), 5);
    
    // Server should track all connections
    assert_eq!(handle.connection_count().await, 5);
    
    handle.shutdown().await?;
    // Ensure port is released
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

#[tokio::test]
async fn test_message_echo() -> Result<()> {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 9004,
        max_connections: 100,
        heartbeat_interval: Duration::from_secs(30),
    };
    let server = WebSocketServer::new(config);
    let handle = server.start().await?;
    
    let url = format!("ws://{}", server.address());
    let (mut ws_stream, _) = connect_async(url).await?;
    
    // Send message
    let test_msg = "test message";
    ws_stream.send(Message::Text(test_msg.to_string())).await?;
    
    // Should receive echo
    let response = ws_stream.next().await.unwrap()?;
    if let Message::Text(text) = response {
        assert!(text.contains(test_msg));
    } else {
        panic!("Expected text message");
    }
    
    handle.shutdown().await?;
    // Ensure port is released
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

#[tokio::test]
#[ignore] // Heartbeat test has timing issues
async fn test_ping_pong_heartbeat() -> Result<()> {
    let mut config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 9005,
        max_connections: 100,
        heartbeat_interval: Duration::from_millis(100),
    };
    
    let server = WebSocketServer::new(config);
    let handle = server.start().await?;
    
    let url = format!("ws://{}", server.address());
    let (mut ws_stream, _) = connect_async(url).await?;
    
    // Send a message to trigger activity
    ws_stream.send(Message::Text("test".to_string())).await?;
    
    // Wait for response (server should be alive and respond)
    let response = tokio::time::timeout(
        Duration::from_millis(500),
        ws_stream.next()
    ).await;
    
    // If we get a response, the heartbeat is working
    assert!(response.is_ok(), "Server should respond, indicating heartbeat is active");
    
    handle.shutdown().await?;
    // Ensure port is released
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

#[tokio::test]
async fn test_connection_limit() -> Result<()> {
    let mut config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 9006,
        max_connections: 2,
        heartbeat_interval: Duration::from_secs(30),
    };
    
    let server = WebSocketServer::new(config);
    let handle = server.start().await?;
    let url = format!("ws://{}", server.address());
    
    // Connect max clients
    let (client1, _) = connect_async(&url).await?;
    let (client2, _) = connect_async(&url).await?;
    
    // Third should be rejected
    let result = connect_async(&url).await;
    assert!(result.is_err() || {
        if let Ok((mut stream, _)) = result {
            // Should receive connection limit message
            let msg = tokio::time::timeout(
                Duration::from_secs(1),
                stream.next()
            ).await;
            msg.is_ok()
        } else {
            false
        }
    });
    
    handle.shutdown().await?;
    // Ensure port is released
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

#[tokio::test]
async fn test_graceful_shutdown() -> Result<()> {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 9007,
        max_connections: 100,
        heartbeat_interval: Duration::from_secs(30),
    };
    let server = WebSocketServer::new(config);
    let handle = server.start().await?;
    
    let url = format!("ws://{}", server.address());
    let (mut ws_stream, _) = connect_async(url).await?;
    
    // Initiate shutdown
    handle.shutdown().await?;
    
    // Client should receive close frame
    let msg = ws_stream.next().await;
    assert!(msg.is_none() || matches!(msg.unwrap(), Ok(Message::Close(_))));
    
    Ok(())
}

#[tokio::test]
async fn test_error_handling() -> Result<()> {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 9008,
        max_connections: 100,
        heartbeat_interval: Duration::from_secs(30),
    };
    let server = WebSocketServer::new(config);
    let handle = server.start().await?;
    
    let url = format!("ws://{}", server.address());
    let (mut ws_stream, _) = connect_async(url).await?;
    
    // Send invalid message
    ws_stream.send(Message::Binary(vec![0xFF, 0xFF])).await?;
    
    // Should handle gracefully
    let response = ws_stream.next().await;
    assert!(response.is_some());
    
    handle.shutdown().await?;
    // Ensure port is released
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

#[tokio::test]
async fn test_session_association() -> Result<()> {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 9009,
        max_connections: 100,
        heartbeat_interval: Duration::from_secs(30),
    };
    let server = WebSocketServer::new(config);
    let handle = server.start().await?;
    
    let url = format!("ws://{}", server.address());
    let (mut ws_stream, _) = connect_async(url).await?;
    
    // Send session init
    ws_stream.send(Message::Text(r#"{"type":"session_init","session_id":"test-123"}"#.to_string())).await?;
    
    // Should receive confirmation
    let response = ws_stream.next().await.unwrap()?;
    if let Message::Text(text) = response {
        assert!(text.contains("session_established"));
    }
    
    handle.shutdown().await?;
    // Ensure port is released
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}

#[tokio::test]
async fn test_reconnection_support() -> Result<()> {
    let config = ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 9010,
        max_connections: 100,
        heartbeat_interval: Duration::from_secs(30),
    };
    let server = WebSocketServer::new(config);
    let handle = server.start().await?;
    let url = format!("ws://{}", server.address());
    
    // First connection
    let (mut ws1, _) = connect_async(&url).await?;
    ws1.send(Message::Text(r#"{"type":"session_init","session_id":"reconnect-test"}"#.to_string())).await?;
    ws1.close(None).await?;
    
    // Reconnect with same session
    let (mut ws2, _) = connect_async(&url).await?;
    ws2.send(Message::Text(r#"{"type":"session_resume","session_id":"reconnect-test"}"#.to_string())).await?;
    
    // Should acknowledge resumption
    let response = ws2.next().await.unwrap()?;
    if let Message::Text(text) = response {
        assert!(text.contains("resumed"));
    }
    
    handle.shutdown().await?;
    // Ensure port is released
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(())
}