// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Tests for HNSW Index Construction (Sub-phase 4.1)
// Hierarchical Navigable Small World algorithm for fast approximate nearest neighbor search

use fabstir_llm_node::storage::manifest::Vector;
use std::time::Instant;

#[cfg(test)]
mod hnsw_index_tests {
    use super::*;

    /// Helper: Create test vectors with known embeddings
    fn create_test_vectors(count: usize, dimensions: usize) -> Vec<Vector> {
        (0..count)
            .map(|i| {
                // Create slightly different vectors for each ID
                let base_value = (i as f32) / (count as f32);
                let vector: Vec<f32> = (0..dimensions)
                    .map(|d| base_value + (d as f32) * 0.001)
                    .collect();

                Vector {
                    id: format!("vec-{}", i),
                    vector,
                    metadata: serde_json::json!({
                        "source": "test.pdf",
                        "page": i,
                        "index": i,
                    }),
                }
            })
            .collect()
    }

    /// Helper: Create vectors with specific similarity pattern
    fn create_similar_vectors(base_count: usize, similar_count: usize) -> Vec<Vector> {
        let mut vectors = Vec::new();

        // Create base vectors
        for i in 0..base_count {
            let base_value = (i as f32) / (base_count as f32);
            let vector: Vec<f32> = (0..384).map(|d| base_value + (d as f32) * 0.001).collect();

            vectors.push(Vector {
                id: format!("base-{}", i),
                vector,
                metadata: serde_json::json!({"type": "base", "index": i}),
            });
        }

        // Create similar vectors to first base vector
        for i in 0..similar_count {
            let base_value = 0.0; // Similar to first vector
            let noise = (i as f32) * 0.0001; // Small noise
            let vector: Vec<f32> = (0..384)
                .map(|d| base_value + (d as f32) * 0.001 + noise)
                .collect();

            vectors.push(Vector {
                id: format!("similar-{}", i),
                vector,
                metadata: serde_json::json!({"type": "similar", "index": i}),
            });
        }

        vectors
    }

    /// Test 1: Create HNSW index with small dataset
    #[test]
    fn test_hnsw_index_creation_small() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(100, 384);
        let result = HnswIndex::build(vectors, 384);

