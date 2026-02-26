# Client ABIs

This directory contains the Application Binary Interfaces (ABIs) for client integration.

---

## UPGRADEABLE CONTRACTS (February 22, 2026 - Post-Audit Remediation Deployment)

> **POST-AUDIT REMEDIATION**: All 20 findings from the Hacken security audit addressed.
> **ADDRESS CHANGE**: JobMarketplace has a NEW proxy address (fresh deployment for clean storage layout).
> **RECOMMENDED**: Use these upgradeable contracts for all integrations.

### JobMarketplaceWithModelsUpgradeable
- **Proxy Address**: `0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4` (**FRESH PROXY** - Feb 22, 2026)
- **Implementation**: `0x51C3F60D2e3756Cc3F119f9aE1876e2B947347ba` ✅ Full audit remediation (Feb 22, 2026)
- **Network**: Base Sepolia
- **Status**: ✅ ACTIVE - UUPS Upgradeable
- **ABI File**: `JobMarketplaceWithModelsUpgradeable-CLIENT-ABI.json`
- **Minimum Deposits**:
  - ETH: 0.0001 ETH (~$0.50)
  - USDC: 500000 (0.50 USDC)
- **Key Features**:
  - All features from non-upgradeable version
  - UUPS proxy pattern for future upgrades
  - Emergency pause/unpause functions
  - Owner-only upgrade authorization
  - `updateTokenMinDeposit(address, uint256)` - Admin function to update minimum deposits
  - `TokenMinDepositUpdated` event - Emitted when minimum deposit is changed
- **Security Fixes (Jan 2026)**:
  - ✅ Host validation - Hosts must be registered in NodeRegistry
  - ✅ Double-spend prevention - Fixed deposit tracking for inline sessions
  - ✅ Legacy dead code removed (claimWithProof, Job types)
  - NEW: `getLockedBalanceNative(address)` - View locked funds in active sessions
  - NEW: `getLockedBalanceToken(address, address)` - View locked token funds
  - NEW: `getTotalBalanceNative(address)` - View total balance (withdrawable + locked)
  - NEW: `getTotalBalanceToken(address, address)` - View total token balance
  - NEW: `getProofSubmission(uint256 sessionId, uint256 proofIndex)` - View proof details including verified flag
- **Breaking Change (Feb 26, 2026) — Phase 18: Non-Model Session Functions Removed**:
  - **Removed**: `createSessionJob()`, `createSessionJobWithToken()`, `createSessionFromDeposit()` — all sessions now require a model ID
  - **Use instead**: `createSessionJobForModel()`, `createSessionJobForModelWithToken()`, `createSessionFromDepositForModel()`, `createSessionForModelAsDelegate()`
- **Breaking Change (Feb 22, 2026) — Audit Remediation**:
  - `submitProofOfWork` now takes 5 parameters: `(sessionId, tokensClaimed, proofHash, proofCID, deltaCID)` — signature removed (F202614998+F202614976)
  - New `proofTimeoutWindow` parameter on all session creation functions (F202614911)
  - New `createSessionFromDepositForModel()` and `createSessionForModelAsDelegate()` functions (F202614916)
  - Require strings shortened for EVM size limit compliance (F202615067) — error text changed but behavior unchanged
  - Early cancellation fee via `minTokensFee` (F202614917)
  - Per-model rate limits via `ModelRegistry.getModelRateLimit()` (F202614913)
  - Pull-pattern refunds — `RefundCreditedToDeposit` event (F202614898)
- **Previous Breaking Change (Jan 14, 2026)**:
  - `submitProofOfWork` previously required 6 parameters (added `string calldata deltaCID`)

### NodeRegistryWithModelsUpgradeable
- **Proxy Address**: `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22`
- **Implementation**: Pending redeployment for Phase 18 per-model per-token pricing
- **Network**: Base Sepolia
- **Status**: ✅ ACTIVE - UUPS Upgradeable
- **ABI File**: `NodeRegistryWithModelsUpgradeable-CLIENT-ABI.json`
- **Key Features**:
  - All features from non-upgradeable version
  - UUPS proxy pattern for future upgrades
  - Owner-only upgrade authorization
  - Per-model per-token pricing via `setModelTokenPricing(modelId, token, price)`
- **Breaking Change (Feb 26, 2026) — Phase 18: Per-Model Per-Token Pricing Migration**:
  - **Removed functions**: `getNodePricing`, `setTokenPricing`, `updatePricingNative`, `updatePricingStable`, `setModelPricing`, `clearModelPricing`
  - **Added functions**: `setModelTokenPricing(bytes32 modelId, address token, uint256 price)`, `clearModelTokenPricing(bytes32 modelId, address token)`
  - **Changed**: `getModelPricing(operator, modelId, token)` now reads from `modelTokenPricing` only (no fallback); reverts with `"No model pricing"` if not set
  - **Changed**: `getHostModelPrices(operator, token)` now takes a `token` parameter and returns prices from `modelTokenPricing` mapping
  - Hosts **must** call `setModelTokenPricing(modelId, tokenAddress, price)` for each model+token combination they support
  - Supersedes Phase 17 (F202614977) which introduced `setTokenPricing`
- **Stake Slashing (Jan 16, 2026)**:
  - `slashStake(address, uint256, string, string)` - Slash host stake for misbehavior
  - `initializeSlashing(address)` - Initialize slashing after upgrade (owner, one-time)
  - `setSlashingAuthority(address)` - Set slashing authority (owner, for DAO migration)
  - `setTreasury(address)` - Set treasury for slashed tokens (owner)
  - `lastSlashTime(address)` - Query last slash timestamp for a host
  - Max 50% slash per action, 24-hour cooldown between slashes
  - Auto-unregisters host if stake falls below 100 FAB
