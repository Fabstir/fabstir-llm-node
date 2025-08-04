// src/models/mod.rs - Example structure for Claude Code

pub mod downloading;
pub mod validation;
pub mod caching;
pub mod updates;
pub mod finetuned;
pub mod private;
pub mod gdpr;
pub mod specialization;

// Re-export downloading types
pub use downloading::{
    ModelDownloader, DownloadConfig, DownloadSource,
    DownloadProgress, DownloadResult, DownloadError, DownloadStatus,
    ModelMetadata, ChunkSize, RetryPolicy, AuthConfig
};

// Re-export validation types
pub use validation::{
    ModelValidator, ValidationConfig, ValidationResult, ValidationStatus,
    ModelInfo, ValidationError, CompatibilityCheck,
    ModelRequirements, HardwareRequirements, ValidationLevel,
    IntegrityCheck, FormatCheck, SchemaVersion, SecurityResult,
    PerformanceCharacteristics, InferenceCompatibility, BatchValidationResult,
    ModelMetadata as ValidationModelMetadata, QuantizationInfo, CompatibilityResult, SecurityValidationResult
};

// Re-export caching types  
pub use caching::{
    ModelCache, CacheConfig, CacheEntry, CacheStatus, CacheError,
    EvictionPolicy, CacheMetrics, PersistenceConfig, CacheEvent,
    ModelHandle, CachePriority, WarmupStrategy, WarmupResult,
    ModelMetrics, CompressionInfo
};

// Re-export update types
pub use updates::{
    ModelUpdater, UpdateConfig, UpdateStrategy, UpdateResult,
    UpdateStatus, ModelVersion, UpdateSource, UpdateError,
    RollbackPolicy, UpdateMetadata, VersionComparison,
    UpdateNotification, MigrationPlan, UpdateSchedule,
    RecoveryInfo, UpdateInfo, CleanupResult, MigrationStep,
    BatchUpdateResult, UpdateTracking
};

// Re-export fine-tuned types
pub use finetuned::{
    FineTunedManager, FineTunedConfig, FineTunedModel, BaseModel,
    FineTuneMetadata, FineTuneType, ModelAdapter, AdapterConfig,
    FineTuneRegistry, FineTuneStatus, FineTuneCapabilities,
    ModelMerger, MergeStrategy, FineTuneValidator, 
    ValidationResult as FineTuneValidationResult,
    ValidationLevel as FineTuneValidationLevel, 
    InferenceSession, GenerationConfig, GenerationResponse,
};

// Re-export private model types
pub use private::{
    PrivateModelManager, PrivateModelConfig, PrivateModel, AccessLevel,
    ModelOwner, AccessToken, ModelLicense, LicenseType, UsagePolicy,
    PrivateModelRegistry, ModelVisibility, SharingSettings, AuditLog,
    EncryptionConfig, StorageIsolation, AccessControl, RateLimits,
    ExportPolicy, StorageInfo, UsageStats, LicenseAcceptance,
    IsolatedSession, ApiSession,
};

// Re-export GDPR compliance types
pub use gdpr::{
    DecentralizedGdprManager, GdprConfig, UserKeys, SignedRequest,
    ConsentRecord, DeletionBroadcast, PortableDataPackage, 
    RegionalPreference, ZkComplianceProof, EncryptedData,
    P2PGdprNetwork, OnChainConsent, UserControlledData,
    AnonymizationProof, AuditProof, ComplianceAttestation,
};

// Re-export specialization types
pub use specialization::{
    SpecializationManager, SpecializationConfig, ModelSpecialization,
    DomainType, TaskType, LanguageSupport, IndustryVertical,
    SpecializedModel, SpecializationMetrics, TokenizerConfig,
    InferencePipeline, EnsembleStrategy, BenchmarkResult,
    QueryAnalyzer, CostOptimizer, SpecializationMarketplace,
    PerformanceProfile, AccuracyRequirement, SpecializedRouter,
    RegistrationResult, QueryAnalysis, TokenizationResult, PipelineResult,
    EnsembleInfo, EnsembleResult, DetectionResult, CostProfile,
    CostRequirements, CostOptimalModel, MarketplaceListing, PricingModel,
    MarketplaceRatings, SearchCriteria, TransactionType, Transaction,
};

// Common types used across modules
#[derive(Debug, Clone, PartialEq)]
pub enum ModelFormat {
    GGUF,
    ONNX,
    SafeTensors,
    PyTorch,
    TensorFlow,
    Unknown,
}

impl ModelFormat {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "gguf" => ModelFormat::GGUF,
            "onnx" => ModelFormat::ONNX,
            "safetensors" => ModelFormat::SafeTensors,
            "pt" | "pth" => ModelFormat::PyTorch,
            "pb" => ModelFormat::TensorFlow,
            _ => ModelFormat::Unknown,
        }
    }
    
    pub fn to_extension(&self) -> &str {
        match self {
            ModelFormat::GGUF => "gguf",
            ModelFormat::ONNX => "onnx",
            ModelFormat::SafeTensors => "safetensors",
            ModelFormat::PyTorch => "pt",
            ModelFormat::TensorFlow => "pb",
            ModelFormat::Unknown => "bin",
        }
    }
}

// Model registry for tracking all models
pub struct ModelRegistry {
    models: std::collections::HashMap<String, ModelEntry>,
}

#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub format: ModelFormat,
    pub version: ModelVersion,
    pub path: std::path::PathBuf,
    pub size_bytes: u64,
    pub checksum: String,
    pub last_accessed: u64,
    pub cache_priority: CachePriority,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self {
            models: std::collections::HashMap::new(),
        }
    }
    
    pub fn register(&mut self, entry: ModelEntry) {
        self.models.insert(entry.id.clone(), entry);
    }
    
    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.models.get(id)
    }
    
    pub fn list(&self) -> Vec<&ModelEntry> {
        self.models.values().collect()
    }
}

// Utility functions
pub fn calculate_model_hash(data: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

pub fn estimate_memory_usage(model_size_bytes: u64, format: &ModelFormat) -> u64 {
    // Rough estimates based on format
    match format {
        ModelFormat::GGUF => model_size_bytes * 1,  // Already optimized
        ModelFormat::ONNX => model_size_bytes * 2,  // Needs runtime memory
        ModelFormat::SafeTensors => model_size_bytes * 1,
        ModelFormat::PyTorch => model_size_bytes * 3,  // Python overhead
        ModelFormat::TensorFlow => model_size_bytes * 3,
        ModelFormat::Unknown => model_size_bytes * 2,
    }
}

// Version information
pub const MODEL_MANAGER_VERSION: &str = "0.1.0";

// Error types are already re-exported above

// Testing utilities
#[cfg(test)]
pub mod test_utils {
    use super::*;
    use std::path::PathBuf;
    
    pub fn create_mock_model_file(path: &PathBuf, size_mb: u64) {
        std::fs::create_dir_all(path.parent().unwrap()).ok();
        let data = vec![0u8; (size_mb * 1024 * 1024) as usize];
        std::fs::write(path, data).ok();
    }
    
    pub fn create_mock_model_metadata() -> ModelMetadata {
        ModelMetadata {
            model_id: "test-model".to_string(),
            model_name: "Test Model".to_string(),
            model_size_bytes: 1_000_000_000,
            format: ModelFormat::GGUF,
            quantization: Some("Q4_K_M".to_string()),
            created_at: chrono::Utc::now().timestamp() as u64,
            sha256_hash: "0".repeat(64),
            author: "test".to_string(),
            license: "MIT".to_string(),
            tags: vec!["test".to_string()],
            requires_auth: false,
        }
    }
}