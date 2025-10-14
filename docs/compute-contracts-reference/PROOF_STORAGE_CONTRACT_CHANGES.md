# Proof Storage Contract Changes - Implementation Guide

**Date**: 2025-10-14
**Target Contract**: JobMarketplaceWithModels
**Network**: Base Sepolia (Chain ID: 84532)
**Current Address**: `0xe169A4B57700080725f9553E3Cc69885fea13629`
**Estimated Time**: 2-3 hours

---

## Problem Statement

**Current Issue**: STARK proofs are ~221KB but RPC transaction limit is 128KB.

**Root Cause**: `submitProofOfWork()` currently accepts full proof as `bytes calldata proof` parameter:
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes calldata proof  // ❌ 221KB - exceeds RPC limit
) external
```

**Impact**:
- Node generates STARK proofs successfully (✅ working in v8.1.1)
- Node cannot submit proofs to blockchain (❌ RPC rejects: "oversized data")
- All checkpoint submissions failing with error: `transaction size 221715, limit 131072`

---

## Solution Overview

**Architecture Change**: Store full proofs off-chain in S5, submit only hash + CID on-chain.

**What Changes**:
1. Contract stores proof **hash** (32 bytes) instead of full proof
2. Contract stores proof **CID** (S5 storage location) for retrieval
3. Disputants fetch proof from S5, verify hash, then verify cryptographically

**Benefits**:
- ✅ Transaction size: ~300 bytes (fits in 128KB RPC limit)
- ✅ Storage cost: ~$0.001 vs ~$50 for 221KB on-chain
- ✅ Proof integrity: SHA256 hash prevents tampering
- ✅ Proof availability: S5 decentralized storage

---

## Required Contract Changes

### Change 1: Update `submitProofOfWork` Function Signature

**File**: `contracts/JobMarketplaceWithModels.sol`

**BEFORE** (Current):
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes calldata proof
) external nonReentrant {
    // ... implementation ...
}
```

**AFTER** (Required):
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,      // NEW: SHA256 hash of proof (32 bytes)
    string calldata proofCID // NEW: S5 CID for proof retrieval
) external nonReentrant {
    // ... implementation (updated) ...
}
```

**Changes**:
- Replace `bytes calldata proof` with `bytes32 proofHash, string calldata proofCID`
- Hash is 32 bytes (SHA256 of full proof)
- CID is S5 blob identifier from s5-rs (e.g., "u8pDTQHOOY..." - no "s5://" prefix)

---

### Change 2: Update `SessionJob` Struct

**File**: `contracts/JobMarketplaceWithModels.sol`

Add fields to store proof hash and CID:

```solidity
struct SessionJob {
    uint256 id;
    address depositor;
    address requester;
    address host;
    address paymentToken;
    uint256 deposit;
    uint256 pricePerToken;
    uint256 tokensUsed;
    uint256 maxDuration;
    uint256 startTime;
    uint256 lastProofTime;
    uint256 proofInterval;
    SessionStatus status;
    uint256 withdrawnByHost;
    uint256 refundedToUser;
    string conversationCID;

