# SDK Integration Guide: Dual Pricing System

**Version**: 2.0
**Last Updated**: January 28, 2025
**Status**: PRODUCTION READY

## Overview

This guide helps SDK developers integrate the new **corrected dual pricing system** into their applications. The system separates pricing for native tokens (ETH/BNB) and stablecoins (USDC) with proper 10,000x range validation.

## What Changed

### Previous System (DEPRECATED)
- **Single pricing field**: `minPricePerToken` used for both ETH and USDC
- **Problem**: Single constant couldn't account for different decimal places (ETH: 18 decimals, USDC: 6 decimals)
- **Incorrect range**: MAX_PRICE_NATIVE was only 10x MIN instead of 10,000x
- **Deprecated Contracts**:
  - JobMarketplaceWithModels: `0x462050a4a551c4292586D9c1DE23e3158a9bF3B3`
  - NodeRegistryWithModels: `0xC8dDD546e0993eEB4Df03591208aEDF6336342D7`

### New System (CURRENT)
- **Dual pricing fields**: Separate `minPricePerTokenNative` and `minPricePerTokenStable`
- **Proper ranges**: Both have 10,000x range (MIN to MAX)
- **Correct validation**: Contract validates against the appropriate field based on payment type
- **Active Contracts**:
  - JobMarketplaceWithModels: `0xe169A4B57700080725f9553E3Cc69885fea13629`
  - NodeRegistryWithModels: `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6`

## Breaking Changes

### 1. Contract Addresses
**Action Required**: Update all contract addresses in your SDK configuration.

```javascript
// OLD (DEPRECATED)
const OLD_NODE_REGISTRY = '0xC8dDD546e0993eEB4Df03591208aEDF6336342D7';
const OLD_JOB_MARKETPLACE = '0x462050a4a551c4292586D9c1DE23e3158a9bF3B3';

// NEW (CURRENT)
const NODE_REGISTRY = '0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6';
const JOB_MARKETPLACE = '0xe169A4B57700080725f9553E3Cc69885fea13629';
```

### 2. Node Struct Fields
**Action Required**: Update struct parsing to handle 8 fields instead of 7.

```javascript
// OLD (7 fields)
const [operator, stake, active, metadata, apiUrl, models, minPrice] =
  await nodeRegistry.getNodeFullInfo(hostAddress);

// NEW (8 fields - DUAL PRICING)
const [operator, stake, active, metadata, apiUrl, models, minPriceNative, minPriceStable] =
  await nodeRegistry.getNodeFullInfo(hostAddress);
```

### 3. Pricing Query Response
**Action Required**: `getNodePricing()` now returns TWO values, not one.

```javascript
// OLD (single value)
const hostMinPrice = await nodeRegistry.getNodePricing(hostAddress);

// NEW (dual values - returns tuple)
const [hostMinPriceNative, hostMinPriceStable] = await nodeRegistry.getNodePricing(hostAddress);
```

### 4. Host Registration
**Action Required**: Hosts must provide BOTH pricing values during registration.

```javascript
// OLD (single pricing)
await nodeRegistry.registerNode(
  metadata,
  apiUrl,
  [modelId],
  minPricePerToken  // Single value
);

// NEW (dual pricing)
await nodeRegistry.registerNode(
  metadata,
  apiUrl,
  [modelId],
  minPricePerTokenNative,   // For ETH/BNB sessions
  minPricePerTokenStable    // For USDC sessions
);
```

### 5. Pricing Update Functions
**Action Required**: Use separate functions for native vs stable pricing updates.

```javascript
// OLD (single update function)
await nodeRegistry.updatePricing(newPrice);

// NEW (separate functions)
await nodeRegistry.updatePricingNative(newNativePrice);
await nodeRegistry.updatePricingStable(newStablePrice);
```

## Pricing Ranges

### Native Token Pricing (ETH/BNB)

```javascript
const NATIVE_PRICING = {
  MIN: 2_272_727_273,           // wei (~$0.00001 @ $4400 ETH)
  MAX: 22_727_272_727_273,      // wei (~$0.1 @ $4400 ETH)
  DECIMALS: 18,
  RANGE_MULTIPLIER: 10_000      // 10,000x range
};

// Example validation
function validateNativePrice(price) {
  if (price < NATIVE_PRICING.MIN || price > NATIVE_PRICING.MAX) {
    throw new Error(
      `Native price must be between ${NATIVE_PRICING.MIN} and ${NATIVE_PRICING.MAX} wei`
    );
  }
  return true;
}
```

