# Fabstir LLM Marketplace - API Reference

**Last Updated:** January 6, 2026
**Network:** Base Sepolia (Chain ID: 84532)
**PRICE_PRECISION:** 1000 (all prices multiplied by 1000 for sub-$1/million support)

---

## Quick Start

### Contract Addresses (Upgradeable - UUPS Proxy Pattern)

> **NEW (December 14, 2025):** All contracts have been upgraded to UUPS proxy pattern for future upgradeability. Use the **proxy addresses** for all interactions.

```javascript
// UPGRADEABLE CONTRACTS (UUPS Proxies) - Use these addresses
const contracts = {
  // Proxy addresses (interact with these)
  jobMarketplace: "0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D",
  nodeRegistry: "0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22",
  modelRegistry: "0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2",
  proofSystem: "0x5afB91977e69Cc5003288849059bc62d47E7deeb",
  hostEarnings: "0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0",

  // Tokens (unchanged)
  fabToken: "0xC78949004B4EB6dEf2D66e49Cd81231472612D62",
  usdcToken: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
};

// Implementation addresses (for verification only)
const implementations = {
  jobMarketplace: "0x05c7d3a1b748dEbdbc12dd75D1aC195fb93228a3",
  nodeRegistry: "0x68298e2b74a106763aC99E3D973E98012dB5c75F",
  modelRegistry: "0xd7Df5c6D4ffe6961d47753D1dd32f844e0F73f50",
  proofSystem: "0x83eB050Aa3443a76a4De64aBeD90cA8d525E7A3A",
  hostEarnings: "0x588c42249F85C6ac4B4E27f97416C0289980aabB"
};
```

### Approved Models

| Model | Repo | File | Model ID (bytes32) |
|-------|------|------|-------------------|
| TinyVicuna-1B | CohereForAI/TinyVicuna-1B-32k-GGUF | tiny-vicuna-1b.q4_k_m.gguf | `0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced` |
| TinyLlama-1.1B | TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF | tinyllama-1b.Q4_K_M.gguf | `0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca` |
| OpenAI GPT-OSS-20B | bartowski/openai_gpt-oss-20b-GGUF | openai_gpt-oss-20b-MXFP4.gguf | `0x7583557c14f71d2bf21d48ffb7cde9329f9494090869d2d311ea481b26e7e06c` |

```javascript
// Model IDs (use these exact values)
const TINY_VICUNA = "0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced";
const TINY_LLAMA = "0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca";
const GPT_OSS_20B = "0x7583557c14f71d2bf21d48ffb7cde9329f9494090869d2d311ea481b26e7e06c";
```

> **Note:** Model IDs are derived from the model's on-chain registration, not simple keccak256 hashes of names. Always use the exact bytes32 values above.

---

## NodeRegistryWithModels (Upgradeable)

Host registration and pricing management.

**Proxy Address:** `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22`
**Implementation:** `0x68298e2b74a106763aC99E3D973E98012dB5c75F`

### Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `MIN_STAKE` | 1000 FAB | Minimum stake to register |
| `PRICE_PRECISION` | 1000 | Prices stored with 1000x multiplier |
| `MIN_PRICE_PER_TOKEN_NATIVE` | 227,273 | ~$0.001/million @ $4400 ETH |
| `MAX_PRICE_PER_TOKEN_NATIVE` | 22,727,272,727,273,000 | ~$100,000/million @ $4400 ETH |
| `MIN_PRICE_PER_TOKEN_STABLE` | 1 | $0.001 per million tokens |
| `MAX_PRICE_PER_TOKEN_STABLE` | 100,000,000 | $100,000 per million tokens |

> **IMPORTANT: PRICE_PRECISION**
>
> All prices are stored with a **1000x multiplier**. This allows sub-$1/million pricing for budget models.
>
> **To convert:** `pricePerToken = USD_per_million * 1000`
>
> Examples:
> - $0.06/million (Llama 3.2 3B) → `pricePerToken = 60`
> - $5/million → `pricePerToken = 5000`
> - $10/million → `pricePerToken = 10000`
>
> See [BREAKING_CHANGES.md](./BREAKING_CHANGES.md) for migration details.

### Host Registration

#### `registerNode`

Register as a host with model support and pricing.

