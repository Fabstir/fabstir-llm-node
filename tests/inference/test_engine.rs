use fabstir_llm_node::inference::{
    LlmEngine, EngineConfig, ModelConfig, InferenceRequest, InferenceResult,
    ChatMessage
};
use std::path::PathBuf;
use std::time::Duration;
use futures::StreamExt;

#[tokio::test]
async fn test_engine_initialization() {
    let config = EngineConfig {
        models_directory: PathBuf::from("./models"),
        max_loaded_models: 3,
        max_context_length: 4096,
        gpu_layers: 35,
        thread_count: 8,
        batch_size: 512,
        use_mmap: true,
        use_mlock: false,
        max_concurrent_inferences: 4,
        model_eviction_policy: "lru".to_string(),
    };
    
    let engine = LlmEngine::new(config).await.expect("Failed to create LLM engine");
    
    // Engine should be ready
    assert!(engine.is_ready());
    
    // Should report capabilities
    let capabilities = engine.capabilities();
    assert!(capabilities.max_context_length >= 2048);
    assert!(capabilities.supports_gpu == false); // Mock always returns false
}

#[tokio::test]
async fn test_model_loading() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    
    // Load a test model
    let model_path = PathBuf::from("./models/llama-2-7b-q4_0.gguf");
    let model_config = ModelConfig {
        model_path: model_path.clone(),
        model_type: "llama-7b".to_string(),
        context_size: 2048,
        gpu_layers: 35,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
    };
    
    let model_id = engine.load_model(model_config)
        .await
        .expect("Failed to load model");
    
    // Model should be loaded
    assert!(!model_id.is_empty());
    assert!(engine.is_model_loaded(&model_id));
    
    // Should be in loaded models list
    let loaded_models = engine.list_loaded_models();
    assert!(loaded_models.iter().any(|m| m.id == model_id));
}

#[tokio::test]
async fn test_inference_execution() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    
    // Load model
    let model_id = load_test_model(&mut engine).await;
    
    // Execute inference
    let request = InferenceRequest {
        model_id: model_id.clone(),
        prompt: "Once upon a time".to_string(),
        max_tokens: 50,
        temperature: 0.7,
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.1,
        seed: Some(42),
        stop_sequences: vec!["\n\n".to_string()],
        stream: false,
    };
    
    let result = engine.run_inference(request)
        .await
        .expect("Failed to run inference");
    
    // Should generate text
    assert!(!result.text.is_empty());
    assert!(result.tokens_generated > 0);
    assert!(result.tokens_generated <= 50);
    
    // Should include timing info
    assert!(result.generation_time.as_millis() > 0);
    assert!(result.tokens_per_second > 0.0);
}

#[tokio::test]
async fn test_streaming_inference() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    
    let model_id = load_test_model(&mut engine).await;
    
    let request = InferenceRequest {
        model_id,
        prompt: "The meaning of life is".to_string(),
        max_tokens: 100,
        temperature: 0.8,
        top_p: 0.95,
        top_k: 40,
        repeat_penalty: 1.0,
        seed: None,
        stop_sequences: vec![],
        stream: true,
    };
    
    let mut stream = engine.run_inference_stream(request)
        .await
        .expect("Failed to start streaming");
    
    let mut tokens_received = 0;
    let mut full_text = String::new();
    
    while let Some(token) = stream.next().await {
        let token = token.expect("Failed to receive token");
        full_text.push_str(&token.text);
        tokens_received += 1;
        
        // Should have timing info
        assert!(token.token_id >= 0);
        assert!(token.logprob.is_some());
    }
    
    assert!(tokens_received > 0);
    assert!(!full_text.is_empty());
}