### Stablecoin Pricing (USDC)

```javascript
const STABLE_PRICING = {
  MIN: 10,                      // 0.00001 USDC per token
  MAX: 100_000,                 // 0.1 USDC per token
  DECIMALS: 6,
  RANGE_MULTIPLIER: 10_000      // 10,000x range
};

// Example validation
function validateStablePrice(price) {
  if (price < STABLE_PRICING.MIN || price > STABLE_PRICING.MAX) {
    throw new Error(
      `Stable price must be between ${STABLE_PRICING.MIN} and ${STABLE_PRICING.MAX}`
    );
  }
  return true;
}
```

### Helper Functions

```javascript
import { ethers } from 'ethers';

// Convert USD target to native token price (wei)
function usdToNativePrice(usdAmount, ethPriceUSD = 4400) {
  const ethAmount = usdAmount / ethPriceUSD;
  return ethers.utils.parseEther(ethAmount.toString());
}

// Convert USD target to stable price (USDC with 6 decimals)
function usdToStablePrice(usdAmount) {
  return Math.floor(usdAmount * 1_000_000);
}

// Format native price for display
function formatNativePrice(weiPrice, ethPriceUSD = 4400) {
  const ethAmount = parseFloat(ethers.utils.formatEther(weiPrice));
  const usdAmount = ethAmount * ethPriceUSD;
  return {
    wei: weiPrice.toString(),
    eth: ethAmount.toFixed(18),
    usd: usdAmount.toFixed(6)
  };
}

// Format stable price for display
function formatStablePrice(stablePrice) {
  const usdAmount = stablePrice / 1_000_000;
  return {
    raw: stablePrice,
    usdc: usdAmount.toFixed(6)
  };
}
```

## Migration Guide

### Step 1: Update Contract Addresses

```javascript
// config.js or constants.js
export const CONTRACTS = {
  // Base Sepolia (Testnet)
  baseSepolia: {
    chainId: 84532,
    nodeRegistry: '0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6',
    jobMarketplace: '0xe169A4B57700080725f9553E3Cc69885fea13629',
    modelRegistry: '0x92b2De840bB2171203011A6dBA928d855cA8183E',
    proofSystem: '0x2ACcc60893872A499700908889B38C5420CBcFD1',
    hostEarnings: '0x908962e8c6CE72610021586f85ebDE09aAc97776',
    usdcToken: '0x036CbD53842c5426634e7929541eC2318f3dCF7e',
    fabToken: '0xC78949004B4EB6dEf2D66e49Cd81231472612D62'
  }
};
```

### Step 2: Update ABI Imports

```javascript
// Import updated ABIs
import NodeRegistryABI from '@fabstir/contracts/client-abis/NodeRegistryWithModels-CLIENT-ABI.json';
import JobMarketplaceABI from '@fabstir/contracts/client-abis/JobMarketplaceWithModels-CLIENT-ABI.json';
```

### Step 3: Update Host Registration Logic

