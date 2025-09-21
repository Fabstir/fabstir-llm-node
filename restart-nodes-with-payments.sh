#!/bin/bash

source .env.local.test

docker stop llm-node-prod-1 llm-node-prod-2 2>/dev/null || true
docker rm llm-node-prod-1 llm-node-prod-2 2>/dev/null || true

docker run -d \
  --name llm-node-prod-1 \
  -p 9001-9003:9001-9003 \
  -p 8080:8080 \
  -e P2P_PORT=9001 \
  -e API_PORT=8080 \
  -e MODEL_PATH=/models/tiny-vicuna-1b.q4_k_m.gguf \
  -e HOST_PRIVATE_KEY="${TEST_HOST_1_PRIVATE_KEY}" \
  -e RPC_URL="https://base-sepolia.g.alchemy.com/v2/1pZoccdtgU8CMyxXzE3l_ghnBBaJABMR" \
  -e RUST_LOG=info \
  --gpus all \
  --add-host host.docker.internal:host-gateway \
  llm-node-prod:latest

docker run -d \
  --name llm-node-prod-2 \
  -p 9011-9013:9011-9013 \
  -p 8083:8083 \
  -e P2P_PORT=9011 \
  -e API_PORT=8083 \
  -e MODEL_PATH=/models/tiny-vicuna-1b.q4_k_m.gguf \
  -e HOST_PRIVATE_KEY="${TEST_HOST_2_PRIVATE_KEY}" \
  -e RPC_URL="https://base-sepolia.g.alchemy.com/v2/1pZoccdtgU8CMyxXzE3l_ghnBBaJABMR" \
  -e RUST_LOG=info \
  --gpus all \
  --add-host host.docker.internal:host-gateway \
  llm-node-prod:latest

echo "Done"
