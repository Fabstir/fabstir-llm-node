// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// S5 Vector Database Manifest and Chunk structures (Sub-phase 2.2)

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// Manifest file structure for S5 vector databases
///
/// Matches SDK format from S5VectorStore (TypeScript)
/// Stored at: home/vector-databases/{userAddress}/{databaseName}/manifest.json
/// Encrypted with AES-GCM on client side
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Database name
    pub name: String,

    /// Owner's Ethereum address (0x...)
    pub owner: String,

    /// Human-readable description
    pub description: String,

    /// Vector dimensions (e.g., 384 for all-MiniLM-L6-v2)
    pub dimensions: usize,

    /// Total number of vectors across all chunks
    pub vector_count: usize,

    /// Total storage size in bytes
    pub storage_size_bytes: u64,

    /// Creation timestamp (Unix milliseconds)
    pub created: i64,

    /// Last accessed timestamp (Unix milliseconds)
    pub last_accessed: i64,

    /// Last updated timestamp (Unix milliseconds)
    pub updated: i64,

    /// Metadata for each chunk
    pub chunks: Vec<ChunkMetadata>,

    /// Number of chunks (should match chunks.len())
    pub chunk_count: usize,

    /// Folder paths in the database
    pub folder_paths: Vec<String>,

    /// Soft delete flag
    pub deleted: bool,
}

impl Manifest {
    /// Validate manifest structure
    ///
    /// Checks:
    /// - Chunk count matches actual chunks.len()
    /// - Dimensions > 0
    /// - Chunk IDs are sequential starting from 0
    /// - Vector counts sum up correctly
    pub fn validate(&self) -> Result<()> {
        // Validate dimensions
        if self.dimensions == 0 {
            return Err(anyhow!("Invalid dimensions: must be > 0"));
        }

        // Validate chunk count
        if self.chunks.len() != self.chunk_count {
            return Err(anyhow!(
                "Chunk count mismatch: manifest says {} but has {} chunks",
                self.chunk_count,
                self.chunks.len()
            ));
        }

        // Validate chunk IDs are sequential
        for (i, chunk) in self.chunks.iter().enumerate() {
            if chunk.chunk_id != i {
                return Err(anyhow!(
                    "Invalid chunk IDs: expected chunk {} at index {} but found chunk {}",
                    i,
                    i,
                    chunk.chunk_id
                ));
            }
        }

        // Validate vector count sum
        let total_vectors: usize = self.chunks.iter().map(|c| c.vector_count).sum();
        if total_vectors != self.vector_count {
            return Err(anyhow!(
                "Vector count mismatch: manifest says {} but chunks sum to {}",
                self.vector_count,
                total_vectors
            ));
        }

        Ok(())
    }

    /// Get total number of chunks
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Check if database is deleted
    pub fn is_deleted(&self) -> bool {
        self.deleted
    }

    /// Get chunk by ID
    pub fn get_chunk(&self, chunk_id: usize) -> Option<&ChunkMetadata> {
        self.chunks.get(chunk_id)
    }
}

/// Metadata for a single vector chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkMetadata {
    /// Chunk identifier (0-indexed)
    pub chunk_id: usize,

    /// S5 CID for this chunk
    pub cid: String,

    /// Number of vectors in this chunk
    pub vector_count: usize,

    /// Size of chunk file in bytes
    pub size_bytes: u64,

    /// Last update timestamp (Unix milliseconds)
    pub updated_at: i64,
}

impl ChunkMetadata {
    /// Validate chunk metadata
    pub fn validate(&self) -> Result<()> {
        if self.cid.is_empty() {
            return Err(anyhow!("Chunk CID cannot be empty"));
        }

        if self.vector_count == 0 {
            return Err(anyhow!("Chunk must contain at least one vector"));
        }

        Ok(())
    }
}

/// Vector chunk file structure
///
/// Stored at: home/vector-databases/{userAddress}/{databaseName}/chunk-{N}.json
/// Encrypted with AES-GCM on client side
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorChunk {
    /// Chunk identifier (matches filename chunk-{N}.json)
    pub chunk_id: usize,

    /// Vectors in this chunk (up to 10,000 per chunk)
    pub vectors: Vec<Vector>,
}

