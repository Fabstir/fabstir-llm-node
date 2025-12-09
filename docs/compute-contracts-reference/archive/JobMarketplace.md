# JobMarketplace Contract Documentation

## Current Implementation: JobMarketplaceWithModels (Multi-Chain)

**Contract Address**: `0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E`
**Network**: Base Sepolia (ETH) | opBNB support planned post-MVP
**Status**: ✅ ACTIVE - S5 Off-Chain Proof Storage with Dual Pricing
**Last Updated**: October 14, 2025

> **🚀 LATEST UPDATE**: S5 Off-Chain Proof Storage (Oct 14, 2025)
>
> **Breaking Change**: `submitProofOfWork` now accepts hash + CID instead of full proof bytes
> - Transaction size: 221KB → 300 bytes (737x reduction)
> - Storage cost: ~$50 → ~$0.001 per proof
> - Full proofs stored in S5 decentralized storage
> - On-chain: SHA256 hash (32 bytes) + S5 CID (string) only

### Key Features
- **S5 Off-Chain Proof Storage**: Proofs stored in S5, only hash + CID on-chain (737x size reduction)
- **Dual Pricing System**: Separate validation for native (ETH/BNB) and stable (USDC) pricing
- **10,000x Range**: Both native and stable pricing have proper 10,000x range validation
- **Price Validation**: Validates against CORRECT pricing field based on payment type
- **Price Discovery**: Query dual pricing before creating sessions
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
- **Proof Integrity**: SHA256 hash verification prevents proof tampering
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

// SessionJob struct (18 fields)
struct SessionJob {
    address depositor;           // User who created and funded the session
    address host;                // Host serving AI inference
    uint256 deposit;             // Total USDC/ETH deposited
    uint256 pricePerToken;       // Wei/smallest unit per AI token
    uint256 maxDuration;         // Session timeout
    uint256 proofInterval;       // Tokens between proofs (e.g., 100)
    uint256 tokensConsumed;      // Total tokens consumed so far
    uint256 createdAt;           // Block timestamp
    bool active;                 // Session status
    string conversationCID;      // IPFS CID after completion
    address paymentToken;        // address(0) for ETH, or USDC address
    uint256 lastProofTimestamp;  // Timestamp of last proof submission
    uint256 totalPayment;        // Total paid to host (accumulated)
    uint256 hostEarnings;        // Host's earnings from this session
    uint256 treasuryFee;         // Treasury fee from this session
    bytes32 modelHash;           // Approved model identifier
    bytes32 lastProofHash;       // NEW: SHA256 hash of most recent proof
    string lastProofCID;         // NEW: S5 CID for proof retrieval
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

// Submit proof of work (S5 off-chain storage)
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,       // SHA256 hash of proof (32 bytes)
    string calldata proofCID // S5 CID for proof retrieval
) external

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
- Validates host registration AND dual pricing
- Checks FAB token stake (1000 FAB minimum)
- Returns 8-field struct (includes minPricePerTokenNative and minPricePerTokenStable)
- Address: `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6`

**Dual Price Validation Flow**:
```solidity
// In JobMarketplaceWithModels
(, , , , , , uint256 hostMinNative, uint256 hostMinStable) = nodeRegistry.getNodeFullInfo(host);
require(node.operator != address(0), "Host not registered");
require(node.active, "Host not active");

// For ETH sessions - validate against native pricing
require(pricePerToken >= hostMinNative, "Price below host minimum (native)");

// For USDC sessions - validate against stable pricing
require(pricePerToken >= hostMinStable, "Price below host minimum (stable)");
```

#### ProofSystem (Updated for S5 Storage)
- **Note**: With S5 off-chain storage, on-chain proof verification is no longer performed
- Contract trusts host's proof hash; disputes fetch full proof from S5 for verification
- ProofSystem configured but not actively verifying (hash verification instead)
- Address: `0x2ACcc60893872A499700908889B38C5420CBcFD1`

**S5 Proof Flow**:
1. Host generates STARK proof (221KB)
2. Host uploads proof to S5 → receives CID
3. Host calculates SHA256 hash of proof
4. Host submits `submitProofOfWork(jobId, tokens, hash, cid)`
5. Contract stores hash + CID on-chain (~300 bytes)
6. On dispute: Full proof retrieved from S5 via CID and verified

