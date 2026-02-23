# Node Software Migration Guide: January-February 2026 Update

**Version:** 3.0.0
**Date:** February 4, 2026
**Network:** Base Sepolia (Chain ID: 84532)
**Solidity:** ^0.8.24 (compiled with 0.8.30)

---

## Executive Summary

This guide covers all changes required for host node software to work with the January-February 2026 contract updates. These updates include security audit remediation, Solidity version upgrade, gas optimizations, and **signature removal** (February 4, 2026).

### Quick Reference: What Changed

| Change | Impact | Action Required |
|--------|--------|-----------------|
| **Signature REMOVED** (Feb 4) | **SIMPLIFICATION** | Remove signature generation code |
| submitProofOfWork: 5 params | **BREAKING** | Update proof submission (remove signature param) |
| deltaCID parameter (Jan 14) | Required | Add deltaCID to proof submissions |
| Solidity ^0.8.24 | None | No action (internal change) |
| ReentrancyGuardTransient | None | No action (~4,900 gas savings) |
| ABI updates | **Required** | Update local ABI files |

### Contract Addresses

#### Remediation Contracts (Active Development - Use These)

| Contract | Proxy Address |
|----------|---------------|
| JobMarketplace | `0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4` ✅ |
| ProofSystem | `0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31` ✅ |
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` |
| HostEarnings | `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` |
| USDC Token | `0x036CbD53842c5426634e7929541eC2318f3dCF7e` |
| FAB Token | `0xC78949004B4EB6dEf2D66e49Cd81231472612D62` |

#### Frozen Audit Contracts (DO NOT USE for new development)

| Contract | Proxy Address |
|----------|---------------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` 🔒 |
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` 🔒 |

---

## 1. SIMPLIFIED: Proof Submission (No Signature Required!)

### Overview (February 4, 2026 Update)

**Good news!** The signature requirement has been **removed**. Authentication is now handled via `msg.sender == session.host` check, which provides equivalent security with ~3,000 gas savings per proof.

### Current Function Signature (5 Parameters)

```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    string calldata proofCID,
    string calldata deltaCID    // For incremental changes (can be "")
) external
```

### TypeScript Implementation (Simplified!)

```typescript
import { ethers, keccak256, Wallet } from 'ethers';

interface ProofSubmissionData {
  sessionId: bigint;
  tokensClaimed: bigint;
  proofData: Uint8Array;
  proofCID: string;
  deltaCID: string;  // Use "" if not tracking incremental changes
}

class HostProofSubmitter {
  private hostWallet: Wallet;
  private marketplace: ethers.Contract;

  constructor(
    hostPrivateKey: string,
    marketplaceAddress: string,
    provider: ethers.Provider
  ) {
    this.hostWallet = new Wallet(hostPrivateKey, provider);
    this.marketplace = new ethers.Contract(
      marketplaceAddress,
      JobMarketplaceABI,
      this.hostWallet
    );
  }

  async submitProof(data: ProofSubmissionData): Promise<ethers.TransactionReceipt> {
    const { sessionId, tokensClaimed, proofData, proofCID, deltaCID } = data;

    // Step 1: Generate proof hash
    const proofHash = keccak256(proofData);

    // Step 2: Submit directly - NO SIGNATURE NEEDED!
    // Authentication via msg.sender == session.host
    const tx = await this.marketplace.submitProofOfWork(
      sessionId,
      tokensClaimed,
      proofHash,
      proofCID,
      deltaCID
    );

    return await tx.wait();
  }
}
```

### Python Implementation (Simplified!)

```python
from web3 import Web3
from eth_account import Account

class HostProofSubmitter:
    def __init__(self, private_key: str, marketplace_address: str, rpc_url: str):
        self.w3 = Web3(Web3.HTTPProvider(rpc_url))
        self.account = Account.from_key(private_key)
        self.marketplace = self.w3.eth.contract(
            address=marketplace_address,
            abi=JOB_MARKETPLACE_ABI
        )

    def submit_proof(
        self,
        session_id: int,
        tokens_claimed: int,
        proof_data: bytes,
        proof_cid: str,
        delta_cid: str = ""  # Optional, use "" if not tracking
    ) -> str:
        # Step 1: Generate proof hash
        proof_hash = Web3.keccak(proof_data)

        # Step 2: Submit directly - NO SIGNATURE NEEDED!
        tx = self.marketplace.functions.submitProofOfWork(
            session_id,
            tokens_claimed,
            proof_hash,
            proof_cid,
            delta_cid
        ).build_transaction({
            'from': self.account.address,
            'nonce': self.w3.eth.get_transaction_count(self.account.address),
            'gas': 250000,  # Lower gas without signature verification
            'gasPrice': self.w3.eth.gas_price
        })

        signed_tx = self.account.sign_transaction(tx)
        tx_hash = self.w3.eth.send_raw_transaction(signed_tx.rawTransaction)
        return tx_hash.hex()
