# HostEarnings Contract Documentation

## Current Implementation

**Contract Address**: `0x908962e8c6CE72610021586f85ebDE09aAc97776`
**Network**: Base Sepolia | opBNB support planned post-MVP
**Status**: ✅ ACTIVE - Authorized for JobMarketplaceWithModels
**Last Updated**: January 28, 2025

### Overview

The HostEarnings contract provides a gas-efficient earnings accumulation system for Fabstir marketplace hosts. Instead of receiving direct payments on each job completion, hosts' earnings accumulate in this contract and can be withdrawn in batch, saving ~70% in gas costs.

### Key Features
- **Multi-chain Support**: Works with native tokens (ETH on Base, BNB on opBNB)
- **Multi-token Support**: Accumulates native tokens and any ERC20 (USDC, DAI, etc.)
- **Batch Withdrawals**: Withdraw all tokens in a single transaction
- **Gas Efficiency**: 70% reduction in gas costs for hosts
- **Authorization System**: Only authorized contracts can credit earnings
- **Transparent Tracking**: View accumulated balances anytime
- **Anyone-Can-Complete**: Sessions can be completed by any address, host still gets paid
- **Emergency Recovery**: Owner can recover stuck funds

### Architecture

```solidity
contract HostEarnings {
    // Earnings tracking
    mapping(address => mapping(address => uint256)) public earnings; // host => token => amount
    
    // Authorization
    mapping(address => bool) public authorizedCallers;
    address public owner;
    
    // Events
    event EarningsAdded(address indexed host, address indexed token, uint256 amount);
    event EarningsWithdrawn(address indexed host, address indexed token, uint256 amount);
}
```

### Integration with JobMarketplace

The HostEarnings contract works in tandem with JobMarketplaceWithModels (`0xe169A4B57700080725f9553E3Cc69885fea13629` - corrected dual pricing):

1. **Session Completion**: Anyone completes a session (user, host, or third party)
2. **Payment Calculation**: Marketplace calculates 90% host / 10% treasury split
3. **Credit Earnings**: Marketplace calls `creditEarnings()` on HostEarnings
4. **Accumulation**: Host's earnings accumulate across multiple sessions
5. **Batch Withdrawal**: Host withdraws all earnings when convenient
6. **Multi-chain**: Works identically on Base (ETH) and future opBNB (BNB)

**Authorization Status**: JobMarketplaceWithModels is authorized via `setAuthorizedCaller()`

### Key Functions

#### For JobMarketplace (Authorized Callers Only)

```solidity
// Credit earnings to a host
function creditEarnings(
    address host, 
    address token, 
    uint256 amount
) external payable onlyAuthorized

// Credit to multiple hosts (batch operations)
function creditMultiple(
    address[] calldata hosts,
    address[] calldata tokens,
    uint256[] calldata amounts
) external payable onlyAuthorized
```

#### For Hosts

```solidity
// Withdraw all earnings for a specific token
function withdrawAll(address token) external nonReentrant

// Withdraw multiple tokens at once
function withdrawMultiple(
    address[] calldata tokens
) external nonReentrant

// View accumulated earnings
function getEarnings(
    address host, 
    address token
) external view returns (uint256)
```

#### For Owner

```solidity
// Authorize/deauthorize marketplace contracts
function setAuthorizedCaller(
    address caller, 
    bool authorized
) external onlyOwner

// Emergency withdrawal of stuck funds
function emergencyWithdraw(
    address token, 
    uint256 amount
) external onlyOwner
```

### Gas Savings Analysis

The accumulation pattern provides significant gas savings:

| Jobs Completed | Direct Transfer | With Accumulation | Savings |
|----------------|-----------------|-------------------|---------|
| 1 job | 45,000 gas | 27,000 gas | 40% |
| 5 jobs | 225,000 gas | 135,000 gas | 40% |
| 10 jobs | 450,000 gas | 270,000 gas | 40% |
| 50 jobs | 2,250,000 gas | 1,350,000 gas | 40% |
| Withdrawal | N/A | 35,000 gas | One-time |

**Net Savings**: For a host completing 10 jobs:
- Old: 450,000 gas total
- New: 270,000 (credits) + 35,000 (withdrawal) = 305,000 gas
- **Savings: 32%** overall

