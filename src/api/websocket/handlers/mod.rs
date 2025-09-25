pub mod session_init;
pub mod session_resume;
pub mod prompt;
pub mod response;
pub mod inference;
pub mod disconnect;

use super::messages::{WebSocketMessage, ErrorCode};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Main message router for WebSocket handlers
pub struct MessageRouter {
    session_init_handler: Arc<session_init::SessionInitHandler>,
    session_resume_handler: Arc<session_resume::SessionResumeHandler>,
    prompt_handler: Arc<prompt::PromptHandler>,
    response_handler: Arc<response::ResponseHandler>,
}

impl MessageRouter {
    /// Create a new message router
    pub fn new() -> Self {
        let session_init_handler = Arc::new(session_init::SessionInitHandler::new());
        let session_resume_handler = Arc::new(session_resume::SessionResumeHandler::new());
        let prompt_handler = Arc::new(prompt::PromptHandler::new(session_init_handler.clone()));
        let response_handler = Arc::new(response::ResponseHandler::new(session_init_handler.clone(), None));
        
        Self {
            session_init_handler,
            session_resume_handler,
            prompt_handler,
            response_handler,
        }
    }
    
    /// Route a message to the appropriate handler
    pub async fn route_message(&self, message: WebSocketMessage) -> Result<WebSocketMessage> {
        match message {
            WebSocketMessage::SessionInit { session_id, job_id, conversation_context } => {
                match self.session_init_handler.handle_session_init(&session_id, job_id, conversation_context).await {
                    Ok(response) => Ok(WebSocketMessage::Response {
                        session_id: response.session_id,
                        content: format!("Session initialized with {} messages", response.message_count),
                        tokens_used: response.total_tokens,
                        message_index: 0,
                    }),
                    Err(e) => Ok(WebSocketMessage::Error {
                        session_id,
                        error: e.to_string(),
                        code: ErrorCode::InternalError,
                    }),
                }
            },
            
            WebSocketMessage::SessionResume { session_id, job_id, conversation_context, last_message_index } => {
                match self.session_resume_handler.handle_session_resume(&session_id, job_id, conversation_context, last_message_index).await {
                    Ok(response) => Ok(WebSocketMessage::Response {
                        session_id: response.session_id,
                        content: format!("Session resumed with {} messages", response.message_count),
                        tokens_used: response.total_tokens,
                        message_index: response.last_message_index,
                    }),
                    Err(e) => Ok(WebSocketMessage::Error {
                        session_id,
                        error: e.to_string(),
                        code: ErrorCode::InternalError,
                    }),
                }
            },
            
            WebSocketMessage::Prompt { session_id, content, message_index } => {
                match self.prompt_handler.handle_prompt(&session_id, &content, message_index).await {
                    Ok(_) => {
                        // Start streaming response
                        Ok(WebSocketMessage::Response {
                            session_id,
                            content: "Processing...".to_string(),
                            tokens_used: 0,
                            message_index: message_index + 1,
                        })
                    },
                    Err(e) => Ok(WebSocketMessage::Error {
                        session_id,
                        error: e.to_string(),
                        code: ErrorCode::InternalError,
                    }),
                }
            },
            
            WebSocketMessage::SessionEnd { session_id } => {
                // Clean up session
                self.session_init_handler.cleanup_session(&session_id).await;
                Ok(WebSocketMessage::SessionEnd { session_id })
            },
            
            _ => Ok(WebSocketMessage::Error {
                session_id: message.session_id().to_string(),
                error: "Unsupported message type".to_string(),
                code: ErrorCode::InternalError,
            }),
        }
    }
}

impl Default for MessageRouter {
    fn default() -> Self {
        Self::new()
    }
}