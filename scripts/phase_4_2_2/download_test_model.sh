#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1


# Download a small test model for llama.cpp integration testing
echo "Creating models directory..."
mkdir -p models

echo "Downloading TinyLlama 1.1B model (Q4_K_M quantized, ~700MB)..."
echo "This is a small, fast model perfect for testing."

if [ ! -f "models/tinyllama-1.1b.Q4_K_M.gguf" ]; then
    wget -O models/tinyllama-1.1b.Q4_K_M.gguf \
        https://huggingface.co/TheBloke/TinyLlama-1.1B-GGUF/resolve/main/tinyllama-1.1b.Q4_K_M.gguf
    
    if [ $? -eq 0 ]; then
        echo "✅ Model downloaded successfully!"
        echo "📍 Location: models/tinyllama-1.1b.Q4_K_M.gguf"
        echo "📊 Size: $(du -h models/tinyllama-1.1b.Q4_K_M.gguf | cut -f1)"
        echo ""
        echo "To use this model, update your example to point to:"
        echo "  PathBuf::from(\"models/tinyllama-1.1b.Q4_K_M.gguf\")"
    else
        echo "❌ Download failed. Please check your internet connection."
        exit 1
    fi
else
    echo "✅ Model already exists: models/tinyllama-1.1b.Q4_K_M.gguf"
fi

echo ""
echo "Next steps:"
echo "1. Follow LLAMA_CPP_INTEGRATION.md to integrate real llama.cpp"
echo "2. Update engine.rs to use llama-cpp-rs crate"
echo "3. Test with: cargo run --example test_inference"