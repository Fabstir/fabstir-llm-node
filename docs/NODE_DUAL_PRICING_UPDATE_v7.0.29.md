# Fabstir LLM Node v7.0.29 - Dual Pricing System Update

**Version**: v7.0.29-dual-pricing-support-2025-01-28
**Status**: PRODUCTION READY
**Breaking Change**: YES - Requires SDK coordination

## Executive Summary

Fabstir LLM Node v7.0.29 implements the corrected dual pricing system that separates pricing for native tokens (ETH/BNB) and stablecoins (USDC). This update is synchronized with the contract deployment from January 28, 2025 and requires SDK updates for compatibility.

### Key Changes

- ✅ Updated contract addresses (NodeRegistry, JobMarketplace)
- ✅ Dual pricing support in host registration
- ✅ Separate pricing for native and stable tokens
- ✅ 10,000x price range for both token types
- ✅ Automatic pricing validation
- ✅ Default pricing fallbacks

## Breaking Changes

### 1. New Contract Addresses

**CRITICAL**: The node now uses different contract addresses than v7.0.28.

| Contract | Old Address (v7.0.28) | New Address (v7.0.29) |
|----------|----------------------|----------------------|
| NodeRegistryWithModels | `0xC8dDD546e0993eEB4Df03591208aEDF6336342D7` | `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6` |
| JobMarketplaceWithModels | `0x462050a4a551c4292586D9c1DE23e3158a9bF3B3` | `0xe169A4B57700080725f9553E3Cc69885fea13629` |

**SDK Action Required**: Update your contract addresses to match the node's new addresses.

### 2. Host Registration Changes

**Old Registration (v7.0.28)**:
```rust
registerNode(metadata, apiUrl, modelIds)
// 3 parameters
```

**New Registration (v7.0.29)**:
```rust
registerNode(metadata, apiUrl, modelIds, minPricePerTokenNative, minPricePerTokenStable)
// 5 parameters - added dual pricing
```

### 3. Node Info Response Changes

**Old Response (7 fields)**:
```javascript
[operator, stake, active, metadata, apiUrl, models, minPricePerToken]
```

**New Response (8 fields)**:
```javascript
[operator, stake, active, metadata, apiUrl, models, minPricePerTokenNative, minPricePerTokenStable]
```

### 4. Pricing Query Changes

**Old**:
```javascript
getNodePricing(operator) // Returns single U256
```

**New**:
```javascript
getNodePricing(operator, token) // Requires token address parameter
// Returns: minPricePerTokenNative if token == 0x0
// Returns: minPricePerTokenStable if token == USDC address
```

## Pricing Ranges

The node validates all pricing against these contract-enforced ranges:

### Native Token (ETH/BNB) Pricing

```rust
MIN_PRICE_PER_TOKEN_NATIVE: 2,272,727,273 wei
MAX_PRICE_PER_TOKEN_NATIVE: 22,727,272,727,273 wei
RANGE_MULTIPLIER: 10,000x
```

**USD Equivalent** (@ $4,400 ETH):
- Minimum: ~$0.00001 per token
- Maximum: ~$0.1 per token

**Default Value**: `11,363,636,363,636 wei` (~$0.00005 @ $4,400 ETH)

### Stablecoin (USDC) Pricing

```rust
MIN_PRICE_PER_TOKEN_STABLE: 10
MAX_PRICE_PER_TOKEN_STABLE: 100,000
RANGE_MULTIPLIER: 10,000x
```

**USD Equivalent** (USDC has 6 decimals):
- Minimum: 0.00001 USDC per token
- Maximum: 0.1 USDC per token

**Default Value**: `316` (~0.000316 USDC per token)

## SDK Integration Guide

### 1. Discovering Host Pricing

When querying a host's pricing information, you need to specify which token type:

```javascript
import { ethers } from 'ethers';

const nodeRegistry = new ethers.Contract(
  '0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6', // NEW ADDRESS
  NodeRegistryABI,
  provider
);

// Query native token pricing (ETH/BNB)
const NATIVE_TOKEN = ethers.constants.AddressZero; // 0x0000...0000
const nativePricing = await nodeRegistry.getNodePricing(
  hostAddress,
  NATIVE_TOKEN
);

// Query stablecoin pricing (USDC)
const USDC_ADDRESS = '0x036CbD53842c5426634e7929541eC2318f3dCF7e';
const stablePricing = await nodeRegistry.getNodePricing(
  hostAddress,
  USDC_ADDRESS
);

console.log('Host Pricing:');
console.log(`  Native (ETH): ${nativePricing.toString()} wei`);
console.log(`  Stable (USDC): ${stablePricing.toString()} (raw value)`);
```

