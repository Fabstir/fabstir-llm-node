# HostEarnings Contract

## Overview

The HostEarnings contract is a gas-efficient earnings accumulation system for Fabstir marketplace hosts. Instead of receiving direct payments for each completed job, hosts accumulate earnings that can be withdrawn in batches, resulting in 40-46% gas savings for hosts completing multiple jobs.

**Contract Address (Base Sepolia)**: `0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E`  
**Deployed**: 2025-08-24  
**Source**: [`src/HostEarnings.sol`](../../../src/HostEarnings.sol)

### Key Features
- Accumulates host earnings from multiple jobs
- Supports multiple ERC20 tokens (primarily USDC)
- Batch withdrawal for gas efficiency
- 40-46% gas reduction for multiple jobs
- Transparent balance tracking
- Emergency withdrawal by owner
- Reentrancy protection

### Gas Savings Analysis
| Jobs Completed | Traditional Gas | With Accumulation | Savings |
|----------------|-----------------|-------------------|---------|
| 1 job | 115,000 | 69,000 | 40% |
| 5 jobs | 575,000 | 345,000 + 115,000 | 20% |
| 10 jobs | 1,150,000 | 690,000 + 115,000 | 30% |
| 20 jobs | 2,300,000 | 1,380,000 + 115,000 | 35% |

*Note: Additional 115,000 gas for single withdrawal transaction*

## Constructor

```solidity
constructor() Ownable(msg.sender)
```

Creates the HostEarnings contract with the deployer as owner.

## State Variables

### Public Variables
| Name | Type | Description |
|------|------|-------------|
| `earnings` | `mapping(address => mapping(address => uint256))` | Host → Token → Balance |
| `totalEarnings` | `mapping(address => uint256)` | Total earnings per token |
| `authorized` | `mapping(address => bool)` | Authorized contracts to credit earnings |

## Core Functions

### creditEarnings

Credits earnings to a host's balance (authorized contracts only).

```solidity
function creditEarnings(
    address host,
    uint256 amount,
    address token
) external onlyAuthorized
```

#### Parameters
| Name | Type | Description |
|------|------|-------------|
| `host` | `address` | Host address to credit |
| `amount` | `uint256` | Amount to credit |
| `token` | `address` | Token address (typically USDC) |

#### Requirements
- Only authorized contracts (PaymentEscrowWithEarnings)
- Valid host address
- Amount > 0

#### Effects
- Increases host's balance for specified token
- Updates total earnings tracking
- Emits EarningsCredited event

### withdraw

Withdraws a specific amount of accumulated earnings.

```solidity
function withdraw(uint256 amount, address token) external nonReentrant
```

#### Parameters
| Name | Type | Description |
|------|------|-------------|
| `amount` | `uint256` | Amount to withdraw |
| `token` | `address` | Token address |

#### Requirements
- Sufficient balance
- Non-zero amount
- Reentrancy protected

#### Effects
- Deducts from host's balance
- Transfers tokens to host
- Emits EarningsWithdrawn event

### withdrawAll

Withdraws all accumulated earnings for a specific token.

```solidity
function withdrawAll(address token) external nonReentrant
```

#### Parameters
| Name | Type | Description |
|------|------|-------------|
| `token` | `address` | Token address to withdraw |

#### Requirements
- Non-zero balance
- Reentrancy protected

#### Effects
- Zeroes host's balance
- Transfers all tokens to host
- Emits EarningsWithdrawn event

#### Example Usage
```javascript
// Check accumulated USDC
const balance = await hostEarnings.getBalance(hostAddress, USDC_ADDRESS);
console.log("Accumulated:", ethers.formatUnits(balance, 6), "USDC");

// Withdraw all USDC
await hostEarnings.withdrawAll(USDC_ADDRESS);
```

### withdrawMultiple

Withdraws earnings from multiple tokens in one transaction.

```solidity
function withdrawMultiple(address[] calldata tokens) external nonReentrant
```

#### Parameters
| Name | Type | Description |
|------|------|-------------|
| `tokens` | `address[]` | Array of token addresses |

#### Benefits
- Single transaction for multiple tokens
- Further gas savings
- Convenient for hosts with diverse earnings

### getBalance

View function to check accumulated earnings.

```solidity
function getBalance(address host, address token) external view returns (uint256)
```

#### Returns
Current balance of specified token for the host.

### getBalances

Get balances for multiple tokens.

```solidity
function getBalances(address host, address[] calldata tokens) 
    external view returns (uint256[] memory)
```

#### Returns
Array of balances corresponding to input tokens.

## Access Control Functions

### addAuthorized

Adds an authorized contract to credit earnings.

```solidity
function addAuthorized(address account) external onlyOwner
```

#### Requirements
- Only owner
- Valid address
- Not already authorized

### removeAuthorized

Removes authorization from a contract.

```solidity
function removeAuthorized(address account) external onlyOwner
```

#### Requirements
- Only owner
- Currently authorized

## Emergency Functions

### emergencyWithdraw

Owner-only emergency withdrawal function.

```solidity
function emergencyWithdraw(address token, uint256 amount) external onlyOwner
```

