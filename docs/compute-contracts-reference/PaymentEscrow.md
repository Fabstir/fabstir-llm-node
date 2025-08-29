# PaymentEscrowWithEarnings Contract

## Overview

The PaymentEscrowWithEarnings contract provides a secure multi-token escrow system for the Fabstir marketplace with integrated host earnings accumulation. It holds payments during job execution and handles release to the HostEarnings contract for gas-efficient batch withdrawals, supporting both ETH and ERC20 tokens (primarily USDC).

**Contract Address (Base Sepolia)**: `0x7abC91AF9E5aaFdc954Ec7a02238d0796Bbf9a3C` (LATEST - with earnings accumulation)  
**Previous Version**: `0xF382E11ebdB90e6cDE55521C659B70eEAc1C9ac3` (direct payment, deprecated)  
**Source**: [`src/PaymentEscrowWithEarnings.sol`](../../../src/PaymentEscrowWithEarnings.sol)

### Key Features
- Multi-token support (ETH and ERC20, especially USDC)
- Fee collection mechanism (10% = 1000 basis points)
- **NEW: Earnings accumulation via HostEarnings contract**
- **NEW: 40-46% gas savings for hosts completing multiple jobs**
- Direct payment release from JobMarketplaceFABWithEarnings
- TreasuryManager integration for fee distribution
- Dispute resolution with arbiter (optional)
- Refund request workflow
- Migration support
- Reentrancy protection

### Dependencies
- OpenZeppelin ReentrancyGuard
- OpenZeppelin Ownable
- IERC20 interface

## Constructor

```solidity
constructor(address _arbiter, uint256 _feeBasisPoints) Ownable(msg.sender)
```

### Parameters
| Name | Type | Description |
|------|------|-------------|
| `_arbiter` | `address` | Address authorized to resolve disputes |
| `_feeBasisPoints` | `uint256` | Fee percentage in basis points (e.g., 1000 = 10%) |

### Example Deployment
```solidity
// Deploy with 10% fee (1000 basis points), TreasuryManager as arbiter
PaymentEscrowWithEarnings escrow = new PaymentEscrowWithEarnings(TREASURY_MANAGER_ADDRESS, 1000);

// Set JobMarketplaceFABWithEarnings as authorized marketplace
escrow.setJobMarketplace(JOB_MARKETPLACE_FAB_WITH_EARNINGS_ADDRESS);
```

## State Variables

### Public Variables
| Name | Type | Description |
|------|------|-------------|
| `arbiter` | `address` | Address that can resolve disputes |
| `feeBasisPoints` | `uint256` | Platform fee in basis points |
| `feeBalance` | `uint256` | Accumulated ETH fees |
| `tokenFeeBalances` | `mapping(address => uint256)` | Accumulated token fees |
| `jobMarketplace` | `address` | Authorized JobMarketplace contract |
| `migrationHelper` | `address` | Address for migration operations |

### Escrow Structure
```solidity
struct Escrow {
    address renter;          // Job poster
    address host;           // Service provider
    uint256 amount;         // Payment amount
    address token;          // Token address (0x0 for ETH)
    EscrowStatus status;    // Current status
    bool refundRequested;   // Refund flag
}
```

### Escrow Status
```solidity
enum EscrowStatus {
    Active,     // Payment locked
    Released,   // Payment sent to host
    Disputed,   // Under dispute
    Resolved,   // Dispute resolved
    Refunded    // Payment returned to renter
}
```

## Core Functions

### setJobMarketplace

Configure the authorized JobMarketplace contract.

```solidity
function setJobMarketplace(address _jobMarketplace) external onlyOwner
```

#### Requirements
- Only owner
- Valid contract address (checked via extcodesize)

#### Security
- Validates address is a contract
- One-way setting (cannot be changed once set)

### createEscrow

Create a new escrow for a job (JobMarketplace only).

```solidity
function createEscrow(
    bytes32 _jobId,
    address _host,
    uint256 _amount,
    address _token
) external payable onlyMarketplace
```

#### Parameters
| Name | Type | Description |
|------|------|-------------|
| `_jobId` | `bytes32` | Unique job identifier |
| `_host` | `address` | Host who will receive payment |
| `_amount` | `uint256` | Payment amount |
| `_token` | `address` | Token address (0x0 for ETH) |

