#!/bin/bash

# Colors for output
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}🔄 Restarting production nodes with new code...${NC}"

# Stop and remove existing containers if they exist
echo -e "${YELLOW}📦 Stopping existing nodes...${NC}"
docker stop llm-node-prod-1 llm-node-prod-2 2>/dev/null || true
docker rm llm-node-prod-1 llm-node-prod-2 2>/dev/null || true

# Note: Build should be done separately with: docker build --no-cache -t llm-node-prod:latest -f Dockerfile.production .

# Start Node 1 (WebSocket on port 8080)
echo -e "${YELLOW}🚀 Starting LLM Node 1 (ws://localhost:8080)...${NC}"
docker run -d \
  --name llm-node-prod-1 \
  -p 9001:9001 \
  -p 9002:9002 \
  -p 9003:9003 \
  -p 8080:8080 \
  -e P2P_PORT=9001 \
  -e API_PORT=8080 \
  -e NODE_ID=node-1 \
  -e MODEL_PATH=/models/tiny-vicuna-1b.q4_k_m.gguf \
  -e MODEL_REGISTRY_ADDRESS=0x92b2De840bB2171203011A6dBA928d855cA8183E \
  -e NODE_REGISTRY_WITH_MODELS_ADDRESS=0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218 \
  -e USE_NEW_REGISTRY=true \
  -e ENABLE_PROOF_GENERATION=true \
  -e PROOF_TYPE=EZKL \
  -e PROOF_MODEL_PATH=/models/tiny-vicuna-1b.q4_k_m.gguf \
  -e PROOF_CACHE_SIZE=100 \
  -e PROOF_BATCH_SIZE=10 \
  -e VECTOR_DB_URL=http://host.docker.internal:7533 \
  -e S5_URL=http://host.docker.internal:5522 \
  -e RUST_LOG=info \
  --gpus all \
  --add-host host.docker.internal:host-gateway \
  --restart unless-stopped \
  llm-node-prod:latest

# Start Node 2 (WebSocket on port 8083)
echo -e "${YELLOW}🚀 Starting LLM Node 2 (ws://localhost:8083)...${NC}"
docker run -d \
  --name llm-node-prod-2 \
  -p 9011:9011 \
  -p 9012:9012 \
  -p 9013:9013 \
  -p 8083:8083 \
  -e P2P_PORT=9011 \
  -e API_PORT=8083 \
  -e NODE_ID=node-2 \
  -e MODEL_PATH=/models/tiny-vicuna-1b.q4_k_m.gguf \
  -e MODEL_REGISTRY_ADDRESS=0x92b2De840bB2171203011A6dBA928d855cA8183E \
  -e NODE_REGISTRY_WITH_MODELS_ADDRESS=0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218 \
  -e USE_NEW_REGISTRY=true \
  -e ENABLE_PROOF_GENERATION=true \
  -e PROOF_TYPE=EZKL \
  -e PROOF_MODEL_PATH=/models/tiny-vicuna-1b.q4_k_m.gguf \
  -e PROOF_CACHE_SIZE=100 \
  -e PROOF_BATCH_SIZE=10 \
  -e VECTOR_DB_URL=http://host.docker.internal:7533 \
  -e S5_URL=http://host.docker.internal:5522 \
  -e RUST_LOG=info \
  --gpus all \
  --add-host host.docker.internal:host-gateway \
  --restart unless-stopped \
  llm-node-prod:latest

# Check if containers started successfully
echo -e "${YELLOW}📋 Checking container status...${NC}"
docker ps | grep llm-node-prod

echo -e "${YELLOW}✅ Production nodes restarted!${NC}"