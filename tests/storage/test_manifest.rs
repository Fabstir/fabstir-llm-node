// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Tests for S5 Vector Database Manifest and Chunk structures (Sub-phase 2.2)

use serde_json::json;

#[cfg(test)]
mod manifest_tests {
    use super::*;

    /// Test 1: Manifest deserialization from SDK format (camelCase)
    #[test]
    fn test_manifest_deserialization() {
        use fabstir_llm_node::storage::manifest::Manifest;

        let json = json!({
            "name": "my-docs",
            "owner": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12",
            "description": "Documentation for Project X",
            "dimensions": 384,
            "vectorCount": 15000,
            "storageSizeBytes": 64000000,
            "created": 1700000000000i64,
            "lastAccessed": 1700000000000i64,
            "updated": 1700000000000i64,
            "chunks": [
                {
                    "chunkId": 0,
                    "cid": "s5://abc123...",
                    "vectorCount": 10000,
                    "sizeBytes": 43000000,
                    "updatedAt": 1700000000000i64
                },
                {
                    "chunkId": 1,
                    "cid": "s5://def456...",
                    "vectorCount": 5000,
                    "sizeBytes": 21000000,
                    "updatedAt": 1700000000000i64
                }
            ],
            "chunkCount": 2,
            "folderPaths": ["/docs", "/research"],
            "deleted": false
        });

        let manifest: Manifest = serde_json::from_value(json).expect("Failed to deserialize");

        assert_eq!(manifest.name, "my-docs");
        assert_eq!(manifest.owner, "0xABCDEF1234567890ABCDEF1234567890ABCDEF12");
        assert_eq!(manifest.description, "Documentation for Project X");
        assert_eq!(manifest.dimensions, 384);
        assert_eq!(manifest.vector_count, 15000);
        assert_eq!(manifest.storage_size_bytes, 64000000);
        assert_eq!(manifest.created, 1700000000000);
        assert_eq!(manifest.last_accessed, 1700000000000);
        assert_eq!(manifest.updated, 1700000000000);
        assert_eq!(manifest.chunks.len(), 2);
        assert_eq!(manifest.chunk_count, 2);
        assert_eq!(manifest.folder_paths, vec!["/docs", "/research"]);
        assert_eq!(manifest.deleted, false);
    }

    /// Test 2: ChunkMetadata deserialization
    #[test]
    fn test_chunk_metadata_deserialization() {
        use fabstir_llm_node::storage::manifest::ChunkMetadata;

        let json = json!({
            "chunkId": 0,
            "cid": "s5://abc123...",
            "vectorCount": 10000,
            "sizeBytes": 43000000,
            "updatedAt": 1700000000000i64
        });

        let chunk: ChunkMetadata = serde_json::from_value(json).expect("Failed to deserialize");

        assert_eq!(chunk.chunk_id, 0);
        assert_eq!(chunk.cid, "s5://abc123...");
        assert_eq!(chunk.vector_count, 10000);
        assert_eq!(chunk.size_bytes, 43000000);
        assert_eq!(chunk.updated_at, 1700000000000);
    }

    /// Test 3: VectorChunk deserialization
    #[test]
    fn test_vector_chunk_deserialization() {
        use fabstir_llm_node::storage::manifest::VectorChunk;

        let json = json!({
            "chunkId": 0,
            "vectors": [
                {
                    "id": "vec1",
                    "vector": [0.1, 0.2, 0.3],
                    "metadata": {
                        "source": "doc1.pdf",
                        "page": 3,
                        "folderPath": "/docs"
                    }
                },
                {
                    "id": "vec2",
                    "vector": [0.4, 0.5, 0.6],
                    "metadata": {
                        "source": "doc2.pdf",
                        "page": 1,
                        "folderPath": "/research"
                    }
                }
            ]
        });

        let chunk: VectorChunk = serde_json::from_value(json).expect("Failed to deserialize");

        assert_eq!(chunk.chunk_id, 0);
        assert_eq!(chunk.vectors.len(), 2);
        assert_eq!(chunk.vectors[0].id, "vec1");
        assert_eq!(chunk.vectors[0].vector, vec![0.1, 0.2, 0.3]);
        assert_eq!(chunk.vectors[1].id, "vec2");
    }

    /// Test 4: Vector struct deserialization with metadata
    #[test]
    fn test_vector_with_metadata() {
        use fabstir_llm_node::storage::manifest::Vector;

        let json = json!({
            "id": "vec123",
            "vector": [0.1, 0.2, 0.3, 0.4],
            "metadata": {
                "source": "document.pdf",
                "page": 5,
                "folderPath": "/docs",
                "customField": "value"
            }
        });

        let vec: Vector = serde_json::from_value(json).expect("Failed to deserialize");

        assert_eq!(vec.id, "vec123");
        assert_eq!(vec.vector.len(), 4);
        assert_eq!(vec.vector[0], 0.1);

        // Check metadata
        assert_eq!(vec.metadata["source"], "document.pdf");
        assert_eq!(vec.metadata["page"], 5);
        assert_eq!(vec.metadata["folderPath"], "/docs");
        assert_eq!(vec.metadata["customField"], "value");
    }

