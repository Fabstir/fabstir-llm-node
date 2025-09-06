use super::protocol::{ProtocolMessage, MessageType, ProtocolError};
use anyhow::Result;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde_json::json;

pub type HandlerFn = Box<dyn Fn(ProtocolMessage) -> Pin<Box<dyn Future<Output = Result<ProtocolMessage>> + Send>> + Send + Sync>;
pub type MiddlewareFn = Box<dyn Fn(ProtocolMessage) -> Pin<Box<dyn Future<Output = Result<ProtocolMessage>> + Send>> + Send + Sync>;

pub struct HandlerRegistry {
    handlers: Arc<RwLock<HashMap<MessageType, HandlerFn>>>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            handlers: Arc::new(RwLock::new(HashMap::new())),
        };
        
        // Register default handlers
        registry.register_defaults();
        registry
    }

    fn register_defaults(&mut self) {
        // Default heartbeat handler
        self.register(MessageType::Heartbeat, |msg| {
            Box::pin(async move {
                Ok(ProtocolMessage {
                    msg_type: MessageType::HeartbeatAck,
                    command: None,
                    session_id: msg.session_id,
                    metadata: Some(json!({
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    })),
                    payload: None,
                })
            })
        });
    }

    pub fn register<F>(&mut self, msg_type: MessageType, handler: F)
    where
        F: Fn(ProtocolMessage) -> Pin<Box<dyn Future<Output = Result<ProtocolMessage>> + Send>> + Send + Sync + 'static,
    {
        let mut handlers = futures::executor::block_on(self.handlers.write());
        handlers.insert(msg_type, Box::new(handler));
    }

    pub async fn handle(&self, msg: ProtocolMessage) -> Result<ProtocolMessage> {
        let handlers = self.handlers.read().await;
        
        if let Some(handler) = handlers.get(&msg.msg_type) {
            handler(msg).await
        } else {
            // For Data messages without handlers, pass through as-is (for middleware testing)
            if msg.msg_type == MessageType::Data {
                Ok(msg)
            } else {
                // Default error response for other unhandled message types
                Ok(ProtocolMessage {
                    msg_type: MessageType::Error,
                    command: None,
                    session_id: msg.session_id,
                    metadata: Some(json!({
                        "error": "No handler registered for message type",
                        "msg_type": format!("{:?}", msg.msg_type),
                    })),
                    payload: None,
                })
            }
        }
    }
}

pub struct MessageHandler {
    middleware: Vec<MiddlewareFn>,
    registry: HandlerRegistry,
}

impl MessageHandler {
    pub fn new() -> Self {
        Self {
            middleware: Vec::new(),
            registry: HandlerRegistry::new(),
        }
    }

    pub fn add_middleware<F>(&mut self, middleware: F)
    where
        F: Fn(ProtocolMessage) -> Pin<Box<dyn Future<Output = Result<ProtocolMessage>> + Send>> + Send + Sync + 'static,
    {
        self.middleware.push(Box::new(middleware));
    }

    pub async fn process(&self, mut msg: ProtocolMessage) -> Result<ProtocolMessage> {
        // Apply middleware in order
        for middleware in &self.middleware {
            msg = middleware(msg).await?;
        }
        
        // Process with handler
        self.registry.handle(msg).await
    }

    pub async fn process_batch(&self, messages: Vec<ProtocolMessage>) -> Result<Vec<ProtocolMessage>> {
        let mut results = Vec::new();
        
        for msg in messages {
            let result = self.process(msg).await?;
            results.push(result);
        }
        
        Ok(results)
    }
}

// Extension trait for WebSocketSession to support metadata
use super::session::WebSocketSession;
use serde_json::Value;

pub trait SessionMetadataExt {
    fn get_metadata(&self) -> HashMap<String, Value>;
    fn set_metadata(&mut self, metadata: Value);
}

impl SessionMetadataExt for WebSocketSession {
    fn get_metadata(&self) -> HashMap<String, Value> {
        // In a real implementation, WebSocketSession would have a metadata field
        // For now, return empty HashMap
        HashMap::new()
    }

