# Breaking Changes

---

## January 9, 2026: Security Audit Remediation - Solidity Upgrade & Contract Changes

**Contracts Affected**: All upgradeable contracts
**Impact Level**: HIGH - JobMarketplace proxy address changed (clean slate deployment)

### ⚠️ CRITICAL: NEW PROXY ADDRESS

**JobMarketplace has a NEW proxy address** due to clean slate deployment for security audit:

| Contract | OLD Proxy (deprecated) | NEW Proxy (use this) |
|----------|------------------------|----------------------|
| JobMarketplace | `0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D` | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` |

**Other contract addresses remain unchanged.**

### Summary

Security audit remediation completed with the following breaking changes:

| Change | Impact | Action Required |
|--------|--------|-----------------|
| **JobMarketplace proxy address** | **HIGH** | **Update SDK/node configuration to new address** |
| Solidity ^0.8.24 | LOW | Update build tooling if compiling locally |
| ReentrancyGuardTransient | LOW | No action (internal change) |
| ProofSystem function rename | HIGH | Update any direct ProofSystem calls |
| Custom ReentrancyGuard removed | NONE | No action (internal change) |

### 1. Solidity Version Upgrade

**Changed From**: `^0.8.19`
**Changed To**: `^0.8.24` (compiled with 0.8.30)

**Why**: Required to use OpenZeppelin's `ReentrancyGuardTransient` which uses EIP-1153 transient storage.

**Impact for SDK Developers**: None - ABI remains compatible.

**Impact for Node Operators**: None - no interface changes.

**Impact for Local Development**: Update Foundry/Forge if you compile contracts locally:
```bash
foundryup  # Updates to latest Foundry with Solidity 0.8.30 support
```

### 2. EIP-1153 Transient Storage Requirement

The contracts now require **EIP-1153** (Cancun upgrade) support on the target network.

**Supported Networks**:
| Network | EIP-1153 Support |
|---------|------------------|
| Base Mainnet | ✅ Since March 2024 |
| Base Sepolia | ✅ Since March 2024 |
| Ethereum Mainnet | ✅ Since March 2024 |
| opBNB | ✅ Since March 2024 |

**Impact**: If deploying to a network that hasn't undergone the Cancun upgrade, contracts will fail to deploy.

### 3. ProofSystem Function Rename (BREAKING)

**Changed From**: `verifyEKZL(bytes proof, address prover, uint256 claimedTokens)`
**Changed To**: `verifyHostSignature(bytes proof, address prover, uint256 claimedTokens)`

**Why**: The function verifies host ECDSA signatures, not EZKL proofs. The rename improves code clarity.

**Impact for SDK Developers**:
- If you call `ProofSystem` directly (rare), update function name
- If you only interact with `JobMarketplace`, no changes needed (it calls ProofSystem internally)

**Migration**:
```javascript
// Before (OLD)
await proofSystem.verifyEKZL(proof, prover, tokens);

// After (NEW)
await proofSystem.verifyHostSignature(proof, prover, tokens);
```

### 4. ReentrancyGuard Implementation Change

**Changed From**: Custom `ReentrancyGuardUpgradeable` (src/utils/)
**Changed To**: OpenZeppelin's `ReentrancyGuardTransient`

**Benefits**:
- ~4,900 gas savings per `nonReentrant` call
- Uses battle-tested OpenZeppelin code
- Removes custom code (security improvement)

**Impact**: None for SDK/Node developers - this is an internal implementation detail.

### 5. Storage Layout Unchanged

Despite the ReentrancyGuard change, **storage layout is preserved**:
- ReentrancyGuardTransient uses transient storage (not contract storage)
- Existing proxy storage slots are unaffected
- No data migration required

### 6. Updated Implementation Addresses

After deploying the upgraded implementations, proxy implementations will change:

| Contract | Proxy | Implementation |
|----------|-------|----------------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` ⚠️ NEW | `0x26f27C19F80596d228D853dC39A204f0f6C45C7E` |
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` | `0xb85424dd91D4ae0C6945e512bfDdF8a494299115` |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` | `0x1D31d9688a4ffD2aFE738BC6C9a4cb27C272AA5A` |
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` | `0xCF46BBa79eA69A68001A1c2f5Ad9eFA1AD435EF9` |
| HostEarnings | `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` | `0x8584AeAC9687613095D13EF7be4dE0A796F84D7a` |

**Note**: JobMarketplace has a NEW proxy address (clean slate deployment to remove deprecated storage slots). Other proxies unchanged (January 9, 2026).

### 7. ABI Changes

**ProofSystem ABI**: Function renamed
```json
// Before
{"name": "verifyEKZL", "type": "function", ...}

