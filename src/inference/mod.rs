// Export all submodules and their public types
pub mod engine;
pub mod models;
pub mod cache;
pub mod format;

// Re-export main types for convenience
pub use engine::{
    LlmEngine, EngineConfig, ModelConfig, InferenceRequest, InferenceResult,
    TokenStream, TokenInfo, Model, ModelCapability, EngineCapabilities, EngineMetrics,
    InferenceHandle, ModelCapabilities, ChatMessage
};

// Create alias for all uses (tests expect this name)
pub use engine::LlmEngine as InferenceEngine;
pub use models::{
    ModelRegistry, ModelManager, ModelInfo, ModelSource, ModelRequirements, 
    ModelStatus, DownloadProgress, ModelEvent, ModelEventType, CleanupPolicy,
    ModelMetadata, CleanupResult, SystemInfo, PreloadHandle, ModelRequest,
    StorageUsage
};
pub use cache::{
    InferenceCache, CacheConfig, CacheEntry, CacheKey, CacheStats, EvictionPolicy,
    SemanticCache
};
pub use format::{
    ResultFormatter, FormatConfig, OutputFormat, Citation, SafetyCheck, ContentFilter
};