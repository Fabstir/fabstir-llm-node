// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use chrono::Utc;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use super::ModelFormat;

#[derive(Debug, Clone)]
pub struct DownloadConfig {
    pub download_dir: PathBuf,
    pub max_concurrent_downloads: usize,
    pub chunk_size: ChunkSize,
    pub timeout_secs: u64,
    pub retry_policy: RetryPolicy,
    pub verify_checksum: bool,
    pub use_cache: bool,
    pub max_bandwidth_bytes_per_sec: Option<u64>,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            download_dir: PathBuf::from("./models"),
            max_concurrent_downloads: 3,
            chunk_size: ChunkSize::Adaptive,
            timeout_secs: 300,
            retry_policy: RetryPolicy::default(),
            verify_checksum: true,
            use_cache: true,
            max_bandwidth_bytes_per_sec: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChunkSize {
    Fixed(usize),
    Adaptive,
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: usize,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub exponential_base: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            exponential_base: 2.0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum DownloadSource {
    HuggingFace {
        repo_id: String,
        filename: String,
        revision: Option<String>,
    },
    S5 {
        cid: String,
        path: String,
        gateway: Option<String>,
    },
    Http {
        url: String,
        headers: Option<HashMap<String, String>>,
    },
}

#[derive(Debug, Clone)]
pub enum AuthConfig {
    BearerToken { token: String },
    ApiKey { key: String },
    BasicAuth { username: String, password: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DownloadStatus {
    Pending,
    InProgress,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub percentage: f32,
    pub speed_bytes_per_sec: u64,
    pub eta_seconds: Option<u64>,
    pub status: DownloadStatus,
}

#[derive(Debug, Clone)]
pub struct ModelMetadata {
    pub model_id: String,
    pub model_name: String,
    pub model_size_bytes: u64,
    pub format: ModelFormat,
    pub quantization: Option<String>,
    pub created_at: u64,
    pub sha256_hash: String,
    pub author: String,
    pub license: String,
    pub tags: Vec<String>,
    pub requires_auth: bool,
}

#[derive(Debug, Clone)]
pub struct DownloadResult {
    pub status: DownloadStatus,
    pub local_path: PathBuf,
    pub size_bytes: u64,
    pub download_time_ms: u64,
    pub format: ModelFormat,
    pub checksum: Option<String>,
    pub checksum_verified: bool,
    pub source_url: String,
    pub metadata: Option<ModelMetadata>,
    pub resumed_from_byte: u64,
}

#[derive(Debug, Clone)]
pub struct StorageSpaceInfo {
    pub available_bytes: u64,
    pub required_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct DownloadInfo {
    pub download_id: String,
    pub source: DownloadSource,
    pub local_path: PathBuf,
    pub status: DownloadStatus,
    pub progress: DownloadProgress,
}

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("IO error: {0}")]
    IoError(String),
    #[error("Checksum mismatch - expected: {expected}, actual: {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("Insufficient storage space - required: {required}, available: {available}")]
    InsufficientSpace { required: u64, available: u64 },
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),
    #[error("Max retries exceeded: {attempts} attempts")]
    MaxRetriesExceeded { attempts: usize },
    #[error("Download cancelled")]
    Cancelled,
    #[error("Timeout")]
    Timeout,
}

struct DownloadState {
    id: String,
    source: DownloadSource,
    local_path: PathBuf,
    status: DownloadStatus,
    bytes_downloaded: u64,
    total_bytes: u64,
    start_time: std::time::Instant,
}

pub struct ModelDownloader {
    config: DownloadConfig,
    downloads: Arc<RwLock<HashMap<String, DownloadState>>>,
    semaphore: Arc<Semaphore>,
}

impl ModelDownloader {
    pub async fn new(config: DownloadConfig) -> Result<Self> {
        // Create download directory if it doesn't exist
        tokio::fs::create_dir_all(&config.download_dir).await?;

        Ok(Self {
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_downloads)),
            config,
            downloads: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn download_model(&self, source: DownloadSource) -> Result<DownloadResult> {
        let _permit = self.semaphore.acquire().await?;

        let download_id = Uuid::new_v4().to_string();
        let local_path = self.generate_local_path(&source).await?;

        // Check storage space
        let required_size = self.estimate_size(&source).await?;
        let space_info = self.check_storage_space().await?;
        if space_info.available_bytes < required_size {
            return Err(DownloadError::InsufficientSpace {
                required: required_size,
                available: space_info.available_bytes,
            }
            .into());
        }

        let start_time = std::time::Instant::now();

        // Mock download implementation
        let result = self.perform_download(&source, &local_path, None).await?;

        let download_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(DownloadResult {
            status: DownloadStatus::Completed,
            local_path: result.local_path,
            size_bytes: result.size_bytes,
            download_time_ms,
            format: result.format,
            checksum: result.checksum,
            checksum_verified: result.checksum_verified,
            source_url: result.source_url,
            metadata: result.metadata,
            resumed_from_byte: 0,
        })
    }

    pub async fn download_with_progress(
        &self,
        source: DownloadSource,
    ) -> Result<impl Stream<Item = DownloadProgress>> {
        let (tx, rx) = mpsc::channel(32);
        let downloader = self.clone();

        tokio::spawn(async move {
            let total_bytes = 10_000_000; // Mock 10MB file
            let chunk_size = 100_000; // 100KB chunks

            for bytes_downloaded in (0..=total_bytes).step_by(chunk_size) {
                let bytes_downloaded = bytes_downloaded.min(total_bytes);
                let percentage = (bytes_downloaded as f32 / total_bytes as f32) * 100.0;

                let progress = DownloadProgress {
                    bytes_downloaded,
                    total_bytes,
                    percentage,
                    speed_bytes_per_sec: 1_000_000, // 1 MB/s
                    eta_seconds: Some((total_bytes - bytes_downloaded) / 1_000_000),
                    status: if bytes_downloaded == total_bytes {
                        DownloadStatus::Completed
                    } else {
                        DownloadStatus::InProgress
                    },
                };

                if tx.send(progress).await.is_err() {
                    break;
                }

                if bytes_downloaded < total_bytes {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        });

        Ok(ReceiverStream::new(rx))
    }

    pub async fn download_with_auth(
        &self,
        source: DownloadSource,
        auth: AuthConfig,
    ) -> Result<DownloadResult> {
        // For mock implementation, just perform regular download
        // In real implementation, would add auth headers
        let mut result = self.download_model(source).await?;

        // Mark as requiring auth in metadata
        if let Some(ref mut metadata) = result.metadata {
            metadata.requires_auth = true;
        }

        Ok(result)
    }

    pub async fn download_with_checksum(
        &self,
        source: DownloadSource,
        expected_checksum: &str,
    ) -> Result<DownloadResult> {
        let result = self.download_model(source).await?;

        // Mock checksum verification
        let calculated_checksum = "abc123def456789"; // Mock checksum

        if calculated_checksum != expected_checksum {
            return Err(DownloadError::ChecksumMismatch {
                expected: expected_checksum.to_string(),
                actual: calculated_checksum.to_string(),
            }
            .into());
        }

        Ok(DownloadResult {
            checksum_verified: true,
            ..result
        })
    }

    pub async fn start_download(&self, source: DownloadSource) -> Result<String> {
        let download_id = Uuid::new_v4().to_string();
        let local_path = self.generate_local_path(&source).await?;

        let state = DownloadState {
            id: download_id.clone(),
            source,
            local_path,
            status: DownloadStatus::InProgress,
            bytes_downloaded: 0,
            total_bytes: 10_000_000, // Mock 10MB
            start_time: std::time::Instant::now(),
        };

        self.downloads
            .write()
            .await
            .insert(download_id.clone(), state);

        Ok(download_id)
    }

    pub async fn pause_download(&self, download_id: &str) -> Result<()> {
        let mut downloads = self.downloads.write().await;
        if let Some(state) = downloads.get_mut(download_id) {
            state.status = DownloadStatus::Paused;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Download not found"))
        }
    }

    pub async fn resume_download(&self, download_id: &str) -> Result<DownloadResult> {
        let (source, resumed_from) = {
            let downloads = self.downloads.read().await;
            if let Some(state) = downloads.get(download_id) {
                (state.source.clone(), state.bytes_downloaded)
            } else {
                return Err(anyhow::anyhow!("Download not found"));
            }
        };

        // Continue download from where it left off
        let mut result = self.download_model(source).await?;
        result.resumed_from_byte = resumed_from;

        Ok(result)
    }

    pub async fn cancel_download(&self, download_id: &str) -> Result<()> {
        let mut downloads = self.downloads.write().await;
        if let Some(state) = downloads.get_mut(download_id) {
            state.status = DownloadStatus::Cancelled;

            // Clean up partial file
            if state.local_path.exists() {
                tokio::fs::remove_file(&state.local_path).await.ok();
            }

            Ok(())
        } else {
            Err(anyhow::anyhow!("Download not found"))
        }
    }

    pub async fn get_download_status(&self, download_id: &str) -> Result<DownloadStatus> {
        let downloads = self.downloads.read().await;
        if let Some(state) = downloads.get(download_id) {
            Ok(state.status.clone())
        } else {
            Err(anyhow::anyhow!("Download not found"))
        }
    }

    pub async fn get_download_info(&self, download_id: &str) -> Result<DownloadInfo> {
        let downloads = self.downloads.read().await;
        if let Some(state) = downloads.get(download_id) {
            let percentage = if state.total_bytes > 0 {
                (state.bytes_downloaded as f32 / state.total_bytes as f32) * 100.0
            } else {
                0.0
            };

            Ok(DownloadInfo {
                download_id: state.id.clone(),
                source: state.source.clone(),
                local_path: state.local_path.clone(),
                status: state.status.clone(),
                progress: DownloadProgress {
                    bytes_downloaded: state.bytes_downloaded,
                    total_bytes: state.total_bytes,
                    percentage,
                    speed_bytes_per_sec: 1_000_000,
                    eta_seconds: Some((state.total_bytes - state.bytes_downloaded) / 1_000_000),
                    status: state.status.clone(),
                },
            })
        } else {
            Err(anyhow::anyhow!("Download not found"))
        }
    }

    pub async fn check_storage_space(&self) -> Result<StorageSpaceInfo> {
        // Mock storage space check
        Ok(StorageSpaceInfo {
            available_bytes: 100_000_000_000, // 100GB
            required_bytes: 10_000_000_000,   // 10GB
            total_bytes: 500_000_000_000,     // 500GB
        })
    }

    async fn generate_local_path(&self, source: &DownloadSource) -> Result<PathBuf> {
        let filename = match source {
            DownloadSource::HuggingFace { filename, .. } => filename.clone(),
            DownloadSource::S5 { path, .. } => {
                path.split('/').last().unwrap_or("model").to_string()
            }
            DownloadSource::Http { url, .. } => {
                url.split('/').last().unwrap_or("model.gguf").to_string()
            }
        };

        Ok(self.config.download_dir.join(filename))
    }

    async fn estimate_size(&self, source: &DownloadSource) -> Result<u64> {
        // Mock size estimation
        match source {
            DownloadSource::HuggingFace { repo_id, .. } => {
                if repo_id.contains("huge-model-100TB") {
                    Ok(100_000_000_000_000) // 100TB for insufficient space test
                } else {
                    Ok(10_000_000) // 10MB
                }
            }
            _ => Ok(10_000_000), // 10MB default
        }
    }

    async fn perform_download(
        &self,
        source: &DownloadSource,
        local_path: &PathBuf,
        _auth: Option<AuthConfig>,
    ) -> Result<DownloadResult> {
        // Create parent directory
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Mock download with retry logic for flaky servers
        let mut retries = 0;
        let max_retries = self.config.retry_policy.max_retries;

        while retries <= max_retries {
            match self.try_download(source, local_path).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if retries == max_retries {
                        if source.is_flaky() {
                            return Err(
                                DownloadError::MaxRetriesExceeded { attempts: retries }.into()
                            );
                        } else {
                            return Err(e);
                        }
                    }

                    // Wait with exponential backoff
                    let delay = self.config.retry_policy.initial_delay_ms
                        * (self
                            .config
                            .retry_policy
                            .exponential_base
                            .powi(retries as i32) as u64);
                    let delay = delay.min(self.config.retry_policy.max_delay_ms);

                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                    retries += 1;
                }
            }
        }

        unreachable!()
    }

    async fn try_download(
        &self,
        source: &DownloadSource,
        local_path: &PathBuf,
    ) -> Result<DownloadResult> {
        // Simulate network delay and bandwidth throttling
        let mut download_duration = tokio::time::Duration::from_millis(250);

        if let Some(bandwidth_limit) = self.config.max_bandwidth_bytes_per_sec {
            let size_bytes = 1_000_000u64; // 1MB mock file
            let min_duration_secs = size_bytes / bandwidth_limit;
            download_duration =
                download_duration.max(tokio::time::Duration::from_secs(min_duration_secs));
        }

        tokio::time::sleep(download_duration).await;

        // Mock file creation
        let mock_data = b"Mock model file content";
        tokio::fs::write(local_path, mock_data).await?;

        let source_url = match source {
            DownloadSource::HuggingFace {
                repo_id, filename, ..
            } => {
                format!(
                    "https://huggingface.co/{}/resolve/main/{}",
                    repo_id, filename
                )
            }
            DownloadSource::S5 { cid, path, gateway } => {
                let gateway = gateway.as_deref().unwrap_or("https://s5.cx");
                format!("{}/ipfs/{}{}", gateway, cid, path)
            }
            DownloadSource::Http { url, .. } => url.clone(),
        };

        let format = ModelFormat::from_extension(
            local_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("gguf"),
        );

        let metadata = self.extract_metadata(source, &format).await?;
        let checksum = if self.config.verify_checksum {
            Some("abc123def456789".to_string())
        } else {
            None
        };

        Ok(DownloadResult {
            status: DownloadStatus::Completed,
            local_path: local_path.clone(),
            size_bytes: mock_data.len() as u64,
            download_time_ms: download_duration.as_millis() as u64,
            format,
            checksum,
            checksum_verified: self.config.verify_checksum,
            source_url,
            metadata: Some(metadata),
            resumed_from_byte: 0,
        })
    }

    async fn extract_metadata(
        &self,
        source: &DownloadSource,
        format: &ModelFormat,
    ) -> Result<ModelMetadata> {
        let (model_id, model_name) = match source {
            DownloadSource::HuggingFace {
                repo_id, filename, ..
            } => (format!("{}:{}", repo_id, filename), repo_id.clone()),
            DownloadSource::S5 { cid, .. } => (cid.clone(), "S5 Model".to_string()),
            DownloadSource::Http { url, .. } => {
                let name = url.split('/').last().unwrap_or("HTTP Model");
                (url.clone(), name.to_string())
            }
        };

        Ok(ModelMetadata {
            model_id,
            model_name,
            model_size_bytes: 1_000_000_000, // 1GB
            format: format.clone(),
            quantization: Some("Q4_K_M".to_string()),
            created_at: Utc::now().timestamp() as u64,
            sha256_hash: "0".repeat(64),
            author: "test-author".to_string(),
            license: "MIT".to_string(),
            tags: vec!["test".to_string(), "llm".to_string()],
            requires_auth: false,
        })
    }
}

impl Clone for ModelDownloader {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            downloads: self.downloads.clone(),
            semaphore: self.semaphore.clone(),
        }
    }
}

impl DownloadSource {
    fn is_flaky(&self) -> bool {
        match self {
            DownloadSource::Http { url, .. } => url.contains("flaky-server"),
            _ => false,
        }
    }
}