#### Requirements
- Only callable by JobMarketplace
- Escrow doesn't already exist
- Valid host address
- Amount > 0
- For ETH: msg.value == amount
- For ERC20: msg.value == 0

#### Emitted Events
- `EscrowCreated(bytes32 indexed jobId, address indexed renter, address indexed host, uint256 amount, address token)`

#### Example Usage
```solidity
// ETH payment
escrow.createEscrow{value: 1 ether}(jobId, hostAddress, 1 ether, address(0));

// ERC20 payment (requires prior approval)
escrow.createEscrow(jobId, hostAddress, 1000e18, tokenAddress);
```

### releaseToEarnings (NEW)

Release payment to HostEarnings contract for accumulation instead of direct transfer.

```solidity
function releaseToEarnings(
    bytes32 _jobId,
    address _host,
    uint256 _amount,
    address _token,
    address _hostEarnings
) external onlyMarketplace nonReentrant
```

#### Parameters
| Name | Type | Description |
|------|------|-------------|
| `_jobId` | `bytes32` | Job identifier for tracking |
| `_host` | `address` | Host address for earnings credit |
| `_amount` | `uint256` | Total payment amount |
| `_token` | `address` | Token address (typically USDC) |
| `_hostEarnings` | `address` | HostEarnings contract address |

#### Requirements
- Only callable by JobMarketplace
- Valid host address
- Amount > 0
- Reentrancy protected
- Token must have sufficient balance

#### Effects
- Deducts platform fee (10% = 1000 basis points)
- **Credits net amount to host's earnings balance**
- Transfers fee to TreasuryManager immediately
- No escrow record created
- **No direct transfer to host (accumulated for later withdrawal)**

#### Fee Calculation
```solidity
fee = (amount * feeBasisPoints) / 10000
payment = amount - fee
```

#### Example Usage
```solidity
// Called by JobMarketplaceFABWithEarnings when USDC job completes
escrow.releaseToEarnings(
    jobId,
    hostAddress,
    10000000,  // 10 USDC (6 decimals)
    usdcAddress,
    hostEarningsAddress
);
// Host earnings credited: 9,000,000 (90% after 10% fee)
// TreasuryManager receives: 1,000,000 (10% fee)
// Host can withdraw accumulated earnings later
```

### releaseEscrow

Release payment to host after job completion.

```solidity
function releaseEscrow(bytes32 _jobId) external onlyParties(_jobId) nonReentrant
```

#### Requirements
- Caller must be renter or host
- Escrow status must be Active
- Reentrancy protected

#### Fee Calculation
```solidity
fee = (amount * feeBasisPoints) / 10000
payment = amount - fee
```

#### Effects
- Deducts platform fee
- Transfers payment to host
- Updates fee balances
- Changes status to Released

#### Emitted Events
- `EscrowReleased(bytes32 indexed jobId, uint256 amount, uint256 fee)`

#### Gas Considerations
- ETH transfers use low-level call
- ERC20 transfers use transfer (not safeTransfer)

### disputeEscrow

Initiate a dispute on active escrow.

```solidity
function disputeEscrow(bytes32 _jobId) external onlyParties(_jobId)
```

#### Requirements
- Caller must be renter or host
- Escrow must be Active

#### Effects
- Changes status to Disputed
- Prevents normal release

#### Emitted Events
- `EscrowDisputed(bytes32 indexed jobId, address disputer)`

### resolveDispute

Arbiter resolves a disputed escrow.

```solidity
function resolveDispute(bytes32 _jobId, address _winner) external onlyArbiter nonReentrant
```

#### Parameters
| Name | Type | Description |
|------|------|-------------|
| `_jobId` | `bytes32` | Job ID |
| `_winner` | `address` | Either renter or host |

#### Requirements
- Only arbiter
- Escrow in Disputed status
- Winner must be renter or host
- Reentrancy protected

#### Effects
- If host wins: Payment released with fee
- If renter wins: Full refund

#### Emitted Events
- `DisputeResolved(bytes32 indexed jobId, address winner)`

### requestRefund

Host requests refund (job cannot be completed).

```solidity
function requestRefund(bytes32 _jobId) external onlyParties(_jobId)
```

