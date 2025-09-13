# Production Node Update Guide

## Complete Process for Updating Production Nodes with New Contract Addresses and Code Changes

### Prerequisites
- New contract addresses from contracts developer
- Code changes completed in fabstir-llm-node
- Access to production server
- Docker and docker-compose installed

### Step 1: Update Contract Addresses

1. Add new contract addresses to `.env.contracts`:
```bash
# Add to .env.contracts
MODEL_REGISTRY_ADDRESS=0x92b2De840bB2171203011A6dBA928d855cA8183E
NODE_REGISTRY_WITH_MODELS_ADDRESS=0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218
USE_NEW_REGISTRY=true
```

2. Copy new contract ABIs to contracts directory:
```bash
cp docs/compute-contracts-reference/client-abis/ModelRegistry-CLIENT-ABI.json contracts/
cp docs/compute-contracts-reference/client-abis/NodeRegistryWithModels-CLIENT-ABI.json contracts/
```

### Step 2: Update Code to Remove Mock Fallbacks

If not already done, ensure all mock implementations are removed from:
- `src/contracts/model_registry.rs` - Remove mock model validation
- `src/host/registration.rs` - Remove mock registration calls

### Step 3: Build New Binary in Dev Container

1. Enter the dev container:
```bash
docker exec -it fabstir-llm-marketplace-node-dev-1 bash
# Or if container name is different:
docker exec -it c5c8c22dc775 bash  # Use actual container ID
```

2. Build the release binary inside container:
```bash
cd /workspace
cargo clean
cargo build --release
# This will take 2-3 minutes
exit
```

### Step 4: Copy Binary to Host

1. Find your dev container ID:
```bash
docker ps | grep dev
# Note the container ID (e.g., c5c8c22dc775)
```

2. Copy the compiled binary to host:
```bash
# Create target directory if needed
mkdir -p target/release

# Copy binary from container to host
docker cp c5c8c22dc775:/workspace/target/release/fabstir-llm-node ./target/release/fabstir-llm-node

# Verify the copy (should be ~797MB with current timestamp)
ls -lah target/release/fabstir-llm-node
```

### Step 5: Rebuild Docker Production Image

```bash
# Rebuild the production image with new binary (use --no-cache to force rebuild)
docker build --no-cache -t llm-node-prod:latest -f Dockerfile.production .

# Verify image was rebuilt (should show "About a minute ago")
docker images | grep llm-node-prod
```

### Step 6: Create/Update Restart Script

Create `restart-production-nodes.sh`:

```bash
#!/bin/bash

# Colors for output
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}🔄 Restarting production nodes with new code...${NC}"

# Stop and remove existing containers if they exist
echo -e "${YELLOW}📦 Stopping existing nodes...${NC}"
docker stop llm-node-prod-1 llm-node-prod-2 2>/dev/null || true
docker rm llm-node-prod-1 llm-node-prod-2 2>/dev/null || true

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
  -e MODEL_REGISTRY_ADDRESS=0xfE54c2aa68A7Afe8E0DD571933B556C8b6adC357 \
  -e NODE_REGISTRY_WITH_MODELS_ADDRESS=0xaa14Ed58c3EF9355501bc360E5F09Fb9EC8c1100 \
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
  -e MODEL_REGISTRY_ADDRESS=0xfE54c2aa68A7Afe8E0DD571933B556C8b6adC357 \
  -e NODE_REGISTRY_WITH_MODELS_ADDRESS=0xaa14Ed58c3EF9355501bc360E5F09Fb9EC8c1100 \
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
```

Make it executable:
```bash
chmod +x restart-production-nodes.sh
```

### Step 7: Stop Old Containers and Start New Ones

```bash
# Stop any existing containers
docker stop llm-node-prod-1 llm-node-prod-2 2>/dev/null || true
docker rm llm-node-prod-1 llm-node-prod-2 2>/dev/null || true

# Run the restart script
./restart-production-nodes.sh
```

