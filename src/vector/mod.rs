// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
pub mod client;
pub mod embeddings;
pub mod semantic_cache;
pub mod storage;
pub mod vector_db_client;

// Re-export commonly used types from client module
pub use client::{
    FilterOperator, FilterValue, SearchOptions, SearchResult, VectorBackend, VectorDBClient,
    VectorDBConfig, VectorEntry, VectorError, VectorId, VectorStats,
};

// Re-export embedding types
pub use embeddings::{
    BatchEmbeddingRequest, Embedding, EmbeddingCache, EmbeddingConfig, EmbeddingError,
    EmbeddingGenerator, EmbeddingModel, TokenizerConfig,
};

// Re-export semantic cache types
pub use semantic_cache::{
    CacheEntry, CacheError, CacheEvictionPolicy, CacheHit, CacheStats, SemanticCache,
    SemanticCacheConfig, SimilarityThreshold,
};

// Re-export storage types
pub use storage::{
    IndexType, MigrationConfig, MigrationStatus, MigrationStatusType, StorageBackend,
    StorageError as VectorStorageError, StorageMetadata, StorageStats, VectorStorage,
    VectorStorageConfig,
};

// Re-export S5 types from main storage module
pub use crate::storage::{S5Backend, S5Client, S5Storage, S5StorageConfig};

// Keep the vector_db_client export
pub use vector_db_client::{VectorDbClient, VectorDbConfig};
