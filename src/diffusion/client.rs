// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SGLang Diffusion sidecar client for image generation via OpenAI-compatible API

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info};

/// Allowed output sizes for image generation
pub const ALLOWED_SIZES: &[&str] = &[
    "256x256",
    "512x512",
    "768x768",
    "1024x1024",
    "1024x768",
    "768x1024",
];

fn default_size() -> String {
    "1024x1024".to_string()
}

fn default_steps() -> u32 {
    4
}

fn default_guidance_scale() -> f32 {
    3.5
}

fn default_response_format() -> String {
    "b64_json".to_string()
}

fn default_n() -> u32 {
    1
}

/// Client for calling an SGLang Diffusion sidecar via OpenAI-compatible API
pub struct DiffusionClient {
    client: Client,
    endpoint: String,
    model_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGenerationRequest {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default = "default_size")]
    pub size: String,
    #[serde(default = "default_steps")]
    pub steps: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    #[serde(default = "default_guidance_scale")]
    pub guidance_scale: f32,
    #[serde(default = "default_response_format")]
    pub response_format: String,
    #[serde(default = "default_n")]
    pub n: u32,
}

#[derive(Debug, Clone)]
pub struct DiffusionResult {
    pub base64_image: String,
    pub model: String,
    pub processing_time_ms: u64,
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    pub revised_prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

// --- OpenAI-compatible response types ---

#[derive(Debug, Deserialize)]
pub struct OpenAIImageResponse {
    pub data: Vec<OpenAIImageData>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAIImageData {
    pub b64_json: Option<String>,
    pub revised_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIModelList {
    data: Vec<OpenAIModelEntry>,
}

#[derive(Debug, Deserialize)]
struct OpenAIModelEntry {
    id: String,
}

// --- Implementations ---

impl ImageGenerationRequest {
    /// Validate the request fields
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.prompt.trim().is_empty() {
            return Err("prompt must not be empty".to_string());
        }
        if !ALLOWED_SIZES.contains(&self.size.as_str()) {
            return Err(format!(
                "invalid size '{}'; allowed: {}",
                self.size,
                ALLOWED_SIZES.join(", ")
            ));
        }
        if self.steps == 0 || self.steps > 100 {
            return Err(format!(
                "steps must be between 1 and 100, got {}",
                self.steps
            ));
        }
        Ok(())
    }
}

impl ImageSize {
    /// Parse a size string like "1024x1024" into an ImageSize
    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        let parts: Vec<&str> = s.split('x').collect();
        if parts.len() != 2 {
            return Err(format!(
                "invalid size format '{}'; expected WIDTHxHEIGHT",
                s
            ));
        }
        let width = parts[0]
            .parse::<u32>()
            .map_err(|_| format!("invalid width in '{}'", s))?;
        let height = parts[1]
            .parse::<u32>()
            .map_err(|_| format!("invalid height in '{}'", s))?;
        if width == 0 || height == 0 {
            return Err(format!("width and height must be > 0 in '{}'", s));
        }
        Ok(Self { width, height })
    }

    /// Calculate megapixels for this size
    pub fn megapixels(&self) -> f64 {
        (self.width as f64 * self.height as f64) / 1_048_576.0
    }
}

impl DiffusionClient {
    /// Create a new DiffusionClient
    pub fn new(endpoint: &str, model_name: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;

        let endpoint = endpoint.trim_end_matches('/').to_string();
        info!(
            "Diffusion client configured: endpoint={}, model={}",
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

    /// Check if the diffusion sidecar is healthy
    pub async fn health_check(&self) -> bool {
        match self
            .client
            .get(format!("{}/health", self.endpoint))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                debug!("Diffusion health check failed: {}", e);
                false
            }
        }
    }

