# Client ABIs Changelog

## February 26, 2026 - Phase 18: Per-Model Per-Token Pricing Migration (BREAKING CHANGE)

### ⚠️ BREAKING CHANGE — Major Pricing API Overhaul
Phase 18 consolidates all pricing into a single per-model per-token mapping (`modelTokenPricing`), removing the layered fallback system from Phase 17. Non-model session creation functions are also removed from JobMarketplace.

**Why**: The previous system had multiple overlapping pricing layers (node-level default, custom token pricing, model-level overrides) which created confusion and potential for mispricing. The new system has exactly one pricing source: `modelTokenPricing[host][modelId][token]`.

### Supersedes Phase 17 (F202614977)
Phase 17's `setTokenPricing`/`getNodePricing` approach is entirely replaced. All Phase 17 changes are superseded by Phase 18.

### NodeRegistry: Removed Functions
| Function | Replacement |
|----------|-------------|
| `getNodePricing(address, address)` | `getModelPricing(address, bytes32, address)` |
| `setTokenPricing(address, uint256)` | `setModelTokenPricing(bytes32, address, uint256)` |
| `updatePricingNative(uint256)` | `setModelTokenPricing(modelId, address(0), price)` |
| `updatePricingStable(uint256)` | `setModelTokenPricing(modelId, token, price)` |
| `setModelPricing(bytes32, uint256, uint256)` | `setModelTokenPricing(bytes32, address, uint256)` |
| `clearModelPricing(bytes32)` | `clearModelTokenPricing(bytes32, address)` |

### NodeRegistry: Added Functions
```solidity
// Set pricing for a specific model + token combination
function setModelTokenPricing(bytes32 modelId, address token, uint256 price) external

// Clear pricing for a specific model + token combination
function clearModelTokenPricing(bytes32 modelId, address token) external
```

### NodeRegistry: Changed Functions
```solidity
// getModelPricing — no fallback chain, reads modelTokenPricing directly
// Reverts with "No model pricing" if not set (was "No token pricing" in Phase 17)
function getModelPricing(address operator, bytes32 modelId, address token) external view returns (uint256)

// getHostModelPrices — now takes a token parameter (was: no token param, returned dual native/stable)
function getHostModelPrices(address operator, address token) external view returns (
    bytes32[] memory modelIds,
    uint256[] memory prices
)
```

### NodeRegistry: New Event
```solidity
event ModelTokenPricingUpdated(address indexed operator, bytes32 indexed modelId, address indexed token, uint256 price);
```

### NodeRegistry: New Storage
```solidity
// Per-operator, per-model, per-token pricing
mapping(address => mapping(bytes32 => mapping(address => uint256))) public modelTokenPricing;
```

### JobMarketplace: Removed Functions
| Function | Replacement |
|----------|-------------|
| `createSessionJob(...)` | `createSessionJobForModel(...)` |
| `createSessionJobWithToken(...)` | `createSessionJobForModelWithToken(...)` |
| `createSessionFromDeposit(...)` | `createSessionFromDepositForModel(...)` |

All sessions now require a model ID parameter.

### Host Migration Required
All registered hosts must call `setModelTokenPricing()` for each model+token combination they accept:
```javascript
// After registerNode(), set pricing for each model+token pair
await nodeRegistry.setModelTokenPricing(modelId, usdcAddress, pricePerToken);
await nodeRegistry.setModelTokenPricing(modelId, ethers.constants.AddressZero, nativePricePerToken);
```

```bash
# Via cast
cast send 0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22 \
  "setModelTokenPricing(bytes32,address,uint256)" \
  <MODEL_ID> 0x036CbD53842c5426634e7929541eC2318f3dCF7e <price> \
  --private-key $HOST_KEY --rpc-url "https://sepolia.base.org" --legacy
```