- **Slashing Events**:
  - `SlashExecuted(host, amount, remainingStake, evidenceCID, reason, executor, timestamp)`
  - `HostAutoUnregistered(host, slashedAmount, returnedAmount, reason)`
  - `SlashingAuthorityUpdated(previousAuthority, newAuthority)`
  - `TreasuryUpdated(newTreasury)`
- **Previous Updates (Jan 10, 2026)**:
  - `repairCorruptNode(address)` - Owner-only function to fix corrupt node state from upgrades
  - `unregisterNode()` - Now handles corrupt state gracefully (safety check added)
  - `CorruptNodeRepaired(address indexed operator, uint256 stakeReturned)` event

### ModelRegistryUpgradeable
- **Proxy Address**: `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2`
- **Implementation**: `0xF12a0A07d4230E0b045dB22057433a9826d21652` ✅ Rejected fees + lateVotes cleanup + per-model rate limits (Feb 22, 2026)
- **Network**: Base Sepolia
- **Status**: ✅ ACTIVE - UUPS Upgradeable
- **ABI File**: `ModelRegistryUpgradeable-CLIENT-ABI.json`
- **Approved Models** (2 models):
  - TinyVicuna-1B-32k (CohereForAI/TinyVicuna-1B-32k-GGUF)
  - TinyLlama-1.1B Chat (TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF)
- **New Features (Jan 11, 2026) - Security Audit Remediation**:
  - **Anti-Sniping Vote Extension**: Large votes (≥10k FAB) in last 4 hours extend voting by 1 day (max 3 extensions)
  - **Re-proposal Cooldown**: Rejected models can be re-proposed after 30 days
  - New constants: `EXTENSION_THRESHOLD`, `EXTENSION_WINDOW`, `EXTENSION_DURATION`, `MAX_EXTENSIONS`, `REPROPOSAL_COOLDOWN`
  - New mappings: `lateVotes(bytes32)`, `lastProposalExecutionTime(bytes32)`
  - New event: `VotingExtended(bytes32 indexed modelId, uint256 newEndTime, uint8 extensionCount)`
  - `ModelProposal` struct now has `endTime` and `extensionCount` fields

### HostEarningsUpgradeable
- **Proxy Address**: `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0`
- **Implementation**: `0x8584AeAC9687613095D13EF7be4dE0A796F84D7a`
- **Network**: Base Sepolia
- **Status**: ✅ ACTIVE - UUPS Upgradeable
- **ABI File**: `HostEarningsUpgradeable-CLIENT-ABI.json`

### ProofSystemUpgradeable
- **Proxy Address**: `0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31`
- **Implementation**: `0xC46C84a612Cbf4C2eAaf5A9D1411aDA6309EC963` ✅ Dead code removal + storage placeholders (Feb 22, 2026)
- **Network**: Base Sepolia
- **Status**: ✅ ACTIVE - UUPS Upgradeable
- **ABI File**: `ProofSystemUpgradeable-CLIENT-ABI.json`
- **Audit Remediation (Feb 2026)**:
  - Dead circuit/verification code removed (F202615147, F202615149, F202615002, F202615064-66)
  - Only `markProofUsed()` and `setAuthorizedCaller()` remain
  - Storage placeholders preserve slot alignment for `authorizedCallers` at slot 3
  - `setAuthorizedCaller(address, bool)` - Owner authorizes callers
  - `authorizedCallers(address)` - Check if address is authorized

### Upgradeable Contracts Configuration

```javascript
// Use these addresses for all new integrations (Updated February 22, 2026)
const upgradeableContracts = {
  jobMarketplace: "0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4",  // FRESH PROXY (Feb 22, 2026)
  nodeRegistry: "0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22",
  modelRegistry: "0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2",
  hostEarnings: "0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0",
  proofSystem: "0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31",
  fabToken: "0xC78949004B4EB6dEf2D66e49Cd81231472612D62",
  usdcToken: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
};
```

### Corrupt Node Recovery (January 10, 2026)

If a host has node data but cannot unregister (corrupt state from contract upgrades):

**Option 1: Host calls `unregisterNode()` directly** (safety check now handles corrupt state):
```javascript
// Host can now unregister even with corrupt state
await nodeRegistry.unregisterNode();
// Stake is returned, node data is cleared
```

**Option 2: Owner calls `repairCorruptNode()`**:
```javascript
// Owner can repair corrupt nodes
await nodeRegistry.repairCorruptNode(corruptHostAddress);
// Stake is returned to the host, node data is cleared
// Emits CorruptNodeRepaired event
```

---

## LEGACY CONTRACTS (Deprecated - December 10, 2025)

> **WARNING**: These contracts are deprecated. Migrate to upgradeable versions above.

## Current Deployed Contracts (December 10, 2025 - Rate Limit Fix) ⚠️ DEPRECATED

### JobMarketplaceWithModels
- **Address**: 0x75C72e8C3eC707D8beF5Ba9b9C4f75CbB5bced97
- **Network**: Base Sepolia
- **Status**: ✅ PRICE_PRECISION=1000, 2000 tokens/sec rate limit (December 10, 2025)
- **Configuration**:
  - ProofSystem: 0x2ACcc60893872A499700908889B38C5420CBcFD1 ✅ SET
  - Authorized in HostEarnings: ✅ CONFIRMED
  - NodeRegistry: 0x906F4A8Cb944E4fe12Fb85Be7E627CeDAA8B8999 (PRICE_PRECISION support)
