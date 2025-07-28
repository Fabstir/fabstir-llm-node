// Direct test using llm crate from rustformers
use std::path::Path;
use std::sync::{Arc, Mutex};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Real LLM Test ===");
    println!("Loading model from rustformers/llm...\n");

    let model_path = Path::new("models/tiny-vicuna-1b.q4_k_m.gguf");
    
    // Model parameters
    let model_params = llm::ModelParameters {
        prefer_mmap: true,
        context_size: 2048,
        lora_adapters: None,
        use_gpu: false,
        gpu_layers: None,
        rope_overrides: None,
        n_gqa: None,
    };

    // Load the model
    println!("Loading model: {:?}", model_path);
    let model = llm::load_dynamic(
        Some(llm::ModelArchitecture::Llama),
        model_path,
        llm::TokenizerSource::Embedded,
        model_params,
        |progress| match progress {
            llm::LoadProgress::HyperparametersLoaded => {
                println!("✓ Hyperparameters loaded");
            }
            llm::LoadProgress::ContextSize { bytes } => {
                println!("✓ Context size: {} MB", bytes as f64 / 1024.0 / 1024.0);
            }
            llm::LoadProgress::LoraApplied { name, source } => {
                println!("✓ LoRA adapter applied: {} from {:?}", name, source);
            }
            llm::LoadProgress::TensorLoaded { current_tensor, tensor_count } => {
                if current_tensor % 50 == 0 || current_tensor == tensor_count {
                    println!("✓ Loading tensors: {}/{}", current_tensor, tensor_count);
                }
            }
            llm::LoadProgress::Loaded { file_size, tensor_count } => {
                println!("✓ Model loaded! Size: {:.2} MB, Tensors: {}", 
                    file_size as f64 / 1024.0 / 1024.0, tensor_count);
            }
        }
    )?;

    println!("\nModel loaded successfully!");
    
    // Create inference session
    let mut session = model.start_session(Default::default());
    
    // Test prompt
    let prompt = "The capital of France is";
    println!("\nPrompt: \"{}\"", prompt);
    println!("Generating response...\n");
    
    // Run inference
    let inference_params = llm::InferenceParameters::default();
    let mut output = String::new();
    
    let stats = session.infer::<std::convert::Infallible>(
        model.as_ref(),
        &mut rand::thread_rng(),
        &llm::InferenceRequest {
            prompt: llm::Prompt::Text(prompt),
            parameters: &inference_params,
            play_back_previous_tokens: false,
            maximum_token_count: Some(50),
        },
        &mut Default::default(),
        |response| {
            match response {
                llm::InferenceResponse::InferredToken(token) => {
                    print!("{}", token);
                    output.push_str(&token);
                    Ok(llm::InferenceFeedback::Continue)
                }
                _ => Ok(llm::InferenceFeedback::Continue),
            }
        }
    )?;
    
    println!("\n\n=== Results ===");
    println!("Generated text: {}", output);
    println!("Tokens generated: {}", stats.predict_tokens);
    println!("Generation time: {:.2}s", stats.predict_duration.as_secs_f32());
    println!("Speed: {:.1} tokens/second", 
        stats.predict_tokens as f32 / stats.predict_duration.as_secs_f32());
    
    println!("\n✅ Real LLM inference completed successfully!");
    println!("🎉 No memory corruption or crashes!");
    
    Ok(())
}