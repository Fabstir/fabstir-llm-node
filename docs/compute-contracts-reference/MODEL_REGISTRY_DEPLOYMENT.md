# Model Registry Deployment Guide

## Clean Deployment for MVP Testing

This guide shows how to deploy the ModelRegistry with ONLY the two approved models for testing.

## Deployed Contracts

- **ModelRegistry**: `0xA1F2FCf756551cbEE90D4224f30C887B36c08d6D`
  - ⚠️ **Note**: Currently has 4 models (should only have 2)
  - Need to redeploy with only the approved models

- **NodeRegistryWithModels**: Not yet deployed to mainnet
  - Ready for deployment once ModelRegistry is corrected

## Approved Models for Testing (ONLY THESE TWO)

### 1. TinyVicuna-1B-32k
```json
{
  "huggingfaceRepo": "CohereForAI/TinyVicuna-1B-32k-GGUF",
  "fileName": "tiny-vicuna-1b.q4_k_m.gguf",
  "sha256Hash": "0x329d002bc20d4e7baae25df802c9678b5a4340b3ce91f23e6a0644975e95935f",
  "quantization": "Q4_K_M",
  "description": "TinyVicuna 1B model with 32k context"
}
```

### 2. TinyLlama-1.1B Chat
```json
{
  "huggingfaceRepo": "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF",
  "fileName": "tinyllama-1b.Q4_K_M.gguf",
  "sha256Hash": "0x45b71fe98efe5f530b825dce6f5049d738e9c16869f10be4370ab81a9912d4a6",
  "quantization": "Q4_K_M",
  "description": "TinyLlama 1.1B Chat model"
}
```

## Correct Deployment Commands

### Step 1: Deploy ModelRegistry
```bash
forge create src/ModelRegistry.sol:ModelRegistry \
    --rpc-url "$BASE_SEPOLIA_RPC_URL" \
    --private-key "$PRIVATE_KEY" \
    --constructor-args "0xC78949004B4EB6dEf2D66e49Cd81231472612D62" \
    --legacy \
    --broadcast
```

### Step 2: Add ONLY the two approved models
```bash
# Add TinyVicuna-1B
cast send <MODEL_REGISTRY_ADDRESS> "addTrustedModel(string,string,bytes32)" \
    "CohereForAI/TinyVicuna-1B-32k-GGUF" \
    "tiny-vicuna-1b.q4_k_m.gguf" \
    "0x329d002bc20d4e7baae25df802c9678b5a4340b3ce91f23e6a0644975e95935f" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL" \
    --private-key "$PRIVATE_KEY" \
    --legacy

# Add TinyLlama-1.1B
cast send <MODEL_REGISTRY_ADDRESS> "addTrustedModel(string,string,bytes32)" \
    "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF" \
    "tinyllama-1b.Q4_K_M.gguf" \
    "0x45b71fe98efe5f530b825dce6f5049d738e9c16869f10be4370ab81a9912d4a6" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL" \
    --private-key "$PRIVATE_KEY" \
    --legacy
```

### Step 3: Deploy NodeRegistryWithModels
```bash
forge create src/NodeRegistryWithModels.sol:NodeRegistryWithModels \
    --rpc-url "$BASE_SEPOLIA_RPC_URL" \
    --private-key "$PRIVATE_KEY" \
    --constructor-args "0xC78949004B4EB6dEf2D66e49Cd81231472612D62" "<MODEL_REGISTRY_ADDRESS>" \
    --legacy \
    --broadcast
```

## Host Registration Example

Once deployed, hosts can register with these model IDs:

```javascript
// Calculate model IDs
const tinyVicunaId = ethers.utils.keccak256(
  ethers.utils.toUtf8Bytes("CohereForAI/TinyVicuna-1B-32k-GGUF/tiny-vicuna-1b.q4_k_m.gguf")
);

const tinyLlamaId = ethers.utils.keccak256(
  ethers.utils.toUtf8Bytes("TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/tinyllama-1b.Q4_K_M.gguf")
);

// Register with structured metadata
const metadata = JSON.stringify({
  "hardware": {
    "gpu": "rtx-4090",
    "vram": 24,
    "cpu": "AMD Ryzen 9"
  },
  "capabilities": ["inference", "streaming"],
  "location": "us-east",
  "maxConcurrent": 5
});

await nodeRegistry.registerNode(
  metadata,
  "http://my-host.example.com:8080",
  [tinyVicunaId, tinyLlamaId]  // Supporting both approved models
);
```

## Important Notes

1. **ONLY 2 MODELS**: The system should only have the two models specified above for MVP testing
2. **SHA256 Verification**: The hashes are real and should be verified against HuggingFace
3. **No Additional Models**: Do not add Llama-2, Mistral, or any other models until after MVP
4. **Structured Metadata**: Use JSON format for node metadata, not comma-separated strings

## Current Issue

The currently deployed ModelRegistry at `0xA1F2FCf756551cbEE90D4224f30C887B36c08d6D` has 4 models instead of 2:
- ❌ Llama-2-7B-GGUF (should not be there)
- ❌ Mistral-7B-Instruct (should not be there)
- ✅ TinyVicuna-1B-32k
- ✅ TinyLlama-1.1B

**Recommendation**: Deploy a fresh ModelRegistry with only the two approved models.