- **Key Features**:
  - 🆕 **Per-Model Pricing Support**: Query model-specific pricing with `getModelPricing()`
  - 🆕 **Model-Aware Sessions**: Create sessions tied to specific models
  - 🆕 **createSessionJobForModel()**: Native ETH session with model pricing validation
  - 🆕 **createSessionJobForModelWithToken()**: Token session with model pricing validation
  - 🆕 **sessionModel mapping**: Track which model is used for each session
  - 🆕 **100% Backward Compatible**: All existing SDK functions work unchanged
  - S5 Off-Chain Proof Storage: Full STARK proofs (221KB) stored in S5, only hash + CID on-chain
  - Transaction Size Reduction: 737x reduction (221KB → 300 bytes)
  - Cost Reduction: 5000x reduction (~$50 → ~$0.001 per proof)
  - 18-Field SessionJob Struct: lastProofHash and lastProofCID fields
  - submitProofOfWork takes 4 params (jobId, tokensClaimed, proofHash, proofCID)
  - DUAL PRICING: Separate native (ETH/BNB) and stable (USDC) pricing fields
  - On-chain MIN/MAX constants for both native and stable pricing
  - Price Validation: Validates against correct pricing field based on payment type
  - Works with NodeRegistryWithModels per-model pricing
  - Wallet-agnostic deposit/withdrawal functions (depositNative, withdrawNative)
  - createSessionFromDeposit for pre-funded session creation
  - Anyone-can-complete pattern for gasless session ending
  - ChainConfig support for multi-chain deployment (ETH on Base, BNB on opBNB)
  - MIN_DEPOSIT: 0.0002 ETH or 0.80 USDC minimum

### ModelRegistry (NEW - CORRECTED)
- **Address**: 0x92b2De840bB2171203011A6dBA928d855cA8183E
- **Network**: Base Sepolia
- **Status**: ✅ MODEL GOVERNANCE ENABLED
- **Purpose**: Manages approved AI models for the marketplace
- **Key Features**:
  - Two-tier approval system (owner-curated and community-voted)
  - SHA256 hash verification for model integrity
  - FAB token voting for community proposals
  - Emergency model deactivation capability
- **Approved Models** (3 models):
  - TinyVicuna-1B-32k (CohereForAI/TinyVicuna-1B-32k-GGUF)
  - TinyLlama-1.1B Chat (TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF)
  - OpenAI-GPT-OSS-20B (bartowski/openai_gpt-oss-20b-GGUF) - MXFP4 quantization

### NodeRegistryWithModels
- **Address**: 0x906F4A8Cb944E4fe12Fb85Be7E627CeDAA8B8999 ✅ NEW - PRICE_PRECISION=1000
- **Previous**: 0x48aa4A8047A45862Da8412FAB71ef66C17c7766d (Without PRICE_PRECISION - deprecated)
- **Network**: Base Sepolia
- **Status**: ✅ PRICE_PRECISION=1000 for sub-$1/million pricing (December 9, 2025)
- **Stake Required**: 1000 FAB tokens
- **Key Features**:
  - 🆕 **Per-Model Per-Token Pricing**: Set different prices for different AI models and tokens
  - 🆕 **setModelTokenPricing(bytes32 modelId, address token, uint256 price)**: Set per-model per-token pricing
  - 🆕 **clearModelTokenPricing(bytes32 modelId, address token)**: Clear per-model per-token pricing
  - 🆕 **getModelPricing(address, bytes32, address)**: Query per-model per-token pricing (reverts if not set)
  - 🆕 **getHostModelPrices(address, address)**: Batch query all model prices for a host and token
  - Dual Pricing: Separate minPricePerTokenNative and minPricePerTokenStable fields
  - Native Pricing Range: 227,273 to 22,727,272,727,273,000 wei
  - Stable Pricing Range: 1 to 100,000,000
  - Dynamic Pricing Updates: Separate updatePricingNative() and updatePricingStable() functions
  - 8-Field Node Struct: Both pricing fields (getNodeFullInfo returns 8 values)
  - Integrates with ModelRegistry for approved models only
  - Tracks which hosts support which models
  - Hosts must register with specific model IDs

### NodeRegistryFAB (DEPRECATED - Use NodeRegistryWithModels)
- **Address**: 0x2B745E45818e1dE570f253259dc46b91A82E3204
- **Previous**: 0x87516C13Ea2f99de598665e14cab64E191A0f8c4
- **Network**: Base Sepolia
- **Status**: ⚠️ DEPRECATED - Will be replaced by NodeRegistryWithModels
- **Stake Required**: 1000 FAB tokens
- **Note**: Still functional but lacks model governance

### ProofSystem
- **Address**: 0x2ACcc60893872A499700908889B38C5420CBcFD1
- **Network**: Base Sepolia
- **Status**: ✅ Configured (standby mode for disputes)
- **Purpose**: RISC0 STARK proof verification for dispute resolution
- **S5 Integration**: With S5 storage, proofs are verified on-chain only during disputes
- **Trust Model**: Hash commitment during normal operation, full verification on challenge
- **Requirement**: Proofs stored in S5 with SHA256 hash on-chain

### HostEarnings
- **Address**: 0x908962e8c6CE72610021586f85ebDE09aAc97776
- **Network**: Base Sepolia
- **Purpose**: Tracks accumulated earnings for hosts with batch withdrawal support
- **Authorized Marketplace**: 0x75C72e8C3eC707D8beF5Ba9b9C4f75CbB5bced97 ✅ UPDATED (2000 tok/sec Rate Limit - Dec 10, 2025)

