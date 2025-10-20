#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1


# Script to create the Phase 4.1.3 test files in the correct location

echo "Creating Phase 4.1.3 test files..."

# Create directory structure
mkdir -p tests/integration/mock

# Create test_e2e_workflow.rs
cat > tests/integration/mock/test_e2e_workflow.rs << 'EOF'
// tests/integration/mock/test_e2e_workflow.rs
// Phase 4.1.3: Integration with Both Mocks
// This test verifies the complete workflow:
// 1. Store model in Enhanced S5.js
// 2. Generate embeddings for model
// 3. Store embeddings in Vector DB
// 4. Test semantic search for similar models

use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;

// Import from our crate (these modules need to be implemented)
use crate::{
    storage::{EnhancedS5Client, S5Config},
    vector::{VectorDbClient, VectorDbConfig},
    embeddings::{EmbeddingGenerator, EmbeddingConfig},
    models::{ModelRegistry, ModelConfig, ModelMetadata},
};

#[tokio::test]
async fn test_store_model_in_enhanced_s5() -> Result<()> {
    // Initialize Enhanced S5.js client (running with mock backend)
    let s5_config = S5Config {
        api_url: "http://enhanced-s5-container:5050".to_string(),
        api_key: Some("test-api-key".to_string()),
        timeout_secs: 30,
    };
    let s5_client = EnhancedS5Client::new(s5_config)?;
    
    // Test model data
    let model_id = "llama-3.2-1b-instruct";
    let model_data = b"Mock GGUF model binary data for testing";
    let model_metadata = json!({
        "name": model_id,
        "version": "1.0.0",
        "type": "instruct",
        "size_bytes": model_data.len(),
        "format": "gguf",
        "created_at": "2025-01-06T10:00:00Z"
    });
    
    // Store model in Enhanced S5.js
    let path = format!("/models/{}/model.gguf", model_id);
    let cid = s5_client.put(&path, model_data.to_vec(), Some(model_metadata)).await?;
    
    assert!(!cid.is_empty());
    assert!(cid.starts_with("b")); // BLAKE3 hash prefix
    
    // Verify retrieval
    let (retrieved_data, retrieved_metadata) = s5_client.get(&path).await?;
    assert_eq!(retrieved_data, model_data);
    assert_eq!(retrieved_metadata.unwrap()["name"], model_id);
    
    Ok(())
}

#[tokio::test]
async fn test_generate_embeddings_for_model() -> Result<()> {
    // Initialize embedding generator
    let embedding_config = EmbeddingConfig {
        model: "all-MiniLM-L6-v2".to_string(),
        dimension: 384,
        batch_size: 32,
        normalize: true,
    };
    let generator = EmbeddingGenerator::new(embedding_config).await?;
    
    // Model description for embedding
    let model_description = "Llama 3.2 1B Instruct is a lightweight language model \
        optimized for instruction following and conversational AI. \
        It supports context lengths up to 4096 tokens and excels at \
        tasks like question answering, summarization, and code generation.";
    
    // Generate embeddings
    let embedding = generator.generate(model_description).await?;
    
    assert_eq!(embedding.len(), 384); // Correct dimension
    assert!(embedding.iter().all(|&x| x >= -1.0 && x <= 1.0)); // Normalized
    
    // Test batch generation
    let descriptions = vec![
        "A model for text generation",
        "A model for code completion",
        "A model for question answering",
    ];
    let embeddings = generator.generate_batch(&descriptions).await?;
    
    assert_eq!(embeddings.len(), 3);
    assert!(embeddings.iter().all(|e| e.len() == 384));
    
    Ok(())
}

// Add remaining tests...
#[tokio::test] 
async fn test_store_embeddings_in_vector_db() -> Result<()> {
    // TODO: Will fail until implemented
    panic!("Not implemented");
}

#[tokio::test]
async fn test_semantic_search_for_similar_models() -> Result<()> {
    // TODO: Will fail until implemented
    panic!("Not implemented");
}

#[tokio::test]
async fn test_complete_workflow_integration() -> Result<()> {
    // TODO: Will fail until implemented
    panic!("Not implemented");
}

#[tokio::test]
async fn test_model_discovery_by_capability() -> Result<()> {
    // TODO: Will fail until implemented
    panic!("Not implemented");
}

