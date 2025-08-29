# JobMarketplaceFABWithEarnings Contract

## Overview

The JobMarketplaceFABWithEarnings contract is an enhanced version of JobMarketplaceFAB that implements host earnings accumulation for significant gas savings. It maintains FAB token staking and USDC payment functionality while reducing gas costs by 40-46% for hosts completing multiple jobs.

**Contract Address (Base Sepolia)**: `0xEB646BF2323a441698B256623F858c8787d70f9F` (LATEST - with earnings accumulation)  
**Previous Version**: `0x870E74D1Fe7D9097deC27651f67422B598b689Cd` (direct payment, deprecated)  
**Source**: [`src/JobMarketplaceFABWithEarnings.sol`](../../../src/JobMarketplaceFABWithEarnings.sol)

### Key Features
- Integration with NodeRegistryFAB for FAB-based host verification
- USDC payment support through PaymentEscrowWithEarnings
- **NEW: Host earnings accumulation via HostEarnings contract**
- **NEW: 40-46% gas savings for multiple job completions**
- Job lifecycle management (Posted → Claimed → Completed)
- Automatic earnings credit with 10% platform fee
- Deadline enforcement for job completion

### Dependencies
- NodeRegistryFAB (for host verification)
- PaymentEscrowWithEarnings (for USDC escrow with earnings routing)
- HostEarnings (for accumulating host payments)
- OpenZeppelin ReentrancyGuard
- IERC20 (for USDC transfers)

## Constructor

```solidity
constructor(address _nodeRegistry, address payable _hostEarnings)
```

### Parameters
| Name | Type | Description |
|------|------|-------------|
| `_nodeRegistry` | `address` | Address of NodeRegistryFAB contract |
| `_hostEarnings` | `address payable` | Address of HostEarnings contract |

### Example Deployment
```solidity
// Deploy with NodeRegistryFAB and HostEarnings addresses
JobMarketplaceFABWithEarnings marketplace = new JobMarketplaceFABWithEarnings(
    0x87516C13Ea2f99de598665e14cab64E191A0f8c4,  // NodeRegistryFAB
    0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E   // HostEarnings
);
// Configure USDC
marketplace.setUsdcAddress(USDC_ADDRESS);
// Set PaymentEscrowWithEarnings
marketplace.setPaymentEscrow(PAYMENT_ESCROW_ADDRESS);
```

## State Variables

### Constants
| Name | Type | Value | Description |
|------|------|-------|-------------|
| `MAX_PAYMENT` | `uint256` | 1000 ether | Maximum payment amount per job |
| `usdcAddress` | `address` | Configurable | USDC token address (Base Sepolia: `0x036CbD53842c5426634e7929541eC2318f3dCF7e`) |

### Public Variables
| Name | Type | Description |
|------|------|-------------|
| `nodeRegistry` | `NodeRegistryFAB` | FAB-based node registry contract |
| `paymentEscrow` | `IPaymentEscrow` | Payment escrow contract with earnings routing |
| `hostEarnings` | `HostEarnings` | Host earnings accumulation contract |
| `reputationSystem` | `IReputationSystem` | Optional reputation tracking |
| `nextJobId` | `uint256` | Counter for internal job IDs |
| `totalEarningsCredited` | `mapping(address => uint256)` | Total earnings credited per host |

## Structs

### Job
```solidity
struct Job {
    address renter;           // Job creator address
    JobStatus status;         // Current job status
    address assignedHost;     // Assigned host address
    uint256 maxPrice;         // Payment amount in USDC
    uint256 deadline;         // Completion deadline timestamp
    uint256 completedAt;      // Completion timestamp
    address paymentToken;     // Payment token (USDC)
    bytes32 escrowId;        // PaymentEscrow job ID
    string modelId;          // AI model identifier
    string inputHash;        // Input data hash
    string resultHash;       // Result data hash
}
```

### JobStatus
```solidity
enum JobStatus {
    Posted,    // Job created and funded
    Claimed,   // Job assigned to host
    Completed  // Job finished and paid
}
```

## Core Functions

### postJobWithToken
Posts a new job with USDC payment.

```solidity
function postJobWithToken(
    IJobMarketplace.JobDetails memory details,
    IJobMarketplace.JobRequirements memory requirements,
    address paymentToken,
    uint256 paymentAmount
) external nonReentrant returns (bytes32)
```