#### HostEarnings
- Accumulates host payments
- Enables batch withdrawals
- Address: `0x908962e8c6CE72610021586f85ebDE09aAc97776`

### Events

```solidity
// Session lifecycle
event SessionJobCreated(uint256 indexed jobId, address indexed user, address indexed host, uint256 deposit, uint256 pricePerToken, uint256 maxDuration)
event ProofSubmitted(uint256 indexed jobId, address indexed host, uint256 tokensClaimed, bytes32 proofHash, string proofCID)
event SessionCompleted(uint256 indexed jobId, address indexed completedBy, uint256 tokensPaid, uint256 paymentAmount, uint256 refundAmount)

// Treasury accumulation
event TreasuryFeesAccumulated(uint256 amount, address token)
event TreasuryFeesWithdrawn(uint256 amount, address token)

// Host earnings
event EarningsCredited(address indexed host, uint256 amount, address token)
```

### Security Considerations

1. **ReentrancyGuard**: All state-changing functions protected
2. **Proof Integrity**: SHA256 hash verification prevents proof tampering
3. **Proof Availability**: S5 decentralized storage ensures proof retrieval
4. **Timeout Protection**: Automatic refunds for abandoned sessions
5. **Access Control**: Treasury-only functions for fee withdrawal
6. **Emergency Withdrawal**: Respects accumulated amounts
7. **Price Validation**: Contract enforces host minimum pricing (prevents under-payment)
8. **Trust Model**: Contract trusts host's hash; disputes fetch proof from S5

### Breaking Changes (October 14, 2025)

#### `submitProofOfWork` Function Signature Changed

**OLD (Deprecated)**:
```solidity
function submitProofOfWork(
    uint256 jobId,
    bytes calldata ekzlProof,  // 221KB STARK proof
    uint256 tokensInBatch
) external returns (bool verified)
```

**NEW (Current)**:
```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,        // SHA256 hash (32 bytes)
    string calldata proofCID  // S5 CID for retrieval
) external
```

#### Migration Guide for Node Operators

**Before** (❌ No longer works):
```javascript
const proof = await generateProof(jobData);
await marketplace.submitProofOfWork(jobId, proof, 1000);
```

**After** (✅ Required):
```javascript
// 1. Generate proof
const proof = await generateProof(jobData);

// 2. Upload to S5
const proofCID = await s5.uploadBlob(proof);

// 3. Calculate hash
const proofHash = '0x' + crypto.createHash('sha256').update(proof).digest('hex');

// 4. Submit hash + CID
await marketplace.submitProofOfWork(jobId, 1000, proofHash, proofCID);
```

#### Migration Guide for SDK/Client Developers

1. **Update Contract Address**: `0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E`
2. **Update ABI**: Use latest `JobMarketplaceWithModels-CLIENT-ABI.json`
3. **Update Event Listeners**: `ProofSubmitted` event now includes `proofCID` parameter
4. **S5 Integration**: Add S5 client library for proof retrieval
5. **Hash Verification**: Implement SHA256 verification for downloaded proofs

#### Why This Change?

- **Problem**: STARK proofs (221KB) exceeded RPC transaction limit (128KB)
- **Impact**: ALL proof submissions were failing with "oversized data" error
- **Solution**: Store full proofs off-chain, submit only hash + CID
- **Benefits**:
  - Transaction size: 221KB → 300 bytes (737x reduction)
  - Storage cost: ~$50 → ~$0.001 per proof (5000x cheaper)
  - Gas cost: Minimal increase for string storage
  - Proof integrity: SHA256 hash prevents tampering
  - Proof availability: S5 decentralized storage

#### Backward Compatibility

⚠️ **NO backward compatibility** - the old contract (`0xe169A4B57700080725f9553E3Cc69885fea13629`) remains functional for existing sessions, but new sessions should use the updated contract.

**Rollback Plan**: If critical issues discovered, old contract can still accept new sessions as a fallback.

### Dual Pricing System (NEW - January 28, 2025)

All session creation functions now validate against the CORRECT pricing field based on payment type:

