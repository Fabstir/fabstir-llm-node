# Llama.cpp Integration Guide

## Current State

The inference module is currently using a **mock implementation** that simulates LLM inference. This allows the system to compile and run while we work on integrating real llama.cpp functionality.

### What's Working:
- ✅ Full API surface implemented
- ✅ Mock model that responds to prompts
- ✅ Example runs successfully
- ✅ Library compiles without errors
- ✅ Tests have the interfaces they need (though they don't compile due to other issues)

### What's Mock:
- Model loading (accepts any path)
- Inference (returns predefined responses)
- Token generation (simulated)
- Model management (no real files)

## Running the Example

```bash
cargo run --example test_inference
```

This will show mock inference working with the prompt "The capital of France is" and return "Paris. It is the largest city in France."

## Real Llama.cpp Integration Options

### Option 1: llama-cpp-rs Crate (Recommended)

The `llama-cpp-rs` crate provides Rust bindings for llama.cpp:

```toml
[dependencies]
llama-cpp-rs = "0.3"
```

Example integration:
```rust
use llama_cpp_rs::{Model, InferenceParameters};

// Load model
let model = Model::load(&model_path, &ModelParameters::default())?;

// Run inference
let response = model.infer(&prompt, &InferenceParameters {
    max_tokens: 100,
    temperature: 0.7,
    ..Default::default()
})?;
```

### Option 2: Direct FFI Bindings

Create direct FFI bindings to llama.cpp:

```rust
#[link(name = "llama")]
extern "C" {
    fn llama_load_model_from_file(path: *const c_char) -> *mut LlamaModel;
    fn llama_eval(model: *mut LlamaModel, tokens: *const i32, n_tokens: i32) -> i32;
    // ... more functions
}
```

### Option 3: Updated llm Crate

Wait for or contribute to updates for the `llm` crate to support the latest API.

## Integration Steps

1. **Choose Integration Method**: Recommend starting with llama-cpp-rs for simplicity

2. **Update Cargo.toml**:
   ```toml
   [dependencies]
   llama-cpp-rs = "0.3"
   # Remove or comment out: llm = "1.3"
   ```

3. **Update engine.rs**:
   - Replace `MockLlmModel` trait with real model wrapper
   - Update `load_model()` to load actual GGUF files
   - Update `run_inference()` to use real inference

4. **Download Test Model**:
   ```bash
   mkdir -p models
   # Download a small model for testing
   wget https://huggingface.co/TheBloke/TinyLlama-1.1B-GGUF/resolve/main/tinyllama-1.1b.Q4_K_M.gguf \
        -O models/tinyllama-1.1b.Q4_K_M.gguf
   ```

5. **Update Example**:
   - Point to real model file
   - Test with various prompts

## Mock Implementation Details

The current mock (`SimpleMockModel`) provides intelligent responses for testing:
- "capital of france" → "Paris. It is the largest city in France."
- "hello" → "Hello! How can I help you today?"
- Default → "This is a mock response from the model."

## Testing Real Integration

Once integrated, test with:

```rust
// Load a real model
let model_id = engine.load_model(ModelConfig {
    model_path: PathBuf::from("models/tinyllama-1.1b.Q4_K_M.gguf"),
    model_type: "llama".to_string(),
    context_size: 2048,
    gpu_layers: 0, // Start with CPU
    ..Default::default()
}).await?;

// Test inference
let result = engine.run_inference(InferenceRequest {
    model_id,
    prompt: "Write a haiku about programming:".to_string(),
    max_tokens: 50,
    temperature: 0.8,
    ..Default::default()
}).await?;

println!("Generated: {}", result.text);
```

## Performance Considerations

1. **GPU Support**: Set `gpu_layers > 0` to offload layers to GPU
2. **Context Size**: Larger contexts use more memory
3. **Batch Size**: Tune for throughput vs latency
4. **Model Quantization**: Use Q4_K_M or Q5_K_M for good balance

## Next Steps

1. Choose integration method (recommend llama-cpp-rs)
2. Update dependencies and code
3. Test with real models
4. Add streaming support
5. Implement proper error handling
6. Add model format validation
7. Integrate with the rest of the system