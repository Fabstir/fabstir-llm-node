use fabstir_llm_node::{
    api::{ApiConfig, ApiServer},
    inference::{EngineConfig, LlmEngine, ModelConfig, InferenceRequest, InferenceResult},
};
use reqwest;
use serde_json::json;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::time::timeout;

const TEST_MODEL_PATH: &str = "models/tiny-vicuna-1b.q4_k_m.gguf";
const TEST_API_PORT: u16 = 8089; // Use different port for tests

async fn setup_test_server() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize engine with real model
    let engine_config = EngineConfig {
        models_directory: PathBuf::from("./models"),
        max_loaded_models: 1,
        max_context_length: 2048,
        gpu_layers: 0, // CPU for tests, can be changed to test GPU
        thread_count: 4,
        batch_size: 512,
        use_mmap: true,
        use_mlock: false,
        max_concurrent_inferences: 2,
        model_eviction_policy: "lru".to_string(),
    };

    let mut engine = LlmEngine::new(engine_config).await?;
    
    // Load the real model
    let model_config = ModelConfig {
        model_path: PathBuf::from(TEST_MODEL_PATH),
        model_type: "llama".to_string(),
        context_size: 2048,
        gpu_layers: 0,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
    };
    
    let model_id = engine.load_model(model_config).await?;
    println!("Loaded model with ID: {}", model_id);

    // Start API server
    let api_config = ApiConfig {
        listen_addr: format!("127.0.0.1:{}", TEST_API_PORT),
        enable_websocket: true,
        cors_allowed_origins: vec!["*".to_string()],
        ..Default::default()
    };

    let api_server = ApiServer::new(api_config).await?;
    
    // Start server in background
    tokio::spawn(async move {
        if let Err(e) = api_server.run().await {
            eprintln!("API server error: {}", e);
        }
    });

    // Wait for server to start
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    Ok(())
}

#[tokio::test]
async fn test_load_real_model_on_startup() {
    // Test that the model loads successfully
    let engine_config = EngineConfig {
        models_directory: PathBuf::from("./models"),
        max_loaded_models: 1,
        max_context_length: 2048,
        gpu_layers: 0,
        thread_count: 4,
        batch_size: 512,
        use_mmap: true,
        use_mlock: false,
        max_concurrent_inferences: 2,
        model_eviction_policy: "lru".to_string(),
    };

    let mut engine = LlmEngine::new(engine_config).await
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
        .expect("Failed to load real GGUF model");
    
    assert!(!model_id.is_empty(), "Model ID should not be empty");
    
    // Verify model is actually loaded
    let models = engine.list_loaded_models().await;
    assert!(models.contains(&model_id), "Model should be in loaded models list");
}

#[tokio::test]
async fn test_inference_with_real_model() {
    // Setup server with real model
    setup_test_server().await.expect("Failed to setup test server");
    
    // Make inference request
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://localhost:{}/v1/inference", TEST_API_PORT))
        .json(&json!({
            "model": "tiny-vicuna",
            "prompt": "What is the capital of France?",
            "max_tokens": 50,
            "temperature": 0.7,
            "stream": false
        }))
        .send()
        .await
        .expect("Failed to send request");
    
    assert_eq!(response.status(), 200, "Should return 200 OK");
    
    let result: serde_json::Value = response.json().await
        .expect("Failed to parse response");
    
    // Verify response structure
    assert!(result.get("content").is_some(), "Response should have content field");
    assert!(result.get("tokens_used").is_some(), "Response should have tokens_used field");
    assert!(result.get("model").is_some(), "Response should have model field");
    
    // Verify it's not mock data
    let content = result["content"].as_str().unwrap();
    assert!(!content.contains("Response to:"), "Should not contain mock response pattern");
    assert!(!content.contains("(generated"), "Should not contain mock token count");
    
    // Content should be non-empty and reasonable
    assert!(content.len() > 10, "Response should have meaningful content");
    
    let tokens_used = result["tokens_used"].as_u64().unwrap();
    assert!(tokens_used > 0 && tokens_used <= 50, "Tokens used should be within limit");
}