```solidity
function registerNode(
    string memory metadata,      // JSON metadata (name, description, etc.)
    string memory apiUrl,        // Host's inference endpoint
    bytes32[] memory modelIds,   // Supported model IDs
    uint256 minPricePerTokenNative,  // Min price for ETH payments
    uint256 minPricePerTokenStable   // Min price for USDC payments
) external
```

**Requirements:**
- Caller must have approved FAB tokens for staking (1000 FAB minimum)
- All model IDs must be approved in ModelRegistry
- Prices must be within MIN/MAX ranges

**Example:**
```javascript
const nodeRegistry = new ethers.Contract(contracts.nodeRegistry, NodeRegistryABI, signer);
const fabToken = new ethers.Contract(contracts.fabToken, ERC20ABI, signer);

// Approve FAB tokens for staking
const stakeAmount = ethers.parseEther("1000");
await fabToken.approve(contracts.nodeRegistry, stakeAmount);

// Register node
const metadata = JSON.stringify({
  name: "My GPU Node",
  description: "RTX 4090 inference server",
  location: "US-East"
});
const apiUrl = "https://my-node.example.com/api";
const modelIds = [TINY_VICUNA, TINY_LLAMA];

// Pricing with PRICE_PRECISION (1000x multiplier)
// Native: ~$0.013/million @ $4400 ETH
const nativePrice = 3_000_000;  // Use integer, not parseUnits
// Stable: $5/million tokens
const stablePrice = 5000;  // 5 * 1000 = 5000

await nodeRegistry.registerNode(metadata, apiUrl, modelIds, nativePrice, stablePrice);
```

**Events:**
```solidity
event NodeRegistered(
    address indexed operator,
    uint256 stakedAmount,
    string metadata,
    bytes32[] models
);
```

#### `unregisterNode`

Unregister and withdraw staked FAB tokens.

```solidity
function unregisterNode() external
```

### Pricing Management

#### `updatePricingNative`

Update default native token (ETH) pricing.

```solidity
function updatePricingNative(uint256 newMinPrice) external
```

#### `updatePricingStable`

Update default stablecoin (USDC) pricing.

```solidity
function updatePricingStable(uint256 newMinPrice) external
```

#### `setModelPricing`

Set model-specific pricing (overrides default).

```solidity
function setModelPricing(
    bytes32 modelId,       // Model to set pricing for
    uint256 nativePrice,   // Native price (0 = use default)
    uint256 stablePrice    // Stable price (0 = use default)
) external
```

**Requirements:**
- Caller must be registered and active
- Model must be in caller's supported models
- Prices must be 0 (use default) or within MIN/MAX ranges

**Example:**
```javascript
// Set premium pricing for TinyVicuna model
// With PRICE_PRECISION=1000, this is $25/million tokens
const premiumNativePrice = 15_000_000;  // ~$0.066/million @ $4400 ETH
const premiumStablePrice = 25000;  // $25/million (25 * 1000)

await nodeRegistry.setModelPricing(TINY_VICUNA, premiumNativePrice, premiumStablePrice);
```

**Events:**
```solidity
event ModelPricingUpdated(
    address indexed operator,
    bytes32 indexed modelId,
    uint256 nativePrice,
    uint256 stablePrice
);
```

#### `clearModelPricing`

Clear model-specific pricing (revert to default).

```solidity
function clearModelPricing(bytes32 modelId) external
```

#### `setTokenPricing`

Set custom pricing for a specific stablecoin token.

```solidity
function setTokenPricing(
    address token,    // Token address (e.g., USDC, USDT)
    uint256 price     // Price per token for this stablecoin
) external
```

**Events:**
```solidity
event TokenPricingUpdated(
    address indexed operator,
    address indexed token,
    uint256 price
);
```

### Query Functions

#### `getNodePricing`

Get host's minimum price for a token (with fallbacks).

```solidity
function getNodePricing(
    address operator,  // Host address
    address token      // Token address (address(0) for native)
) external view returns (uint256)
```

**Fallback Logic:**
1. If token-specific price is set → return it
2. Else if token is native → return `minPricePerTokenNative`
3. Else → return `minPricePerTokenStable`

#### `getModelPricing`

Get model-specific pricing with fallback to default.

```solidity
function getModelPricing(
    address operator,  // Host address
    bytes32 modelId,   // Model ID
    address token      // Token address (address(0) for native)
) external view returns (uint256)
```

**Fallback Logic:**
1. If model-specific price is set → return it
2. Else → return default price for that token type

