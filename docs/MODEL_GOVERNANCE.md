# Model Governance System Documentation

## Overview

The Fabstir LLM Node implements a decentralized model governance system that ensures only approved, verified AI models can be used within the network. This system provides security, quality control, and trust through smart contract validation and cryptographic verification.

## Architecture

### Smart Contracts

#### 1. ModelRegistry Contract
- **Address**: `0xfE54c2aa68A7Afe8E0DD571933B556C8b6adC357` (Base Sepolia)
- **Purpose**: Maintains the authoritative list of approved models
- **Features**:
  - Curated allowlist of trusted models
  - SHA256 hash verification for model integrity
  - Tiered approval system (Tier 1: Auto-approved, Tier 2: Community)

#### 2. NodeRegistryWithModels Contract
- **Address**: `0xaa14Ed58c3EF9355501bc360E5F09Fb9EC8c1100` (Base Sepolia)
- **Purpose**: Enhanced node registry with model validation
- **Features**:
  - Validates models against ModelRegistry during registration
  - Links nodes to their supported models
  - Enables model-based node discovery

## Approved Models

Currently, only two models are approved for MVP testing:

### 1. TinyVicuna 1B (32k context)
```json
{
  "huggingface_repo": "CohereForAI/TinyVicuna-1B-32k-GGUF",
  "file_name": "tiny-vicuna-1b.q4_k_m.gguf",
  "sha256_hash": "329d002bc20d4e7baae25df802c9678b5a4340b3ce91f23e6a0644975e95935f",
  "model_id": "0x[keccak256_hash]",
  "approval_tier": 1,
  "file_size": 667814400
}
```

### 2. TinyLlama 1.1B Chat
```json
{
  "huggingface_repo": "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF",
  "file_name": "tinyllama-1b.Q4_K_M.gguf",
  "sha256_hash": "45b71fe98efe5f530b825dce6f5049d738e9c16869f10be4370ab81a9912d4a6",
  "model_id": "0x[keccak256_hash]",
  "approval_tier": 1,
  "file_size": 29
}
```

## Model ID Calculation

Model IDs are deterministically calculated using Keccak256:

```rust
fn calculate_model_id(repo: &str, filename: &str) -> H256 {
    let input = format!("{}/{}", repo, filename);
    let hash = keccak256(input.as_bytes());
    H256::from_slice(&hash)
}
```

Example:
- Input: `"CohereForAI/TinyVicuna-1B-32k-GGUF/tiny-vicuna-1b.q4_k_m.gguf"`
- Output: `0x[64-character-hex-hash]`

## Node Registration Flow

### 1. Model Validation
Before a node can register, all models must be validated:

```rust
// Load model registry client
let model_registry = ModelRegistryClient::new(
    provider,
    model_registry_address,
    Some(node_registry_address)
).await?;

// Validate models
let model_paths = vec![
    "models/tiny-vicuna-1b.q4_k_m.gguf",
    "models/tinyllama-1b.Q4_K_M.gguf"
];

let model_ids = model_registry
    .validate_models_for_registration(&model_paths)
    .await?;
```

### 2. Hash Verification
Each model file is verified against its expected SHA256 hash:

```rust
// Verify model integrity
let verified = model_registry
    .verify_model_hash(
        Path::new("models/tiny-vicuna-1b.q4_k_m.gguf"),
        "329d002bc20d4e7baae25df802c9678b5a4340b3ce91f23e6a0644975e95935f"
    )
    .await?;
```

### 3. Registration with Model IDs
Nodes register with validated model IDs:

```rust
let metadata = NodeMetadata {
    models: model_paths,
    model_ids: validated_model_ids,
    gpu: "RTX 4090".to_string(),
    ram_gb: 64,
    cost_per_token: 0.0001,
    max_concurrent_jobs: 5,
    api_url: "http://node.example.com:8080".to_string()
};

// Register with NodeRegistryWithModels
node_registration.register_node().await?;
```

## Configuration

### Environment Variables
Add to `.env` or `.env.local`:

```bash
# Model Registry Configuration
MODEL_REGISTRY_ADDRESS=0xfE54c2aa68A7Afe8E0DD571933B556C8b6adC357
NODE_REGISTRY_WITH_MODELS_ADDRESS=0xaa14Ed58c3EF9355501bc360E5F09Fb9EC8c1100

# Use new registry system
USE_NEW_REGISTRY=true

# Model paths
MODEL_PATH=./models/tiny-vicuna-1b.q4_k_m.gguf
```

### Registration Config
```rust
let config = RegistrationConfig {
    contract_address: node_registry_address,
    model_registry_address: model_registry_address,
    stake_amount: U256::from(1000000),
    auto_register: true,
    heartbeat_interval: 60,
    use_new_registry: true,  // Enable model governance
};
```

## Client Discovery

Clients can discover nodes offering specific models:

### Find Nodes by Model
```rust
// Find all nodes supporting TinyVicuna
let vicuna_id = model_registry.get_model_id(
    "CohereForAI/TinyVicuna-1B-32k-GGUF",
    "tiny-vicuna-1b.q4_k_m.gguf"
);

let nodes = model_registry
    .find_hosts_for_model(vicuna_id)
    .await?;
```

