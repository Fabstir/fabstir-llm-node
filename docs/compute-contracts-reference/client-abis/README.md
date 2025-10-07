# Client ABIs

This directory contains the Application Binary Interfaces (ABIs) for client integration.

## Current Deployed Contracts (January 28, 2025 - Host-Controlled Pricing)

### JobMarketplaceWithModels
- **Address**: 0x462050a4a551c4292586D9c1DE23e3158a9bF3B3 ✅ NEW
- **Previous**: 0xdEa1B47872C27458Bb7331Ade99099761C4944Dc
- **Network**: Base Sepolia (Block: 32,051,983)
- **Status**: ✅ HOST-CONTROLLED PRICING ENABLED
- **Configuration**:
  - ProofSystem: 0x2ACcc60893872A499700908889B38C5420CBcFD1 ✅ SET
  - Authorized in HostEarnings: ✅ CONFIRMED
  - NodeRegistry: 0xC8dDD546e0993eEB4Df03591208aEDF6336342D7 (7-field struct)
- **Key Features**:
  - 🆕 **HOST-CONTROLLED PRICING**: Contract enforces client price >= host minimum
  - 🆕 **Price Validation**: All session creation functions validate pricing (100-100,000 range)
  - 🆕 **Query Pricing**: Get host pricing before creating sessions
  - 🆕 Works with NodeRegistryWithModels 7-field struct (includes minPricePerToken)
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
- **Address**: 0xC8dDD546e0993eEB4Df03591208aEDF6336342D7 ✅ NEW
- **Previous**: 0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218
- **Network**: Base Sepolia (Block: 32,051,950)
- **Status**: ✅ HOST-CONTROLLED PRICING ENABLED
- **Stake Required**: 1000 FAB tokens
- **Key Features**:
  - 🆕 **Host-Controlled Pricing**: Hosts set minPricePerToken (100-100,000 range = 0.0001 to 0.1 USDC per token)
  - 🆕 **Dynamic Pricing Updates**: Hosts can call updatePricing() to change prices
  - 🆕 **Price Discovery**: Clients call getNodePricing(host) to query pricing
  - 🆕 **7-Field Node Struct**: Added minPricePerToken field (getNodeFullInfo returns 7 values)
  - 🆕 **PricingUpdated Event**: Track pricing changes on-chain
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
- **Authorized Marketplace**: 0x462050a4a551c4292586D9c1DE23e3158a9bF3B3 ✅ UPDATED

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

// For hosts - register with approved models AND pricing
const nodeRegistry = new ethers.Contract(
  '0xC8dDD546e0993eEB4Df03591208aEDF6336342D7',  // NodeRegistryWithModels - NEW with pricing
  NodeRegistryABI,
  signer
);

const metadata = JSON.stringify({
  hardware: { gpu: "rtx-4090", vram: 24 },
  capabilities: ["inference", "streaming"],
  location: "us-east"
});

const minPricePerToken = 2000; // 0.002 USDC per token (range: 100-100,000)

await nodeRegistry.registerNode(
  metadata,
  "http://my-host.com:8080",
  [modelId], // Must be an approved model ID
  minPricePerToken // NEW: Host sets minimum price
);
```

## Host-Controlled Pricing Usage (NEW - January 28, 2025)

### For Hosts: Setting and Updating Pricing

```javascript
import NodeRegistryABI from './NodeRegistryWithModels-CLIENT-ABI.json';
import { ethers } from 'ethers';

const nodeRegistry = new ethers.Contract(
  '0xC8dDD546e0993eEB4Df03591208aEDF6336342D7',
  NodeRegistryABI,
  signer
);

// Register with pricing (required parameter)
const minPricePerToken = 2000; // 0.002 USDC per token
// Price range: 100 (0.0001 USDC) to 100,000 (0.1 USDC) per token

await nodeRegistry.registerNode(
  metadata,
  apiUrl,
  [modelId],
  minPricePerToken
);

// Update pricing dynamically
const newPrice = 3000; // Increase to 0.003 USDC per token
await nodeRegistry.updatePricing(newPrice);

// Query own pricing
const myPricing = await nodeRegistry.getNodePricing(myAddress);
console.log(`My minimum price: ${myPricing} (${myPricing / 1e6} USDC per token)`);
```

### For Clients: Querying Pricing and Creating Sessions

```javascript
import JobMarketplaceABI from './JobMarketplaceWithModels-CLIENT-ABI.json';
import NodeRegistryABI from './NodeRegistryWithModels-CLIENT-ABI.json';
import { ethers } from 'ethers';

const nodeRegistry = new ethers.Contract(
  '0xC8dDD546e0993eEB4Df03591208aEDF6336342D7',
  NodeRegistryABI,
  provider
);

const marketplace = new ethers.Contract(
  '0x462050a4a551c4292586D9c1DE23e3158a9bF3B3',
  JobMarketplaceABI,
  signer
);

// STEP 1: Query host pricing BEFORE creating session
const hostAddress = '0x...';
const hostMinPrice = await nodeRegistry.getNodePricing(hostAddress);
console.log(`Host minimum: ${hostMinPrice}`);

// Or get all host info including pricing (7th field)
const [operator, stake, active, metadata, apiUrl, models, minPrice] =
  await nodeRegistry.getNodeFullInfo(hostAddress);

// STEP 2: Create session with price >= host minimum
const myPricePerToken = 2500; // Must be >= hostMinPrice
const deposit = ethers.utils.parseEther('0.1'); // 0.1 ETH

// This will REVERT if myPricePerToken < hostMinPrice
const tx = await marketplace.createSessionJob(
  hostAddress,
  myPricePerToken,
  3600, // 1 hour max duration
  100,  // Proof every 100 tokens
  { value: deposit }
);

await tx.wait();
console.log('Session created with validated pricing!');
```

### Price Validation Rules

⚠️ **CRITICAL**: All session creation functions now validate pricing:
- `createSessionJob()` - Native token (ETH) sessions
- `createSessionJobWithToken()` - ERC20 token (USDC) sessions
- `createSessionFromDeposit()` - Pre-funded sessions

**Validation**: `clientPricePerToken >= hostMinPricePerToken`

**If validation fails**: Transaction reverts with "Price below host minimum"

**Price Range**: 100 to 100,000 (0.0001 to 0.1 USDC per token)
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
  '0x462050a4a551c4292586D9c1DE23e3158a9bF3B3', // CURRENT deployment with pricing validation
  JobMarketplaceABI,
  provider
);

const nodeRegistry = new ethers.Contract(
  '0xC8dDD546e0993eEB4Df03591208aEDF6336342D7', // NodeRegistryWithModels with pricing
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
Update contract addresses and use the new discovery functions:
```javascript
const NODE_REGISTRY = '0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218';
const JOB_MARKETPLACE = '0x56431bDeA20339c40470eC86BC2E3c09B065AFFe';
const MODEL_REGISTRY = '0x92b2De840bB2171203011A6dBA928d855cA8183E';
```

## Last Updated
January 13, 2025 - Model governance system added with ModelRegistry and NodeRegistryWithModels contracts