### 2. Getting Complete Host Information

```javascript
// getNodeFullInfo now returns 8 fields instead of 7
const [
  operator,           // address
  stakedAmount,       // uint256
  active,             // bool
  metadata,           // string (JSON)
  apiUrl,             // string
  supportedModels,    // bytes32[]
  minPriceNative,     // uint256 - NEW
  minPriceStable      // uint256 - NEW
] = await nodeRegistry.getNodeFullInfo(hostAddress);

console.log('Host Info:');
console.log(`  Operator: ${operator}`);
console.log(`  Active: ${active}`);
console.log(`  API URL: ${apiUrl}`);
console.log(`  Native Pricing: ${minPriceNative.toString()} wei`);
console.log(`  Stable Pricing: ${minPriceStable.toString()}`);
```

### 3. Displaying Pricing to Users

```javascript
function formatHostPricing(nativePrice, stablePrice, ethPriceUSD = 4400) {
  // Format native pricing
  const nativeETH = ethers.utils.formatEther(nativePrice);
  const nativeUSD = parseFloat(nativeETH) * ethPriceUSD;

  // Format stable pricing (USDC has 6 decimals)
  const stableUSDC = stablePrice / 1_000_000;

  return {
    native: {
      wei: nativePrice.toString(),
      eth: `${nativeETH} ETH`,
      usd: `$${nativeUSD.toFixed(6)}`,
      per1000Tokens: `$${(nativeUSD * 1000).toFixed(4)} per 1000 tokens`
    },
    stable: {
      raw: stablePrice,
      usdc: `${stableUSDC.toFixed(6)} USDC`,
      per1000Tokens: `$${(stableUSDC * 1000).toFixed(4)} per 1000 tokens`
    }
  };
}

// Usage
const [nativePrice, stablePrice] = await Promise.all([
  nodeRegistry.getNodePricing(hostAddress, ethers.constants.AddressZero),
  nodeRegistry.getNodePricing(hostAddress, '0x036CbD53842c5426634e7929541eC2318f3dCF7e')
]);

const pricing = formatHostPricing(nativePrice, stablePrice);
console.log('ETH Session:', pricing.native.per1000Tokens);
console.log('USDC Session:', pricing.stable.per1000Tokens);
```

### 4. Creating Sessions with Price Validation

The node will reject sessions if client pricing is below the host's minimums. Your SDK should validate before submission:

```javascript
// For ETH sessions
async function createETHSession(hostAddress, clientPricePerToken, depositAmount) {
  // STEP 1: Get host's native minimum
  const hostMinNative = await nodeRegistry.getNodePricing(
    hostAddress,
    ethers.constants.AddressZero
  );

  // STEP 2: Validate client price
  if (clientPricePerToken.lt(hostMinNative)) {
    throw new Error(
      `Your ETH session price (${clientPricePerToken.toString()}) is below ` +
      `the host's minimum (${hostMinNative.toString()} wei). ` +
      `Please increase your price.`
    );
  }

  // STEP 3: Create session
  const marketplace = new ethers.Contract(
    '0xe169A4B57700080725f9553E3Cc69885fea13629', // NEW ADDRESS
    JobMarketplaceABI,
    signer
  );

  const tx = await marketplace.createSessionJob(
    hostAddress,
    clientPricePerToken,
    3600,  // maxDuration
    100,   // proofInterval
    { value: depositAmount }
  );

  return await tx.wait();
}

