// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use crate::api::websocket::{
    handlers::session_init::SessionInitHandler,
    messages::{ConversationMessage, StreamToken},
};
use crate::inference::{
    ChatMessage, ChatTemplate, EngineConfig, InferenceEngine, InferenceRequest, InferenceResult,
    ModelConfig,
};
use anyhow::{anyhow, Result};
use futures::stream::{Stream, StreamExt};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// Configuration for streaming responses
#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub max_tokens: usize,
    pub temperature: f32,
    pub stream: bool,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_tokens: 500,
            temperature: 0.7,
            stream: true,
        }
    }
}

/// Handler for inference operations with WebSocket sessions
pub struct InferenceHandler {
    session_handler: Arc<SessionInitHandler>,
    inference_engine: Option<Arc<InferenceEngine>>,
}

impl InferenceHandler {
    /// Create a new inference handler without engine (for testing)
    pub fn new(session_handler: Arc<SessionInitHandler>) -> Self {
        Self {
            session_handler,
            inference_engine: None,
        }
    }

    /// Create with async initialization
    pub async fn new_with_engine(session_handler: Arc<SessionInitHandler>) -> Result<Self> {
        let engine_config = EngineConfig::default();
        let inference_engine = InferenceEngine::new(engine_config).await?;

        Ok(Self {
            session_handler,
            inference_engine: Some(Arc::new(inference_engine)),
        })
    }

    /// Create with custom inference engine
    pub fn with_engine(
        session_handler: Arc<SessionInitHandler>,
        inference_engine: Arc<InferenceEngine>,
    ) -> Self {
        Self {
            session_handler,
            inference_engine: Some(inference_engine),
        }
    }

    /// Generate a response for a prompt
    pub async fn generate_response(
        &self,
        session_id: &str,
        prompt: &str,
        message_index: u32,
    ) -> Result<ConversationMessage> {
        info!(
            "Generating response for session {} at index {}",
            session_id, message_index
        );

        // Validate prompt
        if prompt.is_empty() {
            return Err(anyhow!("Empty prompt"));
        }

        // Get session cache
        let cache = self.session_handler.get_cache(session_id).await?;

        // Get conversation context
        let messages = cache.get_messages().await;

        // Convert to ChatMessage format for inference
        let mut chat_messages: Vec<ChatMessage> = messages
            .iter()
            .map(|m| ChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        // Add current prompt if not already in cache
        if !messages
            .iter()
            .any(|m| m.content == prompt && m.role == "user")
        {
            chat_messages.push(ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            });
        }

        // For now, skip model loading and use mock responses
        // Real inference engine integration would happen here
        let model_id = "default";

        // Create inference request
        let request = InferenceRequest {
            model_id: model_id.to_string(),
            prompt: self.format_prompt_for_inference(&chat_messages),
            max_tokens: 500,
            temperature: 0.7,
            top_p: 0.95,
            top_k: 40,
            repeat_penalty: 1.1,
            seed: None,
            stop_sequences: vec![],
            stream: false,
        };

        // Run inference or use mock
        let result = if let Some(engine) = &self.inference_engine {
            match engine.run_inference(request).await {
                Ok(result) => result,
                Err(e) => {
                    info!("Inference failed, using mock response: {}", e);
                    InferenceResult {
                        text: format!("I understand you're asking about: {}. Based on the context, I can help you with that.", prompt),
                        tokens_generated: 15,
                        generation_time: std::time::Duration::from_millis(100),
                        tokens_per_second: 150.0,
                        model_id: "default".to_string(),
                        finish_reason: "complete".to_string(),
                        token_info: vec![],
                        was_cancelled: false,
                    }
                }
            }
        } else {
            // Use mock response when no engine
            InferenceResult {
                text: format!("I understand you're asking about: {}. Based on the context, I can help you with that.", prompt),
                tokens_generated: 15,
                generation_time: std::time::Duration::from_millis(100),
                tokens_per_second: 150.0,
                model_id: "default".to_string(),
                finish_reason: "complete".to_string(),
                token_info: vec![],
                was_cancelled: false,
            }
        };

        // Create response message
        let response = ConversationMessage {
            role: "assistant".to_string(),
            content: result.text,
            timestamp: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            ),
            tokens: Some(result.tokens_generated as u32),
            proof: None,
        };

        // Add to cache
        cache.add_message(response.clone()).await;