#[tokio::test]
async fn test_streaming_inference_real_model() {
    setup_test_server().await.expect("Failed to setup test server");
    
    let client = reqwest::Client::new();
    let mut response = client
        .post(format!("http://localhost:{}/v1/inference", TEST_API_PORT))
        .json(&json!({
            "model": "tiny-vicuna",
            "prompt": "Write a short story about a robot:",
            "max_tokens": 100,
            "temperature": 0.8,
            "stream": true
        }))
        .send()
        .await
        .expect("Failed to send streaming request");
    
    assert_eq!(response.status(), 200);
    
    // Collect streamed chunks
    let mut chunks = Vec::new();
    let mut total_tokens = 0;
    
    while let Some(chunk) = response.chunk().await.expect("Failed to get chunk") {
        let text = String::from_utf8_lossy(&chunk);
        if text.starts_with("data: ") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text[6..]) {
                chunks.push(json.clone());
                if let Some(tokens) = json.get("tokens").and_then(|t| t.as_u64()) {
                    total_tokens += tokens;
                }
            }
        }
    }
    
    // Verify we got multiple chunks (streaming behavior)
    assert!(chunks.len() > 1, "Should receive multiple chunks when streaming");
    assert!(total_tokens > 0, "Should have generated real tokens");
    
    // Verify content is not mock
    let has_real_content = chunks.iter().any(|chunk| {
        chunk.get("content")
            .and_then(|c| c.as_str())
            .map(|s| !s.is_empty() && !s.contains("mock"))
            .unwrap_or(false)
    });
    assert!(has_real_content, "Should have real generated content");
}

#[tokio::test]
async fn test_performance_metrics() {
    setup_test_server().await.expect("Failed to setup test server");
    
    let client = reqwest::Client::new();
    let start = Instant::now();
    
    let response = client
        .post(format!("http://localhost:{}/v1/inference", TEST_API_PORT))
        .json(&json!({
            "model": "tiny-vicuna",
            "prompt": "Count from 1 to 10:",
            "max_tokens": 100,
            "temperature": 0.5,
            "stream": false
        }))
        .send()
        .await
        .expect("Failed to send request");
    
    let elapsed = start.elapsed();
    let result: serde_json::Value = response.json().await
        .expect("Failed to parse response");
    
    let tokens_used = result["tokens_used"].as_u64().unwrap() as f32;
    let tokens_per_second = tokens_used / elapsed.as_secs_f32();
    
    println!("Performance: {:.1} tokens/second", tokens_per_second);
    println!("Generated {} tokens in {:.2}s", tokens_used, elapsed.as_secs_f32());
    
    // Verify performance meets minimum requirements
    assert!(
        tokens_per_second >= 20.0,
        "Should generate at least 20 tokens/second, got {:.1}",
        tokens_per_second
    );
    
    // Verify first token latency
    assert!(
        elapsed < Duration::from_millis(500),
        "First token should arrive within 500ms"
    );
}

#[tokio::test]
async fn test_concurrent_requests() {
    setup_test_server().await.expect("Failed to setup test server");
    
    let client = reqwest::Client::new();
    let mut handles = Vec::new();
    
    // Send 3 concurrent requests
    for i in 0..3 {
        let client = client.clone();
        let handle = tokio::spawn(async move {
            let response = client
                .post(format!("http://localhost:{}/v1/inference", TEST_API_PORT))
                .json(&json!({
                    "model": "tiny-vicuna",
                    "prompt": format!("Request {}: What is {}?", i, i),
                    "max_tokens": 20,
                    "temperature": 0.7,
                    "stream": false
                }))
                .send()
                .await
                .expect("Failed to send concurrent request");
            
            assert_eq!(response.status(), 200);
            let result: serde_json::Value = response.json().await
                .expect("Failed to parse concurrent response");
            
            result
        });
        handles.push(handle);
    }
    
    // Wait for all requests with timeout
    let results = timeout(Duration::from_secs(30), async {
        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await.expect("Task panicked"));
        }
        results
    }).await.expect("Concurrent requests timed out");
    
    // Verify all requests succeeded
    assert_eq!(results.len(), 3);
    for (i, result) in results.iter().enumerate() {
        assert!(result.get("content").is_some(), "Request {} should have content", i);
        let content = result["content"].as_str().unwrap();
        assert!(!content.is_empty(), "Request {} should have non-empty content", i);
    }
}

#[tokio::test]
async fn test_model_not_found_error() {
    setup_test_server().await.expect("Failed to setup test server");
    
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://localhost:{}/v1/inference", TEST_API_PORT))
        .json(&json!({
            "model": "nonexistent-model",
            "prompt": "Test",
            "max_tokens": 10,
            "temperature": 0.7,
            "stream": false
        }))
        .send()
        .await
        .expect("Failed to send request");
    
    // Should return an error status
    assert!(
        response.status().is_client_error() || response.status().is_server_error(),
        "Should return error for non-existent model"
    );
}