## Model Registry Usage (NEW)

```javascript
import ModelRegistryABI from './ModelRegistry-CLIENT-ABI.json';
import NodeRegistryABI from './NodeRegistryWithModels-CLIENT-ABI.json';
import { ethers } from 'ethers';

const provider = new ethers.providers.JsonRpcProvider('https://base-sepolia.g.alchemy.com/v2/YOUR_KEY');

// Model Registry instance
const modelRegistry = new ethers.Contract(
  '0x92b2De840bB2171203011A6dBA928d855cA8183E',  // CORRECTED - Only 2 models
  ModelRegistryABI,
  provider
);

// Get model ID for registration
const modelId = await modelRegistry.getModelId(
  "CohereForAI/TinyVicuna-1B-32k-GGUF",
  "tiny-vicuna-1b.q4_k_m.gguf"
);

// Check if model is approved
const isApproved = await modelRegistry.isModelApproved(modelId);

// Get model details
const model = await modelRegistry.getModel(modelId);
console.log(`Model: ${model.huggingfaceRepo}/${model.fileName}`);
console.log(`SHA256: ${model.sha256Hash}`);
console.log(`Active: ${model.active}`);

// For hosts - register with approved models AND set per-model per-token pricing
const nodeRegistry = new ethers.Contract(
  '0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22',  // NodeRegistryWithModels - UUPS Proxy
  NodeRegistryABI,
  signer
);

const metadata = JSON.stringify({
  hardware: { gpu: "rtx-4090", vram: 24 },
  capabilities: ["inference", "streaming"],
  location: "us-east"
});

// Default pricing: separate native and stable (set during registration)
const minPriceNative = ethers.BigNumber.from("3000000000"); // 3B wei (~$0.000013 @ $4400 ETH)
const minPriceStable = 15000; // 0.015 USDC per token

await nodeRegistry.registerNode(
  metadata,
  "http://my-host.com:8080",
  [modelId], // Must be an approved model ID
  minPriceNative, // Default native token pricing (ETH/BNB)
  minPriceStable  // Default stablecoin pricing (USDC)
);

// REQUIRED: Set per-model per-token pricing for each model+token you accept
const usdcAddress = '0x036CbD53842c5426634e7929541eC2318f3dCF7e';
await nodeRegistry.setModelTokenPricing(modelId, usdcAddress, minPriceStable);
await nodeRegistry.setModelTokenPricing(modelId, ethers.constants.AddressZero, minPriceNative);
```

## Per-Model Per-Token Pricing (Phase 18 - February 26, 2026)

### For Hosts: Setting Per-Model Per-Token Pricing

```javascript
import NodeRegistryABI from './NodeRegistryWithModelsUpgradeable-CLIENT-ABI.json';
import { ethers } from 'ethers';

const nodeRegistry = new ethers.Contract(
  '0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22',
  NodeRegistryABI,
  signer
);

// Register node with models and default pricing
const minPriceNative = ethers.BigNumber.from("3000000000"); // 3B wei
const minPriceStable = 15000; // 0.015 USDC per token

await nodeRegistry.registerNode(
  metadata,
  apiUrl,
  [modelId],
  minPriceNative,
  minPriceStable
);

// Set per-model per-token pricing (REQUIRED for each model+token combination)
const usdcAddress = '0x036CbD53842c5426634e7929541eC2318f3dCF7e';

// Set USDC pricing for a specific model
await nodeRegistry.setModelTokenPricing(modelId, usdcAddress, 15000);

// Set native ETH pricing for a specific model
await nodeRegistry.setModelTokenPricing(modelId, ethers.constants.AddressZero, ethers.BigNumber.from("3000000000"));

// Clear pricing for a model+token combination
await nodeRegistry.clearModelTokenPricing(modelId, usdcAddress);

// Query model pricing for a specific token
const price = await nodeRegistry.getModelPricing(myAddress, modelId, usdcAddress);
console.log(`Model price for USDC: ${price}`);

// Batch query all model prices for a specific token
const [modelIds, prices] = await nodeRegistry.getHostModelPrices(myAddress, usdcAddress);
for (let i = 0; i < modelIds.length; i++) {
  console.log(`Model ${modelIds[i]}: ${prices[i]}`);
}
```

### For Clients: Querying Pricing and Creating Sessions

```javascript
import JobMarketplaceABI from './JobMarketplaceWithModelsUpgradeable-CLIENT-ABI.json';
import NodeRegistryABI from './NodeRegistryWithModelsUpgradeable-CLIENT-ABI.json';
import { ethers } from 'ethers';

const nodeRegistry = new ethers.Contract(
  '0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22',
  NodeRegistryABI,
  provider
);

const marketplace = new ethers.Contract(
  '0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4',
  JobMarketplaceABI,
  signer
);

// STEP 1: Query per-model pricing for the specific token
const hostAddress = '0x...';
const modelId = '0x...'; // bytes32 model hash
const usdcAddress = '0x036CbD53842c5426634e7929541eC2318f3dCF7e';

try {
  const hostPrice = await nodeRegistry.getModelPricing(hostAddress, modelId, usdcAddress);
  console.log(`Host price for this model+token: ${hostPrice}`);
} catch (e) {
  console.log("Host has not set pricing for this model+token combination");
}

// Or batch query all model prices for a token
const [modelIds, prices] = await nodeRegistry.getHostModelPrices(hostAddress, usdcAddress);

// STEP 2a: Create ETH model session
const myPriceNative = ethers.BigNumber.from("4000000000");
const deposit = ethers.utils.parseEther('0.1');
const proofTimeoutWindow = 300; // 5 minutes

const txNative = await marketplace.createSessionJobForModel(
  modelId,
  hostAddress,
  myPriceNative,
  3600, // 1 hour max duration
  100,  // Proof every 100 tokens
  proofTimeoutWindow,
  { value: deposit }
);

await txNative.wait();
console.log('ETH model session created!');

// STEP 2b: Create USDC model session
const myPriceStable = 20000;
const usdcDeposit = ethers.utils.parseUnits("10", 6);

// Approve USDC first
const usdcContract = new ethers.Contract(
  usdcAddress,
  ['function approve(address,uint256)'],
  signer
);
await usdcContract.approve(marketplace.address, usdcDeposit);

const txStable = await marketplace.createSessionJobForModelWithToken(
  modelId,
  hostAddress,
  usdcAddress,
  usdcDeposit,
  myPriceStable,
  3600,
  100,
  proofTimeoutWindow
);

await txStable.wait();
console.log('USDC model session created!');
```

