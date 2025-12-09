// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// src/models/mod.rs

pub mod caching;
pub mod downloading;
pub mod finetuned;
pub mod gdpr;
pub mod private;
pub mod specialization;
pub mod updates;
pub mod validation;

// Re-export downloading types
pub use downloading::{
    AuthConfig, ChunkSize, DownloadConfig, DownloadError, DownloadProgress, DownloadResult,
    DownloadSource, DownloadStatus, ModelDownloader, ModelMetadata, RetryPolicy,
};

// Re-export validation types
pub use validation::{
    BatchValidationResult, CompatibilityCheck, CompatibilityResult, FormatCheck,
    HardwareRequirements, InferenceCompatibility, IntegrityCheck, ModelInfo,
    ModelMetadata as ValidationModelMetadata, ModelRequirements, ModelValidator,
    PerformanceCharacteristics, QuantizationInfo, SchemaVersion, SecurityResult,
    SecurityValidationResult, ValidationConfig, ValidationError, ValidationLevel, ValidationResult,
    ValidationStatus,
};

// Re-export caching types
pub use caching::{
    CacheConfig, CacheEntry, CacheError, CacheEvent, CacheMetrics, CachePriority, CacheStatus,
    CompressionInfo, EvictionPolicy, ModelCache, ModelHandle, ModelMetrics, PersistenceConfig,
    WarmupResult, WarmupStrategy,
};

// Re-export update types
pub use updates::{
    BatchUpdateResult, CleanupResult, MigrationPlan, MigrationStep, ModelUpdater, ModelVersion,
    RecoveryInfo, RollbackPolicy, UpdateConfig, UpdateError, UpdateInfo, UpdateMetadata,
    UpdateNotification, UpdateResult, UpdateSchedule, UpdateSource, UpdateStatus, UpdateStrategy,
    UpdateTracking, VersionComparison,
};

// Re-export fine-tuned types
pub use finetuned::{
    AdapterConfig, BaseModel, FineTuneCapabilities, FineTuneMetadata, FineTuneRegistry,
    FineTuneStatus, FineTuneType, FineTuneValidator, FineTunedConfig, FineTunedManager,
    FineTunedModel, GenerationConfig, GenerationResponse, InferenceSession, MergeStrategy,
    ModelAdapter, ModelMerger, ValidationLevel as FineTuneValidationLevel,
    ValidationResult as FineTuneValidationResult,
};

// Re-export private model types
pub use private::{
    AccessControl, AccessLevel, AccessToken, ApiSession, AuditLog, EncryptionConfig, ExportPolicy,
    IsolatedSession, LicenseAcceptance, LicenseType, ModelLicense, ModelOwner, ModelVisibility,
    PrivateModel, PrivateModelConfig, PrivateModelManager, PrivateModelRegistry, RateLimits,
    SharingSettings, StorageInfo, StorageIsolation, UsagePolicy, UsageStats,
};

// Re-export GDPR compliance types
pub use gdpr::{
    AnonymizationProof, AuditProof, ComplianceAttestation, ConsentRecord, DecentralizedGdprManager,
    DeletionBroadcast, EncryptedData, GdprConfig, OnChainConsent, P2PGdprNetwork,
    PortableDataPackage, RegionalPreference, SignedRequest, UserControlledData, UserKeys,
    ZkComplianceProof,
};

// Re-export specialization types
pub use specialization::{
    AccuracyRequirement, BenchmarkResult, CostOptimalModel, CostOptimizer, CostProfile,
    CostRequirements, DetectionResult, DomainType, EnsembleInfo, EnsembleResult, EnsembleStrategy,
    IndustryVertical, InferencePipeline, LanguageSupport, MarketplaceListing, MarketplaceRatings,
    ModelSpecialization, PerformanceProfile, PipelineResult, PricingModel, QueryAnalysis,
    QueryAnalyzer, RegistrationResult, SearchCriteria, SpecializationConfig, SpecializationManager,
    SpecializationMarketplace, SpecializationMetrics, SpecializedModel, SpecializedRouter,
    TaskType, TokenizationResult, TokenizerConfig, Transaction, TransactionType,
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

// Basic structs for E2E workflow tests
// ModelRegistry is defined below with actual implementation

#[derive(Debug, Clone)]
pub struct ModelConfig;

// Note: ModelMetadata is already defined in downloading module,
// but we'll re-export it at the top level for convenience
// It's already re-exported above from the downloading module

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
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

pub fn estimate_memory_usage(model_size_bytes: u64, format: &ModelFormat) -> u64 {
    // Rough estimates based on format
    match format {
        ModelFormat::GGUF => model_size_bytes * 1, // Already optimized
        ModelFormat::ONNX => model_size_bytes * 2, // Needs runtime memory
        ModelFormat::SafeTensors => model_size_bytes * 1,
        ModelFormat::PyTorch => model_size_bytes * 3, // Python overhead
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
