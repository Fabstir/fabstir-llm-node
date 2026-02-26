# Fabstir LLM Marketplace - API Reference

**Last Updated:** February 26, 2026
**Network:** Base Sepolia (Chain ID: 84532)

---

## Quick Start

### Contract Addresses (Upgradeable - UUPS Proxy Pattern)

> **POST-AUDIT REMEDIATION (February 22-26, 2026):** All contracts upgraded with 20 security audit findings addressed, Phase 18 per-model per-token pricing, delegated sessions, early cancellation fees, pull-pattern refunds, and per-model rate limits.

```javascript
// POST-AUDIT REMEDIATION CONTRACTS (Feb 22-26, 2026) - Use these addresses
const contracts = {
  // Proxy addresses (interact with these)
  jobMarketplace: "0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4", // FRESH PROXY — All 20 audit findings (Feb 22, 2026)
  nodeRegistry: "0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22",   // Per-model per-token pricing (needs redeployment)
  modelRegistry: "0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2",   // Per-model rate limits (Feb 22, 2026)
  proofSystem: "0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31",     // markProofUsed (Feb 22, 2026)
  hostEarnings: "0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0",

  // Tokens (unchanged)
  fabToken: "0xC78949004B4EB6dEf2D66e49Cd81231472612D62",
  usdcToken: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
};

// Implementation addresses (for verification only) - Updated Feb 26, 2026
const implementations = {
  jobMarketplace: "0x51C3F60D2e3756Cc3F119f9aE1876e2B947347ba", // Full audit remediation (Feb 22)
  nodeRegistry: "0xeeB3ABad9d27Bb3a5D7ACA3c282CDD8C80aAD24b",   // Needs redeployment for Phase 18
  modelRegistry: "0xF12a0A07d4230E0b045dB22057433a9826d21652",   // Rate limits + rejected fee (Feb 22)
  proofSystem: "0xC46C84a612Cbf4C2eAaf5A9D1411aDA6309EC963",    // markProofUsed + dead code removal (Feb 22)
  hostEarnings: "0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0",    // Unchanged
};
```

### Approved Models

| Model              | Repo                                    | File                           | Model ID (bytes32)                                                   |
| ------------------ | --------------------------------------- | ------------------------------ | -------------------------------------------------------------------- |
| TinyVicuna-1B      | CohereForAI/TinyVicuna-1B-32k-GGUF     | tiny-vicuna-1b.q4_k_m.gguf    | `0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced` |
| TinyLlama-1.1B     | TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF | tinyllama-1b.Q4_K_M.gguf      | `0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca` |
| OpenAI GPT-OSS-20B | bartowski/openai_gpt-oss-20b-GGUF       | openai_gpt-oss-20b-MXFP4.gguf | `0x7583557c14f71d2bf21d48ffb7cde9329f9494090869d2d311ea481b26e7e06c` |

```javascript
// Model IDs (use these exact values)
const TINY_VICUNA =
  "0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced";
const TINY_LLAMA =
  "0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca";
const GPT_OSS_20B =
  "0x7583557c14f71d2bf21d48ffb7cde9329f9494090869d2d311ea481b26e7e06c";
```

> **Note:** 5 models are currently approved. The 3 IDs listed above are commonly used. For the full set (including GPT-OSS-120B and GLM-4.7-Flash), query the ModelRegistry on-chain via `isModelApproved(bytes32)`.

---

## NodeRegistryWithModels (Upgradeable)

Host registration and pricing management.

**Proxy Address:** `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22`
**Implementation:** `0xeeB3ABad9d27Bb3a5D7ACA3c282CDD8C80aAD24b` (needs redeployment for Phase 18 per-model per-token pricing)

### Constants

| Constant                     | Value                 | Description                           |
| ---------------------------- | --------------------- | ------------------------------------- |
| `MIN_STAKE`                  | 1000 FAB              | Minimum stake to register             |
| `PRICE_PRECISION`            | 1000                  | Prices stored with 1000x multiplier   |
| `MIN_PRICE_PER_TOKEN_NATIVE` | 227,273               | Min native price per token            |
| `MAX_PRICE_PER_TOKEN_NATIVE` | 22,727,272,727,273,000 | Max native price per token           |
| `MIN_PRICE_PER_TOKEN_STABLE` | 1                     | Min stable price per token            |
| `MAX_PRICE_PER_TOKEN_STABLE` | 100,000,000           | Max stable price per token            |
| `MAX_SLASH_PERCENTAGE`       | 50                    | Max 50% stake slashed per action      |
| `MIN_STAKE_AFTER_SLASH`      | 100 FAB               | Auto-unregister threshold             |
| `SLASH_COOLDOWN`             | 24 hours              | Cooldown between slashes on same host |

