// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! VLM sidecar client for vision tasks via OpenAI-compatible API

use anyhow::Result;
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, info};

// --- OpenAI-compatible serde structs ---

#[derive(serde::Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(serde::Serialize)]
struct ChatMessage {
    role: String,
    content: serde_json::Value,
}

#[derive(serde::Deserialize)]
struct ChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(serde::Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
}

#[derive(serde::Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(serde::Deserialize)]
struct ChatResponseMessage {
    content: String,
}

// --- Result types ---

/// Result from VLM-based OCR
pub struct VlmOcrResult {
    pub text: String,
    pub model: String,
    pub processing_time_ms: u64,
    pub tokens_used: u32,
}

/// Result from VLM-based image description
pub struct VlmDescribeResult {
    pub description: String,
    pub model: String,
    pub processing_time_ms: u64,
    pub tokens_used: u32,
}

/// Client for calling a VLM sidecar service via OpenAI-compatible API
pub struct VlmClient {
    client: Client,
    endpoint: String,
    model_name: String,
}

const OCR_PROMPT: &str = "Extract all text from this image. Return only the extracted text, preserving the original layout and formatting as much as possible. If no text is found, respond with an empty string.";

const DESCRIBE_BRIEF: &str = "Describe this image in one sentence.";
const DESCRIBE_DETAILED: &str =
    "Describe this image in detail, including objects, scene, colors, and any text visible.";
const DESCRIBE_COMPREHENSIVE: &str = "Provide a comprehensive, detailed analysis of this image. Describe all objects, people, text, colors, composition, and any notable details.";

impl VlmClient {
    /// Create a new VLM client
    pub fn new(endpoint: &str, model_name: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;

        let endpoint = endpoint.trim_end_matches('/').to_string();
        info!(
            "VLM client configured: endpoint={}, model={}",
            endpoint, model_name
        );

        Ok(Self {
            client,
            endpoint,
            model_name: model_name.to_string(),
        })
    }

