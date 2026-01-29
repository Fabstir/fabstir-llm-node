# GPT-OSS-120B Model Registration

## Model Details

- **Hugging Face Repository**: `ggml-org/gpt-oss-120b-GGUF`
- **Model Type**: Split GGUF (3 parts, auto-loaded by llama.cpp)
- **Quantization**: MXFP4 (optimal quality/size ratio)
- **Total Size**: ~65GB (3 × ~21GB files)
- **Context Length**: 131,072 tokens (131K)
- **Parameters**: 120 billion
- **Architecture**: GPT-based decoder-only transformer
- **GPU Requirement**: 97GB+ VRAM (tested on NVIDIA RTX Pro 6000 Blackwell)

## File Information

```
Part 1: gpt-oss-120b-mxfp4-00001-of-00003.gguf (~21GB)
Part 2: gpt-oss-120b-mxfp4-00002-of-00003.gguf (~21GB)
Part 3: gpt-oss-120b-mxfp4-00003-of-00003.gguf (~21GB)
```

**Important**: Only the first file needs to be specified in `MODEL_PATH`. llama.cpp automatically discovers and loads the remaining parts when they are co-located in the same directory.

## Integrity Verification

### Why Composite Hash?

This model uses a **composite hash** approach because:
- ✅ Verifies integrity of ALL 3 parts (not just first file)
- ✅ More secure - any tampering of any part is detectable
- ✅ Provides complete model authenticity for marketplace clients
- ✅ Standard approach for split-file models in production

The composite hash is calculated as:
```
composite_hash = SHA256(hash_part1 + hash_part2 + hash_part3)
```

### Individual File Hashes (SHA256)

*These will be filled in after running the download script:*

```
Part 1: <to be computed during download>
Part 2: <to be computed during download>
Part 3: <to be computed during download>
```

### Composite Hash (Registered in ModelRegistry)

```
Composite: <to be computed during download>
Calculation: sha256(hash1 + hash2 + hash3)
```

This composite hash is what gets registered in the ModelRegistry contract.

### Model ID (Contract)

```
Model ID: <to be computed>
Calculation: keccak256("ggml-org/gpt-oss-120b-GGUF/gpt-oss-120b-mxfp4-00001-of-00003.gguf")
```

## Download Instructions

### Prerequisites

```bash
# Option 1: Install huggingface-cli (recommended)
pip install -U "huggingface_hub[cli]"

# Option 2: Use wget (fallback)
# No installation needed, but slower and less reliable
```

### Download Command

Run the provided download script:

```bash
./scripts/download_gpt_oss_120b.sh
```

This script will:
1. Download all 3 parts (~65GB total)
2. Verify file sizes
3. Compute individual SHA256 hashes
4. Calculate composite hash for ModelRegistry
5. Save composite hash to `models/gpt-oss-120b-GGUF/gpt-oss-120b-composite-hash.txt`

**Expected download time**: 2-4 hours (depends on bandwidth)

## Verification Commands for Other Hosts

After downloading, other hosts can verify the model integrity:

```bash
# 1. Download model (if not already done)
cd /path/to/fabstir-llm-node
./scripts/download_gpt_oss_120b.sh

# 2. Verify individual hashes
cd models/gpt-oss-120b-GGUF
sha256sum gpt-oss-120b-mxfp4-*.gguf

# 3. Compute composite and verify
HASH1=$(sha256sum gpt-oss-120b-mxfp4-00001-of-00003.gguf | cut -d' ' -f1)
HASH2=$(sha256sum gpt-oss-120b-mxfp4-00002-of-00003.gguf | cut -d' ' -f1)
HASH3=$(sha256sum gpt-oss-120b-mxfp4-00003-of-00003.gguf | cut -d' ' -f1)
COMPUTED=$(echo -n "${HASH1}${HASH2}${HASH3}" | sha256sum | cut -d' ' -f1)

echo "Computed composite: $COMPUTED"
echo "Expected composite: $(cat gpt-oss-120b-composite-hash.txt)"
# These MUST match!

# 4. Verify model is approved on-chain
cd ../..
source .env.local.test
MODEL_ID=$(cast keccak "ggml-org/gpt-oss-120b-GGUF/gpt-oss-120b-mxfp4-00001-of-00003.gguf")
cast call $CONTRACT_MODEL_REGISTRY \
    "isModelApproved(bytes32)" \
    "$MODEL_ID" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL"

# Expected output: 0x0000000000000000000000000000000000000000000000000000000000000001 (true)
```

