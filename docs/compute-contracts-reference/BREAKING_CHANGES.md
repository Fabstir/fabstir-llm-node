# Breaking Changes

---

## January 14, 2026: deltaCID Proof Tracking & conversationCID

**Contracts Affected**: JobMarketplaceWithModelsUpgradeable
**Impact Level**: HIGH - Function signature changes require SDK updates

### Summary

Added cryptographic audit trail support via CID (Content Identifier) parameters for proof submissions and session completion.

| Change | Impact | Action Required |
|--------|--------|-----------------|
| `submitProofOfWork` signature | HIGH | Update to 6 parameters |
| `getProofSubmission` return | HIGH | Update tuple unpacking (5 values) |
| `ProofSubmitted` event | MEDIUM | Update event listeners |
| `completeSessionJob` signature | HIGH | Add `conversationCID` parameter |

### 1. submitProofOfWork Signature Change (BREAKING)

**Before (5 parameters):**
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    bytes calldata signature,
    string calldata proofCID
) external
```

**After (6 parameters):**
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    bytes calldata signature,
    string calldata proofCID,
    string calldata deltaCID    // NEW: Delta CID for incremental changes
) external
```

**Migration:**
```javascript
// Before
await marketplace.submitProofOfWork(jobId, tokens, proofHash, signature, proofCID);

// After
await marketplace.submitProofOfWork(jobId, tokens, proofHash, signature, proofCID, deltaCID);
// Use "" for deltaCID if not tracking incremental changes
```

### 2. getProofSubmission Return Change (BREAKING)

**Before (4 return values):**
```solidity
returns (bytes32 proofHash, uint256 tokensClaimed, uint256 timestamp, bool verified)
```

**After (5 return values):**
```solidity
returns (bytes32 proofHash, uint256 tokensClaimed, uint256 timestamp, bool verified, string memory deltaCID)
```

**Migration:**
```javascript
// Before
const [proofHash, tokensClaimed, timestamp, verified] =
  await marketplace.getProofSubmission(sessionId, proofIndex);

// After
const [proofHash, tokensClaimed, timestamp, verified, deltaCID] =
  await marketplace.getProofSubmission(sessionId, proofIndex);
```

### 3. ProofSubmitted Event Change

**Before (5 fields):**
```solidity
event ProofSubmitted(
    uint256 indexed jobId,
    address indexed host,
    uint256 tokensClaimed,
    bytes32 proofHash,
    string proofCID
);
```

**After (6 fields):**
```solidity
event ProofSubmitted(
    uint256 indexed jobId,
    address indexed host,
    uint256 tokensClaimed,
    bytes32 proofHash,
    string proofCID,
    string deltaCID    // NEW
);
```

### 4. completeSessionJob Signature Change (BREAKING)

**Before (1 parameter):**
```solidity
function completeSessionJob(uint256 jobId) external
```

**After (2 parameters):**
```solidity
function completeSessionJob(uint256 jobId, string calldata conversationCID) external
```

**Migration:**
```javascript
// Before
await marketplace.completeSessionJob(jobId);

// After
const conversationCID = await s5Client.upload(JSON.stringify(conversationLog));
await marketplace.completeSessionJob(jobId, conversationCID);
// Use "" if not storing conversation record
```

### 5. Updated Implementation Address