```

### Go Implementation (Simplified!)

```go
package main

import (
    "math/big"

    "github.com/ethereum/go-ethereum/accounts/abi/bind"
    "github.com/ethereum/go-ethereum/crypto"
)

type HostProofSubmitter struct {
    transactOpts *bind.TransactOpts
    marketplace  *JobMarketplace // Generated from ABI
}

func (h *HostProofSubmitter) SubmitProof(
    sessionId *big.Int,
    tokensClaimed *big.Int,
    proofData []byte,
    proofCID string,
    deltaCID string,
) (*types.Transaction, error) {
    // Step 1: Generate proof hash
    proofHash := crypto.Keccak256Hash(proofData)

    // Step 2: Submit directly - NO SIGNATURE NEEDED!
    return h.marketplace.SubmitProofOfWork(
        h.transactOpts,
        sessionId,
        tokensClaimed,
        proofHash,
        proofCID,
        deltaCID,
    )
}
```

### Migration from Signature-Based Code

If you previously had signature generation code, **remove it**:

```typescript
// OLD CODE (January 2026) - REMOVE THIS
const dataHash = keccak256(solidityPacked(['bytes32', 'address', 'uint256'], [proofHash, hostAddress, tokensClaimed]));
const signature = await hostWallet.signMessage(getBytes(dataHash));
await marketplace.submitProofOfWork(sessionId, tokensClaimed, proofHash, signature, proofCID, deltaCID);

// NEW CODE (February 2026) - USE THIS
await marketplace.submitProofOfWork(sessionId, tokensClaimed, proofHash, proofCID, deltaCID);
```

---

## 2. Node Registration (Unchanged)

Node registration remains the same. For reference:

```typescript
async function registerNode(
  nodeRegistry: ethers.Contract,
  fabToken: ethers.Contract,
  hostWallet: Wallet,
  params: {
    metadata: string;
    apiUrl: string;
    modelIds: string[];
    minPriceNative: bigint;  // With PRICE_PRECISION (×1000)
    minPriceStable: bigint;  // With PRICE_PRECISION (×1000)
  }
) {
  const { metadata, apiUrl, modelIds, minPriceNative, minPriceStable } = params;
  const MIN_STAKE = ethers.parseEther("1000"); // 1000 FAB

  // Approve FAB token spending
  await fabToken.connect(hostWallet).approve(nodeRegistry.target, MIN_STAKE);

  // Register node
  const tx = await nodeRegistry.connect(hostWallet).registerNode(
    metadata,
    apiUrl,
    modelIds,
    minPriceNative,
    minPriceStable
  );

  return await tx.wait();
}
```

### Price Calculation Reference

```typescript
const PRICE_PRECISION = 1000n;

// For $5/million tokens (stable)
const stablePrice = 5n * PRICE_PRECISION; // 5000

// For $0.01/million tokens (native, assuming $4400 ETH)
const nativePrice = (10n * 10n**18n) / (4400n * 1_000_000n) * PRICE_PRECISION;
```

---

## 3. Withdrawing Earnings

Hosts withdraw earnings from the HostEarnings contract:

```typescript
async function withdrawEarnings(
  hostEarnings: ethers.Contract,
  hostWallet: Wallet,
  tokenAddress: string // address(0) for ETH
) {
  // Check balance first
  const balance = await hostEarnings.getBalance(hostWallet.address, tokenAddress);
  console.log(`Available: ${ethers.formatUnits(balance, tokenAddress === ethers.ZeroAddress ? 18 : 6)}`);

  if (balance > 0n) {
    // Withdraw all
    const tx = await hostEarnings.connect(hostWallet).withdrawAll(tokenAddress);
    return await tx.wait();
  }
}

