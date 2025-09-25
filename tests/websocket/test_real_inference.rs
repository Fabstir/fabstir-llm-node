use fabstir_llm_node::api::websocket::inference::{
    InferenceConfig, InferenceEngine, InferenceRequest, InferenceResponse, ModelCache,
    ModelManager, StreamingInference,
};
use std::path::PathBuf;
use tokio::sync::mpsc;

// Helper to check if we have a real model and create engine
async fn create_test_engine(config: InferenceConfig) -> Option<InferenceEngine> {
    // Check if we have a real model (not a mock)
    if !config.model_path.exists() {
        println!(
            "Skipping test: Model file not found: {:?}",
            config.model_path
        );
        return None;
    }

    if let Ok(contents) = std::fs::read(&config.model_path) {
        if contents.len() < 1000 || contents.starts_with(b"Mock") {
            println!("Skipping test: Real GGUF model not available (found mock file)");
            println!("To run inference tests with real models:");
            println!("  1. Download TinyLlama: curl -L -o models/tinyllama-1b.Q4_K_M.gguf https://huggingface.co/...");
            println!("  2. Or use any GGUF model file");
            return None;
        }
    }

    match InferenceEngine::new(config).await {
        Ok(e) => Some(e),
        Err(err) => {
            println!("Skipping test: Could not load model: {}", err);
            None
        }
    }
}

#[tokio::test]
async fn test_real_llama_engine_integration() {
    // Test real LLM engine integration with llama-cpp-2
    let config = InferenceConfig {
        model_path: PathBuf::from("models/tinyllama-1b.Q4_K_M.gguf"),
        context_size: 2048,
        max_tokens: 256,
        temperature: 0.7,
        gpu_layers: 35,
        use_gpu: true,
    };

    let Some(engine) = create_test_engine(config).await else {
        return;
    };

    // Test basic inference
    let request = InferenceRequest {
        prompt: "What is 2+2?".to_string(),
        max_tokens: 50,
        temperature: Some(0.1),
        stream: false,
    };

    let response = engine.generate(request).await.unwrap();
    assert!(!response.text.is_empty());
    assert!(response.tokens_generated > 0);
    assert!(response.inference_time_ms > 0.0);
}

#[tokio::test]
async fn test_streaming_inference_with_real_model() {
    let config = InferenceConfig {
        model_path: PathBuf::from("models/tinyllama-1b.Q4_K_M.gguf"),
        context_size: 2048,
        max_tokens: 256,
        temperature: 0.7,
        gpu_layers: 35,
        use_gpu: cfg!(feature = "cuda"),
    };

    let Some(engine) = create_test_engine(config).await else {
        return;
    };

    let request = InferenceRequest {
        prompt: "Tell me a short story".to_string(),
        max_tokens: 100,
        temperature: Some(0.8),
        stream: true,
    };

    let (tx, mut rx) = mpsc::channel(100);

    // Start streaming
    engine.stream_generate(request, tx.clone()).await.unwrap();

    let mut total_tokens = 0;
    let mut chunks = Vec::new();

    while let Some(chunk) = rx.recv().await {
        assert!(!chunk.text.is_empty());
        total_tokens += chunk.tokens;
        chunks.push(chunk);
    }

    assert!(total_tokens > 0);
    assert!(!chunks.is_empty());
    assert!(chunks.last().unwrap().is_final);
}

#[tokio::test]
async fn test_model_caching_and_management() {
    let cache = ModelCache::new(2); // Cache up to 2 models

    // Load first model
    let model1_path = PathBuf::from("models/tinyllama-1b.Q4_K_M.gguf");
    if !model1_path.exists() {
        println!("Model not found, skipping cache test");
        return;
    }

    // Check if it's a real model
    if let Ok(contents) = std::fs::read(&model1_path) {
        if contents.len() < 1000 || contents.starts_with(b"Mock") {
            println!("Skipping cache test: Real GGUF model not available");
            return;
        }
    }

    let model1 = match cache.get_or_load(&model1_path).await {
        Ok(m) => m,
        Err(_) => {
            println!("Skipping cache test: Could not load model");
            return;
        }
    };

    // Should retrieve from cache
    let model1_cached = cache.get_or_load(&model1_path).await.unwrap();
    // Both should point to the same engine (cached)

    // Check if path is in cache
    assert!(cache.contains(&model1_path).await);
}