### SDK Changes Required
```javascript
// 1. Host registration flow — add setModelTokenPricing for each model+token
await nodeRegistry.registerNode(metadata, apiUrl, [modelId], nativePrice, stablePrice);
await nodeRegistry.setModelTokenPricing(modelId, usdcAddress, stablePrice); // NEW — required
await nodeRegistry.setModelTokenPricing(modelId, ethers.constants.AddressZero, nativePrice); // NEW — required

// 2. Query pricing — use getModelPricing (getNodePricing is removed)
try {
  const price = await nodeRegistry.getModelPricing(host, modelId, usdcAddress);
} catch (e) {
  console.log("Host has not set pricing for this model+token");
}

// 3. Batch query — getHostModelPrices now takes a token parameter
const [modelIds, prices] = await nodeRegistry.getHostModelPrices(host, usdcAddress);

// 4. Session creation — use model-specific functions (non-model variants removed)
// OLD (removed):
// await marketplace.createSessionJob(host, price, duration, interval, timeout, { value: deposit });
// NEW:
await marketplace.createSessionJobForModel(modelId, host, price, duration, interval, timeout, { value: deposit });
```

### ABI Files Updated
- `NodeRegistryWithModelsUpgradeable-CLIENT-ABI.json` — re-extracted
- `JobMarketplaceWithModelsUpgradeable-CLIENT-ABI.json` — re-extracted

---

## ~~February 24, 2026 - F202614977: Per-Token Pricing Fix (BREAKING CHANGE)~~ SUPERSEDED BY PHASE 18

> **Note**: This entry is superseded by Phase 18 (Feb 26, 2026) above. The `setTokenPricing`/`getNodePricing` approach has been replaced by per-model per-token pricing via `setModelTokenPricing`/`getModelPricing`.

### Original Description (for historical reference)
`getNodePricing()` and `getModelPricing()` no longer silently fall back to `minPricePerTokenStable` for ERC20 tokens. They now **revert** with `"No token pricing"` if the host hasn't explicitly set pricing via `setTokenPricing(token, price)`.

**Why**: A 6-decimal token (USDC) and an 18-decimal token (DAI) previously got the same raw price value, enabling users to pay dust amounts for inference.

---

## February 22, 2026 - Post-Audit Remediation Deployment (ALL 20 FINDINGS ADDRESSED)

### PROXY ADDRESS CHANGED
**JobMarketplace proxy address has changed** due to fresh proxy deployment (clean storage layout for `minTokensFee` + `isAuthorizedDelegate`).

| Contract | Old Proxy | New Proxy |
|----------|-----------|-----------|
| JobMarketplace | `0x95132177F964FF053C1E874b53CF74d819618E06` | `0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4` |

### Implementation Upgrades
| Contract | Proxy (unchanged unless noted) | New Implementation |
|----------|-------------------------------|-------------------|
| JobMarketplace | `0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4` (FRESH) | `0x51C3F60D2e3756Cc3F119f9aE1876e2B947347ba` |
| ProofSystem | `0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31` | `0xC46C84a612Cbf4C2eAaf5A9D1411aDA6309EC963` |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` | `0xF12a0A07d4230E0b045dB22057433a9826d21652` |

### SDK Breaking Changes

**`submitProofOfWork` signature changed (signature parameter removed):**
```solidity
// Old (no longer works):
function submitProofOfWork(uint256 jobId, uint256 tokensClaimed, bytes32 proofHash,
    bytes calldata signature, string calldata proofCID, string calldata deltaCID)

// New (required):
function submitProofOfWork(uint256 sessionId, uint256 tokensClaimed, bytes32 proofHash,
    string calldata proofCID, string calldata deltaCID)
```

**All session creation functions now require `proofTimeoutWindow` parameter:**
- `createSessionJob()`, `createSessionJobWithToken()`, `createSessionJobForModel()`, etc.
- Value in seconds (60-3600), use 0 for default (300s)

**Require string text changed (F202615067):**
- Error messages shortened for EVM contract size compliance
- Example: `"Only host can submit proof"` -> `"Not host"`
- Behavior unchanged; only error message text differs

### New Functions (Audit Remediation)
```solidity
// F202614916: Deposit-based model sessions + delegation
function createSessionFromDepositForModel(bytes32 modelId, address host, address paymentToken,
    uint256 amount, uint256 pricePerToken, uint256 maxDuration, uint256 proofInterval,
    uint256 proofTimeoutWindow) external returns (uint256)
