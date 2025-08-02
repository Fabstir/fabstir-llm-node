pub mod client;
pub mod embeddings;
pub mod semantic_cache;
pub mod storage;

// Re-export main types for convenience
pub use client::{
    VectorDBClient, VectorDBConfig, VectorBackend, VectorId,
    VectorEntry, SearchOptions, SearchResult, VectorError,
    FilterOperator, FilterValue, VectorStats, HealthStatus,
    InsertResult, BatchInsertResult, UpdateEvent
};

pub use embeddings::{
    EmbeddingGenerator, EmbeddingConfig, EmbeddingModel,
    Embedding, EmbeddingError, TokenizerConfig, 
    BatchEmbeddingRequest, EmbeddingCache, CacheStats,
    TruncationInfo, ModelInfo
};

pub use semantic_cache::{
    SemanticCache, SemanticCacheConfig, CacheEntry, CacheHit,
    SimilarityThreshold, CacheStats as SemanticCacheStats, 
    CacheError, CacheEvictionPolicy, PerformanceMetrics,
    StorageInfo as CacheStorageInfo
};

pub use storage::{
    VectorStorage, VectorStorageConfig, StorageBackend,
    StorageMetadata, StorageError as VectorStorageError,
    MigrationConfig, MigrationStatus, MigrationStatusType, IndexType, StorageStats,
    ChunkInfo, CompressionInfo, BackupResult, RestoreResult,
    BatchStoreResult
};

// Re-export S5 types from storage module
pub use crate::storage::{S5Storage, S5StorageConfig, S5Backend, S5Client};