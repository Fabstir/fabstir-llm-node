use super::session::{SessionConfig, WebSocketSession};
use super::session_store::SessionStore;
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    SessionControl,
    Heartbeat,
    HeartbeatAck,
    Metadata,
    MetadataAck,
    StateSync,
    StateSyncAck,
    Capabilities,
    CapabilitiesAck,
    Version,
    VersionAck,
    Error,
    Data,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionCommand {
    Init,
    InitAck,
    Resume,
    ResumeAck,
    Clear,
    ClearAck,
    Handoff,
    HandoffReady,
    Terminate,
    TerminateAck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessage {
    pub msg_type: MessageType,
    pub command: Option<SessionCommand>,
    pub session_id: Option<String>,
    pub metadata: Option<Value>,
    pub payload: Option<Value>,
}

impl ProtocolMessage {
    pub fn builder() -> ProtocolMessageBuilder {
        ProtocolMessageBuilder::new()
    }

    pub fn validate(&self) -> Result<()> {
        match &self.msg_type {
            MessageType::SessionControl => {
                if self.command.is_none() {
                    return Err(anyhow!("SessionControl message requires a command"));
                }

                // Validate specific commands
                if let Some(command) = &self.command {
                    match command {
                        SessionCommand::Resume
                        | SessionCommand::Clear
                        | SessionCommand::Handoff
                        | SessionCommand::Terminate => {
                            if self.session_id.is_none() {
                                return Err(anyhow!("{:?} command requires session_id", command));
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub struct ProtocolMessageBuilder {
    msg_type: Option<MessageType>,
    command: Option<SessionCommand>,
    session_id: Option<String>,
    metadata: Option<Value>,
    payload: Option<Value>,
}

impl ProtocolMessageBuilder {
    pub fn new() -> Self {
        Self {
            msg_type: None,
            command: None,
            session_id: None,
            metadata: None,
            payload: None,
        }
    }

    pub fn msg_type(mut self, msg_type: MessageType) -> Self {
        self.msg_type = Some(msg_type);
        self
    }

    pub fn command(mut self, command: SessionCommand) -> Self {
        self.command = Some(command);
        self
    }

    pub fn session_id(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }

    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn payload(mut self, payload: Value) -> Self {
        self.payload = Some(payload);
        self
    }

    pub fn build(self) -> ProtocolMessage {
        ProtocolMessage {
            msg_type: self.msg_type.unwrap_or(MessageType::Data),
            command: self.command,
            session_id: self.session_id,
            metadata: self.metadata,
            payload: self.payload,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ProtocolError {
    SessionNotFound(String),
    InvalidMessage(String),
    UnsupportedVersion(u32, u32),
    CapabilityMismatch(Vec<String>),
    HandoffFailed(String),
    Timeout(u64),
}

impl ProtocolError {
    pub fn to_protocol_message(&self) -> ProtocolMessage {
        let (error_code, error_message) = match self {
            ProtocolError::SessionNotFound(id) => {
                ("SESSION_NOT_FOUND", format!("Session {} not found", id))
            }
            ProtocolError::InvalidMessage(msg) => ("INVALID_MESSAGE", msg.clone()),
            ProtocolError::UnsupportedVersion(client, server) => (
                "UNSUPPORTED_VERSION",
                format!(
                    "Client version {} not supported by server version {}",
                    client, server
                ),
            ),
            ProtocolError::CapabilityMismatch(caps) => (
                "CAPABILITY_MISMATCH",
                format!("Unsupported capabilities: {:?}", caps),
            ),
            ProtocolError::HandoffFailed(reason) => ("HANDOFF_FAILED", reason.clone()),
            ProtocolError::Timeout(ms) => {
                ("TIMEOUT", format!("Operation timed out after {}ms", ms))
            }
        };

        ProtocolMessage {
            msg_type: MessageType::Error,
            command: None,
            session_id: None,
            metadata: Some(serde_json::json!({
                "error_code": error_code,
                "error_message": error_message,
                "timestamp": Utc::now().to_rfc3339(),
            })),
            payload: None,
        }
    }
}

#[derive(Clone)]
pub struct SessionProtocol {
    sessions: Arc<RwLock<SessionStore>>,
    capabilities: Arc<RwLock<Vec<String>>>,
    version: String,
}

impl SessionProtocol {
    pub fn new() -> Self {
        let store_config = super::session_store::SessionStoreConfig::default();
        Self {
            sessions: Arc::new(RwLock::new(SessionStore::new(store_config))),
            capabilities: Arc::new(RwLock::new(vec![
                "streaming".to_string(),
                "context_management".to_string(),
                "compression".to_string(),
                "batching".to_string(),
            ])),
            version: "1.0.0".to_string(),
        }
    }

    pub async fn handle_message(&self, msg: ProtocolMessage) -> Result<ProtocolMessage> {
        msg.validate()?;

        match msg.msg_type {
            MessageType::SessionControl => self.handle_session_control(msg).await,
            MessageType::Heartbeat => self.handle_heartbeat(msg).await,
            MessageType::Metadata => self.handle_metadata(msg).await,
            MessageType::StateSync => self.handle_state_sync(msg).await,
            MessageType::Capabilities => self.handle_capabilities(msg).await,
            MessageType::Version => self.handle_version(msg).await,
            _ => Ok(ProtocolMessage {
                msg_type: MessageType::Error,
                command: None,
                session_id: msg.session_id,
                metadata: Some(serde_json::json!({
                    "error": "Unsupported message type"
                })),
                payload: None,
            }),
        }
    }

    async fn handle_session_control(&self, msg: ProtocolMessage) -> Result<ProtocolMessage> {
        let command = msg
            .command
            .as_ref()
            .ok_or_else(|| anyhow!("Missing command"))?;

        match command {
            SessionCommand::Init => {
                let session_id = uuid::Uuid::new_v4().to_string();
                self.create_session(&session_id).await?;

                Ok(ProtocolMessage {
                    msg_type: MessageType::SessionControl,
                    command: Some(SessionCommand::InitAck),
                    session_id: Some(session_id),
                    metadata: Some(serde_json::json!({
                        "server_version": self.version,
                        "timestamp": Utc::now().to_rfc3339(),
                    })),
                    payload: None,
                })
            }
            SessionCommand::Resume => {
                let session_id = msg
                    .session_id
                    .as_ref()
                    .ok_or_else(|| anyhow!("Missing session_id"))?;

                // Verify session exists
                match self.get_session(session_id).await {
                    Ok(_) => {}
                    Err(_) => {
                        return Ok(ProtocolError::SessionNotFound(session_id.clone())
                            .to_protocol_message());
                    }
                }

                Ok(ProtocolMessage {
                    msg_type: MessageType::SessionControl,
                    command: Some(SessionCommand::ResumeAck),
                    session_id: Some(session_id.clone()),
                    metadata: None,
                    payload: None,
                })
            }
            SessionCommand::Clear => {
                let session_id = msg
                    .session_id
                    .as_ref()
                    .ok_or_else(|| anyhow!("Missing session_id"))?;

                let mut sessions = self.sessions.write().await;
                sessions.clear_session(session_id).await?;

                Ok(ProtocolMessage {
                    msg_type: MessageType::SessionControl,
                    command: Some(SessionCommand::ClearAck),
                    session_id: Some(session_id.clone()),
                    metadata: None,
                    payload: None,
                })
            }
            SessionCommand::Handoff => {
                let session_id = msg
                    .session_id
                    .as_ref()
                    .ok_or_else(|| anyhow!("Missing session_id"))?;

                let session = self.get_session(session_id).await?;
                let session_data = serde_json::json!({
                    "session_id": session_id,
                    "message_count": session.message_count(),
                    "created_at": session.created_at_iso(),
                });

                let conversation_history = session
                    .conversation_history()
                    .iter()
                    .map(|msg| {
                        let mut obj = serde_json::json!({
                            "role": msg.role,
                            "content": msg.content,
                        });
                        if let Some(ts) = msg.timestamp {
                            obj["timestamp"] = serde_json::json!(ts);
                        }
                        obj
                    })
                    .collect::<Vec<_>>();

                Ok(ProtocolMessage {
                    msg_type: MessageType::SessionControl,
                    command: Some(SessionCommand::HandoffReady),
                    session_id: Some(session_id.clone()),
                    metadata: msg.metadata,
                    payload: Some(serde_json::json!({
                        "session_data": session_data,
                        "conversation_history": conversation_history,
                    })),
                })
            }
            SessionCommand::Terminate => {
                let session_id = msg
                    .session_id
                    .as_ref()
                    .ok_or_else(|| anyhow!("Missing session_id"))?;

                let sessions = self.sessions.write().await;
                sessions.remove_session(session_id).await?;

                Ok(ProtocolMessage {
                    msg_type: MessageType::SessionControl,
                    command: Some(SessionCommand::TerminateAck),
                    session_id: Some(session_id.clone()),
                    metadata: msg.metadata,
                    payload: None,
                })
            }
            _ => Err(anyhow!("Unsupported session command: {:?}", command)),
        }
    }

    async fn handle_heartbeat(&self, msg: ProtocolMessage) -> Result<ProtocolMessage> {
        Ok(ProtocolMessage {
            msg_type: MessageType::HeartbeatAck,
            command: None,
            session_id: msg.session_id,
            metadata: Some(serde_json::json!({
                "server_timestamp": Utc::now().to_rfc3339(),
                "client_metadata": msg.metadata,
            })),
            payload: None,
        })
    }

    async fn handle_metadata(&self, msg: ProtocolMessage) -> Result<ProtocolMessage> {
        if let Some(session_id) = &msg.session_id {
            if let Some(_metadata) = &msg.metadata {
                // In a real implementation, we'd store metadata
                // For now, just acknowledge
            }
        }

        Ok(ProtocolMessage {
            msg_type: MessageType::MetadataAck,
            command: None,
            session_id: msg.session_id,
            metadata: None,
            payload: None,
        })
    }

    async fn handle_state_sync(&self, msg: ProtocolMessage) -> Result<ProtocolMessage> {
        let session_id = msg
            .session_id
            .as_ref()
            .ok_or_else(|| anyhow!("Missing session_id"))?;

        let session = self.get_session(session_id).await?;

        Ok(ProtocolMessage {
            msg_type: MessageType::StateSyncAck,
            command: None,
            session_id: Some(session_id.clone()),
            metadata: None,
            payload: Some(serde_json::json!({
                "message_count": session.message_count(),
                "last_activity": session.last_activity_iso(),
                "memory_used": session.memory_used(),
            })),
        })
    }

    async fn handle_capabilities(&self, msg: ProtocolMessage) -> Result<ProtocolMessage> {
        let server_capabilities = self.capabilities.read().await.clone();

        let mut negotiated_capabilities = server_capabilities.clone();
        if let Some(metadata) = &msg.metadata {
            if let Some(client_caps) = metadata["client_capabilities"].as_array() {
                let client_cap_strings: Vec<String> = client_caps
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();

                negotiated_capabilities.retain(|cap| client_cap_strings.contains(cap));
            }
        }

        Ok(ProtocolMessage {
            msg_type: MessageType::CapabilitiesAck,
            command: None,
            session_id: None,
            metadata: Some(serde_json::json!({
                "server_capabilities": server_capabilities,
                "negotiated_capabilities": negotiated_capabilities,
            })),
            payload: None,
        })
    }

    async fn handle_version(&self, msg: ProtocolMessage) -> Result<ProtocolMessage> {
        let compatible = if let Some(metadata) = &msg.metadata {
            if let Some(min_version) = metadata["min_version"].as_str() {
                // Simple version comparison (in production, use semver)
                min_version <= self.version.as_str()
            } else {
                true
            }
        } else {
            true
        };

        Ok(ProtocolMessage {
            msg_type: MessageType::VersionAck,
            command: None,
            session_id: None,
            metadata: Some(serde_json::json!({
                "server_version": self.version,
                "compatible": compatible,
            })),
            payload: None,
        })
    }

    pub async fn create_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions
            .create_session_with_id(session_id.to_string(), SessionConfig::default())
            .await?;
        Ok(())
    }

    pub async fn get_session(&self, session_id: &str) -> Result<WebSocketSession> {
        let sessions = self.sessions.read().await;
        sessions
            .get_session(session_id)
            .await
            .ok_or_else(|| anyhow!("Session not found: {}", session_id))
    }

    pub async fn get_session_mut(&self, session_id: &str) -> Result<WebSocketSession> {
        let sessions = self.sessions.read().await;
        // SessionStore doesn't have get_session_mut, so get a clone
        sessions
            .get_session(session_id)
            .await
            .ok_or_else(|| anyhow!("Session not found: {}", session_id))
    }
}
