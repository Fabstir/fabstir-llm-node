# JobMarketplace Contract Documentation

## Current Implementation: JobMarketplaceWithModels (Multi-Chain)

**Contract Address**: `0x462050a4a551c4292586D9c1DE23e3158a9bF3B3` ✅ NEW
**Previous Address**: `0xaa38e7fcf5d7944ef7c836e8451f3bf93b98364f` (deprecated)
**Network**: Base Sepolia (ETH) | opBNB support planned post-MVP
**Status**: ✅ ACTIVE - Host-controlled pricing with multi-chain/multi-wallet support
**Last Updated**: January 28, 2025

### Key Features
- **Host-Controlled Pricing**: Contract enforces client price >= host minimum (NEW)
- **Price Validation**: All session creation functions validate pricing (100-100,000 range)
- **Price Discovery**: Query host pricing before creating sessions
- **Multi-Chain Support**: Native token agnostic (ETH on Base, BNB on opBNB)
- **Wallet Agnostic**: Works with EOA and Smart Contract wallets
- **Deposit/Withdrawal Pattern**: Pre-fund accounts for gasless operations
- **Anyone-Can-Complete**: Any address can complete sessions for gasless UX
- **Model Governance**: Integration with ModelRegistry for approved models only
- **Session-Based Jobs**: Uses `sessionJobs` mapping (NOT `jobs` mapping)
- **Treasury Fee Accumulation**: Treasury fees accumulate for batch withdrawals
- **Host Earnings Accumulation**: Via HostEarnings contract with proper creditEarnings
- **Streaming Payments**: Proof-of-work based token consumption model
- **Multi-Token Support**: Native tokens and ERC20 (USDC: 0x036CbD53842c5426634e7929541eC2318f3dCF7e)
- **EZKL Proof Verification**: Integration with ProofSystem contract
- **Economic Minimums**: MIN_DEPOSIT (0.0002 ETH on Base), MIN_PROVEN_TOKENS (100)
- **Gas Savings**: ~80% reduction through dual accumulation

### Contract Architecture

```solidity
contract JobMarketplaceWithModels {
    // Core components
    NodeRegistryWithModels public nodeRegistry;
    IProofSystem public proofSystem;
    HostEarnings public hostEarnings;

    // Multi-chain configuration
    struct ChainConfig {
        address nativeWrapper;      // WETH/WBNB address
        address stablecoin;         // USDC address
        uint256 minDeposit;         // Min deposit in native token
        string nativeTokenSymbol;   // "ETH" or "BNB"
    }
    ChainConfig public chainConfig;

    // User deposits (wallet agnostic)
    mapping(address => uint256) public userDepositsNative;
    mapping(address => mapping(address => uint256)) public userDepositsToken;

    // Session management
    mapping(uint256 => SessionJob) public sessionJobs;
    mapping(address => uint256[]) public userSessions;
    mapping(address => uint256[]) public hostSessions;
}
```

### Session Job Lifecycle

1. **Creation**: User creates session with deposit
2. **Active**: Host submits periodic proofs of work
3. **Completion**: User or host completes, payments distributed
4. **Settlement**: HOST_EARNINGS_PERCENTAGE to host (accumulated), TREASURY_FEE_PERCENTAGE to treasury (accumulated)

### Key Functions

#### Deposit/Withdrawal Functions (Multi-Chain)
```solidity
// Deposit native token (ETH/BNB)
function depositNative() external payable

// Deposit ERC20 token
function depositToken(address token, uint256 amount) external

// Withdraw native token
function withdrawNative(uint256 amount) external

// Withdraw ERC20 token
function withdrawToken(address token, uint256 amount) external

// Query balances
function getUserBalances(address user, address[] calldata tokens)
    external view returns (uint256[] memory)
```

#### Session Management
```solidity
// Create session with inline payment (backward compatible)
function createSessionJob(
    address host,
    uint256 pricePerToken,
    uint256 maxDuration,
    uint256 proofInterval
) external payable returns (uint256 jobId)

// Create session from deposits (gasless-friendly)
function createSessionFromDeposit(
    address host,
    address token,  // address(0) for native
    uint256 deposit,
    uint256 pricePerToken,
    uint256 duration,
    uint256 proofInterval
) external returns (uint256)

// Submit proof of work
function submitProofOfWork(
    uint256 jobId,
    bytes calldata ekzlProof,
    uint256 tokensInBatch
) external returns (bool verified)

// Complete session (anyone can call)
function completeSessionJob(
    uint256 jobId,
    string memory conversationCID
) external
```