## ModelRegistry Registration

### For Contract Owner

Only the contract owner can register trusted models in the ModelRegistry.

```bash
# Run the registration script
./scripts/register_gpt_oss_120b.sh

# This script will:
# 1. Load the composite hash
# 2. Register the model with addTrustedModel()
# 3. Verify the registration succeeded
# 4. Display next steps
```

### Manual Registration (Alternative)

```bash
source .env.local.test

# Load composite hash
cd models/gpt-oss-120b-GGUF
COMPOSITE_HASH=$(cat gpt-oss-120b-composite-hash.txt)
BYTES32_HASH="0x${COMPOSITE_HASH}"

# Register model
cast send $CONTRACT_MODEL_REGISTRY \
    "addTrustedModel(string,string,bytes32)" \
    "ggml-org/gpt-oss-120b-GGUF" \
    "gpt-oss-120b-mxfp4-00001-of-00003.gguf" \
    "$BYTES32_HASH" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL" \
    --private-key "$OWNER_PRIVATE_KEY" \
    --legacy

# Verify
MODEL_ID=$(cast keccak "ggml-org/gpt-oss-120b-GGUF/gpt-oss-120b-mxfp4-00001-of-00003.gguf")
cast call $CONTRACT_MODEL_REGISTRY \
    "isModelApproved(bytes32)" \
    "$MODEL_ID" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL"
```

## Node Configuration

### Environment Variables

Add to `.env.prod`:

```bash
# Model path - point to FIRST part only
MODEL_PATH=/models/gpt-oss-120b-GGUF/gpt-oss-120b-mxfp4-00001-of-00003.gguf

# Context window - 131K tokens
MAX_CONTEXT_LENGTH=131072

# GPU layers - all layers to GPU
GPU_LAYERS=99

# Batch size - optimal for most scenarios
LLAMA_BATCH_SIZE=2048

# Chat template - Harmony format
MODEL_CHAT_TEMPLATE=harmony
```

### Docker Deployment

Use the updated deployment script:

```bash
./restart-and-deploy-openai.sh
```

Or manually with Docker:

```bash
docker run -d \
  --name llm-node-prod-1 \
  -p 9000-9003:9000-9003 \
  -p 8080:8080 \
  -v "$(pwd)/models:/models:ro" \
  -e MODEL_PATH=/models/gpt-oss-120b-GGUF/gpt-oss-120b-mxfp4-00001-of-00003.gguf \
  -e MAX_CONTEXT_LENGTH=131072 \
  -e GPU_LAYERS=99 \
  -e LLAMA_BATCH_SIZE=2048 \
  -e MODEL_CHAT_TEMPLATE=harmony \
  -e HOST_PRIVATE_KEY="$HOST_PRIVATE_KEY" \
  # ... other env vars
  --gpus all \
  llm-node-prod:latest
```

## Update Node Registration

After the model is registered in ModelRegistry, update your node to support it:

```bash
source .env.local.test

# Calculate model ID
MODEL_ID=$(cast keccak "ggml-org/gpt-oss-120b-GGUF/gpt-oss-120b-mxfp4-00001-of-00003.gguf")

# Update node's supported models
cast send $CONTRACT_NODE_REGISTRY \
    "updateSupportedModels(bytes32[])" \
    "[$MODEL_ID]" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL" \
    --private-key "$TEST_HOST_1_PRIVATE_KEY" \
    --legacy

# Verify update
cast call $CONTRACT_NODE_REGISTRY \
    "getNode(address)" \
    "$TEST_HOST_1_ADDRESS" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL"
```

