# Client ABIs

This directory contains the Application Binary Interfaces (ABIs) for client integration.

## Current Deployed Contracts (January 28, 2025 - Corrected Dual Pricing)

### JobMarketplaceWithModels
- **Address**: 0xe169A4B57700080725f9553E3Cc69885fea13629
- **Network**: Base Sepolia
- **Status**: ✅ DUAL PRICING WITH 10,000x RANGE
- **Configuration**:
  - ProofSystem: 0x2ACcc60893872A499700908889B38C5420CBcFD1 ✅ SET
  - Authorized in HostEarnings: ✅ CONFIRMED
  - NodeRegistry: 0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6 (8-field struct with dual pricing)
- **Key Features**:
  - 🆕 **DUAL PRICING**: Separate native (ETH/BNB) and stable (USDC) pricing fields
  - 🆕 **10,000x Range**: Both native and stable have 10,000x range (MIN to MAX)
  - 🆕 **Price Validation**: Validates against correct pricing field based on payment type
  - 🆕 **Query Pricing**: Get host pricing for both native and stable tokens
  - 🆕 Works with NodeRegistryWithModels 8-field struct (includes both pricing fields)
  - Wallet-agnostic deposit/withdrawal functions (depositNative, withdrawNative)
  - createSessionFromDeposit for pre-funded session creation
  - Anyone-can-complete pattern for gasless session ending
  - ChainConfig support for multi-chain deployment (ETH on Base, BNB on opBNB)
  - Enhanced event indexing for better filtering
  - depositor field tracks who paid (EOA or Smart Account)
  - Properly calls creditEarnings() for host balance tracking
  - Validates hosts have supported models
  - User refunds fixed for session jobs
  - Treasury fee accumulation for batch withdrawals (90% host / 10% treasury)
  - Host earnings accumulation WITH PROPER TRACKING
  - USDC payment settlement with proper split distribution
  - ETH and USDC payment support fully functional
  - Direct payment distribution (no external escrow)
  - Session jobs with proof checkpoints
  - EZKL proof verification integration
  - MIN_DEPOSIT: 0.0002 ETH or 0.80 USDC minimum
  - MIN_PROVEN_TOKENS: 100 tokens minimum
  - Total gas savings: ~80%

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
- **Approved Models** (MVP Testing - ONLY THESE TWO):
  - TinyVicuna-1B-32k (CohereForAI/TinyVicuna-1B-32k-GGUF)
  - TinyLlama-1.1B Chat (TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF)

### NodeRegistryWithModels
- **Address**: 0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6 ✅ NEW - Corrected Dual Pricing
- **Previous**: 0xC8dDD546e0993eEB4Df03591208aEDF6336342D7 (Incorrect MAX_PRICE_NATIVE - deprecated)
- **Network**: Base Sepolia
- **Status**: ✅ DUAL PRICING WITH 10,000x RANGE
- **Stake Required**: 1000 FAB tokens
- **Key Features**:
  - 🆕 **Dual Pricing**: Separate minPricePerTokenNative and minPricePerTokenStable fields
  - 🆕 **Native Pricing Range**: 2,272,727,273 to 22,727,272,727,273 wei (~$0.00001 to $0.1 @ $4400 ETH)
  - 🆕 **Stable Pricing Range**: 10 to 100,000 (0.00001 to 0.1 USDC per token)
  - 🆕 **Dynamic Pricing Updates**: Separate updatePricingNative() and updatePricingStable() functions
  - 🆕 **Price Discovery**: Query both native and stable pricing separately
  - 🆕 **8-Field Node Struct**: Added both pricing fields (getNodeFullInfo returns 8 values)
  - 🆕 **PricingUpdated Events**: Separate events for native and stable pricing changes
  - Integrates with ModelRegistry for approved models only
  - Structured JSON metadata format
  - Tracks which hosts support which models
  - API endpoint discovery (inherited from previous version)
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
- **Purpose**: EZKL proof verification for trustless AI inference
- **Fixed**: Internal verification function call for USDC sessions
- **Requirement**: Minimum 64-byte proofs

### HostEarnings
- **Address**: 0x908962e8c6CE72610021586f85ebDE09aAc97776
- **Network**: Base Sepolia
- **Purpose**: Tracks accumulated earnings for hosts with batch withdrawal support
- **Authorized Marketplace**: 0xe169A4B57700080725f9553E3Cc69885fea13629 ✅ UPDATED (Corrected Dual Pricing)

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

// For hosts - register with approved models AND dual pricing
const nodeRegistry = new ethers.Contract(
  '0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6',  // NodeRegistryWithModels - Corrected dual pricing
  NodeRegistryABI,
  signer
);

const metadata = JSON.stringify({
  hardware: { gpu: "rtx-4090", vram: 24 },
  capabilities: ["inference", "streaming"],
  location: "us-east"
});

