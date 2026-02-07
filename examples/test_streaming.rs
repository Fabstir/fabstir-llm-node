// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::inference::*;
use std::path::PathBuf;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing streaming inference...");

    let mut engine = LlmEngine::new(EngineConfig::default()).await?;

    let model_id = engine
        .load_model(ModelConfig {
            model_path: PathBuf::from("models/tinyllama-1.1b.Q4_K_M.gguf"),
            model_type: "llama".to_string(),
            context_size: 2048,
            gpu_layers: 0,
            rope_freq_base: 10000.0,
            rope_freq_scale: 1.0,
        })
        .await?;

    println!("Model loaded! Starting streaming inference...");

    let mut stream = engine
        .run_inference_stream(InferenceRequest {
            model_id,
            prompt: "Hello there!".to_string(),
            max_tokens: 20,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            repeat_penalty: 1.1,
            min_p: 0.0,
            seed: None,
            stop_sequences: vec![],
            stream: true,
        })
        .await?;

    print!("Streaming tokens: ");
    while let Some(token_result) = stream.next().await {
        match token_result {
            Ok(token_info) => {
                print!("{}", token_info.text);
                std::io::Write::flush(&mut std::io::stdout())?;
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }

    println!("\n✅ Streaming complete!");

    Ok(())
}