**Example:**
```javascript
// Query model pricing before creating session
const hostAddress = "0x...";
const modelPrice = await nodeRegistry.getModelPricing(
  hostAddress,
  TINY_VICUNA,
  ethers.ZeroAddress  // Native ETH
);
console.log("Min price:", ethers.formatEther(modelPrice), "ETH per token");
```

#### `getHostModelPrices`

Batch query all model prices for a host.

```solidity
function getHostModelPrices(address operator) external view returns (
    bytes32[] memory modelIds,
    uint256[] memory nativePrices,
    uint256[] memory stablePrices
)
```

**Returns effective prices** (model-specific or default fallback).

**Example:**
```javascript
const [modelIds, nativePrices, stablePrices] = await nodeRegistry.getHostModelPrices(hostAddress);

for (let i = 0; i < modelIds.length; i++) {
  console.log(`Model ${modelIds[i]}: Native=${nativePrices[i]}, Stable=${stablePrices[i]}`);
}
```

#### `getNodeFullInfo`

Get complete node information.

```solidity
function getNodeFullInfo(address operator) external view returns (
    address operator,
    uint256 stakedAmount,
    bool active,
    string memory metadata,
    string memory apiUrl,
    bytes32[] memory supportedModels,
    uint256 minPricePerTokenNative,
    uint256 minPricePerTokenStable
)
```

#### `getNodesForModel`

Get all active nodes supporting a specific model.

```solidity
function getNodesForModel(bytes32 modelId) external view returns (address[] memory)
```

#### `getAllActiveNodes`

Get all active node addresses.

```solidity
function getAllActiveNodes() external view returns (address[] memory)
```

#### `nodeSupportsModel`

Check if a node supports a specific model.

```solidity
function nodeSupportsModel(address nodeAddress, bytes32 modelId) external view returns (bool)
```

#### `isActiveNode`

Check if a node is currently active.

```solidity
function isActiveNode(address operator) external view returns (bool)
```

---

## JobMarketplaceWithModels (Upgradeable)

Session management and payments.

**Proxy Address:** `0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D`
**Implementation:** `0x05c7d3a1b748dEbdbc12dd75D1aC195fb93228a3`

### Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `MIN_DEPOSIT` | 0.0001 ETH (~$0.50) | Minimum ETH deposit |
| `USDC_MIN_DEPOSIT` | 500,000 (0.50 USDC) | Minimum USDC deposit |
| `PRICE_PRECISION` | 1000 | Prices stored with 1000x multiplier |
| `FEE_BASIS_POINTS` | 1000 | 10% platform fee |
| `DISPUTE_WINDOW` | 30 seconds | Time for disputes |
| `MIN_PROVEN_TOKENS` | 100 | Minimum tokens per proof |

> **Payment Calculations with PRICE_PRECISION:**
> ```
> maxTokens = (deposit * PRICE_PRECISION) / pricePerToken
> hostPayment = (tokensUsed * pricePerToken) / PRICE_PRECISION
> ```

### Session Creation

#### `createSessionJob`

Create a session with native ETH payment (uses default pricing).

```solidity
function createSessionJob(
    address host,           // Host address
    uint256 pricePerToken,  // Agreed price per token
    uint256 maxDuration,    // Max session duration in seconds
    uint256 proofInterval   // Tokens between proofs (min 100)
) external payable returns (uint256 jobId)
```

**Requirements:**
- `msg.value >= MIN_DEPOSIT` (0.0001 ETH)
- `pricePerToken >= host's minPricePerTokenNative`
- Host must be registered and active

**Example:**
```javascript
const marketplace = new ethers.Contract(contracts.jobMarketplace, JobMarketplaceABI, signer);

// Query host pricing first
const hostPrice = await nodeRegistry.getNodePricing(hostAddress, ethers.ZeroAddress);

// Create session with 0.01 ETH deposit
const tx = await marketplace.createSessionJob(
  hostAddress,
  hostPrice,           // Must meet host's minimum
  3600,                // 1 hour max duration
  1000,                // Proof every 1000 tokens
  { value: ethers.parseEther("0.01") }
);

const receipt = await tx.wait();
const sessionId = receipt.logs[0].args.jobId;
```

**Events:**
```solidity
event SessionJobCreated(
    uint256 indexed jobId,
    address indexed requester,
    address indexed host,
    uint256 deposit
);
```