**Functions with Dual Price Validation**:
- `createSessionJob()` - Native token (ETH) sessions → validates against `hostMinPriceNative`
- `createSessionJobWithToken()` - ERC20 token (USDC) sessions → validates against `hostMinPriceStable`
- `createSessionFromDeposit()` - Pre-funded sessions → validates based on token type

**Validation Logic**:
```solidity
(, , , , , , uint256 hostMinNative, uint256 hostMinStable) = nodeRegistry.getNodeFullInfo(host);

// For native token (ETH/BNB) sessions
require(pricePerToken >= hostMinNative, "Price below host minimum (native)");

// For stablecoin (USDC) sessions
require(pricePerToken >= hostMinStable, "Price below host minimum (stable)");
```

**Error Handling**:
- Transaction reverts with "Price below host minimum (native)" for ETH sessions
- Transaction reverts with "Price below host minimum (stable)" for USDC sessions
- Client must query dual pricing first using `nodeRegistry.getNodePricing(host)`

**Usage Example (ETH Session)**:
```javascript
import JobMarketplaceABI from './JobMarketplaceWithModels-CLIENT-ABI.json';
import NodeRegistryABI from './NodeRegistryWithModels-CLIENT-ABI.json';
import { ethers } from 'ethers';

const nodeRegistry = new ethers.Contract(
  '0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6',
  NodeRegistryABI,
  provider
);

const marketplace = new ethers.Contract(
  '0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E',
  JobMarketplaceABI,
  signer
);

// STEP 1: Query host DUAL pricing BEFORE creating session
const hostAddress = '0x...';
const [hostMinNative, hostMinStable] = await nodeRegistry.getNodePricing(hostAddress);
console.log(`Host native minimum: ${hostMinNative.toString()} wei`);
console.log(`Host stable minimum: ${hostMinStable}`);

// STEP 2: Create ETH session with price >= host native minimum
const myPriceNative = ethers.BigNumber.from("4000000000"); // Must be >= hostMinNative
const deposit = ethers.utils.parseEther('0.1'); // 0.1 ETH

// This will REVERT if myPriceNative < hostMinNative
const tx = await marketplace.createSessionJob(
  hostAddress,
  myPriceNative,
  3600, // 1 hour max duration
  100,  // Proof every 100 tokens
  { value: deposit }
);

await tx.wait();
console.log('ETH session created with validated native pricing!');
```

**Usage Example (USDC Session)**:
```javascript
// STEP 1: Query host DUAL pricing
const [hostMinNative, hostMinStable] = await nodeRegistry.getNodePricing(hostAddress);

// STEP 2: Create USDC session with price >= host stable minimum
const myPriceStable = 20000; // Must be >= hostMinStable
const usdcDeposit = ethers.utils.parseUnits("10", 6); // 10 USDC

// Approve USDC first
const usdcContract = new ethers.Contract(
  '0x036CbD53842c5426634e7929541eC2318f3dCF7e',
  ['function approve(address,uint256)'],
  signer
);
await usdcContract.approve(marketplace.address, usdcDeposit);

// This will REVERT if myPriceStable < hostMinStable
const tx = await marketplace.createSessionJobWithToken(
  hostAddress,
  '0x036CbD53842c5426634e7929541eC2318f3dCF7e', // USDC
  usdcDeposit,
  myPriceStable,
  3600,
  100
);

await tx.wait();
console.log('USDC session created with validated stable pricing!');
```

**Usage Example (Host Proof Submission with S5)**:
```javascript
import crypto from 'crypto';
import { S5Client } from '@lumeweb/s5-js';

// Initialize S5 client
const s5 = new S5Client('https://s5.lumeweb.com');

// 1. Generate STARK proof (existing node logic)
const proof = await generateRisc0Proof(jobData);
console.log(`Proof size: ${proof.length} bytes`); // ~221KB

// 2. Upload proof to S5
const proofCID = await s5.uploadBlob(proof);
console.log(`Proof uploaded to S5: ${proofCID}`);

// 3. Calculate SHA256 hash of proof
const proofHash = '0x' + crypto.createHash('sha256').update(proof).digest('hex');
console.log(`Proof hash: ${proofHash}`);

// 4. Submit hash + CID to blockchain
const marketplace = new ethers.Contract(
  '0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E',
  JobMarketplaceABI,
  signer
);

const tx = await marketplace.submitProofOfWork(
  jobId,
  tokensClaimed,  // e.g., 1000 tokens
  proofHash,      // 32-byte SHA256 hash
  proofCID        // S5 CID string
);

await tx.wait();
console.log('Proof submitted! Transaction size: ~300 bytes (vs 221KB)');

// 5. Listen for event
marketplace.on('ProofSubmitted', (jobId, host, tokens, hash, cid) => {
  console.log(`Proof event: jobId=${jobId}, tokens=${tokens}, cid=${cid}`);
});
```