#[tokio::test]
async fn test_multiple_concurrent_inferences() {
    let config = EngineConfig {
        max_concurrent_inferences: 3,
        ..Default::default()
    };
    
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    let model_id = load_test_model(&mut engine).await;
    
    // Start multiple inferences
    let mut handles = Vec::new();
    
    for i in 0..3 {
        let engine_clone = engine.clone();
        let model_id_clone = model_id.clone();
        
        let handle = tokio::spawn(async move {
            let request = InferenceRequest {
                model_id: model_id_clone,
                prompt: format!("Count to five: {}", i),
                max_tokens: 20,
                temperature: 0.5,
                top_p: 0.9,
                top_k: 40,
                repeat_penalty: 1.0,
                seed: Some(i as u64),
                stop_sequences: vec![],
                stream: false,
            };
            
            engine_clone.run_inference(request).await
        });
        
        handles.push(handle);
    }
    
    // All should complete
    let mut results = Vec::new();
    for handle in handles {
        let result = handle.await.expect("Task failed").expect("Inference failed");
        results.push(result);
    }
    
    assert_eq!(results.len(), 3);
    for result in results {
        assert!(!result.text.is_empty());
    }
}

#[tokio::test]
async fn test_context_window_management() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    
    let model_id = load_test_model(&mut engine).await;
    
    // Create a long prompt that approaches context limit
    let long_prompt = "Hello world. ".repeat(500); // ~6500 tokens
    
    let request = InferenceRequest {
        model_id,
        prompt: long_prompt,
        max_tokens: 100,
        temperature: 0.7,
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.0,
        seed: None,
        stop_sequences: vec![],
        stream: false,
    };
    
    // Should handle gracefully
    let result = engine.run_inference(request).await;
    
    match result {
        Ok(res) => {
            // If it fits, should generate
            assert!(!res.text.is_empty());
        }
        Err(e) => {
            // If too long, should error clearly
            assert!(e.to_string().contains("context") || e.to_string().contains("token"));
        }
    }
}

#[tokio::test]
async fn test_model_unloading() {
    let config = EngineConfig {
        max_loaded_models: 2,
        ..Default::default()
    };
    
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    
    // Load multiple models
    let model1 = load_test_model_with_name(&mut engine, "llama-7b").await;
    let model2 = load_test_model_with_name(&mut engine, "mistral-7b").await;
    
    assert_eq!(engine.list_loaded_models().len(), 2);
    
    // Unload first model
    engine.unload_model(&model1).await.expect("Failed to unload model");
    
    assert_eq!(engine.list_loaded_models().len(), 1);
    assert!(!engine.is_model_loaded(&model1));
    assert!(engine.is_model_loaded(&model2));
}

#[tokio::test]
async fn test_automatic_model_eviction() {
    let config = EngineConfig {
        max_loaded_models: 2,
        model_eviction_policy: "lru".to_string(),
        ..Default::default()
    };
    
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    
    // Load max models
    let model1 = load_test_model_with_name(&mut engine, "llama-7b").await;
    let model2 = load_test_model_with_name(&mut engine, "mistral-7b").await;
    
    // Use model1
    let _ = run_quick_inference(&engine, &model1).await;
    
    // Load third model - should evict model2 (LRU)
    let model3 = load_test_model_with_name(&mut engine, "codellama-7b").await;
    
    assert!(engine.is_model_loaded(&model1));
    assert!(!engine.is_model_loaded(&model2)); // Evicted
    assert!(engine.is_model_loaded(&model3));
}

#[tokio::test]
async fn test_inference_cancellation() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    
    let model_id = load_test_model(&mut engine).await;
    
    // Start long inference
    let request = InferenceRequest {
        model_id,
        prompt: "Write a very long story about".to_string(),
        max_tokens: 1000,
        temperature: 0.8,
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.0,
        seed: None,
        stop_sequences: vec![],
        stream: false,
    };
    
    let inference_handle = engine.run_inference_async(request).await;
    
    // Cancel after short delay
    tokio::time::sleep(Duration::from_millis(100)).await;
    inference_handle.cancel().await;
    
    // Should be cancelled
    let result = inference_handle.await;
    assert!(result.is_err() || result.unwrap().was_cancelled);
}

