pub mod client;
pub mod embeddings;
pub mod semantic_cache;
pub mod storage;
pub mod vector_db_client;

// Re-export commonly used types from client module
pub use client::{
    VectorDBClient, VectorDBConfig, VectorBackend, VectorId,
    VectorEntry, SearchOptions, SearchResult, VectorError,
    FilterOperator, FilterValue, VectorStats
};

// Re-export embedding types
pub use embeddings::{
    EmbeddingGenerator, EmbeddingConfig, EmbeddingModel,
    Embedding, EmbeddingError, TokenizerConfig,
    BatchEmbeddingRequest, EmbeddingCache
};

// Re-export semantic cache types
pub use semantic_cache::{
    SemanticCache, SemanticCacheConfig, CacheEntry, CacheHit,
    CacheError, CacheStats, SimilarityThreshold,
    CacheEvictionPolicy
};

// Re-export storage types
pub use storage::{
    VectorStorage, VectorStorageConfig, StorageBackend,
    StorageMetadata, StorageError as VectorStorageError,
    MigrationConfig, MigrationStatus, MigrationStatusType, 
    IndexType, StorageStats
};

// Re-export S5 types from main storage module
pub use crate::storage::{S5Storage, S5StorageConfig, S5Backend, S5Client};

// Keep the vector_db_client export
pub use vector_db_client::VectorDbClient;