### Pricing Validation Rules

All session creation functions now require a model ID and validate against per-model per-token pricing:

**Native Token Sessions** (ETH/BNB):
- `createSessionJobForModel()` - Validates against `modelTokenPricing[host][modelId][address(0)]`
- **Range**: 227,273 to 22,727,272,727,273,000 wei

**Stablecoin Sessions** (USDC):
- `createSessionJobForModelWithToken()` - Validates against `modelTokenPricing[host][modelId][token]`
- **Range**: 1 to 100,000,000

**If model pricing not set**: `getModelPricing()` reverts with `"No model pricing"`
```

## S5 Proof Storage Usage (NEW - October 14, 2025)

### The Problem That Was Solved

STARK proofs generated by RISC0 are approximately **221KB in size**, which exceeds the Base Sepolia RPC transaction limit of **128KB**. This caused **100% of proof submissions to fail** with "oversized data" errors, making the entire proof system non-functional.

### The S5 Solution

**NEW Architecture**: Store full proofs in S5 decentralized storage, submit only hash + CID on-chain.

```solidity
// OLD (Pre-S5) - FAILED due to 221KB > 128KB limit
function submitProofOfWork(
    uint256 jobId,
    bytes calldata ekzlProof,  // ❌ 221KB - exceeds RPC limit
    uint256 tokensInBatch
) external

// NEW (S5 Storage) - SUCCESS with 300 bytes
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,       // ✅ 32 bytes - SHA256 hash
    string calldata proofCID // ✅ S5 CID for retrieval (~50-100 bytes)
) external
```

**Benefits**:
- ✅ Transaction size: 221KB → ~300 bytes (737x reduction)
- ✅ Storage cost: ~$50 → ~$0.001 per proof (5000x cheaper)
- ✅ Proof submission success rate: 0% → 100%
- ✅ Proof integrity: SHA256 hash prevents tampering
- ✅ Proof availability: S5 decentralized storage ensures retrieval

### For Hosts: S5 Proof Submission Workflow

```javascript
import JobMarketplaceABI from './JobMarketplaceWithModels-CLIENT-ABI.json';
import { S5Client } from '@lumeweb/s5-js';
import crypto from 'crypto';
import { ethers } from 'ethers';

const s5 = new S5Client('https://s5.lumeweb.com');
const marketplace = new ethers.Contract(
  '0x75C72e8C3eC707D8beF5Ba9b9C4f75CbB5bced97',
  JobMarketplaceABI,
  signer
);

// STEP 1: Generate RISC0 STARK proof (221KB) - your existing proof generation
const proofBytes = await generateRisc0Proof(jobData);
console.log(`Generated proof: ${proofBytes.length} bytes`);

// STEP 2: Upload proof to S5 decentralized storage
console.log('📤 Uploading proof to S5...');
const proofCID = await s5.uploadBlob(proofBytes);
console.log(`✅ Proof uploaded to S5: CID=${proofCID}`);

// STEP 3: Calculate SHA256 hash of proof
const proofHash = '0x' + crypto
    .createHash('sha256')
    .update(proofBytes)
    .digest('hex');
console.log(`📊 Proof hash: ${proofHash}`);

// STEP 4: Submit hash + CID to blockchain (NOT full proof)
const tx = await marketplace.submitProofOfWork(
    jobId,
    tokensClaimed,  // e.g., 1000 tokens
    proofHash,      // bytes32 - 32 bytes
    proofCID        // string - S5 CID
);

await tx.wait();
console.log('✅ Proof submitted successfully (only hash + CID on-chain)');

// The session's tokensConsumed field is now updated
// Payment will be calculated on session completion
```

### Complete Host Integration Example

```javascript
import { S5Client } from '@lumeweb/s5-js';
import crypto from 'crypto';
import { ethers } from 'ethers';

const s5 = new S5Client('https://s5.lumeweb.com');
const marketplace = new ethers.Contract(
  '0x75C72e8C3eC707D8beF5Ba9b9C4f75CbB5bced97',
  JobMarketplaceABI,
  signer
);

// Get session details
const session = await marketplace.sessionJobs(jobId);
console.log(`Session price: ${session.pricePerToken} per token`);
console.log(`Checkpoint interval: ${session.proofInterval}`);