    /// Test 5: Manifest validation - valid structure
    #[test]
    fn test_manifest_validation_valid() {
        use fabstir_llm_node::storage::manifest::Manifest;

        let json = json!({
            "name": "test-db",
            "owner": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12",
            "description": "Test database",
            "dimensions": 384,
            "vectorCount": 1000,
            "storageSizeBytes": 5000000,
            "created": 1700000000000i64,
            "lastAccessed": 1700000000000i64,
            "updated": 1700000000000i64,
            "chunks": [
                {
                    "chunkId": 0,
                    "cid": "s5://test",
                    "vectorCount": 1000,
                    "sizeBytes": 5000000,
                    "updatedAt": 1700000000000i64
                }
            ],
            "chunkCount": 1,
            "folderPaths": ["/test"],
            "deleted": false
        });

        let manifest: Manifest = serde_json::from_value(json).unwrap();
        let result = manifest.validate();
        assert!(result.is_ok());
    }

    /// Test 6: Manifest validation - mismatched chunk count
    #[test]
    fn test_manifest_validation_chunk_count_mismatch() {
        use fabstir_llm_node::storage::manifest::Manifest;

        let json = json!({
            "name": "test-db",
            "owner": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12",
            "description": "Test",
            "dimensions": 384,
            "vectorCount": 1000,
            "storageSizeBytes": 5000000,
            "created": 1700000000000i64,
            "lastAccessed": 1700000000000i64,
            "updated": 1700000000000i64,
            "chunks": [
                {
                    "chunkId": 0,
                    "cid": "s5://test",
                    "vectorCount": 1000,
                    "sizeBytes": 5000000,
                    "updatedAt": 1700000000000i64
                }
            ],
            "chunkCount": 2,  // Mismatch: says 2 but only 1 chunk
            "folderPaths": ["/test"],
            "deleted": false
        });

        let manifest: Manifest = serde_json::from_value(json).unwrap();
        let result = manifest.validate();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.to_lowercase().contains("chunk count"),
            "Error was: {}",
            err_msg
        );
    }

    /// Test 7: Manifest validation - invalid dimensions
    #[test]
    fn test_manifest_validation_invalid_dimensions() {
        use fabstir_llm_node::storage::manifest::Manifest;

        let json = json!({
            "name": "test-db",
            "owner": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12",
            "description": "Test",
            "dimensions": 0,  // Invalid: must be > 0
            "vectorCount": 1000,
            "storageSizeBytes": 5000000,
            "created": 1700000000000i64,
            "lastAccessed": 1700000000000i64,
            "updated": 1700000000000i64,
            "chunks": [],
            "chunkCount": 0,
            "folderPaths": [],
            "deleted": false
        });

        let manifest: Manifest = serde_json::from_value(json).unwrap();
        let result = manifest.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("dimensions"));
    }

    /// Test 8: ChunkMetadata validation - invalid chunk_id
    #[test]
    fn test_chunk_metadata_validation_invalid_id() {
        use fabstir_llm_node::storage::manifest::Manifest;

        let json = json!({
            "name": "test-db",
            "owner": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12",
            "description": "Test",
            "dimensions": 384,
            "vectorCount": 1000,
            "storageSizeBytes": 5000000,
            "created": 1700000000000i64,
            "lastAccessed": 1700000000000i64,
            "updated": 1700000000000i64,
            "chunks": [
                {
                    "chunkId": 1,  // Invalid: should start at 0
                    "cid": "s5://test",
                    "vectorCount": 1000,
                    "sizeBytes": 5000000,
                    "updatedAt": 1700000000000i64
                }
            ],
            "chunkCount": 1,
            "folderPaths": [],
            "deleted": false
        });

        let manifest: Manifest = serde_json::from_value(json).unwrap();
        let result = manifest.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("chunk IDs"));
    }

    /// Test 9: VectorChunk validation - dimension mismatch
    #[test]
    fn test_vector_chunk_validation_dimension_mismatch() {
        use fabstir_llm_node::storage::manifest::VectorChunk;

        let json = json!({
            "chunkId": 0,
            "vectors": [
                {
                    "id": "vec1",
                    "vector": [0.1, 0.2, 0.3],  // 3 dimensions
                    "metadata": {}
                },
                {
                    "id": "vec2",
                    "vector": [0.1, 0.2, 0.3, 0.4],  // 4 dimensions (mismatch!)
                    "metadata": {}
                }
            ]
        });

        let chunk: VectorChunk = serde_json::from_value(json).unwrap();
        let result = chunk.validate(3); // Expected 3 dimensions
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("dimension"));
    }

    /// Test 10: Vector validation - correct dimensions
    #[test]
    fn test_vector_validation_correct_dimensions() {
        use fabstir_llm_node::storage::manifest::Vector;

        let json = json!({
            "id": "vec1",
            "vector": [0.1, 0.2, 0.3, 0.4],
            "metadata": {"source": "test.pdf"}
        });

        let vec: Vector = serde_json::from_value(json).unwrap();
        let result = vec.validate(4);
        assert!(result.is_ok());
    }

    /// Test 11: Vector validation - incorrect dimensions
    #[test]
    fn test_vector_validation_incorrect_dimensions() {
        use fabstir_llm_node::storage::manifest::Vector;

        let json = json!({
            "id": "vec1",
            "vector": [0.1, 0.2, 0.3],
            "metadata": {}
        });

        let vec: Vector = serde_json::from_value(json).unwrap();
        let result = vec.validate(384); // Expected 384, got 3
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Expected 384 dimensions"));
    }

    /// Test 12: Manifest with optional fields (minimal valid manifest)
    #[test]
    fn test_manifest_minimal() {
        use fabstir_llm_node::storage::manifest::Manifest;

        let json = json!({
            "name": "minimal-db",
            "owner": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12",
            "description": "",
            "dimensions": 384,
            "vectorCount": 0,
            "storageSizeBytes": 0,
            "created": 1700000000000i64,
            "lastAccessed": 1700000000000i64,
            "updated": 1700000000000i64,
            "chunks": [],
            "chunkCount": 0,
            "folderPaths": [],
            "deleted": false
        });

        let manifest: Manifest = serde_json::from_value(json).unwrap();
        assert_eq!(manifest.name, "minimal-db");
        assert_eq!(manifest.vector_count, 0);
        assert_eq!(manifest.chunks.len(), 0);
    }

    /// Test 13: Large manifest with many chunks
    #[test]
    fn test_manifest_many_chunks() {
        use fabstir_llm_node::storage::manifest::Manifest;

        let chunks: Vec<serde_json::Value> = (0..10)
            .map(|i| {
                json!({
                    "chunkId": i,
                    "cid": format!("s5://chunk{}", i),
                    "vectorCount": 10000,
                    "sizeBytes": 43000000,
                    "updatedAt": 1700000000000i64
                })
            })
            .collect();

        let json = json!({
            "name": "large-db",
            "owner": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12",
            "description": "Large database",
            "dimensions": 384,
            "vectorCount": 100000,
            "storageSizeBytes": 430000000,
            "created": 1700000000000i64,
            "lastAccessed": 1700000000000i64,
            "updated": 1700000000000i64,
            "chunks": chunks,
            "chunkCount": 10,
            "folderPaths": ["/docs"],
            "deleted": false
        });

        let manifest: Manifest = serde_json::from_value(json).unwrap();
        assert_eq!(manifest.chunks.len(), 10);
        assert_eq!(manifest.chunk_count, 10);

        let result = manifest.validate();
        assert!(result.is_ok());
    }

    /// Test 14: VectorChunk with 384-dimensional vectors
    #[test]
    fn test_vector_chunk_384_dimensions() {
        use fabstir_llm_node::storage::manifest::VectorChunk;

        let vector_384d: Vec<f32> = (0..384).map(|i| i as f32 / 384.0).collect();

        let json = json!({
            "chunkId": 0,
            "vectors": [
                {
                    "id": "vec1",
                    "vector": vector_384d,
                    "metadata": {"source": "test.pdf", "page": 1}
                }
            ]
        });

        let chunk: VectorChunk = serde_json::from_value(json).unwrap();
        assert_eq!(chunk.vectors[0].vector.len(), 384);

        let result = chunk.validate(384);
        assert!(result.is_ok());
    }

    /// Test 15: Roundtrip serialization (serialize then deserialize)
    #[test]
    fn test_manifest_roundtrip() {
        use fabstir_llm_node::storage::manifest::Manifest;

        let original = json!({
            "name": "roundtrip-test",
            "owner": "0xABCDEF1234567890ABCDEF1234567890ABCDEF12",
            "description": "Test roundtrip",
            "dimensions": 384,
            "vectorCount": 1000,
            "storageSizeBytes": 5000000,
            "created": 1700000000000i64,
            "lastAccessed": 1700000000000i64,
            "updated": 1700000000000i64,
            "chunks": [
                {
                    "chunkId": 0,
                    "cid": "s5://test",
                    "vectorCount": 1000,
                    "sizeBytes": 5000000,
                    "updatedAt": 1700000000000i64
                }
            ],
            "chunkCount": 1,
            "folderPaths": ["/test"],
            "deleted": false
        });

        let manifest: Manifest = serde_json::from_value(original.clone()).unwrap();
        let serialized = serde_json::to_value(&manifest).unwrap();
        let deserialized: Manifest = serde_json::from_value(serialized).unwrap();

        assert_eq!(manifest.name, deserialized.name);
        assert_eq!(manifest.owner, deserialized.owner);
        assert_eq!(manifest.dimensions, deserialized.dimensions);
        assert_eq!(manifest.vector_count, deserialized.vector_count);
    }
}
