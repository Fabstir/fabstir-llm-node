# Inference Module

This module provides LLM inference capabilities for the Fabstir LLM Node.

## Components

### LLM Engine (`engine.rs`)
- Manages model loading and inference execution
- Supports concurrent inference requests
- Provides streaming token generation
- Mock implementation for testing (actual llama.cpp integration pending)

### Model Management (`models.rs`)
- Model registry for discovering available models
- Model downloader with progress tracking
- Support for HuggingFace and direct downloads
- Model lifecycle management with cleanup policies

### Inference Cache (`cache.rs`)
- LRU cache for inference results
- Memory-based eviction policies
- Semantic similarity search (placeholder)
- Cache persistence and warming

### Output Formatting (`format.rs`)
- Multiple output formats: Text, JSON, Markdown, HTML, XML
- Streaming JSON for real-time responses
- PII detection and redaction
- Safety checks and content filtering

## Usage

```rust
use fabstir_llm_node::inference::{LlmEngine, EngineConfig, ModelConfig, InferenceRequest};

// Create engine
let config = EngineConfig::default();
let mut engine = LlmEngine::new(config).await?;

// Load model
let model_config = ModelConfig {
    model_path: PathBuf::from("./models/llama-7b.gguf"),
    model_type: "llama-7b".to_string(),
    context_size: 2048,
    gpu_layers: 35,
    rope_freq_base: 10000.0,
    rope_freq_scale: 1.0,
};
let model_id = engine.load_model(model_config).await?;

// Run inference
let request = InferenceRequest {
    model_id,
    prompt: "What is the meaning of life?".to_string(),
    max_tokens: 100,
    temperature: 0.7,
    top_p: 0.9,
    top_k: 40,
    repeat_penalty: 1.1,
    seed: None,
    stop_sequences: vec![],
    stream: false,
};

let result = engine.run_inference(request).await?;
println!("Response: {}", result.text);
```

## Implementation Status

- ✅ Basic structure and types
- ✅ Mock implementation for testing
- ✅ All core APIs defined
- ⚠️ Tests expect additional features not yet implemented
- ❌ Actual llama.cpp integration pending
- ❌ GPU acceleration not implemented
- ❌ Semantic cache not implemented

## Testing

The module includes comprehensive test coverage in `tests/inference/`:
- `test_engine.rs` - Engine lifecycle and inference
- `test_models.rs` - Model management
- `test_cache.rs` - Caching functionality
- `test_format.rs` - Output formatting

Note: Tests expect more features than currently implemented. The basic functionality works but advanced features are mocked.