> **IMPORTANT: PRICE_PRECISION**
>
> All prices are stored with a **1000x multiplier**. This allows sub-$1/million pricing for budget models.
>
> **To convert:** `pricePerToken = USD_per_million * 1000`
>
> Examples:
>
> - $0.06/million (Llama 3.2 3B) -> `pricePerToken = 60`
> - $5/million -> `pricePerToken = 5000`
> - $10/million -> `pricePerToken = 10000`
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
const nodeRegistry = new ethers.Contract(
  contracts.nodeRegistry,
  NodeRegistryABI,
  signer
);
const fabToken = new ethers.Contract(contracts.fabToken, ERC20ABI, signer);

// 1. Approve FAB tokens for staking
const stakeAmount = ethers.parseEther("1000");
await fabToken.approve(contracts.nodeRegistry, stakeAmount);

// 2. Register node
const metadata = JSON.stringify({
  name: "My GPU Node",
  description: "RTX 4090 inference server",
  location: "US-East",
});
const apiUrl = "https://my-node.example.com/api";
const modelIds = [TINY_VICUNA, TINY_LLAMA];

// Pricing with PRICE_PRECISION (1000x multiplier)
// Native: ~$0.013/million @ $4400 ETH
const nativePrice = 3_000_000; // Use integer, not parseUnits
// Stable: $5/million tokens
const stablePrice = 5000; // 5 * 1000 = 5000

await nodeRegistry.registerNode(
  metadata,
  apiUrl,
  modelIds,
  nativePrice,
  stablePrice
);

// 3. Set per-model per-token pricing (REQUIRED for each model+token combo)
await nodeRegistry.setModelTokenPricing(
  TINY_VICUNA,
  contracts.usdcToken,
  5000  // $5/million tokens (with PRICE_PRECISION=1000)
);
await nodeRegistry.setModelTokenPricing(
  TINY_VICUNA,
  ethers.ZeroAddress,  // Native ETH
  nativePrice
);
// Repeat for each model the host supports
await nodeRegistry.setModelTokenPricing(
  TINY_LLAMA,
  contracts.usdcToken,
  5000
);
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

#### `setModelTokenPricing`

Set pricing for a specific model+token combination. This is the primary pricing function (Phase 18).

```solidity
function setModelTokenPricing(
    bytes32 modelId,   // Model to set pricing for
    address token,     // Token address (address(0) for native ETH)
    uint256 price      // Price per token (with PRICE_PRECISION=1000)
) external
```

**Requirements:**

- Caller must be registered and active
- Model must be in caller's supported models
- Price must be within valid range for the token type

> **REQUIRED (Phase 18):** Hosts MUST call this for each model+token combination they wish to accept. `getModelPricing()` reads only from `modelTokenPricing` and reverts with `"No model pricing"` if not set.

**Example:**

```javascript
// Set pricing for TinyVicuna model
// USDC pricing: $25/million tokens (with PRICE_PRECISION=1000)
await nodeRegistry.setModelTokenPricing(
  TINY_VICUNA,
  contracts.usdcToken,
  25000  // $25/million (25 * 1000)
);

// Native ETH pricing: ~$0.066/million @ $4400 ETH
await nodeRegistry.setModelTokenPricing(
  TINY_VICUNA,
  ethers.ZeroAddress,  // address(0) for native
  15_000_000
);
```

**Events:**

```solidity
event ModelTokenPricingUpdated(
    address indexed operator,
    bytes32 indexed modelId,
    address indexed token,
    uint256 price
);
```

#### `clearModelTokenPricing`

Remove pricing for a specific model+token combination.

```solidity
function clearModelTokenPricing(
    bytes32 modelId,   // Model to clear pricing for
    address token      // Token address (address(0) for native ETH)
) external
```

**Requirements:**

