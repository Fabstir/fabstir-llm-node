# NodeRegistry Contract Documentation

## Current Implementation: NodeRegistryWithModels

**Contract Address**: `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6` ✅ NEW - Corrected Dual Pricing
**Previous Address**: `0xC8dDD546e0993eEB4Df03591208aEDF6336342D7` (deprecated - incorrect MAX_PRICE_NATIVE)
**Network**: Base Sepolia
**Status**: ✅ ACTIVE - Dual pricing with 10,000x range
**Last Updated**: January 28, 2025
**Source**: [`src/NodeRegistryWithModels.sol`](../../../src/NodeRegistryWithModels.sol)

### Overview

The NodeRegistryWithModels contract manages GPU host registration with integrated model validation and **dual pricing system**. Hosts must specify which approved models they support and set their minimum pricing for BOTH native tokens (ETH/BNB) and stablecoins (USDC) during registration.

### Key Features
- **Dual Pricing System**: Separate pricing for native tokens (ETH/BNB) and stablecoins (USDC)
- **10,000x Range**: Both native and stable pricing have identical 10,000x range (MIN to MAX)
- **Dynamic Pricing Updates**: Separate update functions for native and stable pricing
- **Price Discovery**: Clients can query both native and stable pricing before creating sessions
- **FAB Token Staking**: 1000 FAB minimum stake required
- **Model Validation**: Hosts must support approved models from ModelRegistry
- **API Discovery**: Hosts provide API endpoints for automatic discovery
- **Metadata Storage**: Flexible string field for capabilities description
- **Active Tracking**: Efficient enumeration of active nodes
- **Non-Custodial**: Hosts can unregister and reclaim stake anytime
- **Multi-Chain Ready**: Works across Base (ETH) and future opBNB (BNB)

## Contract Architecture

```solidity
contract NodeRegistryWithModels is Ownable, ReentrancyGuard {
    IERC20 public immutable fabToken;
    ModelRegistry public modelRegistry;

    uint256 public constant MIN_STAKE = 1000 * 10**18;

    // Native token pricing (ETH/BNB)
    uint256 public constant MIN_PRICE_PER_TOKEN_NATIVE = 2_272_727_273;        // ~$0.00001 @ $4400 ETH
    uint256 public constant MAX_PRICE_PER_TOKEN_NATIVE = 22_727_272_727_273;   // ~$0.1 @ $4400 ETH

    // Stablecoin pricing (USDC)
    uint256 public constant MIN_PRICE_PER_TOKEN_STABLE = 10;        // 0.00001 USDC per token
    uint256 public constant MAX_PRICE_PER_TOKEN_STABLE = 100_000;   // 0.1 USDC per token

    struct Node {
        address operator;
        uint256 stakedAmount;
        bool active;
        string metadata;
        string apiUrl;
        bytes32[] supportedModels;
        uint256 minPricePerTokenNative;  // NEW: Minimum price for ETH/BNB sessions (wei)
        uint256 minPricePerTokenStable;  // NEW: Minimum price for USDC sessions
    }

    mapping(address => Node) public nodes;
    mapping(bytes32 => address[]) public modelToNodes;
    address[] public activeNodesList;
}
```

## Key Functions

### Registration

```solidity
function registerNode(
    string memory metadata,
    string memory apiUrl,
    bytes32[] memory modelIds,
    uint256 minPricePerTokenNative,  // NEW: Required native pricing (ETH/BNB)
    uint256 minPricePerTokenStable   // NEW: Required stable pricing (USDC)
) external nonReentrant
```

Registers a new host node with dual pricing:
- Requires exactly 1000 FAB tokens
- Validates minPricePerTokenNative is within range (2,272,727,273 to 22,727,272,727,273 wei)
- Validates minPricePerTokenStable is within range (10 to 100,000)
- Validates all modelIds are approved by ModelRegistry
- Transfers tokens from caller to contract
- Adds to active nodes list and model mappings
- Emits `NodeRegistered` event

**Requirements**:
1. Not already registered
2. Non-empty metadata string
3. Approved FAB tokens ≥ MIN_STAKE
4. At least one model ID provided
5. All model IDs approved by ModelRegistry
6. minPricePerTokenNative between 2,272,727,273 and 22,727,272,727,273 wei
7. minPricePerTokenStable between 10 and 100,000

### Pricing Management (NEW)

```solidity
function updatePricingNative(uint256 newMinPrice) external
```

