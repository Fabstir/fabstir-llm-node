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
use fabstir_llm_node::{
    embeddings::{EmbeddingConfig, EmbeddingGenerator},
    models::{ModelConfig, ModelMetadata, ModelRegistry},
    storage::{EnhancedS5Client, S5Config},
    vector::{VectorDbClient, VectorDbConfig},
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
    let cid = s5_client
        .put(&path, model_data.to_vec(), Some(model_metadata))
        .await?;

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

#[tokio::test]
async fn test_store_embeddings_in_vector_db() -> Result<()> {
    // Initialize Vector DB client
    let vector_config = VectorDbConfig {
        api_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        api_key: Some("test-vector-key".to_string()),
        timeout_secs: 30,
    };
    let vector_client = VectorDbClient::new(vector_config)?;

    // Initialize embedding generator
    let embedding_config = EmbeddingConfig {
        model: "all-MiniLM-L6-v2".to_string(),
        dimension: 384,
        batch_size: 32,
        normalize: true,
    };
    let generator = EmbeddingGenerator::new(embedding_config).await?;

    // Generate embeddings for model descriptions
    let model_descriptions = vec![
        ("llama-3.2-1b", "Small efficient model for edge devices"),
        ("llama-3.2-3b", "Medium model with good performance balance"),
        ("llama-3.2-7b", "Large model for complex reasoning tasks"),
    ];

    for (model_id, description) in model_descriptions {
        let embedding = generator.generate(description).await?;

        let metadata = json!({
            "model_id": model_id,
            "description": description,
            "type": "llm",
            "created_at": "2025-01-06T10:00:00Z"
        });

        // Store embedding in vector DB
        let result = vector_client
            .insert_vector(model_id, embedding, metadata)
            .await?;
        assert_eq!(result, model_id);
    }

    // Verify retrieval
    let retrieved = vector_client.get_vector("llama-3.2-1b").await?;
    assert_eq!(retrieved["id"], "llama-3.2-1b");
    assert!(retrieved["metadata"]["model_id"].is_string());

    Ok(())
}

#[tokio::test]
async fn test_semantic_search_for_similar_models() -> Result<()> {
    // Initialize clients
    let vector_config = VectorDbConfig {
        api_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        api_key: Some("test-vector-key".to_string()),
        timeout_secs: 30,
    };
    let vector_client = VectorDbClient::new(vector_config)?;

    let embedding_config = EmbeddingConfig {
        model: "all-MiniLM-L6-v2".to_string(),
        dimension: 384,
        batch_size: 32,
        normalize: true,
    };
    let generator = EmbeddingGenerator::new(embedding_config).await?;

    // Store various model embeddings
    let models = vec![
        (
            "code-llama-7b",
            "Specialized model for code generation and completion",
        ),
        (
            "mistral-7b-instruct",
            "Instruction-tuned model for following prompts",
        ),
        ("phi-2", "Small efficient model for reasoning tasks"),
        (
            "vicuna-13b",
            "Large conversational model with chat capabilities",
        ),
        (
            "codegen-6b",
            "Model optimized for code synthesis and generation",
        ),
    ];

    for (model_id, description) in &models {
        let embedding = generator.generate(description).await?;
        let metadata = json!({
            "model_id": model_id,
            "description": description,
            "capabilities": if model_id.contains("code") {
                vec!["code_generation"]
            } else {
                vec!["general"]
            }
        });
        vector_client
            .insert_vector(model_id, embedding, metadata)
            .await?;
    }

    // Search for code generation models
    let query = "I need a model for writing Python code";
    let query_embedding = generator.generate(query).await?;

    let filter = Some(json!({
        "capabilities": ["code_generation"]
    }));

    let results = vector_client.search(query_embedding, 3, filter).await?;

    assert!(!results.is_empty());
    assert!(results.len() <= 3);

    // Verify results contain code-related models
    for result in &results {
        if let Some(metadata) = result.get("metadata") {
            if let Some(caps) = metadata.get("capabilities") {
                assert!(caps.as_array().unwrap().contains(&json!("code_generation")));
            }
        }
        assert!(result["score"].as_f64().unwrap() >= 0.0);
        assert!(result["score"].as_f64().unwrap() <= 1.0);
    }

    Ok(())
}

#[tokio::test]
async fn test_complete_workflow_integration() -> Result<()> {
    // Initialize all clients
    let s5_config = S5Config {
        api_url: "http://enhanced-s5-container:5050".to_string(),
        api_key: Some("test-api-key".to_string()),
        timeout_secs: 30,
    };
    let s5_client = EnhancedS5Client::new(s5_config)?;

    let vector_config = VectorDbConfig {
        api_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        api_key: Some("test-vector-key".to_string()),
        timeout_secs: 30,
    };
    let vector_client = VectorDbClient::new(vector_config)?;

    let embedding_config = EmbeddingConfig {
        model: "all-MiniLM-L6-v2".to_string(),
        dimension: 384,
        batch_size: 32,
        normalize: true,
    };
    let generator = EmbeddingGenerator::new(embedding_config).await?;

    // Complete workflow for a model
    let model_id = "phi-3-mini";
    let model_data = b"Mock GGUF model data for Phi-3 Mini";
    let model_description = "Phi-3 Mini is a lightweight 3.8B parameter model from Microsoft, \
        optimized for mobile and edge deployment with strong reasoning capabilities";

    // 1. Store model in S5
    let model_metadata = json!({
        "name": model_id,
        "version": "1.0.0",
        "parameters": "3.8B",
        "format": "gguf",
        "quantization": "q4_k_m",
        "size_bytes": model_data.len(),
    });

    let path = format!("/models/{}/model.gguf", model_id);
    let cid = s5_client
        .put(&path, model_data.to_vec(), Some(model_metadata.clone()))
        .await?;
    assert!(cid.starts_with("b"));

    // 2. Generate embeddings
    let embedding = generator.generate(model_description).await?;
    assert_eq!(embedding.len(), 384);

    // 3. Store in vector DB with S5 reference
    let vector_metadata = json!({
        "model_id": model_id,
        "description": model_description,
        "s5_cid": cid,
        "s5_path": path,
        "model_metadata": model_metadata,
    });

    let vector_id = vector_client
        .insert_vector(model_id, embedding, vector_metadata)
        .await?;
    assert_eq!(vector_id, model_id);

    // 4. Search for the model
    let search_query = "I need a small efficient model for mobile deployment";
    let query_embedding = generator.generate(search_query).await?;
    let results = vector_client.search(query_embedding, 5, None).await?;

    assert!(!results.is_empty());

    // 5. Retrieve model from S5 using CID from search results
    if let Some(first_result) = results.first() {
        if let Some(metadata) = first_result.get("metadata") {
            if let Some(s5_path) = metadata.get("s5_path") {
                let (retrieved_data, retrieved_metadata) =
                    s5_client.get(s5_path.as_str().unwrap()).await?;
                assert_eq!(retrieved_data, model_data);
                assert_eq!(retrieved_metadata.unwrap()["name"], model_id);
            }
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_model_discovery_by_capability() -> Result<()> {
    // Initialize clients
    let vector_config = VectorDbConfig {
        api_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        api_key: Some("test-vector-key".to_string()),
        timeout_secs: 30,
    };
    let vector_client = VectorDbClient::new(vector_config)?;

    let embedding_config = EmbeddingConfig {
        model: "all-MiniLM-L6-v2".to_string(),
        dimension: 384,
        batch_size: 32,
        normalize: true,
    };
    let generator = EmbeddingGenerator::new(embedding_config).await?;

    // Store models with different capabilities
    let models_with_capabilities = vec![
        (
            "starcoder-3b",
            "Code generation model",
            vec!["code", "completion"],
        ),
        (
            "llama-guard",
            "Safety and moderation model",
            vec!["safety", "moderation"],
        ),
        (
            "stable-diffusion-xl",
            "Image generation model",
            vec!["image", "generation"],
        ),
        (
            "whisper-large",
            "Speech recognition model",
            vec!["audio", "transcription"],
        ),
        (
            "bert-base",
            "Text classification model",
            vec!["classification", "nlp"],
        ),
    ];

    for (model_id, description, capabilities) in &models_with_capabilities {
        let embedding = generator.generate(description).await?;
        let metadata = json!({
            "model_id": model_id,
            "description": description,
            "capabilities": capabilities,
            "domain": if capabilities.contains(&"code") { "programming" }
                     else if capabilities.contains(&"image") { "visual" }
                     else if capabilities.contains(&"audio") { "audio" }
                     else { "text" }
        });
        vector_client
            .insert_vector(model_id, embedding, metadata)
            .await?;
    }

    // Test 1: Find code generation models
    let code_query = "I need to generate Python functions";
    let code_embedding = generator.generate(code_query).await?;
    let code_filter = Some(json!({
        "capabilities": ["code"]
    }));

    let code_results = vector_client.search(code_embedding, 2, code_filter).await?;
    assert!(!code_results.is_empty());

    // Test 2: Find safety models
    let safety_query = "Content moderation and safety checking";
    let safety_embedding = generator.generate(safety_query).await?;
    let safety_filter = Some(json!({
        "capabilities": ["safety"]
    }));

    let safety_results = vector_client
        .search(safety_embedding, 2, safety_filter)
        .await?;
    assert!(!safety_results.is_empty());

    // Test 3: Find models by domain
    let audio_query = "Process speech and audio";
    let audio_embedding = generator.generate(audio_query).await?;
    let audio_filter = Some(json!({
        "domain": "audio"
    }));

    let audio_results = vector_client
        .search(audio_embedding, 2, audio_filter)
        .await?;
    assert!(!audio_results.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_model_versioning_workflow() -> Result<()> {
    // Initialize clients
    let s5_config = S5Config {
        api_url: "http://enhanced-s5-container:5050".to_string(),
        api_key: Some("test-api-key".to_string()),
        timeout_secs: 30,
    };
    let s5_client = EnhancedS5Client::new(s5_config)?;

    let vector_config = VectorDbConfig {
        api_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        api_key: Some("test-vector-key".to_string()),
        timeout_secs: 30,
    };
    let vector_client = VectorDbClient::new(vector_config)?;

    let embedding_config = EmbeddingConfig {
        model: "all-MiniLM-L6-v2".to_string(),
        dimension: 384,
        batch_size: 32,
        normalize: true,
    };
    let generator = EmbeddingGenerator::new(embedding_config).await?;

    let base_model_id = "gemma-2b";
    let versions = vec![
        (
            "1.0.0",
            "Initial release of Gemma 2B",
            b"Gemma 2B v1.0.0 data",
        ),
        (
            "1.1.0",
            "Gemma 2B with improved reasoning",
            b"Gemma 2B v1.1.0 data",
        ),
        (
            "2.0.0",
            "Gemma 2B major update with new architecture",
            b"Gemma 2B v2.0.0 data",
        ),
    ];

    let mut version_cids = Vec::new();

    for (version, description, data) in &versions {
        // Store each version in S5
        let versioned_id = format!("{}-{}", base_model_id, version);
        let path = format!("/models/{}/v{}/model.gguf", base_model_id, version);

        let metadata = json!({
            "model_id": base_model_id,
            "version": version,
            "description": description,
            "size_bytes": data.len(),
            "created_at": "2025-01-06T10:00:00Z",
        });

        let cid = s5_client
            .put(&path, data.to_vec(), Some(metadata.clone()))
            .await?;
        version_cids.push((version.to_string(), cid.clone()));

        // Generate and store embeddings for each version
        let embedding = generator.generate(description).await?;
        let vector_metadata = json!({
            "model_id": base_model_id,
            "version": version,
            "versioned_id": versioned_id,
            "description": description,
            "s5_cid": cid,
            "s5_path": path,
            "is_latest": version == &"2.0.0",
        });

        vector_client
            .insert_vector(&versioned_id, embedding, vector_metadata)
            .await?;
    }

    // Search for latest version
    let query = "Gemma model with best performance";
    let query_embedding = generator.generate(query).await?;
    let filter = Some(json!({
        "model_id": base_model_id,
        "is_latest": true
    }));

    let results = vector_client.search(query_embedding, 1, filter).await?;
    assert_eq!(results.len(), 1);

    if let Some(result) = results.first() {
        if let Some(metadata) = result.get("metadata") {
            assert_eq!(metadata["version"], "2.0.0");
            assert_eq!(metadata["is_latest"], true);
        }
    }

    // Verify we can retrieve any version from S5
    for (version, cid) in &version_cids {
        let path = format!("/models/{}/v{}/model.gguf", base_model_id, version);
        let (data, metadata) = s5_client.get(&path).await?;
        assert!(!data.is_empty());
        assert_eq!(metadata.unwrap()["version"], version.as_str());
    }

    Ok(())
}