#### Treasury Functions (NEW - January 5, 2025)
```solidity
// Withdraw accumulated ETH fees
function withdrawTreasuryETH() external onlyTreasury nonReentrant

// Withdraw accumulated token fees
function withdrawTreasuryTokens(address token) external onlyTreasury nonReentrant

// Batch withdraw all fees
function withdrawAllTreasuryFees(address[] calldata tokens) external onlyTreasury nonReentrant

// View accumulated fees
function accumulatedTreasuryETH() external view returns (uint256)
function accumulatedTreasuryTokens(address token) external view returns (uint256)
```

### Economic Parameters

| Parameter | Value | Description |
|-----------|-------|-------------|
| MIN_DEPOSIT | 0.0002 ETH | Minimum session deposit |
| MIN_PROVEN_TOKENS | 100 | Minimum tokens per proof |
| TREASURY_FEE_PERCENT | Configurable via env | Treasury fee percentage |
| MIN_SESSION_DURATION | 600 seconds | Minimum session length |
| ABANDONMENT_TIMEOUT | 24 hours | Timeout for inactive sessions |
| DISPUTE_WINDOW | 1 hour | Time to dispute after completion |

### Gas Optimization

The dual accumulation pattern provides significant gas savings:

| Operation | Direct Transfer | With Accumulation | Savings |
|-----------|----------------|-------------------|---------|
| Job Completion | ~70,000 gas | ~14,000 gas | 80% |
| 10 Jobs (Host) | ~700,000 gas | ~140,000 gas | 80% |
| 10 Jobs (Treasury) | ~250,000 gas | ~140,000 gas | 44% |

### Integration with Other Contracts

#### NodeRegistryWithModels (NEW)
- Validates host registration AND pricing
- Checks FAB token stake (1000 FAB minimum)
- Returns 7-field struct (includes minPricePerToken)
- Address: `0xC8dDD546e0993eEB4Df03591208aEDF6336342D7`

**Price Validation Flow**:
```solidity
// In JobMarketplaceWithModels
(, , , , , , uint256 hostMinPrice) = nodeRegistry.getNodeFullInfo(host);
require(node.operator != address(0), "Host not registered");
require(node.active, "Host not active");
require(pricePerToken >= hostMinPrice, "Price below host minimum");  // NEW
```

#### ProofSystem
- Verifies EZKL proofs
- Prevents proof replay attacks
- Address: `0x2ACcc60893872A499700908889B38C5420CBcFD1`

#### HostEarnings
- Accumulates host payments
- Enables batch withdrawals
- Address: `0x908962e8c6CE72610021586f85ebDE09aAc97776`

### Events

```solidity
// Session lifecycle
event SessionJobCreated(uint256 indexed jobId, address indexed user, address indexed host, uint256 deposit, uint256 pricePerToken, uint256 maxDuration)
event ProofSubmitted(uint256 indexed jobId, address indexed host, uint256 tokensClaimed, bytes32 proofHash, bool verified)
event SessionCompleted(uint256 indexed jobId, address indexed completedBy, uint256 tokensPaid, uint256 paymentAmount, uint256 refundAmount)

// Treasury accumulation
event TreasuryFeesAccumulated(uint256 amount, address token)
event TreasuryFeesWithdrawn(uint256 amount, address token)

// Host earnings
event EarningsCredited(address indexed host, uint256 amount, address token)
```

### Security Considerations

1. **ReentrancyGuard**: All state-changing functions protected
2. **Proof Verification**: Integration with ProofSystem prevents fraud
3. **Timeout Protection**: Automatic refunds for abandoned sessions
4. **Access Control**: Treasury-only functions for fee withdrawal
5. **Emergency Withdrawal**: Respects accumulated amounts
6. **Price Validation**: Contract enforces host minimum pricing (prevents under-payment)

### Host-Controlled Pricing (NEW - January 28, 2025)

All session creation functions now validate that client's offered price meets or exceeds the host's minimum:

**Functions with Price Validation**:
- `createSessionJob()` - Native token (ETH) sessions
- `createSessionJobWithToken()` - ERC20 token (USDC) sessions
- `createSessionFromDeposit()` - Pre-funded sessions

**Validation Logic**:
```solidity
(, , , , , , uint256 hostMinPrice) = nodeRegistry.getNodeFullInfo(host);
require(pricePerToken >= hostMinPrice, "Price below host minimum");
```

**Error Handling**:
- Transaction reverts with "Price below host minimum" if client price < host minimum
- Client must query pricing first using `nodeRegistry.getNodePricing(host)`

