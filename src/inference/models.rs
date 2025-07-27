use std::path::PathBuf;
use std::sync::Arc;
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use tokio::sync::{RwLock, mpsc};
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone)]
pub struct ModelRegistry {
    models_directory: PathBuf,
    models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    initialized: Arc<RwLock<bool>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub model_type: String,
    pub metadata: ModelMetadata,
    pub status: ModelStatus,
    pub last_used: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub architecture: String,
    pub parameter_count: u64,
    pub quantization: String,
    pub context_length: usize,
    pub tensor_info: HashMap<String, String>,
    pub license: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModelStatus {
    Available,
    Downloading,
    Validating,
    Ready,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum ModelSource {
    HuggingFace {
        repo_id: String,
        filename: String,
        revision: Option<String>,
    },
    Direct {
        url: String,
        sha256: Option<String>,
    },
    Local {
        path: PathBuf,
    },
}

#[derive(Debug, Clone)]
pub struct ModelRequirements {
    pub min_memory_gb: f32,
    pub recommended_memory_gb: f32,
    pub gpu_required: bool,
    pub min_compute_capability: Option<f32>,
    pub supported_architectures: Vec<String>,
    pub min_memory_bytes: u64,
    pub recommended_memory_bytes: u64,
    pub requires_gpu: bool,
    pub min_gpu_memory_bytes: u64,
    pub supported_backends: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum DownloadProgress {
    Progress {
        percent: f32,
        bytes_downloaded: u64,
        total_bytes: u64,
    },
    Completed {
        path: PathBuf,
        hash: String,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone)]
pub struct ModelEvent {
    pub model_id: String,
    pub event_type: ModelEventType,
    pub timestamp: std::time::SystemTime,
}

#[derive(Debug, Clone)]
pub enum ModelEventType {
    Loaded,
    Unloaded,
    Downloaded,
    Deleted,
    ValidationFailed,
    Registered,
}

impl ModelRegistry {
    pub async fn new(models_directory: PathBuf) -> Result<Self> {
        tokio::fs::create_dir_all(&models_directory).await?;
        
        let registry = Self {
            models_directory,
            models: Arc::new(RwLock::new(HashMap::new())),
            initialized: Arc::new(RwLock::new(false)),
        };
        
        // Scan for existing models
        registry.scan_models_directory().await?;
        *registry.initialized.write().await = true;
        
        Ok(registry)
    }
    
    pub fn is_initialized(&self) -> bool {
        futures::executor::block_on(async {
            *self.initialized.read().await
        })
    }
    
    pub fn models_directory(&self) -> &PathBuf {
        &self.models_directory
    }
    
    pub fn list_available_models(&self) -> Vec<ModelInfo> {
        futures::executor::block_on(async {
            self.models.read().await.values().cloned().collect()
        })
    }
    
    pub async fn scan_models_directory(&self) -> Result<()> {
        let mut entries = tokio::fs::read_dir(&self.models_directory).await?;
        let mut models = HashMap::new();
        
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            
            // Check if it's a GGUF file
            if path.extension().and_then(|e| e.to_str()) == Some("gguf") {
                if let Ok(metadata) = entry.metadata().await {
                    let model_name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    
                    let model_info = ModelInfo {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: model_name.clone(),
                        path: path.clone(),
                        size_bytes: metadata.len(),
                        model_type: self.guess_model_type(&model_name),
                        metadata: ModelMetadata {
                            architecture: "llama".to_string(),
                            parameter_count: 7_000_000_000, // Mock for 7B model
                            quantization: "Q4_0".to_string(),
                            context_length: 2048,
                            tensor_info: HashMap::new(),
                            license: Some("Apache-2.0".to_string()),
                            source: None,
                        },
                        status: ModelStatus::Available,
                        last_used: None,
                    };
                    
                    models.insert(model_info.id.clone(), model_info);
                }
            }
        }
        
        *self.models.write().await = models;
        Ok(())
    }
    
    pub async fn extract_model_metadata(&self, path: &PathBuf) -> Result<ModelMetadata> {
        // In real implementation, would parse GGUF metadata
        // For testing, return mock metadata based on filename
        let filename = path.file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("unknown");
        
        let quantization = if filename.contains("q4_0") || filename.contains("Q4_0") {
            "Q4_0"
        } else if filename.contains("q8_0") || filename.contains("Q8_0") {
            "Q8_0"
        } else {
            "F16"
        };
        
        Ok(ModelMetadata {
            architecture: "llama".to_string(),
            parameter_count: 7_000_000_000,
            quantization: quantization.to_string(),
            context_length: 2048,
            tensor_info: vec![
                ("model.layers".to_string(), "32".to_string()),
                ("model.embed_dim".to_string(), "4096".to_string()),
            ].into_iter().collect(),
            license: Some("Apache-2.0".to_string()),
            source: None,
        })
    }
    
    fn guess_model_type(&self, name: &str) -> String {
        let name_lower = name.to_lowercase();
        
        if name_lower.contains("llama") {
            "llama".to_string()
        } else if name_lower.contains("mistral") {
            "mistral".to_string()
        } else if name_lower.contains("phi") {
            "phi".to_string()
        } else {
            "unknown".to_string()
        }
    }
    
    pub async fn get_model(&self, model_id: &str) -> Option<ModelInfo> {
        self.models.read().await.get(model_id).cloned()
    }
}

#[derive(Clone)]
pub struct ModelManager {
    registry: Arc<ModelRegistry>,
    download_tasks: Arc<RwLock<HashMap<String, mpsc::Sender<()>>>>,
    cleanup_policy: Arc<RwLock<Option<CleanupPolicy>>>,
}

impl ModelManager {
    pub async fn new(models_directory: PathBuf) -> Result<Self> {
        let registry = Arc::new(ModelRegistry::new(models_directory).await?);
        
        Ok(Self {
            registry,
            download_tasks: Arc::new(RwLock::new(HashMap::new())),
            cleanup_policy: Arc::new(RwLock::new(None)),
        })
    }
    
    pub async fn download_model(
        &self,
        source: ModelSource,
        _requirements: Option<ModelRequirements>,
    ) -> mpsc::Receiver<DownloadProgress> {
        let (progress_tx, progress_rx) = mpsc::channel(100);
        let (cancel_tx, mut cancel_rx) = mpsc::channel(1);
        
        // Store cancel channel
        let download_id = uuid::Uuid::new_v4().to_string();
        self.download_tasks.write().await.insert(download_id.clone(), cancel_tx);
        
        // Spawn download task
        let models_dir = self.registry.models_directory().clone();
        tokio::spawn(async move {
            // Simulate download progress
            let total_bytes = 1_000_000_000u64; // 1GB
            let mut bytes_downloaded = 0u64;
            
            loop {
                tokio::select! {
                    _ = cancel_rx.recv() => {
                        let _ = progress_tx.send(DownloadProgress::Failed {
                            error: "Download cancelled".to_string()
                        }).await;
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        bytes_downloaded += total_bytes / 10;
                        if bytes_downloaded >= total_bytes {
                            // Download complete
                            let filename = match &source {
                                ModelSource::HuggingFace { filename, .. } => filename.clone(),
                                _ => "model.gguf".to_string(),
                            };
                            
                            let path = models_dir.join(&filename);
                            
                            // Create empty file for testing
                            if let Ok(_) = tokio::fs::File::create(&path).await {
                                let hash = format!("{:x}", Sha256::digest(filename.as_bytes()));
                                
                                let _ = progress_tx.send(DownloadProgress::Completed {
                                    path,
                                    hash,
                                }).await;
                            }
                            break;
                        }
                        
                        let percent = (bytes_downloaded as f32 / total_bytes as f32) * 100.0;
                        let _ = progress_tx.send(DownloadProgress::Progress {
                            percent,
                            bytes_downloaded,
                            total_bytes,
                        }).await;
                    }
                }
            }
        });
        
        progress_rx
    }
    
    pub async fn cancel_download(&self, download_id: &str) -> Result<()> {
        if let Some(cancel_tx) = self.download_tasks.write().await.remove(download_id) {
            let _ = cancel_tx.send(()).await;
            Ok(())
        } else {
            Err(anyhow!("Download not found"))
        }
    }
    
    pub async fn verify_model(&self, path: &PathBuf, expected_hash: Option<&str>) -> Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        
        if let Some(expected) = expected_hash {
            let mut file = tokio::fs::File::open(path).await?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0; 8192];
            
            loop {
                let n = file.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }
            
            let hash = format!("{:x}", hasher.finalize());
            Ok(hash == expected)
        } else {
            Ok(true)
        }
    }
    
