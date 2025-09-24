#!/bin/bash
# Complete restart and deployment script

source .env.local.test

# Stop and remove old containers
docker stop llm-node-prod-1 llm-node-prod-2 2>/dev/null || true
docker rm llm-node-prod-1 llm-node-prod-2 2>/dev/null || true

# Start node 1 with all proper settings
docker run -d \
  --name llm-node-prod-1 \
  -p 9001-9003:9001-9003 \
  -p 8080:8080 \
  -e P2P_PORT=9001 \
  -e API_PORT=8080 \
  -e MODEL_PATH=/models/tiny-vicuna-1b.q4_k_m.gguf \
  -e HOST_PRIVATE_KEY="${TEST_HOST_1_PRIVATE_KEY}" \
  -e RPC_URL="https://base-sepolia.g.alchemy.com/v2/1pZoccdtgU8CMyxXzE3l_ghnBBaJABMR" \
  -e CONTRACT_JOB_MARKETPLACE="${CONTRACT_JOB_MARKETPLACE}" \
  -e CONTRACT_HOST_EARNINGS="${CONTRACT_HOST_EARNINGS}" \
  -e CONTRACT_NODE_REGISTRY="${CONTRACT_NODE_REGISTRY}" \
  -e TREASURY_FEE_PERCENTAGE="${TREASURY_FEE_PERCENTAGE}" \
  -e HOST_EARNINGS_PERCENTAGE="${HOST_EARNINGS_PERCENTAGE}" \
  -e RUST_LOG=info \
  --gpus all \
  --add-host host.docker.internal:host-gateway \
  llm-node-prod:latest

# Start node 2 with all proper settings
docker run -d \
  --name llm-node-prod-2 \
  -p 9011-9013:9011-9013 \
  -p 8083:8083 \
  -e P2P_PORT=9011 \
  -e API_PORT=8083 \
  -e MODEL_PATH=/models/tiny-vicuna-1b.q4_k_m.gguf \
  -e HOST_PRIVATE_KEY="${TEST_HOST_2_PRIVATE_KEY}" \
  -e RPC_URL="https://base-sepolia.g.alchemy.com/v2/1pZoccdtgU8CMyxXzE3l_ghnBBaJABMR" \
  -e CONTRACT_JOB_MARKETPLACE="${CONTRACT_JOB_MARKETPLACE}" \
  -e CONTRACT_HOST_EARNINGS="${CONTRACT_HOST_EARNINGS}" \
  -e CONTRACT_NODE_REGISTRY="${CONTRACT_NODE_REGISTRY}" \
  -e TREASURY_FEE_PERCENTAGE="${TREASURY_FEE_PERCENTAGE}" \
  -e HOST_EARNINGS_PERCENTAGE="${HOST_EARNINGS_PERCENTAGE}" \
  -e RUST_LOG=info \
  --gpus all \
  --add-host host.docker.internal:host-gateway \
  llm-node-prod:latest

# Wait for containers to start
sleep 3

# Copy the correct binary into both containers
echo "Deploying latest binary..."
docker cp target/release/fabstir-llm-node llm-node-prod-1:/usr/local/bin/fabstir-llm-node
docker cp target/release/fabstir-llm-node llm-node-prod-2:/usr/local/bin/fabstir-llm-node

# Restart to use new binary
docker restart llm-node-prod-1 llm-node-prod-2

echo "Done - nodes started with correct ports and latest binary"

# Verify
sleep 5
echo ""
echo "Verification:"
docker logs llm-node-prod-1 2>&1 | grep -E "VERSION" | head -2