- Caller must be registered and active
- Model must be in caller's supported models

### Query Functions

#### `getModelPricing`

Get pricing for a specific model+token combination. Reads only from `modelTokenPricing` mapping (no fallback).

```solidity
function getModelPricing(
    address operator,  // Host address
    bytes32 modelId,   // Model ID
    address token      // Token address (address(0) for native)
) external view returns (uint256)
```

**Pricing Resolution (Phase 18):**

1. Reads `modelTokenPricing[operator][modelId][token]`
2. If price > 0 -> return it
3. Otherwise -> **reverts with `"No model pricing"`**

> **BREAKING CHANGE (Phase 18):** No fallback to legacy pricing fields. Hosts MUST call `setModelTokenPricing(modelId, token, price)` for each model+token combo they wish to accept.

**Example:**

```javascript
// Query model pricing before creating session
const hostAddress = "0x...";
const modelPrice = await nodeRegistry.getModelPricing(
  hostAddress,
  TINY_VICUNA,
  ethers.ZeroAddress // Native ETH
);
console.log("Min price:", ethers.formatEther(modelPrice), "ETH per token");
```

#### `getHostModelPrices`

Batch query all model prices for a host for a specific token.

```solidity
function getHostModelPrices(
    address operator,  // Host address
    address token      // Token address (address(0) for native)
) external view returns (
    bytes32[] memory modelIds,
    uint256[] memory prices
)
```

**Returns prices from `modelTokenPricing`** for all models the host supports with the given token.

**Example:**

```javascript
const [modelIds, prices] =
  await nodeRegistry.getHostModelPrices(hostAddress, contracts.usdcToken);

for (let i = 0; i < modelIds.length; i++) {
  console.log(
    `Model ${modelIds[i]}: Price=${prices[i]}`
  );
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

### Stake Slashing

Slashing allows penalizing hosts for proven misbehavior. Owner-controlled at MVP with future DAO upgrade path.

#### `slashStake`

Slash a host's staked FAB tokens for misbehavior.

```solidity
function slashStake(
    address host,           // Host to slash
    uint256 amount,         // Amount of FAB to slash
    string calldata evidenceCID,  // S5 CID of evidence
    string calldata reason  // Human-readable reason
) external
```

**Requirements:**

- Caller must be `slashingAuthority` (initially owner)
- Host must be active with stake
- `amount <= stakedAmount * 50 / 100` (max 50% per slash)
- `block.timestamp >= lastSlashTime[host] + 24 hours` (cooldown)
- `evidenceCID` and `reason` must be non-empty

**Side Effects:**

- If `stakedAmount < 100 FAB` after slash: Host is auto-unregistered, remaining stake returned
- Slashed amount sent to treasury

**Example:**

```javascript
// Owner slashes host for proof fraud
await nodeRegistry.slashStake(
  hostAddress,
  ethers.parseEther("200"), // 200 FAB
  "bafyreib...", // Evidence CID
  "Invalid proof submission"
);
```

**Events:**

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
```

#### `initializeSlashing`

Initialize slashing system (one-time after upgrade).

```solidity
function initializeSlashing(address _treasury) external
```

**Requirements:**

- Caller must be owner
- Can only be called once (sets slashingAuthority to owner)

#### `setSlashingAuthority`

Update the slashing authority address (for future DAO migration).

```solidity
function setSlashingAuthority(address newAuthority) external
```

**Requirements:**

- Caller must be owner
- `newAuthority` cannot be zero address

**Events:**

```solidity
event SlashingAuthorityUpdated(address indexed previousAuthority, address indexed newAuthority);
```

#### `setTreasury`

Update the treasury address for receiving slashed tokens.

```solidity
function setTreasury(address newTreasury) external
```

**Requirements:**

- Caller must be owner
- `newTreasury` cannot be zero address

**Events:**

```solidity
event TreasuryUpdated(address indexed newTreasury);
```

#### `lastSlashTime`

Query the last slash timestamp for a host.

```solidity
function lastSlashTime(address host) external view returns (uint256)
```

---

## JobMarketplaceWithModels (Upgradeable)

Session management and payments.

**Proxy Address:** `0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4`
**Implementation:** `0x51C3F60D2e3756Cc3F119f9aE1876e2B947347ba` (Full audit remediation - Feb 22, 2026)