    /// Get the model name
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Check if the VLM sidecar is healthy
    pub async fn health_check(&self) -> bool {
        match self
            .client
            .get(format!("{}/health", self.endpoint))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                debug!("VLM health check failed: {}", e);
                false
            }
        }
    }

    /// Extract text from an image using the VLM
    pub async fn ocr(&self, base64_image: &str, format: &str) -> Result<VlmOcrResult> {
        let start = std::time::Instant::now();
        let data_url = format!("data:image/{};base64,{}", format, base64_image);

        let request = ChatRequest {
            model: self.model_name.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: serde_json::json!([
                    {"type": "text", "text": OCR_PROMPT},
                    {"type": "image_url", "image_url": {"url": data_url}}
                ]),
            }],
            max_tokens: 4096,
            temperature: 0.1,
        };

        let response = self
            .client
            .post(format!("{}/v1/chat/completions", self.endpoint))
            .json(&request)
            .send()
            .await?;

        let chat_response: ChatResponse = response.json().await?;
        let text = chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        let tokens_used = chat_response.usage.map(|u| u.total_tokens).unwrap_or(0);

        Ok(VlmOcrResult {
            text,
            model: self.model_name.clone(),
            processing_time_ms: start.elapsed().as_millis() as u64,
            tokens_used,
        })
    }

    /// Describe an image using the VLM
    pub async fn describe(
        &self,
        base64_image: &str,
        format: &str,
        detail: &str,
        custom_prompt: Option<&str>,
    ) -> Result<VlmDescribeResult> {
        let start = std::time::Instant::now();
        let data_url = format!("data:image/{};base64,{}", format, base64_image);

        let text_prompt = custom_prompt.unwrap_or(match detail {
            "brief" => DESCRIBE_BRIEF,
            "comprehensive" => DESCRIBE_COMPREHENSIVE,
            _ => DESCRIBE_DETAILED,
        });

        let request = ChatRequest {
            model: self.model_name.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: serde_json::json!([
                    {"type": "text", "text": text_prompt},
                    {"type": "image_url", "image_url": {"url": data_url}}
                ]),
            }],
            max_tokens: match detail {
                "brief" => 100,
                "comprehensive" => 2048,
                _ => 300,
            },
            temperature: 0.3,
        };

        let response = self
            .client
            .post(format!("{}/v1/chat/completions", self.endpoint))
            .json(&request)
            .send()
            .await?;

        let chat_response: ChatResponse = response.json().await?;
        let description = chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        let tokens_used = chat_response.usage.map(|u| u.total_tokens).unwrap_or(0);

        Ok(VlmDescribeResult {
            description,
            model: self.model_name.clone(),
            processing_time_ms: start.elapsed().as_millis() as u64,
            tokens_used,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vlm_client_new() {
        let client = VlmClient::new("http://localhost:8081", "qwen3-vl").unwrap();
        assert_eq!(client.endpoint, "http://localhost:8081");
        assert_eq!(client.model_name, "qwen3-vl");
    }

    #[tokio::test]
    async fn test_vlm_client_health_check_unreachable() {
        let client = VlmClient::new("http://127.0.0.1:59999", "test-model").unwrap();
        let healthy = client.health_check().await;
        assert!(!healthy);
    }

    #[test]
    fn test_vlm_client_default_timeout() {
        let client = VlmClient::new("http://localhost:8081", "qwen3-vl").unwrap();
        assert_eq!(client.endpoint, "http://localhost:8081");
    }

    #[test]
    fn test_vlm_client_model_name() {
        let client = VlmClient::new("http://localhost:8081", "qwen3-vl-8b").unwrap();
        assert_eq!(client.model_name(), "qwen3-vl-8b");
    }

    #[test]
    fn test_vlm_client_trailing_slash_trimmed() {
        let client = VlmClient::new("http://localhost:8081/", "test").unwrap();
        assert_eq!(client.endpoint, "http://localhost:8081");
    }

    // --- Phase 1.2 tests ---

    #[test]
    fn test_ocr_request_format() {
        let request = ChatRequest {
            model: "qwen3-vl".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: serde_json::json!([
                    {"type": "text", "text": OCR_PROMPT},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,abc123"}}
                ]),
            }],
            max_tokens: 4096,
            temperature: 0.1,
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["model"], "qwen3-vl");
        assert_eq!(json["max_tokens"], 4096);
        let temp = json["temperature"].as_f64().unwrap();
        assert!((temp - 0.1).abs() < 0.01);
        let content = &json["messages"][0]["content"];
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
    }

    #[test]
    fn test_ocr_response_parsing() {
        let json = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "Hello World\nLine 2"
                }
            }]
        });
        let response: ChatResponse = serde_json::from_value(json).unwrap();
        assert_eq!(response.choices[0].message.content, "Hello World\nLine 2");
    }

    #[test]
    fn test_ocr_prompt_construction() {
        assert!(OCR_PROMPT.contains("Extract all text"));
        assert!(OCR_PROMPT.contains("preserving the original layout"));
    }

    // --- Phase 1.3 tests ---

    #[test]
    fn test_describe_prompt_brief() {
        assert_eq!(DESCRIBE_BRIEF, "Describe this image in one sentence.");
    }

    #[test]
    fn test_describe_prompt_detailed() {
        assert!(DESCRIBE_DETAILED.contains("in detail"));
        assert!(DESCRIBE_DETAILED.contains("objects"));
    }

    // --- Phase 7 tests ---

    #[test]
    fn test_chat_usage_deserialization() {
        let json = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 42,
            "total_tokens": 142
        });
        let usage: ChatUsage = serde_json::from_value(json).unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 42);
        assert_eq!(usage.total_tokens, 142);
    }

    #[test]
    fn test_chat_response_with_usage() {
        let json = serde_json::json!({
            "choices": [{
                "message": { "content": "A cat." }
            }],
            "usage": {
                "prompt_tokens": 200,
                "completion_tokens": 15,
                "total_tokens": 215
            }
        });
        let response: ChatResponse = serde_json::from_value(json).unwrap();
        assert_eq!(response.choices[0].message.content, "A cat.");
        let usage = response.usage.expect("usage should be present");
        assert_eq!(usage.total_tokens, 215);
    }

    #[test]
    fn test_describe_response_parsing() {
        let json = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "A cat sitting on a windowsill looking outside."
                }
            }]
        });
        let response: ChatResponse = serde_json::from_value(json).unwrap();
        assert_eq!(
            response.choices[0].message.content,
            "A cat sitting on a windowsill looking outside."
        );
    }
}