Updates host's native token (ETH/BNB) minimum pricing:
- Must be registered and active
- Validates new price is within range (2,272,727,273 to 22,727,272,727,273 wei)
- Emits `PricingUpdatedNative` event

**Requirements**:
1. Caller must be registered
2. Node must be active
3. newMinPrice >= 2,272,727,273 wei
4. newMinPrice <= 22,727,272,727,273 wei

```solidity
function updatePricingStable(uint256 newMinPrice) external
```

Updates host's stablecoin (USDC) minimum pricing:
- Must be registered and active
- Validates new price is within range (10 to 100,000)
- Emits `PricingUpdatedStable` event

**Requirements**:
1. Caller must be registered
2. Node must be active
3. newMinPrice >= 10
4. newMinPrice <= 100,000

### Price Discovery (NEW)

```solidity
function getNodePricing(address operator) external view returns (
    uint256 minPricePerTokenNative,
    uint256 minPricePerTokenStable
)
```

Returns BOTH minimum prices for a host:
- Returns (0, 0) if host not registered
- Used by clients to query dual pricing before creating sessions
- **BREAKING CHANGE**: Now returns tuple of 2 values instead of single value

```solidity
function getNodeFullInfo(address operator) external view returns (
    address operator,
    uint256 stakedAmount,
    bool active,
    string memory metadata,
    string memory apiUrl,
    bytes32[] memory supportedModels,
    uint256 minPricePerTokenNative,  // NEW: 7th field
    uint256 minPricePerTokenStable   // NEW: 8th field
)
```

Returns complete host information including dual pricing:
- **BREAKING CHANGE**: Now returns 8 fields instead of 7
- 7th field is minPricePerTokenNative (wei)
- 8th field is minPricePerTokenStable (raw USDC value)

### Unregistration

```solidity
function unregisterNode() external nonReentrant
```

Unregisters host and returns staked tokens:
- Marks node as inactive
- Returns ALL staked tokens
- Removes from active nodes list and model mappings
- Emits `NodeUnregistered` event

### Additional Staking

```solidity
function stake(uint256 amount) external nonReentrant
```

Adds more FAB tokens to existing stake:
- Must be registered and active
- No maximum limit
- Useful for reputation/priority systems

### Metadata Management

```solidity
function updateMetadata(string memory newMetadata) external
function updateApiUrl(string memory newApiUrl) external
function updateSupportedModels(bytes32[] memory newModelIds) external
```

Updates node information:
- Must be registered and active
- Models must be approved by ModelRegistry
- Emits respective update events

## Events

```solidity
event NodeRegistered(
    address indexed operator,
    uint256 stakedAmount,
    string metadata,
    bytes32[] models
)
event NodeUnregistered(address indexed operator, uint256 returnedAmount)
event StakeAdded(address indexed operator, uint256 additionalAmount)
event MetadataUpdated(address indexed operator, string newMetadata)
event ApiUrlUpdated(address indexed operator, string newApiUrl)
event ModelsUpdated(address indexed operator, bytes32[] newModels)
event PricingUpdatedNative(address indexed operator, uint256 newMinPrice)  // NEW
event PricingUpdatedStable(address indexed operator, uint256 newMinPrice)  // NEW
```

## Node Data Structure

```solidity
struct Node {
    address operator;               // Node operator address
    uint256 stakedAmount;          // Total FAB staked
    bool active;                   // Registration status
    string metadata;               // Capabilities description
    string apiUrl;                 // API endpoint URL
    bytes32[] supportedModels;     // Approved model IDs
    uint256 minPricePerTokenNative; // NEW: Min price for ETH/BNB (2.27B-22.7T wei)
    uint256 minPricePerTokenStable; // NEW: Min price for USDC (10-100,000)
}
```

## Integration with JobMarketplace

The JobMarketplaceWithModels (`0xe169A4B57700080725f9553E3Cc69885fea13629`) verifies host registration AND dual pricing:

```solidity
// In JobMarketplaceWithModels
(, , , , , , uint256 hostMinNative, uint256 hostMinStable) = nodeRegistry.getNodeFullInfo(host);
require(node.operator != address(0), "Host not registered");
require(node.active, "Host not active");

// For ETH sessions
require(pricePerToken >= hostMinNative, "Price below host minimum (native)");

// For USDC sessions
require(pricePerToken >= hostMinStable, "Price below host minimum (stable)");
```

## Gas Costs