#### Requirements
- Only host can request
- Escrow must be Active

#### Effects
- Sets refundRequested flag
- Requires renter confirmation

#### Emitted Events
- `RefundRequested(bytes32 indexed jobId)`

### confirmRefund

Renter confirms refund request.

```solidity
function confirmRefund(bytes32 _jobId) external onlyParties(_jobId) nonReentrant
```

#### Requirements
- Only renter can confirm
- Refund must be requested
- Escrow must be Active
- Reentrancy protected

#### Effects
- Full refund to renter
- Status changed to Refunded

#### Emitted Events
- `EscrowRefunded(bytes32 indexed jobId)`

### getEscrow

Retrieve escrow details.

```solidity
function getEscrow(bytes32 _jobId) external view returns (Escrow memory)
```

#### Returns
Complete Escrow struct with all fields.

## Migration Functions

### setMigrationHelper

Set authorized migration address.

```solidity
function setMigrationHelper(address _migrationHelper) external onlyOwner
```

### addMigratedEscrow

Add escrow from previous contract version.

```solidity
function addMigratedEscrow(
    bytes32 escrowId,
    address payer,
    address payee,
    uint256 amount,
    address token,
    uint256 releaseTime,
    bool isReleased,
    bool isRefunded
) external onlyMigrationHelper
```

#### Access Control
- Only migration helper
- Used during contract upgrades

### emergencyWithdraw

Emergency fund withdrawal.

```solidity
function emergencyWithdraw(address to, uint256 amount) external
```

#### Access Control
- Owner or migration helper only
- For emergency recovery

### getActiveEscrowIds

Get list of active escrows.

```solidity
function getActiveEscrowIds() external view returns (bytes32[] memory)
```

#### Note
Currently returns empty array - proper tracking needed in production.

## Integration with JobMarketplaceFABWithEarnings

The PaymentEscrowWithEarnings contract is tightly integrated with JobMarketplaceFABWithEarnings for USDC payment handling with earnings accumulation:

### Payment Flow
1. **Job Posting**: JobMarketplaceFABWithEarnings transfers USDC from renter to PaymentEscrow
2. **Job Completion**: JobMarketplaceFABWithEarnings calls `releaseToEarnings()` to credit earnings
3. **Fee Distribution**: 10% fee sent to TreasuryManager, 90% credited to host's earnings
4. **Batch Withdrawal**: Host withdraws accumulated earnings when convenient

### Key Integration Functions

```solidity
function releaseToEarnings(
    bytes32 _jobId,
    address _host,
    uint256 _amount,
    address _token,
    address _hostEarnings
) external onlyMarketplace nonReentrant
```

This function is called by JobMarketplaceFABWithEarnings when a job is completed:
- **Access**: Only callable by authorized JobMarketplace
- **Fee**: Automatically deducts 10% platform fee
- **Payment**: Credits 90% to host's earnings balance
- **Token**: Supports USDC and other ERC20 tokens
- **Gas Savings**: 40-46% reduction for multiple job completions

### Configuration for JobMarketplaceFABWithEarnings

```javascript
// Current production configuration on Base Sepolia
const PAYMENT_ESCROW = "0x7abC91AF9E5aaFdc954Ec7a02238d0796Bbf9a3C"; // LATEST
const JOB_MARKETPLACE_FAB = "0xEB646BF2323a441698B256623F858c8787d70f9F"; // LATEST
const HOST_EARNINGS = "0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E"; // NEW
const TREASURY_MANAGER = "0x4e770e723B95A0d8923Db006E49A8a3cb0BAA078";
const USDC = "0x036CbD53842c5426634e7929541eC2318f3dCF7e";

// Fee configuration
const FEE_BASIS_POINTS = 1000; // 10% fee
```

## Events

### Escrow Lifecycle
```solidity
event EscrowCreated(bytes32 indexed jobId, address indexed renter, address indexed host, uint256 amount, address token)
event EscrowReleased(bytes32 indexed jobId, uint256 amount, uint256 fee)
event EscrowRefunded(bytes32 indexed jobId)
```

### Dispute Events
```solidity
event EscrowDisputed(bytes32 indexed jobId, address disputer)
event DisputeResolved(bytes32 indexed jobId, address winner)
```