#### `createSessionJobForModel`

Create a session for a specific model with model-specific pricing validation.

```solidity
function createSessionJobForModel(
    address host,           // Host address
    bytes32 modelId,        // Model to use
    uint256 pricePerToken,  // Agreed price per token
    uint256 maxDuration,    // Max session duration
    uint256 proofInterval   // Tokens between proofs
) external payable returns (uint256 jobId)
```

**Requirements:**
- Host must support the specified model
- `pricePerToken >= host's model-specific minimum (or default fallback)`

**Example:**
```javascript
// Query model-specific pricing
const modelPrice = await nodeRegistry.getModelPricing(hostAddress, TINY_VICUNA, ethers.ZeroAddress);

// Create model-aware session
const tx = await marketplace.createSessionJobForModel(
  hostAddress,
  TINY_VICUNA,         // Specific model
  modelPrice,          // Model-specific minimum
  3600,
  1000,
  { value: ethers.parseEther("0.01") }
);
```

**Events:**
```solidity
event SessionJobCreatedForModel(
    uint256 indexed jobId,
    address indexed requester,
    address indexed host,
    bytes32 modelId,
    uint256 deposit
);
```

#### `createSessionJobWithToken`

Create a session with ERC20 token payment (USDC).

```solidity
function createSessionJobWithToken(
    address host,
    address token,          // Payment token (e.g., USDC)
    uint256 deposit,        // Amount to deposit
    uint256 pricePerToken,
    uint256 maxDuration,
    uint256 proofInterval
) external returns (uint256 jobId)
```

**Requirements:**
- Token must be accepted (`acceptedTokens[token] == true`)
- Caller must have approved tokens for transfer
- `deposit >= tokenMinDeposits[token]`
- `pricePerToken >= host's minPricePerTokenStable`

**Example:**
```javascript
const usdc = new ethers.Contract(contracts.usdcToken, ERC20ABI, signer);

// Approve USDC
const depositAmount = ethers.parseUnits("10", 6);  // 10 USDC
await usdc.approve(contracts.jobMarketplace, depositAmount);

// Query host pricing for USDC
const hostPrice = await nodeRegistry.getNodePricing(hostAddress, contracts.usdcToken);

// Create USDC session
const tx = await marketplace.createSessionJobWithToken(
  hostAddress,
  contracts.usdcToken,
  depositAmount,
  hostPrice,
  3600,
  1000
);
```

#### `createSessionJobForModelWithToken`

Create a model-aware session with ERC20 token payment.

```solidity
function createSessionJobForModelWithToken(
    address host,
    bytes32 modelId,
    address token,
    uint256 deposit,
    uint256 pricePerToken,
    uint256 maxDuration,
    uint256 proofInterval
) external returns (uint256 jobId)
```

### Proof Submission (Host)

#### `submitProofOfWork`

Submit signed proof of work for tokens generated.

```solidity
function submitProofOfWork(
    uint256 jobId,          // Session ID
    uint256 tokensClaimed,  // Number of tokens in this proof
    bytes32 proofHash,      // SHA256 hash of STARK proof
    bytes calldata signature,  // Host's ECDSA signature (65 bytes)
    string memory proofCID  // S5 CID where proof is stored
) external
```

**Requirements:**
- Only the session host can submit proofs
- `tokensClaimed >= MIN_PROVEN_TOKENS` (100)
- `signature.length == 65` bytes (r, s, v format)
- Signature must be from the session host
- Session must be Active

**Example:**
```javascript
import { keccak256, solidityPacked, getBytes } from 'ethers';

// Host submits signed proof after generating tokens
const proofHash = keccak256(proofBytes);
const proofCID = "bafyreib...";  // S5 storage CID
const tokensClaimed = 1000;

// 1. Generate signature
const dataHash = keccak256(
  solidityPacked(
    ['bytes32', 'address', 'uint256'],
    [proofHash, hostAddress, tokensClaimed]
  )
);
const signature = await hostWallet.signMessage(getBytes(dataHash));

// 2. Submit with signature
await marketplace.submitProofOfWork(
  sessionId,
  tokensClaimed,
  proofHash,
  signature,
  proofCID
);
```

#### `getProofSubmission`

Get details of a specific proof submission.