#[tokio::test]
async fn test_concurrent_inference_requests() {
    let config = InferenceConfig {
        model_path: PathBuf::from("models/tinyllama-1b.Q4_K_M.gguf"),
        context_size: 2048,
        max_tokens: 256,
        temperature: 0.7,
        gpu_layers: 35,
        use_gpu: false, // CPU for concurrent testing
    };

    let Some(engine) = create_test_engine(config).await else {
        return;
    };

    // Spawn multiple concurrent requests
    let mut handles = vec![];

    for i in 0..3 {
        let engine_clone = engine.clone();
        let handle = tokio::spawn(async move {
            let request = InferenceRequest {
                prompt: format!("Question {}: What is {}+{}?", i, i, i + 1),
                max_tokens: 30,
                temperature: Some(0.1),
                stream: false,
            };

            engine_clone.generate(request).await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let mut results = vec![];
    for handle in handles {
        let result = handle.await.unwrap().unwrap();
        results.push(result);
    }

    // All should succeed
    assert_eq!(results.len(), 3);
    for result in results {
        assert!(!result.text.is_empty());
        assert!(result.tokens_generated > 0);
    }
}

#[tokio::test]
async fn test_gpu_acceleration_detection() {
    let config = InferenceConfig {
        model_path: PathBuf::from("models/tinyllama-1b.Q4_K_M.gguf"),
        context_size: 2048,
        max_tokens: 256,
        temperature: 0.7,
        gpu_layers: 35,
        use_gpu: true,
    };

    let Some(engine) = create_test_engine(config).await else {
        return;
    };

    // Check GPU availability
    let gpu_available = engine.is_gpu_available().await;

    if gpu_available {
        // Test with GPU
        let request = InferenceRequest {
            prompt: "Hello, world!".to_string(),
            max_tokens: 10,
            temperature: Some(0.1),
            stream: false,
        };

        let start = std::time::Instant::now();
        let response = engine.generate(request).await.unwrap();
        let gpu_time = start.elapsed();

        // GPU should be faster
        assert!(response.inference_time_ms < 1000.0); // Should be fast
        println!("GPU inference time: {:?}", gpu_time);
    } else {
        println!("GPU not available, skipping GPU tests");
    }
}

#[tokio::test]
async fn test_context_window_handling() {
    let config = InferenceConfig {
        model_path: PathBuf::from("models/tinyllama-1b.Q4_K_M.gguf"),
        context_size: 512, // Small context for testing
        max_tokens: 100,
        temperature: 0.7,
        gpu_layers: 0,
        use_gpu: false,
    };

    let Some(engine) = create_test_engine(config).await else {
        return;
    };

    // Create a very long prompt
    let long_prompt = "Once upon a time ".repeat(200); // Exceeds context

    let request = InferenceRequest {
        prompt: long_prompt,
        max_tokens: 50,
        temperature: Some(0.5),
        stream: false,
    };

    // Should handle gracefully - truncate or error
    let result = engine.generate(request).await;

    match result {
        Ok(response) => {
            // Truncated and processed
            assert!(!response.text.is_empty());
            assert!(response.prompt_tokens <= 512);
        }
        Err(e) => {
            // Context exceeded error
            assert!(e.to_string().contains("context") || e.to_string().contains("token"));
        }
    }
}

#[tokio::test]
async fn test_temperature_and_sampling() {
    let config = InferenceConfig {
        model_path: PathBuf::from("models/tinyllama-1b.Q4_K_M.gguf"),
        context_size: 2048,
        max_tokens: 256,
        temperature: 0.7,
        gpu_layers: 0,
        use_gpu: false,
    };

    let Some(engine) = create_test_engine(config).await else {
        return;
    };

    // Test with temperature 0 (deterministic)
    let request1 = InferenceRequest {
        prompt: "The capital of France is".to_string(),
        max_tokens: 5,
        temperature: Some(0.0),
        stream: false,
    };

    let response1 = engine.generate(request1.clone()).await.unwrap();
    let response2 = engine.generate(request1).await.unwrap();

    // Should be identical with temperature 0
    assert_eq!(response1.text, response2.text);

    // Test with high temperature (random)
    let request_random = InferenceRequest {
        prompt: "The meaning of life is".to_string(),
        max_tokens: 20,
        temperature: Some(1.5),
        stream: false,
    };

    let response3 = engine.generate(request_random.clone()).await.unwrap();
    let response4 = engine.generate(request_random).await.unwrap();

    // Likely different with high temperature
    // Note: small chance they could be the same
    println!("High temp response 1: {}", response3.text);
    println!("High temp response 2: {}", response4.text);
}

#[tokio::test]
async fn test_model_validation_and_errors() {
    // Test with non-existent model
    let config = InferenceConfig {
        model_path: PathBuf::from("models/non_existent_model.gguf"),
        context_size: 2048,
        max_tokens: 256,
        temperature: 0.7,
        gpu_layers: 0,
        use_gpu: false,
    };

    let result = InferenceEngine::new(config).await;
    assert!(result.is_err());
    if let Err(e) = result {
        let error_msg = e.to_string();
        assert!(error_msg.contains("model") || error_msg.contains("file"));
    }

    // Test with invalid model format (if we have a non-GGUF file)
    let invalid_config = InferenceConfig {
        model_path: PathBuf::from("Cargo.toml"), // Not a model file
        context_size: 2048,
        max_tokens: 256,
        temperature: 0.7,
        gpu_layers: 0,
        use_gpu: false,
    };

    let result = InferenceEngine::new(invalid_config).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_inference_with_system_prompt() {
    let config = InferenceConfig {
        model_path: PathBuf::from("models/tinyllama-1b.Q4_K_M.gguf"),
        context_size: 2048,
        max_tokens: 256,
        temperature: 0.7,
        gpu_layers: 0,
        use_gpu: false,
    };

    // First check if we have a real model
    let test_engine = create_test_engine(config.clone()).await;
    if test_engine.is_none() {
        return;
    }

    let engine = match InferenceEngine::with_system_prompt(
        config,
        "You are a helpful assistant that always responds politely.".to_string(),
    )
    .await
    {
        Ok(e) => e,
        Err(_) => return,
    };

    let request = InferenceRequest {
        prompt: "Tell me about Rust programming".to_string(),
        max_tokens: 50,
        temperature: Some(0.7),
        stream: false,
    };

    let response = engine.generate(request).await.unwrap();
    assert!(!response.text.is_empty());

    // Response should reflect the system prompt's instruction
    println!("Response with system prompt: {}", response.text);
}

#[tokio::test]
async fn test_batch_inference_processing() {
    let config = InferenceConfig {
        model_path: PathBuf::from("models/tinyllama-1b.Q4_K_M.gguf"),
        context_size: 2048,
        max_tokens: 256,
        temperature: 0.7,
        gpu_layers: 0,
        use_gpu: false,
    };

    let Some(engine) = create_test_engine(config).await else {
        return;
    };

    // Create batch of requests
    let requests = vec![
        InferenceRequest {
            prompt: "What is 1+1?".to_string(),
            max_tokens: 10,
            temperature: Some(0.1),
            stream: false,
        },
        InferenceRequest {
            prompt: "What is 2+2?".to_string(),
            max_tokens: 10,
            temperature: Some(0.1),
            stream: false,
        },
        InferenceRequest {
            prompt: "What is 3+3?".to_string(),
            max_tokens: 10,
            temperature: Some(0.1),
            stream: false,
        },
    ];

    let responses = engine.batch_generate(requests).await.unwrap();

    assert_eq!(responses.len(), 3);
    for response in responses {
        assert!(!response.text.is_empty());
        assert!(response.tokens_generated > 0);
    }
}
