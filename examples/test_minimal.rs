// Minimal test of llama_cpp crate
use llama_cpp::{LlamaModel, LlamaParams, SessionParams};
use llama_cpp::standard_sampler::StandardSampler;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing llama_cpp v0.3.2...");
    
    let model_path = Path::new("models/tiny-vicuna-1b.q4_k_m.gguf");
    
    // Load the model
    println!("Loading model...");
    let model = LlamaModel::load_from_file(model_path, LlamaParams::default())?;
    println!("Model loaded successfully!");
    
    // Create a session
    let mut session = model.create_session(SessionParams::default())?;
    
    // Feed prompt
    let prompt = "The capital of France is";
    println!("Feeding prompt: {}", prompt);
    session.advance_context(prompt)?;
    
    // Create sampler
    let sampler = StandardSampler::default()
        .temperature(0.7)
        .top_k(40)
        .top_p(0.9);
    
    // Generate tokens
    println!("Generating tokens...");
    let mut output = String::new();
    let mut token_count = 0;
    let max_tokens = 20;
    
    let completions = session.start_completing_with(sampler, max_tokens)
        .into_strings();
    
    for completion in completions {
        print!("{}", completion);
        output.push_str(&completion);
        token_count += 1;
        
        if token_count >= max_tokens {
            break;
        }
    }
    
    println!("\n\nGenerated {} tokens", token_count);
    println!("Full output: {}{}", prompt, output);
    
    Ok(())
}