**Retrieving Proofs from S5** (for disputes or verification):
```javascript
// Download proof from S5 using CID
const storedProof = await s5.downloadBlob(proofCID);

// Verify integrity
const downloadedHash = '0x' + crypto.createHash('sha256').update(storedProof).digest('hex');
if (downloadedHash !== proofHash) {
  throw new Error('Proof integrity check failed!');
}

console.log('Proof retrieved and verified from S5');
```

**Pricing Ranges**:
- **Native (ETH/BNB)**: 2,272,727,273 to 22,727,272,727,273 wei (~$0.00001 to $0.1 @ $4400 ETH)
- **Stable (USDC)**: 10 to 100,000 (0.00001 to 0.1 USDC per token)
- **Both have 10,000x range** (MIN to MAX)

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
   - **Query host DUAL pricing BEFORE creating sessions** (use `nodeRegistry.getNodePricing()` which returns tuple)
   - Extract both native and stable prices from the tuple
   - Ensure your pricePerToken >= appropriate host minimum (native for ETH, stable for USDC)
   - Pre-fund deposits for gasless operations
   - Use `createSessionFromDeposit()` for better gas efficiency
   - Let hosts complete sessions to avoid gas costs
   - Works with both EOA and Smart Wallets

2. **For Hosts**:
   - Set competitive DUAL pricing via `nodeRegistry.updatePricingNative()` and `updatePricingStable()`
   - Monitor market rates AND ETH price to adjust pricing accordingly
   - Keep both native and stable pricing updated based on market dynamics
   - Consider gas costs when setting native pricing
   - Complete sessions to claim payment faster
   - Submit proofs regularly at checkpoint intervals
   - Withdraw accumulated earnings periodically
   - Maintain sufficient FAB stake

3. **For Integrators**:
   - **Always query DUAL pricing before session creation**
   - Handle tuple return from `getNodePricing()` - extracts (native, stable)
   - Validate against CORRECT pricing field: native for ETH, stable for USDC
   - Handle "Price below host minimum (native)" and "(stable)" errors separately
   - Support both inline payment and pre-funded patterns
   - Track `depositor` field, not just `msg.sender`
   - Enable anyone-can-complete for better UX
   - Test with different wallet types
   - Handle 8-field struct from `nodeRegistry.getNodeFullInfo()` (not 7!)

### References

- [S5_PROOF_STORAGE_DEPLOYMENT.md](../../S5_PROOF_STORAGE_DEPLOYMENT.md) - **NEW** S5 proof storage deployment guide
- [NodeRegistry.md](./NodeRegistry.md) - Host registration and pricing documentation
- [IMPLEMENTATION-MARKET.md](../../IMPLEMENTATION-MARKET.md) - Pricing implementation plan
- [MULTI_CHAIN_DEPLOYMENT.md](../../MULTI_CHAIN_DEPLOYMENT.md) - Multi-chain deployment guide
- [WALLET_AGNOSTIC_GUIDE.md](../../WALLET_AGNOSTIC_GUIDE.md) - Wallet compatibility patterns
- [MULTI_CHAIN_USAGE_EXAMPLES.md](../../MULTI_CHAIN_USAGE_EXAMPLES.md) - Code examples
- [SESSION_JOBS.md](../../SESSION_JOBS.md) - Session job guide
- [CONTRACT_ADDRESSES.md](../../../CONTRACT_ADDRESSES.md) - Latest addresses
- [Source Code](../../../src/JobMarketplaceWithModels.sol) - Contract implementation
- [Tests](../../../test/JobMarketplace/MultiChain/) - Multi-chain test suite
- [S5 Documentation](https://docs.sfive.net/) - S5 decentralized storage documentation