**Parameters:**
- `details`: Job execution details (model, prompt, parameters)
- `requirements`: Job requirements (GPU memory, time limit, etc.)
- `paymentToken`: Must be USDC address
- `paymentAmount`: Payment in USDC (6 decimals)

**Requirements:**
- Payment token must be USDC
- Payment amount must be positive
- User must approve USDC transfer
- PaymentEscrow must be configured

**Example:**
```javascript
// Approve USDC
await usdc.approve(marketplace.address, "10000000"); // 10 USDC

// Create job details
const details = {
    modelId: "gpt-4",
    prompt: "Analyze this dataset",
    maxTokens: 1000,
    temperature: 70,
    seed: 42,
    resultFormat: "json"
};

const requirements = {
    minGPUMemory: 16,
    minReputationScore: 0,
    maxTimeToComplete: 3600,
    requiresProof: false
};

// Post job
const jobId = await marketplace.postJobWithToken(
    details,
    requirements,
    USDC_ADDRESS,
    "10000000" // 10 USDC
);
```

### claimJob
Claims an available job (host only).

```solidity
function claimJob(uint256 _jobId) external
```

**Requirements:**
- Job must exist and be in Posted status
- Job must not be expired
- Caller must be registered in NodeRegistryFAB
- Caller must have active status
- Caller must have at least 1000 FAB staked

**Effects:**
- Assigns job to caller
- Changes status to Claimed
- Emits JobClaimed event

### completeJob
Completes a claimed job and triggers payment.

```solidity
function completeJob(
    uint256 _jobId,
    string memory _resultHash,
    bytes memory _proof
) external nonReentrant
```

**Requirements:**
- Caller must be the assigned host
- Job must be in Claimed status
- Must be before deadline
- Result hash must be provided

**Effects:**
- Changes status to Completed
- Credits earnings to host's balance (minus 10% fee)
- Updates reputation if available
- Emits JobCompleted and EarningsCredited events

**Payment Flow:**
1. PaymentEscrowWithEarnings routes payment to HostEarnings
2. 10% fee is deducted (1000 basis points) and sent to TreasuryManager
3. Host's earnings balance credited with 90% of payment
4. Host can withdraw accumulated earnings anytime

### getJob
Retrieves job details.

```solidity
function getJob(uint256 _jobId) external view returns (
    address renter,
    uint256 payment,
    JobStatus status,
    address assignedHost,
    string memory resultHash,
    uint256 deadline
)
```

### setPaymentEscrow
Sets the PaymentEscrow contract (one-time configuration).

```solidity
function setPaymentEscrow(address _paymentEscrow) external
```

**Requirements:**
- Can only be set once
- Must be valid contract address

### setUsdcAddress
Configures USDC token address.

```solidity
function setUsdcAddress(address _usdc) external
```

## Events

```solidity
event JobCreated(uint256 indexed jobId, address indexed renter, string modelId, uint256 maxPrice);
event JobCreatedWithToken(bytes32 indexed jobId, address indexed renter, address paymentToken, uint256 paymentAmount);
event JobClaimed(uint256 indexed jobId, address indexed host);
event JobCompleted(uint256 indexed jobId, string resultHash);
event EarningsCredited(address indexed host, uint256 amount, address token);
```

## Integration Flow

### Complete Job Lifecycle
1. **Host Registration** (in NodeRegistryFAB)
   - Host stakes 1000 FAB tokens
   - Registers with metadata

2. **Job Posting**
   - Renter approves USDC to marketplace
   - Posts job with payment
   - USDC transferred to PaymentEscrow

3. **Job Claiming**
   - Host claims job
   - Marketplace verifies FAB stake
   - Job assigned to host

4. **Job Completion**
   - Host completes work
   - Submits result hash
   - Payment released automatically

5. **Payment Settlement**
   - 90% credited to host's earnings (e.g., 9 USDC from 10 USDC job)
   - 10% platform fee to TreasuryManager

6. **Earnings Withdrawal** (NEW)
   - Host can withdraw accumulated earnings
   - Batch withdrawal saves gas

## Security Considerations

1. **Reentrancy Protection**: Critical functions use `nonReentrant`
2. **Deadline Enforcement**: Jobs expire if not completed on time
3. **Host Verification**: Only FAB-staked hosts can claim jobs
4. **Payment Safety**: USDC held in escrow until completion
5. **One-time Configuration**: Critical addresses can only be set once

