// Integration test for GPT-OSS-20B model with actual inference
use fabstir_llm_node::inference::{ChatTemplate, InferenceEngine, InferenceRequest};
use std::path::PathBuf;

#[tokio::test]
async fn test_gpt_oss_20b_model_inference() {
    // This test requires the actual model file to be present
    let model_path = PathBuf::from("/app/models/openai_gpt-oss-20b-Q8_0.gguf");

    if !model_path.exists() {
        println!("⚠️ Skipping test - model file not found at {:?}", model_path);
        println!("   This test should be run in the Docker container with the model");
        return;
    }

    println!("🧪 Testing GPT-OSS-20B inference with REAL model file");

    // Create inference engine
    let config = fabstir_llm_node::inference::EngineConfig {
        batch_size: 512,
        context_size: 2048,
        n_threads: 4,
        use_gpu: true,
    };

    let engine = InferenceEngine::new(config);

    // Load the model
    println!("📦 Loading model from {:?}", model_path);
    let model_id = engine
        .load_model(model_path.clone())
        .await
        .expect("Failed to load GPT-OSS-20B model");

    println!("✅ Model loaded with ID: {}", model_id);

    // Test 1: Simple math (What is 2+2?)
    println!("\n🧪 TEST 1: What is 2+2?");

    let template = ChatTemplate::Harmony;
    let messages = vec![
        ("user".to_string(), "What is 2+2?".to_string()),
    ];
    let prompt = template.format_messages(&messages);

    println!("📝 Formatted prompt:\n{}", prompt);

    let request = InferenceRequest {
        model_id,
        prompt,
        max_tokens: 20,
        temperature: 0.1,
        top_p: 0.9,
        stream: false,
    };

    let result = engine.run_inference(request).await;

    match result {
        Ok(response) => {
            println!("✅ Response: {}", response.content);
            println!("   Tokens used: {}", response.tokens_used);

            // Check for garbage output
            let garbage_chars = response.content.matches('…').count()
                + response.content.matches("...").count();
            let total_chars = response.content.len();
            let garbage_ratio = (garbage_chars as f64) / (total_chars as f64);

            println!("   Garbage ratio: {:.2}%", garbage_ratio * 100.0);

            // Assert response is not garbage
            assert!(
                garbage_ratio < 0.5,
                "Response is mostly garbage! Got: {}",
                response.content
            );

            // Check response contains expected content
            let lower = response.content.to_lowercase();
            assert!(
                lower.contains("4") || lower.contains("four"),
                "Response should contain the answer '4'. Got: {}",
                response.content
            );
        }
        Err(e) => {
            panic!("❌ Inference failed: {}", e);
        }
    }

    // Test 2: Capital question
    println!("\n🧪 TEST 2: What is the capital of France?");

    let messages2 = vec![
        ("user".to_string(), "What is the capital of France?".to_string()),
    ];
    let prompt2 = template.format_messages(&messages2);

    let request2 = InferenceRequest {
        model_id,
        prompt: prompt2,
        max_tokens: 20,
        temperature: 0.1,
        top_p: 0.9,
        stream: false,
    };

    let result2 = engine.run_inference(request2).await;

    match result2 {
        Ok(response) => {
            println!("✅ Response: {}", response.content);

            // Check for garbage output
            let garbage_chars = response.content.matches('…').count()
                + response.content.matches("...").count();
            let total_chars = response.content.len();
            let garbage_ratio = (garbage_chars as f64) / (total_chars as f64);

            println!("   Garbage ratio: {:.2}%", garbage_ratio * 100.0);

            assert!(
                garbage_ratio < 0.5,
                "Response is mostly garbage! Got: {}",
                response.content
            );

            // Check response contains expected content
            let lower = response.content.to_lowercase();
            assert!(
                lower.contains("paris"),
                "Response should mention Paris. Got: {}",
                response.content
            );
        }
        Err(e) => {
            panic!("❌ Inference failed: {}", e);
        }
    }

    println!("\n✅ ALL TESTS PASSED! Model generates clean responses, not garbage!");
}

#[test]
fn test_harmony_template_format() {
    // Verify the template format is correct
    let template = ChatTemplate::Harmony;
    let messages = vec![
        ("user".to_string(), "Hello".to_string()),
    ];

    let formatted = template.format_messages(&messages);

    println!("Formatted prompt:\n{}", formatted);

    // Must have system message
    assert!(formatted.contains("<|start|>system<|message|>"));
    assert!(formatted.contains("You are ChatGPT"));

    // Must have user message
    assert!(formatted.contains("<|start|>user<|message|>Hello<|end|>"));

    // Must end with assistant prompt
    assert!(formatted.ends_with("<|start|>assistant<|message|>"));

    // Must NOT use ChatML format
    assert!(!formatted.contains("<|im_start|>"));
    assert!(!formatted.contains("<|im_end|>"));
}