function createSessionForModelAsDelegate(address payer, bytes32 modelId, address host,
    address paymentToken, uint256 amount, uint256 pricePerToken, uint256 maxDuration,
    uint256 proofInterval, uint256 proofTimeoutWindow) external returns (uint256)
function authorizeDelegate(address delegate, bool authorized) external
function isDelegateAuthorized(address depositor, address delegate) external view returns (bool)

// F202614917: Early cancellation fee
function setMinTokensFee(uint256 fee) external  // owner-only
function minTokensFee() external view returns (uint256)

// F202614964: Rejected proposal fee withdrawal (ModelRegistry)
function withdrawRejectedFees() external  // owner-only
function accumulatedRejectedFees() external view returns (uint256)

// F202614913: Per-model rate limits (ModelRegistry)
function setModelRateLimit(bytes32 modelId, uint256 maxTokensPerSecond) external  // owner-only
function getModelRateLimit(bytes32 modelId) external view returns (uint256)
```

### New Events
```solidity
// F202614898: Pull-pattern refund
event RefundCreditedToDeposit(uint256 indexed jobId, address indexed depositor, uint256 amount, address indexed token);

// F202614916: Delegation
event DelegateAuthorized(address indexed depositor, address indexed delegate, bool authorized);
event SessionCreatedByDelegate(uint256 indexed sessionId, address indexed depositor, address indexed delegate);

// F202614913: Rate limits (ModelRegistry)
event ModelRateLimitUpdated(bytes32 indexed modelId, uint256 maxTokensPerSecond);

// F202614964: Rejected fees (ModelRegistry)
event RejectedFeesWithdrawn(address indexed to, uint256 amount);
```

### SDK Migration Guide
```javascript
// Update contract address
const CONTRACTS = {
  jobMarketplace: "0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4",  // CHANGED
  proofSystem: "0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31",
  // ... other addresses unchanged
};

// Update submitProofOfWork (signature parameter removed)
// OLD:
await marketplace.submitProofOfWork(jobId, tokensClaimed, proofHash, signature, proofCID, deltaCID);
// NEW:
await marketplace.submitProofOfWork(sessionId, tokensClaimed, proofHash, proofCID, deltaCID);

// Update session creation (proofTimeoutWindow added)
// OLD:
await marketplace.createSessionJob(host, pricePerToken, maxDuration, proofInterval, { value: deposit });
// NEW:
const proofTimeoutWindow = 300; // 5 minutes (or 0 for default)
await marketplace.createSessionJob(host, pricePerToken, maxDuration, proofInterval, proofTimeoutWindow, { value: deposit });
```

---

## January 16, 2026 - Stake Slashing Feature

### New Feature: Host Stake Slashing
Penalize hosts for proven misbehavior (e.g., overclaiming tokens, invalid proofs).

**New Constants:**
```solidity
uint256 public constant MAX_SLASH_PERCENTAGE = 50;      // Max 50% per slash
uint256 public constant MIN_STAKE_AFTER_SLASH = 100e18; // 100 FAB minimum
uint256 public constant SLASH_COOLDOWN = 24 hours;      // Cooldown between slashes
```

**New Functions:**
```solidity
// Main slashing function (slashing authority only)
function slashStake(
    address host,
    uint256 amount,
    string calldata evidenceCID,
    string calldata reason
) external

// Initialize slashing after upgrade (owner only, one-time)
function initializeSlashing(address _treasury) external

// Set slashing authority (owner only - for DAO migration)
function setSlashingAuthority(address newAuthority) external

// Set treasury for slashed tokens (owner only)
function setTreasury(address newTreasury) external

// Query last slash time
function lastSlashTime(address host) external view returns (uint256)
```

**New Events:**
```solidity
event SlashExecuted(
    address indexed host,
    uint256 amount,
    uint256 remainingStake,
    string evidenceCID,
    string reason,
    address indexed executor,
    uint256 timestamp
);
event HostAutoUnregistered(
    address indexed host,
    uint256 slashedAmount,
    uint256 returnedAmount,
    string reason
);
event SlashingAuthorityUpdated(address indexed previousAuthority, address indexed newAuthority);
event TreasuryUpdated(address indexed newTreasury);
```

**Auto-Unregister Behavior:**
- If host stake falls below 100 FAB after slash, host is automatically unregistered
- Remaining stake is returned to the host
- Host is removed from active nodes list and model mappings

### Implementation Upgrade
| Contract | Proxy (unchanged) | New Implementation |
|----------|-------------------|-------------------|
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` | `0xF2D98D38B2dF95f4e8e4A49750823C415E795377` |