```solidity
function getProofSubmission(
    uint256 sessionId,
    uint256 proofIndex
) external view returns (
    bytes32 proofHash,
    uint256 tokensClaimed,
    uint256 timestamp,
    bool verified
)
```

**Example:**
```javascript
const [proofHash, tokensClaimed, timestamp, verified] =
  await marketplace.getProofSubmission(sessionId, 0);
console.log(`Proof verified: ${verified}`);
```

**Events:**
```solidity
event ProofSubmitted(
    uint256 indexed jobId,
    address indexed host,
    uint256 tokensClaimed,
    bytes32 proofHash,
    string proofCID
);
```

### Session Completion

#### `completeSessionJob`

Complete a session and distribute payments.

```solidity
function completeSessionJob(
    uint256 jobId,
    string memory conversationCID  // S5 CID of conversation log
) external
```

**Can be called by anyone** (enables gasless completion for users).

**Payment Distribution:**
- 90% of earned amount → Host (via HostEarnings)
- 10% of earned amount → Treasury
- Remaining deposit → Refunded to user

**Example:**
```javascript
// Complete session (usually called by host)
await marketplace.completeSessionJob(sessionId, "bafyreic...");
```

**Events:**
```solidity
event SessionCompleted(
    uint256 indexed jobId,
    address indexed completedBy,
    uint256 tokensUsed,
    uint256 paymentAmount,
    uint256 refundAmount
);
```

### Query Functions

#### `sessionJobs`

Get session details.

```solidity
function sessionJobs(uint256 jobId) external view returns (
    uint256 id,
    address depositor,
    address requester,
    address host,
    address paymentToken,
    uint256 deposit,
    uint256 pricePerToken,
    uint256 tokensUsed,
    uint256 maxDuration,
    uint256 startTime,
    uint256 lastProofTime,
    uint256 proofInterval,
    SessionStatus status,
    uint256 withdrawnByHost,
    uint256 refundedToUser,
    string conversationCID,
    bytes32 lastProofHash,
    string lastProofCID
)
```

**SessionStatus enum:**
```solidity
enum SessionStatus {
    Active,      // 0 - In progress
    Completed,   // 1 - Successfully completed
    TimedOut,    // 2 - Timed out
    Disputed,    // 3 - Under dispute
    Abandoned,   // 4 - Abandoned by host
    Cancelled    // 5 - Cancelled
}
```

#### `sessionModel`

Get the model used for a session.

```solidity
function sessionModel(uint256 jobId) external view returns (bytes32)
```

Returns `bytes32(0)` for legacy sessions without model tracking.

#### `userSessions`

Get session IDs for a user.

```solidity
function userSessions(address user, uint256 index) external view returns (uint256)
```

#### `hostSessions`

Get session IDs for a host.

```solidity
function hostSessions(address host, uint256 index) external view returns (uint256)
```

### Deposit Management

#### `depositNative`

Pre-deposit ETH for future sessions.

```solidity
function depositNative() external payable
```

#### `depositToken`

Pre-deposit ERC20 tokens for future sessions.

```solidity
function depositToken(address token, uint256 amount) external
```

#### `withdrawNative`

Withdraw deposited ETH.

```solidity
function withdrawNative(uint256 amount) external
```

#### `withdrawToken`

Withdraw deposited tokens.

```solidity
function withdrawToken(address token, uint256 amount) external
```

#### `getDepositBalance`

Get deposit balance for an account.

```solidity
function getDepositBalance(address account, address token) external view returns (uint256)
```

Use `address(0)` for native ETH balance.

#### `createSessionFromDeposit`

Create session from pre-deposited funds.

```solidity
function createSessionFromDeposit(
    address host,
    address paymentToken,   // address(0) for native
    uint256 deposit,
    uint256 pricePerToken,
    uint256 maxDuration,
    uint256 proofInterval
) external returns (uint256 jobId)
```

### Admin Functions

#### `addAcceptedToken`

Add a new accepted payment token (treasury only).

```solidity
function addAcceptedToken(address token, uint256 minDeposit) external
```

#### `updateTokenMinDeposit`

Update minimum deposit for an accepted token (treasury or owner only).

```solidity
function updateTokenMinDeposit(address token, uint256 minDeposit) external
```

**Requirements:**
- Caller must be treasury or owner
- Token must already be accepted
- minDeposit must be > 0

