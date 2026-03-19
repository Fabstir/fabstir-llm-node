// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! TranscoderClient — HTTP client for the fabstir-transcoder sidecar.

use anyhow::Result;
use jsonwebtoken::{encode, EncodingKey, Header};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

use super::types::{TranscodeStatusResponse, TranscodeSubmitResponse, VideoFormat};

/// JWT claims for transcoder auth.
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iat: u64,
    exp: u64,
}

/// Client for the fabstir-transcoder sidecar REST API.
pub struct TranscoderClient {
    client: Client,
    endpoint: String,
    jwt_token: String,
}

impl TranscoderClient {
    /// Create a new TranscoderClient.
    pub fn new(endpoint: &str, secret_key: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?;
        let endpoint = endpoint.trim_end_matches('/').to_string();
        let jwt_token = Self::generate_jwt(secret_key)?;

        Ok(Self {
            client,
            endpoint,
            jwt_token,
        })
    }

    /// Generate a JWT token for transcoder auth (HS256, 24h expiry).
    pub fn generate_jwt(secret_key: &str) -> Result<String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let claims = Claims {
            iat: now,
            exp: now + 86400, // 24 hours
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret_key.as_bytes()),
        )?;
        Ok(token)
    }

    /// Check if the transcoder sidecar is healthy.
    pub async fn health_check(&self) -> bool {
        match self
            .client
            .get(format!("{}/health", self.endpoint))
            .timeout(Duration::from_secs(30))
            .send()
            .await
        {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                debug!("Transcoder health check failed: {}", e);
                false
            }
        }
    }

    /// Submit a transcode job.
    pub async fn submit_transcode(
        &self,
        source_cid: &str,
        formats: &[VideoFormat],
        is_encrypted: bool,
        is_gpu: bool,
    ) -> Result<TranscodeSubmitResponse> {
        let url = format!("{}/transcode", self.endpoint);
        let media_formats_json = serde_json::to_string(formats)?;

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.jwt_token)
            .query(&[
                ("source_cid", source_cid),
                ("media_formats", &media_formats_json),
                ("is_encrypted", &is_encrypted.to_string()),
                ("is_gpu", &is_gpu.to_string()),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "transcoder sidecar returned {}: {}",
                status,
                text
            ));
        }

        Ok(response.json().await?)
    }

    /// Get the status of a transcode job.
    pub async fn get_status(&self, task_id: &str) -> Result<TranscodeStatusResponse> {
        let url = format!("{}/get_transcoded/{}", self.endpoint, task_id);

        let response = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(30))
            .bearer_auth(&self.jwt_token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "transcoder status check returned {}: {}",
                status,
                text
            ));
        }

        Ok(response.json().await?)
    }

    /// Get the configured endpoint.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Get the model name (always "transcoder").
    pub fn model_name(&self) -> &str {
        "transcoder"
    }
}
