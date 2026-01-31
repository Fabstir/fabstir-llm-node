# Architecture Documentation

**Version:** 2.2
**Last Updated:** January 31, 2026
**Network:** Base Sepolia (Testnet)

---

## 1. Contract Addresses

### Frozen Contracts (Auditors - DO NOT UPGRADE)

| Contract | Proxy Address | Implementation |
|----------|---------------|----------------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` | `0x1B6C6A1E373E5E00Bf6210e32A6DA40304f6484c` |
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` | `0xF2D98D38B2dF95f4e8e4A49750823C415E795377` |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` | `0x8491af1f0D47f6367b56691dCA0F4996431fB0A5` |
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` | `0xCF46BBa79eA69A68001A1c2f5Ad9eFA1AD435EF9` |
| HostEarnings | `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` | `0x8584AeAC9687613095D13EF7be4dE0A796F84D7a` |

### Test Contracts (Remediation - January 31, 2026)

> **AUDIT Remediation**: These contracts include fixes for AUDIT-F1 through AUDIT-F5. Deployed as new proxies to avoid upgrading frozen audit contracts.

| Contract | Test Proxy | Test Implementation |
|----------|------------|---------------------|
| JobMarketplace | `0x95132177F964FF053C1E874b53CF74d819618E06` | `0x06dB705BcBdda50A1712635fdC64A28d75de5603` |
| ProofSystem | `0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31` | `0x56657bCBAE50AB656A9452f7B52e317650f90267` |

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
│  • proofTimeoutWindow (NEW)    • createSessionFromDepositForModel (NEW)    │
└────────────────┬────────────────────────────────────┬───────────────────────┘
                 │                                    │
                 │ verifies signatures                │ credits earnings
                 │ (now includes modelId)            │
                 ▼                                    ▼
    ┌───────────────────────┐            ┌───────────────────────┐
    │     ProofSystem       │            │    HostEarnings       │
    │  ─────────────────    │            │  ─────────────────    │
    │  • ECDSA verification │            │  • Earnings ledger    │
    │  • Proof recording    │            │  • Batch withdrawals  │
    │  • Replay prevention  │            │  • Multi-token        │
    │  • modelId validation │            │                       │
    │    (NEW - AUDIT-F4)   │            │                       │
    └───────────────────────┘            └───────────────────────┘
```

### Dependency Matrix

| Contract | Depends On | Depended By |
|----------|------------|-------------|
| ModelRegistry | OpenZeppelin | NodeRegistry |
| NodeRegistry | ModelRegistry, FAB Token | JobMarketplace |
| JobMarketplace | NodeRegistry, ProofSystem, HostEarnings, ModelRegistry | - |
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
             │ createSessionJobForModel(... proofTimeoutWindow)  // NEW param
             │ createSessionJobForModelWithToken(... proofTimeoutWindow)
             │ createSessionFromDepositForModel(...)  // NEW function
             │
             ▼
    ┌──────────────────┐
    │                  │◄──────────────────────────────────────────┐
    │     ACTIVE       │                                           │
    │                  │──── submitProofOfWork() ───────────────────┘
    │  status = 0      │     (updates tokensUsed, stores deltaCID)
    │                  │     (signature now includes modelId)
    └────────┬─────────┘
             │
             ├─────────────────────────┬────────────────────────────┐
             │                         │                            │
             │ completeSessionJob()    │ triggerSessionTimeout()    │
             │ (host or depositor)     │ (anyone, after timeout)    │
             │ + conversationCID       │ Uses proofTimeoutWindow    │
             │                         │ (NOT proofInterval * 3)    │
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
    │                         (uses proofTimeoutWindow, not       ││
    │                          proofInterval * 3 - AUDIT-F3 fix)  ││
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
     │    + proofTimeoutWindow (NEW)  │                                  │
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
     │                                │  8. Validate proofTimeoutWindow  │
     │                                │     (60-3600 seconds) - NEW      │
     │                                │                                  │
     │  9. SessionJobCreated event    │                                  │
     │ <──────────────────────────────│                                  │
     │                                │                                  │
```

### 4.2 Proof Submission Flow (Updated for AUDIT-F4)

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
   │  3. Get modelId from session  │                                  │
   │     sessionModel(sessionId)   │                                  │
   │                               │                                  │
   │  4. Sign proof WITH modelId:  │                                  │
   │     hash(proof, host, tokens, │                                  │
   │          modelId)  // NEW     │                                  │
   │                               │                                  │
   │  5. submitProofOfWork(        │                                  │
   │       jobId, tokens,          │                                  │
   │       proofHash, signature,   │                                  │
   │       proofCID, deltaCID)     │                                  │
   │ ─────────────────────────────>│                                  │
   │                               │                                  │
   │                               │  6. Check proofSystem != 0       │
   │                               │     (AUDIT-F2 fix)               │
   │                               │                                  │
   │                               │  7. Get modelId = sessionModel[] │
   │                               │                                  │
   │                               │  8. verifyAndMarkComplete(       │
   │                               │       proof, host, tokens,       │
   │                               │       modelId)  // NEW param     │
   │                               │ ────────────────────────────────>│
   │                               │                                  │
   │                               │  9. ECDSA.recover() with modelId │
   │                               │     in signed message (AUDIT-F4) │
   │                               │ <────────────────────────────────│
   │                               │                                  │
   │                               │ 10. Update tokensUsed            │
   │                               │     Store proofHash, deltaCID    │
   │                               │                                  │
   │ 11. ProofSubmitted event      │                                  │
   │     (includes deltaCID)       │                                  │
   │ <─────────────────────────────│                                  │
   │                               │                                  │