**Events:**
```solidity
event TokenMinDepositUpdated(
    address indexed token,
    uint256 oldMinDeposit,
    uint256 newMinDeposit
)
```

**Example:**
```javascript
// Update USDC minimum deposit to $0.25
await marketplace.updateTokenMinDeposit(
  "0x036CbD53842c5426634e7929541eC2318f3dCF7e", // USDC
  250000  // 0.25 USDC (6 decimals)
);
```

---

## Common Workflows

### 1. Host Registration Flow

```javascript
// 1. Approve FAB tokens
await fabToken.approve(nodeRegistry.address, ethers.parseEther("1000"));

// 2. Register with models and pricing
await nodeRegistry.registerNode(
  JSON.stringify({ name: "My Node", gpu: "RTX 4090" }),
  "https://my-api.com/inference",
  [TINY_VICUNA, TINY_LLAMA],
  ethers.parseUnits("0.0001", 18),  // 0.0001 ETH/token
  1000                              // 0.001 USDC/token
);

// 3. Optionally set premium pricing for specific models
await nodeRegistry.setModelPricing(
  TINY_VICUNA,
  ethers.parseUnits("0.0005", 18),  // 5x for TinyVicuna
  5000
);
```

### 2. Client Session Flow

```javascript
// 1. Find hosts supporting desired model
const hosts = await nodeRegistry.getNodesForModel(TINY_VICUNA);

// 2. Query pricing for each host
for (const host of hosts) {
  const price = await nodeRegistry.getModelPricing(host, TINY_VICUNA, ethers.ZeroAddress);
  console.log(`Host ${host}: ${ethers.formatEther(price)} ETH/token`);
}

// 3. Create session with chosen host
const tx = await marketplace.createSessionJobForModel(
  chosenHost,
  TINY_VICUNA,
  chosenHostPrice,
  3600,
  1000,
  { value: ethers.parseEther("0.01") }
);
const { jobId } = (await tx.wait()).logs[0].args;

// 4. Use the session (off-chain inference calls)
// ...

// 5. Session completes when host calls completeSessionJob
```

### 3. Host Inference Flow

```javascript
import { keccak256, solidityPacked, getBytes } from 'ethers';

// 1. Listen for new sessions
marketplace.on("SessionJobCreated", async (jobId, requester, host, deposit) => {
  if (host === myAddress) {
    console.log(`New session ${jobId} from ${requester}`);
    // Start inference service
  }
});

// 2. Submit signed proofs periodically
const tokensClaimed = 1000;
const proofHash = keccak256(proofBytes);

// Generate host signature
const dataHash = keccak256(
  solidityPacked(['bytes32', 'address', 'uint256'], [proofHash, hostAddress, tokensClaimed])
);
const signature = await hostWallet.signMessage(getBytes(dataHash));

await marketplace.submitProofOfWork(
  sessionId,
  tokensClaimed,
  proofHash,
  signature,        // Host's ECDSA signature
  "bafyreib..."     // S5 CID
);

// 3. Complete session when done
await marketplace.completeSessionJob(sessionId, "bafyreic...");

// 4. Withdraw earnings from HostEarnings contract
const hostEarningsContract = new ethers.Contract(contracts.hostEarnings, HostEarningsABI, signer);
await hostEarningsContract.withdrawNative();
```

---

## Events Reference

### NodeRegistryWithModels Events

| Event | Description |
|-------|-------------|
| `NodeRegistered(address operator, uint256 stakedAmount, string metadata, bytes32[] models)` | New host registered |
| `NodeUnregistered(address operator, uint256 returnedAmount)` | Host unregistered |
| `ModelPricingUpdated(address operator, bytes32 modelId, uint256 nativePrice, uint256 stablePrice)` | Model pricing changed |
| `TokenPricingUpdated(address operator, address token, uint256 price)` | Token pricing changed |
| `PricingUpdated(address operator, uint256 newMinPrice)` | Default pricing changed |
| `ModelsUpdated(address operator, bytes32[] newModels)` | Supported models changed |
| `ApiUrlUpdated(address operator, string newApiUrl)` | API URL changed |
| `MetadataUpdated(address operator, string newMetadata)` | Metadata changed |

### JobMarketplaceWithModels Events

