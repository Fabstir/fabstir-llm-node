#!/bin/bash

# Script to start two LLM nodes with proof system enabled
# WebSocket endpoints: ws://localhost:8080 and ws://localhost:8081

set -e

echo "🚀 Starting two LLM nodes with proof system enabled..."

# Build the production Docker image if needed
if [[ "$(docker images -q llm-node-prod:latest 2> /dev/null)" == "" ]]; then
    echo "📦 Building production Docker image..."
    docker build -f Dockerfile.production -t llm-node-prod:latest .
else
    echo "✅ Production image already exists"
fi

# Create models directory if it doesn't exist
if [ ! -d "./models" ]; then
    echo "📁 Creating models directory..."
    mkdir -p ./models
fi

# Download model if not present
if [ ! -f "./models/tiny-vicuna-1b.q4_k_m.gguf" ]; then
    echo "📥 Downloading Tiny Vicuna model..."
    cd models
    curl -LO https://huggingface.co/afrideva/Tiny-Vicuna-1B-GGUF/resolve/main/tiny-vicuna-1b.q4_k_m.gguf
    cd ..
else
    echo "✅ Model already downloaded"
fi

# Stop any existing containers
echo "🛑 Stopping any existing containers..."
docker-compose -f docker-compose.two-nodes.yml down 2>/dev/null || true

# Start the services
echo "🎯 Starting services..."
docker-compose -f docker-compose.two-nodes.yml up -d

# Wait for services to be ready
echo "⏳ Waiting for services to be ready..."
sleep 5

# Check if nodes are running
echo "🔍 Checking node status..."
docker ps | grep llm-node

echo ""
echo "✨ Two LLM nodes are now running with proof system enabled!"
echo ""
echo "📡 WebSocket endpoints:"
echo "   - Node 1: ws://localhost:8080"
echo "   - Node 2: ws://localhost:8081"
echo ""
echo "🔧 P2P ports:"
echo "   - Node 1: 9001-9003"
echo "   - Node 2: 9011-9013"
echo ""
echo "🔐 Proof System Configuration:"
echo "   - Type: EZKL"
echo "   - Cache Size: 100"
echo "   - Batch Size: 10"
echo ""
echo "📊 View logs:"
echo "   docker logs llm-node-1"
echo "   docker logs llm-node-2"
echo ""
echo "🛑 To stop:"
echo "   docker-compose -f docker-compose.two-nodes.yml down"