// After
{"name": "verifyHostSignature", "type": "function", ...}
```

**All Other ABIs**: No changes to function signatures.

### Migration Checklist

#### For SDK Developers

- [ ] Update Foundry/Forge if compiling locally (`foundryup`)
- [ ] If calling ProofSystem directly: rename `verifyEKZL` → `verifyHostSignature`
- [ ] Update ProofSystem ABI if cached locally
- [ ] No other code changes required

#### For Node Operators

- [ ] No action required for existing registrations
- [ ] Update local tooling if compiling contracts
- [ ] Verify your RPC provider supports EIP-1153 (all major providers do)

#### For Contract Deployers

- [ ] Deploy new implementations after upgrade
- [ ] Call `upgradeToAndCall()` on each proxy (owner only)
- [ ] Update implementation addresses in documentation
- [ ] Regenerate and distribute new ABIs

### Verification After Upgrade

```bash
# Verify Solidity version in deployed bytecode
cast code $PROXY_ADDRESS --rpc-url $RPC_URL | head -c 100

# Verify ProofSystem function exists
cast call $PROOF_SYSTEM "verifyHostSignature(bytes,address,uint256)" 0x... $ADDR 100 --rpc-url $RPC_URL

# Verify nonReentrant still works (any protected function)
cast call $JOB_MARKETPLACE "createSessionJob(address,uint256,uint256,uint256)" ... --rpc-url $RPC_URL
```

---

## December 14, 2025: Minimum Deposit Reduction + UUPS Migration

**Contracts Affected**: `JobMarketplaceWithModelsUpgradeable`
**Impact Level**: LOW - Non-breaking, reduces minimum requirements

### Summary

Minimum session deposits have been reduced from ~$0.80 to ~$0.50:

| Parameter | Old Value | New Value |
|-----------|-----------|-----------|
| `MIN_DEPOSIT` (ETH) | 0.0002 ETH (~$0.88) | 0.0001 ETH (~$0.50) |
| `USDC_MIN_DEPOSIT` | 800,000 ($0.80) | 500,000 ($0.50) |

### New Admin Function

A new function allows admins to update minimum deposits without contract redeployment:

```solidity
function updateTokenMinDeposit(address token, uint256 minDeposit) external
```

**Event emitted:**
```solidity
event TokenMinDepositUpdated(address indexed token, uint256 oldMinDeposit, uint256 newMinDeposit)
```

### Contract Addresses (UUPS Proxies) - Updated January 9, 2026

All contracts are now UUPS upgradeable:

| Contract | Proxy Address | Implementation | Notes |
|----------|---------------|----------------|-------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` | `0x26f27C19F80596d228D853dC39A204f0f6C45C7E` | ⚠️ NEW Jan 9, 2026 |
| NodeRegistry | `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` | `0x68298e2b74a106763aC99E3D973E98012dB5c75F` | |
| ModelRegistry | `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` | `0xd7Df5c6D4ffe6961d47753D1dd32f844e0F73f50` | |
| ProofSystem | `0x5afB91977e69Cc5003288849059bc62d47E7deeb` | `0x83eB050Aa3443a76a4De64aBeD90cA8d525E7A3A` | |
| HostEarnings | `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` | `0x588c42249F85C6ac4B4E27f97416C0289980aabB` | |

**Deprecated JobMarketplace** (do not use): `0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D`

### Migration Required