// Track tokens and submit S5 proofs at checkpoints
let tokenCount = 0;
for (const prompt of prompts) {
    const response = await model.generate(prompt);
    tokenCount += response.tokenCount;

    // Submit proof at checkpoint
    if (tokenCount >= session.proofInterval) {
        // Generate STARK proof (221KB)
        const proof = await generateRisc0Proof(tokenCount);

        // Upload to S5
        const proofCID = await s5.uploadBlob(proof);

        // Calculate hash
        const proofHash = '0x' + crypto.createHash('sha256').update(proof).digest('hex');

        // Submit hash + CID (NOT full proof)
        await marketplace.submitProofOfWork(jobId, tokenCount, proofHash, proofCID);
        console.log(`✅ Checkpoint: ${tokenCount} tokens verified via S5`);

        tokenCount = 0; // Reset for next batch
    }
}

// Submit final proof if any remaining tokens
if (tokenCount > 0) {
    const finalProof = await generateRisc0Proof(tokenCount);
    const proofCID = await s5.uploadBlob(finalProof);
    const proofHash = '0x' + crypto.createHash('sha256').update(finalProof).digest('hex');
    await marketplace.submitProofOfWork(jobId, tokenCount, proofHash, proofCID);
}
```

### Proof Verification (For Disputes)

```javascript
import { S5Client } from '@lumeweb/s5-js';
import crypto from 'crypto';

// In normal operation, proofs are NOT verified on-chain
// Verification only happens during disputes

async function verifyProofIntegrity(jobId) {
    const s5 = new S5Client('https://s5.lumeweb.com');

    // 1. Get proof CID and hash from session
    const session = await marketplace.sessionJobs(jobId);
    const storedCID = session.lastProofCID;
    const storedHash = session.lastProofHash;

    console.log(`Stored hash: ${storedHash}`);
    console.log(`Stored CID: ${storedCID}`);

    // 2. Retrieve proof from S5
    console.log('📥 Downloading proof from S5...');
    const retrievedProof = await s5.downloadBlob(storedCID);

    // 3. Calculate hash of retrieved proof
    const calculatedHash = '0x' + crypto
        .createHash('sha256')
        .update(retrievedProof)
        .digest('hex');

    // 4. Verify integrity
    if (calculatedHash !== storedHash) {
        throw new Error('⚠️ Proof integrity check failed!');
    }

    console.log('✅ Proof integrity verified');

    // 5. Optionally verify proof validity via ProofSystem
    // (only needed in disputes)
    // await proofSystem.verifyProof(retrievedProof);
}
```

### SessionJob Struct (18 Fields)

With S5 storage, the SessionJob struct has been expanded to 18 fields:

```solidity
struct SessionJob {
    address depositor;          // Who created and funded the session
    address host;               // Assigned host
    uint256 deposit;            // Total deposit amount
    uint256 pricePerToken;      // Price per AI token
    uint256 maxDuration;        // Session timeout
    uint256 proofInterval;      // Tokens between proofs
    uint256 tokensConsumed;     // Total tokens consumed (via S5 proofs)
    uint256 createdAt;          // Creation timestamp
    bool active;                // Session status
    string conversationCID;     // S5 CID after completion
    address paymentToken;       // Payment token (0x0 for ETH)
    uint256 lastProofTimestamp; // Last proof submission time
    uint256 totalPayment;       // Total paid to host
    uint256 hostEarnings;       // Host's earnings from session
    uint256 treasuryFee;        // Treasury fee from session
    bytes32 modelHash;          // Approved model identifier
    bytes32 lastProofHash;      // ✅ NEW: SHA256 hash of most recent proof
    string lastProofCID;        // ✅ NEW: S5 CID for proof retrieval
}
```

### Reading Session Data with S5 Fields

```javascript
const session = await marketplace.sessionJobs(jobId);

// All 18 fields
console.log('Depositor:', session.depositor);
console.log('Host:', session.host);
console.log('Tokens consumed:', session.tokensConsumed);
console.log('Last proof hash:', session.lastProofHash);    // NEW field
console.log('Last proof CID:', session.lastProofCID);      // NEW field
console.log('Active:', session.active);
```

## API Discovery Usage

```javascript
import NodeRegistryABI from './NodeRegistryFAB-CLIENT-ABI.json';
import { ethers } from 'ethers';

const provider = new ethers.providers.JsonRpcProvider('https://base-sepolia.g.alchemy.com/v2/YOUR_KEY');
const nodeRegistry = new ethers.Contract(
  '0x906F4A8Cb944E4fe12Fb85Be7E627CeDAA8B8999',
  NodeRegistryABI,
  provider
);

// For hosts - register with API URL
const signer = provider.getSigner();
const registryWithSigner = nodeRegistry.connect(signer);
await registryWithSigner.registerNodeWithUrl(
  'llama-2-7b,gpt-4,inference',
  'http://your-host.com:8080'
);

// For hosts - update API URL
await registryWithSigner.updateApiUrl('https://new-host.com:8443');

// For clients - discover host endpoints
const apiUrl = await nodeRegistry.getNodeApiUrl(hostAddress);
console.log(`Host API endpoint: ${apiUrl}`);

// Get full host information
const [operator, stakedAmount, active, metadata, apiUrl] = await nodeRegistry.getNodeFullInfo(hostAddress);
```

## Usage Example

```javascript
import JobMarketplaceABI from './JobMarketplaceFABWithS5Deploy-CLIENT-ABI.json';
import NodeRegistryABI from './NodeRegistryFAB-CLIENT-ABI.json';
import HostEarningsABI from './HostEarnings-CLIENT-ABI.json';
import { ethers } from 'ethers';

// Initialize provider
const provider = new ethers.providers.JsonRpcProvider('https://base-sepolia.g.alchemy.com/v2/YOUR_KEY');