// Withdraw multiple tokens at once
async function withdrawMultipleTokens(
  hostEarnings: ethers.Contract,
  hostWallet: Wallet,
  tokenAddresses: string[]
) {
  const tx = await hostEarnings.connect(hostWallet).withdrawMultiple(tokenAddresses);
  return await tx.wait();
}
```

---

## 4. Session Completion

Hosts should complete sessions to settle payments:

```typescript
async function completeSession(
  marketplace: ethers.Contract,
  hostWallet: Wallet,
  sessionId: bigint,
  conversationCID: string
) {
  // Host must wait for dispute window (default 30 seconds)
  // Check if dispute window has passed
  const session = await marketplace.sessionJobs(sessionId);
  const disputeWindow = await marketplace.disputeWindow();
  const canComplete = BigInt(Math.floor(Date.now() / 1000)) >= session.startTime + disputeWindow;

  if (!canComplete) {
    throw new Error('Must wait for dispute window');
  }

  const tx = await marketplace.connect(hostWallet).completeSessionJob(
    sessionId,
    conversationCID
  );

  return await tx.wait();
}
```

---

## 5. ABI Updates Required

Download the latest ABIs from the `client-abis/` folder:

| File | Description |
|------|-------------|
| `JobMarketplaceWithModelsUpgradeable-CLIENT-ABI.json` | Main marketplace |
| `NodeRegistryWithModelsUpgradeable-CLIENT-ABI.json` | Node registration |
| `HostEarningsUpgradeable-CLIENT-ABI.json` | Earnings withdrawal |
| `ProofSystemUpgradeable-CLIENT-ABI.json` | Proof verification |
| `ModelRegistryUpgradeable-CLIENT-ABI.json` | Model queries |

### Key ABI Changes (February 4, 2026)

**submitProofOfWork** - Signature REMOVED, deltaCID added (5 params):
```json
{
  "name": "submitProofOfWork",
  "inputs": [
    {"name": "jobId", "type": "uint256"},
    {"name": "tokensClaimed", "type": "uint256"},
    {"name": "proofHash", "type": "bytes32"},
    {"name": "proofCID", "type": "string"},
    {"name": "deltaCID", "type": "string"}
  ]
}
```

**ProofSystem** - Signature verification removed:
```json
// REMOVED (no longer exists)
{"name": "verifyHostSignature", ...}
{"name": "verifyAndMarkComplete", ...}

// NEW (simple replay protection)
{"name": "markProofUsed", "inputs": [{"name": "proofHash", "type": "bytes32"}]}
```

---

## 6. Rate Limits and Constraints

### Proof Submission Rate Limit

```
expectedTokens = timeSinceLastProof * 1000
maxAllowed = expectedTokens * 2

// Example: If 5 seconds passed, max tokens = 10,000
```

### Minimum Token Claim

```solidity
uint256 public constant MIN_PROVEN_TOKENS = 100;
```

### Session Timeout

Sessions can be timed out if host doesn't submit proofs:
- Timeout after: 3× proofInterval (e.g., 300 seconds if proofInterval=100)
- Anyone can call `triggerSessionTimeout(sessionId)`

---

## 7. Event Monitoring

Monitor these events for operational awareness:

```solidity
// Session created (new work available)
event SessionJobCreated(
    uint256 indexed jobId,
    address indexed depositor,
    address indexed host,
    uint256 deposit
);

// Proof accepted
event ProofSubmitted(
    uint256 indexed jobId,
    address indexed host,
    uint256 tokensClaimed,
    bytes32 proofHash,
    string proofCID
);

// Session completed (payment settled)
event SessionJobCompleted(
    uint256 indexed jobId,
    address indexed depositor,
    address indexed host,
    uint256 totalTokens,
    uint256 hostPayment
);

// Earnings credited (check HostEarnings)
event EarningsCredited(
    address indexed host,
    address indexed token,
    uint256 amount,
    uint256 newBalance
);
```

---

## 8. Migration Checklist

### Pre-Migration

- [ ] Download latest ABIs from `client-abis/`
- [ ] Review simplified proof submission in Section 1
- [ ] Note: NO signature generation needed anymore!

### Code Changes

- [ ] **REMOVE** signature generation code (no longer needed)
- [ ] Update `submitProofOfWork` to 5 parameters (no signature)
- [ ] Add `deltaCID` parameter (can be empty string "")
- [ ] Replace ABI files in your project
- [ ] Update contract address to remediation proxy: `0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4`

### Testing

- [ ] Test proof submission on Base Sepolia (remediation contract)
- [ ] Verify proofs are accepted without signature
- [ ] Test session completion flow
- [ ] Test earnings withdrawal

### Post-Migration

- [ ] Monitor for any rejected proofs
- [ ] Check gas costs (should be ~3,000 lower without signature verification)
- [ ] Update any documentation/runbooks

---

## 9. Troubleshooting

### "Not host"

**Cause**: Transaction sender doesn't match session host
**Note**: Error string shortened from "Only host can submit proof" to "Not host" in Feb 2026 deployment.

**Solution**: The wallet sending the transaction must be the same address registered as the session host. No signature needed - just send from the correct wallet.

### "Excessive tokens claimed"

**Cause**: Token claim exceeds rate limit

**Solution**: Wait longer between proof submissions or claim fewer tokens. Rate limit is based on time elapsed since last proof.

### "Must claim minimum tokens"

**Cause**: tokensClaimed < 100

**Solution**: Minimum claim is 100 tokens per proof

### "Unauthorized" (from ProofSystem)

**Cause**: JobMarketplace not authorized to call ProofSystem

**Solution**: This is a contract configuration issue. Contact admin to verify `proofSystem.setAuthorizedCaller(marketplace, true)` was called.

### Using wrong contract address

**Cause**: Still using frozen audit contract instead of remediation contract

**Solution**: Update to remediation JobMarketplace: `0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4`

---

## 10. Support

For questions or issues:
- GitHub Issues: https://github.com/fabstirp2p/contracts/issues
- Documentation: https://docs.fabstir.com

---

## Appendix A: Complete TypeScript Example

```typescript
import { ethers, Wallet } from 'ethers';
import JobMarketplaceABI from './abis/JobMarketplaceWithModelsUpgradeable-CLIENT-ABI.json';
import HostEarningsABI from './abis/HostEarningsUpgradeable-CLIENT-ABI.json';