### No SDK Breaking Changes
- All existing functions work as before
- Slashing functions are admin-only (slashing authority)
- New query function `lastSlashTime(address)` available for all

### SDK Integration (Optional)
```javascript
// Query if a host was recently slashed
const lastSlash = await nodeRegistry.lastSlashTime(hostAddress);
if (lastSlash > 0) {
  console.log(`Host was last slashed at: ${new Date(lastSlash * 1000)}`);
}

// Listen for slash events
nodeRegistry.on("SlashExecuted", (host, amount, remaining, evidenceCID, reason, executor, timestamp) => {
  console.log(`Host ${host} slashed ${amount} FAB. Remaining: ${remaining}`);
});
```

---

## January 14, 2026 - deltaCID Support for Proof Submissions (BREAKING CHANGE)

### ⚠️ SDK BREAKING CHANGE
**`submitProofOfWork` signature changed from 5 to 6 parameters**

A new `deltaCID` parameter has been added to track delta CIDs for incremental proof storage.

**Old signature (no longer works):**
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    bytes calldata signature,
    string calldata proofCID
)
```

**New signature (required):**
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    bytes calldata signature,
    string calldata proofCID,
    string calldata deltaCID  // NEW: Delta CID for incremental storage
)
```

### Additional Breaking Changes

**`getProofSubmission` returns 5 values instead of 4:**
```solidity
// Old (4 values)
(bytes32 proofHash, uint256 tokensClaimed, uint256 timestamp, bool verified)

// New (5 values)
(bytes32 proofHash, uint256 tokensClaimed, uint256 timestamp, bool verified, string deltaCID)
```

**`ProofSubmitted` event has new field:**
```solidity
// Old
event ProofSubmitted(uint256 indexed jobId, address indexed host, uint256 tokensClaimed, bytes32 proofHash, string proofCID);

// New
event ProofSubmitted(uint256 indexed jobId, address indexed host, uint256 tokensClaimed, bytes32 proofHash, string proofCID, string deltaCID);
```

### SDK Migration Guide
```javascript
// OLD (no longer works):
await marketplace.submitProofOfWork(jobId, tokensClaimed, proofHash, signature, proofCID);

// NEW (required):
const deltaCID = "QmDeltaCID..."; // or "" if no delta
await marketplace.submitProofOfWork(jobId, tokensClaimed, proofHash, signature, proofCID, deltaCID);

// Reading proofs - OLD (4 values):
const [proofHash, tokens, timestamp, verified] = await marketplace.getProofSubmission(sessionId, 0);

// Reading proofs - NEW (5 values):
const [proofHash, tokens, timestamp, verified, deltaCID] = await marketplace.getProofSubmission(sessionId, 0);
```