| Contract | Proxy | New Implementation |
|----------|-------|-------------------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` | `0x1B6C6A1E373E5E00Bf6210e32A6DA40304f6484c` |

### Migration Checklist

#### For SDK Developers

- [ ] Update `submitProofOfWork` calls to include 6th parameter (`deltaCID`)
- [ ] Update `getProofSubmission` tuple unpacking (5 values)
- [ ] Update `completeSessionJob` calls to include `conversationCID`
- [ ] Update event listeners for `ProofSubmitted` (6 fields)
- [ ] Update cached ABIs from `client-abis/`

#### For Node Operators (Hosts)

- [ ] Update proof submission code to pass `deltaCID` (can be empty string)
- [ ] Update session completion code to pass `conversationCID`
- [ ] Implement S5 upload for CID generation (optional but recommended)

### Why This Change?

The deltaCID and conversationCID parameters enable:
1. **Audit trail**: Every proof and conversation is stored on S5
2. **Dispute evidence**: CIDs provide cryptographic proof of what was delivered
3. **Future DAO governance**: Evidence-based dispute resolution with stake slashing

---

## January 11, 2026: ModelRegistry Voting Improvements (Phase 14-15)

**Contracts Affected**: ModelRegistryUpgradeable
**Impact Level**: LOW - Struct field addition (backward compatible)

### Summary

Voting mechanism improvements from security audit remediation:

| Change | Impact | Action Required |
|--------|--------|-----------------|
| `ModelProposal` struct expanded | LOW | Update struct destructuring (7 → 9 fields) |
| New constants added | NONE | Optional - use for UI display |
| New `VotingExtended` event | NONE | Optional - subscribe for UI updates |

### 1. ModelProposal Struct Change

**Before (7 fields):**
```solidity
struct ModelProposal {
    bytes32 modelId;
    address proposer;
    uint256 votesFor;
    uint256 votesAgainst;
    uint256 proposalTime;
    bool executed;
    Model modelData;
}
```

**After (9 fields):**
```solidity
struct ModelProposal {
    bytes32 modelId;
    address proposer;
    uint256 votesFor;
    uint256 votesAgainst;
    uint256 proposalTime;
    bool executed;
    Model modelData;
    uint256 endTime;        // NEW: Dynamic end time for anti-sniping
    uint8 extensionCount;   // NEW: Number of extensions applied
}
```

**Impact for SDK Developers**: If you destructure `proposals(bytes32)` return value, update from 7 to 9 fields:
```javascript
// Before
const [modelId, proposer, votesFor, votesAgainst, proposalTime, executed, modelData] =
  await modelRegistry.proposals(proposalId);

// After
const [modelId, proposer, votesFor, votesAgainst, proposalTime, executed, modelData, endTime, extensionCount] =
  await modelRegistry.proposals(proposalId);
```

### 2. New Constants (Read-Only)

```solidity
EXTENSION_THRESHOLD = 10000 * 10**18  // 10k FAB triggers extension
EXTENSION_WINDOW = 4 hours            // Last 4 hours is danger zone
EXTENSION_DURATION = 1 days           // Extend by 1 day
MAX_EXTENSIONS = 3                    // Maximum extensions per proposal
REPROPOSAL_COOLDOWN = 30 days         // Wait time before re-proposing
```

### 3. New Event

```solidity
event VotingExtended(bytes32 indexed modelId, uint256 newEndTime, uint8 extensionCount);
```

Subscribe to this event to update UI when voting is extended due to late whale votes.

### 4. New Mappings

```solidity
mapping(bytes32 => uint256) public lateVotes;                  // Cumulative late votes
mapping(bytes32 => uint256) public lastProposalExecutionTime;  // For cooldown tracking
```

---

## January 9, 2026: Security Audit Remediation - Solidity Upgrade & Contract Changes

**Contracts Affected**: All upgradeable contracts
**Impact Level**: MEDIUM - Requires redeployment awareness and potential ABI updates

### Summary

Security audit remediation completed with the following breaking changes:

| Change | Impact | Action Required |
|--------|--------|-----------------|
| Solidity ^0.8.24 | LOW | Update build tooling if compiling locally |
| ReentrancyGuardTransient | LOW | No action (internal change) |
| ProofSystem function rename | HIGH | Update any direct ProofSystem calls |
| Custom ReentrancyGuard removed | NONE | No action (internal change) |

### 1. Solidity Version Upgrade

**Changed From**: `^0.8.19`
**Changed To**: `^0.8.24` (compiled with 0.8.30)

**Why**: Required to use OpenZeppelin's `ReentrancyGuardTransient` which uses EIP-1153 transient storage.

**Impact for SDK Developers**: None - ABI remains compatible.

**Impact for Node Operators**: None - no interface changes.

**Impact for Local Development**: Update Foundry/Forge if you compile contracts locally:
```bash
foundryup  # Updates to latest Foundry with Solidity 0.8.30 support
```

### 2. EIP-1153 Transient Storage Requirement

The contracts now require **EIP-1153** (Cancun upgrade) support on the target network.

**Supported Networks**:
| Network | EIP-1153 Support |
|---------|------------------|
| Base Mainnet | ✅ Since March 2024 |
| Base Sepolia | ✅ Since March 2024 |
| Ethereum Mainnet | ✅ Since March 2024 |
| opBNB | ✅ Since March 2024 |

**Impact**: If deploying to a network that hasn't undergone the Cancun upgrade, contracts will fail to deploy.

### 3. ProofSystem Function Rename (BREAKING)

**Changed From**: `verifyEKZL(bytes proof, address prover, uint256 claimedTokens)`
**Changed To**: `verifyHostSignature(bytes proof, address prover, uint256 claimedTokens)`

**Why**: The function verifies host ECDSA signatures, not EZKL proofs. The rename improves code clarity.

**Impact for SDK Developers**:
- If you call `ProofSystem` directly (rare), update function name
- If you only interact with `JobMarketplace`, no changes needed (it calls ProofSystem internally)

**Migration**:
```javascript
// Before (OLD)
await proofSystem.verifyEKZL(proof, prover, tokens);