## Performance Expectations

### VRAM Usage
- **Model**: ~65GB
- **Context (131K tokens)**: ~20-25GB
- **Total during inference**: ~85-90GB
- **GPU VRAM required**: 97GB+ (tested on RTX Pro 6000 Blackwell)

### Inference Performance
- **Model load time**: 30-60 seconds
- **First token latency**: 2-5 seconds
- **Throughput**: 5-15 tokens/second (depends on context size and batch)

### Context Handling
- **Maximum context**: 131,072 tokens (131K)
- **Recommended for most use cases**: 8K-32K tokens
- **Large context (>64K)**: May reduce throughput, monitor VRAM

## Testing

Run the comprehensive test suite:

```bash
./scripts/test_gpt_oss_120b.sh
```

This will verify:
- ✅ Model loading
- ✅ GPU configuration
- ✅ Harmony template formatting
- ✅ VRAM usage
- ✅ Inference correctness
- ✅ Streaming functionality
- ✅ Performance benchmarks

### Manual Tests

```bash
# Test 1: Simple math
curl -s -X POST http://localhost:8080/v1/inference \
  -H 'Content-Type: application/json' \
  -d '{"model": "gpt-oss-120b", "prompt": "What is 2+2?", "max_tokens": 20, "temperature": 0.1, "chain_id": 84532}' | jq .

# Test 2: Capital city
curl -s -X POST http://localhost:8080/v1/inference \
  -H 'Content-Type: application/json' \
  -d '{"model": "gpt-oss-120b", "prompt": "What is the capital of France?", "max_tokens": 50, "temperature": 0.1, "chain_id": 84532}' | jq .

# Test 3: Long context
curl -s -X POST http://localhost:8080/v1/inference \
  -H 'Content-Type: application/json' \
  -d '{"model": "gpt-oss-120b", "prompt": "Explain quantum computing in detail", "max_tokens": 500, "temperature": 0.7, "chain_id": 84532}' | jq .
```

## Troubleshooting

### Model won't load (OOM errors)
- Check VRAM availability: `nvidia-smi`
- Ensure no other processes using GPU
- Verify GPU_LAYERS=99 is appropriate for your GPU

### Inference produces garbage output
- Verify MODEL_CHAT_TEMPLATE=harmony
- Check all 3 model parts are co-located in same directory
- Review logs for "Invalid UTF-8" warnings

### Performance slower than expected
- Monitor VRAM with: `watch -n 1 nvidia-smi`
- Check context size - larger contexts are slower
- Verify Flash Attention is enabled (automatic in llama-cpp-2)

### Verification failed
- Re-download model files
- Verify no corruption: check file sizes match expected (~21GB each)
- Recompute hashes with provided commands

## Rollback Plan

If issues occur, revert to 20B model:

```bash
# Stop upgraded node
docker stop llm-node-prod-1 && docker rm llm-node-prod-1

# Edit restart-and-deploy-openai.sh to use old config:
# MODEL_PATH=/models/openai_gpt-oss-20b-MXFP4.gguf
# Remove MAX_CONTEXT_LENGTH=131072
# GPU_LAYERS=35 (or remove to use default)

# Restart with old config
./restart-and-deploy-openai.sh
```

## References

- **Hugging Face Repo**: https://huggingface.co/ggml-org/gpt-oss-120b-GGUF
- **OpenAI GPT Documentation**: https://github.com/openai/gpt-oss
- **llama.cpp Split GGUF Support**: https://github.com/ggerganov/llama.cpp/pull/2766
- **Fabstir Node Documentation**: `/workspace/docs/`

## Support

For issues or questions:
1. Check Docker logs: `docker logs llm-node-prod-1`
2. Run test suite: `./scripts/test_gpt_oss_120b.sh`
3. Review troubleshooting section above
4. Consult main documentation in `/workspace/docs/`
