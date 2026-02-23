# Architecture Documentation

**Version:** 2.4
**Last Updated:** February 4, 2026
**Network:** Base Sepolia (Testnet)

---

## 1. Contract Addresses (UUPS Proxies)

> **⚠️ TWO DEPLOYMENT SETS:** Frozen (Audit) and Remediation (Active Development)

### 1.1 Remediation Contracts (Active Development)

Use for SDK development. Includes Signature Removal, Early Cancellation Fee + Per-Model Rate Limits.

| Contract | Proxy Address | Implementation |
|----------|---------------|----------------|
| JobMarketplace | `0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4` | `0x1a0436a15d2fD911b2F062D08aA312141A978955` |
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` | `0xF2D98D38B2dF95f4e8e4A49750823C415E795377` |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` | `0x3F22fd532Ac051aE09b0F2e45F3DBfc835AfCD45` |
| ProofSystem | `0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31` | `0x5345a926dcf3B0E1A6895406FB68210ED19AC556` |
| HostEarnings | `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` | `0x8584AeAC9687613095D13EF7be4dE0A796F84D7a` |

### 1.2 Frozen Contracts (Security Audit - DO NOT MODIFY)

| Contract | Proxy Address | Implementation |
|----------|---------------|----------------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` 🔒 | `0x1B6C6A1E373E5E00Bf6210e32A6DA40304f6484c` |
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` 🔒 | `0xF2D98D38B2dF95f4e8e4A49750823C415E795377` |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` 🔒 | `0x8491af1f0D47f6367b56691dCA0F4996431fB0A5` |
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` 🔒 | `0xCF46BBa79eA69A68001A1c2f5Ad9eFA1AD435EF9` |
| HostEarnings | `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` 🔒 | `0x8584AeAC9687613095D13EF7be4dE0A796F84D7a` |

**Tokens:**
- FAB Token: `0xC78949004B4EB6dEf2D66e49Cd81231472612D62`
- USDC: `0x036CbD53842c5426634e7929541eC2318f3dCF7e`

---

## 2. Contract Dependency Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         FABSTIR COMPUTE ARCHITECTURE                         │
└─────────────────────────────────────────────────────────────────────────────┘

                          ┌───────────────────────┐
                          │    ModelRegistry      │
                          │  ─────────────────    │
                          │  • Model whitelist    │
                          │  • Community voting   │
                          │  • Trusted models     │
                          │  • Per-model rate     │
                          │    limits (NEW)       │
                          └───────────┬───────────┘
                                      │ validates models
                                      ▼
                          ┌───────────────────────┐
                          │    NodeRegistry       │
                          │  ─────────────────    │
                          │  • Host registration  │
                          │  • FAB staking        │
                          │  • Dual pricing       │
                          │  • Model support      │
                          │  • Stake slashing     │
                          └───────────┬───────────┘
                                      │ validates hosts
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        JobMarketplaceWithModels                              │
│  ─────────────────────────────────────────────────────────────────────────  │
│  • Session management          • Deposit handling                           │
│  • Proof submission            • Payment settlement                         │
│  • Timeout enforcement         • Treasury collection                        │
└────────────────┬────────────────────────────────────┬───────────────────────┘
                 │                                    │
                 │ marks proofs used                  │ credits earnings
                 ▼                                    ▼
    ┌───────────────────────┐            ┌───────────────────────┐
    │     ProofSystem       │            │    HostEarnings       │
    │  ─────────────────    │            │  ─────────────────    │
    │  • Replay prevention  │            │  • Earnings ledger    │
    │  • markProofUsed()    │            │  • Batch withdrawals  │
    │  • Proof hash storage │            │  • Multi-token        │
    └───────────────────────┘            └───────────────────────┘
