use super::{
    message_types::{
        ConnectionMode, InferenceMessage, MessageType, SessionControl, SessionControlMessage,
        WebSocketMessage,
    },
    session::SessionConfig,
    session_store::SessionStore,
};
use crate::job_processor::Message;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandlerConfig {
    pub auto_create_session: bool,
    pub enable_stateless_fallback: bool,
    pub session_timeout_seconds: u64,
    pub max_context_messages: usize,
}

impl Default for HandlerConfig {
    fn default() -> Self {
        Self {
            auto_create_session: true,
            enable_stateless_fallback: false,
            session_timeout_seconds: 1800,
            max_context_messages: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandlerMetrics {
    pub active_sessions: usize,
    pub total_messages_processed: usize,
    pub total_memory_bytes: usize,
}

pub struct WebSocketHandler {
    store: Arc<RwLock<SessionStore>>,
    config: HandlerConfig,
    messages_processed: Arc<RwLock<usize>>,
}

impl WebSocketHandler {
    pub fn new(store: Arc<RwLock<SessionStore>>, config: HandlerConfig) -> Self {
        Self {
            store,
            config,
            messages_processed: Arc::new(RwLock::new(0)),
        }
    }

    pub fn active_sessions(&self) -> usize {
        // For tests, we'll use a blocking approach
        futures::executor::block_on(async {
            let store = self.store.read().await;
            store.async_session_count().await
        })
    }

    pub async fn on_connect(&mut self) -> Result<String> {
        let session_config = SessionConfig {
            timeout_seconds: self.config.session_timeout_seconds,
            context_window_size: self.config.max_context_messages,
            ..Default::default()
        };

        let mut store = self.store.write().await;
        let session_id = store.create_session(session_config).await;
        Ok(session_id)
    }

    pub async fn on_connect_with_fallback(&mut self) -> Result<ConnectionMode> {
        let session_config = SessionConfig {
            timeout_seconds: self.config.session_timeout_seconds,
            context_window_size: self.config.max_context_messages,
            ..Default::default()
        };

        let mut store = self.store.write().await;
        match store.try_create_session(session_config).await {
            Ok(session_id) => Ok(ConnectionMode {
                mode: "stateful".to_string(),
                session_id: Some(session_id),
                reason: None,
            }),
            Err(_) if self.config.enable_stateless_fallback => Ok(ConnectionMode {
                mode: "stateless".to_string(),
                session_id: None,
                reason: Some("Session limit reached, falling back to stateless".to_string()),
            }),
            Err(e) => Err(e),
        }
    }

    pub async fn on_disconnect(&mut self, session_id: &str) -> Result<()> {
        let mut store = self.store.write().await;
        if store.destroy_session(session_id).await {
            Ok(())
        } else {
            Err(anyhow!("Session not found"))
        }
    }

    pub async fn has_session(&self, session_id: &str) -> bool {
        let store = self.store.read().await;
        store.session_exists(session_id).await
    }

    pub async fn handle_message(&mut self, message: WebSocketMessage) -> Result<WebSocketMessage> {
        // Increment message counter
        {
            let mut count = self.messages_processed.write().await;
            *count += 1;
        }

        match message.msg_type {
            MessageType::Init => self.handle_init(message).await,
            MessageType::Inference => self.handle_inference(message).await,
            MessageType::StatelessInference => self.handle_stateless_inference(message).await,
            MessageType::SessionControl => self.handle_session_control(message).await,
            MessageType::Ping => Ok(WebSocketMessage::new(MessageType::Pong, message.payload)),
            MessageType::Close => self.handle_close(message).await,
            MessageType::Unknown => Err(anyhow!(
                "UNKNOWN_MESSAGE_TYPE: Cannot handle unknown message type"
            )),
            _ => Err(anyhow!("Unsupported message type: {:?}", message.msg_type)),
        }
    }

    async fn handle_init(&mut self, message: WebSocketMessage) -> Result<WebSocketMessage> {
        // Check if trying to resume a session
        if let Some(session_id) = &message.session_id {
            if let Some(action) = message.payload.get("action") {
                if action == "resume" {
                    // We don't support persistence yet
                    return Err(anyhow!("Session persistence not implemented"));
                }
            }

            // Check if session exists
            if !self.has_session(session_id).await {
                return Err(anyhow!(
                    "SESSION_NOT_FOUND: Session {} not found",
                    session_id
                ));
            }
        }

        // Create new session
        let session_id = self.on_connect().await?;
        Ok(WebSocketMessage::new(
            MessageType::Init,
            serde_json::json!({
                "session_id": session_id,
                "mode": "stateful"
            }),
        )
        .with_session(session_id))
    }

    async fn handle_inference(
        &mut self,
        mut message: WebSocketMessage,
    ) -> Result<WebSocketMessage> {
        // Get or create session
        let session_id = match message.session_id {
            Some(ref id) => {
                // Check if session exists
                if !self.has_session(id).await {
                    return Err(anyhow!("SESSION_NOT_FOUND: Session {} not found", id));
                }

                // Check if session is expired
                let store = self.store.read().await;
                if let Some(session) = store.get_session(id).await {
                    if session.is_expired() {
                        return Err(anyhow!("Session {} has expired", id));
                    }
                }

                id.clone()
            }
            None if self.config.auto_create_session => {
                // Auto-create session
                let id = self.on_connect().await?;
                message.session_id = Some(id.clone());
                id
            }
            None => {
                return Err(anyhow!(
                    "No session ID provided and auto-create is disabled"
                ));
            }
        };

        // Parse inference message
        let inference = InferenceMessage::from_payload(&message.payload).map_err(|e| anyhow!(e))?;

        // Check for required fields
        if inference.prompt.is_empty() {
            return Err(anyhow!("Missing required field: prompt"));
        }

        // Add user message to session
        self.add_message_to_session(
            &session_id,
            Message {
                role: "user".to_string(),
                content: inference.prompt.clone(),
                timestamp: None,
            },
        )
        .await?;

        // Build context from session
        let context = self
            .build_context_for_session(&session_id, &inference.prompt)
            .await?;

        // TODO: Actually call inference engine here
        // For now, return a mock response
        let response_content = format!("Mock response to: {}", inference.prompt);

        // Add assistant response to session
        self.add_message_to_session(
            &session_id,
            Message {
                role: "assistant".to_string(),
                content: response_content.clone(),
                timestamp: None,
            },
        )
        .await?;

        Ok(WebSocketMessage::inference_response(
            Some(session_id),
            response_content,
        ))
    }

    async fn handle_stateless_inference(
        &mut self,
        message: WebSocketMessage,
    ) -> Result<WebSocketMessage> {
        // Check if trying to use stateless with an active session
        if message.session_id.is_some() {
            return Err(anyhow!("Cannot switch mode mid-session"));
        }

        // Parse inference message
        let inference = InferenceMessage::from_payload(&message.payload).map_err(|e| anyhow!(e))?;

        // TODO: Actually call inference engine with provided context
        // For now, return a mock response
        let response_content = format!("Stateless response to: {}", inference.prompt);

        Ok(WebSocketMessage::inference_response(None, response_content))
    }

    async fn handle_session_control(
        &mut self,
        message: WebSocketMessage,
    ) -> Result<WebSocketMessage> {
        let session_id = message
            .session_id
            .ok_or_else(|| anyhow!("Session ID required for session control"))?;

        if !self.has_session(&session_id).await {
            return Err(anyhow!(
                "SESSION_NOT_FOUND: Session {} not found",
                session_id
            ));
        }

        let control =
            SessionControlMessage::from_payload(&message.payload).map_err(|e| anyhow!(e))?;

        match control.action {
            SessionControl::Clear => {
                // Clear session messages
                let mut store = self.store.write().await;
                store.clear_session(&session_id).await?;
                drop(store);

                Ok(WebSocketMessage::new(
                    MessageType::SessionControlAck,
                    serde_json::json!({
                        "action": "clear",
                        "success": true
                    }),
                )
                .with_session(session_id))
            }
            SessionControl::Resume => Err(anyhow!("Session persistence not implemented")),
            SessionControl::Status => {
                let store = self.store.read().await;
                if let Some(session) = store.get_session(&session_id).await {
                    let metrics = session.metrics();
                    Ok(WebSocketMessage::new(
                        MessageType::SessionControlAck,
                        serde_json::json!({
                            "action": "status",
                            "messages": metrics.total_messages,
                            "memory_bytes": metrics.memory_bytes
                        }),
                    )
                    .with_session(session_id))
                } else {
                    Err(anyhow!("Session not found"))
                }
            }
        }
    }

    async fn handle_close(&mut self, message: WebSocketMessage) -> Result<WebSocketMessage> {
        if let Some(session_id) = &message.session_id {
            self.on_disconnect(session_id).await?;
        }
        Ok(WebSocketMessage::new(
            MessageType::Close,
            serde_json::json!({}),
        ))
    }

    pub async fn add_message_to_session(
        &mut self,
        session_id: &str,
        message: Message,
    ) -> Result<()> {
        let mut store = self.store.write().await;
        store.update_session(session_id, message).await
    }

    pub async fn build_context_for_session(
        &self,
        session_id: &str,
        current_prompt: &str,
    ) -> Result<String> {
        let store = self.store.read().await;
        let session = store
            .get_session(session_id)
            .await
            .ok_or_else(|| anyhow!("Session not found"))?;

        let context_messages = session.get_context_messages();

        let mut context = String::new();
        for msg in context_messages {
            context.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }
        context.push_str(&format!("user: {}\nassistant:", current_prompt));

        Ok(context)
    }

    pub async fn get_session_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let store = self.store.read().await;
        let session = store
            .get_session(session_id)
            .await
            .ok_or_else(|| anyhow!("Session not found"))?;
        Ok(session.conversation_history().to_vec())
    }

    pub fn can_handle_message_type(&self, msg_type: &MessageType) -> bool {
        matches!(
            msg_type,
            MessageType::Init
                | MessageType::Inference
                | MessageType::StatelessInference
                | MessageType::SessionControl
                | MessageType::Ping
                | MessageType::Close
        )
    }

    pub async fn get_metrics(&self) -> HandlerMetrics {
        let store = self.store.read().await;
        let store_metrics = store.get_store_metrics().await;
        let messages_processed = *self.messages_processed.read().await;

        HandlerMetrics {
            active_sessions: store_metrics.active_sessions,
            total_messages_processed: messages_processed,
            total_memory_bytes: store_metrics.total_memory_bytes,
        }
    }
}
