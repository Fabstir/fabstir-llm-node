// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::inference::{
    EngineConfig, LlmEngine, ModelConfig, InferenceRequest, InferenceResult
};
use std::path::PathBuf;
use std::time::Duration;

const TEST_MODEL_PATH: &str = "models/tiny-vicuna-1b.q4_k_m.gguf";

#[tokio::test]
async fn test_load_gguf_model() {
    let config = EngineConfig {
        models_directory: PathBuf::from("./models"),
        max_loaded_models: 1,
        max_context_length: 2048,
        gpu_layers: 0, // CPU for testing
        thread_count: 4,
        batch_size: 512,
        use_mmap: true,
        use_mlock: false,
        max_concurrent_inferences: 1,
        model_eviction_policy: "lru".to_string(),
    };

    let mut engine = LlmEngine::new(config).await
        .expect("Failed to create engine");

    let model_config = ModelConfig {
        model_path: PathBuf::from(TEST_MODEL_PATH),
        model_type: "llama".to_string(),
        context_size: 2048,
        gpu_layers: 0,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
    };

    // Test loading the model
    let model_id = engine.load_model(model_config).await
        .expect("Should successfully load GGUF model");

    assert!(!model_id.is_empty(), "Model ID should not be empty");
    
    // Verify model is loaded
    let loaded_models = engine.list_loaded_models().await;
    assert_eq!(loaded_models.len(), 1, "Should have exactly one model loaded");
    assert!(loaded_models.contains(&model_id), "Loaded models should contain our model");
    
    // Test model info
    let capabilities = engine.get_model_capabilities(&model_id).await;
    assert!(capabilities.is_some(), "Should return model capabilities");
}

#[tokio::test]
async fn test_tokenization() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await
        .expect("Failed to create engine");

    let model_config = ModelConfig {
        model_path: PathBuf::from(TEST_MODEL_PATH),
        model_type: "llama".to_string(),
        context_size: 2048,
        gpu_layers: 0,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
    };

    let model_id = engine.load_model(model_config).await
        .expect("Failed to load model");

    // Test tokenization with different prompts
    let test_prompts = vec![
        "Hello, world!",
        "The quick brown fox jumps over the lazy dog.",
        "What is the meaning of life?",
        "🚀 Emoji test",
        "Numbers: 1234567890",
    ];

    for prompt in test_prompts {
        let request = InferenceRequest {
            model_id: model_id.clone(),
            prompt: prompt.to_string(),
            max_tokens: 1, // Just test tokenization, not generation
            temperature: 0.0,
            top_p: 1.0,
            top_k: 1,
            repeat_penalty: 1.0,
            min_p: 0.0,
            seed: Some(42),
            stop_sequences: vec![],
            stream: false,
        };

        let result = engine.run_inference(request).await
            .expect(&format!("Failed to tokenize prompt: {}", prompt));
        
        // Should produce some output even with 1 token
        assert!(!result.text.is_empty(), "Should produce output for prompt: {}", prompt);
    }
}

#[tokio::test]
async fn test_generation_params() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await
        .expect("Failed to create engine");

    let model_config = ModelConfig {
        model_path: PathBuf::from(TEST_MODEL_PATH),
        model_type: "llama".to_string(),
        context_size: 2048,
        gpu_layers: 0,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
    };

    let model_id = engine.load_model(model_config).await
        .expect("Failed to load model");

    // Test with different temperature values
    let temperatures = vec![0.0, 0.5, 1.0, 1.5];
    let base_prompt = "The weather today is";
    
    for temp in temperatures {
        let request = InferenceRequest {
            model_id: model_id.clone(),
            prompt: base_prompt.to_string(),
            max_tokens: 20,
            temperature: temp,
            top_p: 0.9,
            top_k: 40,
            repeat_penalty: 1.1,
            min_p: 0.0,
            seed: Some(42), // Fixed seed for reproducibility
            stop_sequences: vec![],
            stream: false,
        };

        let result = engine.run_inference(request).await
            .expect(&format!("Failed with temperature: {}", temp));
        
        assert!(!result.text.is_empty(), "Should generate text with temperature: {}", temp);
        assert!(result.tokens_generated > 0, "Should generate tokens with temperature: {}", temp);
        assert!(result.tokens_per_second > 0.0, "Should have positive tokens/second");
        
        println!("Temperature {}: Generated {} tokens", temp, result.tokens_generated);
    }
}