### Refund Flow
```solidity
event RefundRequested(bytes32 indexed jobId)
```

## Access Modifiers

### onlyArbiter
```solidity
modifier onlyArbiter()
```
Restricts function to arbiter address.

### onlyParties
```solidity
modifier onlyParties(bytes32 jobId)
```
Restricts function to renter or host of specific escrow.

### onlyMarketplace
```solidity
modifier onlyMarketplace()
```
Restricts function to JobMarketplace contract.

## Security Considerations

1. **Reentrancy Protection**: 
   - All payment functions use nonReentrant modifier
   - State changes before external calls

2. **Access Control**:
   - Owner functions for configuration
   - Arbiter for dispute resolution
   - Party restrictions for escrow actions

3. **Input Validation**:
   - Zero address checks
   - Amount validation
   - Contract existence verification

4. **Token Safety**:
   - Separate handling for ETH and ERC20
   - No assumptions about token behavior
   - Fee calculation overflow protection

5. **Status Management**:
   - Clear state transitions
   - No duplicate operations
   - Status checks before actions

## Gas Optimization

1. **Storage Efficiency**:
   - Escrow struct optimized for packing
   - Minimal storage updates

2. **Earnings Accumulation (NEW)**:
   - Host earnings accumulated in HostEarnings contract
   - **40-46% gas reduction for multiple job completions**
   - Batch withdrawal significantly reduces per-job gas cost
   - Example: 5 jobs save ~220,000 gas vs direct transfers

3. **Fee Distribution**:
   - Fees sent directly to TreasuryManager
   - No intermediate accumulation needed

4. **Transfer Patterns**:
   - Low-level calls for ETH
   - Optimized ERC20 transfers via HostEarnings

## Integration Examples

### Basic ETH Escrow Flow
```solidity
// 1. JobMarketplace creates escrow
bytes32 jobId = keccak256(abi.encode(jobIdNumber));
escrow.createEscrow{value: 1 ether}(jobId, hostAddress, 1 ether, address(0));

// 2. After job completion, release payment
escrow.releaseEscrow(jobId);

// Host receives: 0.975 ETH (assuming 2.5% fee)
// Platform fee: 0.025 ETH
```

### ERC20 Token Escrow
```solidity
// 1. Renter approves token transfer
IERC20(token).approve(address(escrow), amount);

// 2. Create escrow
escrow.createEscrow(jobId, hostAddress, amount, token);

// 3. Release with fee deduction
escrow.releaseEscrow(jobId);
```

### Dispute Resolution Flow
```solidity
// 1. Either party initiates dispute
escrow.disputeEscrow(jobId);

// 2. Arbiter investigates and resolves
escrow.resolveDispute(jobId, winnerAddress);
```

### Refund Flow
```solidity
// 1. Host requests refund (cannot complete job)
escrow.requestRefund(jobId);

// 2. Renter confirms refund
escrow.confirmRefund(jobId);
```

### Fee Withdrawal Pattern
```solidity
// Check accumulated fees
uint256 ethFees = escrow.feeBalance();
uint256 tokenFees = escrow.tokenFeeBalances(tokenAddress);

// Owner withdraws fees (separate implementation needed)
```

## Best Practices

1. **Token Approvals**: Always approve exact amounts before createEscrow
2. **Job ID Generation**: Use consistent hashing method across contracts
3. **Fee Settings**: Consider fee impact on small payments
4. **Dispute Timeline**: Establish clear dispute resolution timelines
5. **Emergency Procedures**: Document emergency withdrawal conditions

## Limitations & Future Improvements

1. **Active Escrow Tracking**: getActiveEscrowIds needs implementation
2. **Partial Payments**: Currently only supports full payment/refund
3. **Time Locks**: No automatic release after time period
4. **Multi-sig**: Consider multi-sig for arbiter role
5. **Emergency Withdrawal**: Enhanced controls for earnings contract

## Related Contracts

- **HostEarnings**: [`0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E`](./HostEarnings.md)
- **JobMarketplaceFABWithEarnings**: [`0xEB646BF2323a441698B256623F858c8787d70f9F`](./JobMarketplace.md)
- **TreasuryManager**: [`0x4e770e723B95A0d8923Db006E49A8a3cb0BAA078`](./TreasuryManager.md)