// Dual pricing: separate native and stable
const minPriceNative = ethers.BigNumber.from("3000000000"); // 3B wei (~$0.000013 @ $4400 ETH)
const minPriceStable = 15000; // 0.015 USDC per token

await nodeRegistry.registerNode(
  metadata,
  "http://my-host.com:8080",
  [modelId], // Must be an approved model ID
  minPriceNative, // Native token pricing (ETH/BNB)
  minPriceStable  // Stablecoin pricing (USDC)
);
```

## Dual Pricing Usage (NEW - January 28, 2025)

### For Hosts: Setting and Updating Dual Pricing

```javascript
import NodeRegistryABI from './NodeRegistryWithModels-CLIENT-ABI.json';
import { ethers } from 'ethers';

const nodeRegistry = new ethers.Contract(
  '0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6',
  NodeRegistryABI,
  signer
);

// Register with DUAL pricing (both required)
const minPriceNative = ethers.BigNumber.from("3000000000"); // 3B wei
const minPriceStable = 15000; // 0.015 USDC per token

// Native range: 2,272,727,273 to 22,727,272,727,273 wei (~$0.00001 to $0.1 @ $4400 ETH)
// Stable range: 10 to 100,000 (0.00001 to 0.1 USDC per token)

await nodeRegistry.registerNode(
  metadata,
  apiUrl,
  [modelId],
  minPriceNative,
  minPriceStable
);

// Update native pricing (ETH/BNB)
const newPriceNative = ethers.BigNumber.from("5000000000"); // 5B wei
await nodeRegistry.updatePricingNative(newPriceNative);

// Update stable pricing (USDC)
const newPriceStable = 20000; // 0.02 USDC per token
await nodeRegistry.updatePricingStable(newPriceStable);

// Query own pricing (returns both)
const [nativePrice, stablePrice] = await nodeRegistry.getNodePricing(myAddress);
console.log(`Native: ${nativePrice.toString()} wei`);
console.log(`Stable: ${stablePrice} (${stablePrice / 1e6} USDC per token)`);
```

### For Clients: Querying Dual Pricing and Creating Sessions

```javascript
import JobMarketplaceABI from './JobMarketplaceWithModels-CLIENT-ABI.json';
import NodeRegistryABI from './NodeRegistryWithModels-CLIENT-ABI.json';
import { ethers } from 'ethers';

const nodeRegistry = new ethers.Contract(
  '0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6',
  NodeRegistryABI,
  provider
);

const marketplace = new ethers.Contract(
  '0xe169A4B57700080725f9553E3Cc69885fea13629',
  JobMarketplaceABI,
  signer
);

// STEP 1: Query host dual pricing BEFORE creating session
const hostAddress = '0x...';
const [hostMinNative, hostMinStable] = await nodeRegistry.getNodePricing(hostAddress);
console.log(`Host native minimum: ${hostMinNative.toString()} wei`);
console.log(`Host stable minimum: ${hostMinStable} (${hostMinStable / 1e6} USDC)`);

// Or get all host info including dual pricing (7th and 8th fields)
const [operator, stake, active, metadata, apiUrl, models, minNative, minStable] =
  await nodeRegistry.getNodeFullInfo(hostAddress);

// STEP 2a: Create ETH session with price >= host native minimum
const myPriceNative = ethers.BigNumber.from("4000000000"); // Must be >= hostMinNative
const deposit = ethers.utils.parseEther('0.1'); // 0.1 ETH

// This will REVERT if myPriceNative < hostMinNative
const txNative = await marketplace.createSessionJob(
  hostAddress,
  myPriceNative,
  3600, // 1 hour max duration
  100,  // Proof every 100 tokens
  { value: deposit }
);

await txNative.wait();
console.log('ETH session created with validated native pricing!');

// STEP 2b: Create USDC session with price >= host stable minimum
const myPriceStable = 20000; // Must be >= hostMinStable
const usdcDeposit = ethers.utils.parseUnits("10", 6); // 10 USDC

// Approve USDC first
const usdcContract = new ethers.Contract(
  '0x036CbD53842c5426634e7929541eC2318f3dCF7e',
  ['function approve(address,uint256)'],
  signer
);
await usdcContract.approve(marketplace.address, usdcDeposit);

// This will REVERT if myPriceStable < hostMinStable
const txStable = await marketplace.createSessionJobWithToken(
  hostAddress,
  '0x036CbD53842c5426634e7929541eC2318f3dCF7e', // USDC
  usdcDeposit,
  myPriceStable,
  3600,
  100
);

