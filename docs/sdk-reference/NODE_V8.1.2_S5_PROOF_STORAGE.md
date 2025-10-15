# Node v8.1.2 - S5 Off-Chain Proof Storage Integration

**Release Date**: October 15, 2025
**Version**: v8.1.2-proof-s5-storage
**Tarball**: `fabstir-llm-node-v8.1.2-proof-s5-storage.tar.gz`
**SHA256**: `31b4b28eb07fa761d8edba9af075f4fc230b5e5d47bdc3432a71c29feb23da9f`

---

## 🎯 What's New in v8.1.2

### Major Change: Off-Chain Proof Storage with S5

**Problem Solved**: STARK proofs are ~221KB, causing RPC transaction size limit issues when submitting on-chain.

**Solution**: Proofs are now stored in the S5 decentralized storage network, with only a hash (32 bytes) and CID (Content Identifier) submitted on-chain.

**Size Reduction**: **737x smaller** - from 221,466 bytes to ~300 bytes per checkpoint transaction

---

## 🏗️ Architecture Changes

### Before v8.1.2 (Full Proof On-Chain)
```
Node → Generate 221KB Proof → Submit to Blockchain ❌ (RPC limit: 128KB)
```

### After v8.1.2 (Hash + CID On-Chain)
```
Node → Generate 221KB Proof → Upload to S5 → Get CID
     → Calculate SHA256 Hash
     → Submit Hash + CID to Blockchain ✅ (~300 bytes)
```

### Data Flow
1. **Proof Generation**: Node generates STARK proof using Risc0 zkVM (221KB)
2. **S5 Upload**: Proof uploaded to S5 decentralized storage network
3. **CID Generation**: S5 returns Content Identifier (content-addressed, deterministic)
4. **Hash Calculation**: SHA256 hash of proof calculated (32 bytes)
5. **On-Chain Submission**: Only hash + CID submitted via `submitProofOfWork(jobId, tokensClaimed, proofHash, proofCID)`
6. **Verification**: Anyone can retrieve proof from S5 using CID and verify hash matches

---

## 📝 Contract Changes

### New Contract Address
- **JobMarketplace**: `0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E` (Base Sepolia)
- **Deployment Date**: August 26, 2025

### Updated Function Signature
```solidity
// OLD (v8.1.1 and earlier)
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes calldata proof  // ❌ 221KB - too large!
) external;

// NEW (v8.1.2+)
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,    // ✅ 32 bytes - SHA256 hash
    string calldata proofCID  // ✅ ~50-100 chars - S5 CID
) external;
```

### Event Changes
```solidity
// NEW Event
event ProofSubmitted(
    uint256 indexed jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    string proofCID,
    uint256 timestamp
);
```

---

## 🧪 Testing with SDK

### Prerequisites
1. **Node Binary**: Extract `fabstir-llm-node-v8.1.2-proof-s5-storage.tar.gz`
2. **Host Registration**: Use SDK test UI to register host on Base Sepolia
3. **Test Account**: Ensure TEST_HOST_1 has funds on Base Sepolia
4. **Model**: TinyLlama or other GGUF model for testing

### Environment Setup
```bash
# Host credentials (from .env.local.test in node repo)
export HOST_PRIVATE_KEY="0xe7855c0ea54ccca55126d40f97d90868b2a73bad0363e92ccdec0c4fbd6c0ce2"
export HOST_ADDRESS="0x4594F755F593B517Bb3194F4DeC20C48a3f04504"

# Node configuration
export MODEL_PATH="./models/tiny-vicuna-1b.q4_k_m.gguf"
export RUST_LOG="info"

# Start node
./fabstir-llm-node
```

### Test Flow with SDK Test UI

#### 1. Host Registration
- Navigate to SDK "Register Host" test page
- Register `TEST_HOST_1_ADDRESS` with minimum pricing
- Verify registration on-chain

#### 2. Start Production Node
- Use "Start/Stop Node" test page
- Start node with v8.1.2 binary
- Monitor health check endpoint
- Verify node appears as "available" in registry

#### 3. Create Test Session
- Navigate to "Create Session" test page
- Create session with:
  - Model: Match what node is running
  - Max tokens: **100+** (checkpoint triggers every 50 tokens)
  - Payment: Sufficient for test
- Note the `jobId` returned

#### 4. Stream Inference
- Navigate to "WebSocket Streaming" test page
- Connect to node via WebSocket
- Send inference request with **100+ token generation**
- Watch for streaming responses

