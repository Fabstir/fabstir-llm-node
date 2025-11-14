// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! HNSW Index for Fast Vector Search (Sub-phase 4.1)
//!
//! Hierarchical Navigable Small World (HNSW) algorithm for approximate nearest neighbor search.
//! Provides fast search on large vector databases (10K-100K+ vectors) loaded from S5 storage.
//!
//! ## Features
//!
//! - **Fast Search**: O(log n) average time complexity for k-NN search
//! - **Cosine Similarity**: Optimized for semantic similarity search
//! - **Vector Normalization**: Automatic normalization for accurate cosine similarity
//! - **Metadata Preservation**: Keeps vector metadata for search results
//! - **Thread-Safe**: Safe for concurrent searches from multiple threads
//!
//! ## Performance
//!
//! | Dataset Size | Build Time | Search Time (k=10) |
//! |--------------|------------|--------------------|
//! | 1K vectors   | < 2s       | < 10ms             |
//! | 10K vectors  | < 5s       | < 50ms             |
//! | 100K vectors | < 30s      | < 100ms            |
//!
//! ## Usage
//!
//! ```rust,ignore
//! use fabstir_llm_node::vector::hnsw::HnswIndex;
//! use fabstir_llm_node::storage::manifest::Vector;
//!
//! // Build index from vectors
//! let vectors: Vec<Vector> = load_vectors_from_somewhere();
//! let index = HnswIndex::build(vectors, 384)?;
//!
//! // Search for similar vectors
//! let query = vec![0.1; 384];
//! let results = index.search(&query, k=10, threshold=0.7)?;
//!
//! for result in results {
//!     println!("ID: {}, Score: {}", result.id, result.score);
//! }
//! ```

use crate::storage::manifest::Vector;
use anyhow::{anyhow, Result};
use hnsw_rs::hnsw::{Hnsw, Neighbour};
use hnsw_rs::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Search result from HNSW index
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Vector ID
    pub id: String,

    /// Similarity score (cosine similarity, 0.0 to 1.0)
    pub score: f32,

    /// Vector metadata
    pub metadata: Value,
}

/// HNSW index for fast approximate nearest neighbor search
///
/// Uses cosine distance for semantic similarity search on 384-dimensional vectors.
pub struct HnswIndex {
    /// HNSW data structure
    /// Note: Wrapped in Arc for thread-safe sharing during concurrent searches
    hnsw: Arc<Hnsw<'static, f32, DistCosine>>,

    /// Maps HNSW internal IDs to vector IDs
    id_map: Arc<HashMap<usize, String>>,

    /// Maps vector IDs to metadata
    metadata_map: Arc<HashMap<String, Value>>,

    /// Number of dimensions
    dimensions: usize,
}

impl HnswIndex {
    /// Build HNSW index from vectors
    ///
    /// # Arguments
    ///
    /// * `vectors` - Vectors to index (must all have same dimensions)
    /// * `dimensions` - Expected vector dimensions (e.g., 384 for all-MiniLM-L6-v2)
    ///
    /// # Returns
    ///
    /// HnswIndex ready for searching
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Vectors have wrong dimensions
    /// - Vectors contain NaN or Infinity values
    /// - Index construction fails
    ///
    /// # Performance
    ///
    /// - 1K vectors: ~1-2 seconds
    /// - 10K vectors: ~3-5 seconds
    /// - 100K vectors: ~20-30 seconds
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use fabstir_llm_node::vector::hnsw::HnswIndex;
    ///
    /// let vectors = load_vectors();
    /// let index = HnswIndex::build(vectors, 384)?;
    /// println!("Built index with {} vectors", index.vector_count());
    /// ```
    pub fn build(vectors: Vec<Vector>, dimensions: usize) -> Result<Self> {
        // Handle empty vector case
        if vectors.is_empty() {
            return Ok(Self {
                hnsw: Arc::new(Hnsw::new(
                    16,                // max_nb_connection (M parameter)
                    vectors.len(),     // nb_layer (will be 0 for empty)
                    16,                // ef_construction
                    200,               // max elements (doesn't matter for empty)
                    DistCosine,
                )),
                id_map: Arc::new(HashMap::new()),
                metadata_map: Arc::new(HashMap::new()),
                dimensions,
            });
        }

        // Validate all vectors
        for (i, vector) in vectors.iter().enumerate() {
            // Check dimensions
            if vector.vector.len() != dimensions {
                return Err(anyhow!(
                    "Vector {} has wrong dimensions: expected {}, got {}",
                    i,
                    dimensions,
                    vector.vector.len()
                ));
            }

            // Check for NaN/Infinity
            if vector.vector.iter().any(|&v| !v.is_finite()) {
                return Err(anyhow!(
                    "Vector {} contains NaN or Infinity values",
                    i
                ));
            }
        }

        // HNSW parameters (optimized for fast construction and 384D embeddings)
        // Reduced M and ef_construction for better build performance
        let max_nb_connection = 12;      // M parameter: connections per layer (reduced for speed)
        let ef_construction = 48;         // ef during construction (lower = faster build)
        // Calculate layers based on dataset size (log2(n), clamped to reasonable range)
        let nb_layer = if vectors.len() > 1 {
            ((vectors.len() as f32).log2().ceil() as usize).max(4).min(16)
        } else {
            4
        };

        // Create HNSW index
        let mut hnsw: Hnsw<f32, DistCosine> = Hnsw::new(
            max_nb_connection,
            nb_layer,
            ef_construction,
            vectors.len(),
            DistCosine,
        );

        // Build ID and metadata maps
        let mut id_map = HashMap::with_capacity(vectors.len());
        let mut metadata_map = HashMap::with_capacity(vectors.len());

        // Insert vectors into HNSW
        for (hnsw_id, vector) in vectors.into_iter().enumerate() {
            // Normalize vector for cosine similarity
            let normalized = normalize_vector(&vector.vector);

            // Insert into HNSW (using hnsw_id as the index)
            hnsw.insert((&normalized, hnsw_id));

            // Store mappings
            id_map.insert(hnsw_id, vector.id.clone());
            metadata_map.insert(vector.id, vector.metadata);
        }

        // Set search parameters
        hnsw.set_searching_mode(true);

        Ok(Self {
            hnsw: Arc::new(hnsw),
            id_map: Arc::new(id_map),
            metadata_map: Arc::new(metadata_map),
            dimensions,
        })
    }

