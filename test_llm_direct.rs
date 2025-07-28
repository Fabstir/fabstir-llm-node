// Direct test of llm crate without full project dependencies
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing llm crate directly...");
    
    let model_path = PathBuf::from("models/tiny-vicuna-1b.q4_k_m.gguf");
    
    // Load the model
    println!("Loading model...");
    let model = llm::load::<llm::models::Llama>(
        &model_path,
        llm::ModelParameters {
            context_size: 2048,
            lora_adapters: None,
            use_gpu: false,
            gpu_layers: None,
            rope_overrides: None,
            n_gqa: None,
        },
        |progress| match progress {
            llm::LoadProgress::TensorLoaded { current_tensor, tensor_count } => {
                if current_tensor % 50 == 0 || current_tensor == tensor_count {
                    println!("Loading progress: {}/{}", current_tensor, tensor_count);
                }
            }
            _ => {}
        },
    )?;
    
    println!("Model loaded successfully!");
    
    // Create a session
    let mut session = model.start_session(Default::default());
    
    // Set up inference parameters
    let inference_params = llm::InferenceParameters {
        n_threads: 8,
        n_batch: 512,
        top_k: 40,
        top_p: 0.9,
        repeat_penalty: 1.1,
        temperature: 0.7,
        bias_tokens: llm::TokenBias::default(),
        repetition_penalty_last_n: 64,
    };
    
    let prompt = "The capital of France is";
    let mut output = String::new();
    let mut token_count = 0;
    
    println!("Running inference with prompt: {}", prompt);
    
    // Run inference
    let result = session.infer::<std::convert::Infallible>(
        &model,
        &mut rand::thread_rng(),
        &llm::InferenceRequest {
            prompt: prompt,
            parameters: Some(&inference_params),
            play_back_previous_tokens: false,
            maximum_token_count: Some(20),
        },
        &mut Default::default(),
        |token: &str| {
            print!("{}", token);
            output.push_str(token);
            token_count += 1;
            Ok(())
        },
    )?;
    
    println!("\n\nGenerated {} tokens", token_count);
    println!("Full output: {}", output);
    
    Ok(())
}