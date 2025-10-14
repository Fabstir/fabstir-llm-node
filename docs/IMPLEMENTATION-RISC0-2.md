# IMPLEMENTATION-RISC0-2.md - Off-Chain Proof Storage with S5

## Overview
Implementation plan for fixing RPC transaction size limit by storing full STARK proofs off-chain in S5 and submitting only proof hash + CID on-chain.

**Timeline**: 4-6 hours total (node) + 2-3 hours (contracts)
**Location**: `fabstir-llm-node/` (Rust project) + smart contracts
**Approach**: Proper architecture - contracts accept hash+CID, not full proof
**Proof Storage**: S5 decentralized storage (221KB STARK proofs)

---

## Problem Statement

**Discovered**: 2025-10-14 during v8.1.1 deployment

**Issue**: RPC endpoint rejects transactions with STARK proofs
```
❌ Transaction failed: (code: -32000, message: oversized data: transaction size 221715, limit 131072)
```

**Root Cause**:
- STARK proofs are ~221KB (216.28 KB measured)
- RPC transaction limit is 128KB (131,072 bytes)
- Current code attempts to submit full proof in `submitProofOfWork(uint256 jobId, uint256 tokensClaimed, bytes proof)`

**Impact**:
- ✅ Proof generation working perfectly (v8.1.1)
- ✅ GPU acceleration functional
- ❌ Cannot submit proofs to blockchain
- ❌ Checkpoints failing with oversized data error

---

## Solution Architecture

### Correct Approach: Off-Chain Storage

**What needs to change:**

1. **Smart Contract** (`JobMarketplace.sol`):
   ```solidity
   // OLD (current)
   function submitProofOfWork(uint256 jobId, uint256 tokensClaimed, bytes calldata proof)

   // NEW (required)
   function submitProofOfWork(
       uint256 jobId,
       uint256 tokensClaimed,
       bytes32 proofHash,      // 32 bytes - SHA256 of proof
       string calldata proofCID // S5 CID for retrieval
   )
   ```

2. **Node** (`checkpoint_manager.rs`):
   - Generate STARK proof (221KB) ✅ Already working
   - Upload proof to S5 → get CID
   - Calculate SHA256 hash of proof
   - Submit only hash (32 bytes) + CID (string) to blockchain

3. **Verification Flow**:
   - Disputant fetches proof from S5 using CID
   - Verifies SHA256 hash matches on-chain hash
   - Verifies STARK proof cryptographically using Risc0

**Transaction Size**:
- Proof hash: 32 bytes
- CID string: ~50-100 bytes
- Function call overhead: ~100 bytes
- **Total**: ~200-300 bytes (well under 128KB limit)

---

## Implementation Status

### ✅ Prerequisites Complete
- ✅ STARK proof generation working (v8.1.1)
- ✅ GPU acceleration functional (RTX 4090: 280ms per proof)
- ✅ S5 client infrastructure exists (`src/storage/s5_client.rs`)
- ✅ Proof size measured: 221,466 bytes (216.28 KB)

### ⏳ In Progress
- [ ] Phase 1: Contract Updates (Contracts Developer)
- [ ] Phase 2: Node S5 Integration
- [ ] Phase 3: Testing and Deployment

---

## Implementation Phases

### Phase 1: Smart Contract Updates (Contracts Developer)
**Timeline**: 2-3 hours
**Owner**: Contracts developer
**Goal**: Update JobMarketplace to accept proof hash + CID instead of full proof

### Phase 2: Node S5 Integration (Node Developer)
**Timeline**: 3-4 hours
**Owner**: Node developer (Claude)
**Goal**: Integrate S5 upload and modify checkpoint submission to send hash+CID

### Phase 3: Testing and Deployment
**Timeline**: 1-2 hours
**Goal**: End-to-end validation with real proofs on testnet

---

## Phase 1: Smart Contract Updates

**Timeline**: 2-3 hours
**Prerequisites**: None
**Goal**: Update contract to accept proof hash and S5 CID

### Sub-phase 1.1: Update Contract Signature

**Goal**: Modify `submitProofOfWork` function signature

#### Tasks