| Event | Description |
|-------|-------------|
| `SessionJobCreated(uint256 jobId, address requester, address host, uint256 deposit)` | Session created |
| `SessionJobCreatedForModel(uint256 jobId, address requester, address host, bytes32 modelId, uint256 deposit)` | Model-aware session created |
| `ProofSubmitted(uint256 jobId, address host, uint256 tokensClaimed, bytes32 proofHash, string proofCID)` | Proof of work submitted |
| `SessionCompleted(uint256 jobId, address completedBy, uint256 tokensUsed, uint256 paymentAmount, uint256 refundAmount)` | Session completed |
| `SessionTimedOut(uint256 jobId, uint256 hostEarnings, uint256 userRefund)` | Session timed out |
| `SessionAbandoned(uint256 jobId, uint256 userRefund)` | Session abandoned |
| `DepositReceived(address account, address token, uint256 amount)` | Deposit received |
| `WithdrawalProcessed(address account, address token, uint256 amount)` | Withdrawal processed |
| `TokenAccepted(address token, uint256 minDeposit)` | New token accepted |

---

## Error Messages

### NodeRegistryWithModels

| Error | Cause |
|-------|-------|
| `"Not registered"` | Caller is not a registered host |
| `"Node not active"` | Host is registered but not active |
| `"Model not supported"` | Model not in host's supported list |
| `"Native price below minimum"` | Price < MIN_PRICE_PER_TOKEN_NATIVE |
| `"Native price above maximum"` | Price > MAX_PRICE_PER_TOKEN_NATIVE |
| `"Stable price below minimum"` | Price < MIN_PRICE_PER_TOKEN_STABLE |
| `"Stable price above maximum"` | Price > MAX_PRICE_PER_TOKEN_STABLE |
| `"Invalid model"` | Model not approved in ModelRegistry |

### JobMarketplaceWithModels

| Error | Cause |
|-------|-------|
| `"Insufficient deposit"` | Deposit below minimum |
| `"Deposit too large"` | Deposit > 1000 ETH |
| `"Invalid price"` | pricePerToken is 0 |
| `"Invalid duration"` | Duration is 0 or > 365 days |
| `"Invalid proof interval"` | proofInterval is 0 |
| `"Invalid host"` | Host address is zero |
| `"Host does not support model"` | Host doesn't support requested model |
| `"Price below host minimum for model"` | Price < host's model-specific minimum |
| `"Price below host minimum (native)"` | Price < host's native minimum |
| `"Price below host minimum (stable)"` | Price < host's stable minimum |
| `"Token not accepted"` | Payment token not in accepted list |
| `"Token not configured"` | Token has no minimum deposit set |
| `"Only host can submit proof"` | Non-host trying to submit proof |
| `"Session not active"` | Session is not in Active status |

---

## ABI Files

ABIs are available in `/client-abis/`:

- `NodeRegistryWithModels-CLIENT-ABI.json`
- `JobMarketplaceWithModels-CLIENT-ABI.json`
- `ModelRegistry-CLIENT-ABI.json`
- `HostEarnings-CLIENT-ABI.json`
- `ProofSystem-CLIENT-ABI.json`

---

## Network Configuration

```javascript
const config = {
  chainId: 84532,
  rpcUrl: "https://sepolia.base.org",
  explorer: "https://sepolia.basescan.org",

  // UPGRADEABLE CONTRACTS (UUPS Proxies) - December 14, 2025
  contracts: {
    jobMarketplace: "0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D",
    nodeRegistry: "0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22",
    modelRegistry: "0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2",
    proofSystem: "0x5afB91977e69Cc5003288849059bc62d47E7deeb",
    hostEarnings: "0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0",
    fabToken: "0xC78949004B4EB6dEf2D66e49Cd81231472612D62",
    usdcToken: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
  },

  // Implementation addresses (for contract verification)
  implementations: {
    jobMarketplace: "0x05c7d3a1b748dEbdbc12dd75D1aC195fb93228a3",
    nodeRegistry: "0x68298e2b74a106763aC99E3D973E98012dB5c75F",
    modelRegistry: "0xd7Df5c6D4ffe6961d47753D1dd32f844e0F73f50",
    proofSystem: "0x83eB050Aa3443a76a4De64aBeD90cA8d525E7A3A",
    hostEarnings: "0x588c42249F85C6ac4B4E27f97416C0289980aabB"
  }
};
```

---

## Support

- **GitHub Issues**: https://github.com/anthropics/claude-code/issues
- **Contract Explorer**: https://sepolia.basescan.org
