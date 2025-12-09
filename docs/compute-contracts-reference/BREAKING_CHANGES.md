# Breaking Changes: PRICE_PRECISION Update

**Date**: December 2025
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

The following contracts have been updated with PRICE_PRECISION:

| Contract | Address (Base Sepolia) | Status |
|----------|------------------------|--------|
| NodeRegistryWithModels | TBD | Pending deployment |
| JobMarketplaceWithModels | TBD | Pending deployment |

## Questions?

If you have questions about this migration, please contact the Fabstir team or open an issue at https://github.com/fabstirp2p/contracts/issues.
