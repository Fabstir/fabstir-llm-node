# Node v8.13.0 AUDIT-F4 Migration Guide for SDK Developers

**Version**: v8.13.0-audit-remediation-2026-02-01
**Date**: February 1, 2026
**Author**: Fabstir Core Team
**Audience**: SDK Developers integrating with Fabstir LLM Node

---

## Table of Contents

1. [Overview](#overview)
2. [What's New](#whats-new)
3. [Breaking Changes](#breaking-changes)
4. [Tarball Deployment](#tarball-deployment)
5. [Contract Updates](#contract-updates)
6. [SDK Integration](#sdk-integration)
7. [Testing & Verification](#testing--verification)
8. [Migration Checklist](#migration-checklist)
9. [Troubleshooting](#troubleshooting)

---

## Overview

Node version **v8.13.0-audit-remediation** implements fixes for the AUDIT pre-report security audit findings (AUDIT-F1 through AUDIT-F5). The primary change is **AUDIT-F4**, which requires proof signatures to include `modelId` to prevent cross-model replay attacks.

### Key Facts

- **Tarball**: `fabstir-llm-node-v8.13.0-audit-remediation.tar.gz` (557MB)
- **Binary Size**: 991MB (uncompressed)
- **Build Date**: 2026-02-01
- **Compliance**: AUDIT-F4 pre-report remediation
- **Real Proofs**: Risc0 STARK proofs enabled (221KB per proof)
- **Test Coverage**: 852/856 tests passing (99.5%)

---

## What's New

### AUDIT-F4: Model ID in Proof Signatures (BREAKING)

**Problem**: Previous signature format allowed replay attacks where a proof from a cheap model could be reused on an expensive model.

**Solution**: Signatures now include `modelId` as the 4th parameter.

```solidity
// OLD (3 parameters - DEPRECATED)
dataHash = keccak256(abi.encodePacked(
    proofHash,      // bytes32
    hostAddress,    // address
    tokensClaimed   // uint256
));
// Encoded size: 84 bytes

// NEW (4 parameters - AUDIT-F4)
dataHash = keccak256(abi.encodePacked(
    proofHash,      // bytes32
    hostAddress,    // address
    tokensClaimed,  // uint256
    modelId         // bytes32
));
// Encoded size: 116 bytes
```

### Feature Flags in Binary

The following features are verified in the release binary:

- ✅ `audit-f4-compliance` - AUDIT-F4 signature format
- ✅ `model-id-signature` - 4-parameter signatures
- ✅ `cross-model-replay-protection` - Security enhancement
- ✅ `session-model-query` - Automatic modelId lookup
- ✅ `risc0-zkvm` - Real STARK proof generation
- ✅ `zero-knowledge-proofs` - Production ZK proofs
- ✅ `end-to-end-encryption` - ECDH + XChaCha20-Poly1305
- ✅ `checkpoint-blockchain-events` - Delta CID on-chain

---

## Breaking Changes

### 1. Proof Signature Format (AUDIT-F4)

**Impact**: All proof submissions now require `modelId` in signature.

**Node Behavior**:
- Node automatically queries `sessionModel(sessionId)` from JobMarketplace contract
- For non-model sessions: `modelId = bytes32(0)`
- Node generates AUDIT-F4 compliant signatures with 4 parameters

**SDK Impact**:
- ❌ **No SDK changes required** - node handles modelId internally
- ✅ SDK continues to receive proof events as before
- ✅ Backward compatible with existing SDK code

### 2. Contract Addresses (Updated)

**Old Contracts (Deprecated)**:
```
JobMarketplace: 0x3CaCbf3f448B420918A93a88706B26Ab27a3523E (pre-AUDIT-F4)
ProofSystem:    0x5afB91977e69Cc5003288849059bc62d47E7deeb (pre-AUDIT-F4)
```

**New Contracts (AUDIT-F4 Remediated)**:
```
JobMarketplace: 0x95132177F964FF053C1E874b53CF74d819618E06 (AUDIT-F4 compliant)
ProofSystem:    0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31 (AUDIT-F4 compliant)
NodeRegistry:   0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22 (unchanged)
ModelRegistry:  0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2 (unchanged)
HostEarnings:   0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0 (unchanged)
```

### 3. submitProofOfWork Signature

**Old Contract Function**:
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    string memory proofCID,
    bytes memory signature  // 3-parameter signature
) external;
```

**New Contract Function (AUDIT-F4)**:
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    string memory proofCID,
    bytes memory signature,      // 4-parameter signature
    string memory deltaCID       // 6th parameter (already in v8.12.4+)
) external;
```

**What Changed**:
- `signature` parameter now expects 4-parameter encoding (116 bytes vs 84 bytes)
- Node automatically generates correct signature format
- SDK doesn't need to handle signature generation

---

## Tarball Deployment

### Step 1: Extract Tarball

The tarball contains the binary at the **root level** (not in `target/release/`):

```bash
# Extract tarball
tar -xzvf fabstir-llm-node-v8.13.0-audit-remediation.tar.gz

# Contents:
# fabstir-llm-node                    (binary at root)
# scripts/download_embedding_model.sh
# scripts/download_florence_model.sh
# scripts/download_ocr_models.sh
# scripts/setup_models.sh
```

### Step 2: Move Binary to Expected Location

Docker and deployment scripts expect the binary at `target/release/`:

```bash
# Create target directory
mkdir -p target/release/

# Move binary to expected location
mv fabstir-llm-node target/release/

# Verify
ls -lh target/release/fabstir-llm-node
# Expected: -rwxr-xr-x ... 991M ... fabstir-llm-node
```

### Step 3: Set Up Models (Optional)

If using vision, OCR, or RAG features:

```bash
# Run master setup script (downloads all models)
./scripts/setup_models.sh

# Or download individually:
./scripts/download_embedding_model.sh   # RAG embeddings (all-MiniLM-L6-v2)
./scripts/download_florence_model.sh    # Vision model (Florence-2)
./scripts/download_ocr_models.sh        # OCR models (PaddleOCR)
```

### Step 4: Verify Binary

```bash
# Check version
strings target/release/fabstir-llm-node | grep "v8.13.0"
# Expected: v8.13.0-audit-remediation-2026-02-01

# Verify AUDIT-F4 features
strings target/release/fabstir-llm-node | grep "audit-f4-compliance"
# Expected: audit-f4-compliance

# Verify real proofs enabled
strings target/release/fabstir-llm-node | grep "risc0-zkvm"
# Expected: risc0-zkvm

# Check CUDA support (if deploying with GPU)
ldd target/release/fabstir-llm-node | grep cuda
# Expected: libcuda.so, libcudart.so, etc.
```

### Step 5: Update Environment Configuration

Create or update `.env` file with new contract addresses:

```bash
# .env
CHAIN_ID=84532  # Base Sepolia

# AUDIT-F4 Remediated Contracts (Use these!)
CONTRACT_JOB_MARKETPLACE=0x95132177F964FF053C1E874b53CF74d819618E06
CONTRACT_PROOF_SYSTEM=0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31

# Unchanged Contracts
CONTRACT_NODE_REGISTRY=0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22
CONTRACT_MODEL_REGISTRY=0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2
CONTRACT_HOST_EARNINGS=0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0

# Token Contracts (Base Sepolia)
FAB_TOKEN=0xC78949004B4EB6dEf2D66e49Cd81231472612D62
USDC_TOKEN=0x036CbD53842c5426634e7929541eC2318f3dCF7e

# RPC
RPC_URL=https://sepolia.base.org

# Host Configuration (Required for checkpoints & encryption)
HOST_PRIVATE_KEY=0x...  # Your host wallet private key

# Optional: S5 Storage
ENHANCED_S5_URL=http://localhost:5522
S5_NODE_URL=https://s5.platformlessai.ai

# Optional: Model Paths
MODEL_PATH=./models/model.gguf
EMBEDDING_MODEL_PATH=./models/all-MiniLM-L6-v2-onnx/model.onnx
```

### Step 6: Start Node

```bash
# Standard startup
./target/release/fabstir-llm-node

# With custom ports
P2P_PORT=9001 API_PORT=8081 ./target/release/fabstir-llm-node

# With GPU selection
CUDA_VISIBLE_DEVICES=0 ./target/release/fabstir-llm-node

# With debug logging
RUST_LOG=debug ./target/release/fabstir-llm-node
```

### Step 7: Verify Node Startup

Look for these log messages:

```
✅ Expected startup logs:
[INFO] fabstir_llm_node: Starting Fabstir LLM Node v8.13.0-audit-remediation-2026-02-01
[INFO] Using contract addresses: JobMarketplace=0x9513...8E06, ProofSystem=0xE8DC...1B31
[INFO] AUDIT-F4 compliance enabled: 4-parameter signatures
[S5-INIT] Using EnhancedS5Backend with ENHANCED_S5_URL=http://localhost:5522
[INFO] P2P listening on /ip4/0.0.0.0/tcp/9000
[INFO] API server listening on http://0.0.0.0:8080

❌ Warning signs (check configuration):
[WARN] Using MockS5Backend - uploads won't reach S5 network!
[ERROR] Failed to connect to contract at 0x...
[ERROR] Invalid HOST_PRIVATE_KEY format
```

---

## Contract Updates

### Contract ABI Changes

**IMPORTANT**: The SDK must use updated contract ABIs.

**Updated Files** (from node repository):
```
docs/compute-contracts-reference/client-abis/JobMarketplaceWithModels-CLIENT-ABI.json
docs/compute-contracts-reference/client-abis/ProofSystemAuditF4-CLIENT-ABI.json
```

**Key ABI Differences**:

```javascript
// JobMarketplace - New function for modelId queries
{
    "inputs": [{"type": "uint256", "name": "sessionId"}],
    "name": "sessionModel",
    "outputs": [{"type": "bytes32", "name": "modelId"}],
    "stateMutability": "view",
    "type": "function"
}

// ProofSystem - Updated submitProofOfWork signature
{
    "inputs": [
        {"type": "uint256", "name": "jobId"},
        {"type": "uint256", "name": "tokensClaimed"},
        {"type": "bytes32", "name": "proofHash"},
        {"type": "string", "name": "proofCID"},
        {"type": "bytes", "name": "signature"},      // Now 4-parameter (116 bytes)
        {"type": "string", "name": "deltaCID"}
    ],
    "name": "submitProofOfWork",
    "outputs": [],
    "stateMutability": "nonpayable",
    "type": "function"
}
```

### Updating SDK Contract Configuration

```typescript
// sdk/src/config/contracts.ts

// ❌ OLD (Deprecated)
export const CONTRACTS = {
  JOB_MARKETPLACE: '0x3CaCbf3f448B420918A93a88706B26Ab27a3523E',  // Pre-AUDIT-F4
  PROOF_SYSTEM: '0x5afB91977e69Cc5003288849059bc62d47E7deeb',     // Pre-AUDIT-F4
  // ...
};

// ✅ NEW (AUDIT-F4 Compliant)
export const CONTRACTS = {
  JOB_MARKETPLACE: '0x95132177F964FF053C1E874b53CF74d819618E06',  // AUDIT-F4
  PROOF_SYSTEM: '0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31',     // AUDIT-F4
  NODE_REGISTRY: '0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22',    // Unchanged
  MODEL_REGISTRY: '0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2',   // Unchanged
  HOST_EARNINGS: '0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0',   // Unchanged
};

// Update ABI imports
import JobMarketplaceABI from './abis/JobMarketplaceWithModels-CLIENT-ABI.json';
import ProofSystemABI from './abis/ProofSystemAuditF4-CLIENT-ABI.json';
```

---

## SDK Integration

### What SDK Developers Need to Know

**Good News**: Most SDK code doesn't need changes! The node handles AUDIT-F4 compliance internally.

### SDK Code Changes Required

#### 1. Update Contract Addresses (Required)

```typescript
// Update to new contract addresses
const JOB_MARKETPLACE_ADDRESS = '0x95132177F964FF053C1E874b53CF74d819618E06';
const PROOF_SYSTEM_ADDRESS = '0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31';
```

#### 2. Update Contract ABIs (Required)

Download latest ABIs from node repository:
- `JobMarketplaceWithModels-CLIENT-ABI.json`
- `ProofSystemAuditF4-CLIENT-ABI.json`

#### 3. No Changes to Inference API (No Action Required)

The inference API remains unchanged:

```typescript
// ✅ This code still works unchanged
const response = await fetch('http://localhost:8080/v1/inference', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    prompt: 'Hello, world!',
    max_tokens: 100,
    stream: false
  })
});
```

#### 4. No Changes to WebSocket API (No Action Required)

WebSocket protocol remains unchanged:

```typescript
// ✅ This code still works unchanged
const ws = new WebSocket('ws://localhost:8080/v1/ws');

ws.on('message', (data) => {
  const msg = JSON.parse(data);
  if (msg.type === 'token') {
    console.log('Token:', msg.content);
  }
});
```

#### 5. No Changes to Checkpoint Recovery (No Action Required)

Checkpoint API remains unchanged:

```typescript
// ✅ This code still works unchanged
const checkpoints = await fetch(`http://localhost:8080/v1/checkpoints/${sessionId}`);
const data = await checkpoints.json();
```

### Event Monitoring (Optional Update)

If your SDK monitors ProofSubmitted events, note the signature format change:

```typescript
// ProofSubmitted event structure (unchanged)
event ProofSubmitted(
    uint256 indexed jobId,
    address indexed host,
    bytes32 proofHash,
    string proofCID,
    uint256 tokensClaimed,
    string deltaCID
);

// ✅ Event fields are the same - no SDK changes needed
// The signature parameter is internal to submitProofOfWork function
```

### Model-Based Sessions (Optional)

If your SDK creates model-based sessions, no changes are needed:

```typescript
// ✅ Node automatically queries modelId for model-based sessions
const tx = await jobMarketplace.createSessionFromDepositForModel(
  modelId,
  paymentToken,
  tokenAmount,
  proofTimeoutWindow  // New parameter (AUDIT-F3)
);

// Node will query: sessionModel(sessionId) => modelId
// Node will include modelId in proof signature
```

### Non-Model Sessions (Optional)

For non-model sessions (if supported):

```typescript
// ✅ Node automatically uses modelId = bytes32(0)
const tx = await jobMarketplace.createSessionFromDeposit(
  paymentToken,
  tokenAmount,
  proofTimeoutWindow
);

// Node will use: modelId = 0x0000...0000
// Signature still includes modelId (as zero bytes)
```

---

## Testing & Verification

### Test 1: Verify Node Version

```bash
# Check version string
strings target/release/fabstir-llm-node | grep "v8.13.0"

# Expected output:
# v8.13.0-audit-remediation-2026-02-01
```

### Test 2: Verify AUDIT-F4 Features

```bash
# Check for AUDIT-F4 compliance
strings target/release/fabstir-llm-node | grep -E "audit-f4|model-id-signature"

# Expected output:
# audit-f4-compliance
# model-id-signature
# cross-model-replay-protection
# session-model-query
```

### Test 3: Verify Real Proofs Enabled

```bash
# Check for real-ezkl feature
strings target/release/fabstir-llm-node | grep "risc0-zkvm"

# Expected output:
# risc0-zkvm
# zero-knowledge-proofs
```

### Test 4: Node Startup Test

```bash
# Start node with debug logging
RUST_LOG=debug ./target/release/fabstir-llm-node

# Look for these logs:
# ✅ "Starting Fabstir LLM Node v8.13.0-audit-remediation"
# ✅ "AUDIT-F4 compliance enabled"
# ✅ "Using contract addresses: JobMarketplace=0x9513..."
# ✅ "P2P listening on /ip4/0.0.0.0/tcp/9000"
# ✅ "API server listening on http://0.0.0.0:8080"
```

### Test 5: Inference Request Test

```bash
# Simple inference test
curl -X POST http://localhost:8080/v1/inference \
  -H 'Content-Type: application/json' \
  -d '{
    "prompt": "Hello, what is 2+2?",
    "max_tokens": 50,
    "stream": false
  }'

# Expected: JSON response with generated text
```

### Test 6: Checkpoint Submission Test

Create a test session and verify checkpoint submission:

```typescript
// 1. Create session (SDK)
const tx = await jobMarketplace.createSessionFromDepositForModel(
  modelId,
  paymentToken,
  ethers.parseUnits('1', 6), // 1 USDC
  3600 // proofTimeoutWindow
);
await tx.wait();

// 2. Send inference request (SDK)
const response = await fetch('http://localhost:8080/v1/inference', {
  method: 'POST',
  body: JSON.stringify({
    job_id: sessionId,
    prompt: 'Generate 100 tokens to trigger checkpoint',
    max_tokens: 100
  })
});

// 3. Monitor node logs for checkpoint submission
// Look for:
// ✅ "📊 Tracking tokens for job X: 50 tokens"
// ✅ "🚨 TRIGGERING checkpoint submission for job X (50 tokens)"
// ✅ "Querying sessionModel for session X"
// ✅ "Got modelId for session X: 0x..."
// ✅ "Generating 4-parameter signature (AUDIT-F4)"
// ✅ "✅ Checkpoint submitted successfully"
```

### Test 7: Contract Event Monitoring

Monitor ProofSubmitted events from new contract:

```typescript
const proofSystem = new ethers.Contract(
  '0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31',  // New address
  ProofSystemABI,
  provider
);

proofSystem.on('ProofSubmitted', (jobId, host, proofHash, proofCID, tokensClaimed, deltaCID) => {
  console.log('Proof submitted:', {
    jobId: jobId.toString(),
    host,
    proofHash,
    proofCID,
    tokensClaimed: tokensClaimed.toString(),
    deltaCID
  });
});

// Expected: Events from checkpoint submissions
```

---

## Migration Checklist

Use this checklist to ensure smooth migration:

### Pre-Deployment

- [ ] Download tarball: `fabstir-llm-node-v8.13.0-audit-remediation.tar.gz`
- [ ] Verify tarball integrity: `sha256sum fabstir-llm-node-v8.13.0-audit-remediation.tar.gz`
- [ ] Extract tarball and verify contents
- [ ] Move binary to `target/release/fabstir-llm-node`
- [ ] Verify version: `strings target/release/fabstir-llm-node | grep v8.13.0`
- [ ] Verify AUDIT-F4 features: `strings target/release/fabstir-llm-node | grep audit-f4`

### SDK Updates

- [ ] Update contract addresses in SDK configuration
- [ ] Download and integrate new contract ABIs
- [ ] Update `JOB_MARKETPLACE_ADDRESS` to `0x95132177F964FF053C1E874b53CF74d819618E06`
- [ ] Update `PROOF_SYSTEM_ADDRESS` to `0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31`
- [ ] Run SDK tests with new contract addresses
- [ ] Update event listeners to use new contract addresses

### Node Configuration

- [ ] Create/update `.env` file with new contract addresses
- [ ] Set `HOST_PRIVATE_KEY` (required for checkpoint submission)
- [ ] Configure `ENHANCED_S5_URL` for S5 storage
- [ ] Set `RPC_URL` for Base Sepolia
- [ ] Configure model paths (optional)
- [ ] Set up models using `./scripts/setup_models.sh` (optional)

### Deployment

- [ ] Stop old node version
- [ ] Backup old configuration files
- [ ] Start new node: `./target/release/fabstir-llm-node`
- [ ] Verify startup logs show AUDIT-F4 compliance
- [ ] Verify contract addresses in startup logs
- [ ] Check S5 backend initialization logs

### Testing

- [ ] Test basic inference request
- [ ] Test streaming inference
- [ ] Test WebSocket connection
- [ ] Test checkpoint submission (verify modelId query in logs)
- [ ] Test proof submission events from new contract
- [ ] Monitor ProofSubmitted events
- [ ] Verify settlement transactions

### Monitoring

- [ ] Monitor node logs for errors
- [ ] Check checkpoint submission logs for AUDIT-F4 signatures
- [ ] Verify S5 uploads are reaching network
- [ ] Monitor gas usage for proof submissions
- [ ] Check proof verification on-chain

---

## Troubleshooting

### Issue 1: Binary Not Found

```
Error: target/release/fabstir-llm-node: No such file or directory
```

**Solution**:
```bash
# The tarball extracts binary to root, not target/release/
tar -xzvf fabstir-llm-node-v8.13.0-audit-remediation.tar.gz
mkdir -p target/release
mv fabstir-llm-node target/release/
```

### Issue 2: Wrong Contract Address

```
[ERROR] Failed to query sessionModel: Contract call reverted
```

**Solution**: Verify you're using the new AUDIT-F4 contract addresses:
```bash
# Check .env file
grep "CONTRACT_JOB_MARKETPLACE" .env

# Expected: 0x95132177F964FF053C1E874b53CF74d819618E06
# NOT:      0x3CaCbf3f448B420918A93a88706B26Ab27a3523E
```

### Issue 3: Mock S5 Backend

```
[WARN] Using MockS5Backend - uploads won't reach S5 network!
```

**Solution**: Configure ENHANCED_S5_URL:
```bash
# Add to .env
ENHANCED_S5_URL=http://localhost:5522

# Start S5 bridge
cd services/s5-bridge
npm install
npm start
```

### Issue 4: Missing HOST_PRIVATE_KEY

```
[ERROR] Cannot submit checkpoint: HOST_PRIVATE_KEY not configured
```

**Solution**: Add host private key to `.env`:
```bash
# Add to .env
HOST_PRIVATE_KEY=0x1234...  # Your host wallet private key
```

### Issue 5: CUDA Not Found

```
[WARN] CUDA not available, using CPU inference
```

**Solution**: Verify CUDA installation:
```bash
# Check CUDA libraries
ldd target/release/fabstir-llm-node | grep cuda

# If missing, ensure you're running on a machine with CUDA
# The binary was built with CUDA support and requires GPU environment
```

### Issue 6: Old ABI Format

```
Error: Transaction reverted: function signature not found
```

**Solution**: Update contract ABIs in SDK:
```bash
# Download latest ABIs from node repository
cp node/docs/compute-contracts-reference/client-abis/*.json sdk/src/abis/
```

### Issue 7: Signature Verification Failed

```
[ERROR] Proof signature verification failed on-chain
```

**Solution**: This is likely a contract mismatch. Verify:
1. Node is using new contract addresses (0x9513...8E06)
2. SDK is listening to new contract addresses
3. No old transactions in flight
4. Node logs show "Generating 4-parameter signature (AUDIT-F4)"

### Issue 8: Version Mismatch

```
[INFO] Starting Fabstir LLM Node v8.3.13-harmony-channels
```

**Solution**: You're running the old binary:
```bash
# Verify binary version
strings target/release/fabstir-llm-node | grep "v8\."

# Expected: v8.13.0-audit-remediation
# If not, re-extract tarball and verify you moved the correct binary
```

---

## Additional Resources

### Node Documentation

- **Contract Reference**: `docs/compute-contracts-reference/PRE-REPORT-REMEDIATION-NODE.md`
- **API Reference**: `docs/compute-contracts-reference/API_REFERENCE.md`
- **Breaking Changes**: `docs/compute-contracts-reference/BREAKING_CHANGES.md`
- **Architecture**: `docs/compute-contracts-reference/ARCHITECTURE.md`

### SDK Integration Guides

- **Checkpoint Recovery**: `docs/sdk-reference/NODE_CHECKPOINT_SPEC.md`
- **Encryption**: `docs/sdk-reference/SDK_ENCRYPTION_INTEGRATION.md`
- **WebSocket API**: `docs/sdk-reference/WEBSOCKET_API_SDK_GUIDE.md`
- **S5 Storage**: `docs/sdk-reference/NODE_V8.1.2_S5_PROOF_STORAGE.md`
- **Web Search**: `docs/sdk-reference/SDK_WEB_SEARCH_INTEGRATION.md`

### Contract ABIs

Located in: `docs/compute-contracts-reference/client-abis/`
- `JobMarketplaceWithModels-CLIENT-ABI.json` (AUDIT-F4 compliant)
- `ProofSystemAuditF4-CLIENT-ABI.json` (AUDIT-F4 compliant)
- `NodeRegistryWithModels-CLIENT-ABI.json` (unchanged)
- `ModelRegistryWithModels-CLIENT-ABI.json` (unchanged)
- `HostEarnings-CLIENT-ABI.json` (unchanged)

---

## Support

If you encounter issues not covered in this guide:

1. **Check Node Logs**: Look for ERROR or WARN messages
2. **Verify Configuration**: Ensure `.env` file has correct contract addresses
3. **Test with curl**: Verify basic API functionality before SDK integration
4. **Check Contract Events**: Monitor on-chain events to verify transactions
5. **Contact Fabstir Team**: Report issues with logs and configuration details

---

## Summary

**What SDK Developers Must Do**:
1. ✅ Extract and deploy new binary
2. ✅ Update contract addresses in SDK
3. ✅ Update contract ABIs in SDK
4. ✅ Configure `.env` with new addresses
5. ✅ Test inference and checkpoint submission

**What Doesn't Change**:
- ❌ Inference API (HTTP/WebSocket)
- ❌ Checkpoint recovery API
- ❌ Encryption protocol
- ❌ Event structures
- ❌ Session creation flow

**Key Takeaway**: The node handles AUDIT-F4 compliance internally. SDK developers only need to update contract addresses and ABIs - no protocol or API changes required.

---

**Document Version**: 1.0
**Last Updated**: 2026-02-01
**Next Review**: After AUDIT final report
