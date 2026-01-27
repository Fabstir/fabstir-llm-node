# Model Governance Quick Start Guide

## For Node Operators

### 1. Download Approved Models

Only these two models are currently approved:

```bash
# Create models directory
mkdir -p models

# Download TinyVicuna (667MB)
wget https://huggingface.co/CohereForAI/TinyVicuna-1B-32k-GGUF/resolve/main/tiny-vicuna-1b.q4_k_m.gguf \
  -O models/tiny-vicuna-1b.q4_k_m.gguf

# Download TinyLlama (placeholder - needs actual model)
# Note: The actual model file needs to be downloaded
wget https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf \
  -O models/tinyllama-1b.Q4_K_M.gguf
```

### 2. Verify Model Hashes

```bash
# Expected hashes
VICUNA_HASH="329d002bc20d4e7baae25df802c9678b5a4340b3ce91f23e6a0644975e95935f"
LLAMA_HASH="45b71fe98efe5f530b825dce6f5049d738e9c16869f10be4370ab81a9912d4a6"

# Verify
sha256sum models/*.gguf
```

### 3. Update Configuration

Add to `.env`:

```bash
# Model Governance (REQUIRED)
MODEL_REGISTRY_ADDRESS=0xfE54c2aa68A7Afe8E0DD571933B556C8b6adC357
NODE_REGISTRY_WITH_MODELS_ADDRESS=0xaa14Ed58c3EF9355501bc360E5F09Fb9EC8c1100
USE_NEW_REGISTRY=true

# Your node configuration
API_URL=http://your-node.com:8080
MODEL_PATH=./models/tiny-vicuna-1b.q4_k_m.gguf
```

### 4. Run Node with Model Validation

```bash
# The node will automatically:
# 1. Validate models against ModelRegistry
# 2. Calculate model IDs
# 3. Register with approved models only

cargo run --release
```

## For SDK Developers

### 1. Find Nodes with Specific Models

```typescript
// Calculate model ID
const modelId = keccak256(
  toUtf8Bytes("CohereForAI/TinyVicuna-1B-32k-GGUF/tiny-vicuna-1b.q4_k_m.gguf")
);

// Find nodes
const nodes = await nodeRegistry.getNodesForModel(modelId);
```

### 2. Verify Model Support

```typescript
// Check if node supports a model
const nodeInfo = await nodeRegistry.getNodeFullInfo(nodeAddress);
const supportedModels = nodeInfo[5]; // Array of model IDs

if (supportedModels.includes(modelId)) {
  console.log("Node supports requested model");
}
```

## Approved Models Reference

| Model | Repo | File | SHA256 | Size |
|-------|------|------|--------|------|
| TinyVicuna 1B | `CohereForAI/TinyVicuna-1B-32k-GGUF` | `tiny-vicuna-1b.q4_k_m.gguf` | `329d002b...` | 667MB |
| TinyLlama 1.1B | `TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF` | `tinyllama-1b.Q4_K_M.gguf` | `45b71fe9...` | TBD |

## Contract Addresses

| Contract | Address | Network |
|----------|---------|---------|
| ModelRegistry | `0xfE54c2aa68A7Afe8E0DD571933B556C8b6adC357` | Base Sepolia |
| NodeRegistryWithModels | `0xaa14Ed58c3EF9355501bc360E5F09Fb9EC8c1100` | Base Sepolia |

## Troubleshooting

### "Model not approved" error
- Ensure you're using exact filenames: `tiny-vicuna-1b.q4_k_m.gguf` or `tinyllama-1b.Q4_K_M.gguf`
- Verify SHA256 hashes match exactly

### "Failed to calculate model ID"
- Model ID = keccak256("`repo`/`filename`")
- Use exact repo paths from approved list

### "Registration failed"
- Check `USE_NEW_REGISTRY=true` is set
- Verify both contract addresses are correct
- Ensure models pass hash verification

## Testing

```bash
# Test model validation
cargo test test_model_id_calculation
cargo test test_model_validation

# Run with test configuration
MODEL_REGISTRY_ADDRESS=0xfE54c2aa68A7Afe8E0DD571933B556C8b6adC357 \
NODE_REGISTRY_WITH_MODELS_ADDRESS=0xaa14Ed58c3EF9355501bc360E5F09Fb9EC8c1100 \
USE_NEW_REGISTRY=true \
cargo run --release
```