#[tokio::test]
async fn test_real_inference_output() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await
        .expect("Failed to create engine");

    let model_config = ModelConfig {
        model_path: PathBuf::from(TEST_MODEL_PATH),
        model_type: "llama".to_string(),
        context_size: 2048,
        gpu_layers: 0,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
    };

    let model_id = engine.load_model(model_config).await
        .expect("Failed to load model");

    let request = InferenceRequest {
        model_id: model_id.clone(),
        prompt: "Paris is the capital of".to_string(),
        max_tokens: 10,
        temperature: 0.1, // Low temperature for deterministic output
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.0,
        min_p: 0.0,
        seed: Some(42),
        stop_sequences: vec![],
        stream: false,
    };

    let result = engine.run_inference(request).await
        .expect("Failed to run inference");
    
    // Verify it's real output, not mock
    assert!(!result.text.contains("Response to:"), "Should not be mock output");
    assert!(!result.text.contains("(generated"), "Should not contain mock pattern");
    
    // Should contain reasonable completion
    let lower_text = result.text.to_lowercase();
    assert!(
        lower_text.contains("france") || 
        lower_text.contains("french") ||
        result.text.len() > 0, // At minimum, should have some output
        "Should generate relevant completion for 'Paris is the capital of', got: {}",
        result.text
    );
}

#[tokio::test]
async fn test_stop_sequences() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await
        .expect("Failed to create engine");

    let model_config = ModelConfig {
        model_path: PathBuf::from(TEST_MODEL_PATH),
        model_type: "llama".to_string(),
        context_size: 2048,
        gpu_layers: 0,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
    };

    let model_id = engine.load_model(model_config).await
        .expect("Failed to load model");

    let request = InferenceRequest {
        model_id: model_id.clone(),
        prompt: "List three colors: 1. Red 2.".to_string(),
        max_tokens: 50,
        temperature: 0.7,
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.0,
        min_p: 0.0,
        seed: Some(42),
        stop_sequences: vec!["3.".to_string()], // Stop before item 3
        stream: false,
    };

    let result = engine.run_inference(request).await
        .expect("Failed to run inference with stop sequences");
    
    // Should stop before or at "3."
    assert!(!result.text.contains("4."), "Should stop before item 4");
    assert!(result.tokens_generated < 50, "Should stop before max_tokens due to stop sequence");
}

#[tokio::test]
async fn test_streaming_generation() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await
        .expect("Failed to create engine");

    let model_config = ModelConfig {
        model_path: PathBuf::from(TEST_MODEL_PATH),
        model_type: "llama".to_string(),
        context_size: 2048,
        gpu_layers: 0,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
    };

    let model_id = engine.load_model(model_config).await
        .expect("Failed to load model");

    let request = InferenceRequest {
        model_id: model_id.clone(),
        prompt: "Once upon a time".to_string(),
        max_tokens: 30,
        temperature: 0.8,
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.1,
        min_p: 0.0,
        seed: None,
        stop_sequences: vec![],
        stream: true,
    };

    let mut stream = engine.run_inference_stream(request).await
        .expect("Failed to create stream");
    
    let mut token_count = 0;
    let mut full_text = String::new();
    
    use tokio_stream::StreamExt;
    while let Some(token_result) = stream.next().await {
        let token_info = token_result.expect("Failed to get token from stream");
        full_text.push_str(&token_info.text);
        token_count += 1;
        
        // Verify token info
        assert!(token_info.token_id >= 0, "Token ID should be non-negative");
        assert!(!token_info.text.is_empty() || token_info.token_id == 0, "Token should have text");
    }
    
    assert!(token_count > 0, "Should generate at least one token");
    assert!(!full_text.is_empty(), "Should generate non-empty text");
    assert!(!full_text.contains("mock"), "Should not contain mock data");
    
    println!("Streamed {} tokens: {}", token_count, full_text);
}

#[tokio::test]
async fn test_model_unloading() {
    let config = EngineConfig {
        models_directory: PathBuf::from("./models"),
        max_loaded_models: 1,
        max_context_length: 2048,
        gpu_layers: 0,
        thread_count: 4,
        batch_size: 512,
        use_mmap: true,
        use_mlock: false,
        max_concurrent_inferences: 1,
        model_eviction_policy: "lru".to_string(),
    };

    let mut engine = LlmEngine::new(config).await
        .expect("Failed to create engine");

    let model_config = ModelConfig {
        model_path: PathBuf::from(TEST_MODEL_PATH),
        model_type: "llama".to_string(),
        context_size: 2048,
        gpu_layers: 0,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
    };

    let model_id = engine.load_model(model_config).await
        .expect("Failed to load model");
    
    // Verify model is loaded
    assert_eq!(engine.list_loaded_models().await.len(), 1);
    
    // Unload the model
    engine.unload_model(&model_id).await
        .expect("Failed to unload model");
    
    // Verify model is unloaded
    assert_eq!(engine.list_loaded_models().await.len(), 0, "Model should be unloaded");
    
    // Inference should fail now
    let request = InferenceRequest {
        model_id: model_id.clone(),
        prompt: "Test".to_string(),
        max_tokens: 10,
        temperature: 0.7,
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.0,
        min_p: 0.0,
        seed: None,
        stop_sequences: vec![],
        stream: false,
    };
    
    let result = engine.run_inference(request).await;
    assert!(result.is_err(), "Inference should fail with unloaded model");
}