impl VectorChunk {
    /// Validate vector chunk
    ///
    /// # Arguments
    /// * `expected_dimensions` - Expected vector dimensions (from manifest)
    pub fn validate(&self, expected_dimensions: usize) -> Result<()> {
        if self.vectors.is_empty() {
            return Err(anyhow!("Vector chunk cannot be empty"));
        }

        // Validate each vector has correct dimensions
        for (i, vector) in self.vectors.iter().enumerate() {
            vector.validate(expected_dimensions).map_err(|e| {
                anyhow!(
                    "Vector {} (id: {}) validation failed: {}",
                    i,
                    vector.id,
                    e
                )
            })?;
        }

        Ok(())
    }

    /// Get number of vectors in chunk
    pub fn vector_count(&self) -> usize {
        self.vectors.len()
    }

    /// Get vector by ID
    pub fn get_vector(&self, id: &str) -> Option<&Vector> {
        self.vectors.iter().find(|v| v.id == id)
    }
}

/// Individual vector with embeddings and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vector {
    /// Unique vector identifier
    pub id: String,

    /// Vector embeddings (384 floats for all-MiniLM-L6-v2)
    pub vector: Vec<f32>,

    /// Arbitrary metadata (source, page, folderPath, etc.)
    pub metadata: serde_json::Value,
}

impl Vector {
    /// Validate vector dimensions
    ///
    /// # Arguments
    /// * `expected_dimensions` - Expected number of dimensions
    pub fn validate(&self, expected_dimensions: usize) -> Result<()> {
        if self.id.is_empty() {
            return Err(anyhow!("Vector ID cannot be empty"));
        }

        if self.vector.len() != expected_dimensions {
            return Err(anyhow!(
                "Invalid vector dimensions for '{}': Expected {} dimensions, got {}",
                self.id,
                expected_dimensions,
                self.vector.len()
            ));
        }

        // Check for NaN or infinite values
        for (i, &val) in self.vector.iter().enumerate() {
            if !val.is_finite() {
                return Err(anyhow!(
                    "Vector '{}' contains non-finite value at dimension {}: {}",
                    self.id,
                    i,
                    val
                ));
            }
        }

        Ok(())
    }

    /// Get dimension count
    pub fn dimensions(&self) -> usize {
        self.vector.len()
    }

    /// Get metadata field as string
    pub fn get_metadata_string(&self, key: &str) -> Option<String> {
        self.metadata.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    }

    /// Get metadata field as i64
    pub fn get_metadata_i64(&self, key: &str) -> Option<i64> {
        self.metadata.get(key).and_then(|v| v.as_i64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_creation() {
        let manifest = Manifest {
            name: "test".to_string(),
            owner: "0xABC".to_string(),
            description: "Test database".to_string(),
            dimensions: 384,
            vector_count: 0,
            storage_size_bytes: 0,
            created: 1700000000000,
            last_accessed: 1700000000000,
            updated: 1700000000000,
            chunks: vec![],
            chunk_count: 0,
            folder_paths: vec![],
            deleted: false,
        };

        assert_eq!(manifest.name, "test");
        assert_eq!(manifest.dimensions, 384);
    }

    #[test]
    fn test_vector_validation_nan() {
        let vector = Vector {
            id: "test".to_string(),
            vector: vec![1.0, 2.0, f32::NAN, 4.0],
            metadata: serde_json::json!({}),
        };

        let result = vector.validate(4);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("non-finite"));
    }

    #[test]
    fn test_vector_validation_infinity() {
        let vector = Vector {
            id: "test".to_string(),
            vector: vec![1.0, 2.0, f32::INFINITY, 4.0],
            metadata: serde_json::json!({}),
        };

        let result = vector.validate(4);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("non-finite"));
    }

    #[test]
    fn test_chunk_metadata_helper_methods() {
        let chunk = ChunkMetadata {
            chunk_id: 0,
            cid: "s5://test".to_string(),
            vector_count: 100,
            size_bytes: 5000,
            updated_at: 1700000000000,
        };

        assert!(chunk.validate().is_ok());
    }
}