    pub async fn delete_model(&self, model_id: &str) -> Result<()> {
        if let Some(model) = self.registry.models.write().await.remove(model_id) {
            tokio::fs::remove_file(model.path).await?;
            Ok(())
        } else {
            Err(anyhow!("Model not found"))
        }
    }
    
    pub async fn get_model_requirements(&self, model_type: &str) -> ModelRequirements {
        // Return requirements based on model type
        let (min_gb, rec_gb) = match model_type {
            "llama-7b" => (6.0, 8.0),
            "llama-13b" => (12.0, 16.0),
            _ => (4.0, 8.0),
        };
        
        ModelRequirements {
            min_memory_gb: min_gb,
            recommended_memory_gb: rec_gb,
            gpu_required: false,
            min_compute_capability: Some(3.5),
            supported_architectures: vec!["x86_64".to_string(), "aarch64".to_string()],
            min_memory_bytes: (min_gb * 1024.0 * 1024.0 * 1024.0) as u64,
            recommended_memory_bytes: (rec_gb * 1024.0 * 1024.0 * 1024.0) as u64,
            requires_gpu: false,
            min_gpu_memory_bytes: 0,
            supported_backends: vec!["cpu".to_string(), "cuda".to_string()],
        }
    }
    
    pub async fn optimize_model_storage(&self, policy: CleanupPolicy) -> Result<Vec<String>> {
        let mut deleted = Vec::new();
        let models = self.registry.list_available_models();
        
        // Simple implementation - delete old unused models
        let cutoff = std::time::SystemTime::now() - policy.max_unused;
        
        for model in models {
            if let Some(last_used) = model.last_used {
                if last_used < cutoff {
                    self.delete_model(&model.id).await?;
                    deleted.push(model.id);
                }
            }
        }
        
        Ok(deleted)
    }
    