**Step 1: Update Function Signature**
- [ ] Modify `JobMarketplaceWithModels.sol`
- [ ] Change signature from `bytes calldata proof` to `bytes32 proofHash, string calldata proofCID`
- [ ] Update internal storage if needed (store both hash and CID)
- [ ] Update `ProofSubmitted` event to include CID

**Expected Changes**:
```solidity
// contracts/JobMarketplaceWithModels.sol

function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,      // NEW: Just the hash
    string calldata proofCID // NEW: S5 CID for retrieval
) external nonReentrant {
    SessionJob storage job = sessionJobs[jobId];

    // ... existing validation logic ...

    // Store proof hash and CID
    job.lastProofHash = proofHash;
    job.lastProofCID = proofCID;
    job.lastProofTime = block.timestamp;

    // ... existing token tracking and payment logic ...

    emit ProofSubmitted(jobId, msg.sender, tokensClaimed, proofHash, proofCID);
}
```

**Step 2: Update Storage**
- [ ] Add `bytes32 lastProofHash` to SessionJob struct
- [ ] Add `string lastProofCID` to SessionJob struct
- [ ] Verify storage layout compatibility

**Step 3: Update Events**
- [ ] Modify `ProofSubmitted` event: add `string proofCID` parameter
- [ ] Ensure events emit both hash and CID for off-chain indexing

#### Success Criteria
- [ ] Contract compiles without errors
- [ ] Function signature accepts hash (32 bytes) + CID (string)
- [ ] Events include both hash and CID
- [ ] Storage fields added to SessionJob struct

#### Files Modified
- [ ] `contracts/JobMarketplaceWithModels.sol` - Function signature, storage, events
- [ ] `contracts/IJobMarketplace.sol` - Interface update (if exists)

#### Estimated Time
**~1.5-2 hours** (including testing compilation)

---

### Sub-phase 1.2: Generate and Deploy New ABI

**Goal**: Create new ABI and deploy updated contract to Base Sepolia

#### Tasks

**Step 1: Generate ABI**
- [ ] Compile contract with Hardhat/Foundry
- [ ] Export ABI JSON to `docs/compute-contracts-reference/client-abis/JobMarketplaceWithModels-CLIENT-ABI-v2.json`
- [ ] Verify ABI includes updated `submitProofOfWork` signature

**Step 2: Deploy to Base Sepolia**
- [ ] Deploy updated contract to Base Sepolia testnet
- [ ] Record new contract address
- [ ] Update `.env.contracts` with new address
- [ ] Update `docs/compute-contracts-reference/JobMarketplace.md` with deployment info

**Step 3: Verify Deployment**
- [ ] Verify contract on BaseScan
- [ ] Test contract interaction via Etherscan UI
- [ ] Confirm storage layout correct

#### Success Criteria
- [ ] New ABI generated and saved
- [ ] Contract deployed to Base Sepolia
- [ ] Contract verified on BaseScan
- [ ] New address documented in `.env.contracts`

#### Files Modified
- [ ] `docs/compute-contracts-reference/client-abis/JobMarketplaceWithModels-CLIENT-ABI-v2.json` - New ABI
- [ ] `.env.contracts` - New JOB_MARKETPLACE_FAB_WITH_S5_ADDRESS
- [ ] `docs/compute-contracts-reference/JobMarketplace.md` - Deployment history

#### Estimated Time
**~1-1.5 hours** (deployment and verification)

---

## Phase 2: Node S5 Integration

**Timeline**: 3-4 hours
**Prerequisites**: Phase 1 complete (new contract deployed)
**Goal**: Integrate S5 proof upload and modify checkpoint submission

### Sub-phase 2.1: Add S5 Client to CheckpointManager

**Goal**: Integrate S5Storage client into CheckpointManager for proof uploads

#### Tasks

**Step 1: Add S5 Client Field**
- [ ] Modify `CheckpointManager` struct in `src/contracts/checkpoint_manager.rs`
- [ ] Add `s5_client: Arc<Box<dyn S5Storage>>` field
- [ ] Update constructor to initialize S5 client from environment

