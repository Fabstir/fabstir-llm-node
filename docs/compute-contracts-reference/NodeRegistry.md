# NodeRegistry Contract Documentation

## Current Implementation: NodeRegistryWithModels

**Contract Address**: `0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218`
**Network**: Base Sepolia | opBNB support planned post-MVP
**Status**: ✅ ACTIVE - Node registration with model validation and API discovery
**Last Updated**: January 25, 2025
**Source**: [`src/NodeRegistryWithModels.sol`](../../../src/NodeRegistryWithModels.sol)

### Overview

The NodeRegistryWithModels contract manages GPU host registration with integrated model validation. Hosts must specify which approved models they support during registration.

### Key Features
- **FAB Token Staking**: 1000 FAB minimum stake required
- **Model Validation**: Hosts must support approved models from ModelRegistry
- **API Discovery**: Hosts provide API endpoints for automatic discovery
- **Metadata Storage**: Flexible string field for capabilities description
- **Active Tracking**: Efficient enumeration of active nodes
- **Non-Custodial**: Hosts can unregister and reclaim stake anytime
- **Multi-Chain Ready**: Works across Base (ETH) and future opBNB (BNB)

## Contract Architecture

```solidity
contract NodeRegistryFAB is Ownable, ReentrancyGuard {
    IERC20 public immutable fabToken;
    uint256 public constant MIN_STAKE = 1000 * 10**18;
    
    struct Node {
        address operator;
        uint256 stakedAmount;
        bool active;
        string metadata;
    }
    
    mapping(address => Node) public nodes;
    address[] public activeNodesList;
}
```

## Key Functions

### Registration

```solidity
function registerNode(string memory metadata) external nonReentrant
```

Registers a new host node:
- Requires exactly 1000 FAB tokens
- Transfers tokens from caller to contract
- Adds to active nodes list
- Emits `NodeRegistered` event

**Requirements**:
1. Not already registered
2. Non-empty metadata string
3. Approved FAB tokens ≥ MIN_STAKE

### Unregistration

```solidity
function unregisterNode() external nonReentrant
```

Unregisters host and returns staked tokens:
- Marks node as inactive
- Returns ALL staked tokens
- Removes from active nodes list
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
```

Updates node capabilities description:
- Must be registered and active
- No gas-intensive validation
- Emits `MetadataUpdated` event

## Events

```solidity
event NodeRegistered(address indexed operator, uint256 stakedAmount, string metadata)
event NodeUnregistered(address indexed operator, uint256 returnedAmount)
event StakeAdded(address indexed operator, uint256 additionalAmount)
event MetadataUpdated(address indexed operator, string newMetadata)
```

## Node Data Structure

```solidity
struct Node {
    address operator;      // Node operator address
    uint256 stakedAmount; // Total FAB staked
    bool active;          // Registration status
    string metadata;      // Capabilities description
}
```

## View Functions

### Get Node Information
```solidity
function nodes(address operator) external view returns (
    address operator,
    uint256 stakedAmount,
    bool active,
    string memory metadata
)
```

### Get Active Nodes Count
```solidity
function getActiveNodesCount() external view returns (uint256)
```

### Get Active Node by Index
```solidity
function getActiveNode(uint256 index) external view returns (address)
```

## Integration with JobMarketplace

The JobMarketplaceWithModels (`0xaa38e7fcf5d7944ef7c836e8451f3bf93b98364f`) verifies host registration:

```solidity
// In JobMarketplaceWithModels
Node memory node = nodeRegistry.nodes(host);
require(node.operator != address(0), "Host not registered");
require(node.active, "Host not active");
require(node.stakedAmount >= MIN_STAKE, "Insufficient stake");
```

## Gas Costs

| Operation | Estimated Gas | Cost at 10 Gwei |
|-----------|--------------|-----------------|
| Register | ~200,000 | 0.002 ETH |
| Unregister | ~100,000 | 0.001 ETH |
| Add Stake | ~80,000 | 0.0008 ETH |
| Update Metadata | ~50,000 | 0.0005 ETH |

## Security Considerations

1. **ReentrancyGuard**: All state-changing functions protected
2. **Token Safety**: Uses safe transfer methods
3. **No Admin Functions**: Fully decentralized after deployment
4. **Immutable Token**: FAB token address cannot be changed
5. **No Slashing**: Currently no penalty mechanism

## Differences from Original NodeRegistry

| Feature | NodeRegistry (Old) | NodeRegistryFAB (Current) |
|---------|-------------------|---------------------------|
| Staking Token | ETH | FAB |
| Minimum Stake | 100 ETH | 1000 FAB |
| Registration Function | `registerNodeSimple(metadata)` payable | `registerNode(metadata)` |
| Additional Data | Region, models, compute units | Single metadata string |
| Complexity | High | Simplified |
| Gas Costs | Higher | Lower |

## Usage Example

```javascript
// Register as host
const registry = new ethers.Contract(registryAddress, abi, signer);

// 1. Approve FAB tokens
await fabToken.approve(registryAddress, MIN_STAKE);

// 2. Register with metadata
await registry.registerNode("llama-2-7b,gpt-4,inference");

// 3. Check registration
const nodeInfo = await registry.nodes(myAddress);
console.log('Registered:', nodeInfo.active);
console.log('Staked:', nodeInfo.stakedAmount);
```

## Best Practices

### For Hosts
1. Ensure sufficient FAB balance before registration
2. Set meaningful metadata describing capabilities
3. Keep metadata updated as capabilities change
4. Monitor stake amount for potential benefits

### For Integrators
1. Always verify registration before job assignment
2. Check both operator address and active status
3. Parse metadata for capability matching
4. Handle registration events for indexing

## Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| "Already registered" | Address already has a node | Unregister first |
| "Transfer failed" | Insufficient FAB or no approval | Approve tokens first |
| "Empty metadata" | Metadata string is empty | Provide capabilities description |
| "Not registered" | Operating on non-existent node | Register first |
| "Node not active" | Node was unregistered | Re-register if needed |

## Deployment Parameters

When deploying a new instance:
```solidity
constructor(address _fabToken) {
    require(_fabToken != address(0), "Invalid token");
    fabToken = IERC20(_fabToken);
}
```

## Future Enhancements

Potential improvements not yet implemented:
1. **Slashing Mechanism**: Penalize poor performance
2. **Tiered Staking**: Different stake levels for benefits
3. **Delegation**: Allow others to stake on behalf
4. **Reputation Integration**: Link stake to reputation score
5. **Dynamic Pricing**: Stake amount affects job pricing

## References

- [HOST_REGISTRATION_GUIDE.md](../../HOST_REGISTRATION_GUIDE.md) - Step-by-step registration guide
- [JobMarketplace.md](./JobMarketplace.md) - Integration details
- [Source Code](../../../src/NodeRegistryFAB.sol) - Contract implementation
- [Tests](../../../test/NodeRegistryFAB.t.sol) - Test coverage