```javascript
// sdk/host.js
export class HostManager {
  constructor(nodeRegistry, signer) {
    this.nodeRegistry = nodeRegistry.connect(signer);
  }

  /**
   * Register as a host with dual pricing
   * @param {Object} params - Registration parameters
   * @param {string} params.metadata - JSON metadata string
   * @param {string} params.apiUrl - Host API endpoint
   * @param {string[]} params.modelIds - Array of approved model IDs
   * @param {BigNumber} params.minPriceNative - Min price for ETH/BNB (wei)
   * @param {number} params.minPriceStable - Min price for USDC (raw)
   */
  async registerHost({
    metadata,
    apiUrl,
    modelIds,
    minPriceNative,
    minPriceStable
  }) {
    // Validate pricing ranges
    if (minPriceNative.lt(NATIVE_PRICING.MIN) || minPriceNative.gt(NATIVE_PRICING.MAX)) {
      throw new Error('Native price out of valid range');
    }
    if (minPriceStable < STABLE_PRICING.MIN || minPriceStable > STABLE_PRICING.MAX) {
      throw new Error('Stable price out of valid range');
    }

    const tx = await this.nodeRegistry.registerNode(
      metadata,
      apiUrl,
      modelIds,
      minPriceNative,
      minPriceStable
    );

    const receipt = await tx.wait();
    console.log('Host registered with dual pricing:', receipt.transactionHash);
    return receipt;
  }

  /**
   * Update native token pricing (ETH/BNB)
   * @param {BigNumber} newPrice - New price in wei
   */
  async updateNativePricing(newPrice) {
    if (newPrice.lt(NATIVE_PRICING.MIN) || newPrice.gt(NATIVE_PRICING.MAX)) {
      throw new Error('Native price out of valid range');
    }

    const tx = await this.nodeRegistry.updatePricingNative(newPrice);
    await tx.wait();
    console.log('Native pricing updated to:', newPrice.toString());
  }

  /**
   * Update stablecoin pricing (USDC)
   * @param {number} newPrice - New price (raw USDC value)
   */
  async updateStablePricing(newPrice) {
    if (newPrice < STABLE_PRICING.MIN || newPrice > STABLE_PRICING.MAX) {
      throw new Error('Stable price out of valid range');
    }

    const tx = await this.nodeRegistry.updatePricingStable(newPrice);
    await tx.wait();
    console.log('Stable pricing updated to:', newPrice);
  }

  /**
   * Get current dual pricing for host
   * @param {string} hostAddress - Host address
   * @returns {Object} Dual pricing info
   */
  async getHostPricing(hostAddress) {
    const [nativePrice, stablePrice] = await this.nodeRegistry.getNodePricing(hostAddress);

    return {
      native: {
        wei: nativePrice.toString(),
        formatted: formatNativePrice(nativePrice)
      },
      stable: {
        raw: stablePrice,
        formatted: formatStablePrice(stablePrice)
      }
    };
  }
}
```

### Step 4: Update Client Session Creation Logic

```javascript
// sdk/client.js
export class ClientManager {
  constructor(jobMarketplace, nodeRegistry, provider, signer) {
    this.marketplace = jobMarketplace.connect(signer);
    this.nodeRegistry = nodeRegistry;
    this.provider = provider;
  }

  /**
   * Create ETH session with native pricing validation
   * @param {Object} params - Session parameters
   */
  async createETHSession({
    hostAddress,
    pricePerToken,      // BigNumber in wei
    maxDuration,
    proofInterval,
    depositAmount       // BigNumber in wei
  }) {
    // STEP 1: Query host's native minimum
    const [hostMinNative, _] = await this.nodeRegistry.getNodePricing(hostAddress);

    // STEP 2: Validate client price >= host minimum
    if (pricePerToken.lt(hostMinNative)) {
      throw new Error(
        `Price too low. Host minimum: ${hostMinNative.toString()} wei, ` +
        `Your offer: ${pricePerToken.toString()} wei`
      );
    }

    // STEP 3: Create session
    const tx = await this.marketplace.createSessionJob(
      hostAddress,
      pricePerToken,
      maxDuration,
      proofInterval,
      { value: depositAmount }
    );

    const receipt = await tx.wait();

    // Parse JobPosted event to get session ID
    const event = receipt.events?.find(e => e.event === 'JobPosted');
    const sessionId = event?.args?.sessionId;

    console.log('ETH session created:', sessionId?.toString());
    return { sessionId, receipt };
  }

  /**
   * Create USDC session with stable pricing validation
   * @param {Object} params - Session parameters
   */
  async createUSDCSession({
    hostAddress,
    pricePerToken,      // number (raw USDC value)
    maxDuration,
    proofInterval,
    depositAmount       // BigNumber with 6 decimals
  }) {
    // STEP 1: Query host's stable minimum
    const [_, hostMinStable] = await this.nodeRegistry.getNodePricing(hostAddress);

    // STEP 2: Validate client price >= host minimum
    if (pricePerToken < hostMinStable) {
      throw new Error(
        `Price too low. Host minimum: ${hostMinStable} (${hostMinStable / 1e6} USDC), ` +
        `Your offer: ${pricePerToken} (${pricePerToken / 1e6} USDC)`
      );
    }

    // STEP 3: Approve USDC
    const usdcAddress = '0x036CbD53842c5426634e7929541eC2318f3dCF7e';
    const usdcContract = new ethers.Contract(
      usdcAddress,
      ['function approve(address,uint256) returns (bool)'],
      this.marketplace.signer
    );

    const approveTx = await usdcContract.approve(
      this.marketplace.address,
      depositAmount
    );
    await approveTx.wait();

    // STEP 4: Create session
    const tx = await this.marketplace.createSessionJobWithToken(
      hostAddress,
      usdcAddress,
      depositAmount,
      pricePerToken,
      maxDuration,
      proofInterval
    );

    const receipt = await tx.wait();
    const event = receipt.events?.find(e => e.event === 'JobPosted');
    const sessionId = event?.args?.sessionId;

    console.log('USDC session created:', sessionId?.toString());
    return { sessionId, receipt };
  }

  /**
   * Get host info including dual pricing
   * @param {string} hostAddress - Host address
   * @returns {Object} Complete host information
   */
  async getHostInfo(hostAddress) {
    const [
      operator,
      stakedAmount,
      active,
      metadata,
      apiUrl,
      supportedModels,
      minPriceNative,
      minPriceStable
    ] = await this.nodeRegistry.getNodeFullInfo(hostAddress);

    return {
      operator,
      stakedAmount: stakedAmount.toString(),
      active,
      metadata: JSON.parse(metadata),
      apiUrl,
      supportedModels,
      pricing: {
        native: {
          wei: minPriceNative.toString(),
          formatted: formatNativePrice(minPriceNative)
        },
        stable: {
          raw: minPriceStable,
          formatted: formatStablePrice(minPriceStable)
        }
      }
    };
  }
}
```

