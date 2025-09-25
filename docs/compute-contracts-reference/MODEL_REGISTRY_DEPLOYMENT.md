# Model Registry Deployment Guide

*Last Updated: January 2025*

## Clean Deployment for MVP Testing

This guide shows the deployed ModelRegistry with ONLY the two approved models for testing.

## Deployed Contracts (Current - January 2025)

### Core Contracts

- **ModelRegistry**: `0x92b2De840bB2171203011A6dBA928d855cA8183E`
  - ✅ **Status**: Deployed and active with exactly 2 approved models
  - **Owner**: `0xbeaBB2a5AEd358aA0bd442dFFd793411519Bdc11`
  - **Network**: Base Sepolia

- **NodeRegistryWithModels**: `0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218`
  - ✅ **Status**: Deployed and integrated with ModelRegistry
  - **Features**: Enforces model validation, API URL discovery, FAB staking
  - **Min Stake**: 1000 FAB tokens

- **JobMarketplaceWithModels**: `0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944`
  - ✅ **Status**: Active and using model validation
  - **Features**: Session jobs with proof checkpoints
  - **Integration**: Validates hosts have required models

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

## Verification Commands

### Verify ModelRegistry has correct models:
```bash
# Check all registered models
cast call 0x92b2De840bB2171203011A6dBA928d855cA8183E "getAllModels()" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL"

# Should return 2 model IDs:
# 0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced (TinyVicuna)
# 0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca (TinyLlama)
```

### Verify NodeRegistryWithModels integration:
```bash
# Check ModelRegistry address
cast call 0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218 "modelRegistry()" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL"

# Should return: 0x92b2De840bB2171203011A6dBA928d855cA8183E
```

## Deployment Scripts (Already Executed)

For reference, these were the deployment commands used:

### Step 1: Deploy ModelRegistry
```bash
forge script script/DeployModelRegistry.s.sol:DeployModelRegistry \
    --rpc-url "$BASE_SEPOLIA_RPC_URL" \
    --private-key "$PRIVATE_KEY" \
    --broadcast
```

### Step 2: Add approved models
```bash
# TinyVicuna-1B
cast send 0x92b2De840bB2171203011A6dBA928d855cA8183E \
    "addTrustedModel(string,string,bytes32)" \
    "CohereForAI/TinyVicuna-1B-32k-GGUF" \
    "tiny-vicuna-1b.q4_k_m.gguf" \
    "0x329d002bc20d4e7baae25df802c9678b5a4340b3ce91f23e6a0644975e95935f"

# TinyLlama-1.1B
cast send 0x92b2De840bB2171203011A6dBA928d855cA8183E \
    "addTrustedModel(string,string,bytes32)" \
    "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF" \
    "tinyllama-1b.Q4_K_M.gguf" \
    "0x45b71fe98efe5f530b825dce6f5049d738e9c16869f10be4370ab81a9912d4a6"
```

### Step 3: Deploy NodeRegistryWithModels
```bash
forge script script/DeployNodeRegistryWithModels.s.sol:DeployNodeRegistryWithModels \
    --rpc-url "$BASE_SEPOLIA_RPC_URL" \
    --private-key "$PRIVATE_KEY" \
    --broadcast
```

## ModelRegistry Management Functions

### For Contract Owner Only:
```javascript
// Add a new trusted model (owner only)
await modelRegistry.addTrustedModel(
  "NewRepo/Model-GGUF",
  "model.gguf",
  "0xhash..."
);

// Deactivate a model (emergency)
await modelRegistry.deactivateModel(modelId);

// Reactivate a model
await modelRegistry.reactivateModel(modelId);
```

### For Reading Model Data:
```javascript
// Get all model IDs
const modelIds = await modelRegistry.getAllModels();

// Get model details
const model = await modelRegistry.getModel(modelId);
console.log({
  repo: model.huggingfaceRepo,
  file: model.fileName,
  hash: model.sha256Hash,
  tier: model.approvalTier, // 1 = owner, 2 = community
  active: model.active,
  timestamp: model.timestamp
});

// Check if model is approved
const isApproved = await modelRegistry.isModelApproved(modelId);
```

## Host Registration Example

Once deployed, hosts can register with these model IDs:

```javascript
// Model IDs (pre-calculated)
const tinyVicunaId = '0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced';
const tinyLlamaId = '0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca';

// Or calculate them:
const modelRegistry = new ethers.Contract(
  '0x92b2De840bB2171203011A6dBA928d855cA8183E',
  ['function getModelId(string,string) pure returns (bytes32)'],
  provider
);

const tinyVicunaIdCalc = await modelRegistry.getModelId(
  "CohereForAI/TinyVicuna-1B-32k-GGUF",
  "tiny-vicuna-1b.q4_k_m.gguf"
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

## Integration with JobMarketplace

The **JobMarketplaceWithModels** at `0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944` is fully integrated with this ModelRegistry:
- Validates that hosts have registered with supported models
- Ensures only approved models can be used in the marketplace
- Prevents job creation with hosts that don't support required models

## Quick Reference: Model IDs

```
TinyVicuna-1B: 0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced
TinyLlama-1.1B: 0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca
```

## Useful Query Commands

```bash
# Get all approved models
cast call 0x92b2De840bB2171203011A6dBA928d855cA8183E "getAllModels()" --rpc-url "$BASE_SEPOLIA_RPC_URL"

# Check if a specific model is approved
cast call 0x92b2De840bB2171203011A6dBA928d855cA8183E "isModelApproved(bytes32)" <MODEL_ID> --rpc-url "$BASE_SEPOLIA_RPC_URL"

# Get model details
cast call 0x92b2De840bB2171203011A6dBA928d855cA8183E "getModel(bytes32)" <MODEL_ID> --rpc-url "$BASE_SEPOLIA_RPC_URL"

# Check which models a host supports
cast call 0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218 "getNodeSupportedModels(address)" <HOST_ADDRESS> --rpc-url "$BASE_SEPOLIA_RPC_URL"
```