    fn set_metadata(&mut self, _metadata: Value) {
        // In a real implementation, this would store metadata in the session
    }
}

// Helper functions for creating common protocol messages
pub struct ProtocolHelpers;

impl ProtocolHelpers {
    pub fn create_error(error: ProtocolError) -> ProtocolMessage {
        error.to_protocol_message()
    }

    pub fn create_heartbeat_ack(session_id: Option<String>) -> ProtocolMessage {
        ProtocolMessage {
            msg_type: MessageType::HeartbeatAck,
            command: None,
            session_id,
            metadata: Some(json!({
                "timestamp": chrono::Utc::now().to_rfc3339(),
            })),
            payload: None,
        }
    }

    pub fn create_session_init_ack(session_id: String) -> ProtocolMessage {
        use super::protocol::SessionCommand;
        ProtocolMessage {
            msg_type: MessageType::SessionControl,
            command: Some(SessionCommand::InitAck),
            session_id: Some(session_id),
            metadata: Some(json!({
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "server_version": "1.0.0",
            })),
            payload: None,
        }
    }

    pub fn create_capabilities_response(
        server_caps: Vec<String>,
        negotiated_caps: Vec<String>,
    ) -> ProtocolMessage {
        ProtocolMessage {
            msg_type: MessageType::CapabilitiesAck,
            command: None,
            session_id: None,
            metadata: Some(json!({
                "server_capabilities": server_caps,
                "negotiated_capabilities": negotiated_caps,
            })),
            payload: None,
        }
    }
}

// Session state serialization helpers
pub struct SessionSerializer;

impl SessionSerializer {
    pub fn serialize_session(session: &WebSocketSession) -> Result<Value> {
        Ok(json!({
            "id": session.id(),
            "message_count": session.message_count(),
            "created_at": session.created_at_iso(),
            "last_activity": session.last_activity_iso(),
            "memory_used": session.memory_used(),
            "conversation_history": session.conversation_history()
                .iter()
                .map(|msg| json!({
                    "role": msg.role,
                    "content": msg.content,
                    "timestamp": msg.timestamp,
                }))
                .collect::<Vec<_>>(),
        }))
    }

    pub fn deserialize_session(data: &Value) -> Result<WebSocketSession> {
        use super::session::SessionConfig;
        use crate::job_processor::Message;
        
        let id = data["id"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing session id"))?;
        
        let mut session = WebSocketSession::new(
            id.to_string(),
            SessionConfig::default(),
        );
        
        // Restore conversation history
        if let Some(history) = data["conversation_history"].as_array() {
            for msg_data in history {
                let message = Message {
                    role: msg_data["role"].as_str().unwrap_or("user").to_string(),
                    content: msg_data["content"].as_str().unwrap_or("").to_string(),
                    timestamp: msg_data["timestamp"].as_i64(),
                };
                session.add_message(message)?;
            }
        }
        
        Ok(session)
    }
}

// Rate limiting middleware
pub struct RateLimiter {
    limits: Arc<RwLock<HashMap<String, (u32, std::time::Instant)>>>,
    max_requests_per_minute: u32,
}

impl RateLimiter {
    pub fn new(max_requests_per_minute: u32) -> Self {
        Self {
            limits: Arc::new(RwLock::new(HashMap::new())),
            max_requests_per_minute,
        }
    }

    pub async fn check_rate_limit(&self, session_id: &str) -> Result<()> {
        let mut limits = self.limits.write().await;
        let now = std::time::Instant::now();
        
        if let Some((count, last_reset)) = limits.get_mut(session_id) {
            if now.duration_since(*last_reset).as_secs() >= 60 {
                // Reset counter after 1 minute
                *count = 1;
                *last_reset = now;
            } else {
                *count += 1;
                if *count > self.max_requests_per_minute {
                    return Err(anyhow::anyhow!("Rate limit exceeded"));
                }
            }
        } else {
            limits.insert(session_id.to_string(), (1, now));
        }
        
        Ok(())
    }
}