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

**Timeline**: 2.5-3 hours (reduced from 3-4 hours due to s5-rs simplicity)
**Prerequisites**: Phase 1 complete (new contract deployed)
**Goal**: Integrate s5-rs for direct P2P proof upload and modify checkpoint submission

### S5 Integration Architecture

**Native Rust Client**: Using [s5-rs](https://github.com/s5-dev/s5-rs) for direct S5 network access

**Why s5-rs:**
- ✅ Native Rust library (no HTTP bridge)
- ✅ Direct P2P connection to S5 network
- ✅ No external services needed (no ENHANCED_S5_URL)
- ✅ Simpler deployment (single binary)
- ✅ Better performance and type safety
- ✅ Automatic content addressing (CID from hash)

**Architecture:**
```
Rust Node → s5-rs → S5 P2P Network
          (direct)  (decentralized storage)
```

**vs Old Approach:**
```
Rust Node → HTTP → Enhanced S5.js → S5 Network
          (complex) (Node.js service)
```

**S5 Client Initialization:**
```rust
let s5_client = S5Client::builder()
    .initial_peers(vec![
        "wss://z2DWuPbL5pweybXnEB618pMnV58ECj2VPDNfVGm3tFqBvjF@s5.ninja/s5/p2p"
    ])
    .build()
    .await?;
```

**Proof Upload:**
```rust
// Upload 221KB proof
let blob_id = s5_client.upload_blob(&proof_bytes).await?;
let cid = blob_id.to_string();  // e.g., "u8pDTQHOOY..."

// Later: Download for verification
let blob_id = BlobId::from_string(&cid)?;
let proof_bytes = s5_client.download_blob(&blob_id).await?;
```

---

### Sub-phase 2.1: Add s5-rs Dependency and Client

**Goal**: Integrate s5-rs native Rust client into CheckpointManager for direct P2P proof uploads

#### Tasks

**Step 1: Add s5-rs Dependency**
- [ ] Add `s5 = "0.1"` to `Cargo.toml` dependencies (check latest version on crates.io)
- [ ] Verify compilation: `cargo check`

**Step 2: Add S5 Client Field**
- [ ] Modify `CheckpointManager` struct in `src/contracts/checkpoint_manager.rs`
- [ ] Add `s5_client: s5::S5Client` field
- [ ] Update constructor to initialize S5 client with peer list

**Expected Changes**:
```rust
// src/contracts/checkpoint_manager.rs

use s5::S5Client;

pub struct CheckpointManager {
    web3_client: Arc<Web3Client>,
    job_trackers: Arc<RwLock<HashMap<u64, JobTokenTracker>>>,
    proof_system_address: Address,
    host_address: Address,
    s5_client: S5Client, // NEW: Direct s5-rs client (no trait abstraction)
}

impl CheckpointManager {
    pub async fn new(web3_client: Arc<Web3Client>) -> Result<Self> {
        // ... existing code ...

        // Initialize S5 client with P2P peers
        let s5_client = S5Client::builder()
            .initial_peers(vec![
                "wss://z2DWuPbL5pweybXnEB618pMnV58ECj2VPDNfVGm3tFqBvjF@s5.ninja/s5/p2p".to_string()
            ])
            .build()
            .await
            .map_err(|e| anyhow!("Failed to initialize S5 client: {}", e))?;

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

**Step 3: Update Constructor Call Sites**
- [ ] Make CheckpointManager::new() async where called
- [ ] Or keep synchronous constructor and initialize s5_client lazily on first use

**Step 4: Update Tests**
- [ ] For tests, create mock S5Client or use conditional compilation
- [ ] Verify CheckpointManager initialization

#### Success Criteria
- [ ] s5-rs dependency added and compiles
- [ ] CheckpointManager has S5Client field
- [ ] S5 client connects to P2P network (no external services needed)
- [ ] Tests compile and pass

#### Files Modified
- [ ] `Cargo.toml` - Add s5 dependency
- [ ] `src/contracts/checkpoint_manager.rs` - Add field, update constructor

#### Estimated Time
**~30 minutes**

#### Notes
- **No environment variables needed** - peer list hardcoded
- **No external services** - connects directly to S5 P2P network
- **Native Rust** - better performance than HTTP bridge
- Can add peer configuration from env later if needed

---

### Sub-phase 2.2: Implement Proof Upload to S5

**Goal**: Upload generated STARK proof to S5 and get CID using s5-rs

#### Tasks

**Step 1: Add Proof Upload Method**
- [ ] Create `upload_proof_to_s5()` method in CheckpointManager
- [ ] Use s5-rs `upload_blob()` API directly (no paths needed)
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
        info!("📤 Uploading proof to S5 for job {} ({} bytes)", job_id, proof_bytes.len());

        // Upload blob directly using s5-rs
        let blob_id = self.s5_client
            .upload_blob(proof_bytes)
            .await
            .map_err(|e| anyhow!("S5 upload failed: {}", e))?;

        // Convert BlobId to string CID
        let cid = blob_id.to_string();

        info!("✅ Proof uploaded to S5: CID={}", cid);
        Ok(cid)
    }
}
```

**Step 2: Integrate Upload into submit_checkpoint()**
- [ ] Call `upload_proof_to_s5()` after `generate_proof()`
- [ ] Keep proof bytes for hash calculation
- [ ] Get CID from upload

**Step 3: Add Error Handling**
- [ ] Handle S5 upload failures gracefully
- [ ] Add retry logic (max 3 retries with exponential backoff)
- [ ] Log upload status and CID
- [ ] Handle network errors

**Example with Retry**:
```rust
async fn upload_proof_to_s5_with_retry(
    &self,
    job_id: u64,
    proof_bytes: &[u8],
) -> Result<String> {
    let mut retries = 0;
    let max_retries = 3;

    loop {
        match self.s5_client.upload_blob(proof_bytes).await {
            Ok(blob_id) => {
                let cid = blob_id.to_string();
                info!("✅ Proof uploaded to S5: CID={}", cid);
                return Ok(cid);
            }
            Err(e) if retries < max_retries => {
                retries += 1;
                let delay = std::time::Duration::from_secs(2u64.pow(retries));
                warn!("⚠️ S5 upload failed (attempt {}/{}): {}", retries, max_retries, e);
                tokio::time::sleep(delay).await;
            }
            Err(e) => {
                return Err(anyhow!("S5 upload failed after {} retries: {}", max_retries, e));
            }
        }
    }
}
```

#### Success Criteria
- [ ] Proof uploads to S5 successfully via s5-rs
- [ ] CID returned from upload (BlobId string format)
- [ ] Proof retrievable from S5 by CID
- [ ] Error handling and retry logic working
- [ ] No external HTTP services needed

#### Files Modified
- [ ] `src/contracts/checkpoint_manager.rs` - Add upload method

#### Estimated Time
**~45 minutes** (simplified from 1 hour due to s5-rs simplicity)

#### Notes
- **No paths needed** - s5-rs stores blobs by content hash automatically
- **Direct P2P** - uploads go directly to S5 network, no portal needed
- **Automatic deduplication** - same proof uploaded twice gets same CID

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
- [ ] Verify `HOST_PRIVATE_KEY` set
- [ ] No S5 configuration needed (s5-rs connects directly to P2P network)

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
- [ ] s5-rs dependency added
- [ ] S5 client integrated into CheckpointManager
- [ ] Direct P2P connection to S5 network established
- [ ] Proof uploads to S5 successfully
- [ ] Transaction encodes hash + CID
- [ ] Transaction size < 1KB (fits RPC limit)
- [ ] Version updated to v8.1.2
- [ ] No external S5 services required

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
- [ ] `Cargo.toml` - Add s5-rs dependency
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
3. **S5 Issues**: If S5 P2P network unreachable, node will fail checkpoint submission (no fallback - proof generation is mandatory)

---

## Notes and Considerations

### S5 Content Addressing
- **No paths needed**: s5-rs stores blobs by content hash (automatic CID)
- **Automatic deduplication**: Same proof bytes → same CID
- **Unique by content**: Different proofs always have different CIDs

### CID Format
- s5-rs returns CID from `BlobId` (e.g., "u8pDTQHOOY...")
- Store as string in contract
- Retrieve proof: `s5_client.download_blob(&blob_id)`

### Transaction Size
- Proof hash: 32 bytes
- CID string: ~50-100 bytes
- Function selector + params: ~100-200 bytes
- **Total**: ~200-400 bytes (428x smaller than 221KB proof!)

### Verification Flow
1. Get job proof hash from contract
2. Get job proof CID from contract events
3. Parse CID: `let blob_id = BlobId::from_string(&cid)?`
4. Fetch proof from S5: `s5_client.download_blob(&blob_id)`
5. Calculate hash: `sha256(proof_bytes)`
6. Compare: `calculated_hash == on_chain_hash`
7. Verify proof: `risc0::verify(proof, public_inputs)`

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
| **Phase 2: Node S5 Integration** | 2.5-3 hours | Node Developer |
| Sub-phase 2.1: Add s5-rs Client | 30 minutes | Node Developer |
| Sub-phase 2.2: Proof Upload | 45 minutes | Node Developer |
| Sub-phase 2.3: Update Encoding | 1.5 hours | Node Developer |
| Sub-phase 2.4: Version & Build | 30 min + build | Node Developer |
| **Phase 3: Testing** | 1-2 hours | Both |
| Sub-phase 3.1: Local Tests | 1 hour | Node Developer |
| Sub-phase 3.2: Testnet Deploy | 1 hour | Both |
| **TOTAL** | **5.5-8 hours** | (reduced due to s5-rs simplicity) |

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