// After (NEW)
await proofSystem.verifyHostSignature(proof, prover, tokens);
```

### 4. ReentrancyGuard Implementation Change

**Changed From**: Custom `ReentrancyGuardUpgradeable` (src/utils/)
**Changed To**: OpenZeppelin's `ReentrancyGuardTransient`

**Benefits**:
- ~4,900 gas savings per `nonReentrant` call
- Uses battle-tested OpenZeppelin code
- Removes custom code (security improvement)

**Impact**: None for SDK/Node developers - this is an internal implementation detail.

### 5. Storage Layout Unchanged

Despite the ReentrancyGuard change, **storage layout is preserved**:
- ReentrancyGuardTransient uses transient storage (not contract storage)
- Existing proxy storage slots are unaffected
- No data migration required

### 6. Updated Implementation Addresses

After deploying the upgraded implementations, proxy implementations will change:

| Contract | Proxy | Implementation (Jan 9) |
|----------|-------|----------------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` | `0x26f27C19F80596d228D853dC39A204f0f6C45C7E` |
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` | `0x4574d6f1D888cF97eBb8E1bb5E02a5A386b6cFA7` |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` | `0x8491af1f0D47f6367b56691dCA0F4996431fB0A5` |
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` | `0xCF46BBa79eA69A68001A1c2f5Ad9eFA1AD435EF9` |
| HostEarnings | `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` | `0x8584AeAC9687613095D13EF7be4dE0A796F84D7a` |

**Note**: JobMarketplace proxy was a clean slate deployment (January 9, 2026). Implementation later upgraded to `0x1B6C6A1E373E5E00Bf6210e32A6DA40304f6484c` for deltaCID support (January 14, 2026).

### 7. ABI Changes

**ProofSystem ABI**: Function renamed
```json
// Before
{"name": "verifyEKZL", "type": "function", ...}

// After
{"name": "verifyHostSignature", "type": "function", ...}
```

**All Other ABIs**: No changes to function signatures.

### Migration Checklist

#### For SDK Developers

- [ ] Update Foundry/Forge if compiling locally (`foundryup`)
- [ ] If calling ProofSystem directly: rename `verifyEKZL` → `verifyHostSignature`
- [ ] Update ProofSystem ABI if cached locally
- [ ] No other code changes required

#### For Node Operators

- [ ] No action required for existing registrations
- [ ] Update local tooling if compiling contracts
- [ ] Verify your RPC provider supports EIP-1153 (all major providers do)

#### For Contract Deployers

- [ ] Deploy new implementations after upgrade
- [ ] Call `upgradeToAndCall()` on each proxy (owner only)
- [ ] Update implementation addresses in documentation
- [ ] Regenerate and distribute new ABIs

### Verification After Upgrade

```bash
# Verify Solidity version in deployed bytecode
cast code $PROXY_ADDRESS --rpc-url $RPC_URL | head -c 100