### Implementation Upgrade
| Contract | Proxy (unchanged) | New Implementation |
|----------|-------------------|-------------------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` | `0x1B6C6A1E373E5E00Bf6210e32A6DA40304f6484c` |

### ProofSubmission Struct Change
```solidity
struct ProofSubmission {
    bytes32 proofHash;
    uint256 tokensClaimed;
    uint256 timestamp;
    bool verified;
    string deltaCID;  // NEW field
}
```

### What is deltaCID?
- Used for incremental/delta proof storage on decentralized networks
- Allows tracking changes between consecutive proofs
- Pass empty string `""` if not using delta storage

---

## January 11, 2026 - ModelRegistry Voting Improvements (Phase 14-15)

### Security Audit Remediation
Implements voting mechanism improvements from the January 2026 security audit.

### Phase 14: Anti-Sniping Vote Extension
Prevents whale attacks where large votes arrive at the last minute:
- If a large vote (≥10,000 FAB) arrives in the last 4 hours, voting extends by 1 day
- Maximum 3 extensions per proposal
- Cumulative late votes trigger extension

**New Constants:**
```solidity
uint256 public constant EXTENSION_THRESHOLD = 10000 * 10**18;  // 10k FAB
uint256 public constant EXTENSION_WINDOW = 4 hours;
uint256 public constant EXTENSION_DURATION = 1 days;
uint256 public constant MAX_EXTENSIONS = 3;
```

**New Event:**
```solidity
event VotingExtended(bytes32 indexed modelId, uint256 newEndTime, uint8 extensionCount);
```

**Struct Changes (ModelProposal):**
- Added `endTime` (uint256) - Dynamic end time for voting
- Added `extensionCount` (uint8) - Number of extensions applied

### Phase 15: Re-proposal Cooldown System
Allows rejected models to be re-proposed after a cooldown:
- 30-day cooldown after a proposal is executed (approved or rejected)
- Old proposal data is cleared when re-proposing

**New Constant:**
```solidity
uint256 public constant REPROPOSAL_COOLDOWN = 30 days;
```

**New State Variable:**
```solidity
mapping(bytes32 => uint256) public lastProposalExecutionTime;
```

### Implementation Upgrade
| Contract | Proxy (unchanged) | New Implementation |
|----------|-------------------|-------------------|
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` | `0x8491af1f0D47f6367b56691dCA0F4996431fB0A5` |

### ABI Changes
**Added to ModelRegistry:**
- `EXTENSION_THRESHOLD()` - Constant (10k FAB)
- `EXTENSION_WINDOW()` - Constant (4 hours)
- `EXTENSION_DURATION()` - Constant (1 day)
- `MAX_EXTENSIONS()` - Constant (3)
- `REPROPOSAL_COOLDOWN()` - Constant (30 days)
- `lateVotes(bytes32)` - Mapping for cumulative late votes
- `lastProposalExecutionTime(bytes32)` - Mapping for cooldown tracking
- `VotingExtended` event

### No SDK Breaking Changes
- All existing functions work as before
- `proposals(bytes32)` now returns 9 fields instead of 7 (added `endTime`, `extensionCount`)
- Voting and proposal creation work identically

---

## January 10, 2026 - NodeRegistry Corrupt Node Fix

### Bug Fix
Fixed an edge case where hosts registered during contract upgrades could end up in a "corrupt" state:
- `nodes[host].active = true`
- `activeNodesIndex[host] = 0`
- But host NOT in `activeNodesList[]`

This caused `unregisterNode()` to fail or corrupt other nodes' data.

### Changes

**New Admin Function:**
```solidity
function repairCorruptNode(address nodeAddress) external onlyOwner
```
- Owner-only function to clean up corrupt node state
- Returns staked FAB tokens to the host
- Emits `CorruptNodeRepaired(address operator, uint256 stakeReturned)`

**Safety Check in `unregisterNode()`:**
- Now detects and handles corrupt state gracefully
- Hosts can unregister even with corrupt state

### Implementation Upgrade
| Contract | Proxy (unchanged) | New Implementation |
|----------|-------------------|-------------------|
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` | `0x4574d6f1D888cF97eBb8E1bb5E02a5A386b6cFA7` |

### Corrupt Host Repaired
- Host `0x048afA7126A3B684832886b78e7cC1Dd4019557E` fixed
- 1000 FAB stake returned

### ABI Changes
**Added to NodeRegistry:**
- `repairCorruptNode(address)` - Owner-only repair function
- `CorruptNodeRepaired` event

### No SDK Breaking Changes
- `unregisterNode()` works as before (now handles edge cases)
- New function is admin-only

---

## January 9, 2026 - Clean Slate JobMarketplace Deployment

### ⚠️ PROXY ADDRESS CHANGED
**JobMarketplace proxy address has changed** due to clean slate deployment (removed deprecated storage slots for gas optimization).

| Contract | Old Proxy | New Proxy |
|----------|-----------|-----------|
| JobMarketplace | `0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D` | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` |