#### Parameters
| Name | Type | Description |
|------|------|-------------|
| `token` | `address` | Token to withdraw |
| `amount` | `uint256` | Amount to withdraw |

#### Use Cases
- Contract migration
- Emergency recovery
- System maintenance

## Events

```solidity
event EarningsCredited(address indexed host, uint256 amount, address indexed token);
event EarningsWithdrawn(address indexed host, uint256 amount, address indexed token);
event AuthorizedAdded(address indexed account);
event AuthorizedRemoved(address indexed account);
```

## Integration Flow

### How Earnings Accumulation Works

1. **Job Completion**: Host completes job in JobMarketplaceFABWithEarnings
2. **Payment Release**: JobMarketplace calls PaymentEscrowWithEarnings.releaseToEarnings()
3. **Earnings Credit**: PaymentEscrow credits 90% to HostEarnings (10% to Treasury)
4. **Accumulation**: Host's balance increases, no gas-heavy transfer
5. **Batch Withdrawal**: Host withdraws accumulated earnings when convenient

### Integration Example

```javascript
// 1. Complete multiple jobs (accumulates earnings)
for (let jobId of [1, 2, 3, 4, 5]) {
    await marketplace.completeJob(jobId, resultHash, proof);
    // Each completion: ~69,000 gas (vs 115,000 for direct transfer)
}

// 2. Check accumulated earnings
const hostEarnings = await ethers.getContractAt(
    "HostEarnings",
    "0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E"
);

const balance = await hostEarnings.getBalance(
    hostAddress,
    USDC_ADDRESS
);
console.log("Total accumulated:", ethers.formatUnits(balance, 6), "USDC");

// 3. Withdraw all at once
const tx = await hostEarnings.withdrawAll(USDC_ADDRESS);
// Single withdrawal: ~115,000 gas
// Total: 345,000 + 115,000 = 460,000 gas
// Savings: 115,000 gas (20% for 5 jobs)
```

## Security Considerations

1. **Reentrancy Protection**: All withdrawal functions use `nonReentrant`
2. **Access Control**: Only authorized contracts can credit earnings
3. **Balance Tracking**: Accurate internal accounting prevents overflows
4. **Emergency Controls**: Owner can recover funds if needed
5. **Token Safety**: Handles ERC20 transfers safely

## Gas Optimization Benefits

### Per-Job Savings
- **Traditional**: ~115,000 gas per job (direct USDC transfer)
- **With Accumulation**: ~69,000 gas per job (balance update only)
- **Savings**: 46,000 gas per job (40% reduction)

### Batch Processing Benefits
```
10 jobs traditional: 10 × 115,000 = 1,150,000 gas
10 jobs accumulated: (10 × 69,000) + 115,000 = 805,000 gas
Net savings: 345,000 gas (30% reduction)
```

### Optimal Withdrawal Strategy
- Accumulate earnings from 5-10 jobs
- Withdraw during low gas periods
- Use withdrawMultiple for multiple tokens

## Best Practices

### For Hosts
1. Let earnings accumulate before withdrawing
2. Monitor balance regularly
3. Withdraw during low network congestion
4. Keep some ETH for withdrawal gas

### For Integration
1. Always check authorization before crediting
2. Validate amounts before crediting
3. Handle withdrawal failures gracefully
4. Monitor total earnings for accounting

## Example Scenarios

### Scenario 1: Daily Host Operations
```javascript
// Morning: Complete 20 jobs
for (let i = 0; i < 20; i++) {
    await completeJob(i); // 69,000 gas each
}
// Total: 1,380,000 gas

// Evening: Withdraw earnings
await hostEarnings.withdrawAll(USDC); // 115,000 gas
// Total: 1,495,000 gas (vs 2,300,000 traditional)
// Savings: 805,000 gas (35%)
```

### Scenario 2: Weekly Batch Withdrawal
```javascript
// Complete 100 jobs over a week
// Gas: 100 × 69,000 = 6,900,000

// Single withdrawal on weekend
// Gas: 115,000

// Total: 7,015,000 (vs 11,500,000 traditional)
// Savings: 4,485,000 gas (39%)
```

## Limitations & Future Improvements

1. **No Auto-Withdrawal**: Hosts must manually withdraw
2. **No Withdrawal Scheduling**: Could add time-based withdrawals
3. **Single Token Optimization**: Could batch different token withdrawals
4. **No Delegation**: Hosts cannot delegate withdrawal rights
5. **No Partial Authorization**: Binary authorization model

## Related Contracts

- **JobMarketplaceFABWithEarnings**: [`0xEB646BF2323a441698B256623F858c8787d70f9F`](./JobMarketplace.md)
- **PaymentEscrowWithEarnings**: [`0x7abC91AF9E5aaFdc954Ec7a02238d0796Bbf9a3C`](./PaymentEscrow.md)
- **TreasuryManager**: [`0x4e770e723B95A0d8923Db006E49A8a3cb0BAA078`](./TreasuryManager.md)

## Contract Verification

Verified on BaseScan: [0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E](https://sepolia.basescan.org/address/0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E#code)