# NodeRegistry Contract Documentation

## Current Implementation: NodeRegistryWithModels

**Contract Address**: `0xC8dDD546e0993eEB4Df03591208aEDF6336342D7` ✅ NEW
**Previous Address**: `0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218` (deprecated)
**Network**: Base Sepolia (Block: 32,051,950)
**Status**: ✅ ACTIVE - Host-controlled pricing enabled
**Last Updated**: January 28, 2025
**Source**: [`src/NodeRegistryWithModels.sol`](../../../src/NodeRegistryWithModels.sol)

### Overview

The NodeRegistryWithModels contract manages GPU host registration with integrated model validation and **host-controlled pricing**. Hosts must specify which approved models they support and set their minimum pricing during registration.

### Key Features
- **Host-Controlled Pricing**: Hosts set their own minimum price per token (100-100,000 range)
- **Dynamic Pricing Updates**: Hosts can update pricing without re-registering
- **Price Discovery**: Clients can query host pricing before creating sessions
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
    uint256 public constant MIN_PRICE_PER_TOKEN = 100;      // 0.0001 USDC per token
    uint256 public constant MAX_PRICE_PER_TOKEN = 100000;   // 0.1 USDC per token

    struct Node {
        address operator;
        uint256 stakedAmount;
        bool active;
        string metadata;
        string apiUrl;
        bytes32[] supportedModels;
        uint256 minPricePerToken;  // NEW: Minimum price per token
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
    uint256 minPricePerToken  // NEW: Required pricing parameter
) external nonReentrant
```

Registers a new host node with pricing:
- Requires exactly 1000 FAB tokens
- Validates minPricePerToken is within range (100-100,000)
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
6. minPricePerToken between 100 and 100,000

### Pricing Management (NEW)

```solidity
function updatePricing(uint256 newMinPrice) external
```

Updates host's minimum pricing dynamically:
- Must be registered and active
- Validates new price is within range (100-100,000)
- Emits `PricingUpdated` event

**Requirements**:
1. Caller must be registered
2. Node must be active
3. newMinPrice >= 100
4. newMinPrice <= 100,000

### Price Discovery (NEW)

```solidity
function getNodePricing(address operator) external view returns (uint256)
```

Returns the minimum price per token for a host:
- Returns 0 if host not registered
- Used by clients to query pricing before creating sessions

```solidity
function getNodeFullInfo(address operator) external view returns (
    address operator,
    uint256 stakedAmount,
    bool active,
    string memory metadata,
    string memory apiUrl,
    bytes32[] memory supportedModels,
    uint256 minPricePerToken  // NEW: 7th field
)
```

Returns complete host information including pricing:
- **BREAKING CHANGE**: Now returns 7 fields instead of 6
- 7th field is minPricePerToken

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
event PricingUpdated(address indexed operator, uint256 newMinPrice)  // NEW
```

## Node Data Structure

```solidity
struct Node {
    address operator;            // Node operator address
    uint256 stakedAmount;       // Total FAB staked
    bool active;                // Registration status
    string metadata;            // Capabilities description
    string apiUrl;              // API endpoint URL
    bytes32[] supportedModels;  // Approved model IDs
    uint256 minPricePerToken;   // NEW: Minimum price per token (100-100,000)
}
```

## Integration with JobMarketplace

The JobMarketplaceWithModels (`0x462050a4a551c4292586D9c1DE23e3158a9bF3B3`) verifies host registration AND pricing:

```solidity
// In JobMarketplaceWithModels
(, , , , , , uint256 hostMinPrice) = nodeRegistry.getNodeFullInfo(host);
require(node.operator != address(0), "Host not registered");
require(node.active, "Host not active");
require(pricePerToken >= hostMinPrice, "Price below host minimum");  // NEW
```

## Gas Costs

| Operation | Estimated Gas | Cost at 10 Gwei |
|-----------|--------------|-----------------|
| Register (with pricing) | ~1,863,700 | 0.0186 ETH |
| Unregister | ~100,000 | 0.001 ETH |
| Update Pricing | ~30,000 | 0.0003 ETH |
| Add Stake | ~80,000 | 0.0008 ETH |
| Update Metadata | ~50,000 | 0.0005 ETH |

## Price Range

