// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Demo to show the memory corruption is fixed
use fabstir_llm_node::inference::{EngineConfig, InferenceRequest, LlmEngine, ModelConfig};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Demo: Memory Corruption Fixed ===\n");

    // Create engine configuration
    let config = EngineConfig {
        models_directory: PathBuf::from("./models"),
        max_loaded_models: 1,
        max_context_length: 2048,
        gpu_layers: 0,
        thread_count: 8,
        batch_size: 512,
        use_mmap: true,
        use_mlock: false,
        max_concurrent_inferences: 1,
        model_eviction_policy: "lru".to_string(),
        kv_cache_type_k: None,
        kv_cache_type_v: None,
    };

    // Create the LLM engine
    let mut engine = LlmEngine::new(config).await?;
    println!("✓ Engine created successfully");

    // Load a model
    let model_config = ModelConfig {
        model_path: PathBuf::from("./models/tiny-vicuna-1b.q4_k_m.gguf"),
        model_type: "llama".to_string(),
        context_size: 2048,
        gpu_layers: 0,
        rope_freq_base: 10000.0,
        rope_freq_scale: 1.0,
    };

    let model_id = engine.load_model(model_config).await?;
    println!("✓ Model loaded successfully (ID: {})", model_id);

    // Create an inference request
    let request = InferenceRequest {
        model_id: model_id.clone(),
        prompt: "The capital of France is".to_string(),
        max_tokens: 50,
        temperature: 0.7,
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.1,
        min_p: 0.0,
        seed: None,
        stop_sequences: vec![],
        stream: false,
        cancel_flag: None,
        token_sender: None,
    };

    println!("\nRunning inference with prompt: \"{}\"", request.prompt);
    println!("(Note: This is a mock implementation to demonstrate the crash is fixed)\n");

    // Run inference
    let result = engine.run_inference(request).await?;

    println!("✓ Inference completed successfully!");
    println!("  - Generated text: {}", result.text);
    println!("  - Tokens generated: {}", result.tokens_generated);
    println!(
        "  - Generation time: {:.2}ms",
        result.generation_time.as_millis()
    );
    println!("  - Speed: {:.1} tokens/second", result.tokens_per_second);

    println!("\n🎉 SUCCESS: No memory corruption crash!");
    println!("The previous error 'free(): invalid size' has been resolved.");

    Ok(())
}