```

### Dependency Matrix

| Contract | Depends On | Depended By |
|----------|------------|-------------|
| ModelRegistry | OpenZeppelin | NodeRegistry |
| NodeRegistry | ModelRegistry, FAB Token | JobMarketplace |
| JobMarketplace | NodeRegistry, ProofSystem, HostEarnings | - |
| ProofSystem | OpenZeppelin | JobMarketplace |
| HostEarnings | OpenZeppelin | JobMarketplace |

---

## 3. Session Lifecycle State Machine

```
                              ┌─────────────────────────────────────┐
                              │         SESSION LIFECYCLE           │
                              └─────────────────────────────────────┘

    ┌──────────────────┐
    │  (Not Exists)    │
    └────────┬─────────┘
             │
             │ createSessionJobForModel()
             │ createSessionJobForModelWithToken()
             │
             ▼
    ┌──────────────────┐
    │                  │◄──────────────────────────────────────────┐
    │     ACTIVE       │                                           │
    │                  │──── submitProofOfWork() ───────────────────┘
    │  status = 0      │     (updates tokensUsed, stores deltaCID)
    │                  │
    └────────┬─────────┘
             │
             ├─────────────────────────┬────────────────────────────┐
             │                         │                            │
             │ completeSessionJob()    │ triggerSessionTimeout()    │
             │ (host or depositor)     │ (anyone, after 3× interval)│
             │ + conversationCID       │                            │
             ▼                         ▼                            │
    ┌──────────────────┐    ┌──────────────────┐                   │
    │    COMPLETED     │    │    TIMED_OUT     │                   │
    │                  │    │                  │                   │
    │  status = 1      │    │  status = 2      │                   │
    │                  │    │                  │                   │
    │  Payment:        │    │  Payment:        │                   │
    │  • Host: 90%     │    │  • Host: 90%     │                   │
    │  • Treasury: 10% │    │    (of proven)   │                   │
    │  • Refund: rest  │    │  • Treasury: 10% │                   │
    └──────────────────┘    │  • Refund: rest  │                   │
                            └──────────────────┘                   │
                                                                   │
    ┌─────────────────────────────────────────────────────────────┐│
    │                    STATE TRANSITIONS                         ││
    ├─────────────────────────────────────────────────────────────┤│
    │  ACTIVE → ACTIVE      : submitProofOfWork() [tokensUsed++]  ││
    │  ACTIVE → COMPLETED   : completeSessionJob(conversationCID)  │
    │  ACTIVE → TIMED_OUT   : triggerSessionTimeout()             ││
    │                                                              ││
    │  COMPLETED → *        : BLOCKED (immutable)                 ││
    │  TIMED_OUT → *        : BLOCKED (immutable)                 ││
    └─────────────────────────────────────────────────────────────┘│
```

---

## 4. Data Flow Diagrams

### 4.1 Session Creation Flow

```
┌─────────┐                  ┌─────────────────┐                  ┌──────────────┐
│Depositor│                  │  JobMarketplace │                  │ NodeRegistry │
└────┬────┘                  └────────┬────────┘                  └──────┬───────┘
     │                                │                                  │
     │  1. getNodePricing(host)       │                                  │
     │ ──────────────────────────────────────────────────────────────────>
     │                                │                                  │
     │  2. (minNative, minStable)     │                                  │
     │ <──────────────────────────────────────────────────────────────────
     │                                │                                  │
     │  3. createSessionJobForModel() │                                  │
     │    + ETH deposit               │                                  │
     │ ──────────────────────────────>│                                  │
     │                                │                                  │
     │                                │  4. isActiveNode(host)?          │
     │                                │ ────────────────────────────────>│
     │                                │                                  │
     │                                │  5. true                         │
     │                                │ <────────────────────────────────│
     │                                │                                  │
     │                                │  6. nodeSupportsModel()?         │
     │                                │ ────────────────────────────────>│
     │                                │                                  │
     │                                │  7. true                         │
     │                                │ <────────────────────────────────│
     │                                │                                  │
     │  8. SessionJobCreated event    │                                  │
     │ <──────────────────────────────│                                  │
     │                                │                                  │
