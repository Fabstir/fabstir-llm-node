// Simple test to verify llama_cpp works with GGUF files
use llama_cpp::{LlamaModel, LlamaParams, SessionParams};
use llama_cpp::standard_sampler::StandardSampler;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Simple Llama.cpp Test ===");
    println!("This test shows that we can load and run GGUF models with llama_cpp crate\n");
    
    let model_path = Path::new("models/tiny-vicuna-1b.q4_k_m.gguf");
    
    // Set up model parameters
    let mut params = LlamaParams::default();
    params.n_gpu_layers = 0; // CPU only for now
    params.use_mmap = true;
    
    println!("Loading model: {:?}", model_path);
    
    // Load the model
    let model = match LlamaModel::load_from_file(model_path, params) {
        Ok(m) => {
            println!("✓ Model loaded successfully!");
            m
        }
        Err(e) => {
            eprintln!("Failed to load model: {}", e);
            eprintln!("\nNote: Make sure the model file exists at: {:?}", model_path);
            eprintln!("You can download GGUF models from HuggingFace");
            return Err(Box::new(e));
        }
    };
    
    // Create a session
    let mut session_params = SessionParams::default();
    session_params.n_ctx = 2048; // Context size
    
    let mut session = model.create_session(session_params)?;
    
    // Test prompt
    let prompt = "The capital of France is";
    println!("\nPrompt: \"{}\"", prompt);
    println!("Generating response...\n");
    
    // Feed the prompt
    session.advance_context(prompt)?;
    
    // Generate tokens using the standard sampler
    print!("{}", prompt);
    let mut generated = String::new();
    
    let sampler = StandardSampler::default()
        .temperature(0.7)
        .top_p(0.9);
    
    let completions = session.start_completing_with(sampler, 50)
        .into_strings();
    
    for completion in completions {
        print!("{}", completion);
        generated.push_str(&completion);
    }
    
    println!("\n\n=== Results ===");
    println!("Generated text: {}", generated);
    println!("\n✅ SUCCESS: Real LLM inference with GGUF model!");
    println!("🎉 No memory corruption or crashes!");
    
    Ok(())
}