await txStable.wait();
console.log('USDC session created with validated stable pricing!');
```

### Dual Pricing Validation Rules

⚠️ **CRITICAL**: All session creation functions validate against the CORRECT pricing field:

**Native Token Sessions** (ETH/BNB):
- `createSessionJob()` - Validates against `hostMinPricePerTokenNative`
- `createSessionFromDeposit()` with ETH - Validates against native pricing
- **Range**: 2,272,727,273 to 22,727,272,727,273 wei (~$0.00001 to $0.1 @ $4400 ETH)
- **Validation**: `clientPricePerToken >= hostMinPricePerTokenNative`

**Stablecoin Sessions** (USDC):
- `createSessionJobWithToken()` - Validates against `hostMinPricePerTokenStable`
- `createSessionFromDeposit()` with USDC - Validates against stable pricing
- **Range**: 10 to 100,000 (0.00001 to 0.1 USDC per token)
- **Validation**: `clientPricePerToken >= hostMinPricePerTokenStable`

**If validation fails**: Transaction reverts with:
- "Price below host minimum (native)" - for ETH sessions
- "Price below host minimum (stable)" - for USDC sessions

**10,000x Range**: Both native and stable pricing have identical 10,000x range (MIN to MAX)
```

## API Discovery Usage

```javascript
import NodeRegistryABI from './NodeRegistryFAB-CLIENT-ABI.json';
import { ethers } from 'ethers';

const provider = new ethers.providers.JsonRpcProvider('https://base-sepolia.g.alchemy.com/v2/YOUR_KEY');
const nodeRegistry = new ethers.Contract(
  '0x2B745E45818e1dE570f253259dc46b91A82E3204',
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
  '0xe169A4B57700080725f9553E3Cc69885fea13629', // CURRENT deployment with corrected dual pricing
  JobMarketplaceABI,
  provider
);

const nodeRegistry = new ethers.Contract(
  '0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6', // NodeRegistryWithModels with corrected dual pricing
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

// Create USDC session job (with signer)
const signer = provider.getSigner();
const marketplaceWithSigner = marketplace.connect(signer);

// First approve USDC
const usdcAddress = '0x036CbD53842c5426634e7929541eC2318f3dCF7e';
const usdcContract = new ethers.Contract(usdcAddress, ['function approve(address,uint256)'], signer);
await usdcContract.approve(marketplace.address, ethers.utils.parseUnits("10", 6)); // 10 USDC

// Create session with USDC
await marketplaceWithSigner.createSessionJobWithToken(
  hostAddress,
  usdcAddress,
  ethers.utils.parseUnits("10", 6), // 10 USDC deposit
  ethers.utils.parseUnits("0.001", 6), // price per token in USDC
  3600, // max duration (1 hour)
  300 // proof interval
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
- `createSessionJob()` - Create ETH-based session
- `createSessionJobWithToken()` - Create token-based session
- `submitProofOfWork()` - Submit proof with minimum 100 tokens
- `completeSessionJob()` - Complete and settle payments
- `triggerSessionTimeout()` - Handle timeout scenarios

## Treasury Functions

For treasury address only:
- `withdrawTreasuryETH()` - Withdraw accumulated ETH fees
- `withdrawTreasuryTokens(address token)` - Withdraw accumulated token fees
- `withdrawAllTreasuryFees(address[] tokens)` - Batch withdraw ETH + multiple tokens
- `accumulatedTreasuryETH()` - View accumulated ETH fees
- `accumulatedTreasuryTokens(address token)` - View accumulated token fees

## Constants

- `MIN_DEPOSIT`: 200000000000000 wei (0.0002 ETH)
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
Update contract addresses and use the new dual pricing contracts:
```javascript
const NODE_REGISTRY = '0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6'; // Corrected dual pricing
const JOB_MARKETPLACE = '0xe169A4B57700080725f9553E3Cc69885fea13629'; // Corrected dual pricing
const MODEL_REGISTRY = '0x92b2De840bB2171203011A6dBA928d855cA8183E';
const PROOF_SYSTEM = '0x2ACcc60893872A499700908889B38C5420CBcFD1';
const HOST_EARNINGS = '0x908962e8c6CE72610021586f85ebDE09aAc97776';
```

## Deprecated Contracts

### JobMarketplaceWithModels (Incorrect MAX_PRICE_NATIVE)
- **Address**: 0x462050a4a551c4292586D9c1DE23e3158a9bF3B3
- **Deprecated**: January 28, 2025
- **Reason**: Incorrect MAX_PRICE_NATIVE (only 10x range instead of 10,000x)
- **Replacement**: 0xe169A4B57700080725f9553E3Cc69885fea13629

### NodeRegistryWithModels (Incorrect MAX_PRICE_NATIVE)
- **Address**: 0xC8dDD546e0993eEB4Df03591208aEDF6336342D7
- **Deprecated**: January 28, 2025
- **Reason**: Incorrect MAX_PRICE_NATIVE (only 10x range instead of 10,000x)
- **Replacement**: 0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6

## Last Updated
January 28, 2025 - Corrected dual pricing with 10,000x range for both native and stable tokens