#!/bin/bash
# Quick deployment script that bypasses Docker build issues

echo "Deploying new binary to production nodes..."

# Copy binary directly into running containers
echo "Copying binary to llm-node-prod-1..."
docker cp target/release/fabstir-llm-node llm-node-prod-1:/usr/local/bin/fabstir-llm-node

echo "Copying binary to llm-node-prod-2..."
docker cp target/release/fabstir-llm-node llm-node-prod-2:/usr/local/bin/fabstir-llm-node

# Restart containers to use new binary
echo "Restarting containers..."
docker restart llm-node-prod-1 llm-node-prod-2

# Wait for startup
sleep 5

# Verify deployment
echo ""
echo "Checking deployment..."
docker logs llm-node-prod-1 2>&1 | grep -E "VERSION|Token tracking" | head -5

echo ""
echo "Deployment complete!"
echo "Test with: curl -X POST http://localhost:8080/v1/inference -H \"Content-Type: application/json\" -d '{\"model\": \"tiny-vicuna-1b\", \"prompt\": \"Test\", \"max_tokens\": 10, \"temperature\": 0.7, \"stream\": false, \"session_id\": \"test\"}'"