#[tokio::test]
async fn test_model_versioning_workflow() -> Result<()> {
    // TODO: Will fail until implemented
    panic!("Not implemented");
}
EOF

# Create test_cache_flow.rs
cat > tests/integration/mock/test_cache_flow.rs << 'EOF'
// tests/integration/mock/test_cache_flow.rs
// Phase 4.1.3: Cache Flow Implementation
// This test verifies the caching workflow:
// 1. Hash prompts for cache lookup
// 2. Search Vector DB for similar prompts
// 3. Retrieve cached results from S5
// 4. Measure cache hit rates

use anyhow::Result;
use serde_json::json;
use std::time::{Duration, Instant};
use sha2::{Sha256, Digest};

// Import from our crate
use crate::{
    cache::{PromptCache, CacheConfig, CacheMetrics},
    storage::{EnhancedS5Client, S5Config},
    vector::{VectorDbClient, VectorDbConfig},
    embeddings::{EmbeddingGenerator, EmbeddingConfig},
};

fn hash_prompt(prompt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[tokio::test]
async fn test_hash_prompts_for_cache_lookup() -> Result<()> {
    // Test deterministic prompt hashing
    let prompt1 = "What is the capital of France?";
    let prompt2 = "What is the capital of France?"; // Same prompt
    let prompt3 = "What is the capital of Germany?"; // Different prompt
    
    let hash1 = hash_prompt(prompt1);
    let hash2 = hash_prompt(prompt2);
    let hash3 = hash_prompt(prompt3);
    
    // Same prompts should have same hash
    assert_eq!(hash1, hash2);
    // Different prompts should have different hash
    assert_ne!(hash1, hash3);
    
    // Hash should be consistent length (SHA256 = 64 hex chars)
    assert_eq!(hash1.len(), 64);
    assert_eq!(hash3.len(), 64);
    
    // Test with model parameters included
    let prompt_with_params = format!("{};model=llama-3.2;temp=0.7;max_tokens=100", prompt1);
    let hash_with_params = hash_prompt(&prompt_with_params);
    assert_ne!(hash1, hash_with_params); // Different when params included
    
    Ok(())
}

// Add remaining cache tests...
#[tokio::test]
async fn test_search_vector_db_for_similar_prompts() -> Result<()> {
    // TODO: Will fail until implemented
    panic!("Not implemented");
}

#[tokio::test]
async fn test_retrieve_cached_results_from_s5() -> Result<()> {
    // TODO: Will fail until implemented
    panic!("Not implemented");
}

#[tokio::test]
async fn test_measure_cache_hit_rates() -> Result<()> {
    // TODO: Will fail until implemented
    panic!("Not implemented");
}

#[tokio::test]
async fn test_complete_cache_flow() -> Result<()> {
    // TODO: Will fail until implemented
    panic!("Not implemented");
}

#[tokio::test]
async fn test_cache_expiration_and_cleanup() -> Result<()> {
    // TODO: Will fail until implemented
    panic!("Not implemented");
}

#[tokio::test]
async fn test_cache_performance_metrics() -> Result<()> {
    // TODO: Will fail until implemented
    panic!("Not implemented");
}
EOF

# Create mod.rs to register the test modules
cat > tests/integration/mod.rs << 'EOF'
// tests/integration/mod.rs
// Integration test modules

#[cfg(test)]
pub mod mock {
    pub mod test_e2e_workflow;
    pub mod test_cache_flow;
}
EOF

# Update or create the main integration test file
cat > tests/integration_tests.rs << 'EOF'
// tests/integration_tests.rs
// Main integration test entry point

// Import the fabstir-llm-node library
extern crate fabstir_llm_node as crate;

// Include all integration test modules
mod integration;

// Re-export test modules
#[cfg(test)]
pub use integration::mock::*;
EOF

echo "✅ Test files created successfully!"
echo ""
echo "Directory structure:"
echo "tests/"
echo "├── integration_tests.rs"
echo "└── integration/"
echo "    ├── mod.rs"
echo "    └── mock/"
echo "        ├── test_e2e_workflow.rs"
echo "        └── test_cache_flow.rs"
echo ""
echo "Now you can run: ./run_integration_tests.sh"