// Create contract instances
const marketplace = new ethers.Contract(
  '0x75C72e8C3eC707D8beF5Ba9b9C4f75CbB5bced97', // CURRENT - PRICE_PRECISION + 2000 tok/sec
  JobMarketplaceABI,
  provider
);

const nodeRegistry = new ethers.Contract(
  '0x906F4A8Cb944E4fe12Fb85Be7E627CeDAA8B8999', // NodeRegistryWithModels - PRICE_PRECISION=1000
  NodeRegistryABI,
  provider
);

const hostEarnings = new ethers.Contract(
  '0x908962e8c6CE72610021586f85ebDE09aAc97776',
  HostEarningsABI,
  provider
);

// Discover host API endpoint
const hostApiUrl = await nodeRegistry.getNodeApiUrl(hostAddress);
if (!hostApiUrl) {
  throw new Error('Host has not set API URL');
}

// Create USDC model session job (with signer)
const signer = provider.getSigner();
const marketplaceWithSigner = marketplace.connect(signer);

// First approve USDC
const usdcAddress = '0x036CbD53842c5426634e7929541eC2318f3dCF7e';
const usdcContract = new ethers.Contract(usdcAddress, ['function approve(address,uint256)'], signer);
await usdcContract.approve(marketplace.address, ethers.utils.parseUnits("10", 6)); // 10 USDC

// Create model session with USDC (all sessions now require a model ID)
const proofTimeoutWindow = 300; // 5 minutes
await marketplaceWithSigner.createSessionJobForModelWithToken(
  modelId,       // bytes32 model hash
  hostAddress,
  usdcAddress,
  ethers.utils.parseUnits("10", 6), // 10 USDC deposit
  ethers.utils.parseUnits("0.001", 6), // price per token in USDC
  3600, // max duration (1 hour)
  300,  // proof interval
  proofTimeoutWindow
);

// Now connect to host using discovered API URL
const response = await fetch(`${hostApiUrl}/api/v1/inference`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({
    model: 'llama-2-7b',
    prompt: 'Hello, world!',
    jobId: jobId
  })
});
```

## Session Job Functions

Key functions for session jobs:
- `createSessionJobForModel()` - Create ETH-based model session
- `createSessionJobForModelWithToken()` - Create token-based model session
- `createSessionFromDepositForModel()` - Create model session from pre-deposited funds
- `createSessionForModelAsDelegate()` - Delegate creates model session on behalf of depositor
- `submitProofOfWork(sessionId, tokensClaimed, proofHash, proofCID, deltaCID)` - Submit proof (5 params, no signature)
- `getProofSubmission(sessionId, proofIndex)` - Get proof details (returns 5 values including deltaCID)
- `completeSessionJob(jobId, conversationCID)` - Complete and settle payments
- `triggerSessionTimeout()` - Handle timeout scenarios

**REMOVED (Feb 26, 2026)**: `createSessionJob()`, `createSessionJobWithToken()`, `createSessionFromDeposit()` — use model-specific variants above.

**BREAKING CHANGE (Feb 22, 2026)**: `submitProofOfWork()` signature parameter removed:
- Current: `submitProofOfWork(sessionId, tokensClaimed, proofHash, proofCID, deltaCID)` - 5 params

```javascript
// Proof submission (no signature required — msg.sender == host is verified on-chain)
const proofHash = keccak256(workData);
const deltaCID = "QmDeltaCID123"; // or "" if not using delta tracking
await marketplace.submitProofOfWork(sessionId, tokensClaimed, proofHash, proofCID, deltaCID);
```

## Treasury Functions

For treasury address only:
- `withdrawTreasuryETH()` - Withdraw accumulated ETH fees
- `withdrawTreasuryTokens(address token)` - Withdraw accumulated token fees
- `withdrawAllTreasuryFees(address[] tokens)` - Batch withdraw ETH + multiple tokens
- `accumulatedTreasuryETH()` - View accumulated ETH fees
- `accumulatedTreasuryTokens(address token)` - View accumulated token fees

## Constants

- `MIN_DEPOSIT`: 100000000000000 wei (0.0001 ETH ~$0.50)
- `USDC_MIN_DEPOSIT`: 500000 (0.50 USDC)
- `MIN_PROVEN_TOKENS`: 100
- `TREASURY_FEE_PERCENT`: 2.5
- `MIN_SESSION_DURATION`: 1 hour
- `ABANDONMENT_TIMEOUT`: 24 hours
- `DISPUTE_WINDOW`: 1 hour

## Migration Notes

### Breaking Changes
- NodeRegistry `nodes()` now returns 5 fields instead of 4 (added `apiUrl`)
- JobMarketplace contracts must handle the 5-field struct

### For Existing Hosts
If you're already registered, add your API URL:
```javascript
await nodeRegistry.updateApiUrl('http://your-host.com:8080');
```

### For SDK Developers
Update contract addresses and integrate S5 proof storage:
```javascript
const NODE_REGISTRY = '0x906F4A8Cb944E4fe12Fb85Be7E627CeDAA8B8999'; // PRICE_PRECISION=1000
const JOB_MARKETPLACE = '0x75C72e8C3eC707D8beF5Ba9b9C4f75CbB5bced97'; // ✅ PRICE_PRECISION + 2000 tok/sec (Dec 10, 2025)
const MODEL_REGISTRY = '0x92b2De840bB2171203011A6dBA928d855cA8183E';
const PROOF_SYSTEM = '0x2ACcc60893872A499700908889B38C5420CBcFD1';
const HOST_EARNINGS = '0x908962e8c6CE72610021586f85ebDE09aAc97776';