## Gas Optimization

- **NEW: Earnings accumulation saves 40-46% gas**
  - Direct transfer: ~115,000 gas per job
  - Accumulation: ~69,000 gas per job
  - Example: 5 jobs save ~220,000 gas total
- Efficient storage packing in Job struct
- Minimal external calls during critical operations
- Batch withdrawal reduces per-job gas cost

## Differences from Original JobMarketplace

| Feature | JobMarketplace | JobMarketplaceFAB |
|---------|----------------|-------------------|
| Host Registry | NodeRegistry (ETH) | NodeRegistryFAB (FAB) |
| Minimum Stake | 100 ETH | 1000 FAB |
| Payment Support | ETH + ERC20 | USDC (configurable) |
| Host Verification | `getNode()` method | `nodes()` mapping |
| Deployment | Single | With dependencies |

## Example Usage

### Complete Flow Example
```javascript
// 1. Setup contracts
const marketplace = await ethers.getContractAt("JobMarketplaceFAB", MARKETPLACE_ADDRESS);
const usdc = await ethers.getContractAt("IERC20", USDC_ADDRESS);

// 2. Post job (as renter)
await usdc.approve(marketplace.address, ethers.parseUnits("10", 6));
const tx = await marketplace.postJobWithToken(
    details,
    requirements,
    USDC_ADDRESS,
    ethers.parseUnits("10", 6)
);
const receipt = await tx.wait();
const jobId = 1; // First job

// 3. Claim job (as host)
await marketplace.claimJob(jobId);

// 4. Complete job (as host)
await marketplace.completeJob(jobId, "QmResultHash...", "0x");
// Earnings are credited, not transferred directly

// 5. Check accumulated earnings
const hostEarnings = await ethers.getContractAt("HostEarnings", HOST_EARNINGS_ADDRESS);
const earnings = await hostEarnings.getBalance(hostAddress, USDC_ADDRESS);
console.log("Host earnings:", earnings); // 9 USDC credited

// 6. Withdraw accumulated earnings (can be done later)
await hostEarnings.withdrawAll(USDC_ADDRESS);
const balance = await usdc.balanceOf(hostAddress);
console.log("Host balance after withdrawal:", balance); // 9 USDC received
```

## Deployed Addresses

| Network | JobMarketplaceFABWithEarnings | NodeRegistryFAB | PaymentEscrowWithEarnings | HostEarnings |
|---------|-------------------------------|-----------------|---------------------------|-------------|
| Base Sepolia | `0xEB646BF2323a441698B256623F858c8787d70f9F` | `0x87516C13Ea2f99de598665e14cab64E191A0f8c4` | `0x7abC91AF9E5aaFdc954Ec7a02238d0796Bbf9a3C` | `0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E` |
| Base Mainnet | TBD | TBD | TBD | TBD |

## Verified Transactions

Example successful job flow on Base Sepolia:
1. FAB Transfer: [0xdf21f074635f5b03a78d3acd7ea90056779759b0b14feba0c042e9d3224a9067](https://sepolia.basescan.org/tx/0xdf21f074635f5b03a78d3acd7ea90056779759b0b14feba0c042e9d3224a9067)
2. Host Registration: [0xa193198058e70343105b8e8306fa8600421c77417658ad5780b03a202b3666dc](https://sepolia.basescan.org/tx/0xa193198058e70343105b8e8306fa8600421c77417658ad5780b03a202b3666dc)
3. Job Posted: [0xd186457017d07e7ee5e858c9ca3862bac964624629a8581a77e8ba9a9acd6d8f](https://sepolia.basescan.org/tx/0xd186457017d07e7ee5e858c9ca3862bac964624629a8581a77e8ba9a9acd6d8f)
4. Job Claimed: [0xb6995908db02db9620631e15641f3e643f826858cb06c2f955fe2feb0b5fc375](https://sepolia.basescan.org/tx/0xb6995908db02db9620631e15641f3e643f826858cb06c2f955fe2feb0b5fc375)
5. Payment Released: [0x049085aab9e89b8425fd5010c8721a8acb409b952aa9034158b52d0e08062406](https://sepolia.basescan.org/tx/0x049085aab9e89b8425fd5010c8721a8acb409b952aa9034158b52d0e08062406)