### Constants

| Constant            | Value               | Description                         |
| ------------------- | ------------------- | ----------------------------------- |
| `MIN_DEPOSIT`       | 0.0001 ETH (~$0.50) | Minimum ETH deposit                 |
| `USDC_MIN_DEPOSIT`  | 500,000 (0.50 USDC) | Minimum USDC deposit                |
| `PRICE_PRECISION`   | 1000                | Prices stored with 1000x multiplier |
| `FEE_BASIS_POINTS`  | 1000                | 10% platform fee                    |
| `DISPUTE_WINDOW`    | 30 seconds          | Time for disputes                   |
| `MIN_PROVEN_TOKENS` | 100                 | Minimum tokens per proof            |

> **Payment Calculations with PRICE_PRECISION:**
>
> ```
> maxTokens = (deposit * PRICE_PRECISION) / pricePerToken
> hostPayment = (tokensUsed * pricePerToken) / PRICE_PRECISION
> ```

### Session Creation

#### `createSessionJobForModel`

Create a session for a specific model with native ETH payment.

```solidity
function createSessionJobForModel(
    address host,              // Host address
    bytes32 modelId,           // Model to use
    uint256 pricePerToken,     // Agreed price per token
    uint256 maxDuration,       // Max session duration
    uint256 proofInterval,     // Tokens between proofs
    uint256 proofTimeoutWindow // Seconds before proof times out
) external payable returns (uint256 jobId)
```

**Requirements:**

- Host must support the specified model
- `pricePerToken >= host's model pricing` (from `modelTokenPricing[host][modelId][address(0)]`)
- Reverts with `"No model pricing"` if host has not set pricing for this model+native combo

**Example:**

```javascript
// Query model-specific pricing
const modelPrice = await nodeRegistry.getModelPricing(
  hostAddress,
  TINY_VICUNA,
  ethers.ZeroAddress
);

// Create model-aware session
const tx = await marketplace.createSessionJobForModel(
  hostAddress,
  TINY_VICUNA, // Specific model
  modelPrice, // Model-specific minimum
  3600,
  1000,
  300,  // 5 minute proof timeout window
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
    uint256 proofInterval,
    uint256 proofTimeoutWindow // Seconds before proof times out
) external returns (uint256 jobId)
```

### Proof Submission (Host)

#### `submitProofOfWork`

Submit proof of work for tokens generated. Authentication is via `msg.sender == session.host` (no signature required).

```solidity
function submitProofOfWork(
    uint256 jobId,          // Session ID
    uint256 tokensClaimed,  // Number of tokens in this proof
    bytes32 proofHash,      // SHA256 hash of STARK proof
    string calldata proofCID,  // S5 CID where full proof is stored
    string calldata deltaCID   // S5 CID for delta since last proof
) external
```

**Requirements:**

- Only the session host can submit proofs (`msg.sender == session.host`)
- `tokensClaimed >= MIN_PROVEN_TOKENS` (100)
- Session must be Active

**Example:**

```javascript
// Host submits proof (no signature needed -- msg.sender authentication)
const proofHash = keccak256(proofBytes);
const proofCID = "bafyreib...";
const deltaCID = "bafyreic...";
const tokensClaimed = 1000;

await marketplace.submitProofOfWork(
  sessionId,
  tokensClaimed,
  proofHash,
  proofCID,
  deltaCID
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
    bool verified,
    string memory deltaCID
)
```

**Example:**

```javascript
const [proofHash, tokensClaimed, timestamp, verified, deltaCID] =
  await marketplace.getProofSubmission(sessionId, 0);
console.log(`Proof verified: ${verified}, deltaCID: ${deltaCID}`);
```

**Events:**

