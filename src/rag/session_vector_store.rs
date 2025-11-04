// Session-scoped vector storage for RAG
// Vectors are stored in memory during WebSocket session and cleared on disconnect

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;

use crate::vector::embeddings::Embedding;

/// Maximum metadata size per vector entry (10KB)
/// Prevents memory exhaustion attacks (100K vectors × 10KB = 1GB max metadata)
const MAX_METADATA_SIZE: usize = 10 * 1024;

/// Entry stored in the vector store
#[derive(Clone, Debug)]
pub struct VectorEntry {
    pub vector: Vec<f32>,
    pub metadata: Value,
    pub created_at: Instant,
}

/// Result from vector search
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub metadata: Value,
}

/// Session-scoped vector storage
/// - Stores vectors in memory during active session
/// - Cleared when session disconnects
/// - Supports semantic search via cosine similarity
#[derive(Debug)]
pub struct SessionVectorStore {
    session_id: String,
    vectors: HashMap<String, VectorEntry>,
    max_vectors: usize,
}

impl SessionVectorStore {
    /// Create new session vector store
    ///
    /// # Arguments
    /// * `session_id` - Unique session identifier
    /// * `max_vectors` - Maximum number of vectors allowed (memory limit)
    pub fn new(session_id: String, max_vectors: usize) -> Self {
        Self {
            session_id,
            vectors: HashMap::new(),
            max_vectors,
        }
    }

    /// Add vector to store
    ///
    /// # Arguments
    /// * `id` - Unique identifier for this vector
    /// * `vector` - 384-dimensional embedding vector
    /// * `metadata` - JSON metadata associated with this vector
    ///
    /// # Returns
    /// * `Ok(())` if added successfully
    /// * `Err` if dimensions invalid or max capacity reached
    pub fn add(&mut self, id: String, vector: Vec<f32>, metadata: Value) -> Result<()> {
        // Validate dimensions (must be 384 to match host embeddings)
        if vector.len() != 384 {
            return Err(anyhow!(
                "Invalid vector dimensions: expected 384, got {}",
                vector.len()
            ));
        }

        // Validate no NaN or Infinity values (would break similarity calculations)
        if vector.iter().any(|v| v.is_nan() || v.is_infinite()) {
            return Err(anyhow!(
                "Invalid vector values: contains NaN or Infinity (all values must be finite numbers)"
            ));
        }

        // Validate metadata size (prevent memory exhaustion)
        let metadata_size = serde_json::to_string(&metadata)?.len();
        if metadata_size > MAX_METADATA_SIZE {
            return Err(anyhow!(
                "Metadata too large: {} bytes (max: {} bytes / ~{}KB)",
                metadata_size,
                MAX_METADATA_SIZE,
                MAX_METADATA_SIZE / 1024
            ));
        }

        // Check capacity (unless replacing existing)
        if !self.vectors.contains_key(&id) && self.vectors.len() >= self.max_vectors {
            return Err(anyhow!(
                "Maximum vector capacity reached: {} vectors (max: {})",
                self.vectors.len(),
                self.max_vectors
            ));
        }

        // Add or replace vector
        self.vectors.insert(
            id,
            VectorEntry {
                vector,
                metadata,
                created_at: Instant::now(),
            },
        );

        Ok(())
    }

    /// Get vector by ID
    ///
    /// # Arguments
    /// * `id` - Vector identifier
    ///
    /// # Returns
    /// * `Some(&VectorEntry)` if found
    /// * `None` if not found
    pub fn get(&self, id: &str) -> Option<&VectorEntry> {
        self.vectors.get(id)
    }

    /// Delete vector by ID
    ///
    /// # Arguments
    /// * `id` - Vector identifier
    ///
    /// # Returns
    /// * `true` if deleted
    /// * `false` if not found
    pub fn delete(&mut self, id: &str) -> bool {
        self.vectors.remove(id).is_some()
    }

    /// Get count of vectors in store
    pub fn count(&self) -> usize {
        self.vectors.len()
    }

    /// Clear all vectors from store
    /// Called when session disconnects
    pub fn clear(&mut self) {
        self.vectors.clear();
    }

    /// Get session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get maximum vector capacity
    pub fn max_vectors(&self) -> usize {
        self.max_vectors
    }

