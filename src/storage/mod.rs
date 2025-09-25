pub mod cbor_compat;
pub mod enhanced_s5_client;
pub mod model_storage;
pub mod result_cache;
pub mod s5_client;

// Re-export main types for convenience
pub use cbor_compat::{
    CborCompat, CborDecoder, CborEncoder, CborError, CompressionType, DirV1, DirV1Entry, S5Metadata,
};

pub use s5_client::{
    S5Backend, S5Client, S5ClientConfig, S5Entry, S5EntryType, S5ListResult, S5Storage,
    S5StorageConfig, StorageError,
};

pub use model_storage::{
    ChunkInfo, ModelFormat, ModelMetadata, ModelStats, ModelStorage, ModelStorageConfig,
    ModelVersion,
};

pub use result_cache::{
    CacheConfig, CacheEntry, CacheStats, EvictionPolicy, ResultCache, StorageInfo,
};

// Re-export Enhanced S5 types
pub use enhanced_s5_client::{EnhancedS5Client, HealthResponse, S5Config, S5File};