    /// Generate an image from a text prompt
    pub async fn generate(&self, request: &ImageGenerationRequest) -> Result<DiffusionResult> {
        request
            .validate()
            .map_err(|e| anyhow::anyhow!("validation failed: {}", e))?;

        let size =
            ImageSize::parse(&request.size).map_err(|e| anyhow::anyhow!("invalid size: {}", e))?;

        let start = std::time::Instant::now();

        let mut body = serde_json::json!({
            "prompt": request.prompt,
            "model": request.model.as_deref().unwrap_or(&self.model_name),
            "size": request.size,
            "n": request.n,
            "response_format": request.response_format,
            "guidance_scale": request.guidance_scale,
            "num_inference_steps": request.steps,
        });
        if let Some(seed) = request.seed {
            body["seed"] = serde_json::json!(seed);
        }
        if let Some(ref neg) = request.negative_prompt {
            body["negative_prompt"] = serde_json::json!(neg);
        }

        let url = format!("{}/v1/images/generations", self.endpoint);
        debug!("Diffusion generate POST {}", url);

        let response = self.client.post(&url).json(&body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "diffusion sidecar returned {}: {}",
                status,
                text
            ));
        }

        let api_response: OpenAIImageResponse = response.json().await?;
        let first = api_response
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty response from diffusion sidecar"))?;

        let base64_image = first
            .b64_json
            .ok_or_else(|| anyhow::anyhow!("no b64_json in response"))?;

        Ok(DiffusionResult {
            base64_image,
            model: request
                .model
                .clone()
                .unwrap_or_else(|| self.model_name.clone()),
            processing_time_ms: start.elapsed().as_millis() as u64,
            seed: request.seed.unwrap_or(0),
            width: size.width,
            height: size.height,
            steps: request.steps,
            revised_prompt: first.revised_prompt,
        })
    }

    /// Generate an image with an input image (img2img / edit)
    pub async fn generate_with_edit(
        &self,
        request: &ImageGenerationRequest,
        base64_image: &str,
        strength: f32,
    ) -> Result<DiffusionResult> {
        request
            .validate()
            .map_err(|e| anyhow::anyhow!("validation failed: {}", e))?;

        let size =
            ImageSize::parse(&request.size).map_err(|e| anyhow::anyhow!("invalid size: {}", e))?;

        let start = std::time::Instant::now();

        let mut body = serde_json::json!({
            "prompt": request.prompt,
            "model": request.model.as_deref().unwrap_or(&self.model_name),
            "size": request.size,
            "n": request.n,
            "response_format": request.response_format,
            "guidance_scale": request.guidance_scale,
            "num_inference_steps": request.steps,
            "image": base64_image,
            "strength": strength,
        });
        if let Some(seed) = request.seed {
            body["seed"] = serde_json::json!(seed);
        }
        if let Some(ref neg) = request.negative_prompt {
            body["negative_prompt"] = serde_json::json!(neg);
        }

        let url = format!("{}/v1/images/generations", self.endpoint);
        debug!("Diffusion generate_with_edit POST {}", url);

        let response = self.client.post(&url).json(&body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "diffusion sidecar returned {}: {}",
                status,
                text
            ));
        }

        let api_response: OpenAIImageResponse = response.json().await?;
        let first = api_response
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty response from diffusion sidecar"))?;

        let result_image = first
            .b64_json
            .ok_or_else(|| anyhow::anyhow!("no b64_json in response"))?;

        Ok(DiffusionResult {
            base64_image: result_image,
            model: request
                .model
                .clone()
                .unwrap_or_else(|| self.model_name.clone()),
            processing_time_ms: start.elapsed().as_millis() as u64,
            seed: request.seed.unwrap_or(0),
            width: size.width,
            height: size.height,
            steps: request.steps,
            revised_prompt: first.revised_prompt,
        })
    }

    /// List available models from the diffusion sidecar
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/v1/models", self.endpoint);
        debug!("Diffusion list_models GET {}", url);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "diffusion sidecar returned {}: {}",
                status,
                text
            ));
        }

        let model_list: OpenAIModelList = response.json().await?;
        Ok(model_list.data.into_iter().map(|m| m.id).collect())
    }
}
