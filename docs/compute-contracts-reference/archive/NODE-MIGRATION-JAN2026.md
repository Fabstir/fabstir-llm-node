# Node Software Migration Guide: January 2026 Update

**Version:** 2.0.0
**Date:** January 9, 2026
**Network:** Base Sepolia (Chain ID: 84532)
**Solidity:** ^0.8.24 (compiled with 0.8.30)

---

## Executive Summary

This guide covers all changes required for host node software to work with the January 2026 contract updates. These updates include security audit remediation, Solidity version upgrade, and gas optimizations.

### Quick Reference: What Changed

| Change | Impact | Action Required |
|--------|--------|-----------------|
| Proof signing required | **CRITICAL** | Update proof submission code |
| Solidity ^0.8.24 | None | No action (internal change) |
| ReentrancyGuardTransient | None | No action (~4,900 gas savings) |
| ABI updates | **Required** | Update local ABI files |
| ProofSystem rename | Low | Update if calling directly |

### Contract Addresses (Updated January 9, 2026)

⚠️ **JobMarketplace proxy address has changed** due to clean slate deployment.

| Contract | Proxy Address |
|----------|---------------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` ⚠️ NEW |
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` |
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` |
| HostEarnings | `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` |
| USDC Token | `0x036CbD53842c5426634e7929541eC2318f3dCF7e` |
| FAB Token | `0xC78949004B4EB6dEf2D66e49Cd81231472612D62` |

---

## 1. CRITICAL: Proof Signing Requirement

### Overview

Every proof submission must now include a cryptographic signature from the host wallet. This prevents unauthorized proof submissions and token manipulation.

### New Function Signature

```solidity
function submitProofOfWork(
    uint256 jobId,
    uint256 tokensClaimed,
    bytes32 proofHash,
    bytes calldata signature,  // NEW: 65 bytes ECDSA signature
    string calldata proofCID
) external
```

### Signature Generation Process

```
1. proofHash = keccak256(proofData)
2. dataHash = keccak256(abi.encodePacked(proofHash, hostAddress, tokensClaimed))
3. signature = wallet.signMessage(dataHash)  // EIP-191 personal sign
```

### TypeScript Implementation

```typescript
import { ethers, keccak256, solidityPacked, getBytes, Wallet } from 'ethers';

interface ProofSubmissionData {
  sessionId: bigint;
  tokensClaimed: bigint;
  proofData: Uint8Array;
  proofCID: string;
}

class HostProofSigner {
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
    const { sessionId, tokensClaimed, proofData, proofCID } = data;

    // Step 1: Generate proof hash
    const proofHash = keccak256(proofData);

    // Step 2: Create data hash for signing
    const dataHash = keccak256(
      solidityPacked(
        ['bytes32', 'address', 'uint256'],
        [proofHash, this.hostWallet.address, tokensClaimed]
      )
    );

    // Step 3: Sign with EIP-191 personal sign
    const signature = await this.hostWallet.signMessage(getBytes(dataHash));

    // Step 4: Submit to contract
    const tx = await this.marketplace.submitProofOfWork(
      sessionId,
      tokensClaimed,
      proofHash,
      signature,
      proofCID
    );

    return await tx.wait();
  }
}
```

### Python Implementation

```python
from web3 import Web3
from eth_account import Account
from eth_account.messages import encode_defunct

class HostProofSigner:
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
        proof_cid: str
    ) -> str:
        # Step 1: Generate proof hash
        proof_hash = Web3.keccak(proof_data)

        # Step 2: Create data hash for signing
        data_hash = Web3.keccak(
            Web3.solidity_keccak(
                ['bytes32', 'address', 'uint256'],
                [proof_hash, self.account.address, tokens_claimed]
            )
        )

        # Step 3: Sign with EIP-191
        message = encode_defunct(data_hash)
        signed = self.account.sign_message(message)
        signature = signed.signature

        # Step 4: Submit to contract
        tx = self.marketplace.functions.submitProofOfWork(
            session_id,
            tokens_claimed,
            proof_hash,
            signature,
            proof_cid
        ).build_transaction({
            'from': self.account.address,
            'nonce': self.w3.eth.get_transaction_count(self.account.address),
            'gas': 300000,
            'gasPrice': self.w3.eth.gas_price
        })

        signed_tx = self.account.sign_transaction(tx)
        tx_hash = self.w3.eth.send_raw_transaction(signed_tx.rawTransaction)
        return tx_hash.hex()
```

### Go Implementation

```go
package main

import (
    "crypto/ecdsa"
    "math/big"

    "github.com/ethereum/go-ethereum/accounts/abi/bind"
    "github.com/ethereum/go-ethereum/common"
    "github.com/ethereum/go-ethereum/crypto"
)