**Expected Changes**:
```rust
// src/contracts/checkpoint_manager.rs

use crate::storage::s5_client::S5Client;

pub struct CheckpointManager {
    web3_client: Arc<Web3Client>,
    job_trackers: Arc<RwLock<HashMap<u64, JobTokenTracker>>>,
    proof_system_address: Address,
    host_address: Address,
    s5_client: Arc<Box<dyn S5Storage>>, // NEW
}

impl CheckpointManager {
    pub fn new(web3_client: Arc<Web3Client>) -> Result<Self> {
        // ... existing code ...

        // Initialize S5 client
        let s5_client = Arc::new(
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(S5Client::create_from_env())
            })?
        );

        Ok(Self {
            web3_client,
            job_trackers: Arc::new(RwLock::new(HashMap::new())),
            proof_system_address,
            host_address,
            s5_client, // NEW
        })
    }
}
```

**Step 2: Update Tests**
- [ ] Add S5 mock to test fixtures
- [ ] Verify CheckpointManager initialization with S5 client

#### Success Criteria
- [ ] CheckpointManager has S5 client field
- [ ] S5 client initializes from environment (`ENHANCED_S5_URL` or mock)
- [ ] Tests compile and pass with new field

#### Files Modified
- [ ] `src/contracts/checkpoint_manager.rs` - Add field, update constructor

#### Estimated Time
**~30 minutes**

---

### Sub-phase 2.2: Implement Proof Upload to S5

**Goal**: Upload generated STARK proof to S5 and get CID

#### Tasks

**Step 1: Add Proof Upload Method**
- [ ] Create `upload_proof_to_s5()` method in CheckpointManager
- [ ] Generate unique S5 path: `home/proofs/job_{jobId}_ts_{timestamp}.proof`
- [ ] Handle upload errors with retry logic

**Expected Implementation**:
```rust
// src/contracts/checkpoint_manager.rs

impl CheckpointManager {
    async fn upload_proof_to_s5(
        &self,
        job_id: u64,
        proof_bytes: &[u8],
    ) -> Result<String> {
        let timestamp = chrono::Utc::now().timestamp();
        let proof_path = format!("home/proofs/job_{}_ts_{}.proof", job_id, timestamp);

        info!("📤 Uploading proof to S5: {} ({} bytes)", proof_path, proof_bytes.len());

        let cid = self.s5_client
            .put(&proof_path, proof_bytes.to_vec())
            .await
            .map_err(|e| anyhow!("S5 upload failed: {}", e))?;

        info!("✅ Proof uploaded to S5: CID={}", cid);
        Ok(cid)
    }
}
```

**Step 2: Integrate Upload into generate_proof()**
- [ ] Modify `generate_proof()` method to return `(Vec<u8>, String)` (proof bytes + CID)
- [ ] Upload proof after generation
- [ ] Return both proof bytes and CID

**Step 3: Add Error Handling**
- [ ] Handle S5 upload failures gracefully
- [ ] Add retry logic (max 3 retries)
- [ ] Log upload status and CID

#### Success Criteria
- [ ] Proof uploads to S5 successfully
- [ ] CID returned from upload
- [ ] Proof retrievable from S5 by CID
- [ ] Error handling for upload failures

#### Files Modified
- [ ] `src/contracts/checkpoint_manager.rs` - Add upload method, integrate into generate_proof()

#### Estimated Time
**~1 hour**

---

### Sub-phase 2.3: Update Contract Call to Submit Hash+CID

**Goal**: Modify `encode_checkpoint_call()` to use new contract signature

#### Tasks

**Step 1: Calculate Proof Hash**
- [ ] Add SHA256 hash calculation in `submit_checkpoint()`
- [ ] Hash the proof bytes before upload

**Expected Code**:
```rust
// src/contracts/checkpoint_manager.rs

async fn submit_checkpoint(&self, job_id: u64, tokens_generated: u64) -> Result<()> {
    let tokens_to_submit = tokens_generated;

    info!("Submitting proof of work for job {} with {} tokens...", job_id, tokens_to_submit);

    // Generate STARK proof
    let proof_bytes = self.generate_proof(job_id, tokens_generated)?;

    // Calculate proof hash
    let proof_hash = Sha256::digest(&proof_bytes);
    let proof_hash_bytes: [u8; 32] = proof_hash.into();

    // Upload proof to S5
    let proof_cid = self.upload_proof_to_s5(job_id, &proof_bytes).await?;

    info!("📊 Proof hash: 0x{}", hex::encode(&proof_hash_bytes));
    info!("📦 Proof CID: {}", proof_cid);

    // Encode contract call with hash + CID (NOT full proof)
    let data = encode_checkpoint_call(job_id, tokens_to_submit, proof_hash_bytes, proof_cid);

    // Send transaction...
}
```

