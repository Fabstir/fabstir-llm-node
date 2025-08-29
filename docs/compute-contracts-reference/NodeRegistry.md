# NodeRegistryFAB Contract

## Overview

The NodeRegistryFAB contract manages GPU host registration using FAB token staking instead of ETH. It provides a more accessible entry point for hosts by using the platform's native FAB token for staking requirements.

**Contract Address (Base Sepolia)**: `0x87516C13Ea2f99de598665e14cab64E191A0f8c4`  
**Source**: [`src/NodeRegistryFAB.sol`](../../../src/NodeRegistryFAB.sol)

### Key Features
- Host registration with 1000 FAB token stake (instead of 100 ETH)
- Non-custodial staking - hosts can withdraw anytime
- Simplified metadata storage for node information
- Active nodes tracking for efficient enumeration
- Integration with JobMarketplaceFAB

### Dependencies
- OpenZeppelin Ownable
- OpenZeppelin ReentrancyGuard
- IERC20 (FAB Token)

## Constructor

```solidity
constructor(address _fabToken) Ownable(msg.sender)
```

### Parameters
| Name | Type | Description |
|------|------|-------------|
| `_fabToken` | `address` | Address of the FAB token contract |

### Example Deployment
```solidity
// Deploy with FAB token address
NodeRegistryFAB registry = new NodeRegistryFAB(0xC78949004B4EB6dEf2D66e49Cd81231472612D62);
```

## State Variables

### Constants
| Name | Type | Value | Description |
|------|------|-------|-------------|
| `MIN_STAKE` | `uint256` | 1000 * 10^18 | Minimum FAB tokens required for registration |

### Public Variables
| Name | Type | Description |
|------|------|-------------|
| `fabToken` | `IERC20` | FAB token contract interface |
| `nodes` | `mapping(address => Node)` | Registered node data by operator address |
| `activeNodesList` | `address[]` | Array of active node addresses |
| `activeNodesIndex` | `mapping(address => uint256)` | Index mapping for active nodes list |

## Structs

### Node
```solidity
struct Node {
    address operator;      // Node operator address
    uint256 stakedAmount; // Amount of FAB staked
    bool active;          // Whether node is active
    string metadata;      // Node metadata (models, regions, etc.)
}
```

## Functions

### registerNode
Registers a new node by staking FAB tokens.

```solidity
function registerNode(string memory metadata) external nonReentrant
```

**Requirements:**
- Node must not be already registered
- Metadata must not be empty
- Operator must approve MIN_STAKE FAB tokens
- Transfer of FAB tokens must succeed

**Example:**
```javascript
// First approve FAB tokens
await fabToken.approve(nodeRegistryFAB.address, "1000000000000000000000");
// Then register
await nodeRegistryFAB.registerNode("gpu:rtx4090,region:us-west");
```

### unregisterNode
Unregisters a node and returns staked FAB tokens.

```solidity
function unregisterNode() external nonReentrant
```

**Effects:**
- Marks node as inactive
- Returns staked FAB tokens to operator
- Removes from active nodes list

### addStake
Adds additional FAB tokens to existing stake.

```solidity
function addStake(uint256 amount) external nonReentrant
```

### updateMetadata
Updates node metadata (models, capabilities, etc.).

```solidity
function updateMetadata(string memory newMetadata) external
```

### getActiveNodes
Returns list of all active node addresses.

```solidity
function getActiveNodes() external view returns (address[] memory)
```

## Events

```solidity
event NodeRegistered(address indexed operator, uint256 stakedAmount, string metadata);
event NodeUnregistered(address indexed operator, uint256 returnedAmount);
event StakeAdded(address indexed operator, uint256 additionalAmount);
event MetadataUpdated(address indexed operator, string newMetadata);
```

## Integration with JobMarketplaceFAB

The NodeRegistryFAB is specifically designed to work with JobMarketplaceFAB:

1. **Host Verification**: JobMarketplaceFAB checks if a host is registered in NodeRegistryFAB before allowing job claims
2. **Stake Validation**: Ensures hosts have minimum 1000 FAB staked
3. **Active Status**: Only active nodes can claim jobs

## Comparison with Original NodeRegistry

| Feature | NodeRegistry | NodeRegistryFAB |
|---------|--------------|-----------------|
| Staking Token | ETH | FAB |
| Minimum Stake | 100 ETH | 1000 FAB |
| Stake Value (USD) | ~$250,000 | ~$1,000 |
| Accessibility | High barrier | Lower barrier |
| Token Economics | Uses network token | Uses platform token |

## Security Considerations

1. **Reentrancy Protection**: All state-changing functions use `nonReentrant` modifier
2. **Token Safety**: Uses SafeERC20 pattern for token transfers
3. **Access Control**: Owner-only functions for critical operations
4. **Stake Lock**: Minimum stake cannot be withdrawn while node is active

## Gas Optimization

- Efficient active nodes tracking using array + mapping pattern
- Single storage slot for boolean + address in Node struct
- Minimal storage writes during registration/unregistration

## Example Usage

### Complete Registration Flow
```javascript
// 1. Get FAB tokens (from faucet or purchase)
const fabToken = await ethers.getContractAt("IERC20", FAB_ADDRESS);

// 2. Approve NodeRegistryFAB to spend FAB
await fabToken.approve(NODE_REGISTRY_FAB, ethers.parseEther("1000"));

// 3. Register as node operator
const nodeRegistry = await ethers.getContractAt("NodeRegistryFAB", NODE_REGISTRY_FAB);
await nodeRegistry.registerNode("gpu:rtx4090,model:llama2,region:us-west");

// 4. Check registration
const node = await nodeRegistry.nodes(operatorAddress);
console.log("Staked:", node.stakedAmount);
console.log("Active:", node.active);
```

## Deployed Addresses

| Network | Address | FAB Token |
|---------|---------|-----------|
| Base Sepolia | `0x87516C13Ea2f99de598665e14cab64E191A0f8c4` | `0xC78949004B4EB6dEf2D66e49Cd81231472612D62` |
| Base Mainnet | TBD | TBD |