| Operation | Estimated Gas | Cost at 10 Gwei |
|-----------|--------------|-----------------|
| Register (with dual pricing) | ~1,900,000 | 0.019 ETH |
| Unregister | ~100,000 | 0.001 ETH |
| Update Native Pricing | ~30,000 | 0.0003 ETH |
| Update Stable Pricing | ~30,000 | 0.0003 ETH |
| Add Stake | ~80,000 | 0.0008 ETH |
| Update Metadata | ~50,000 | 0.0005 ETH |

## Pricing Ranges

### Native Token Pricing (ETH/BNB)

| Wei Value | ETH @ $4400 | USD Value | Use Case |
|-----------|-------------|-----------|----------|
| 2,272,727,273 | ~0.0000023 ETH | ~$0.00001 | Minimum allowed (testing, low-cost) |
| 22,727,272,727 | ~0.000023 ETH | ~$0.0001 | Budget tier |
| 227,272,727,273 | ~0.00023 ETH | ~$0.001 | Standard tier |
| 2,272,727,272,727 | ~0.0023 ETH | ~$0.01 | Premium tier |
| 22,727,272,727,273 | ~0.023 ETH | ~$0.1 | Maximum allowed (high-end models) |

**Range**: 10,000x from MIN to MAX

### Stablecoin Pricing (USDC)

| Value | USDC per Token | Use Case |
|-------|----------------|----------|
| 10 | 0.00001 USDC | Minimum allowed (testing, low-cost models) |
| 100 | 0.0001 USDC | Budget tier |
| 1,000 | 0.001 USDC | Standard tier |
| 10,000 | 0.01 USDC | Premium tier |
| 100,000 | 0.1 USDC | Maximum allowed (high-end models) |

**Range**: 10,000x from MIN to MAX

## Security Considerations

1. **ReentrancyGuard**: All state-changing functions protected
2. **Token Safety**: Uses safe transfer methods
3. **Price Validation**: Enforced range prevents extreme pricing
4. **Model Validation**: Only approved models can be registered
5. **Immutable Token**: FAB token address cannot be changed
6. **No Admin Price Control**: Hosts have full pricing autonomy

## Breaking Changes from Previous Version

| Change | Old (0xC8dD...42D7) | New (0xDFFD...42D6) | Impact |
|--------|-----|-----|--------|
| Contract Address | `0xC8dDD546e0993eEB4Df03591208aEDF6336342D7` | `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6` | Must update SDK |
| Struct Fields | 7 fields (single pricing) | 8 fields (dual pricing) | getNodeFullInfo() returns 8 values |
| registerNode() Parameters | 4 params (single price) | 5 params (dual pricing) | Must provide both native and stable pricing |
| getNodePricing() Return | Single uint256 | Tuple (uint256, uint256) | Returns (native, stable) |
| updatePricing() Function | Single function | Two functions (Native/Stable) | Use updatePricingNative() or updatePricingStable() |
| Price Range Native | Incorrect 10x | Corrected 10,000x | Native pricing now properly ranges 2.27B to 22.7T wei |
| Migration | - | None available | Hosts must re-register |

## Usage Example

```javascript
import NodeRegistryABI from './NodeRegistryWithModels-CLIENT-ABI.json';
import { ethers } from 'ethers';

const registry = new ethers.Contract(
  '0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6',
  NodeRegistryABI,
  signer
);

// 1. Get model ID from ModelRegistry
const modelRegistry = new ethers.Contract(modelRegistryAddress, modelABI, provider);
const modelId = await modelRegistry.getModelId(
  "CohereForAI/TinyVicuna-1B-32k-GGUF",
  "tiny-vicuna-1b.q4_k_m.gguf"
);

// 2. Approve FAB tokens
const MIN_STAKE = ethers.utils.parseEther('1000');
await fabToken.approve(registryAddress, MIN_STAKE);

// 3. Register with DUAL pricing
const metadata = JSON.stringify({
  hardware: { gpu: "rtx-4090", vram: 24 },
  capabilities: ["inference", "streaming"],
  location: "us-east"
});

const minPriceNative = ethers.BigNumber.from("3000000000"); // 3B wei (~$0.000013 @ $4400 ETH)
const minPriceStable = 15000; // 0.015 USDC per token

await registry.registerNode(
  metadata,
  "https://my-host.com:8080",
  [modelId],
  minPriceNative,  // NEW: Native pricing (ETH/BNB)
  minPriceStable   // NEW: Stable pricing (USDC)
);

// 4. Update native pricing (ETH/BNB)
const newPriceNative = ethers.BigNumber.from("5000000000"); // 5B wei
await registry.updatePricingNative(newPriceNative);

// 5. Update stable pricing (USDC)
const newPriceStable = 20000; // 0.02 USDC per token
await registry.updatePricingStable(newPriceStable);

// 6. Query dual pricing (returns tuple)
const [nativePrice, stablePrice] = await registry.getNodePricing(myAddress);
console.log(`Native pricing: ${nativePrice.toString()} wei`);
console.log(`Stable pricing: ${stablePrice} (${stablePrice / 1e6} USDC)`);

// 7. Check registration (8 fields returned)
const [operator, stake, active, metadata, apiUrl, models, minNative, minStable] =
  await registry.getNodeFullInfo(myAddress);
console.log('Registered:', active);
console.log('Staked:', stake);
console.log('Minimum Native Price:', minNative.toString(), 'wei');
console.log('Minimum Stable Price:', minStable);
```

