#!/bin/bash

# Test script for context-aware API endpoint

echo "Testing API with conversation context..."
echo ""
echo "Example 1: Simple request without context"
echo "=========================================="

curl -X POST http://localhost:8080/v1/inference \
  -H "Content-Type: application/json" \
  -d '{
    "model": "tiny-vicuna",
    "prompt": "What is the capital of France?",
    "max_tokens": 100,
    "temperature": 0.7
  }'

echo ""
echo ""
echo "Example 2: Request with conversation context"
echo "============================================"

curl -X POST http://localhost:8080/v1/inference \
  -H "Content-Type: application/json" \
  -d '{
    "model": "tiny-vicuna",
    "prompt": "What about Germany?",
    "max_tokens": 100,
    "temperature": 0.7,
    "conversation_context": [
      {
        "role": "user",
        "content": "What is the capital of France?"
      },
      {
        "role": "assistant",
        "content": "The capital of France is Paris."
      }
    ]
  }'

echo ""
echo ""
echo "Example 3: Multi-turn conversation context"
echo "=========================================="

curl -X POST http://localhost:8080/v1/inference \
  -H "Content-Type: application/json" \
  -d '{
    "model": "tiny-vicuna",
    "prompt": "And what about Italy?",
    "max_tokens": 100,
    "temperature": 0.7,
    "conversation_context": [
      {
        "role": "user",
        "content": "What is the capital of France?"
      },
      {
        "role": "assistant",
        "content": "The capital of France is Paris."
      },
      {
        "role": "user",
        "content": "What about Germany?"
      },
      {
        "role": "assistant",
        "content": "The capital of Germany is Berlin."
      }
    ]
  }'

echo ""
echo "Done!"