```solidity
event ProofSubmitted(
    uint256 indexed jobId,
    address indexed host,
    uint256 tokensClaimed,
    bytes32 proofHash,
    string proofCID,
    string deltaCID
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

**Access Control:**

- Only `depositor` or `host` can call this function
- `depositor` can complete immediately (no waiting period)
- `host` must wait `DISPUTE_WINDOW` (30 seconds) after session start

**Payment Distribution:**

- 90% of earned amount -> Host (via HostEarnings)
- 10% of earned amount -> Treasury
- Remaining deposit -> Refunded to user

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

### Early Cancellation

Sessions that are completed by the depositor before the dispute window elapses may incur an early cancellation fee. This is computed as the minimum billing amount (1 proof interval worth of tokens) to prevent abuse of free session creation.

### Query Functions

#### `sessionJobs`

Get session details.

```solidity
function sessionJobs(uint256 jobId) external view returns (
    uint256 id,
    address depositor,      // Who deposited funds (receives refunds)
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
    TimedOut     // 2 - Timed out
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

### Delegated Sessions (Smart Wallet Sub-Account Support)

Enables Coinbase Smart Wallet sub-accounts to create sessions using the primary account's pre-deposited funds without popups.

#### `authorizeDelegate`

Authorize or revoke a delegate sub-account.

```solidity
function authorizeDelegate(address delegate, bool authorized) external
```

#### `isDelegateAuthorized`

Check if a delegate is authorized.

```solidity
function isDelegateAuthorized(address depositor, address delegate) external view returns (bool)
```

#### `createSessionForModelAsDelegate`

Create a model-specific session using another account's deposits.

```solidity
function createSessionForModelAsDelegate(
    address payer,              // Primary account with deposits
    bytes32 modelId,
    address host,
    address paymentToken,       // Must be ERC20 (not native)
    uint256 amount,
    uint256 pricePerToken,
    uint256 maxDuration,
    uint256 proofInterval,
    uint256 proofTimeoutWindow
) external returns (uint256 sessionId)
```

**Requirements:**

- `msg.sender` must be authorized delegate of `payer`
- `paymentToken` must be ERC20 (native not supported for delegation)
- Session ownership stays with `payer` (refunds go to payer)
- Delegate cannot access other users' deposits
- Works with pause mechanism

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
  250000 // 0.25 USDC (6 decimals)
);
```

---

## Common Workflows

### 1. Host Registration Flow

```javascript
// 1. Approve FAB tokens
await fabToken.approve(nodeRegistry.address, ethers.parseEther("1000"));

// 2. Register with models (legacy pricing fields retained for storage layout)
await nodeRegistry.registerNode(
  JSON.stringify({ name: "My Node", gpu: "RTX 4090" }),
  "https://my-api.com/inference",
  [TINY_VICUNA, TINY_LLAMA],
  ethers.parseUnits("0.0001", 18), // Legacy native field
  1000 // Legacy stable field
);

// 3. Set per-model per-token pricing (REQUIRED for each model+token combo)
// USDC pricing for TinyVicuna
await nodeRegistry.setModelTokenPricing(
  TINY_VICUNA,
  contracts.usdcToken,
  5000  // $5/million tokens (with PRICE_PRECISION=1000)
);
// Native ETH pricing for TinyVicuna
await nodeRegistry.setModelTokenPricing(
  TINY_VICUNA,
  ethers.ZeroAddress,
  ethers.parseUnits("0.0001", 18)
);
// Repeat for TinyLlama
await nodeRegistry.setModelTokenPricing(
  TINY_LLAMA,
  contracts.usdcToken,
  5000
);
```

### 2. Client Session Flow

```javascript
// 1. Find hosts supporting desired model
const hosts = await nodeRegistry.getNodesForModel(TINY_VICUNA);

// 2. Query pricing for each host
for (const host of hosts) {
  const price = await nodeRegistry.getModelPricing(
    host,
    TINY_VICUNA,
    ethers.ZeroAddress
  );
  console.log(`Host ${host}: ${ethers.formatEther(price)} ETH/token`);
}

// 3. Create session with chosen host
const tx = await marketplace.createSessionJobForModel(
  chosenHost,
  TINY_VICUNA,
  chosenHostPrice,
  3600,
  1000,
  300,  // 5 minute proof timeout window
  { value: ethers.parseEther("0.01") }
);
const { jobId } = (await tx.wait()).logs[0].args;

// 4. Use the session (off-chain inference calls)
// ...

// 5. Session completes when host calls completeSessionJob
```

### 3. Host Inference Flow

```javascript
// 1. Listen for new sessions
marketplace.on("SessionJobCreated", async (jobId, requester, host, deposit) => {
  if (host === myAddress) {
    console.log(`New session ${jobId} from ${requester}`);
    // Start inference service
  }
});