### Step 8: Verify New Code is Running

1. **CRITICAL: Verify environment variables are set** (this confirms the script worked):
```bash
docker exec llm-node-prod-1 env | grep -E "MODEL_REGISTRY|NODE_REGISTRY|USE_NEW"
# Should show:
# MODEL_REGISTRY_ADDRESS=0x92b2De840bB2171203011A6dBA928d855cA8183E
# NODE_REGISTRY_WITH_MODELS_ADDRESS=0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218
# USE_NEW_REGISTRY=true
```

2. Check binary size in containers (should match your compiled binary ~797MB):
```bash
docker exec llm-node-prod-1 ls -lah /usr/local/bin/fabstir-llm-node
docker exec llm-node-prod-2 ls -lah /usr/local/bin/fabstir-llm-node
```

3. Check logs for successful startup:
```bash
docker logs llm-node-prod-1 2>&1 | head -30
docker logs llm-node-prod-2 2>&1 | head -30
```

4. Look for contract connection attempts (if contracts aren't deployed, you'll see errors - this is expected):
```bash
docker logs llm-node-prod-1 2>&1 | grep -i "contract\|registry\|model"
```

5. Test API endpoints:
```bash
# Health check
curl http://localhost:8080/health
curl http://localhost:8083/health

# Models endpoint
curl http://localhost:8080/v1/models
curl http://localhost:8083/v1/models
```

### Troubleshooting

#### Issue: Script error "cannot execute: required file not found"
**Solution**: This is usually due to Windows line endings. Fix with:
```bash
sed -i 's/\r$//' restart-production-nodes.sh
# Or run with bash directly:
bash restart-production-nodes.sh
```

#### Issue: Environment variables not set in containers
**Solution**: Ensure your restart script includes the new contract environment variables:
- MODEL_REGISTRY_ADDRESS
- NODE_REGISTRY_WITH_MODELS_ADDRESS
- USE_NEW_REGISTRY
Check with: `docker exec llm-node-prod-1 env | grep -E "MODEL_REGISTRY|NODE_REGISTRY|USE_NEW"`

#### Issue: Docker uses cached layers
**Solution**: Always use `--no-cache` flag when building:
```bash
docker build --no-cache -t llm-node-prod:latest -f Dockerfile.production .
```

#### Issue: Old binary in image
**Solution**: Ensure you copy the fresh binary from dev container before rebuilding:
```bash
docker cp c5c8c22dc775:/workspace/target/release/fabstir-llm-node ./target/release/fabstir-llm-node
```

#### Issue: Container doesn't start
**Solution**: Check logs for errors:
```bash
docker logs llm-node-prod-1
docker logs llm-node-prod-2
```

#### Issue: Can't find dev container
**Solution**: List all containers to find the correct one:
```bash
docker ps -a | grep -E "dev|workspace"
```

### Important Notes

1. **Binary Location**: The production Dockerfile expects the binary at `target/release/fabstir-llm-node` on the HOST machine
2. **Contract Addresses**: Must be passed as environment variables since mock fallbacks are removed
3. **Build Time**: Compiling in release mode takes 2-3 minutes
4. **Binary Size**: The compiled binary should be approximately 797MB
5. **No Mock Fallbacks**: With mocks removed, if contracts aren't deployed, you'll see connection errors - this is expected behavior

### Quick Command Summary

```bash
# Full rebuild and restart sequence
docker exec -it c5c8c22dc775 bash -c "cd /workspace && cargo clean && cargo build --release"
docker cp c5c8c22dc775:/workspace/target/release/fabstir-llm-node ./target/release/fabstir-llm-node
docker build --no-cache -t llm-node-prod:latest -f Dockerfile.production .
docker stop llm-node-prod-1 llm-node-prod-2 && docker rm llm-node-prod-1 llm-node-prod-2
./restart-production-nodes.sh
```