### Step 5: Update Error Handling

```javascript
// sdk/errors.js
export class SDKError extends Error {
  constructor(message, code, details) {
    super(message);
    this.code = code;
    this.details = details;
  }
}

export function handleContractError(error) {
  const message = error.message || error.toString();

  // Dual pricing validation errors
  if (message.includes('Price below host minimum (native)')) {
    throw new SDKError(
      'Your ETH session price is below the host\'s minimum native price',
      'PRICE_TOO_LOW_NATIVE',
      { type: 'native', error }
    );
  }

  if (message.includes('Price below host minimum (stable)')) {
    throw new SDKError(
      'Your USDC session price is below the host\'s minimum stable price',
      'PRICE_TOO_LOW_STABLE',
      { type: 'stable', error }
    );
  }

  if (message.includes('Native price below minimum')) {
    throw new SDKError(
      `Native price must be >= ${NATIVE_PRICING.MIN} wei`,
      'PRICE_BELOW_CONTRACT_MIN_NATIVE',
      { min: NATIVE_PRICING.MIN, error }
    );
  }

  if (message.includes('Native price above maximum')) {
    throw new SDKError(
      `Native price must be <= ${NATIVE_PRICING.MAX} wei`,
      'PRICE_ABOVE_CONTRACT_MAX_NATIVE',
      { max: NATIVE_PRICING.MAX, error }
    );
  }

  if (message.includes('Stable price below minimum')) {
    throw new SDKError(
      `Stable price must be >= ${STABLE_PRICING.MIN}`,
      'PRICE_BELOW_CONTRACT_MIN_STABLE',
      { min: STABLE_PRICING.MIN, error }
    );
  }

  if (message.includes('Stable price above maximum')) {
    throw new SDKError(
      `Stable price must be <= ${STABLE_PRICING.MAX}`,
      'PRICE_ABOVE_CONTRACT_MAX_STABLE',
      { max: STABLE_PRICING.MAX, error }
    );
  }

  // Default
  throw new SDKError('Contract error', 'CONTRACT_ERROR', { error });
}
```

## Testing Your Integration

### 1. Unit Tests