| Value | USDC per Token | Use Case |
|-------|----------------|----------|
| 100 | 0.0001 USDC | Minimum allowed (testing, low-cost models) |
| 1,000 | 0.001 USDC | Budget tier |
| 2,000 | 0.002 USDC | Standard tier |
| 5,000 | 0.005 USDC | Premium tier |
| 100,000 | 0.1 USDC | Maximum allowed (high-end models) |

## Security Considerations

1. **ReentrancyGuard**: All state-changing functions protected
2. **Token Safety**: Uses safe transfer methods
3. **Price Validation**: Enforced range prevents extreme pricing
4. **Model Validation**: Only approved models can be registered
5. **Immutable Token**: FAB token address cannot be changed
6. **No Admin Price Control**: Hosts have full pricing autonomy

## Breaking Changes from Previous Version

| Change | Old | New | Impact |
|--------|-----|-----|--------|
| Contract Address | `0x2AA3...6218` | `0xC8dD...42D7` | Must update SDK |
| Struct Fields | 6 fields | 7 fields (added minPricePerToken) | getNodeFullInfo() returns 7 values |
| registerNode() Parameters | 3 params | 4 params (added minPricePerToken) | Must provide pricing |
| Migration | - | None available | Hosts must re-register |

## Usage Example

```javascript
import NodeRegistryABI from './NodeRegistryWithModels-CLIENT-ABI.json';
import { ethers } from 'ethers';

const registry = new ethers.Contract(
  '0xC8dDD546e0993eEB4Df03591208aEDF6336342D7',
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

// 3. Register with pricing
const metadata = JSON.stringify({
  hardware: { gpu: "rtx-4090", vram: 24 },
  capabilities: ["inference", "streaming"],
  location: "us-east"
});

const minPricePerToken = 2000; // 0.002 USDC per token

await registry.registerNode(
  metadata,
  "https://my-host.com:8080",
  [modelId],
  minPricePerToken  // NEW: Required parameter
);

// 4. Update pricing dynamically
const newPrice = 3000; // Increase to 0.003 USDC per token
await registry.updatePricing(newPrice);

// 5. Query pricing
const myPricing = await registry.getNodePricing(myAddress);
console.log(`My minimum price: ${myPricing}`);

// 6. Check registration (7 fields returned)
const [operator, stake, active, metadata, apiUrl, models, minPrice] =
  await registry.getNodeFullInfo(myAddress);
console.log('Registered:', active);
console.log('Staked:', stake);
console.log('Minimum Price:', minPrice);
```

## Best Practices

### For Hosts
1. Set competitive pricing based on model capabilities
2. Monitor market rates and adjust pricing accordingly
3. Use `updatePricing()` to respond to demand changes
4. Keep metadata and API URL updated
5. Support only models you can reliably serve

### For Integrators
1. Always query pricing BEFORE creating sessions
2. Handle 7-field struct from `getNodeFullInfo()`
3. Expect "Price below host minimum" errors if price too low
4. Listen to `PricingUpdated` events to track price changes
5. Validate host supports required models

## Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| "Already registered" | Address already has a node | Unregister first |
| "Transfer failed" | Insufficient FAB or no approval | Approve tokens first |
| "Empty metadata" | Metadata string is empty | Provide capabilities description |
| "Price below minimum" | minPricePerToken < 100 | Set price >= 100 |
| "Price above maximum" | minPricePerToken > 100,000 | Set price <= 100,000 |
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

⚠️ **CRITICAL**: No migration path from old contract (`0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218`)

**Hosts must**:
1. Unregister from old contract (optional - to reclaim stake)
2. Approve FAB tokens for new contract
3. Call `registerNode()` on new contract with pricing parameter

**SDK Developers must**:
1. Update contract address to `0xC8dDD546e0993eEB4Df03591208aEDF6336342D7`
2. Import new ABI (handles 7-field struct)
3. Add pricing parameter to registration flows
4. Query pricing before session creation

## References

- [HOST_REGISTRATION_GUIDE.md](../../HOST_REGISTRATION_GUIDE.md) - Step-by-step registration guide
- [JobMarketplace.md](./JobMarketplace.md) - Integration details
- [ModelRegistry.md](./ModelRegistry.md) - Model validation details
- [IMPLEMENTATION-MARKET.md](../../IMPLEMENTATION-MARKET.md) - Pricing implementation plan
- [Source Code](../../../src/NodeRegistryWithModels.sol) - Contract implementation
- [Tests](../../../test/NodeRegistry/) - Test coverage (51 tests passing)