// For USDC sessions
async function createUSDCSession(hostAddress, clientPricePerToken, depositAmount) {
  const USDC_ADDRESS = '0x036CbD53842c5426634e7929541eC2318f3dCF7e';

  // STEP 1: Get host's stable minimum
  const hostMinStable = await nodeRegistry.getNodePricing(
    hostAddress,
    USDC_ADDRESS
  );

  // STEP 2: Validate client price
  if (clientPricePerToken < hostMinStable) {
    throw new Error(
      `Your USDC session price (${clientPricePerToken}) is below ` +
      `the host's minimum (${hostMinStable}). ` +
      `Please increase your price.`
    );
  }

  // STEP 3: Approve USDC
  const usdc = new ethers.Contract(
    USDC_ADDRESS,
    ['function approve(address,uint256)'],
    signer
  );
  await (await usdc.approve(marketplaceAddress, depositAmount)).wait();

  // STEP 4: Create session
  const marketplace = new ethers.Contract(
    '0xe169A4B57700080725f9553E3Cc69885fea13629',
    JobMarketplaceABI,
    signer
  );

  const tx = await marketplace.createSessionJobWithToken(
    hostAddress,
    USDC_ADDRESS,
    depositAmount,
    clientPricePerToken,
    3600,  // maxDuration
    100    // proofInterval
  );

  return await tx.wait();
}
```

### 5. Handling Host Discovery

When listing available hosts, fetch both pricing values:

```javascript
async function discoverHosts(modelId) {
  const hosts = await nodeRegistry.getNodesForModel(modelId);

  const hostDetails = await Promise.all(
    hosts.map(async (hostAddress) => {
      const [
        operator,
        stakedAmount,
        active,
        metadata,
        apiUrl,
        supportedModels,
        minPriceNative,
        minPriceStable
      ] = await nodeRegistry.getNodeFullInfo(hostAddress);

      return {
        address: hostAddress,
        operator,
        active,
        apiUrl,
        metadata: JSON.parse(metadata),
        pricing: {
          native: minPriceNative,
          stable: minPriceStable
        },
        supportedModels
      };
    })
  );

  // Filter active hosts and sort by price
  return hostDetails
    .filter(host => host.active)
    .sort((a, b) => {
      // Sort by native pricing (you could also sort by stable)
      return a.pricing.native.sub(b.pricing.native);
    });
}
```

## Node Configuration

### Environment Variables

The node reads pricing from the host's configuration. If not specified, it uses defaults:

```bash
# These are set during node deployment
# The node uses default pricing if not configured
```

### Default Pricing Values

If a host doesn't specify pricing, the node uses these defaults:

```rust
// Native token default (~$0.00005 @ $4400 ETH)
DEFAULT_NATIVE_PRICE = 11_363_636_363_636 wei

// Stablecoin default (~$0.000316 per token)
DEFAULT_STABLE_PRICE = 316
```

### Pricing Validation

The node **automatically validates** all pricing values against contract ranges:

```rust
// Native validation
if price_native < 2_272_727_273 || price_native > 22_727_272_727_273 {
  return Error("Native pricing out of range");
}

// Stable validation
if price_stable < 10 || price_stable > 100_000 {
  return Error("Stable pricing out of range");
}
```

## API Changes

### WebSocket API

No changes to WebSocket protocol. Pricing is handled at the contract level.

### HTTP API

No changes to HTTP endpoints. Pricing is contract-based.

### Health Endpoint

The `/health` endpoint now reports the new version:

```json
{
  "status": "healthy",
  "version": "7.0.29",
  "build": "v7.0.29-dual-pricing-support-2025-01-28",
  "features": [
    "multi-chain",
    "dual-pricing",
    "native-stable-pricing"
  ],
  "contracts": {
    "nodeRegistry": "0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6",
    "jobMarketplace": "0xe169A4B57700080725f9553E3Cc69885fea13629"
  }
}
```

## Migration Checklist for SDK

### Required Changes

- [ ] Update NodeRegistry address to `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6`
- [ ] Update JobMarketplace address to `0xe169A4B57700080725f9553E3Cc69885fea13629`
- [ ] Update NodeRegistryWithModels ABI (from docs/compute-contracts-reference/client-abis/)
- [ ] Update JobMarketplaceWithModels ABI (from docs/compute-contracts-reference/client-abis/)
- [ ] Update `getNodePricing()` calls to include token address parameter
- [ ] Update `getNodeFullInfo()` parsing to handle 8 fields instead of 7
- [ ] Add dual pricing display in UI (show both ETH and USDC pricing)
- [ ] Implement price validation before session creation
- [ ] Update error handling for new pricing validation errors

### Optional Enhancements

- [ ] Add price comparison feature (ETH vs USDC sessions)
- [ ] Show USD-equivalent pricing to users
- [ ] Implement automatic pricing recommendations
- [ ] Cache pricing data with TTL (1-5 minutes)
- [ ] Monitor for `PricingUpdatedNative` and `PricingUpdatedStable` events

### Testing Checklist

- [ ] Test querying native pricing from registered hosts
- [ ] Test querying stable pricing from registered hosts
- [ ] Test creating ETH sessions with valid pricing
- [ ] Test creating USDC sessions with valid pricing
- [ ] Test rejection of sessions with pricing below host minimum
- [ ] Test parsing of 8-field `getNodeFullInfo()` response
- [ ] Verify USD-equivalent calculations are accurate
- [ ] Test with multiple hosts with different pricing

## Common Issues & Solutions

### Issue 1: "Contract function not found"

**Cause**: Using old ABI that doesn't have dual pricing functions.

**Solution**: Update to latest ABIs from `docs/compute-contracts-reference/client-abis/`:
- `NodeRegistryWithModels-CLIENT-ABI.json`
- `JobMarketplaceWithModels-CLIENT-ABI.json`

### Issue 2: "Transaction reverted: Price below host minimum (native)"

**Cause**: Client offered ETH session price below host's `minPricePerTokenNative`.

**Solution**: Query host pricing first and ensure client price >= host minimum:
```javascript
const hostMin = await nodeRegistry.getNodePricing(host, ethers.constants.AddressZero);
const clientPrice = hostMin.add(ethers.BigNumber.from('1000000000')); // Add buffer
```

### Issue 3: "Transaction reverted: Price below host minimum (stable)"

**Cause**: Client offered USDC session price below host's `minPricePerTokenStable`.

**Solution**: Query stable pricing and add buffer:
```javascript
const USDC = '0x036CbD53842c5426634e7929541eC2318f3dCF7e';
const hostMin = await nodeRegistry.getNodePricing(host, USDC);
const clientPrice = hostMin + 100; // Add buffer
```

### Issue 4: "Wrong number of return values from getNodeFullInfo"

**Cause**: Code expects 7 fields but contract returns 8.

**Solution**: Update destructuring to handle 8 fields:
```javascript
// ❌ Wrong (7 fields)
const [op, stake, active, meta, api, models, price] = await getNodeFullInfo(host);