**Step 2: Update encode_checkpoint_call()**
- [ ] Modify function signature to accept `proof_hash: [u8; 32]` and `proof_cid: String`
- [ ] Update ABI encoding to match new contract signature

**Expected Implementation**:
```rust
// src/contracts/checkpoint_manager.rs

fn encode_checkpoint_call(
    job_id: u64,
    tokens_generated: u64,
    proof_hash: [u8; 32],
    proof_cid: String,
) -> Vec<u8> {
    use ethers::abi::Function;

    let function = Function {
        name: "submitProofOfWork".to_string(),
        inputs: vec![
            ethers::abi::Param {
                name: "jobId".to_string(),
                kind: ethers::abi::ParamType::Uint(256),
                internal_type: None,
            },
            ethers::abi::Param {
                name: "tokensClaimed".to_string(),
                kind: ethers::abi::ParamType::Uint(256),
                internal_type: None,
            },
            ethers::abi::Param {
                name: "proofHash".to_string(),
                kind: ethers::abi::ParamType::FixedBytes(32),  // NEW: bytes32
                internal_type: None,
            },
            ethers::abi::Param {
                name: "proofCID".to_string(),
                kind: ethers::abi::ParamType::String,  // NEW: string
                internal_type: None,
            },
        ],
        outputs: vec![],
        constant: None,
        state_mutability: ethers::abi::StateMutability::NonPayable,
    };

    let tokens = vec![
        Token::Uint(U256::from(job_id)),
        Token::Uint(U256::from(tokens_generated)),
        Token::FixedBytes(proof_hash.to_vec()),  // NEW
        Token::String(proof_cid),                // NEW
    ];

    function.encode_input(&tokens).unwrap()
}
```

**Step 3: Update Contract Address**
- [ ] Read new contract address from `.env.contracts`
- [ ] Update `CONTRACT_JOB_MARKETPLACE` environment variable
- [ ] Verify address loaded correctly at startup

#### Success Criteria
- [ ] Proof hash calculated correctly (32 bytes)
- [ ] Proof uploaded to S5
- [ ] CID obtained from S5
- [ ] Transaction encodes hash + CID (not full proof)
- [ ] Transaction size < 1KB (well under 128KB limit)

#### Files Modified
- [ ] `src/contracts/checkpoint_manager.rs` - Hash calculation, upload integration, encoding update
- [ ] `.env.contracts` - New contract address (already done in Phase 1.2)

#### Estimated Time
**~1.5 hours**

---

### Sub-phase 2.4: Update Version and Build

**Goal**: Increment version to v8.1.2 and rebuild with S5 integration

#### Tasks

**Step 1: Update Version**
- [ ] Update `/workspace/VERSION` to `8.1.2-proof-s5-storage`
- [ ] Update `/workspace/src/version.rs` constants:
  - VERSION → `"v8.1.2-proof-s5-storage-2025-10-14"`
  - VERSION_NUMBER → `"8.1.2"`
  - VERSION_PATCH → `2`
  - BREAKING_CHANGES → Add note about S5 proof storage
  - Test assertions → Check for `8.1.2`

**Step 2: Build and Verify**
- [ ] Build release binary: `RUSTFLAGS="-C target-cpu=native" cargo build --release --features real-ezkl`
- [ ] Verify version in binary: `strings target/release/fabstir-llm-node | grep "v8.1.2"`
- [ ] Verify CUDA linkage: `ldd target/release/fabstir-llm-node | grep cuda`

**Step 3: Create Tarball**
- [ ] Create tarball: `tar -czf fabstir-llm-node-v8.1.2-proof-s5-storage.tar.gz -C target/release fabstir-llm-node`
- [ ] Generate checksum: `sha256sum fabstir-llm-node-v8.1.2-proof-s5-storage.tar.gz > fabstir-llm-node-v8.1.2-proof-s5-storage.tar.gz.sha256`
- [ ] Verify tarball extracts correctly

#### Success Criteria
- [ ] Version updated to v8.1.2 in all files
- [ ] Binary compiles successfully
- [ ] Version embedded in binary
- [ ] Tarball created and verified