```javascript
// tests/dual-pricing.test.js
import { expect } from 'chai';
import { ethers } from 'hardhat';

describe('Dual Pricing Integration', () => {
  let nodeRegistry, marketplace, host, client;

  beforeEach(async () => {
    [host, client] = await ethers.getSigners();

    // Use ACTUAL deployed contracts on Base Sepolia
    nodeRegistry = await ethers.getContractAt(
      'NodeRegistryWithModels',
      '0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6'
    );

    marketplace = await ethers.getContractAt(
      'JobMarketplaceWithModels',
      '0xe169A4B57700080725f9553E3Cc69885fea13629'
    );
  });

  it('should query dual pricing correctly', async () => {
    const [nativePrice, stablePrice] = await nodeRegistry.getNodePricing(host.address);

    expect(nativePrice).to.be.a('BigNumber');
    expect(stablePrice).to.be.a('Number');
  });

  it('should validate ETH session against native pricing', async () => {
    const [hostMinNative, _] = await nodeRegistry.getNodePricing(host.address);

    // Should fail with price below minimum
    await expect(
      marketplace.connect(client).createSessionJob(
        host.address,
        hostMinNative.sub(1), // 1 wei below minimum
        3600,
        100,
        { value: ethers.utils.parseEther('0.1') }
      )
    ).to.be.revertedWith('Price below host minimum (native)');
  });

  it('should validate USDC session against stable pricing', async () => {
    const [_, hostMinStable] = await nodeRegistry.getNodePricing(host.address);

    // Should fail with price below minimum
    await expect(
      marketplace.connect(client).createSessionJobWithToken(
        host.address,
        usdcAddress,
        ethers.utils.parseUnits('10', 6),
        hostMinStable - 1, // Below minimum
        3600,
        100
      )
    ).to.be.revertedWith('Price below host minimum (stable)');
  });

  it('should return 8 fields from getNodeFullInfo', async () => {
    const info = await nodeRegistry.getNodeFullInfo(host.address);

    expect(info.length).to.equal(8);
    expect(info[6]).to.be.a('BigNumber'); // minPriceNative
    expect(info[7]).to.be.a('Number');    // minPriceStable
  });
});
```

### 2. Integration Tests

```javascript
// tests/integration/session-creation.test.js
describe('End-to-End Session Creation', () => {
  it('should create ETH session with valid pricing', async () => {
    const hostManager = new HostManager(nodeRegistry, hostSigner);
    const clientManager = new ClientManager(marketplace, nodeRegistry, provider, clientSigner);

    // Host sets pricing
    await hostManager.registerHost({
      metadata: JSON.stringify({ gpu: 'rtx-4090' }),
      apiUrl: 'http://localhost:8080',
      modelIds: [modelId],
      minPriceNative: ethers.BigNumber.from('3000000000'), // 3B wei
      minPriceStable: 15000 // 0.015 USDC
    });

    // Client queries and creates session
    const pricing = await hostManager.getHostPricing(hostAddress);

    const session = await clientManager.createETHSession({
      hostAddress,
      pricePerToken: pricing.native.wei,
      maxDuration: 3600,
      proofInterval: 100,
      depositAmount: ethers.utils.parseEther('0.1')
    });

    expect(session.sessionId).to.exist;
  });
});
```

## Common Patterns

### 1. Price Discovery UI

```javascript
// Example: Display host pricing to users
async function displayHostPricing(hostAddress) {
  const [nativePrice, stablePrice] = await nodeRegistry.getNodePricing(hostAddress);

  const nativeFormatted = formatNativePrice(nativePrice);
  const stableFormatted = formatStablePrice(stablePrice);

  return {
    eth: {
      label: 'ETH Session Price',
      perToken: `${nativeFormatted.eth} ETH (~$${nativeFormatted.usd})`,
      totalFor1000Tokens: `${(parseFloat(nativeFormatted.eth) * 1000).toFixed(6)} ETH`
    },
    usdc: {
      label: 'USDC Session Price',
      perToken: `${stableFormatted.usdc} USDC`,
      totalFor1000Tokens: `${(parseFloat(stableFormatted.usdc) * 1000).toFixed(2)} USDC`
    }
  };
}
```

### 2. Dynamic Pricing Adjustment

