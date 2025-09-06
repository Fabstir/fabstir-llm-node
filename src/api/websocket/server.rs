use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock, Mutex};
use tokio_tungstenite::{accept_async, WebSocketStream, tungstenite::Message};
use futures::{StreamExt, SinkExt};
use tracing::{info, warn, error, debug};
use std::collections::HashMap;

pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub max_connections: usize,
    pub heartbeat_interval: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 9000,
            max_connections: 100,
            heartbeat_interval: Duration::from_secs(30),
        }
    }
}

pub struct WebSocketServer {
    config: ServerConfig,
    connections: Arc<RwLock<HashMap<String, Arc<WebSocketConnection>>>>,
    shutdown_tx: Arc<Mutex<Option<mpsc::Sender<()>>>>,
}

struct WebSocketConnection {
    id: String,
    session_id: Arc<RwLock<Option<String>>>,
}

impl WebSocketServer {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
            connections: Arc::new(RwLock::new(HashMap::new())),
            shutdown_tx: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start(&self) -> Result<ServerHandle> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = TcpListener::bind(&addr).await?;
        info!("WebSocket server listening on {}", addr);

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        *self.shutdown_tx.lock().await = Some(shutdown_tx.clone());

        let connections = self.connections.clone();
        let max_connections = self.config.max_connections;
        let heartbeat_interval = self.config.heartbeat_interval;

        // Spawn the accept loop
        let accept_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((stream, addr)) => {
                                let conn_count = connections.read().await.len();
                                if conn_count >= max_connections {
                                    warn!("Connection limit reached, rejecting {}", addr);
                                    continue;
                                }
                                
                                let connections = connections.clone();
                                tokio::spawn(handle_connection(stream, addr, connections));
                            }
                            Err(e) => {
                                error!("Failed to accept connection: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Shutting down WebSocket server");
                        break;
                    }
                }
            }
        });

        // Start heartbeat task
        let connections_hb = self.connections.clone();
        let (heartbeat_shutdown_tx, mut heartbeat_shutdown_rx) = mpsc::channel::<()>(1);
        let heartbeat_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(heartbeat_interval);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        send_heartbeats(connections_hb.clone()).await;
                    }
                    _ = heartbeat_shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        });

        Ok(ServerHandle {
            address: addr,
            connections: self.connections.clone(),
            shutdown_tx: shutdown_tx,
            heartbeat_shutdown_tx: Some(heartbeat_shutdown_tx),
            accept_handle: Some(accept_handle),
            heartbeat_handle: Some(heartbeat_handle),
        })
    }

    pub fn address(&self) -> String {
        format!("{}:{}", self.config.host, self.config.port)
    }
}

pub struct ServerHandle {
    address: String,
    connections: Arc<RwLock<HashMap<String, Arc<WebSocketConnection>>>>,
    shutdown_tx: mpsc::Sender<()>,
    heartbeat_shutdown_tx: Option<mpsc::Sender<()>>,
    accept_handle: Option<tokio::task::JoinHandle<()>>,
    heartbeat_handle: Option<tokio::task::JoinHandle<()>>,
}

impl ServerHandle {
    pub async fn shutdown(mut self) -> Result<()> {
        // Send shutdown signals
        self.shutdown_tx.send(()).await.ok();
        if let Some(tx) = self.heartbeat_shutdown_tx.take() {
            tx.send(()).await.ok();
        }

        // Close all connections
        let connections = self.connections.read().await;
        for (id, _conn) in connections.iter() {
            debug!("Closing connection {}", id);
            // Connections will be closed when dropped
        }
        drop(connections);

        // Wait for tasks to finish
        if let Some(handle) = self.accept_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.heartbeat_handle.take() {
            // Give it a moment to exit cleanly, then abort if needed
            tokio::time::timeout(Duration::from_millis(100), handle).await.ok();
        }

        self.connections.write().await.clear();
        info!("WebSocket server shutdown complete");
        Ok(())
    }

    pub async fn connection_count(&self) -> usize {
        self.connections.read().await.len()
    }
}

async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    connections: Arc<RwLock<HashMap<String, Arc<WebSocketConnection>>>>,
) {
    debug!("New connection from {}", addr);

    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            error!("WebSocket handshake failed for {}: {}", addr, e);
            return;
        }
    };

    let conn_id = format!("conn-{}", uuid::Uuid::new_v4());
    let connection = Arc::new(WebSocketConnection {
        id: conn_id.clone(),
        session_id: Arc::new(RwLock::new(None)),
    });

    connections.write().await.insert(conn_id.clone(), connection.clone());
    info!("Connection {} established from {}", conn_id, addr);

    // Handle messages
    let (mut tx, mut rx) = ws_stream.split();
    while let Some(msg) = rx.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                debug!("Received text from {}: {}", conn_id, text);
                
                // Parse and handle message
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(msg_type) = parsed.get("type").and_then(|v| v.as_str()) {
                        match msg_type {
                            "session_init" => {
                                if let Some(session_id) = parsed.get("session_id").and_then(|v| v.as_str()) {
                                    *connection.session_id.write().await = Some(session_id.to_string());
                                    let response = serde_json::json!({
                                        "type": "session_established",
                                        "session_id": session_id
                                    });
                                    tx.send(Message::Text(response.to_string())).await.ok();
                                }
                            }
                            "session_resume" => {
                                if let Some(session_id) = parsed.get("session_id").and_then(|v| v.as_str()) {
                                    *connection.session_id.write().await = Some(session_id.to_string());
                                    let response = serde_json::json!({
                                        "type": "session_resumed",
                                        "session_id": session_id
                                    });
                                    tx.send(Message::Text(response.to_string())).await.ok();
                                }
                            }
                            _ => {
                                // Echo for now
                                let response = serde_json::json!({
                                    "type": "response",
                                    "data": text,
                                    "processed": true
                                });
                                tx.send(Message::Text(response.to_string())).await.ok();
                            }
                        }
                    }
                } else {
                    // Simple echo for non-JSON messages
                    tx.send(Message::Text(format!("Echo: {}", text))).await.ok();
                }
            }
            Ok(Message::Binary(data)) => {
                debug!("Received binary data from {}: {} bytes", conn_id, data.len());
                // Handle binary data
                tx.send(Message::Binary(data)).await.ok();
            }
            Ok(Message::Ping(data)) => {
                tx.send(Message::Pong(data)).await.ok();
            }
            Ok(Message::Close(_)) => {
                debug!("Connection {} closing", conn_id);
                break;
            }
            Err(e) => {
                error!("Error receiving message from {}: {}", conn_id, e);
                break;
            }
            _ => {}
        }
    }

    connections.write().await.remove(&conn_id);
    info!("Connection {} closed", conn_id);
}

async fn send_heartbeats(_connections: Arc<RwLock<HashMap<String, Arc<WebSocketConnection>>>>) {
    // In a production implementation, you would maintain a separate channel
    // to each connection for sending heartbeats
    debug!("Heartbeat tick");
}

// Extension trait for WebSocketStream
pub trait WebSocketStreamExt {
    fn is_active(&self) -> bool;
}

impl<S> WebSocketStreamExt for WebSocketStream<S> {
    fn is_active(&self) -> bool {
        // WebSocketStream doesn't directly expose connection state,
        // so we return true as a placeholder. In production, you'd
        // track this state separately
        true
    }
}