# Verify ProofSystem function exists
cast call $PROOF_SYSTEM "verifyHostSignature(bytes,address,uint256)" 0x... $ADDR 100 --rpc-url $RPC_URL

# Verify nonReentrant still works (any protected function)
cast call $JOB_MARKETPLACE "createSessionJob(address,uint256,uint256,uint256)" ... --rpc-url $RPC_URL
```

---

## December 14, 2025: Minimum Deposit Reduction + UUPS Migration

**Contracts Affected**: `JobMarketplaceWithModelsUpgradeable`
**Impact Level**: LOW - Non-breaking, reduces minimum requirements

### Summary

Minimum session deposits have been reduced from ~$0.80 to ~$0.50:

| Parameter | Old Value | New Value |
|-----------|-----------|-----------|
| `MIN_DEPOSIT` (ETH) | 0.0002 ETH (~$0.88) | 0.0001 ETH (~$0.50) |
| `USDC_MIN_DEPOSIT` | 800,000 ($0.80) | 500,000 ($0.50) |

### New Admin Function

A new function allows admins to update minimum deposits without contract redeployment:

```solidity
function updateTokenMinDeposit(address token, uint256 minDeposit) external
```

**Event emitted:**
```solidity
event TokenMinDepositUpdated(address indexed token, uint256 oldMinDeposit, uint256 newMinDeposit)
```

### Contract Addresses (UUPS Proxies) - December 14, 2025

All contracts are now UUPS upgradeable:

| Contract | Proxy Address | Implementation (Dec 14) |
|----------|---------------|----------------|
| JobMarketplace | `0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D` ⚠️ | `0xe0ee96FC4Cc7a05a6e9d5191d070c5d1d13f143F` |
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` | `0x68298e2b74a106763aC99E3D973E98012dB5c75F` |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` | `0xd7Df5c6D4ffe6961d47753D1dd32f844e0F73f50` |
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` | `0x83eB050Aa3443a76a4De64aBeD90cA8d525E7A3A` |
| HostEarnings | `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` | `0x588c42249F85C6ac4B4E27f97416C0289980aabB` |

⚠️ **JobMarketplace proxy `0xeebE...` was replaced with clean slate deployment `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` on January 9, 2026.**

### Migration Required