**Usage Example**:
```javascript
import JobMarketplaceABI from './JobMarketplaceWithModels-CLIENT-ABI.json';
import NodeRegistryABI from './NodeRegistryWithModels-CLIENT-ABI.json';
import { ethers } from 'ethers';

const nodeRegistry = new ethers.Contract(
  '0xC8dDD546e0993eEB4Df03591208aEDF6336342D7',
  NodeRegistryABI,
  provider
);

const marketplace = new ethers.Contract(
  '0x462050a4a551c4292586D9c1DE23e3158a9bF3B3',
  JobMarketplaceABI,
  signer
);

// STEP 1: Query host pricing BEFORE creating session
const hostAddress = '0x...';
const hostMinPrice = await nodeRegistry.getNodePricing(hostAddress);
console.log(`Host minimum: ${hostMinPrice}`);

// STEP 2: Create session with price >= host minimum
const myPricePerToken = Math.max(hostMinPrice, 2500); // At least host minimum
const deposit = ethers.utils.parseEther('0.1'); // 0.1 ETH

// This will REVERT if myPricePerToken < hostMinPrice
const tx = await marketplace.createSessionJob(
  hostAddress,
  myPricePerToken,
  3600, // 1 hour max duration
  100,  // Proof every 100 tokens
  { value: deposit }
);

await tx.wait();
console.log('Session created with validated pricing!');
```

**Price Range**: 100 to 100,000 (0.0001 to 0.1 USDC per token)

### Deployment History

| Date | Address | Features |
|------|---------|----------|
| Jan 28, 2025 | `0x462050a4a551c4292586D9c1DE23e3158a9bF3B3` | ✅ CURRENT - Host-controlled pricing validation |
| Jan 24, 2025 | `0xaa38e7fcf5d7944ef7c836e8451f3bf93b98364f` | Deprecated - Multi-chain/wallet support |
| Jan 13, 2025 | `0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944` | Deprecated - Single chain only |
| Jan 5, 2025 | `0x55A702Ab5034810F5B9720Fe15f83CFcf914F56b` | Deprecated - Treasury accumulation |
| Jan 4, 2025 | `0x9A945fFBe786881AaD92C462Ad0bd8aC177A8069` | Deprecated - Host accumulation |
| Dec 2024 | Various | Earlier versions |

### Multi-Chain Configuration

#### Base Sepolia (Current)
```javascript
{
    nativeWrapper: "0x4200000000000000000000000000000000000006", // WETH
    stablecoin: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",   // USDC
    minDeposit: 0.0002 ETH,
    nativeTokenSymbol: "ETH"
}
```

#### opBNB (Future - Post-MVP)
```javascript
{
    nativeWrapper: "TBD", // WBNB
    stablecoin: "TBD",    // USDC on opBNB
    minDeposit: 0.01 BNB,
    nativeTokenSymbol: "BNB"
}
```

### Best Practices

1. **For Users**:
   - **Query host pricing BEFORE creating sessions** (use `nodeRegistry.getNodePricing()`)
   - Ensure your pricePerToken >= host's minPricePerToken to avoid revert
   - Pre-fund deposits for gasless operations
   - Use `createSessionFromDeposit()` for better gas efficiency
   - Let hosts complete sessions to avoid gas costs
   - Works with both EOA and Smart Wallets

2. **For Hosts**:
   - Set competitive pricing via `nodeRegistry.updatePricing()`
   - Monitor market rates and adjust pricing accordingly
   - Complete sessions to claim payment faster
   - Submit proofs regularly at checkpoint intervals
   - Withdraw accumulated earnings periodically
   - Maintain sufficient FAB stake

3. **For Integrators**:
   - **Always query pricing before session creation**
   - Handle "Price below host minimum" errors gracefully
   - Support both inline payment and pre-funded patterns
   - Track `depositor` field, not just `msg.sender`
   - Enable anyone-can-complete for better UX
   - Test with different wallet types
   - Handle 7-field struct from `nodeRegistry.getNodeFullInfo()`

### References

- [NodeRegistry.md](./NodeRegistry.md) - Host registration and pricing documentation
- [IMPLEMENTATION-MARKET.md](../../IMPLEMENTATION-MARKET.md) - Pricing implementation plan
- [MULTI_CHAIN_DEPLOYMENT.md](../../MULTI_CHAIN_DEPLOYMENT.md) - Multi-chain deployment guide
- [WALLET_AGNOSTIC_GUIDE.md](../../WALLET_AGNOSTIC_GUIDE.md) - Wallet compatibility patterns
- [MULTI_CHAIN_USAGE_EXAMPLES.md](../../MULTI_CHAIN_USAGE_EXAMPLES.md) - Code examples
- [SESSION_JOBS.md](../../SESSION_JOBS.md) - Session job guide
- [CONTRACT_ADDRESSES.md](../../../CONTRACT_ADDRESSES.md) - Latest addresses
- [Source Code](../../../src/JobMarketplaceWithModels.sol) - Contract implementation
- [Tests](../../../test/JobMarketplace/MultiChain/) - Multi-chain test suite