### Query Node Models via API
```bash
curl http://localhost:8080/v1/models
```

Response includes model registry information:
```json
{
  "models": [
    {
      "id": "0x...",
      "name": "tiny-vicuna-1b.q4_k_m.gguf",
      "huggingface_repo": "CohereForAI/TinyVicuna-1B-32k-GGUF",
      "sha256_hash": "329d002...",
      "approval_tier": 1
    }
  ]
}
```

## Security Features

### 1. Model Integrity
- **SHA256 Verification**: Every model file is verified against its registered hash
- **Immutable Registry**: Model hashes cannot be changed once registered
- **Deterministic IDs**: Model IDs are cryptographically derived from repo/filename

### 2. Access Control
- **Owner-Curated**: Tier 1 models are directly approved by contract owner
- **Community Governance**: Tier 2 models require community voting (future)
- **Registration Gate**: Nodes cannot register without approved models

### 3. Trust Chain
```
ModelRegistry (Source of Truth)
    ↓
NodeRegistryWithModels (Validates against Registry)
    ↓
Node Registration (Must have approved models)
    ↓
Client Discovery (Only finds validated nodes)
```

## Testing

### Unit Tests
```bash
# Test model ID calculation
cargo test test_model_id_calculation

# Test model validation
cargo test test_model_validation

# Test SHA256 verification
cargo test test_sha256_verification
```

### Integration Tests
```bash
# Test full registration flow
cargo test test_node_registration_with_models

# Test model registry client
cargo test test_model_registry_client
```

## Migration Guide

### From Legacy NodeRegistry to NodeRegistryWithModels

1. **Update Contract Addresses**
   ```bash
   # Old
   NODE_REGISTRY_FAB_ADDRESS=0x87516C13Ea2f99de598665e14cab64E191A0f8c4

   # New
   NODE_REGISTRY_WITH_MODELS_ADDRESS=0xaa14Ed58c3EF9355501bc360E5F09Fb9EC8c1100
   MODEL_REGISTRY_ADDRESS=0xfE54c2aa68A7Afe8E0DD571933B556C8b6adC357
   ```

2. **Update Registration Code**
   ```rust
   // Enable new registry
   config.use_new_registry = true;
   config.model_registry_address = model_registry_address;
   ```

3. **Ensure Models are Downloaded**
   ```bash
   # Download approved models
   ./scripts/download_approved_models.sh
   ```

4. **Update Metadata Format**
   - Old: Comma-separated strings
   - New: Structured JSON with model IDs

## Troubleshooting

### Common Issues

#### Model Not Approved
```
Error: Model tiny-llama-2b.gguf is not in approved list
```
**Solution**: Use only approved models (TinyVicuna or TinyLlama)

#### Hash Verification Failed
```
Error: Model hash mismatch! Expected: 329d002..., Got: abc123...
```
**Solution**: Re-download the model from the official source

#### Registration Failed
```
Error: No approved models configured for registration
```
**Solution**: Ensure model files exist and pass validation

### Debug Commands

```bash
# Check model hashes
sha256sum models/*.gguf

# Verify contract addresses
cast call $MODEL_REGISTRY_ADDRESS "getAllModels()" --rpc-url $RPC_URL

# Test model validation
cargo run --example validate_models
```

## Future Enhancements

### Phase 1: Community Governance (Q2 2025)
- Tier 2 model proposals
- DAO voting mechanism
- Reputation-based voting weight

### Phase 2: Advanced Features (Q3 2025)
- Model versioning support
- Automatic model updates
- Performance benchmarking requirements
- Slashing for serving unapproved models

### Phase 3: Decentralized Curation (Q4 2025)
- Multiple registry support
- Cross-chain model validation
- IPFS/Arweave model storage
- Zero-knowledge proofs for private models

## API Reference

### ModelRegistryClient

```rust
pub struct ModelRegistryClient {
    // Get model ID from repo and filename
    pub fn get_model_id(&self, repo: &str, file: &str) -> H256;

    // Check if model is approved
    pub async fn is_model_approved(&self, model_id: H256) -> Result<bool>;

    // Get model details
    pub async fn get_model_details(&self, model_id: H256) -> Result<ModelInfo>;

    // Verify model file hash
    pub async fn verify_model_hash(&self, path: &Path, hash: &str) -> Result<bool>;

    // Validate models for registration
    pub async fn validate_models_for_registration(&self, paths: &[String]) -> Result<Vec<H256>>;

    // Find hosts supporting a model
    pub async fn find_hosts_for_model(&self, model_id: H256) -> Result<Vec<Address>>;
}
```

## Contract ABIs

Contract ABIs are located in:
- `contracts/ModelRegistry-CLIENT-ABI.json`
- `contracts/NodeRegistryWithModels-CLIENT-ABI.json`

## Resources

- [ModelRegistry Contract](https://basescan.org/address/0xfE54c2aa68A7Afe8E0DD571933B556C8b6adC357)
- [NodeRegistryWithModels Contract](https://basescan.org/address/0xaa14Ed58c3EF9355501bc360E5F09Fb9EC8c1100)
- [HuggingFace Model Hub](https://huggingface.co/models)
- [GGUF Format Specification](https://github.com/ggerganov/ggml/blob/master/docs/gguf.md)