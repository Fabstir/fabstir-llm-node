# Node Software Upgrade: Pre-Report Remediation

**Version:** 2.1.0
**Date:** January 31, 2026
**Network:** Base Sepolia (Chain ID: 84532)

---

## Overview

This guide covers the required changes to upgrade your node software to work with the remediated contracts. These changes address security findings from the AUDIT pre-report review.

**Action Required:** Update your node software and test against the new contracts.

### What's Changing

| Change | Impact | Section |
|--------|--------|---------|
| Signature includes `modelId` | **BREAKING** | §1 |
| `deltaCID` parameter added | Additive | §2 |
| New contract addresses | **Required** | §3 |

---

## Contract Addresses

### Test Contracts (Use These Now)

Update your node software to use these addresses:

```javascript
const CONTRACTS = {
  jobMarketplace: "0x95132177F964FF053C1E874b53CF74d819618E06",  // NEW
  proofSystem: "0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31",    // NEW
  nodeRegistry: "0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22",   // Unchanged
  modelRegistry: "0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2", // Unchanged
  hostEarnings: "0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0",  // Unchanged
  fabToken: "0xC78949004B4EB6dEf2D66e49Cd81231472612D62",
  usdcToken: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
};
```

### Frozen Contracts (Auditors Only)

These remain unchanged for security audit. Do not use for new development:

| Contract | Frozen Address |
|----------|----------------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` |
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` |

---

## 1. BREAKING: Signature Must Include modelId

### The Change (AUDIT-F4)

The proof signature now **must** include `modelId` to prevent cross-model replay attacks.

### Old Signature (No Longer Works)

```javascript
// ❌ OLD - Will be rejected
const dataHash = ethers.solidityPackedKeccak256(
  ["bytes32", "address", "uint256"],
  [proofHash, hostAddress, tokensClaimed]
);
```

### New Signature (Required)

```javascript
// ✅ NEW - Required format
const dataHash = ethers.solidityPackedKeccak256(
  ["bytes32", "address", "uint256", "bytes32"],
  [proofHash, hostAddress, tokensClaimed, modelId]
);
```

### Complete Implementation

```javascript
const { ethers } = require("ethers");

/**
 * Generate proof signature with modelId (AUDIT-F4 compliant)
 * @param {string} proofHash - bytes32 hash of proof data
 * @param {string} hostAddress - Host wallet address
 * @param {bigint} tokensClaimed - Number of tokens claimed
 * @param {string} modelId - bytes32 model ID (use ethers.ZeroHash for non-model sessions)
 * @param {string} privateKey - Host private key
 * @returns {string} 65-byte signature
 */
function generateProofSignature(proofHash, hostAddress, tokensClaimed, modelId, privateKey) {
  // Step 1: Create data hash with modelId
  const dataHash = ethers.solidityPackedKeccak256(
    ["bytes32", "address", "uint256", "bytes32"],
    [proofHash, hostAddress, tokensClaimed, modelId]
  );

  // Step 2: Create EIP-191 signed message hash
  const messageHash = ethers.solidityPackedKeccak256(
    ["string", "bytes32"],
    ["\x19Ethereum Signed Message:\n32", dataHash]
  );

  // Step 3: Sign
  const signingKey = new ethers.SigningKey(privateKey);
  const sig = signingKey.sign(messageHash);

  // Step 4: Return 65-byte signature (r + s + v)
  return ethers.concat([sig.r, sig.s, ethers.toBeHex(sig.v)]);
}
```

### Getting the modelId

```javascript
// For model-specific sessions (created with createSessionJobForModel)
const modelId = await jobMarketplace.sessionModel(sessionId);

// For non-model sessions (created with createSessionJob)
// modelId will be bytes32(0)
const modelId = await jobMarketplace.sessionModel(sessionId);
// Returns: 0x0000000000000000000000000000000000000000000000000000000000000000
```

### Full Proof Submission Flow

```javascript
async function submitProofOfWork(sessionId, tokensClaimed, proofData, proofCID, deltaCID = "") {
  // 1. Get session details
  const session = await jobMarketplace.sessionJobs(sessionId);
  const hostAddress = session.host;

  // 2. Get modelId for this session
  const modelId = await jobMarketplace.sessionModel(sessionId);

  // 3. Create proof hash
  const proofHash = ethers.keccak256(proofData);

  // 4. Generate signature with modelId
  const signature = generateProofSignature(
    proofHash,
    hostAddress,
    tokensClaimed,
    modelId,
    process.env.HOST_PRIVATE_KEY
  );

  // 5. Submit to contract
  const tx = await jobMarketplace.submitProofOfWork(
    sessionId,
    tokensClaimed,
    proofHash,
    signature,
    proofCID,
    deltaCID
  );

  return tx;
}
```

---

## 2. New Parameter: deltaCID

### The Change

`submitProofOfWork` now accepts a `deltaCID` parameter for incremental state updates.

### Function Signature

```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    bytes calldata signature,
    string calldata proofCID,
    string calldata deltaCID   // NEW - 6th parameter
) external
```

### Usage

```javascript
// With delta CID (for incremental updates)
await jobMarketplace.submitProofOfWork(
  sessionId,
  tokensClaimed,
  proofHash,
  signature,
  "QmProofCID123...",
  "QmDeltaCID456..."  // Incremental state update
);

// Without delta CID (pass empty string)
await jobMarketplace.submitProofOfWork(
  sessionId,
  tokensClaimed,
  proofHash,
  signature,
  "QmProofCID123...",
  ""  // No delta update
);
```

---

## 3. Update Your ABIs

Replace your local ABI files with the updated versions from `client-abis/`:

- `JobMarketplaceWithModelsUpgradeable-CLIENT-ABI.json`
- `ProofSystemUpgradeable-CLIENT-ABI.json`

Key ABI changes:
- `submitProofOfWork`: Now 6 parameters (added `deltaCID`)
- `getProofSubmission`: Now returns 5 values (added `deltaCID`)
- `verifyAndMarkComplete`: Now requires `modelId` parameter

---

## 4. Verification Checklist

Before testing, verify your node software:

- [ ] Updated contract addresses to test contracts
- [ ] Signature generation includes `modelId` as 4th parameter
- [ ] Query `sessionModel(sessionId)` to get modelId for each session
- [ ] `submitProofOfWork` passes 6 parameters (including `deltaCID`)
- [ ] Updated ABIs from `client-abis/`

### Quick Test

```javascript
// Verify signature is correct format
const testProofHash = ethers.keccak256(ethers.toUtf8Bytes("test proof"));
const testModelId = ethers.ZeroHash; // For non-model session

const sig = generateProofSignature(
  testProofHash,
  hostWallet.address,
  1000n,
  testModelId,
  hostWallet.privateKey
);

console.log("Signature length:", ethers.getBytes(sig).length); // Should be 65
```

---

## Summary of Changes

| Before | After |
|--------|-------|
| Signature: `hash(proofHash, host, tokens)` | Signature: `hash(proofHash, host, tokens, modelId)` |
| `submitProofOfWork(jobId, tokens, hash, sig, cid)` | `submitProofOfWork(jobId, tokens, hash, sig, cid, deltaCID)` |
| JobMarketplace: `0x3CaC...23E` | JobMarketplace: `0x9513...E06` |
| ProofSystem: `0x5afB...eeb` | ProofSystem: `0xE8DC...B31` |

---

## Questions?

- Reference: `docs/REMEDIATION_CHANGES.md` for full commit history
- Breaking changes: `client-abis/CHANGELOG.md`