1. **Update contract addresses** to use proxy addresses (if using old non-upgradeable addresses)
2. **Update ABIs** to use `*Upgradeable-CLIENT-ABI.json` files
3. **No code changes needed** for minimum deposit reduction (it's backwards compatible)

---

## December 2025: PRICE_PRECISION Update

**Contracts Affected**: `NodeRegistryWithModels`, `JobMarketplaceWithModels`
**Impact Level**: HIGH - Requires SDK and Node operator updates

## Summary

Prices are now stored with a **1000x multiplier** (PRICE_PRECISION=1000) to support sub-$1/million token pricing for budget AI models.

**This is a breaking change** - all price values must be multiplied by 1000 to maintain the same USD value.

## Why This Change?

The previous minimum price of $10/million tokens was too high for budget models:

| Model | Actual Price/Million | Previous Min | New Min |
|-------|---------------------|--------------|---------|
| Llama 3.2 3B | $0.06/million | $10/million (too high) | $0.001/million |
| DeepSeek V3 | $0.27/million | $10/million (too high) | $0.001/million |
| Gemma 2 2B | $0.07/million | $10/million (too high) | $0.001/million |

## What Changed

### 1. New Constants

```solidity
// NodeRegistryWithModels.sol
uint256 public constant PRICE_PRECISION = 1000;

// Stable pricing (with 1000x precision):
uint256 public constant MIN_PRICE_PER_TOKEN_STABLE = 1;     // $0.001 per million tokens
uint256 public constant MAX_PRICE_PER_TOKEN_STABLE = 100_000_000; // $100,000 per million tokens

// Native pricing (with 1000x precision, calibrated for ~$4400 ETH):
uint256 public constant MIN_PRICE_PER_TOKEN_NATIVE = 227_273;       // ~$0.001 per million @ $4400 ETH
uint256 public constant MAX_PRICE_PER_TOKEN_NATIVE = 22_727_272_727_273_000; // ~$100,000 per million
```

### 2. Payment Calculation Changes

**Before (OLD):**
```javascript
maxTokens = deposit / pricePerToken
hostPayment = tokensUsed * pricePerToken
```

**After (NEW):**
```javascript
maxTokens = (deposit * PRICE_PRECISION) / pricePerToken
hostPayment = (tokensUsed * pricePerToken) / PRICE_PRECISION
```

## Migration Guide

### For SDK Developers

#### Price Conversion

| USD Price/Million | OLD pricePerToken | NEW pricePerToken |
|-------------------|-------------------|-------------------|
| $0.06/million | Not supported | 60 |
| $0.27/million | Not supported | 270 |
| $1/million | 1 | 1,000 |
| $5/million | 5 | 5,000 |
| $10/million | 10 | 10,000 |
| $50/million | 50 | 50,000 |

**Conversion formula:**
```javascript
// Convert USD/million to pricePerToken
const newPricePerToken = oldPricePerToken * 1000;

// Or from USD directly
const pricePerToken = usdPerMillion * 1000;
```

#### Example SDK Update

**Before (OLD):**
```javascript
const pricePerToken = 5; // $5/million tokens

const session = await marketplace.createSessionJobWithToken(
  host,
  usdcAddress,
  deposit,
  pricePerToken, // 5
  maxDuration,
  proofInterval
);
```

**After (NEW):**
```javascript
const PRICE_PRECISION = 1000;
const pricePerMillion = 5; // $5/million tokens
const pricePerToken = pricePerMillion * PRICE_PRECISION; // 5000

const session = await marketplace.createSessionJobWithToken(
  host,
  usdcAddress,
  deposit,
  pricePerToken, // 5000
  maxDuration,
  proofInterval
);
```

#### Calculating Max Tokens from Deposit

**Before (OLD):**
```javascript
const maxTokens = deposit / pricePerToken;
// Example: 10 USDC / 5 = 2,000,000 tokens
```

**After (NEW):**
```javascript
const PRICE_PRECISION = 1000;
const maxTokens = (deposit * PRICE_PRECISION) / pricePerToken;
// Example: (10 USDC * 1000) / 5000 = 2,000,000 tokens (same result!)
```

#### Calculating Cost from Token Usage

**Before (OLD):**
```javascript
const cost = tokensUsed * pricePerToken;
// Example: 1,000,000 tokens * 5 = 5,000,000 USDC units = $5
```

**After (NEW):**
```javascript
const PRICE_PRECISION = 1000;
const cost = (tokensUsed * pricePerToken) / PRICE_PRECISION;
// Example: (1,000,000 tokens * 5000) / 1000 = 5,000,000 USDC units = $5 (same result!)
```

### For Node Operators

#### Registering a Node

**Before (OLD):**
```javascript
// Register with $5/million stable pricing
await nodeRegistry.registerNode(
  metadata,
  apiUrl,
  modelIds,
  nativePrice,    // e.g., 3_000_000_000 wei
  5               // $5/million stable price
);
```

**After (NEW):**
```javascript
// Register with $5/million stable pricing (multiply by 1000)
await nodeRegistry.registerNode(
  metadata,
  apiUrl,
  modelIds,
  3_000_000,      // Native price (also 1000x smaller for same USD value)
  5000            // $5/million stable price (5 * 1000)
);
```

#### Updating Pricing

**Before (OLD):**
```javascript
await nodeRegistry.updatePricingStable(10); // $10/million
```

**After (NEW):**
```javascript
await nodeRegistry.updatePricingStable(10000); // $10/million (10 * 1000)
```

#### Setting Model-Specific Pricing

**Before (OLD):**
```javascript
await nodeRegistry.setModelPricing(
  modelId,
  nativePrice,
  15  // $15/million for this model
);
```

**After (NEW):**
```javascript
await nodeRegistry.setModelPricing(
  modelId,
  nativePrice,
  15000  // $15/million for this model (15 * 1000)
);
```

### Native Token (ETH/BNB) Pricing

Native pricing also uses PRICE_PRECISION. The values are calibrated for ~$4400 ETH:

| USD Price/Million | NEW Native pricePerToken |
|-------------------|--------------------------|
| $0.001/million | 227,273 (MIN) |
| $0.013/million | 3,000,000 |
| $0.1/million | 22,727,273 |
| $1/million | 227,272,727 |

**Formula:**
```javascript
// Convert USD/million to native pricePerToken (for $4400 ETH)
const nativePricePerToken = (usdPerMillion / 4400) * 1e18 / 1e6 * 1000;
```

## Query Functions

The following query functions return prices in the NEW format (with PRICE_PRECISION):

```javascript
// Get host's base pricing
const [nativePrice, stablePrice] = await nodeRegistry.getNodePricing(hostAddress);
// Returns: [3000000, 5000] for ~$0.013/million native and $5/million stable

// Get model-specific pricing
const modelPrice = await nodeRegistry.getModelPricing(hostAddress, modelId, tokenAddress);
// Returns price with PRICE_PRECISION included

// Get full node info
const [operator, stake, active, metadata, apiUrl, models, nativePrice, stablePrice] =
  await nodeRegistry.getNodeFullInfo(hostAddress);
```

## Checklist for Migration

### SDK Updates

- [ ] Update all `pricePerToken` values by multiplying by 1000
- [ ] Update maxTokens calculation: `(deposit * 1000) / pricePerToken`
- [ ] Update cost calculation: `(tokensUsed * pricePerToken) / 1000`
- [ ] Update price display: divide by 1000 before showing USD
- [ ] Add PRICE_PRECISION constant to SDK config

### Node Operator Updates

- [ ] Re-register nodes with updated price values (×1000)
- [ ] Update any pricing automation scripts
- [ ] Test session creation with new prices

### Testing

- [ ] Verify session creation works with new prices
- [ ] Verify payment calculations are correct
- [ ] Verify price display in UI matches expected USD values

## Contract Addresses

> **Note:** As of December 14, 2025, all contracts have been migrated to UUPS upgradeable pattern. See the December 14, 2025 section above for current proxy addresses.

### Legacy (Non-Upgradeable) - DEPRECATED

| Contract | Address (Base Sepolia) | Status |
|----------|------------------------|--------|
| NodeRegistryWithModels | `0x906F4A8Cb944E4fe12Fb85Be7E627CeDAA8B8999` | ⚠️ DEPRECATED Dec 14, 2025 |
| JobMarketplaceWithModels | `0x75C72e8C3eC707D8beF5Ba9b9C4f75CbB5bced97` | ⚠️ DEPRECATED Dec 14, 2025 |
| JobMarketplaceWithModels (200 tok/sec) | `0x5497a28B4bE6b1219F93e6Fcd631E8aCdC173709` | ⚠️ DEPRECATED Dec 10, 2025 |
| JobMarketplaceWithModels (20 tok/sec) | `0xfD764804C5A5808b79D66746BAF4B65fb4413731` | ⚠️ DEPRECATED Dec 10, 2025 |

## Questions?

If you have questions about this migration, please contact the Fabstir team or open an issue at https://github.com/fabstirp2p/contracts/issues.
