use crate::api::websocket::{
    handlers::session_init::SessionInitHandler,
    messages::{ConversationMessage, StreamToken},
};
use anyhow::{anyhow, Result};
use futures::stream::{Stream, StreamExt};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info};

/// Handler for response generation and streaming
pub struct ResponseHandler {
    session_handler: Arc<SessionInitHandler>,
}

impl ResponseHandler {
    /// Create a new response handler
    pub fn new(session_handler: Arc<SessionInitHandler>) -> Self {
        Self { session_handler }
    }
    
    /// Create a response stream for a prompt
    pub async fn create_response_stream(
        &self,
        session_id: &str,
        prompt: &str,
        message_index: u32,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>>> {
        info!(
            "Creating response stream for session {} at index {}",
            session_id, message_index
        );
        
        // Get the session cache
        let cache = self.session_handler.get_cache(session_id).await?;
        
        // Get all messages for context
        let messages = cache.get_messages().await;
        
        // Create a channel for streaming
        let (tx, mut rx) = mpsc::channel::<Result<StreamToken>>(100);
        
        // Spawn task to generate response
        let session_id_clone = session_id.to_string();
        let cache_clone = cache.clone();
        tokio::spawn(async move {
            // Simulate LLM response generation
            let response_parts = vec![
                "Based on ",
                "the context, ",
                "I can help you ",
                "with that. ",
                "The answer is ",
                "quite interesting.",
            ];
            
            let mut total_content = String::new();
            let mut total_tokens = 0u32;
            
            for (i, part) in response_parts.iter().enumerate() {
                total_content.push_str(part);
                total_tokens += part.len() as u32 / 4; // Rough token estimate
                
                let is_final = i == response_parts.len() - 1;
                
                let token = StreamToken {
                    content: part.to_string(),
                    is_final,
                    total_tokens: if is_final { total_tokens } else { 0 },
                    message_index: message_index + 1,
                };
                
                if tx.send(Ok(token)).await.is_err() {
                    break; // Receiver dropped
                }
                
                // Small delay to simulate streaming
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
            
            // Add complete response to cache
            cache_clone.add_message(ConversationMessage {
                role: "assistant".to_string(),
                content: total_content,
                timestamp: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                ),
                tokens: Some(total_tokens),
            }).await;
            
            debug!("Response generation complete for session {}", session_id_clone);
        });
        
        // Create stream from receiver
        let stream = Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx));
        Ok(stream)
    }
    
    /// Generate a simple response (non-streaming)
    pub async fn generate_response(
        &self,
        session_id: &str,
        prompt: &str,
        message_index: u32,
    ) -> Result<ConversationMessage> {
        info!("Generating response for session {}", session_id);
        
        // Get the session cache
        let cache = self.session_handler.get_cache(session_id).await?;
        
        // Get context
        let messages = cache.get_messages().await;
        
        // Generate response (simplified for testing)
        let response = ConversationMessage {
            role: "assistant".to_string(),
            content: format!("Response to: {}", prompt),
            timestamp: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            ),
            tokens: Some(10), // Mock token count
        };
        
        // Add to cache
        cache.add_message(response.clone()).await;
        
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn test_response_generation() {
        let session_handler = Arc::new(SessionInitHandler::new());
        let response_handler = ResponseHandler::new(session_handler.clone());
        
        // Initialize session
        session_handler
            .handle_session_init("resp-test", 123, vec![])
            .await
            .unwrap();
        
        // Generate response
        let response = response_handler
            .generate_response("resp-test", "Test prompt", 1)
            .await
            .unwrap();
        
        assert_eq!(response.role, "assistant");
        assert!(response.content.contains("Response to:"));
        assert!(response.tokens.is_some());
    }
    
    #[tokio::test]
    async fn test_response_streaming() {
        let session_handler = Arc::new(SessionInitHandler::new());
        let response_handler = ResponseHandler::new(session_handler.clone());
        
        // Initialize session
        session_handler
            .handle_session_init("stream-test", 456, vec![])
            .await
            .unwrap();
        
        // Create stream
        let mut stream = response_handler
            .create_response_stream("stream-test", "Test", 1)
            .await
            .unwrap();
        
        let mut tokens_received = 0;
        let mut has_final = false;
        
        while let Some(result) = stream.next().await {
            if let Ok(token) = result {
                tokens_received += 1;
                if token.is_final {
                    has_final = true;
                    assert!(token.total_tokens > 0);
                }
            }
        }
        
        assert!(tokens_received > 0);
        assert!(has_final);
    }
}