    /// Search for similar vectors using cosine similarity
    ///
    /// # Arguments
    /// * `query` - Query vector (must be 384 dimensions)
    /// * `k` - Number of results to return
    /// * `threshold` - Optional minimum similarity score (0.0 to 1.0)
    ///
    /// # Returns
    /// * `Ok(Vec<SearchResult>)` - Top-k results sorted by score descending
    /// * `Err` if query dimensions invalid
    pub fn search(
        &self,
        query: Vec<f32>,
        k: usize,
        threshold: Option<f32>,
    ) -> Result<Vec<SearchResult>> {
        // Validate query dimensions
        if query.len() != 384 {
            return Err(anyhow!(
                "Invalid query dimensions: expected 384, got {}",
                query.len()
            ));
        }

        // Empty store returns empty results
        if self.vectors.is_empty() {
            return Ok(Vec::new());
        }

        // Create query embedding
        let query_embedding = Embedding::new(query);

        // Calculate similarities for all vectors
        let mut results: Vec<SearchResult> = self
            .vectors
            .iter()
            .map(|(id, entry)| {
                let vector_embedding = Embedding::new(entry.vector.clone());
                let score = query_embedding.cosine_similarity(&vector_embedding);

                SearchResult {
                    id: id.clone(),
                    score,
                    metadata: entry.metadata.clone(),
                }
            })
            .collect();

        // Filter by threshold if provided
        if let Some(min_score) = threshold {
            results.retain(|r| r.score >= min_score);
        }

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Return top-k
        results.truncate(k);

        Ok(results)
    }

    /// Search with metadata filtering
    ///
    /// # Arguments
    /// * `query` - Query vector (must be 384 dimensions)
    /// * `k` - Number of results to return
    /// * `metadata_filter` - JSON filter (supports $eq, $in operators)
    ///
    /// # Returns
    /// * `Ok(Vec<SearchResult>)` - Top-k filtered results sorted by score
    /// * `Err` if query dimensions invalid or filter invalid
    ///
    /// # Example
    /// ```ignore
    /// let filter = json!({"category": {"$eq": "science"}});
    /// let results = store.search_with_filter(query, 5, filter)?;
    /// ```
    pub fn search_with_filter(
        &self,
        query: Vec<f32>,
        k: usize,
        metadata_filter: Value,
    ) -> Result<Vec<SearchResult>> {
        // First perform standard search (no threshold, large k to get all matches)
        let mut all_results = self.search(query, self.vectors.len(), None)?;

        // Apply metadata filtering
        all_results.retain(|result| {
            self.matches_filter(&result.metadata, &metadata_filter)
        });

        // Return top-k after filtering
        all_results.truncate(k);

        Ok(all_results)
    }

    /// Check if metadata matches filter
    ///
    /// Supports basic filter operations:
    /// - `{"field": {"$eq": value}}` - equality
    /// - `{"field": {"$in": [values]}}` - membership
    fn matches_filter(&self, metadata: &Value, filter: &Value) -> bool {
        // Filter must be an object
        let filter_obj = match filter.as_object() {
            Some(obj) => obj,
            None => return true, // Empty/invalid filter matches all
        };

        // Check each filter field
        for (field, condition) in filter_obj {
            let metadata_value = &metadata[field];

            // Parse condition
            if let Some(condition_obj) = condition.as_object() {
                for (op, expected) in condition_obj {
                    match op.as_str() {
                        "$eq" => {
                            if metadata_value != expected {
                                return false;
                            }
                        }
                        "$in" => {
                            if let Some(expected_array) = expected.as_array() {
                                if !expected_array.contains(metadata_value) {
                                    return false;
                                }
                            }
                        }
                        _ => {
                            // Unknown operator, skip
                            continue;
                        }
                    }
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_vector_entry_creation() {
        let entry = VectorEntry {
            vector: vec![0.1; 384],
            metadata: json!({"test": true}),
            created_at: Instant::now(),
        };

        assert_eq!(entry.vector.len(), 384);
        assert_eq!(entry.metadata["test"], true);
    }

    #[test]
    fn test_store_basic_operations() {
        let mut store = SessionVectorStore::new("test-session".to_string(), 100);

        // Test empty store
        assert_eq!(store.count(), 0);
        assert!(store.get("doc1").is_none());

        // Test add
        let result = store.add(
            "doc1".to_string(),
            vec![0.5; 384],
            json!({"title": "Test"}),
        );
        assert!(result.is_ok());
        assert_eq!(store.count(), 1);

        // Test get
        let entry = store.get("doc1");
        assert!(entry.is_some());

        // Test delete
        assert!(store.delete("doc1"));
        assert_eq!(store.count(), 0);

        // Test clear
        store.add("doc2".to_string(), vec![0.1; 384], json!({})).unwrap();
        store.add("doc3".to_string(), vec![0.2; 384], json!({})).unwrap();
        assert_eq!(store.count(), 2);
        store.clear();
        assert_eq!(store.count(), 0);
    }
}