type HostProofSigner struct {
    privateKey  *ecdsa.PrivateKey
    hostAddress common.Address
    marketplace *JobMarketplace // Generated from ABI
}

func (h *HostProofSigner) SubmitProof(
    sessionId *big.Int,
    tokensClaimed *big.Int,
    proofData []byte,
    proofCID string,
) (*types.Transaction, error) {
    // Step 1: Generate proof hash
    proofHash := crypto.Keccak256Hash(proofData)

    // Step 2: Create data hash for signing
    dataHash := crypto.Keccak256Hash(
        proofHash.Bytes(),
        h.hostAddress.Bytes(),
        common.LeftPadBytes(tokensClaimed.Bytes(), 32),
    )

    // Step 3: Sign with EIP-191 personal sign prefix
    prefixedHash := crypto.Keccak256Hash(
        []byte("\x19Ethereum Signed Message:\n32"),
        dataHash.Bytes(),
    )
    signature, err := crypto.Sign(prefixedHash.Bytes(), h.privateKey)
    if err != nil {
        return nil, err
    }
    // Fix v value for Ethereum (27 or 28)
    signature[64] += 27

    // Step 4: Submit to contract
    return h.marketplace.SubmitProofOfWork(
        &bind.TransactOpts{...},
        sessionId,
        tokensClaimed,
        proofHash,
        signature,
        proofCID,
    )
}
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

### Key ABI Changes

**submitProofOfWork** - Added `signature` parameter:
```json
{
  "name": "submitProofOfWork",
  "inputs": [
    {"name": "jobId", "type": "uint256"},
    {"name": "tokensClaimed", "type": "uint256"},
    {"name": "proofHash", "type": "bytes32"},
    {"name": "signature", "type": "bytes"},
    {"name": "proofCID", "type": "string"}
  ]
}
```

**ProofSystem** - Function renamed:
```json
// OLD (deprecated)
{"name": "verifyEKZL", ...}

// NEW
{"name": "verifyHostSignature", ...}
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
- [ ] Review proof signing implementation in Section 1
- [ ] Test signature generation locally
- [ ] Verify host wallet has signing capability

### Code Changes

- [ ] Update `submitProofOfWork` calls to include signature
- [ ] Replace ABI files in your project
- [ ] If using ProofSystem directly: rename `verifyEKZL` → `verifyHostSignature`

### Testing

- [ ] Test proof submission on Base Sepolia
- [ ] Verify signature is accepted by contract
- [ ] Test session completion flow
- [ ] Test earnings withdrawal

### Post-Migration

- [ ] Monitor for any rejected proofs
- [ ] Check gas costs (should be ~4,900 lower per nonReentrant call)
- [ ] Update any documentation/runbooks

---

## 9. Troubleshooting

### "Invalid signature length"

**Cause**: Signature must be exactly 65 bytes (r: 32, s: 32, v: 1)

**Solution**: Ensure you're using proper ECDSA signing, not a truncated signature

### "Invalid signature"

**Cause**: Data hash computed incorrectly or wrong signer

**Solution**:
1. Verify proofHash = keccak256(proofData)
2. Verify dataHash = keccak256(abi.encodePacked(proofHash, hostAddress, tokensClaimed))
3. Verify signing with the registered host wallet
4. Verify using EIP-191 personal sign

### "Only host can submit proof"

**Cause**: Signer address doesn't match session host

**Solution**: The wallet signing must be the same address registered as session host

### "Excessive tokens claimed"

**Cause**: Token claim exceeds rate limit

**Solution**: Wait longer between proof submissions or claim fewer tokens

### "Must claim minimum tokens"

**Cause**: tokensClaimed < 100

**Solution**: Minimum claim is 100 tokens per proof

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

const ADDRESSES = {
  marketplace: '0x3CaCbf3f448B420918A93a88706B26Ab27a3523E',  // Updated Jan 9, 2026
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

  // Submit proof with signature
  const proofData = new Uint8Array([/* your proof data */]);
  const proofHash = ethers.keccak256(proofData);
  const tokensClaimed = 1000n;
  const sessionId = 1n;
  const proofCID = 'QmYourProofCID';

  const dataHash = ethers.keccak256(
    ethers.solidityPacked(
      ['bytes32', 'address', 'uint256'],
      [proofHash, hostWallet.address, tokensClaimed]
    )
  );

  const signature = await hostWallet.signMessage(ethers.getBytes(dataHash));

  const tx = await marketplace.submitProofOfWork(
    sessionId,
    tokensClaimed,
    proofHash,
    signature,
    proofCID
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

**Document Version:** 2.0.0
**Last Updated:** January 10, 2026
