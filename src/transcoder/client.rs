// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! TranscoderClient — HTTP client for the fabstir-transcoder sidecar.

use anyhow::Result;
use reqwest::Client;
use std::time::Duration;
use tracing::debug;

use super::types::{SidecarStatus, TranscodeStatusResponse, TranscodeSubmitResponse, VideoFormat};

/// Client for the fabstir-transcoder sidecar REST API.
///
/// Uses a pre-shared JWT token (generated once via `generate-token` binary,
/// shared via `FABSTIR_TRANSCODER_JWT` env var to both containers).
pub struct TranscoderClient {
    client: Client,
    endpoint: String,
    jwt_token: String,
}

impl TranscoderClient {
    /// Create a new TranscoderClient with a pre-shared JWT token.
    pub fn new(endpoint: &str, jwt_token: &str) -> Result<Self> {
        if jwt_token.is_empty() {
            return Err(anyhow::anyhow!("FABSTIR_TRANSCODER_JWT cannot be empty"));
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?;
        let endpoint = endpoint.trim_end_matches('/').to_string();

        Ok(Self {
            client,
            endpoint,
            jwt_token: jwt_token.to_string(),
        })
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

    /// Cancel a running transcode task. Returns `true` if cancelled, `false` if
    /// the endpoint is not supported (404).
    pub async fn cancel_transcode(&self, task_id: &str) -> Result<bool> {
        let url = format!("{}/transcode/{}/cancel", self.endpoint, task_id);
        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.jwt_token)
            .timeout(Duration::from_secs(10))
            .send()
            .await?;
        match response.status().as_u16() {
            200 => Ok(true),
            404 => Ok(false),
            s => Err(anyhow::anyhow!("cancel returned status {}", s)),
        }
    }

    /// Get the sidecar's current status (active/queued jobs, max concurrent).
    pub async fn get_sidecar_status(&self) -> Result<SidecarStatus> {
        let url = format!("{}/status", self.endpoint);
        let response = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .bearer_auth(&self.jwt_token)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "sidecar status returned {}: {}",
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