```

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

// Slot 20-69: Storage gap (50 slots reserved)
uint256[50] private __gap;
```

### 5.2 SessionJob Struct Layout (Updated - AUDIT-F3)

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
    uint256 proofInterval;     // 32 bytes (min tokens per proof)
    uint256 proofTimeoutWindow;// 32 bytes (NEW - AUDIT-F3: timeout in seconds)
    uint256 lastProofTime;     // 32 bytes
    bytes32 lastProofHash;     // 32 bytes
    string lastProofCID;       // Dynamic (S5 CID)
    string conversationCID;    // Dynamic (S5 CID) - set on completion
    SessionStatus status;      // 1 byte (enum: Active=0, Completed=1, TimedOut=2)
}
// Total: ~13 storage slots per session (plus dynamic strings)
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

### 5.4 Storage Gap Strategy

All upgradeable contracts reserve storage gaps for future additions:

| Contract | Gap Size | Reserved Slots |
|----------|----------|----------------|
| JobMarketplaceWithModelsUpgradeable | 50 | Future payment methods, analytics |
| NodeRegistryWithModelsUpgradeable | 36 | Reputation (reduced from 39 for slashing) |
| ModelRegistryUpgradeable | 49 | Governance extensions |
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
- `depositToken()` (JobMarketplace) - Added for AUDIT remediation

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

### 7.4 Signature Verification (Updated - AUDIT-F4)

```
┌─────────────────────────────────────────────────────────────┐
│              Proof Signature Scheme (AUDIT-F4)               │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  SIGNED MESSAGE FORMAT:                                     │
│  ──────────────────────                                     │
│  keccak256(                                                 │
│    proofHash,      // bytes32 - hash of proof data          │
│    hostAddress,    // address - session host                │
│    tokensClaimed,  // uint256 - tokens in this proof        │
│    modelId         // bytes32 - session model (NEW)         │
│  )                                                          │
│                                                             │
│  WHY modelId IS REQUIRED (AUDIT-F4):                        │
│  • Prevents cross-model replay attacks                      │
│  • Non-model sessions use bytes32(0)                        │
│  • modelId retrieved from sessionModel[sessionId]           │
│                                                             │
│  VERIFICATION FLOW:                                         │
│  1. JobMarketplace gets modelId from sessionModel[]         │
│  2. Passes modelId to ProofSystem.verifyAndMarkComplete()   │
│  3. ProofSystem includes modelId in hash reconstruction     │
│  4. ECDSA.recover() verifies host signed with modelId       │
│                                                             │
└─────────────────────────────────────────────────────────────┘
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
| JobMarketplace | `ModelSessionCreated` | Track model-specific sessions (NEW) |
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

---

## 10. AUDIT Remediation Summary

### Finding Fixes (January 31, 2026)

| Finding | Severity | Description | Fix |
|---------|----------|-------------|-----|
| AUDIT-F1 | Low | Dead `onlyRegisteredHost` modifier | Removed unused code |
| AUDIT-F2 | High | ProofSystem address(0) allows arbitrary proofs | Added require check |
| AUDIT-F3 | Medium | `proofInterval` dual interpretation | Separate `proofTimeoutWindow` param |
| AUDIT-F4 | Medium | Missing modelId in signatures | Include modelId in signed message |
| AUDIT-F5 | Low | Missing `createSessionFromDepositForModel` | Added new function |

### Breaking Changes

| Change | Impact | Migration |
|--------|--------|-----------|
| ProofSystem required | Sessions fail if ProofSystem not configured | Ensure ProofSystem is set before deployment |
| `proofTimeoutWindow` parameter | All session creation calls need new parameter | SDK must pass 60-3600 seconds |
| `modelId` in signature | Hosts MUST include modelId in signed message | Host software update required |
| IProofSystem interface | Functions now require modelId parameter | Update all callers |

### Test Contract Verification

```bash
# Verify test contracts include fixes
cast call 0x95132177F964FF053C1E874b53CF74d819618E06 "MIN_PROOF_TIMEOUT()" --rpc-url https://sepolia.base.org
# Expected: 60

cast call 0x95132177F964FF053C1E874b53CF74d819618E06 "MAX_PROOF_TIMEOUT()" --rpc-url https://sepolia.base.org
# Expected: 3600
```