// 2. Submit proofs periodically with CID evidence (no signature needed)
const tokensClaimed = 1000;
const proofHash = keccak256(proofBytes);

// Upload proof data to S5
const proofCID = await s5Client.upload(proofData);
const deltaCID = await s5Client.upload(deltaData); // Incremental changes

await marketplace.submitProofOfWork(
  sessionId,
  tokensClaimed,
  proofHash,
  proofCID, // Full proof CID
  deltaCID  // Delta CID (can be "" if not tracking)
);

// 3. Complete session when done with conversation record
const conversationCID = await s5Client.upload(conversationLog);
await marketplace.completeSessionJob(sessionId, conversationCID);

// 4. Withdraw earnings from HostEarnings contract
const hostEarningsContract = new ethers.Contract(
  contracts.hostEarnings,
  HostEarningsABI,
  signer
);
await hostEarningsContract.withdrawNative();
```

---

## Events Reference

### NodeRegistryWithModels Events

| Event                                                                                                                                         | Description                        |
| --------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------- |
| `NodeRegistered(address operator, uint256 stakedAmount, string metadata, bytes32[] models)`                                                   | New host registered                |
| `NodeUnregistered(address operator, uint256 returnedAmount)`                                                                                  | Host unregistered                  |
| `ModelTokenPricingUpdated(address operator, bytes32 modelId, address token, uint256 price)`                                                   | Per-model per-token pricing changed |
| `ModelsUpdated(address operator, bytes32[] newModels)`                                                                                        | Supported models changed           |
| `ApiUrlUpdated(address operator, string newApiUrl)`                                                                                           | API URL changed                    |
| `MetadataUpdated(address operator, string newMetadata)`                                                                                       | Metadata changed                   |
| `SlashExecuted(address host, uint256 amount, uint256 remainingStake, string evidenceCID, string reason, address executor, uint256 timestamp)` | Host stake slashed                 |
| `HostAutoUnregistered(address host, uint256 slashedAmount, uint256 returnedAmount, string reason)`                                            | Host auto-unregistered after slash |
| `SlashingAuthorityUpdated(address previousAuthority, address newAuthority)`                                                                   | Slashing authority changed         |
| `TreasuryUpdated(address newTreasury)`                                                                                                        | Slashing treasury changed          |

### JobMarketplaceWithModels Events

| Event                                                                                                                     | Description                 |
| ------------------------------------------------------------------------------------------------------------------------- | --------------------------- |
| `SessionJobCreated(uint256 jobId, address requester, address host, uint256 deposit)`                                      | Session created             |
| `SessionJobCreatedForModel(uint256 jobId, address requester, address host, bytes32 modelId, uint256 deposit)`             | Model-aware session created |
| `ProofSubmitted(uint256 jobId, address host, uint256 tokensClaimed, bytes32 proofHash, string proofCID, string deltaCID)` | Proof of work submitted     |
| `SessionCompleted(uint256 jobId, address completedBy, uint256 tokensUsed, uint256 paymentAmount, uint256 refundAmount)`   | Session completed           |
| `SessionTimedOut(uint256 jobId, uint256 hostEarnings, uint256 userRefund)`                                                | Session timed out           |
| `DepositReceived(address account, address token, uint256 amount)`                                                         | Deposit received            |
| `WithdrawalProcessed(address account, address token, uint256 amount)`                                                     | Withdrawal processed        |
| `TokenAccepted(address token, uint256 minDeposit)`                                                                        | New token accepted          |

---

## Error Messages

> **Note (Feb 22, 2026):** Error messages have been shortened for EVM contract size compliance (F202615067). The messages below show the current terse versions.

### NodeRegistryWithModels

| Error                            | Cause                                                                    |
| -------------------------------- | ------------------------------------------------------------------------ |
| `"Not registered"`               | Caller is not a registered host                                          |
| `"Node not active"`              | Host is registered but not active                                        |
| `"Model not supported"`          | Model not in host's supported list                                       |
| `"Native price below minimum"`   | Price < MIN_PRICE_PER_TOKEN_NATIVE                                       |
| `"Native price above maximum"`   | Price > MAX_PRICE_PER_TOKEN_NATIVE                                       |
| `"Stable price below minimum"`   | Price < MIN_PRICE_PER_TOKEN_STABLE                                       |
| `"Stable price above maximum"`   | Price > MAX_PRICE_PER_TOKEN_STABLE                                       |
| `"Invalid model"`                | Model not approved in ModelRegistry                                      |
| `"No model pricing"`             | Host has not set pricing for this model+token combo via `setModelTokenPricing()` |
| `"Not slashing authority"`       | Caller is not the slashing authority                                     |
| `"Host not active"`              | Host is not active or not registered                                     |
| `"No stake to slash"`            | Host has no stake                                                        |
| `"Amount exceeds stake"`         | Slash amount > host's stake                                              |
| `"Exceeds max slash percentage"` | Slash > 50% of stake                                                     |
| `"Cooldown active"`              | 24h cooldown not elapsed since last slash                                |
| `"Evidence required"`            | evidenceCID is empty                                                     |
| `"Reason required"`              | reason string is empty                                                   |
| `"Slashing already initialized"` | initializeSlashing called twice                                          |
| `"Zero address"`                 | Treasury or authority set to address(0)                                   |

### JobMarketplaceWithModels

| Error                      | Cause                                             |
| -------------------------- | ------------------------------------------------- |
| `"Insufficient deposit"`   | Deposit below minimum                             |
| `"Deposit too large"`      | Deposit > 1000 ETH                                |
| `"Invalid price"`          | pricePerToken is 0                                |
| `"Invalid duration"`       | Duration is 0 or > 365 days                       |
| `"Invalid proof interval"` | proofInterval is 0                                |
| `"Invalid host"`           | Host address is zero                              |
| `"Model unsupported"`      | Host doesn't support requested model              |
| `"Price < model min"`      | Price < host's model-specific minimum             |
| `"Price < host min"`       | Price < host's native minimum                     |
| `"Price < host min (stable)"` | Price < host's stable minimum                  |
| `"Token not accepted"`     | Payment token not in accepted list                |
| `"Token not configured"`   | Token has no minimum deposit set                  |
| `"Only host can submit proof"` | Non-host trying to submit proof               |
| `"Only host/depositor"`    | Third party trying to complete session            |
| `"Session not active"`     | Session is not in Active status                   |
| `"Not authorized"`         | Delegate not authorized by depositor              |
| `"ERC20 only"`             | Delegated sessions only support ERC20 tokens      |
| `"Proof timeout"`          | Proof was not submitted within the timeout window |

---

## ABI Files

ABIs are available in `/client-abis/`:

- `NodeRegistryWithModelsUpgradeable-CLIENT-ABI.json`
- `JobMarketplaceWithModelsUpgradeable-CLIENT-ABI.json`
- `ModelRegistryUpgradeable-CLIENT-ABI.json`
- `HostEarningsUpgradeable-CLIENT-ABI.json`
- `ProofSystemUpgradeable-CLIENT-ABI.json`

---

## Network Configuration

```javascript
const config = {
  chainId: 84532,
  rpcUrl: "https://sepolia.base.org",
  explorer: "https://sepolia.basescan.org",

  // POST-AUDIT REMEDIATION CONTRACTS (Feb 22-26, 2026)
  contracts: {
    jobMarketplace: "0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4",
    nodeRegistry: "0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22",
    modelRegistry: "0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2",
    proofSystem: "0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31",
    hostEarnings: "0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0",
    fabToken: "0xC78949004B4EB6dEf2D66e49Cd81231472612D62",
    usdcToken: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
  },

  // Implementation addresses (for contract verification) - Feb 26, 2026
  implementations: {
    jobMarketplace: "0x51C3F60D2e3756Cc3F119f9aE1876e2B947347ba", // Full audit remediation (Feb 22)
    nodeRegistry: "0xeeB3ABad9d27Bb3a5D7ACA3c282CDD8C80aAD24b",   // Needs redeployment for Phase 18
    modelRegistry: "0xF12a0A07d4230E0b045dB22057433a9826d21652",   // Rate limits + rejected fee (Feb 22)
    proofSystem: "0xC46C84a612Cbf4C2eAaf5A9D1411aDA6309EC963",    // markProofUsed + dead code removal (Feb 22)
    hostEarnings: "0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0",    // Unchanged
  },
};
```

---

## Support

- **Contract Explorer**: https://sepolia.basescan.org