## Best Practices

### For Hosts
1. Set competitive DUAL pricing based on model capabilities and payment type
2. Monitor market rates and ETH price to adjust pricing accordingly
3. Use `updatePricingNative()` and `updatePricingStable()` to respond to demand/price changes
4. Consider gas costs when setting native pricing (users pay ETH gas fees)
5. Keep both pricing fields updated - consider market dynamics for each token type
6. Keep metadata and API URL updated
7. Support only models you can reliably serve

### For Integrators
1. Always query dual pricing BEFORE creating sessions
2. Handle 8-field struct from `getNodeFullInfo()` (not 7!)
3. Extract BOTH pricing values from `getNodePricing()` tuple
4. Validate against correct pricing field: native for ETH, stable for USDC
5. Expect "Price below host minimum (native)" or "(stable)" errors if price too low
6. Listen to `PricingUpdatedNative` and `PricingUpdatedStable` events separately
7. Validate host supports required models

## Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| "Already registered" | Address already has a node | Unregister first |
| "Transfer failed" | Insufficient FAB or no approval | Approve tokens first |
| "Empty metadata" | Metadata string is empty | Provide capabilities description |
| "Native price below minimum" | minPriceNative < 2,272,727,273 | Set price >= 2,272,727,273 wei |
| "Native price above maximum" | minPriceNative > 22,727,272,727,273 | Set price <= 22,727,272,727,273 wei |
| "Stable price below minimum" | minPriceStable < 10 | Set price >= 10 |
| "Stable price above maximum" | minPriceStable > 100,000 | Set price <= 100,000 |
| "Model not approved" | Model ID not in ModelRegistry | Use approved models only |
| "Not registered" | Operating on non-existent node | Register first |
| "Node not active" | Node was unregistered | Re-register if needed |

## Deployment Parameters

When deploying a new instance:
```solidity
constructor(
    address _fabToken,
    address _modelRegistry
) {
    require(_fabToken != address(0), "Invalid FAB token");
    require(_modelRegistry != address(0), "Invalid ModelRegistry");
    fabToken = IERC20(_fabToken);
    modelRegistry = ModelRegistry(_modelRegistry);
}
```

## Migration Notes

⚠️ **CRITICAL**: No migration path from old contracts

**Deprecated Contracts**:
- `0xC8dDD546e0993eEB4Df03591208aEDF6336342D7` - Incorrect MAX_PRICE_NATIVE (10x instead of 10,000x)
- `0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218` - No dual pricing support

**Hosts must**:
1. Unregister from old contract (optional - to reclaim stake)
2. Approve FAB tokens for new contract `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6`
3. Call `registerNode()` with DUAL pricing parameters (5 params total)

**SDK Developers must**:
1. Update contract address to `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6`
2. Import new ABI (handles 8-field struct, not 7)
3. Add BOTH pricing parameters to registration flows
4. Update `getNodePricing()` to handle tuple return (native, stable)
5. Use separate update functions: `updatePricingNative()` and `updatePricingStable()`
6. Query dual pricing before session creation
7. Validate against correct pricing field based on payment type

## References

- [HOST_REGISTRATION_GUIDE.md](../../HOST_REGISTRATION_GUIDE.md) - Step-by-step registration guide
- [JobMarketplace.md](./JobMarketplace.md) - Integration details
- [ModelRegistry.md](./ModelRegistry.md) - Model validation details
- [IMPLEMENTATION-MARKET.md](../../IMPLEMENTATION-MARKET.md) - Pricing implementation plan
- [Source Code](../../../src/NodeRegistryWithModels.sol) - Contract implementation
- [Tests](../../../test/NodeRegistry/) - Test coverage (51 tests passing)