1. **Update contract addresses** to use proxy addresses (if using old non-upgradeable addresses)
2. **Update ABIs** to use `*Upgradeable-CLIENT-ABI.json` files
3. **No code changes needed** for minimum deposit reduction (it's backwards compatible)

---

## December 2025: PRICE_PRECISION Update

**Contracts Affected**: `NodeRegistryWithModels`, `JobMarketplaceWithModels`
**Impact Level**: HIGH - Requires SDK and Node operator updates

## Summary

Prices are now stored with a **1000x multiplier** (PRICE_PRECISION=1000) to support sub-$1/million token pricing for budget AI models.

**This is a breaking change** - all price values must be multiplied by 1000 to maintain the same USD value.

## Why This Change?

The previous minimum price of $10/million tokens was too high for budget models:

| Model | Actual Price/Million | Previous Min | New Min |
|-------|---------------------|--------------|---------|
| Llama 3.2 3B | $0.06/million | $10/million (too high) | $0.001/million |
| DeepSeek V3 | $0.27/million | $10/million (too high) | $0.001/million |
| Gemma 2 2B | $0.07/million | $10/million (too high) | $0.001/million |

## What Changed

### 1. New Constants

```solidity
// NodeRegistryWithModels.sol
uint256 public constant PRICE_PRECISION = 1000;

// Stable pricing (with 1000x precision):
uint256 public constant MIN_PRICE_PER_TOKEN_STABLE = 1;     // $0.001 per million tokens
uint256 public constant MAX_PRICE_PER_TOKEN_STABLE = 100_000_000; // $100,000 per million tokens

// Native pricing (with 1000x precision, calibrated for ~$4400 ETH):
uint256 public constant MIN_PRICE_PER_TOKEN_NATIVE = 227_273;       // ~$0.001 per million @ $4400 ETH
uint256 public constant MAX_PRICE_PER_TOKEN_NATIVE = 22_727_272_727_273_000; // ~$100,000 per million
```

### 2. Payment Calculation Changes

**Before (OLD):**
```javascript
maxTokens = deposit / pricePerToken
hostPayment = tokensUsed * pricePerToken
```

**After (NEW):**
```javascript
maxTokens = (deposit * PRICE_PRECISION) / pricePerToken
hostPayment = (tokensUsed * pricePerToken) / PRICE_PRECISION
```

## Migration Guide

### For SDK Developers

#### Price Conversion

| USD Price/Million | OLD pricePerToken | NEW pricePerToken |
|-------------------|-------------------|-------------------|
| $0.06/million | Not supported | 60 |
| $0.27/million | Not supported | 270 |
| $1/million | 1 | 1,000 |
| $5/million | 5 | 5,000 |
| $10/million | 10 | 10,000 |
| $50/million | 50 | 50,000 |

**Conversion formula:**
```javascript
// Convert USD/million to pricePerToken
const newPricePerToken = oldPricePerToken * 1000;

// Or from USD directly
const pricePerToken = usdPerMillion * 1000;
```

#### Example SDK Update

**Before (OLD):**
```javascript
const pricePerToken = 5; // $5/million tokens

const session = await marketplace.createSessionJobWithToken(
  host,
  usdcAddress,
  deposit,
  pricePerToken, // 5
  maxDuration,
  proofInterval
);
```

**After (NEW):**
```javascript
const PRICE_PRECISION = 1000;
const pricePerMillion = 5; // $5/million tokens
const pricePerToken = pricePerMillion * PRICE_PRECISION; // 5000

const session = await marketplace.createSessionJobWithToken(
  host,
  usdcAddress,
  deposit,
  pricePerToken, // 5000
  maxDuration,
  proofInterval
);
```

#### Calculating Max Tokens from Deposit

**Before (OLD):**
```javascript
const maxTokens = deposit / pricePerToken;
// Example: 10 USDC / 5 = 2,000,000 tokens
```

**After (NEW):**
```javascript
const PRICE_PRECISION = 1000;
const maxTokens = (deposit * PRICE_PRECISION) / pricePerToken;
// Example: (10 USDC * 1000) / 5000 = 2,000,000 tokens (same result!)
```

#### Calculating Cost from Token Usage

**Before (OLD):**
```javascript
const cost = tokensUsed * pricePerToken;
// Example: 1,000,000 tokens * 5 = 5,000,000 USDC units = $5
```

**After (NEW):**
```javascript
const PRICE_PRECISION = 1000;
const cost = (tokensUsed * pricePerToken) / PRICE_PRECISION;
// Example: (1,000,000 tokens * 5000) / 1000 = 5,000,000 USDC units = $5 (same result!)
```

### For Node Operators

#### Registering a Node

**Before (OLD):**
```javascript
// Register with $5/million stable pricing
await nodeRegistry.registerNode(
  metadata,
  apiUrl,
  modelIds,
  nativePrice,    // e.g., 3_000_000_000 wei
  5               // $5/million stable price
);
```

**After (NEW):**
```javascript
// Register with $5/million stable pricing (multiply by 1000)
await nodeRegistry.registerNode(
  metadata,
  apiUrl,
  modelIds,
  3_000_000,      // Native price (also 1000x smaller for same USD value)
  5000            // $5/million stable price (5 * 1000)
);
```

#### Updating Pricing

**Before (OLD):**
```javascript
await nodeRegistry.updatePricingStable(10); // $10/million
```

**After (NEW):**
```javascript
await nodeRegistry.updatePricingStable(10000); // $10/million (10 * 1000)
```

#### Setting Model-Specific Pricing

**Before (OLD):**
```javascript
await nodeRegistry.setModelPricing(
  modelId,
  nativePrice,
  15  // $15/million for this model
);
```

**After (NEW):**
```javascript
await nodeRegistry.setModelPricing(
  modelId,
  nativePrice,
  15000  // $15/million for this model (15 * 1000)
);
```

### Native Token (ETH/BNB) Pricing

Native pricing also uses PRICE_PRECISION. The values are calibrated for ~$4400 ETH:

| USD Price/Million | NEW Native pricePerToken |
|-------------------|--------------------------|
| $0.001/million | 227,273 (MIN) |
| $0.013/million | 3,000,000 |
| $0.1/million | 22,727,273 |
| $1/million | 227,272,727 |

**Formula:**
```javascript
// Convert USD/million to native pricePerToken (for $4400 ETH)
const nativePricePerToken = (usdPerMillion / 4400) * 1e18 / 1e6 * 1000;
```

## Query Functions

The following query functions return prices in the NEW format (with PRICE_PRECISION):

```javascript
// Get host's base pricing
const [nativePrice, stablePrice] = await nodeRegistry.getNodePricing(hostAddress);
// Returns: [3000000, 5000] for ~$0.013/million native and $5/million stable

// Get model-specific pricing
const modelPrice = await nodeRegistry.getModelPricing(hostAddress, modelId, tokenAddress);
// Returns price with PRICE_PRECISION included

// Get full node info
const [operator, stake, active, metadata, apiUrl, models, nativePrice, stablePrice] =
  await nodeRegistry.getNodeFullInfo(hostAddress);
```

## Checklist for Migration

### SDK Updates

- [ ] Update all `pricePerToken` values by multiplying by 1000
- [ ] Update maxTokens calculation: `(deposit * 1000) / pricePerToken`
- [ ] Update cost calculation: `(tokensUsed * pricePerToken) / 1000`
- [ ] Update price display: divide by 1000 before showing USD
- [ ] Add PRICE_PRECISION constant to SDK config

### Node Operator Updates

- [ ] Re-register nodes with updated price values (×1000)
- [ ] Update any pricing automation scripts
- [ ] Test session creation with new prices

### Testing

- [ ] Verify session creation works with new prices
- [ ] Verify payment calculations are correct
- [ ] Verify price display in UI matches expected USD values

## Contract Addresses

> **Note:** As of December 14, 2025, all contracts have been migrated to UUPS upgradeable pattern. See the December 14, 2025 section above for current proxy addresses.

### Legacy (Non-Upgradeable) - DEPRECATED

| Contract | Address (Base Sepolia) | Status |
|----------|------------------------|--------|
| NodeRegistryWithModels | `0x906F4A8Cb944E4fe12Fb85Be7E627CeDAA8B8999` | ⚠️ DEPRECATED Dec 14, 2025 |
| JobMarketplaceWithModels | `0x75C72e8C3eC707D8beF5Ba9b9C4f75CbB5bced97` | ⚠️ DEPRECATED Dec 14, 2025 |
| JobMarketplaceWithModels (200 tok/sec) | `0x5497a28B4bE6b1219F93e6Fcd631E8aCdC173709` | ⚠️ DEPRECATED Dec 10, 2025 |
| JobMarketplaceWithModels (20 tok/sec) | `0xfD764804C5A5808b79D66746BAF4B65fb4413731` | ⚠️ DEPRECATED Dec 10, 2025 |

## Questions?

If you have questions about this migration, please contact the Fabstir team or open an issue at https://github.com/fabstirp2p/contracts/issues.