#### Files Modified
- [ ] `/workspace/VERSION` - Update version
- [ ] `/workspace/src/version.rs` - Update all constants
- [ ] Create tarball files

#### Estimated Time
**~30 minutes** (plus build time ~45 minutes)

---

## Phase 3: Testing and Deployment

**Timeline**: 1-2 hours
**Prerequisites**: Phases 1 and 2 complete
**Goal**: End-to-end validation with real proofs on Base Sepolia testnet

### Sub-phase 3.1: Local Testing

**Goal**: Test S5 upload and hash calculation locally

#### Tasks

**Step 1: Test S5 Upload**
- [ ] Create test for `upload_proof_to_s5()` method
- [ ] Verify proof uploads successfully
- [ ] Verify CID returned
- [ ] Verify proof retrievable by CID

**Step 2: Test Transaction Encoding**
- [ ] Create test for `encode_checkpoint_call()` with hash+CID
- [ ] Verify encoded transaction size < 1KB
- [ ] Verify function selector correct
- [ ] Verify parameters encoded correctly

**Step 3: Integration Test**
- [ ] Test full checkpoint flow with mock S5
- [ ] Verify proof generation → S5 upload → hash calculation → encoding
- [ ] Verify transaction would fit within RPC limits

#### Success Criteria
- [ ] S5 upload tests pass
- [ ] Transaction encoding tests pass
- [ ] Integration test passes
- [ ] Transaction size verified < 1KB

#### Estimated Time
**~1 hour**

---

### Sub-phase 3.2: Testnet Deployment and Validation

**Goal**: Deploy to production and verify proof submissions work

#### Tasks

**Step 1: Update Environment**
- [ ] Set `CONTRACT_JOB_MARKETPLACE` to new contract address
- [ ] Verify `ENHANCED_S5_URL` configured (or using mock)
- [ ] Verify `HOST_PRIVATE_KEY` set

**Step 2: Deploy Node**
- [ ] Extract v8.1.2 tarball
- [ ] Update Docker container with new binary
- [ ] Restart node
- [ ] Verify logs show S5 client initialized

**Step 3: Test Checkpoint Submission**
- [ ] Create test session job
- [ ] Generate 100+ tokens to trigger checkpoint
- [ ] Monitor logs for:
  - `🔐 Generating real Risc0 STARK proof`
  - `📤 Uploading proof to S5`
  - `✅ Proof uploaded to S5: CID=...`
  - `📊 Proof hash: 0x...`
  - Transaction success

**Step 4: Verify On-Chain**
- [ ] Check transaction on BaseScan
- [ ] Verify input data size < 1KB (not 221KB)
- [ ] Verify `ProofSubmitted` event contains hash and CID
- [ ] Query contract to verify hash stored correctly

**Step 5: Test Proof Retrieval**
- [ ] Fetch proof from S5 using CID from event
- [ ] Calculate SHA256 hash of fetched proof
- [ ] Verify hash matches on-chain proof hash
- [ ] Verify proof verifies with Risc0 (offline)

#### Success Criteria
- [ ] Node starts successfully with v8.1.2
- [ ] Checkpoint submission succeeds
- [ ] Transaction size < 1KB
- [ ] Proof hash and CID emitted in event
- [ ] Proof retrievable from S5
- [ ] Hash verification passes
- [ ] No RPC size limit errors

#### Estimated Time
**~1 hour**

---

## Success Criteria (Overall)

### Phase 1: Contract Updates
- [x] Contract accepts `bytes32 proofHash` and `string proofCID` parameters
- [x] Contract deployed to Base Sepolia
- [x] New ABI generated and documented

### Phase 2: Node S5 Integration
- [ ] S5 client integrated into CheckpointManager
- [ ] Proof uploads to S5 successfully
- [ ] Transaction encodes hash + CID
- [ ] Transaction size < 1KB (fits RPC limit)
- [ ] Version updated to v8.1.2

### Phase 3: Testing
- [ ] Local tests pass
- [ ] Testnet checkpoint submission succeeds
- [ ] No RPC size errors
- [ ] Proof retrievable and verifiable

---

## Files Modified Summary