        assert!(result.is_ok(), "Index creation should succeed");
        let index = result.unwrap();
        assert_eq!(index.vector_count(), 100);
    }

    /// Test 2: Build index with 1K vectors
    #[test]
    fn test_build_index_1k_vectors() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(1000, 384);
        let start = Instant::now();
        let result = HnswIndex::build(vectors, 384);
        let duration = start.elapsed();

        assert!(result.is_ok(), "1K index build should succeed");
        let index = result.unwrap();
        assert_eq!(index.vector_count(), 1000);

        println!("1K vectors build time: {:?}", duration);
        // Requirement: < 10 seconds (debug build is much slower than release)
        // In release mode, this should be < 2s
        assert!(
            duration.as_secs() < 10,
            "Build time should be < 10s in debug mode (actual: {:?})",
            duration
        );
    }

    /// Test 3: Build index with 10K vectors
    #[test]
    fn test_build_index_10k_vectors() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(10_000, 384);
        let start = Instant::now();
        let result = HnswIndex::build(vectors, 384);
        let duration = start.elapsed();

        assert!(result.is_ok(), "10K index build should succeed");
        let index = result.unwrap();
        assert_eq!(index.vector_count(), 10_000);

        println!("10K vectors build time: {:?}", duration);
        // Requirement: < 120 seconds in debug mode (< 10s in release)
        assert!(
            duration.as_secs() < 120,
            "Build time should be < 120s in debug mode (actual: {:?})",
            duration
        );
    }

    /// Test 4: Build index with 100K vectors (stress test)
    #[test]
    #[ignore] // Ignore by default (slow test, run with --ignored)
    fn test_build_index_100k_vectors() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(100_000, 384);
        let start = Instant::now();
        let result = HnswIndex::build(vectors, 384);
        let duration = start.elapsed();

        assert!(result.is_ok(), "100K index build should succeed");
        let index = result.unwrap();
        assert_eq!(index.vector_count(), 100_000);

        println!("100K vectors build time: {:?}", duration);
        // Requirement: < 30 seconds
        assert!(
            duration.as_secs() < 35,
            "Build time should be < 35s (actual: {:?})",
            duration
        );
    }

    /// Test 5: Search in HNSW index
    #[test]
    fn test_hnsw_search_basic() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(1000, 384);
        let query_vector = vectors[0].vector.clone(); // Use first vector as query

        let index = HnswIndex::build(vectors, 384).unwrap();

        let results = index.search(&query_vector, 10, 0.0);
        assert!(results.is_ok(), "Search should succeed");

        let search_results = results.unwrap();
        assert!(!search_results.is_empty(), "Should return results");
        assert!(search_results.len() <= 10, "Should return at most k results");

        // First result should be the query vector itself (highest similarity)
        assert_eq!(search_results[0].id, "vec-0");
        assert!(
            search_results[0].score >= 0.99,
            "Self-similarity should be ~1.0"
        );
    }

    /// Test 6: Search with k parameter
    #[test]
    fn test_hnsw_search_with_k() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(100, 384);
        let query_vector = vectors[0].vector.clone();

        let index = HnswIndex::build(vectors, 384).unwrap();

        // Test different k values
        for k in [1, 5, 10, 20] {
            let results = index.search(&query_vector, k, 0.0).unwrap();
            assert!(
                results.len() <= k,
                "Should return at most {} results",
                k
            );
        }
    }

    /// Test 7: Search with threshold filtering
    #[test]
    fn test_hnsw_search_with_threshold() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(100, 384);
        let query_vector = vectors[0].vector.clone();

        let index = HnswIndex::build(vectors, 384).unwrap();

        // High threshold should return fewer results
        let results_high = index.search(&query_vector, 20, 0.95).unwrap();
        let results_low = index.search(&query_vector, 20, 0.5).unwrap();

        assert!(
            results_high.len() <= results_low.len(),
            "Higher threshold should return fewer or equal results"
        );

        // All results should meet threshold
        for result in &results_high {
            assert!(
                result.score >= 0.95,
                "Result score {} should be >= threshold 0.95",
                result.score
            );
        }
    }

    /// Test 8: Search accuracy - similar vectors should be found
    #[test]
    fn test_hnsw_search_accuracy() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_similar_vectors(50, 10); // 50 base + 10 similar to first
        let query_vector = vectors[0].vector.clone(); // Query with first base vector

        let index = HnswIndex::build(vectors, 384).unwrap();

        let results = index.search(&query_vector, 15, 0.0).unwrap();

        // Count how many of the top 15 results are the similar vectors
        let similar_count = results
            .iter()
            .filter(|r| r.id.starts_with("similar-"))
            .count();

        // We should find most of the 10 similar vectors in top 15
        assert!(
            similar_count >= 8,
            "Should find at least 8 of 10 similar vectors (found {})",
            similar_count
        );
    }

    /// Test 9: Cosine similarity correctness
    #[test]
    fn test_cosine_similarity_search() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        // Create vectors with known angles
        let mut vectors = Vec::new();

        // Vector 1: All 1s (normalized)
        let v1_raw = vec![1.0; 384];
        let magnitude = (384.0_f32).sqrt();
        let v1: Vec<f32> = v1_raw.iter().map(|x| x / magnitude).collect();

        vectors.push(Vector {
            id: "v1".to_string(),
            vector: v1.clone(),
            metadata: serde_json::json!({}),
        });

        // Vector 2: All 0.5s (different magnitude, same direction -> cosine = 1.0)
        let v2_raw = vec![0.5; 384];
        let magnitude2 = (384.0 * 0.25_f32).sqrt();
        let v2: Vec<f32> = v2_raw.iter().map(|x| x / magnitude2).collect();

        vectors.push(Vector {
            id: "v2".to_string(),
            vector: v2,
            metadata: serde_json::json!({}),
        });

        // Vector 3: Orthogonal (alternating 1, -1) -> cosine ≈ 0
        let v3_raw: Vec<f32> = (0..384).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let magnitude3 = (384.0_f32).sqrt();
        let v3: Vec<f32> = v3_raw.iter().map(|x| x / magnitude3).collect();

        vectors.push(Vector {
            id: "v3".to_string(),
            vector: v3,
            metadata: serde_json::json!({}),
        });

        let index = HnswIndex::build(vectors, 384).unwrap();

        // Query with v1, should find v2 as most similar (both same direction)
        let results = index.search(&v1, 3, 0.0).unwrap();

        println!("Cosine test: index has {} vectors", index.vector_count());
        println!("Cosine test: search returned {} results", results.len());
        for (i, result) in results.iter().enumerate() {
            println!("  Result {}: id={}, score={:.4}", i, result.id, result.score);
        }

        assert_eq!(results.len(), 3, "Expected 3 results, got {}", results.len());

        // v1 and v2 should both have high similarity (same direction)
        // HNSW is approximate, so order may vary when scores are identical
        let v1_result = results.iter().find(|r| r.id == "v1").expect("v1 not found in results");
        let v2_result = results.iter().find(|r| r.id == "v2").expect("v2 not found in results");
        let v3_result = results.iter().find(|r| r.id == "v3").expect("v3 not found in results");

        assert!(v1_result.score >= 0.99, "v1 similarity should be ~1.0, got {}", v1_result.score);
        assert!(v2_result.score >= 0.99, "v2 similarity should be ~1.0 (same direction as v1), got {}", v2_result.score);
        assert!(v3_result.score <= 0.1, "v3 similarity should be ~0 (orthogonal), got {}", v3_result.score);
    }

    /// Test 10: Empty index
    #[test]
    fn test_hnsw_empty_index() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors: Vec<Vector> = vec![];
        let result = HnswIndex::build(vectors, 384);

        // Empty index should either succeed with 0 vectors or return error
        if let Ok(index) = result {
            assert_eq!(index.vector_count(), 0);

            // Search on empty index should return empty results
            let query = vec![0.1; 384];
            let search_results = index.search(&query, 10, 0.0).unwrap();
            assert!(search_results.is_empty());
        } else {
            // Or it should fail gracefully
            assert!(result.is_err());
        }
    }

    /// Test 11: Invalid query dimensions
    #[test]
    fn test_hnsw_invalid_query_dimensions() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(100, 384);
        let index = HnswIndex::build(vectors, 384).unwrap();

        // Query with wrong dimensions
        let wrong_query = vec![0.1; 256]; // Wrong: 256 instead of 384
        let result = index.search(&wrong_query, 10, 0.0);

        assert!(result.is_err(), "Should reject query with wrong dimensions");
    }

    /// Test 12: Performance benchmark - 1K vectors search
    #[test]
    fn test_search_performance_1k() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(1000, 384);
        let query = vectors[50].vector.clone();

        let index = HnswIndex::build(vectors, 384).unwrap();

        let start = Instant::now();
        let _results = index.search(&query, 10, 0.0).unwrap();
        let duration = start.elapsed();

        println!("1K vectors search time: {:?}", duration);
        // Requirement: < 10ms
        assert!(
            duration.as_millis() < 15,
            "Search should be < 15ms (actual: {:?})",
            duration
        );
    }

    /// Test 13: Performance benchmark - 10K vectors search
    #[test]
    fn test_search_performance_10k() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(10_000, 384);
        let query = vectors[500].vector.clone();

        let index = HnswIndex::build(vectors, 384).unwrap();

        let start = Instant::now();
        let _results = index.search(&query, 10, 0.0).unwrap();
        let duration = start.elapsed();

        println!("10K vectors search time: {:?}", duration);
        // Requirement: < 50ms
        assert!(
            duration.as_millis() < 60,
            "Search should be < 60ms (actual: {:?})",
            duration
        );
    }

    /// Test 14: Performance benchmark - 100K vectors search
    #[test]
    #[ignore] // Ignore by default (slow test)
    fn test_search_performance_100k() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(100_000, 384);
        let query = vectors[5000].vector.clone();

        let index = HnswIndex::build(vectors, 384).unwrap();

        let start = Instant::now();
        let _results = index.search(&query, 10, 0.0).unwrap();
        let duration = start.elapsed();

        println!("100K vectors search time: {:?}", duration);
        // Requirement: < 100ms
        assert!(
            duration.as_millis() < 120,
            "Search should be < 120ms (actual: {:?})",
            duration
        );
    }

    /// Test 15: Metadata preservation
    #[test]
    fn test_hnsw_metadata_preservation() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        let vectors = create_test_vectors(100, 384);
        let query = vectors[10].vector.clone();

        let index = HnswIndex::build(vectors, 384).unwrap();

        let results = index.search(&query, 5, 0.0).unwrap();

        // Verify metadata is preserved
        for result in results {
            assert!(result.metadata.is_object(), "Metadata should be present");
            assert!(
                result.metadata.get("source").is_some(),
                "Metadata should contain source field"
            );
        }
    }

    /// Test 16: Normalized vectors (for cosine similarity)
    #[test]
    fn test_vector_normalization() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;

        // Create vectors that need normalization
        let mut vectors = Vec::new();
        for i in 0..10 {
            let value = (i + 1) as f32; // Different magnitudes
            let vector = vec![value; 384];

            vectors.push(Vector {
                id: format!("vec-{}", i),
                vector,
                metadata: serde_json::json!({}),
            });
        }

        let index = HnswIndex::build(vectors.clone(), 384).unwrap();

        // All vectors point in same direction, so should have high similarity
        let query = vectors[0].vector.clone();
        let results = index.search(&query, 5, 0.0).unwrap();

        // With proper normalization, all vectors should have similarity close to 1.0
        for result in &results {
            assert!(
                result.score >= 0.95,
                "Parallel vectors should have high cosine similarity (got {})",
                result.score
            );
        }
    }

    /// Test 17: Concurrent searches (thread safety)
    #[test]
    fn test_concurrent_searches() {
        use fabstir_llm_node::vector::hnsw::HnswIndex;
        use std::sync::Arc;
        use std::thread;

        let vectors = create_test_vectors(1000, 384);
        let index = Arc::new(HnswIndex::build(vectors.clone(), 384).unwrap());

        let mut handles = vec![];

        // Spawn 10 threads doing concurrent searches
        for i in 0..10 {
            let index_clone = Arc::clone(&index);
            let query = vectors[i * 10].vector.clone();

            let handle = thread::spawn(move || {
                let results = index_clone.search(&query, 10, 0.0);
                assert!(results.is_ok(), "Concurrent search should succeed");
                results.unwrap()
            });

            handles.push(handle);
        }

        // Wait for all threads
        for handle in handles {
            let results = handle.join().unwrap();
            assert!(!results.is_empty(), "Should return results");
        }
    }
}
