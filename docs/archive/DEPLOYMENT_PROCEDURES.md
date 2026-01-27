# Production Deployment Procedures
## September 22, 2025

## Quick Deployment (Most Common)

### Option 1: Binary Update Only (Fastest)
```bash
# 1. Build in dev container
cargo build --release

# 2. Run deployment script
./deploy-binary.sh
```

### Option 2: Full Restart with Binary
```bash
# 1. Build in dev container
cargo build --release

# 2. Run restart and deploy
./restart-and-deploy.sh
```

## Detailed Deployment Process

### Step 1: Build Binary
```bash
# In dev container
cargo build --release

# Verify build
ls -la target/release/fabstir-llm-node
# Should be ~841MB, not 116B
```

### Step 2: Deploy to Production Containers

#### Method A: Using Deploy Script (Recommended)
```bash
./deploy-binary.sh
```

#### Method B: Manual Steps
```bash
# Copy binary to production containers
docker cp target/release/fabstir-llm-node llm-node-prod-1:/usr/local/bin/fabstir-llm-node
docker cp target/release/fabstir-llm-node llm-node-prod-2:/usr/local/bin/fabstir-llm-node

# Restart containers
docker restart llm-node-prod-1 llm-node-prod-2
```

### Step 3: Verify Deployment
```bash
# Check version string
docker logs llm-node-prod-1 2>&1 | grep VERSION

# Check binary size
docker exec llm-node-prod-1 ls -la /usr/local/bin/fabstir-llm-node
# Should be ~841MB

# Test inference
curl -X POST http://localhost:8080/v1/inference \
  -H "Content-Type: application/json" \
  -d '{"model": "tiny-vicuna-1b", "prompt": "Test", "max_tokens": 10}'
```

## Full Container Restart Procedure

### When Needed
- After major configuration changes
- When containers are corrupted
- For clean slate deployment

### Steps
```bash
# 1. Stop and remove old containers
docker stop llm-node-prod-1 llm-node-prod-2
docker rm llm-node-prod-1 llm-node-prod-2

# 2. Start fresh containers
./restart-nodes-with-payments.sh

# 3. Wait for startup
sleep 5

# 4. Deploy binary
docker cp target/release/fabstir-llm-node llm-node-prod-1:/usr/local/bin/fabstir-llm-node
docker cp target/release/fabstir-llm-node llm-node-prod-2:/usr/local/bin/fabstir-llm-node

# 5. Restart to use new binary
docker restart llm-node-prod-1 llm-node-prod-2
```

## Deployment Scripts

### `/workspace/deploy-binary.sh`
- Quick binary update without container restart
- Copies binary and restarts containers
- Shows verification logs

### `/workspace/restart-and-deploy.sh`
- Full container recreation
- Deploys latest binary
- Includes port configuration

### `/workspace/restart-nodes-with-payments.sh`
- Starts production nodes with payment support
- Configures all environment variables
- Sets up GPU access

### `/workspace/restart-nodes-with-local-binary.sh`
- Mounts local binary as volume
- Good for development testing
- Avoids repeated docker cp

## Environment Configuration

### Required Environment Variables
```bash
# In .env.local.test
TEST_HOST_1_PRIVATE_KEY="0x..."
TEST_HOST_2_PRIVATE_KEY="0x..."

# RPC URL (Base Sepolia)
RPC_URL="https://base-sepolia.g.alchemy.com/v2/1pZoccdtgU8CMyxXzE3l_ghnBBaJABMR"
```

### Port Configuration
- Node 1: API 8080, P2P 9001-9003
- Node 2: API 8083, P2P 9011-9013

## Docker Build Issues (Historical)

### Problem
Docker build context couldn't access `target/` directory, resulting in 116B binary instead of 841MB.

### Solutions Tried
1. ❌ Docker build with COPY - Failed due to context
2. ❌ Multi-stage build - Still context issues
3. ✅ Docker cp after container start - Works reliably
4. ✅ Volume mount - Works for development

### Current Best Practice
Use `docker cp` to copy binary directly into running containers. This bypasses all Docker build context issues.

## Common Issues and Solutions

### Issue: Script Execution Errors
```bash
# Fix line endings
sed -i 's/\r$//' script.sh
chmod +x script.sh
```

### Issue: Binary Not Updating
```bash
# Verify binary timestamp
docker exec llm-node-prod-1 stat /usr/local/bin/fabstir-llm-node

# Force restart
docker stop llm-node-prod-1
docker start llm-node-prod-1
```

### Issue: Container Won't Start
```bash
# Check logs
docker logs llm-node-prod-1

# Check if ports are in use
netstat -tulpn | grep 8080
netstat -tulpn | grep 9001
```

## Version Tracking

Add version strings to code for verification:
```rust
eprintln!("🚀 API SERVER VERSION: v4-no-cleanup-on-streaming-2024-09-22-03:49");
```

Version history:
- v1: Initial broken version
- v2: Added JobMarketplace address
- v3: Added token tracking
- v4: Fixed cleanup bug

## Testing After Deployment

### Basic Health Check
```bash
curl http://localhost:8080/health
```

### Token Tracking Test
```bash
# Generate tokens
for i in {1..10}; do
  curl -X POST http://localhost:8080/v1/inference \
    -H "Content-Type: application/json" \
    -d "{\"model\": \"tiny-vicuna-1b\", \"prompt\": \"Test $i\", \"max_tokens\": 15, \"session_id\": \"TEST_SESSION\"}"
done

# Check logs for accumulation
docker logs llm-node-prod-1 2>&1 | grep "TEST_SESSION"
```

### WebSocket Test
Use the UI application to:
1. Start new session
2. Send multiple prompts
3. Verify tokens accumulate (not reset)
4. Check for checkpoint at 100+ tokens

## Production Monitoring

### Log Monitoring
```bash
# Watch for token tracking
docker logs -f llm-node-prod-1 2>&1 | grep "📊"

# Watch for checkpoint submissions
docker logs -f llm-node-prod-1 2>&1 | grep "checkpoint"

# Watch for errors
docker logs -f llm-node-prod-1 2>&1 | grep -i error
```

### Blockchain Monitoring
Monitor Base Sepolia for ProofOfWork events:
- Contract: 0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944
- Event: ProofOfWork(uint256 indexed jobId, address indexed host, uint256 tokensClaimed)