// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::inference::*;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing real LLM inference...");

    let mut engine = LlmEngine::new(EngineConfig::default()).await?;

    let model_id = engine
        .load_model(ModelConfig {
            model_path: PathBuf::from("models/tiny-vicuna-1b.q4_k_m.gguf"),
            model_type: "llama".to_string(),
            context_size: 2048,
            gpu_layers: 35, // Use GPU
            rope_freq_base: 10000.0,
            rope_freq_scale: 1.0,
        })
        .await?;

    println!("Model loaded successfully!");

    let result = engine
        .run_inference(InferenceRequest {
            model_id,
            prompt: "Explain how a quantum computer works in simple terms:".to_string(),
            max_tokens: 512,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            repeat_penalty: 1.5,
            seed: None,
            stop_sequences: vec![],
            stream: false,
        })
        .await?;

    println!("Generated text: {}", result.text);
    println!(
        "Tokens: {}, Speed: {:.1} tok/s",
        result.tokens_generated, result.tokens_per_second
    );

    Ok(())
}