    // NEW: Add these fields
    bytes32 lastProofHash;   // Hash of most recent proof
    string lastProofCID;     // S5 CID of most recent proof
}
```

**Storage Impact**:
- `bytes32 lastProofHash`: 1 storage slot (32 bytes)
- `string lastProofCID`: 1+ storage slots depending on length (~50-100 bytes typical)

**Note**: Verify storage layout compatibility if upgrading existing contract.

---

### Change 3: Update `submitProofOfWork` Implementation

**File**: `contracts/JobMarketplaceWithModels.sol`

**Implementation Changes**:

```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    string calldata proofCID
) external nonReentrant {
    SessionJob storage job = sessionJobs[jobId];

    // Existing validation
    require(job.status == SessionStatus.Active, "Job not active");
    require(msg.sender == job.host, "Not job host");
    require(tokensClaimed >= MIN_PROVEN_TOKENS, "Below minimum tokens");

    // NEW: Store proof hash and CID
    job.lastProofHash = proofHash;
    job.lastProofCID = proofCID;
    job.lastProofTime = block.timestamp;

    // Existing token tracking
    job.tokensUsed += tokensClaimed;

    // Existing payment distribution logic
    uint256 payment = tokensClaimed * job.pricePerToken;
    uint256 treasuryFee = (payment * FEE_BASIS_POINTS) / 10000;
    uint256 hostEarnings = payment - treasuryFee;

    // ... rest of existing logic ...

    // NEW: Updated event emission (see Change 4)
    emit ProofSubmitted(jobId, msg.sender, tokensClaimed, proofHash, proofCID);
}
```

**Key Points**:
- Store both hash and CID in storage
- Update `lastProofTime` as before
- All existing validation and payment logic unchanged
- Emit updated event with hash and CID

---

### Change 4: Update `ProofSubmitted` Event

**File**: `contracts/JobMarketplaceWithModels.sol`

**BEFORE** (Current):
```solidity
event ProofSubmitted(
    uint256 indexed jobId,
    address indexed host,
    uint256 tokensClaimed,
    bytes32 proofHash,  // NOTE: This already exists but was storing hash of full proof
    bool verified
);
```

**AFTER** (Required):
```solidity
event ProofSubmitted(
    uint256 indexed jobId,
    address indexed host,
    uint256 tokensClaimed,
    bytes32 proofHash,      // NOW: Direct hash from node (not hashed again)
    string proofCID         // NEW: S5 CID for retrieval
);
```

**Changes**:
- Add `string proofCID` parameter
- Remove `bool verified` parameter (or keep if needed elsewhere)
- Hash is now the direct SHA256 from node (not hashed by contract)

**Usage**:
- Off-chain indexers can use this event to build proof CID → job ID mapping
- Disputants fetch CID from event logs to retrieve proof from S5

---

## Implementation Checklist

### Step 1: Update Contract Code
- [ ] Modify `submitProofOfWork()` function signature
- [ ] Add `lastProofHash` and `lastProofCID` to `SessionJob` struct
- [ ] Update `submitProofOfWork()` implementation to store hash and CID
- [ ] Update `ProofSubmitted` event definition
- [ ] Update event emission in `submitProofOfWork()`

### Step 2: Verify Storage Layout
- [ ] Check if `SessionJob` struct changes affect existing storage
- [ ] If upgrading: Ensure new fields appended to end of struct
- [ ] If new deployment: No concerns

### Step 3: Update Interface (if exists)
- [ ] If `IJobMarketplace.sol` exists, update interface signature
- [ ] Ensure all inheritance chains updated

### Step 4: Compile and Test
- [ ] Compile contract: `forge build` or `npx hardhat compile`
- [ ] Verify no compilation errors
- [ ] Run existing unit tests (should fail until updated)
- [ ] Update unit tests to use new signature
- [ ] Add tests for proof hash storage
- [ ] Add tests for proof CID storage

### Step 5: Deploy to Base Sepolia
- [ ] Deploy updated contract to Base Sepolia testnet
- [ ] Verify deployment on BaseScan: https://sepolia.basescan.org/
- [ ] Record new contract address
- [ ] Test `submitProofOfWork()` call via Etherscan UI

### Step 6: Generate and Distribute ABI
- [ ] Export ABI JSON from compiled artifacts
- [ ] Save to: `docs/compute-contracts-reference/client-abis/JobMarketplaceWithModels-CLIENT-ABI-v2.json`
- [ ] Update `.env.contracts` with new `JOB_MARKETPLACE_FAB_WITH_S5_ADDRESS`
- [ ] Document deployment in `docs/compute-contracts-reference/JobMarketplace.md`

---

## Testing Guide

### Unit Tests to Add/Update

**Test 1: Verify Function Signature**
```solidity
function testSubmitProofOfWork_WithHashAndCID() public {
    uint256 jobId = 1;
    uint256 tokensClaimed = 100;
    bytes32 proofHash = keccak256("test_proof");
    string memory proofCID = "u8pDTQHOOYtest123";

    vm.prank(host);
    marketplace.submitProofOfWork(jobId, tokensClaimed, proofHash, proofCID);

    // Verify storage
    (, , , , , , , , , , , , , , , , bytes32 storedHash, string memory storedCID)
        = marketplace.sessionJobs(jobId);

    assertEq(storedHash, proofHash, "Proof hash not stored");
    assertEq(storedCID, proofCID, "Proof CID not stored");
}
```

**Test 2: Verify Event Emission**
```solidity
function testSubmitProofOfWork_EmitsEventWithCID() public {
    uint256 jobId = 1;
    uint256 tokensClaimed = 100;
    bytes32 proofHash = keccak256("test_proof");
    string memory proofCID = "u8pDTQHOOYtest123";

    vm.expectEmit(true, true, false, true);
    emit ProofSubmitted(jobId, host, tokensClaimed, proofHash, proofCID);

    vm.prank(host);
    marketplace.submitProofOfWork(jobId, tokensClaimed, proofHash, proofCID);
}
```

**Test 3: Verify Transaction Size**
```solidity
function testSubmitProofOfWork_TransactionSize() public {
    // This test verifies the transaction fits within RPC limits
    uint256 jobId = 1;
    uint256 tokensClaimed = 100;
    bytes32 proofHash = keccak256("test_proof");

    // Simulate realistic CID length (~50-100 chars)
    string memory proofCID = "u8pDTQHOOYabcdef1234567890abcdef1234567890abcdef1234567890";

    vm.prank(host);
    bytes memory txData = abi.encodeWithSignature(
        "submitProofOfWork(uint256,uint256,bytes32,string)",
        jobId,
        tokensClaimed,
        proofHash,
        proofCID
    );

    // Verify transaction size < 1KB (well under 128KB RPC limit)
    assertLt(txData.length, 1024, "Transaction too large");
}
```

**Test 4: Verify Storage Updates**
```solidity
function testSubmitProofOfWork_UpdatesLastProofTime() public {
    uint256 jobId = 1;
    uint256 tokensClaimed = 100;
    bytes32 proofHash = keccak256("test_proof");
    string memory proofCID = "u8pDTQHOOYtest123";

    uint256 beforeTime = block.timestamp;

    vm.prank(host);
    marketplace.submitProofOfWork(jobId, tokensClaimed, proofHash, proofCID);

    (, , , , , , , , , , uint256 lastProofTime, , , , , , , )
        = marketplace.sessionJobs(jobId);

    assertEq(lastProofTime, beforeTime, "Last proof time not updated");
}
```

---

## Integration Testing

After deployment, test with actual node:

1. **Setup**: Point node to new contract address
2. **Trigger**: Create session job, generate 100+ tokens
3. **Verify Node Logs**:
   ```
   🔐 Generating real Risc0 STARK proof for job X
   📤 Uploading proof to S5 for job X (221466 bytes)
   ✅ Proof uploaded to S5: CID=u8pDTQHOOY...
   📊 Proof hash: 0x...
   ✅ Checkpoint SUCCESS for job X
   ```
4. **Verify On-Chain**:
   - Check transaction on BaseScan
   - Verify input data size < 1KB (not 221KB)
   - Check `ProofSubmitted` event has hash and CID
5. **Verify Retrieval**:
   - Fetch proof from S5 using CID
   - Calculate SHA256 hash of proof
   - Verify hash matches on-chain hash

---

## ABI Changes Summary

**Old ABI** (submitProofOfWork):
```json
{
  "type": "function",
  "name": "submitProofOfWork",
  "inputs": [
    {"name": "jobId", "type": "uint256"},
    {"name": "tokensClaimed", "type": "uint256"},
    {"name": "proof", "type": "bytes"}
  ],
  "outputs": [],
  "stateMutability": "nonpayable"
}
```

**New ABI** (submitProofOfWork):
```json
{
  "type": "function",
  "name": "submitProofOfWork",
  "inputs": [
    {"name": "jobId", "type": "uint256"},
    {"name": "tokensClaimed", "type": "uint256"},
    {"name": "proofHash", "type": "bytes32"},
    {"name": "proofCID", "type": "string"}
  ],
  "outputs": [],
  "stateMutability": "nonpayable"
}
```

**Old Event ABI** (ProofSubmitted):
```json
{
  "type": "event",
  "name": "ProofSubmitted",
  "inputs": [
    {"name": "jobId", "type": "uint256", "indexed": true},
    {"name": "host", "type": "address", "indexed": true},
    {"name": "tokensClaimed", "type": "uint256", "indexed": false},
    {"name": "proofHash", "type": "bytes32", "indexed": false},
    {"name": "verified", "type": "bool", "indexed": false}
  ]
}
```

**New Event ABI** (ProofSubmitted):
```json
{
  "type": "event",
  "name": "ProofSubmitted",
  "inputs": [
    {"name": "jobId", "type": "uint256", "indexed": true},
    {"name": "host", "type": "address", "indexed": true},
    {"name": "tokensClaimed", "type": "uint256", "indexed": false},
    {"name": "proofHash", "type": "bytes32", "indexed": false},
    {"name": "proofCID", "type": "string", "indexed": false}
  ]
}
```

---

## Deployment Instructions

### Using Foundry

```bash
# 1. Set environment variables
export PRIVATE_KEY="your_deployer_private_key"
export BASE_SEPOLIA_RPC_URL="https://sepolia.base.org"