#### 5. Monitor Checkpoint Submission
**In Node Logs** (critical events to watch):
```
🔐 Generating real Risc0 STARK proof for job 12345
   ↳ Proof size: 221466 bytes

📤 Uploading proof to S5 for job 12345
   ↳ Upload path: home/proofs/job_12345_proof.bin

✅ Proof uploaded to S5: CID=s5://bafybeig...
   ↳ Upload successful, proof retrievable

📊 Proof hash: 0x7f3d2b1a9c8e4f5d6a7b3c1e9f8d4a2b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f
   ↳ SHA256 hash calculated

📦 Encoding checkpoint transaction:
   ↳ jobId: 12345
   ↳ tokensClaimed: 150
   ↳ proofHash: 0x7f3d2b1a...
   ↳ proofCID: s5://bafybeig...
   ↳ Transaction size: 287 bytes ✅ (vs 221466 bytes)

🚀 Submitting checkpoint transaction...
   ↳ From: 0x4594F755F593B517Bb3194F4DeC20C48a3f04504
   ↳ To: 0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E (JobMarketplace)
   ↳ Function: submitProofOfWork(uint256,uint256,bytes32,string)

✅ Checkpoint submitted: tx_hash=0xabc123def456...
   ↳ Base Sepolia transaction confirmed
```

---

## 🔍 Verification Steps

### 1. Verify Transaction on BaseScan
```
https://sepolia.basescan.org/tx/0x[TX_HASH]
```

**Check for**:
- ✅ Transaction size < 1KB (should be ~300 bytes)
- ✅ Input data contains:
  - Function selector for `submitProofOfWork`
  - `jobId` parameter
  - `tokensClaimed` parameter
  - `proofHash` (32 bytes hex)
  - `proofCID` (string starting with "s5://")
- ✅ Status: Success
- ✅ No "transaction data too large" errors

### 2. Verify Event Emission
In transaction logs, look for `ProofSubmitted` event:
```
Event: ProofSubmitted(uint256,uint256,bytes32,string)
  jobId: 12345
  tokensClaimed: 150
  proofHash: 0x7f3d2b1a9c8e4f5d6a7b3c1e9f8d4a2b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f
  proofCID: "s5://bafybeig..."
  timestamp: 1728950400
```

### 3. Verify Proof Storage
**S5 CID Format**: `s5://[base58-encoded-hash]`

**Properties**:
- Content-addressed: Same proof → same CID
- Deterministic: Re-uploading identical proof returns same CID
- Retrievable: Anyone can fetch proof using CID
- Decentralized: Stored on S5 network, not centralized server

**Verification**:
- Different jobs should have different CIDs
- Same proof re-uploaded should return identical CID
- CID should start with `s5://`

---

## 🎨 SDK UI Integration Recommendations

### Display Checkpoint Status
Show in job/session details:
```
Checkpoint Status:
  ├─ Tokens Generated: 150 / 500
  ├─ Checkpoints Submitted: 3
  ├─ Last Checkpoint:
  │   ├─ Tokens: 150
  │   ├─ Proof Hash: 0x7f3d2b1a...
  │   ├─ Proof CID: s5://bafybeig...
  │   ├─ Transaction: 0xabc123... ✅
  │   └─ Size: 287 bytes (737x reduction)
  └─ Next Checkpoint: 200 tokens
```

### Transaction Details Page
```
Transaction Type: Checkpoint Submission
Job ID: 12345
Tokens Claimed: 150

Proof Storage:
  ├─ Storage Type: S5 Decentralized Network
  ├─ Proof Size: 221,466 bytes (216 KB)
  ├─ Proof Hash: 0x7f3d2b1a9c8e4f5d6a7b3c1e9f8d4a2b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f
  ├─ Proof CID: s5://bafybeighqkjvxl7tdqwm3qf4n2b5wz8x9c0d1e2f3g4h5i6j7k8l9m0n1o2p3
  └─ Verify Proof: [Link to S5 explorer]

On-Chain Data:
  ├─ Transaction Hash: 0xabc123def456...
  ├─ Block: 12345678
  ├─ Transaction Size: 287 bytes ✅
  ├─ Gas Used: ~150,000
  └─ Status: Confirmed ✅
```

### Health Check Integration
Add to node health check response:
```json
{
  "status": "healthy",
  "version": "v8.1.2-proof-s5-storage",
  "features": {
    "proof_storage": "s5",
    "proof_size_reduction": "737x",
    "risc0_zkvm": "v3.0",
    "contract_version": "v8.1.2-hash-cid"
  },
  "checkpoint_stats": {
    "total_submitted": 42,
    "average_tx_size_bytes": 295,
    "average_proof_size_kb": 216
  }
}
```

---

## 📊 Key Metrics to Track

### Performance Metrics
- **Proof Generation Time**: 0.2-2.3s (GPU-accelerated with CUDA)
- **S5 Upload Time**: ~1-3s (depends on network)
- **Transaction Size**: 200-300 bytes (vs 221KB before)
- **Gas Savings**: Significantly lower due to smaller transaction data
- **Size Reduction**: 737-1129x smaller than full proof

### Success Indicators
- ✅ All checkpoint transactions < 1KB
- ✅ No RPC "transaction too large" errors
- ✅ Proof CIDs retrievable from S5
- ✅ Hash verification passes
- ✅ Events emitted correctly