    /// Search for k nearest neighbors
    ///
    /// # Arguments
    ///
    /// * `query` - Query vector (must match index dimensions)
    /// * `k` - Number of results to return
    /// * `threshold` - Minimum similarity score (0.0 to 1.0)
    ///
    /// # Returns
    ///
    /// Vector of search results sorted by similarity (highest first)
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Query has wrong dimensions
    /// - Query contains NaN or Infinity
    ///
    /// # Performance
    ///
    /// - 1K vectors: < 10ms
    /// - 10K vectors: < 50ms
    /// - 100K vectors: < 100ms
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let query = vec![0.1; 384];
    /// let results = index.search(&query, 10, 0.7)?;
    ///
    /// for result in results {
    ///     println!("{}: {:.3}", result.id, result.score);
    /// }
    /// ```
    pub fn search(&self, query: &[f32], k: usize, threshold: f32) -> Result<Vec<SearchResult>> {
        // Validate query dimensions
        if query.len() != self.dimensions {
            return Err(anyhow!(
                "Query has wrong dimensions: expected {}, got {}",
                self.dimensions,
                query.len()
            ));
        }

        // Check for NaN/Infinity
        if query.iter().any(|&v| !v.is_finite()) {
            return Err(anyhow!("Query contains NaN or Infinity values"));
        }

        // Handle empty index
        if self.id_map.is_empty() {
            return Ok(vec![]);
        }

        // Normalize query for cosine similarity
        let normalized_query = normalize_vector(query);

        // Perform k-NN search
        let ef_search = (k * 2).max(50); // ef_search should be >= k (typically 1.5-2x k)
        let neighbours: Vec<Neighbour> = self.hnsw.search(&normalized_query, k, ef_search);

        // Convert to SearchResults
        let mut results = Vec::with_capacity(neighbours.len());

        for neighbour in neighbours {
            let hnsw_id = neighbour.d_id;

            // Get vector ID
            if let Some(vector_id) = self.id_map.get(&hnsw_id) {
                // Convert distance to similarity score
                // HNSW returns distance, we need similarity
                // For cosine distance: similarity = 1 - distance
                let score = 1.0 - neighbour.distance;

                // Apply threshold filter
                if score >= threshold {
                    // Get metadata
                    let metadata = self
                        .metadata_map
                        .get(vector_id)
                        .cloned()
                        .unwrap_or(Value::Null);

                    results.push(SearchResult {
                        id: vector_id.clone(),
                        score,
                        metadata,
                    });
                }
            }
        }

        // Sort by score (highest first)
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        Ok(results)
    }

    /// Get number of vectors in index
    pub fn vector_count(&self) -> usize {
        self.id_map.len()
    }

    /// Get index dimensions
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }
}

/// Normalize vector for cosine similarity
///
/// Divides vector by its magnitude (L2 norm) to make it unit length.
/// This ensures cosine similarity is computed correctly.
fn normalize_vector(vector: &[f32]) -> Vec<f32> {
    // Calculate magnitude (L2 norm)
    let magnitude: f32 = vector.iter().map(|&x| x * x).sum::<f32>().sqrt();

    // Handle zero vector
    if magnitude == 0.0 || !magnitude.is_finite() {
        return vector.to_vec();
    }

    // Normalize
    vector.iter().map(|&x| x / magnitude).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_vector() {
        // Test normalization
        let v = vec![3.0, 4.0]; // magnitude = 5.0
        let normalized = normalize_vector(&v);

        // Should be [0.6, 0.8]
        assert!((normalized[0] - 0.6).abs() < 0.001);
        assert!((normalized[1] - 0.8).abs() < 0.001);

        // Check unit length
        let magnitude: f32 = normalized.iter().map(|&x| x * x).sum::<f32>().sqrt();
        assert!((magnitude - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_normalize_zero_vector() {
        let v = vec![0.0, 0.0, 0.0];
        let normalized = normalize_vector(&v);

        // Zero vector should remain zero
        assert_eq!(normalized, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_vector_count() {
        let vectors = vec![
            Vector {
                id: "v1".to_string(),
                vector: vec![1.0; 384],
                metadata: serde_json::json!({}),
            },
            Vector {
                id: "v2".to_string(),
                vector: vec![0.5; 384],
                metadata: serde_json::json!({}),
            },
        ];

        let index = HnswIndex::build(vectors, 384).unwrap();
        assert_eq!(index.vector_count(), 2);
    }
}