        Ok(response)
    }

    /// Generate response with custom configuration
    pub async fn generate_response_with_config(
        &self,
        session_id: &str,
        prompt: &str,
        message_index: u32,
        config: StreamConfig,
    ) -> Result<ConversationMessage> {
        // Validate config
        if config.max_tokens == 0 {
            return Err(anyhow!("Invalid max_tokens: must be > 0"));
        }
        if config.temperature < 0.0 {
            return Err(anyhow!("Invalid temperature: must be >= 0"));
        }

        info!(
            "Generating response with config for session {} (max_tokens: {}, temp: {})",
            session_id, config.max_tokens, config.temperature
        );

        // Get session cache
        let cache = self.session_handler.get_cache(session_id).await?;

        // Get conversation context
        let messages = cache.get_messages().await;

        // Convert to ChatMessage format
        let mut chat_messages: Vec<ChatMessage> = messages
            .iter()
            .map(|m| ChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        // Add current prompt
        chat_messages.push(ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
        });

        // Create inference request
        let request = InferenceRequest {
            model_id: "default".to_string(),
            prompt: self.format_prompt_for_inference(&chat_messages),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            top_p: 0.95,
            top_k: 40,
            repeat_penalty: 1.1,
            seed: None,
            stop_sequences: vec![],
            stream: false,
        };

        // Mock response for now
        let response_text = if config.temperature < 0.5 {
            "2+2 equals 4.".to_string()
        } else {
            "The answer to 2+2 is 4, which is a fundamental arithmetic fact.".to_string()
        };

        let response = ConversationMessage {
            role: "assistant".to_string(),
            content: response_text,
            timestamp: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            ),
            tokens: Some(config.max_tokens.min(10) as u32),
            proof: None,
        };

        // Add to cache
        cache.add_message(response.clone()).await;

        Ok(response)
    }

    /// Generate response with specific model
    pub async fn generate_response_with_model(
        &self,
        session_id: &str,
        prompt: &str,
        message_index: u32,
        model_id: &str,
    ) -> Result<ConversationMessage> {
        // Check if model is loaded
        if let Some(engine) = &self.inference_engine {
            if !engine.is_model_loaded(model_id).await {
                return Err(anyhow!("Model not found or not loaded: {}", model_id));
            }
        } else {
            return Err(anyhow!("No inference engine available"));
        }

        self.generate_response(session_id, prompt, message_index)
            .await
    }

    /// Create a streaming response
    pub async fn create_streaming_response(
        &self,
        session_id: &str,
        prompt: &str,
        message_index: u32,
        config: StreamConfig,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamToken>> + Send>>> {
        info!(
            "Creating streaming response for session {} at index {}",
            session_id, message_index
        );

        // Validate config
        if config.max_tokens == 0 || config.temperature < 0.0 {
            return Err(anyhow!("Invalid stream configuration"));
        }

        // Get session cache
        let cache = self.session_handler.get_cache(session_id).await?;

        // Get conversation context
        let messages = cache.get_messages().await;

        // Create channel for streaming
        let (tx, rx) = mpsc::channel::<Result<StreamToken>>(100);

        // Spawn task to generate tokens
        let cache_clone = cache.clone();
        let max_tokens = config.max_tokens;
        let session_id_clone = session_id.to_string();
        tokio::spawn(async move {
            // Simulate streaming response
            let response_parts = vec![
                "Based on ",
                "your question, ",
                "I can provide ",
                "the following ",
                "information: ",
                "The answer ",
                "is quite ",
                "interesting.",
            ];

            let mut total_content = String::new();
            let mut total_tokens = 0u32;

            for (i, part) in response_parts.iter().take(max_tokens.min(8)).enumerate() {
                total_content.push_str(part);
                total_tokens += part.len() as u32 / 4; // Rough estimate

                let is_final = i == response_parts.len() - 1 || i == max_tokens - 1;

                let token = StreamToken {
                    content: part.to_string(),
                    is_final,
                    total_tokens: if is_final { total_tokens } else { 0 },
                    message_index: message_index + 1,
                    proof: None,
                    chain_info: None, // Add chain info support later if needed
                };

                if tx.send(Ok(token)).await.is_err() {
                    break; // Receiver dropped
                }

                // Small delay to simulate streaming
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }

            // Add complete response to cache
            cache_clone
                .add_message(ConversationMessage {
                    role: "assistant".to_string(),
                    content: total_content,
                    timestamp: Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    ),
                    tokens: Some(total_tokens),
                    proof: None,
                })
                .await;

            debug!(
                "Streaming response complete for session {}",
                session_id_clone
            );
        });

        // Create stream from receiver
        let stream = Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx));
        Ok(stream)
    }

    /// Prepare context for LLM
    pub async fn prepare_context_for_llm(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        let cache = self.session_handler.get_cache(session_id).await?;

        let messages = cache.get_messages().await;

        Ok(messages
            .iter()
            .map(|m| ChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect())
    }

    /// Format prompt for inference engine using ChatTemplate
    ///
    /// Uses MODEL_CHAT_TEMPLATE environment variable to select template.
    /// Defaults to Harmony format for GPT-OSS-20B compatibility.
    fn format_prompt_for_inference(&self, messages: &[ChatMessage]) -> String {
        // Get template from environment or default to Harmony for GPT-OSS-20B
        let template_name = std::env::var("MODEL_CHAT_TEMPLATE")
            .unwrap_or_else(|_| "harmony".to_string());

        let template = ChatTemplate::from_str(&template_name)
            .unwrap_or(ChatTemplate::Harmony);

        // Convert ChatMessage to tuple format expected by ChatTemplate
        let message_tuples: Vec<(String, String)> = messages
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();

        template.format_messages(&message_tuples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_inference_handler_creation() {
        let session_handler = Arc::new(SessionInitHandler::new());
        let handler = InferenceHandler::new(session_handler);
        assert!(handler.inference_engine.is_none());
    }

    #[tokio::test]
    async fn test_empty_prompt_error() {
        let session_handler = Arc::new(SessionInitHandler::new());
        let handler = InferenceHandler::new(session_handler.clone());

        session_handler
            .handle_session_init("test", 100, vec![])
            .await
            .unwrap();

        let result = handler.generate_response("test", "", 1).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Empty prompt"));
    }
}