### Failure Indicators
- ❌ Transaction size > 1KB
- ❌ RPC rejects transaction (too large)
- ❌ Missing CID in event
- ❌ Hash mismatch on verification
- ❌ S5 upload failures

---

## 🐛 Troubleshooting

### Issue: Checkpoint Not Submitted
**Symptoms**: Job completes but no checkpoint transaction

**Check**:
1. Node logs show "🔐 Generating proof"?
2. Node logs show "📤 Uploading to S5"?
3. Node has HOST_PRIVATE_KEY set?
4. Host has sufficient ETH for gas on Base Sepolia?

**Solution**: Verify environment variables and wallet funding

### Issue: Transaction Rejected
**Symptoms**: Transaction fails or rejected by RPC

**Check**:
1. Transaction size in logs (should be ~300 bytes)
2. Contract address correct? (`0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E`)
3. Function signature matches new contract?

**Solution**: Ensure using v8.1.2 node with new contract

### Issue: CID Not Found
**Symptoms**: Event shows CID but S5 retrieval fails

**Check**:
1. CID format correct? (starts with `s5://`)
2. S5 network accessible?
3. Proof was actually uploaded? (check node logs)

**Solution**: Verify S5 network status and node logs

---

## 🔄 Migration from v8.1.1

### Breaking Changes
1. **Contract Address Changed**: Must use `0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E`
2. **Function Signature Changed**: `submitProofOfWork` now takes hash+CID, not full proof
3. **Event Schema Changed**: `ProofSubmitted` event has new parameters

### SDK Updates Required
- Update contract ABI to v8.1.2
- Update JobMarketplace contract address
- Update event listeners for new `ProofSubmitted` event schema
- Add CID display in UI
- Add proof retrieval link to S5 explorer (if available)

### Backward Compatibility
- ❌ Old nodes (v8.1.1) **cannot** submit to new contract
- ❌ New nodes (v8.1.2) **cannot** submit to old contract
- ✅ All hosts must upgrade to v8.1.2+ to participate

---

## 📦 Deployment Checklist

### For Node Operators
- [ ] Extract v8.1.2 tarball
- [ ] Verify SHA256: `31b4b28eb07fa761d8edba9af075f4fc230b5e5d47bdc3432a71c29feb23da9f`
- [ ] Set HOST_PRIVATE_KEY environment variable
- [ ] Ensure CUDA drivers installed (for GPU acceleration)
- [ ] Verify sufficient disk space for S5 proof storage
- [ ] Test with small job first (100 tokens)
- [ ] Monitor logs for S5 upload success
- [ ] Verify checkpoint transaction on BaseScan

### For SDK Developers
- [ ] Update JobMarketplace ABI to v8.1.2
- [ ] Update contract address in SDK config
- [ ] Update event listeners for ProofSubmitted schema
- [ ] Add CID display to UI
- [ ] Add transaction size metrics
- [ ] Test with SDK test UI pages
- [ ] Verify BaseScan integration shows correct data
- [ ] Update documentation for users

---

## 📚 Additional Resources

- **Node Implementation**: `docs/IMPLEMENTATION-RISC0-2.md`
- **Contract Reference**: `docs/compute-contracts-reference/JobMarketplace.md`
- **S5 Integration Guide**: `docs/compute-contracts-reference/S5_NODE_INTEGRATION_GUIDE.md`
- **Checkpoint Guide**: `docs/CHECKPOINT_IMPLEMENTATION_GUIDE.md`
- **Contract ABI**: `docs/compute-contracts-reference/client-abis/JobMarketplaceWithModels-CLIENT-ABI-v2.json`

---

## 🎯 Test Scenarios

### Scenario 1: Basic Checkpoint Flow
1. Register host via SDK
2. Start node v8.1.2
3. Create session with 100 tokens
4. Monitor checkpoint at 50 tokens
5. Verify transaction on BaseScan
6. Check proof CID in event

**Expected**: Transaction size ~300 bytes, proof stored in S5

### Scenario 2: Multiple Checkpoints
1. Create session with 250 tokens
2. Monitor checkpoints at 50, 100, 150, 200 tokens
3. Verify each has unique CID
4. Verify all transactions < 1KB

**Expected**: 5 checkpoints with different CIDs, all successful

### Scenario 3: Proof Verification
1. Submit checkpoint
2. Extract CID from event
3. Retrieve proof from S5 using CID
4. Calculate SHA256 hash
5. Compare with on-chain hash

**Expected**: Hashes match, proving proof integrity

---

## 📞 Support

For issues with v8.1.2 deployment:
- Check node logs for detailed error messages
- Verify contract addresses match deployment
- Ensure using correct ABI version
- Test with SDK test UI before production

**Node Version Verification**:
```bash
./fabstir-llm-node --version
# Expected: v8.1.2-proof-s5-storage-2025-10-15
```

---

**Last Updated**: October 15, 2025
**Node Version**: v8.1.2-proof-s5-storage
**Contract Version**: v8.1.2-hash-cid
**Network**: Base Sepolia (84532)