// Use REMEDIATION contracts (Feb 4, 2026)
const ADDRESSES = {
  marketplace: '0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4',  // Remediation proxy
  hostEarnings: '0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0',
};

async function main() {
  const provider = new ethers.JsonRpcProvider('https://sepolia.base.org');
  const hostWallet = new Wallet(process.env.HOST_PRIVATE_KEY!, provider);

  const marketplace = new ethers.Contract(
    ADDRESSES.marketplace,
    JobMarketplaceABI,
    hostWallet
  );

  const hostEarnings = new ethers.Contract(
    ADDRESSES.hostEarnings,
    HostEarningsABI,
    hostWallet
  );

  // Submit proof - NO SIGNATURE NEEDED! (Feb 4, 2026 update)
  const proofData = new Uint8Array([/* your proof data */]);
  const proofHash = ethers.keccak256(proofData);
  const tokensClaimed = 1000n;
  const sessionId = 1n;
  const proofCID = 'bafyreib...';  // S5 CID for full proof
  const deltaCID = '';              // Optional: S5 CID for delta changes

  // Direct submission - authentication via msg.sender == session.host
  const tx = await marketplace.submitProofOfWork(
    sessionId,
    tokensClaimed,
    proofHash,
    proofCID,
    deltaCID
  );

  console.log('Proof submitted:', tx.hash);
  await tx.wait();

  // Check and withdraw earnings
  const balance = await hostEarnings.getBalance(hostWallet.address, ethers.ZeroAddress);
  console.log('ETH earnings:', ethers.formatEther(balance));

  if (balance > 0n) {
    const withdrawTx = await hostEarnings.withdrawAll(ethers.ZeroAddress);
    await withdrawTx.wait();
    console.log('Earnings withdrawn');
  }
}

main().catch(console.error);
```

---

## 8. Corrupt Node Recovery (January 10, 2026)

### Problem

During contract upgrades, some registered hosts ended up in a "corrupt" state where:
- `nodes[host].active = true`
- `activeNodesIndex[host] = 0`
- But the host was NOT in `activeNodesList[]`

This caused `unregisterNode()` to fail or corrupt other nodes' data.

### Solution for Node Operators

**If you cannot unregister your node**, try these options:

**Option 1: Call `unregisterNode()` directly** (safety check now handles corrupt state):

```typescript
import { ethers, Wallet } from 'ethers';
import NodeRegistryABI from './NodeRegistryWithModelsUpgradeable-CLIENT-ABI.json';

const provider = new ethers.JsonRpcProvider('https://sepolia.base.org');
const wallet = new Wallet(process.env.HOST_PRIVATE_KEY!, provider);

const nodeRegistry = new ethers.Contract(
  '0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22',
  NodeRegistryABI,
  wallet
);

// This now works even with corrupt state
const tx = await nodeRegistry.unregisterNode();
await tx.wait();
console.log('Node unregistered, stake returned');
```

**Option 2: Contact admin** to call `repairCorruptNode()`:

```bash
# Admin command to repair corrupt node
cast send 0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22 \
  "repairCorruptNode(address)" \
  <YOUR_HOST_ADDRESS> \
  --rpc-url "https://sepolia.base.org" \
  --private-key $ADMIN_PRIVATE_KEY
```

### New ABI Entries

```json
{
  "name": "repairCorruptNode",
  "type": "function",
  "inputs": [{"name": "nodeAddress", "type": "address"}],
  "stateMutability": "nonpayable"
}

{
  "name": "CorruptNodeRepaired",
  "type": "event",
  "inputs": [
    {"name": "operator", "type": "address", "indexed": true},
    {"name": "stakeReturned", "type": "uint256", "indexed": false}
  ]
}
```

---

**Document Version:** 3.0.0
**Last Updated:** February 4, 2026

### Version History
- **3.0.0** (Feb 4, 2026): Signature removal - simplified proof submission
- **2.0.0** (Jan 10, 2026): Added corrupt node recovery
- **1.0.0** (Jan 9, 2026): Initial release with signature requirement