// ✅ Correct (8 fields)
const [op, stake, active, meta, api, models, priceNative, priceStable] =
  await getNodeFullInfo(host);
```

### Issue 5: "getNodePricing requires token parameter"

**Cause**: Calling old single-parameter version.

**Solution**: Add token address:
```javascript
// ❌ Wrong (old version)
const price = await nodeRegistry.getNodePricing(hostAddress);

// ✅ Correct (new version)
const nativePrice = await nodeRegistry.getNodePricing(
  hostAddress,
  ethers.constants.AddressZero  // For native token
);
```

## Backwards Compatibility

### ⚠️ NO BACKWARDS COMPATIBILITY

This is a **BREAKING CHANGE**. The node will NOT work with:
- Old contract addresses (0xC8dDD546e0993eEB4Df03591208aEDF6336342D7, 0x462050a4a551c4292586D9c1DE23e3158a9bF3B3)
- Old contract ABIs (single pricing field)
- Old SDK versions that don't support dual pricing

### Upgrade Path

1. **Update SDK** to support dual pricing (this guide)
2. **Deploy new SDK version** with updated contract addresses
3. **Update node** to v7.0.29 (already done)
4. **Test end-to-end** session creation with both ETH and USDC
5. **Monitor** for any pricing validation errors

## Reference Documentation

- **Contract Addresses**: See `docs/compute-contracts-reference/client-abis/README.md`
- **SDK Dual Pricing Guide**: See `docs/compute-contracts-reference/SDK_DUAL_PRICING_INTEGRATION.md`
- **Node Version Info**: See `src/version.rs`
- **Pricing Constants**: See `src/contracts/pricing_constants.rs`

## Support & Questions

For technical questions about this update:

1. **Contract Details**: Read `docs/compute-contracts-reference/NodeRegistry.md` and `JobMarketplace.md`
2. **ABI Files**: Check `docs/compute-contracts-reference/client-abis/`
3. **Code Examples**: See SDK integration guide above
4. **Issues**: Report bugs via GitHub issues

## Version History

| Version | Date | Changes |
|---------|------|---------|
| v7.0.29 | 2025-01-28 | Dual pricing system implementation |
| v7.0.28 | 2025-10-07 | Contract ABI update (deprecated) |
| v7.0.27 | 2025-10-05 | Checkpoint overcharge fix |

---

**Last Updated**: January 28, 2025
**Node Version**: v7.0.29-dual-pricing-support-2025-01-28
**Contract Deployment**: January 28, 2025