```

### 4.1b V2 Direct Payment Delegation Flow (NEW - Feb 2026)

For Coinbase Smart Wallet sub-accounts creating sessions using primary account's USDC:

```
┌─────────┐   ┌───────────┐                  ┌─────────────────┐
│ Primary │   │Sub-Account│                  │  JobMarketplace │
│ Wallet  │   │(Delegate) │                  │                 │
└────┬────┘   └─────┬─────┘                  └────────┬────────┘
     │              │                                 │
     │  1. approve(marketplace, $1000)               │
     │ ─────────────────────────────────────────────>│  (USDC contract)
     │              │                                 │
     │  2. authorizeDelegate(subAccount, true)       │
     │ ─────────────────────────────────────────────>│
     │              │                                 │
     │              │  3. createSessionForModelAsDelegate()
     │              │     (payer=primary, USDC)       │
     │              │ ───────────────────────────────>│
     │              │                                 │
     │              │     4. Check authorization      │
     │              │     isAuthorizedDelegate[payer][msg.sender]?
     │              │                                 │
     │              │     5. transferFrom(payer, contract, amount)
     │              │        (pulls USDC from primary's wallet)
     │              │                                 │
     │              │  6. Session created             │
     │              │     (depositor = primary)       │
     │              │ <───────────────────────────────│
     │              │                                 │
```

**Key Points:**
- Steps 1-2 are one-time setup (2 popups)
- Step 3 is per-session (NO popup - sub-account signs)
- Refunds go to primary (depositor), not delegate
- USDC only (ETH not supported for delegation)

### 4.2 Proof Submission Flow

```
┌──────┐                  ┌─────────────────┐                  ┌─────────────┐
│ Host │                  │  JobMarketplace │                  │ ProofSystem │
└──┬───┘                  └────────┬────────┘                  └──────┬──────┘
   │                               │                                  │
   │  1. Generate inference        │                                  │
   │     (off-chain)               │                                  │
   │                               │                                  │
   │  2. Upload proof to S5        │                                  │
   │     → get proofCID, deltaCID  │                                  │
   │                               │                                  │
   │  3. submitProofOfWork(        │                                  │
   │       jobId, tokens,          │                                  │
   │       proofHash,              │                                  │
   │       proofCID, deltaCID)     │                                  │
   │ ─────────────────────────────>│                                  │
   │                               │                                  │
   │                               │  4. Verify msg.sender == host    │
   │                               │     (no signature needed)        │
   │                               │                                  │
   │                               │  5. markProofUsed(proofHash)     │
   │                               │ ────────────────────────────────>│
   │                               │                                  │
   │                               │  6. Proof marked (replay protect)│
   │                               │ <────────────────────────────────│
   │                               │                                  │
   │                               │  7. Update tokensUsed            │
   │                               │     Store proofHash, deltaCID    │
   │                               │                                  │
   │  8. ProofSubmitted event      │                                  │
   │     (includes deltaCID)       │                                  │
   │ <─────────────────────────────│                                  │
   │                               │                                  │
```

> **Note (Feb 4, 2026):** Signature verification removed. Host authentication is via `msg.sender == session.host` check.

### 4.3 Payment Settlement Flow

```
┌────────────┐        ┌─────────────────┐        ┌──────────────┐        ┌──────────┐
│Host/Depos. │        │  JobMarketplace │        │ HostEarnings │        │ Treasury │
└─────┬──────┘        └────────┬────────┘        └──────┬───────┘        └────┬─────┘
      │                        │                        │                     │
      │ 1. completeSessionJob()│                        │                     │
      │ ──────────────────────>│                        │                     │
      │                        │                        │                     │
      │                        │ 2. Calculate:          │                     │
      │                        │    hostPayment = 90%   │                     │
      │                        │    treasuryFee = 10%   │                     │
      │                        │    earlyFee (if applicable)                  │
      │                        │    refund = remainder  │                     │
      │                        │                        │                     │
      │                        │ 3. creditEarnings()    │                     │
      │                        │ ──────────────────────>│                     │
      │                        │                        │                     │
      │                        │ 4. Transfer fee        │                     │
      │                        │ ───────────────────────────────────────────>│
      │                        │                        │                     │
      │                        │ 5. Transfer refund     │                     │
      │ <──────────────────────│                        │                     │
      │                        │                        │                     │
      │ 6. SessionCompleted    │                        │                     │
      │ <──────────────────────│                        │                     │
      │                        │                        │                     │

      [Later: Host withdraws from HostEarnings]

┌──────┐        ┌──────────────┐
│ Host │        │ HostEarnings │
└──┬───┘        └──────┬───────┘
   │                   │
   │ withdraw()        │
   │ ─────────────────>│
   │                   │
   │ ETH/USDC transfer │
   │ <─────────────────│
   │                   │
```

### 4.3b Early Cancellation Fee Flow (NEW - Feb 3, 2026)

When depositor cancels **before any proofs** are submitted:

```
┌──────────┐        ┌─────────────────┐        ┌──────────────┐
│ Depositor│        │  JobMarketplace │        │ HostEarnings │
└────┬─────┘        └────────┬────────┘        └──────┬───────┘
     │                       │                        │
     │ 1. completeSessionJob()                        │
     │    (proofs.length == 0)                        │
     │ ─────────────────────>│                        │
     │                       │                        │
     │                       │ 2. Check conditions:   │
     │                       │    - caller == depositor
     │                       │    - proofs.length == 0│
     │                       │    - minTokensFee > 0  │
     │                       │                        │
     │                       │ 3. Calculate earlyFee: │
     │                       │    = minTokensFee *    │
     │                       │      pricePerToken /   │
     │                       │      PRICE_PRECISION   │
     │                       │                        │
     │                       │ 4. creditEarnings()    │
     │                       │    (earlyFee to host)  │
     │                       │ ──────────────────────>│
     │                       │                        │
     │                       │ 5. NO treasury fee     │
     │                       │    (only on proven work)
     │                       │                        │
     │ 6. Refund = deposit - earlyFee                 │
     │ <─────────────────────│                        │
     │                       │                        │
```

**Key Points:**
- Early cancellation fee goes 100% to host (no treasury cut)
- Fee only charged when depositor cancels with 0 proofs
- Protects hosts from free inference exploitation

### 4.4 Model Governance Flow

```
┌──────────┐        ┌───────────────┐        ┌───────────┐
│ Proposer │        │ ModelRegistry │        │  Voters   │
└────┬─────┘        └───────┬───────┘        └─────┬─────┘
     │                      │                      │
     │ 1. proposeModel()    │                      │
     │    + 100 FAB fee     │                      │
     │ ────────────────────>│                      │
     │                      │                      │
     │ 2. ModelProposed     │                      │
     │ <────────────────────│                      │
     │                      │                      │
     │                      │ 3. voteOnProposal()  │
     │                      │    + FAB tokens      │
     │                      │ <────────────────────│
     │                      │                      │
     │                      │  [3 days pass...]    │
     │                      │                      │
     │                      │ 4. executeProposal() │
     │                      │ <────────────────────│
     │                      │                      │
     │                      │ 5. If approved:      │
     │                      │    - Add model       │
     │                      │    - Refund fee      │
     │                      │                      │
     │                      │ 6. withdrawVotes()   │
     │                      │ <────────────────────│
     │                      │                      │
```

---

## 5. Storage Layout Documentation

### 5.1 JobMarketplaceWithModelsUpgradeable

```solidity
// Slot 0-4: Inherited from OwnableUpgradeable, PausableUpgradeable, etc.

// Slot 5+: Contract-specific storage
IERC20 public fabToken;                           // Slot 5
INodeRegistry public nodeRegistry;                // Slot 6
IHostEarnings public hostEarnings;                // Slot 7
IProofSystem public proofSystem;                  // Slot 8

address public treasury;                          // Slot 9
uint256 public nextJobId;                         // Slot 10

mapping(uint256 => SessionJob) public sessionJobs;      // Slot 11
mapping(address => uint256[]) public userSessions;      // Slot 12
mapping(uint256 => bytes32) public sessionModel;        // Slot 13
mapping(address => uint256) public nativeDeposits;      // Slot 14
mapping(address => mapping(address => uint256)) public tokenDeposits;  // Slot 15
mapping(address => bool) public acceptedTokens;         // Slot 16
mapping(address => uint256) public tokenMinDeposits;    // Slot 17

uint256 public accumulatedTreasuryNative;         // Slot 18
mapping(address => uint256) public accumulatedTreasuryTokens;  // Slot 19

// V2 Delegation (Feb 2, 2026)
mapping(address => mapping(address => bool)) public isAuthorizedDelegate;  // Slot 20

// Early Cancellation Fee (Feb 3, 2026)
uint256 public minTokensFee;                      // Slot 21 - Min tokens charged on early cancel

// Slot 22-54: Storage gap (33 slots reserved)
uint256[33] private __gap;
```

### 5.2 SessionJob Struct Layout

```solidity
struct SessionJob {
    address host;              // 20 bytes
    address depositor;         // 20 bytes
    address paymentToken;      // 20 bytes
    uint256 depositAmount;     // 32 bytes
    uint256 pricePerToken;     // 32 bytes
    uint256 tokensUsed;        // 32 bytes (renamed from tokensProven)
    uint256 startTime;         // 32 bytes
    uint256 maxDuration;       // 32 bytes
    uint256 proofInterval;     // 32 bytes
    uint256 lastProofTime;     // 32 bytes
    bytes32 lastProofHash;     // 32 bytes
    string lastProofCID;       // Dynamic (S5 CID)
    string conversationCID;    // Dynamic (S5 CID) - set on completion
    SessionStatus status;      // 1 byte (enum: Active=0, Completed=1, TimedOut=2)
}
// Total: ~12 storage slots per session (plus dynamic strings)
```

### 5.3 NodeRegistryWithModelsUpgradeable

```solidity
// Slot 0-2: Inherited storage

IERC20 public fabToken;                           // Slot 3
ModelRegistryUpgradeable public modelRegistry;    // Slot 4

mapping(address => Node) public nodes;            // Slot 5
mapping(address => uint256) public activeNodesIndex;  // Slot 6
mapping(bytes32 => address[]) public modelToNodes;    // Slot 7
mapping(bytes32 => mapping(address => uint256)) private modelNodeIndex;  // Slot 8

mapping(address => mapping(bytes32 => uint256)) public modelPricingNative;   // Slot 9
mapping(address => mapping(bytes32 => uint256)) public modelPricingStable;   // Slot 10
mapping(address => mapping(address => uint256)) public customTokenPricing;   // Slot 11

address[] public activeNodesList;                 // Slot 12

// Slot 13-15: Slashing state (NEW - Jan 16, 2026)
address public slashingAuthority;                 // Slot 13
address public treasury;                          // Slot 14
mapping(address => uint256) public lastSlashTime; // Slot 15

// Slot 16-51: Storage gap (36 slots)
uint256[36] private __gap;
```

### 5.4 ModelRegistryUpgradeable

```solidity
// Slot 0-2: Inherited storage (Ownable, etc.)

IERC20 public fabToken;                           // Slot 3
mapping(bytes32 => Model) public models;          // Slot 4
mapping(bytes32 => ModelProposal) public proposals;  // Slot 5
bytes32[] public approvedModels;                  // Slot 6
mapping(bytes32 => bool) public trustedModels;    // Slot 7

// Voting state
mapping(bytes32 => mapping(address => uint256)) public voterDeposits;  // Slot 8
mapping(bytes32 => uint256) public lateVotes;     // Slot 9
mapping(bytes32 => uint256) public lastProposalExecutionTime;  // Slot 10

// Per-Model Rate Limits (Feb 3, 2026)
mapping(bytes32 => uint256) public modelRateLimits;  // Slot 11 - tokens/second (0 = unlimited)

// Slot 12-60: Storage gap (49 slots)
uint256[49] private __gap;
```

### 5.6 Storage Gap Strategy

All upgradeable contracts reserve storage gaps for future additions:

| Contract | Gap Size | Reserved Slots |
|----------|----------|----------------|
| JobMarketplaceWithModelsUpgradeable | 33 | Reduced for delegation + early cancel fee |
| NodeRegistryWithModelsUpgradeable | 36 | Reputation (reduced from 39 for slashing) |
| ModelRegistryUpgradeable | 49 | Governance extensions + rate limits |
| ProofSystemUpgradeable | 49 | ZK proof support |
| HostEarningsUpgradeable | 48 | Multi-chain earnings |

---

## 6. External Dependencies

### 6.1 OpenZeppelin Contracts (v5.x)

| Contract | Usage | Import Path |
|----------|-------|-------------|
| OwnableUpgradeable | Access control | `@openzeppelin/contracts-upgradeable/access/` |
| PausableUpgradeable | Emergency stop | `@openzeppelin/contracts-upgradeable/utils/` |
| Initializable | Proxy initialization | `@openzeppelin/contracts-upgradeable/proxy/utils/` |
| UUPSUpgradeable | Upgrade pattern | `@openzeppelin/contracts-upgradeable/proxy/utils/` |
| SafeERC20 | Safe token transfers | `@openzeppelin/contracts/token/ERC20/utils/` |
| Address | Safe ETH transfers | `@openzeppelin/contracts/utils/` |
| ECDSA | Signature verification | `@openzeppelin/contracts/utils/cryptography/` |
| MessageHashUtils | EIP-191 hashing | `@openzeppelin/contracts/utils/cryptography/` |

### 6.2 Token Interfaces

| Interface | Standard | Usage |
|-----------|----------|-------|
| IERC20 | ERC-20 | USDC, FAB token interactions |

### 6.3 Upgrade Pattern: UUPS

```
┌─────────────────────────────────────────────────────────────┐
│                    UUPS Proxy Pattern                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   ┌─────────────┐         ┌─────────────────────┐          │
│   │   Proxy     │────────>│   Implementation    │          │
│   │  (Storage)  │         │   (Logic Only)      │          │
│   │             │         │                     │          │
│   │ • State     │         │ • Functions         │          │
│   │ • Balance   │         │ • _authorizeUpgrade │          │
│   └─────────────┘         └─────────────────────┘          │
│         │                           │                       │
│         │ delegatecall              │                       │
│         └───────────────────────────┘                       │
│                                                             │
│   Upgrade: owner calls proxy.upgradeToAndCall(newImpl)     │
│   Authorization: _authorizeUpgrade() checks onlyOwner      │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## 7. Security Architecture

### 7.1 Reentrancy Protection

```solidity
// OpenZeppelin ReentrancyGuardTransient (EIP-1153 transient storage)
// Gas-efficient: ~4,900 gas savings per nonReentrant call
import {ReentrancyGuardTransient} from "@openzeppelin/contracts/utils/ReentrancyGuardTransient.sol";

contract JobMarketplaceUpgradeable is ReentrancyGuardTransient {
    // Uses transient storage (TSTORE/TLOAD) instead of contract storage
    // Status is automatically cleared at end of transaction
    // No storage slot consumed - works seamlessly with UUPS proxies
}
```

**Benefits of EIP-1153 Transient Storage:**
- ~4,900 gas savings per `nonReentrant` call
- No storage slot collision concerns with proxies
- Automatic cleanup at transaction end

**Protected Functions:**
- `registerNode()`, `unregisterNode()`, `stake()` (NodeRegistry)
- `withdraw()`, `withdrawToken()` (HostEarnings)
- Session creation and completion functions (JobMarketplace)

### 7.2 Safe Transfer Patterns

```solidity
// ERC20: SafeERC20 library
token.safeTransfer(recipient, amount);
token.safeTransferFrom(sender, recipient, amount);

// ETH: Address library
Address.sendValue(payable(recipient), amount);
```

### 7.3 Access Control Hierarchy

```
┌─────────────────────────────────────────────┐
│              Access Control                  │
├─────────────────────────────────────────────┤
│                                             │
│  OWNER (Highest)                            │
│  └── upgradeToAndCall()                     │
│  └── pause(), unpause()                     │
│  └── updateTreasury()                       │
│  └── addTrustedModel()                      │
│  └── setAuthorizedCaller()                  │
│  └── setSlashingAuthority()                 │
│  └── initializeSlashing()                   │
│                                             │
│  SLASHING_AUTHORITY (Medium-High)           │
│  └── slashStake() [any active host]         │
│                                             │
│  AUTHORIZED_CALLER (Medium)                 │
│  └── creditEarnings()                       │
│  └── recordVerifiedProof()                  │
│                                             │
│  HOST (Medium - Economically Bonded)        │
│  └── submitProofOfWork() [own sessions]     │
│  └── completeSessionJob() [own sessions]    │
│  └── update*() [own node]                   │
│                                             │
│  DEPOSITOR (Low)                            │
│  └── completeSessionJob() [own sessions]    │
│  └── session creation                       │
│                                             │
│  ANYONE (Lowest)                            │
│  └── triggerSessionTimeout()                │
│  └── View functions                         │
│  └── proposeModel(), voteOnProposal()       │
│                                             │
└─────────────────────────────────────────────┘
```

---

## 8. Gas Optimization Patterns

### 8.1 O(1) Array Removal

```solidity
// Swap-and-pop pattern for efficient removal
function _removeNodeFromModel(bytes32 modelId, address node) private {
    uint256 index = modelNodeIndex[modelId][node];
    uint256 lastIndex = modelToNodes[modelId].length - 1;

    if (index != lastIndex) {
        address lastNode = modelToNodes[modelId][lastIndex];
        modelToNodes[modelId][index] = lastNode;
        modelNodeIndex[modelId][lastNode] = index;
    }

    modelToNodes[modelId].pop();
    delete modelNodeIndex[modelId][node];
}
```

### 8.2 Batch Operations

- `batchAddTrustedModels()` - Add multiple models in one transaction
- HostEarnings accumulation - Batch withdrawals vs per-session payments

### 8.3 Storage Efficiency

- Struct packing for session data
- Enum for status (1 byte vs 32 bytes)
- Mapping-based lookups vs array iterations

---

## 9. Event Architecture

### 9.1 Key Events for Indexing

| Contract | Event | Purpose |
|----------|-------|---------|
| JobMarketplace | `SessionJobCreated` | Track session starts |
| JobMarketplace | `SessionCompleted` | Track completions, payments |
| JobMarketplace | `ProofSubmitted` | Track proof history (includes deltaCID) |
| NodeRegistry | `NodeRegistered` | Track host onboarding |
| NodeRegistry | `PricingUpdated` | Track price changes |
| ModelRegistry | `ModelProposed` | Track governance |
| HostEarnings | `EarningsCredited` | Track host income |

### 9.2 Event Indexing Strategy

```
┌─────────────────────────────────────────────────────────────┐
│                  Off-Chain Indexing                          │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Events ────────> TheGraph/Custom Indexer ────────> API     │
│                                                             │
│  Indexed Fields:                                            │
│  • jobId (SessionJobCreated, ProofSubmitted)               │
│  • host (NodeRegistered, EarningsCredited)                 │
│  • depositor (SessionJobCreated)                           │
│  • modelId (ModelProposed, SessionJobCreated)              │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```