```javascript
// Example: Host adjusts pricing based on demand
class DynamicPricingManager {
  async adjustPricingBasedOnLoad(currentLoad) {
    const baseNativePrice = ethers.BigNumber.from('3000000000');
    const baseStablePrice = 15000;

    let multiplier = 1.0;
    if (currentLoad > 0.8) multiplier = 1.5;      // 50% increase
    else if (currentLoad > 0.5) multiplier = 1.2; // 20% increase

    const newNativePrice = baseNativePrice.mul(Math.floor(multiplier * 100)).div(100);
    const newStablePrice = Math.floor(baseStablePrice * multiplier);

    // Update both prices
    await hostManager.updateNativePricing(newNativePrice);
    await hostManager.updateStablePricing(newStablePrice);

    console.log('Pricing adjusted for load:', currentLoad);
  }
}
```

### 3. Price Comparison

```javascript
// Example: Help users choose between ETH and USDC
async function comparePricing(hostAddress, ethPriceUSD = 4400) {
  const [nativePrice, stablePrice] = await nodeRegistry.getNodePricing(hostAddress);

  // Convert to USD for comparison
  const nativeInUSD = parseFloat(ethers.utils.formatEther(nativePrice)) * ethPriceUSD;
  const stableInUSD = stablePrice / 1_000_000;

  return {
    native: {
      price: nativePrice.toString(),
      usdValue: nativeInUSD,
      perThousandTokens: nativeInUSD * 1000
    },
    stable: {
      price: stablePrice,
      usdValue: stableInUSD,
      perThousandTokens: stableInUSD * 1000
    },
    recommendation: nativeInUSD < stableInUSD ? 'ETH' : 'USDC',
    savingsPercent: Math.abs((nativeInUSD - stableInUSD) / Math.max(nativeInUSD, stableInUSD) * 100)
  };
}
```

## Troubleshooting

### Issue: "Price below host minimum (native)"

**Cause**: Client offered ETH session price below host's `minPricePerTokenNative`

**Solution**:
```javascript
// Always query before creating session
const [hostMinNative, _] = await nodeRegistry.getNodePricing(hostAddress);
const clientPrice = hostMinNative.add(ethers.BigNumber.from('1000000000')); // Add buffer
```

### Issue: "Price below host minimum (stable)"

**Cause**: Client offered USDC session price below host's `minPricePerTokenStable`

**Solution**:
```javascript
// Always query before creating session
const [_, hostMinStable] = await nodeRegistry.getNodePricing(hostAddress);
const clientPrice = hostMinStable + 1000; // Add buffer
```

### Issue: Wrong pricing field used

**Cause**: Used native pricing for USDC session or vice versa

**Solution**: Always use the correct field based on payment type:
- ETH sessions → use `minPricePerTokenNative`
- USDC sessions → use `minPricePerTokenStable`

### Issue: Struct parsing returns wrong values

**Cause**: Still parsing as 7-field struct instead of 8-field

**Solution**:
```javascript
// Correct: 8 fields
const [op, stake, active, meta, api, models, priceNative, priceStable] =
  await nodeRegistry.getNodeFullInfo(host);
```

## Best Practices

1. **Always Query Pricing First**
   - Never hard-code price assumptions
   - Query `getNodePricing()` before every session creation

2. **Validate Ranges**
   - Check against contract MIN/MAX constants
   - Provide clear error messages to users

3. **Use Type Safety**
   - Native prices: Use `ethers.BigNumber`
   - Stable prices: Use `number` or `BigNumber` for consistency

4. **Cache Pricing Data**
   - Cache for short periods (1-5 minutes)
   - Refresh before session creation

5. **Handle Both Payment Types**
   - Support both ETH and USDC sessions
   - Let users compare pricing

6. **Monitor Events**
   - Listen for `PricingUpdatedNative` and `PricingUpdatedStable` events
   - Update cached data when hosts change pricing

## Reference Links

- **Contract Addresses**: `/workspace/CONTRACT_ADDRESSES.md`
- **Client ABIs**: `/workspace/client-abis/`
- **Technical Docs**: `/workspace/docs/technical/contracts/`
- **Base Sepolia Explorer**: https://sepolia.basescan.org

## Support

For questions or issues:
- GitHub Issues: https://github.com/your-repo/issues
- Documentation: https://docs.fabstir.com
- Discord: https://discord.gg/fabstir
