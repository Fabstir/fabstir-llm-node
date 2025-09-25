use fabstir_llm_node::inference::*;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = LlmEngine::new(EngineConfig::default()).await?;

    let model_id = engine
        .load_model(ModelConfig {
            model_path: PathBuf::from("models/tiny-vicuna-1b.q4_k_m.gguf"),
            model_type: "llama".to_string(),
            context_size: 2048,
            gpu_layers: 0,
            rope_freq_base: 10000.0,
            rope_freq_scale: 1.0,
        })
        .await?;

    for prompt in &["What is 2+2?", "The sky is", "Hello, how are"] {
        let result = engine
            .run_inference(InferenceRequest {
                model_id: model_id.clone(),
                prompt: prompt.to_string(),
                max_tokens: 20,
                temperature: 0.7,
                top_p: 0.9,
                top_k: 40,
                repeat_penalty: 1.1,
                seed: None,
                stop_sequences: vec![],
                stream: false,
            })
            .await?;

        println!("Prompt: {}", prompt);
        println!("Response: {}\n", result.text);
    }

    Ok(())
}