// IMPORTANT: Install S5 client for proof submission
// npm install @lumeweb/s5-js
```

**Breaking Changes for SDK Integration**:
1. **submitProofOfWork()** signature changed from 3 to 4 parameters
2. **SessionJob struct** expanded from 16 to 18 fields (added lastProofHash, lastProofCID)
3. **S5 client required** for proof upload/retrieval
4. **SHA256 hashing** required for proof integrity

## Deprecated Contracts

### JobMarketplaceWithModels (200 tok/sec Rate Limit)
- **Address**: 0x5497a28B4bE6b1219F93e6Fcd631E8aCdC173709
- **Deprecated**: December 10, 2025
- **Reason**: Rate limit of 200 tokens/sec still too slow for small models on high-end GPUs (800-1500 tok/sec)
- **Replacement**: 0x75C72e8C3eC707D8beF5Ba9b9C4f75CbB5bced97
- **Migration Required**: Update contract address in SDK config

### JobMarketplaceWithModels (20 tok/sec Rate Limit)
- **Address**: 0xfD764804C5A5808b79D66746BAF4B65fb4413731
- **Deprecated**: December 10, 2025
- **Reason**: Rate limit of 20 tokens/sec was too slow for real LLM inference (50-100+ tokens/sec)
- **Replacement**: 0x5497a28B4bE6b1219F93e6Fcd631E8aCdC173709 (also deprecated)
- **Migration Required**: Update contract address in SDK config

### JobMarketplaceWithModels (Pre-S5 Storage)
- **Address**: 0xe169A4B57700080725f9553E3Cc69885fea13629
- **Deprecated**: October 14, 2025
- **Reason**: Replaced by S5 proof storage (on-chain proofs exceeded 128KB RPC limit, causing 100% failure rate)
- **Replacement**: 0xfD764804C5A5808b79D66746BAF4B65fb4413731 (also deprecated)
- **Migration Required**: Update to S5 proof submission workflow (4-parameter submitProofOfWork)

### JobMarketplaceWithModels (Incorrect MAX_PRICE_NATIVE)
- **Address**: 0x462050a4a551c4292586D9c1DE23e3158a9bF3B3
- **Deprecated**: January 28, 2025
- **Reason**: Incorrect MAX_PRICE_NATIVE (only 10x range instead of 10,000x)
- **Replacement**: 0xe169A4B57700080725f9553E3Cc69885fea13629 (also deprecated - see above)

### NodeRegistryWithModels (Incorrect MAX_PRICE_NATIVE)
- **Address**: 0xC8dDD546e0993eEB4Df03591208aEDF6336342D7
- **Deprecated**: January 28, 2025
- **Reason**: Incorrect MAX_PRICE_NATIVE (only 10x range instead of 10,000x)
- **Replacement**: 0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6

## Last Updated
February 26, 2026 - Phase 18 per-model per-token pricing migration

### Recent Changes
- **Feb 26, 2026**: **Phase 18 — Per-model per-token pricing migration**. Removed `getNodePricing`, `setTokenPricing`, `updatePricingNative`, `updatePricingStable`, `setModelPricing`, `clearModelPricing` from NodeRegistry. Added `setModelTokenPricing(modelId, token, price)`, `clearModelTokenPricing(modelId, token)`. `getModelPricing` now reads from `modelTokenPricing` only (reverts with `"No model pricing"` if not set). `getHostModelPrices(operator, token)` now takes a `token` parameter. Removed `createSessionJob`, `createSessionJobWithToken`, `createSessionFromDeposit` from JobMarketplace (use model-specific variants). Supersedes Phase 17 (F202614977).
- **Feb 24, 2026**: ~~**F202614977 per-token pricing**~~ **Superseded by Phase 18** — NodeRegistry upgraded to `0xeeB3...D24b`. `getNodePricing()` and `getModelPricing()` reverted with `"No token pricing"` for ERC20 tokens without explicit `setTokenPricing()`. Now replaced by per-model per-token pricing.
- **Feb 22, 2026**: **Post-audit remediation** — Fresh JM proxy `0xD067...adA4`, all 20 Hacken audit findings addressed. Signature removed from submitProofOfWork, proofTimeoutWindow added, per-model rate limits, early cancellation fees, pull-pattern refunds, shortened require strings (F202615067). ProofSystem and ModelRegistry implementations upgraded.
- **Jan 16, 2026**: Stake slashing - `slashStake()`, `initializeSlashing()`, `setSlashingAuthority()`, `setTreasury()`, `lastSlashTime()`
- **Jan 14, 2026**: deltaCID support - `submitProofOfWork` now 6 params, `getProofSubmission` returns 5 values
- **Jan 10, 2026**: NodeRegistry - Added `repairCorruptNode()` admin function and safety check in `unregisterNode()`
- **Jan 9, 2026**: Clean slate JobMarketplace deployment - new proxy address `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E`
- **Jan 6, 2026**: Phase 6 ProofSystem Integration - submitProofOfWork signature changed to 5 params

### PRICE_PRECISION Breaking Change

With PRICE_PRECISION=1000, all prices are now stored with 1000x multiplier:

| USD Price/Million | OLD pricePerToken | NEW pricePerToken |
|-------------------|-------------------|-------------------|
| $0.06/million | Not supported | 60 |
| $0.27/million | Not supported | 270 |
| $1/million | 1 | 1,000 |
| $5/million | 5 | 5,000 |
| $10/million | 10 | 10,000 |

See `/docs/BREAKING_CHANGES.md` for full migration guide.