### Implementation Addresses (January 9, 2026)
| Contract | Proxy | Implementation |
|----------|-------|----------------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` | `0x26f27C19F80596d228D853dC39A204f0f6C45C7E` |
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` | `0xb85424dd91D4ae0C6945e512bfDdF8a494299115` |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` | `0x1D31d9688a4ffD2aFE738BC6C9a4cb27C272AA5A` |
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` | `0xCF46BBa79eA69A68001A1c2f5Ad9eFA1AD435EF9` |
| HostEarnings | `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` | `0x8584AeAC9687613095D13EF7be4dE0A796F84D7a` |

### SDK Migration Required
Update your configuration to use the new JobMarketplace proxy address:
```javascript
const CONTRACTS = {
  jobMarketplace: "0x3CaCbf3f448B420918A93a88706B26Ab27a3523E",  // ⚠️ CHANGED
  nodeRegistry: "0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22",
  // ... other addresses unchanged
};
```

### Technical Details
- Solidity upgraded to ^0.8.24 (compiled with 0.8.30)
- Using OpenZeppelin's ReentrancyGuardTransient (~4,900 gas savings)
- Requires EIP-1153 (transient storage) - supported on Base since March 2024

---

## January 6, 2026 - Phase 6: ProofSystem Integration (BREAKING CHANGE)

### ⚠️ SDK BREAKING CHANGE
**`submitProofOfWork` signature changed from 4 to 5 parameters**

The ProofSystem's signature verification is now enforced. Hosts must sign their proofs.

**Old signature (no longer works):**
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    string calldata proofCID
)
```

**New signature (required):**
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    bytes calldata signature,  // NEW: 65 bytes (r, s, v)
    string calldata proofCID
)
```

### SDK Migration Guide
```javascript
// OLD (no longer works):
await marketplace.submitProofOfWork(jobId, tokensClaimed, proofHash, proofCID);

// NEW (required):
// 1. Generate proofHash (hash of work done)
const proofHash = keccak256(workData);

// 2. Sign the proof data
const dataHash = keccak256(
  solidityPacked(['bytes32', 'address', 'uint256'], [proofHash, hostAddress, tokensClaimed])
);
const signature = await hostWallet.signMessage(getBytes(dataHash));

// 3. Submit with signature
await marketplace.submitProofOfWork(jobId, tokensClaimed, proofHash, signature, proofCID);
```

### New Implementation Deployed
| Contract | Proxy (unchanged) | New Implementation |
|----------|-------------------|-------------------|
| JobMarketplace | `0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D` | `0x05c7d3a1b748dEbdbc12dd75D1aC195fb93228a3` |

### New View Function
- `getProofSubmission(uint256 sessionId, uint256 proofIndex)` - Returns proof details including `verified` flag

### What the Signature Proves
- The host authorized this specific proof
- The host claims exactly N tokens
- Non-repudiation (host signed it)

---

## January 6, 2026 - Security Audit Remediation

### Security Fixes Applied
All CRITICAL and MEDIUM vulnerabilities from the January 2025 security audit have been fixed:

| Issue | Severity | Status |
|-------|----------|--------|
| recordVerifiedProof front-running | CRITICAL | ✅ Fixed - Access control added |
| Missing host validation | CRITICAL | ✅ Fixed - NodeRegistry query |
| withdrawNative double-spend | CRITICAL | ✅ Fixed - Deposit tracking |
| Unreachable claimWithProof | MEDIUM | ✅ Fixed - Removed dead code |

### Implementation Upgrades (UUPS)
Proxies upgraded to new implementations with security fixes:

| Contract | Proxy (unchanged) | New Implementation |
|----------|-------------------|-------------------|
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` | `0xf0DA90e1ae1A3aB7b9Da47790Abd73D26b17670F` |
| JobMarketplace | `0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D` | `0xfa6F48eced34294B4FCe3Ae6Bb78d22858AfEe8B` |

### ABI Changes

**Removed from JobMarketplace (legacy dead code):**
- `claimWithProof()` function
- `JobPosted`, `JobClaimed`, `JobCompleted` events
- `Job`, `JobStatus`, `JobType`, `JobDetails`, `JobRequirements` types

**Added to JobMarketplace:**
- `getLockedBalanceNative(address)` - View locked funds in active sessions
- `getLockedBalanceToken(address, address)` - View locked token funds
- `getTotalBalanceNative(address)` - View total balance (withdrawable + locked)
- `getTotalBalanceToken(address, address)` - View total token balance

**Added to ProofSystem:**
- `setAuthorizedCaller(address, bool)` - Owner sets authorized callers
- `authorizedCallers(address)` - Check if address is authorized
- `AuthorizedCallerUpdated` event

### No SDK Breaking Changes
- **Proxy addresses unchanged** - Use same addresses as before
- **submitProofOfWork unchanged** - Same 4-parameter signature
- **Session flow unchanged** - createSessionJob, submitProofOfWork, completeSessionJob

### Migration Notes
- No SDK code changes required for existing integrations
- New view functions available for balance transparency
- Hosts must be registered in NodeRegistry (was always required for sessions to work)

---

## December 14, 2025 - UUPS Upgradeable Migration + Minimum Deposit Reduction

### Major Changes
- **All contracts migrated to UUPS Upgradeable pattern**
- **Minimum deposits reduced to ~$0.50**
- **Old non-upgradeable ABIs removed**

### Current ABIs (Upgradeable Only)
- `JobMarketplaceWithModelsUpgradeable-CLIENT-ABI.json`
- `NodeRegistryWithModelsUpgradeable-CLIENT-ABI.json`
- `ModelRegistryUpgradeable-CLIENT-ABI.json`
- `HostEarningsUpgradeable-CLIENT-ABI.json`
- `ProofSystemUpgradeable-CLIENT-ABI.json`

### New Contract Addresses (UUPS Proxies)
| Contract | Proxy Address | Implementation |
|----------|---------------|----------------|
| JobMarketplace | `0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D` | `0xe0ee96FC4Cc7a05a6e9d5191d070c5d1d13f143F` |
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` | `0x68298e2b74a106763aC99E3D973E98012dB5c75F` |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` | `0xd7Df5c6D4ffe6961d47753D1dd32f844e0F73f50` |
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` | `0x83eB050Aa3443a76a4De64aBeD90cA8d525E7A3A` |
| HostEarnings | `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` | `0x588c42249F85C6ac4B4E27f97416C0289980aabB` |

### Minimum Deposits (Reduced)
| Payment Type | Old Value | New Value |
|--------------|-----------|-----------|
| ETH | 0.0002 ETH (~$0.88) | 0.0001 ETH (~$0.50) |
| USDC | 800000 ($0.80) | 500000 ($0.50) |

### New Functions Added to JobMarketplace
- `updateTokenMinDeposit(address token, uint256 minDeposit)` - Admin function to update minimum deposits
- `pause()` / `unpause()` - Emergency pause functionality

### New Events Added to JobMarketplace
- `TokenMinDepositUpdated(address indexed token, uint256 oldMinDeposit, uint256 newMinDeposit)`
- `ContractPaused(address indexed by)`
- `ContractUnpaused(address indexed by)`

### Removed ABIs (Deprecated)
The following non-upgradeable ABIs have been removed:
- `HostEarnings-CLIENT-ABI.json`
- `JobMarketplaceWithModels-CLIENT-ABI.json`
- `ModelRegistry-CLIENT-ABI.json`
- `NodeRegistryWithModels-CLIENT-ABI.json`
- `ProofSystem-CLIENT-ABI.json`

### Migration Required
1. **Update all contract addresses** to use proxy addresses above
2. **Update ABIs** to use `*Upgradeable-CLIENT-ABI.json` files
3. **Update minimum deposit checks** (now $0.50 instead of $0.80)

---

## December 10, 2025 - Rate Limit Fix (2000 tokens/sec)

### Changes
- Increased proof submission rate limit from 200 to 2000 tokens/sec
- Supports high-speed inference on small models (RTX 4090: 800-1500 tok/sec)

---

## October 14, 2025 - S5 Proof Storage

### Changes
- Moved STARK proofs to S5 decentralized storage
- On-chain: only hash + CID (300 bytes vs 221KB)
- `submitProofOfWork` signature changed to 4 parameters

---

## September 2025 - Initial Release

### Features
- Session-based streaming payments
- FAB token staking for hosts
- USDC/ETH payment support
- Model governance via ModelRegistry