# 2. Compile
forge build

# 3. Deploy
forge create --rpc-url $BASE_SEPOLIA_RPC_URL \
  --private-key $PRIVATE_KEY \
  --etherscan-api-key $BASESCAN_API_KEY \
  --verify \
  contracts/JobMarketplaceWithModels.sol:JobMarketplaceWithModels \
  --constructor-args <args>

# 4. Record address
echo "New contract: 0x..."
```

### Using Hardhat

```bash
# 1. Update deployment script with new contract
# 2. Deploy
npx hardhat run scripts/deploy-marketplace.js --network baseSepolia

# 3. Verify
npx hardhat verify --network baseSepolia <address> <constructor-args>
```

---

## Post-Deployment Checklist

- [ ] Contract deployed to Base Sepolia
- [ ] Contract verified on BaseScan
- [ ] New address recorded: `0x...`
- [ ] ABI exported to: `docs/compute-contracts-reference/client-abis/JobMarketplaceWithModels-CLIENT-ABI-v2.json`
- [ ] `.env.contracts` updated with new `JOB_MARKETPLACE_FAB_WITH_S5_ADDRESS`
- [ ] `docs/compute-contracts-reference/JobMarketplace.md` updated with deployment info
- [ ] Node developer notified of new address and ABI
- [ ] Test transaction sent via Etherscan UI to verify function works

---

## Rollback Plan

If issues discovered after deployment:

1. **Keep old contract**: Previous address still functional
2. **Node config**: Node can switch back to old address via `.env.contracts`
3. **No data loss**: Old proofs still accessible (though they couldn't submit anyway)
4. **Redeploy**: Fix issues and redeploy updated contract

**Old Contract (for reference)**:
- Address: `0xe169A4B57700080725f9553E3Cc69885fea13629`
- Function: `submitProofOfWork(uint256,uint256,bytes)`

---

## Questions for Contracts Developer

1. **Storage Layout**: Is this a new deployment or upgrade?
   - If upgrade: Need to ensure storage layout compatibility
   - If new deployment: Can modify struct freely

2. **ProofSystem Integration**: Does current contract call `IProofSystem.verifyProof()`?
   - If yes: May need to update proof system to accept hash instead
   - If no: No additional changes needed

3. **Access Control**: Who can call `submitProofOfWork()`?
   - Current: Only job host (`msg.sender == job.host`)
   - Should this change? (Probably not)

4. **Gas Optimization**: Should we index `proofCID` in event?
   - Current plan: Not indexed (saves gas)
   - Alternative: Index for easier filtering (costs more gas)

---

## Contact

**Node Developer**: Ready to integrate once contract deployed
**Coordination**: Update `.env.contracts` and share new ABI when ready
**Testing**: Node developer will validate end-to-end on testnet

---

## References

- [IMPLEMENTATION-RISC0-2.md](../IMPLEMENTATION-RISC0-2.md) - Full implementation plan
- [Current JobMarketplace Docs](./JobMarketplace.md) - Existing contract documentation
- [Base Sepolia Explorer](https://sepolia.basescan.org/) - For verification
- [Current Contract Address](https://sepolia.basescan.org/address/0xe169A4B57700080725f9553E3Cc69885fea13629)