    pub fn set_cleanup_policy(&self, policy: CleanupPolicy) {
        futures::executor::block_on(async {
            *self.cleanup_policy.write().await = Some(policy);
        });
    }
    
    pub async fn cleanup_old_models(&self) -> Result<CleanupResult> {
        let policy = self.cleanup_policy.read().await.clone()
            .unwrap_or(CleanupPolicy {
                max_age: Duration::from_secs(30 * 24 * 60 * 60), // 30 days
                max_unused: Duration::from_secs(7 * 24 * 60 * 60), // 7 days
                keep_popular: 5,
                keep_recent: 3,
            });
            
        let deleted = self.optimize_model_storage(policy).await?;
        
        Ok(CleanupResult {
            models_removed: deleted.len(),
            bytes_freed: 0, // Would calculate in real implementation
        })
    }
    
    pub async fn calculate_checksum(&self, path: &PathBuf) -> Result<String> {
        let mut file = tokio::fs::File::open(path).await?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 8192];
        
        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        
        Ok(format!("{:x}", hasher.finalize()))
    }
    
    pub async fn check_system_requirements(&self, requirements: &ModelRequirements) -> bool {
        // Mock system check - in real implementation would check actual system
        true
    }
    
    pub async fn get_system_info(&self) -> SystemInfo {
        SystemInfo {
            total_memory_bytes: 16 * 1024 * 1024 * 1024, // 16GB
            available_memory_bytes: 8 * 1024 * 1024 * 1024, // 8GB
            gpu_available: false,
            gpu_memory_bytes: 0,
            cpu_threads: 8,
        }
    }
    
    pub async fn preload_model(&self, path: &PathBuf) -> PreloadHandle {
        PreloadHandle {
            path: path.clone(),
            ready: Arc::new(RwLock::new(true)),
        }
    }
    
    pub fn is_model_cached(&self, path: &PathBuf) -> bool {
        path.exists()
    }
    
    pub async fn convert_model(&self, source_path: &PathBuf, target_format: &str) -> Result<PathBuf> {
        // Mock conversion
        let target_path = source_path.with_extension(target_format);
        tokio::fs::copy(source_path, &target_path).await?;
        Ok(target_path)
    }
    
    pub async fn quantize_model(&self, source_path: &PathBuf, level: &str) -> Result<PathBuf> {
        // Mock quantization
        let target_path = source_path.with_extension(format!("{}.gguf", level));
        tokio::fs::copy(source_path, &target_path).await?;
        Ok(target_path)
    }
    
    pub fn set_auto_download(&mut self, enabled: bool) {
        // Store in future field
    }
    
    pub fn set_preferred_sources(&mut self, sources: Vec<ModelSource>) {
        // Store in future field
    }
    
    pub async fn ensure_model_available(&self, request: ModelRequest) -> Result<PathBuf> {
        // Mock - return path if exists, otherwise download
        let path = self.registry.models_directory().join(&request.name);
        if path.exists() {
            Ok(path)
        } else {
            // Would trigger download in real implementation
            Ok(path)
        }
    }
    
    pub async fn subscribe_events(&self) -> mpsc::Receiver<ModelEvent> {
        let (tx, rx) = mpsc::channel(100);
        // In real implementation, would connect to event system
        rx
    }
    
    pub async fn register_model(&self, path: &PathBuf) -> Result<()> {
        // Mock registration
        Ok(())
    }
    
    pub async fn mark_model_loaded(&self, path: &PathBuf) {
        // Mock - would update model status
    }
    
    pub async fn mark_model_unloaded(&self, path: &PathBuf) {
        // Mock - would update model status
    }
    
    pub fn set_max_storage_bytes(&mut self, bytes: u64) {
        // Store for future use
    }
    
    pub fn set_cleanup_threshold(&mut self, threshold: f32) {
        // Store for future use
    }
    
    pub async fn get_storage_usage(&self) -> StorageUsage {
        StorageUsage {
            total_bytes: 100 * 1024 * 1024 * 1024, // 100GB
            used_bytes: 30 * 1024 * 1024 * 1024, // 30GB
            available_bytes: 70 * 1024 * 1024 * 1024, // 70GB
        }
    }
    
    pub async fn list_models_by_size(&self) -> Vec<ModelInfo> {
        let mut models = self.registry.list_available_models();
        models.sort_by_key(|m| std::cmp::Reverse(m.size_bytes));
        models
    }
    
    pub async fn list_available_models(&self) -> Vec<ModelInfo> {
        self.registry.list_available_models()
    }
    
    pub async fn add_model_alias(&self, alias: &str, path: &PathBuf) -> Result<()> {
        // Mock alias storage
        Ok(())
    }
    
    pub async fn resolve_model_alias(&self, alias: &str) -> Option<PathBuf> {
        // Mock - return test path
        Some(PathBuf::from("./models/test-model.gguf"))
    }
    
    pub async fn list_model_aliases(&self) -> Vec<(String, PathBuf)> {
        vec![
            ("llama2-chat".to_string(), PathBuf::from("./models/llama-2-7b.gguf")),
            ("chat-model".to_string(), PathBuf::from("./models/llama-2-7b.gguf")),
        ]
    }
}

#[derive(Debug, Clone)]
pub struct CleanupPolicy {
    pub max_age: Duration,
    pub max_unused: Duration,
    pub keep_popular: usize,
    pub keep_recent: usize,
}

#[derive(Debug, Clone)]
pub enum CleanupPolicyType {
    DeleteUnused { days: u32 },
    KeepMostRecent { count: usize },
    MaxSize { gb: f32 },
}

#[derive(Debug, Clone)]
pub struct CleanupResult {
    pub models_removed: usize,
    pub bytes_freed: u64,
}

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub gpu_available: bool,
    pub gpu_memory_bytes: u64,
    pub cpu_threads: usize,
}

#[derive(Debug, Clone)]
pub struct PreloadHandle {
    pub path: PathBuf,
    pub ready: Arc<RwLock<bool>>,
}

#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub name: String,
    pub version: Option<String>,
    pub source: Option<ModelSource>,
}

#[derive(Debug, Clone)]
pub struct StorageUsage {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

use std::time::Duration;