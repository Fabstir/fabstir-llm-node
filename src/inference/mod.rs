// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Export all submodules and their public types
pub mod cache;
pub mod chat_template;
pub mod engine;
pub mod format;
pub mod models;

// Re-export main types for convenience
pub use chat_template::ChatTemplate;
pub use engine::{
    ChatMessage, EngineCapabilities, EngineConfig, EngineMetrics, InferenceHandle,
    InferenceRequest, InferenceResult, LlmEngine, Model, ModelCapabilities, ModelCapability,
    ModelConfig, TokenInfo, TokenStream,
};

// Create alias for all uses (tests expect this name)
pub use cache::{
    CacheConfig, CacheEntry, CacheKey, CacheStats, EvictionPolicy, InferenceCache, SemanticCache,
};
pub use engine::LlmEngine as InferenceEngine;
pub use format::{
    Citation, ContentFilter, FormatConfig, OutputFormat, ResultFormatter, SafetyCheck,
};
pub use models::{
    CleanupPolicy, CleanupResult, DownloadProgress, ModelEvent, ModelEventType, ModelInfo,
    ModelManager, ModelMetadata, ModelRegistry, ModelRequest, ModelRequirements, ModelSource,
    ModelStatus, PreloadHandle, StorageUsage, SystemInfo,
};