### Phase 1: Contract Updates (Contracts Developer)
- [ ] `contracts/JobMarketplaceWithModels.sol` - Function signature, storage, events
- [ ] `docs/compute-contracts-reference/client-abis/JobMarketplaceWithModels-CLIENT-ABI-v2.json` - New ABI
- [ ] `.env.contracts` - New contract address
- [ ] `docs/compute-contracts-reference/JobMarketplace.md` - Deployment docs

### Phase 2: Node S5 Integration (Node Developer)
- [ ] `src/contracts/checkpoint_manager.rs` - S5 integration, upload method, encoding update
- [ ] `/workspace/VERSION` - Version bump
- [ ] `/workspace/src/version.rs` - Version constants

### Phase 3: Testing
- [ ] Test files as needed

---

## Rollback Plan

If issues arise during deployment:

1. **Contract Rollback**: Revert to previous contract address in `.env.contracts`
2. **Node Rollback**: Deploy v8.1.1 tarball (still generates proofs, but fails submission)
3. **S5 Issues**: Node falls back to mock S5 if `ENHANCED_S5_URL` not set

---

## Notes and Considerations

### S5 Path Convention
- Proof path: `home/proofs/job_{jobId}_ts_{timestamp}.proof`
- Ensures unique paths per checkpoint
- Easy to locate by job ID

### CID Format
- S5 returns CID like `s5://abc123...`
- Store as string in contract
- Use CID to fetch proof: `s5_client.get_by_cid(cid)`

### Transaction Size
- Proof hash: 32 bytes
- CID string: ~50-100 bytes
- Function selector + params: ~100-200 bytes
- **Total**: ~200-400 bytes (428x smaller than 221KB proof!)

### Verification Flow
1. Get job proof hash from contract
2. Get job proof CID from contract events
3. Fetch proof from S5: `s5_client.get_by_cid(cid)`
4. Calculate hash: `sha256(proof_bytes)`
5. Compare: `calculated_hash == on_chain_hash`
6. Verify proof: `risc0::verify(proof, public_inputs)`

### Security Considerations
- ✅ Proof integrity: SHA256 hash prevents tampering
- ✅ Proof availability: S5 decentralized storage (redundant)
- ✅ Proof verifiability: Anyone can fetch and verify
- ⚠️ S5 dependency: If S5 down, proofs temporarily unavailable (but hash still on-chain)

---

## Timeline Summary

| Phase | Duration | Owner |
|-------|----------|-------|
| **Phase 1: Contract Updates** | 2-3 hours | Contracts Developer |
| Sub-phase 1.1: Update Signature | 1.5-2 hours | Contracts Developer |
| Sub-phase 1.2: Deploy and ABI | 1-1.5 hours | Contracts Developer |
| **Phase 2: Node S5 Integration** | 3-4 hours | Node Developer |
| Sub-phase 2.1: Add S5 Client | 30 minutes | Node Developer |
| Sub-phase 2.2: Proof Upload | 1 hour | Node Developer |
| Sub-phase 2.3: Update Encoding | 1.5 hours | Node Developer |
| Sub-phase 2.4: Version & Build | 30 min + build | Node Developer |
| **Phase 3: Testing** | 1-2 hours | Both |
| Sub-phase 3.1: Local Tests | 1 hour | Node Developer |
| Sub-phase 3.2: Testnet Deploy | 1 hour | Both |
| **TOTAL** | **6-9 hours** | |

---

## Current Status

**Date**: 2025-10-14
**Version**: v8.1.1 (proof generation working, submission blocked)
**Next Step**: Phase 1.1 - Contract developer updates `submitProofOfWork` signature

**Blockers**:
- [ ] Contract developer availability
- [ ] Contract deployment to Base Sepolia

**Ready to Start**: Phase 2 can begin in parallel with contract work (prepare code, await deployment)

---

## References

- [IMPLEMENTATION-RISC0.md](./IMPLEMENTATION-RISC0.md) - Original Risc0 implementation
- [GPU-STARK-PROOFS-VERIFICATION.md](../GPU-STARK-PROOFS-VERIFICATION.md) - v8.1.1 verification report
- [S5 Client Implementation](../src/storage/s5_client.rs) - S5Storage trait and implementations
- [JobMarketplace Contract Docs](./compute-contracts-reference/JobMarketplace.md) - Current contract documentation
