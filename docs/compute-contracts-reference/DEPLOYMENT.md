# Fabstir Marketplace Deployment

## Current Production Deployment (2025-08-24)

### 🚀 Active Contracts (Base Sepolia)

- **JobMarketplaceFABWithEarnings**: `0xEB646BF2323a441698B256623F858c8787d70f9F` ✅ (LATEST)
  - FAB token staking integration
  - USDC payments enabled
  - 10% platform fee
  - **NEW: Earnings accumulation system**
  
- **HostEarnings**: `0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E` ✅ (NEW)
  - Accumulates host earnings instead of direct transfers
  - Batch withdrawal support for gas savings
  - 40-46% gas reduction for multiple jobs
  - Transparent balance tracking
  
- **NodeRegistryFAB**: `0x87516C13Ea2f99de598665e14cab64E191A0f8c4` ✅
  - 1000 FAB minimum stake
  - ~$1,000 entry cost
  - Non-custodial staking

- **PaymentEscrowWithEarnings**: `0x7abC91AF9E5aaFdc954Ec7a02238d0796Bbf9a3C` ✅ (LATEST)
  - Multi-token support (primarily USDC)
  - 10% fee handling (1000 basis points)
  - Fees go to TreasuryManager
  - **NEW: Routes payments to HostEarnings contract**

- **TreasuryManager**: `0x4e770e723B95A0d8923Db006E49A8a3cb0BAA078` ✅
  - Receives all platform fees
  - Distributes to 5 sub-funds
  - Transparent fee allocation

- **FAB Token**: `0xC78949004B4EB6dEf2D66e49Cd81231472612D62`
  - Platform native token
  - Used for host staking
  - 18 decimals

- **USDC**: `0x036CbD53842c5426634e7929541eC2318f3dCF7e`
  - Base Sepolia USDC
  - 6 decimals
  - Used for job payments

### Token Addresses

- **FAB Token**: `0xC78949004B4EB6dEf2D66e49Cd81231472612D62`
  - Platform native token
  - Used for host staking
  - 18 decimals

- **USDC**: `0x036CbD53842c5426634e7929541eC2318f3dCF7e`
  - Base Sepolia USDC
  - 6 decimals
  - Used for job payments

## SDK Integration

```javascript
// Production contracts configuration
const CONTRACTS = {
  // Core contracts
  JOB_MARKETPLACE: "0xEB646BF2323a441698B256623F858c8787d70f9F", // LATEST - with earnings accumulation
  HOST_EARNINGS: "0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E", // NEW - accumulation system
  NODE_REGISTRY: "0x87516C13Ea2f99de598665e14cab64E191A0f8c4",
  PAYMENT_ESCROW: "0x7abC91AF9E5aaFdc954Ec7a02238d0796Bbf9a3C", // LATEST - with earnings routing
  TREASURY_MANAGER: "0x4e770e723B95A0d8923Db006E49A8a3cb0BAA078",
  
  // Tokens
  FAB_TOKEN: "0xC78949004B4EB6dEf2D66e49Cd81231472612D62",
  USDC: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
};

// Network configuration
const NETWORK = {
  chainId: 84532, // Base Sepolia
  rpcUrl: "https://sepolia.base.org"
};
```

## Complete Flow Example

### 1. Host Registration (FAB Staking)
```javascript
// Approve FAB tokens
await fabToken.approve(NODE_REGISTRY, ethers.parseEther("1000"));

// Register as host
await nodeRegistry.registerNode("gpu:rtx4090,region:us-west");
```

### 2. Job Posting (USDC Payment)
```javascript
// Approve USDC
await usdc.approve(JOB_MARKETPLACE, ethers.parseUnits("10", 6));

// Post job
const details = {
  modelId: "gpt-4",
  prompt: "Process this request",
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

await jobMarketplace.postJobWithToken(
  details,
  requirements,
  USDC,
  ethers.parseUnits("10", 6) // 10 USDC
);
```

### 3. Job Claiming & Completion
```javascript
// Host claims job
await jobMarketplaceFAB.claimJob(jobId);

// Host completes job
await jobMarketplaceFAB.completeJob(jobId, "result-hash", "0x");

// NEW: Payment accumulated (not transferred directly):
// - Host earnings: +9 USDC (90%) credited to HostEarnings contract
// - TreasuryManager: +1 USDC (10% fee) transferred immediately
```

### 4. Withdrawing Accumulated Earnings (NEW)
```javascript
// Check accumulated earnings
const balance = await hostEarnings.getBalance(hostAddress, USDC);
console.log("Accumulated earnings:", balance);

// Withdraw all accumulated earnings
await hostEarnings.withdrawAll(USDC);
// Host receives all accumulated USDC in one transaction

// Or withdraw specific amount
await hostEarnings.withdraw(ethers.parseUnits("50", 6), USDC);
```

## Verified Transaction Flow

Complete working flow on Base Sepolia:

1. **FAB Transfer**: [0xdf21f074635f5b03a78d3acd7ea90056779759b0b14feba0c042e9d3224a9067](https://sepolia.basescan.org/tx/0xdf21f074635f5b03a78d3acd7ea90056779759b0b14feba0c042e9d3224a9067)
2. **Host Registration**: [0xa193198058e70343105b8e8306fa8600421c77417658ad5780b03a202b3666dc](https://sepolia.basescan.org/tx/0xa193198058e70343105b8e8306fa8600421c77417658ad5780b03a202b3666dc)
3. **Job Posted**: [0xd186457017d07e7ee5e858c9ca3862bac964624629a8581a77e8ba9a9acd6d8f](https://sepolia.basescan.org/tx/0xd186457017d07e7ee5e858c9ca3862bac964624629a8581a77e8ba9a9acd6d8f)
4. **Job Claimed**: [0xb6995908db02db9620631e15641f3e643f826858cb06c2f955fe2feb0b5fc375](https://sepolia.basescan.org/tx/0xb6995908db02db9620631e15641f3e643f826858cb06c2f955fe2feb0b5fc375)
5. **Payment Released**: [0x049085aab9e89b8425fd5010c8721a8acb409b952aa9034158b52d0e08062406](https://sepolia.basescan.org/tx/0x049085aab9e89b8425fd5010c8721a8acb409b952aa9034158b52d0e08062406)

## Testing Commands

### Verify FAB System
```bash
# Check NodeRegistryFAB
cast call 0x87516C13Ea2f99de598665e14cab64E191A0f8c4 "MIN_STAKE()" --rpc-url https://sepolia.base.org
# Expected: 1000000000000000000000 (1000 FAB)

# Check JobMarketplaceFAB connections
cast call 0x870E74D1Fe7D9097deC27651f67422B598b689Cd "nodeRegistry()" --rpc-url https://sepolia.base.org
# Expected: 0x87516C13Ea2f99de598665e14cab64E191A0f8c4

cast call 0x870E74D1Fe7D9097deC27651f67422B598b689Cd "paymentEscrow()" --rpc-url https://sepolia.base.org
# Expected: 0xF382E11ebdB90e6cDE55521C659B70eEAc1C9ac3

# Check USDC configuration
cast call 0x870E74D1Fe7D9097deC27651f67422B598b689Cd "usdcAddress()" --rpc-url https://sepolia.base.org
# Expected: 0x036CbD53842c5426634e7929541eC2318f3dCF7e

# Check fee configuration
cast call 0xF382E11ebdB90e6cDE55521C659B70eEAc1C9ac3 "feeBasisPoints()" --rpc-url https://sepolia.base.org
# Expected: 1000 (10% fee)

cast call 0xF382E11ebdB90e6cDE55521C659B70eEAc1C9ac3 "arbiter()" --rpc-url https://sepolia.base.org
# Expected: 0x4e770e723B95A0d8923Db006E49A8a3cb0BAA078 (TreasuryManager)
```

### Check Balances
```bash
# Check FAB balance
cast call 0xC78949004B4EB6dEf2D66e49Cd81231472612D62 "balanceOf(address)" <ADDRESS> --rpc-url https://sepolia.base.org | cast to-dec

# Check USDC balance
cast call 0x036CbD53842c5426634e7929541eC2318f3dCF7e "balanceOf(address)" <ADDRESS> --rpc-url https://sepolia.base.org | cast to-dec
```

## Deployment Scripts

### Deploy FAB System
```bash
# Deploy NodeRegistryFAB
forge script script/DeployNodeRegistryFAB.s.sol --rpc-url https://sepolia.base.org --broadcast

# Deploy JobMarketplaceFAB
forge script script/DeployFinalJobMarketplaceFAB.s.sol --rpc-url https://sepolia.base.org --broadcast

# Deploy PaymentEscrow
forge script script/DeployNewPaymentEscrow.s.sol --rpc-url https://sepolia.base.org --broadcast
```

## System Features

### FAB Token Staking
- **Minimum Stake**: 1000 FAB tokens
- **USD Value**: ~$1,000
- **Entry Barrier**: Significantly lower than traditional staking
- **Slashing Risk**: None
- **Unstaking**: Anytime when not processing jobs

### USDC Payment System
- **Payment Token**: USDC (6 decimals)
- **Platform Fee**: 10% (1000 basis points)
- **Payment Flow**: Automatic release on job completion
- **Host Earnings**: 90% of job payment
- **Fee Collection**: Sent to TreasuryManager for distribution

### Payment Flow
1. Renter posts job with USDC
2. USDC held in PaymentEscrow
3. Host completes job
4. Payment released with 10% fee:
   - 90% to host
   - 10% to TreasuryManager
5. TreasuryManager distributes fees:
   - 3% Development Fund
   - 2% Ecosystem Growth
   - 2% Insurance/Security
   - 2% FAB Buyback/Burn
   - 1% Future Reserve

## Support & Resources

- **Documentation**: [Technical Docs](./technical/contracts/)
- **GitHub**: [fabstir-compute-contracts](https://github.com/Fabstir/fabstir-compute-contracts)
- **Support**: Discord/Telegram (TBD)

## Contract Verification

All contracts are verified on BaseScan:
- [JobMarketplaceFAB](https://sepolia.basescan.org/address/0x870E74D1Fe7D9097deC27651f67422B598b689Cd) (NEW)
- [NodeRegistryFAB](https://sepolia.basescan.org/address/0x87516C13Ea2f99de598665e14cab64E191A0f8c4)
- [PaymentEscrow](https://sepolia.basescan.org/address/0xF382E11ebdB90e6cDE55521C659B70eEAc1C9ac3) (NEW)
- [TreasuryManager](https://sepolia.basescan.org/address/0x4e770e723B95A0d8923Db006E49A8a3cb0BAA078) (NEW)