### Token Support

The contract supports any ERC20 token and native tokens across chains:

#### Base Sepolia (Current)
| Token | Address | Usage |
|-------|---------|-------|
| ETH | `address(0)` | Native currency |
| USDC | `0x036CbD53842c5426634e7929541eC2318f3dCF7e` | Primary stablecoin |
| FAB | `0xC78949004B4EB6dEf2D66e49Cd81231472612D62` | Governance token |

#### opBNB (Future)
| Token | Address | Usage |
|-------|---------|-------|
| BNB | `address(0)` | Native currency |
| USDC | TBD | Stablecoin on opBNB |

### Events

```solidity
event EarningsAdded(address indexed host, address indexed token, uint256 amount)
event EarningsWithdrawn(address indexed host, address indexed token, uint256 amount)
event CallerAuthorized(address indexed caller, bool authorized)
event EmergencyWithdrawal(address indexed token, uint256 amount)
```

### Security Considerations

1. **ReentrancyGuard**: All withdrawal functions protected against reentrancy
2. **Authorization**: Only approved marketplace contracts can credit earnings
3. **No Lock-in**: Hosts can withdraw anytime, no minimum or maximum limits
4. **Emergency Recovery**: Owner can recover truly stuck funds (not host earnings)
5. **Transparent Accounting**: All balances publicly viewable on-chain

### Usage Examples

#### For Hosts - Withdrawing Earnings

```javascript
// Using ethers.js
const hostEarnings = new ethers.Contract(
  "0x908962e8c6CE72610021586f85ebDE09aAc97776",
  HostEarningsABI,
  signer
);

// Check ETH earnings
const ethEarnings = await hostEarnings.getEarnings(
  myAddress, 
  ethers.constants.AddressZero
);

// Check USDC earnings
const usdcAddress = "0x036CbD53842c5426634e7929541eC2318f3dCF7e";
const usdcEarnings = await hostEarnings.getEarnings(
  myAddress,
  usdcAddress
);

// Withdraw all ETH
await hostEarnings.withdrawAll(ethers.constants.AddressZero);

// Withdraw all USDC
await hostEarnings.withdrawAll(usdcAddress);

// Or withdraw both at once
await hostEarnings.withdrawMultiple([
  ethers.constants.AddressZero,
  usdcAddress
]);
```

#### For Marketplace - Crediting Earnings

```solidity
// In JobMarketplaceFABWithS5._sendPayments()
if (address(hostEarnings) != address(0)) {
    // Credit ETH earnings
    hostEarnings.creditEarnings{value: hostPayment}(
        host,
        address(0),
        hostPayment
    );
    
    // Credit USDC earnings
    hostEarnings.creditEarnings(
        host,
        usdcAddress,
        hostPayment
    );
}
```

### Best Practices

1. **For Hosts**:
   - Monitor accumulated earnings regularly
   - Withdraw when gas prices are low
   - Use `withdrawMultiple()` to save gas on multiple tokens

2. **For Marketplace**:
   - Always check if HostEarnings is set before calling
   - Handle both ETH and token credits appropriately
   - Emit events for transparency

3. **For Platform**:
   - Monitor authorized callers list
   - Review emergency withdrawal needs carefully
   - Consider implementing withdrawal incentives

### Migration and Compatibility

The HostEarnings contract is designed to be forward-compatible:

- Can be integrated with new marketplace versions
- Supports any future ERC20 tokens
- Authorization system allows multiple marketplaces
- No migration needed for hosts when marketplace updates

### Gas Optimization Tips

1. **Batch Operations**: Use `withdrawMultiple()` for multiple tokens
2. **Timing**: Withdraw during low gas price periods
3. **Accumulation**: Let earnings accumulate before withdrawing
4. **Single Token**: If only using one token, use `withdrawAll()` instead of `withdrawMultiple()`

### References

- [JobMarketplace.md](./JobMarketplace.md) - Main marketplace contract
- [CURRENT_STATUS.md](../../CURRENT_STATUS.md) - Latest deployment info
- [Source Code](../../../src/HostEarnings.sol) - Contract implementation
- [Tests](../../../test/HostEarnings/) - Test coverage