#[tokio::test]
async fn test_model_capabilities_detection() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    
    // Load different model types
    let llama_id = load_test_model_with_name(&mut engine, "llama-7b").await;
    let codellama_id = load_test_model_with_name(&mut engine, "codellama-7b").await;
    
    // Check capabilities
    let llama_caps = engine.get_model_capabilities(&llama_id)
        .expect("Failed to get capabilities");
    
    let codellama_caps = engine.get_model_capabilities(&codellama_id)
        .expect("Failed to get capabilities");
    
    // Llama should support general text
    assert!(llama_caps.supports_completion);
    assert!(llama_caps.supports_chat);
    
    // CodeLlama should support code
    assert!(codellama_caps.supports_code);
    assert!(codellama_caps.supports_fim); // Fill-in-middle
}

#[tokio::test]
async fn test_prompt_template_handling() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    
    let model_id = load_test_model(&mut engine).await;
    
    // Test chat format
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "You are a helpful assistant.".to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: "What is 2+2?".to_string(),
        },
    ];
    
    let chat_request = engine.create_chat_request(
        model_id.clone(),
        messages,
    );
    
    let result = engine.run_inference(chat_request)
        .await
        .expect("Failed to run chat inference");
    
    // Should generate appropriate response
    assert!(result.text.contains("4") || result.text.contains("four"));
}

#[tokio::test]
async fn test_token_counting() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    
    let model_id = load_test_model(&mut engine).await;
    
    // Count tokens for various prompts
    let test_cases = vec![
        ("Hello world", 2, 3),
        ("The quick brown fox jumps over the lazy dog", 9, 11),
        ("", 0, 1),
    ];
    
    for (prompt, min_tokens, max_tokens) in test_cases {
        let count = engine.count_tokens(&model_id, prompt)
            .await
            .expect("Failed to count tokens");
        
        assert!(count >= min_tokens && count <= max_tokens,
            "Token count {} for '{}' not in range {}-{}", 
            count, prompt, min_tokens, max_tokens);
    }
}

#[tokio::test]
async fn test_inference_metrics() {
    let config = EngineConfig::default();
    let mut engine = LlmEngine::new(config).await.expect("Failed to create engine");
    
    let model_id = load_test_model(&mut engine).await;
    
    // Reset metrics
    engine.reset_metrics();
    
    // Run some inferences
    for i in 0..3 {
        let request = InferenceRequest {
            model_id: model_id.clone(),
            prompt: format!("Test prompt {}", i),
            max_tokens: 20,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            repeat_penalty: 1.0,
            seed: Some(i as u64),
            stop_sequences: vec![],
            stream: false,
        };
        
        let _ = engine.run_inference(request).await;
    }
    
    // Check metrics
    let metrics = engine.get_metrics();
    
    assert_eq!(metrics.total_inferences, 3);
    assert!(metrics.total_tokens_generated > 0);
    assert!(metrics.average_tokens_per_second > 0.0);
    assert!(metrics.total_inference_time.as_millis() > 0);
}

// Helper functions
async fn load_test_model(engine: &mut LlmEngine) -> String {
    load_test_model_with_name(engine, "llama-7b").await
}

async fn load_test_model_with_name(engine: &mut LlmEngine, name: &str) -> String {
    let model_config = ModelConfig {
        model_path: PathBuf::from(format!("./models/{}-q4_0.gguf", name)),
        model_type: name.to_string(),
        context_size: 2048,
        gpu_layers: 35,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
    };
    
    engine.load_model(model_config)
        .await
        .expect("Failed to load model")
}

async fn run_quick_inference(engine: &LlmEngine, model_id: &str) -> InferenceResult {
    let request = InferenceRequest {
        model_id: model_id.to_string(),
        prompt: "Hello".to_string(),
        max_tokens: 5,
        temperature: 0.7,
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.0,
        seed: Some(42),
        stop_sequences: vec![],
        stream: false,
    };
    
    engine.run_inference(request)
        .await
        .expect("Failed to run inference")
}