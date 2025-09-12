# Host Registration Guide

## Overview

This guide explains how to register as a host (GPU provider) in the Fabstir P2P LLM marketplace on Base Sepolia. The system uses FAB token staking to ensure host commitment and quality of service.

## Host States

In the NodeRegistry, hosts have simple state management:

| State | `active` field | `operator` field | Description |
|-------|---------------|------------------|-------------|
| **Not Registered** | N/A | `0x0000...0000` | Host has never registered or has unregistered |
| **Active** | `true` | Host address | Host is registered and can accept jobs |
| **Inactive** | `false` | Host address | Currently not used - hosts are either registered or not |

**Note**: The `active` field is a **boolean** (not an enum):
- `true` = Host is active and can accept jobs
- `false` = Would indicate inactive (but current implementation deletes node instead)
- When a host unregisters, the entire node data is deleted, not just set to inactive

## Prerequisites

Before registering as a host, you need:

1. **1000 FAB tokens** - Required stake amount
2. **ETH for gas fees** - Approximately 0.01 ETH for registration
3. **Base Sepolia wallet** - MetaMask or compatible wallet
4. **GPU capabilities** - Hardware to run AI models
5. **API Endpoint** - HTTP/HTTPS endpoint where your host serves inference requests (e.g., `http://your-host.com:8080`)

## Contract Information

**NodeRegistryFAB Contract**: `0x039AB5d5e8D5426f9963140202F506A2Ce6988F9`  
**FAB Token Contract**: `0xC78949004B4EB6dEf2D66e49Cd81231472612D62`  
**Network**: Base Sepolia (Chain ID: 84532)

## Registration Process

### Step 1: Obtain FAB Tokens

You need exactly **1000 FAB tokens** to register. These tokens are staked during registration and returned when you unregister.

```javascript
// Check your FAB balance
const fabToken = new ethers.Contract(
  '0xC78949004B4EB6dEf2D66e49Cd81231472612D62',
  ['function balanceOf(address) view returns (uint256)'],
  provider
);

const balance = await fabToken.balanceOf(yourAddress);
console.log(`FAB Balance: ${ethers.utils.formatUnits(balance, 18)}`);
```

### Step 2: Approve Token Spending

Before registration, approve the NodeRegistry to transfer your FAB tokens:

```javascript
// Approve 1000 FAB tokens
const amount = ethers.utils.parseUnits('1000', 18);

const approveTx = await fabToken.approve(
  '0x039AB5d5e8D5426f9963140202F506A2Ce6988F9', // NodeRegistry address
  amount
);
await approveTx.wait();
```

### Step 3: Register as Host

#### Option A: Register with API URL (Recommended)

Register with both your capabilities and API endpoint for automatic discovery:

```javascript
const nodeRegistry = new ethers.Contract(
  '0x039AB5d5e8D5426f9963140202F506A2Ce6988F9',
  ['function registerNodeWithUrl(string metadata, string apiUrl) external'],
  signer
);

// Metadata describes your capabilities
const metadata = 'llama-2-7b,llama-2-13b,gpt-4,inference,base-sepolia';
// API URL where clients can reach your inference service
const apiUrl = 'http://your-host.example.com:8080';

const registerTx = await nodeRegistry.registerNodeWithUrl(metadata, apiUrl);
await registerTx.wait();

console.log('✅ Successfully registered as host with API endpoint!');
```

#### Option B: Register without API URL (Legacy)

You can still use the old method and add your API URL later:

```javascript
const nodeRegistry = new ethers.Contract(
  '0x039AB5d5e8D5426f9963140202F506A2Ce6988F9',
  ['function registerNode(string metadata) external'],
  signer
);

// Metadata describes your capabilities
const metadata = 'llama-2-7b,llama-2-13b,gpt-4,inference,base-sepolia';

const registerTx = await nodeRegistry.registerNode(metadata);
await registerTx.wait();

console.log('✅ Successfully registered as host!');

// Later, add your API URL
const apiUrl = 'http://your-host.example.com:8080';
await nodeRegistry.updateApiUrl(apiUrl);
```

### Step 4: Verify Registration

Check your registration status:

```javascript
const nodeInfo = await nodeRegistry.nodes(yourAddress);

// The nodes() function now returns: (operator, stakedAmount, active, metadata, apiUrl)
console.log({
  operator: nodeInfo.operator,        // Your address if registered, 0x0 if not
  stakedAmount: ethers.utils.formatUnits(nodeInfo.stakedAmount, 18),
  active: nodeInfo.active,            // Boolean: true if active, false if not
  metadata: nodeInfo.metadata,
  apiUrl: nodeInfo.apiUrl             // Your API endpoint URL
});

// Interpret the status
if (nodeInfo.operator === ethers.constants.AddressZero) {
  console.log('❌ Not registered');
} else if (nodeInfo.active === true) {
  console.log('✅ Active and ready to accept jobs');
} else {
  console.log('⚠️ Registered but inactive (rare state)');
}
```

## Complete Code Example

```javascript
const { ethers } = require('ethers');

async function registerHost(privateKey, metadata, apiUrl) {
  // Setup
  const provider = new ethers.providers.JsonRpcProvider(
    'https://base-sepolia.g.alchemy.com/v2/YOUR_API_KEY'
  );
  const wallet = new ethers.Wallet(privateKey, provider);
  
  // Contract addresses
  const FAB_TOKEN = '0xC78949004B4EB6dEf2D66e49Cd81231472612D62';
  const NODE_REGISTRY = '0x039AB5d5e8D5426f9963140202F506A2Ce6988F9';
  
  // ABIs
  const fabTokenAbi = [
    'function balanceOf(address) view returns (uint256)',
    'function approve(address spender, uint256 amount) returns (bool)',
    'function allowance(address owner, address spender) view returns (uint256)'
  ];
  
  const nodeRegistryAbi = [
    'function registerNode(string metadata) external',
    'function registerNodeWithUrl(string metadata, string apiUrl) external',
    'function updateApiUrl(string apiUrl) external',
    'function nodes(address) view returns (address operator, uint256 stakedAmount, bool active, string metadata, string apiUrl)',
    'function getNodeApiUrl(address) view returns (string)',
    'function getNodeFullInfo(address) view returns (address, uint256, bool, string, string)',
    'function MIN_STAKE() view returns (uint256)'
  ];
  
  // Initialize contracts
  const fabToken = new ethers.Contract(FAB_TOKEN, fabTokenAbi, wallet);
  const nodeRegistry = new ethers.Contract(NODE_REGISTRY, nodeRegistryAbi, wallet);
  
  try {
    // 1. Check requirements
    const minStake = await nodeRegistry.MIN_STAKE();
    const balance = await fabToken.balanceOf(wallet.address);
    
    console.log(`Minimum stake required: ${ethers.utils.formatUnits(minStake, 18)} FAB`);
    console.log(`Your balance: ${ethers.utils.formatUnits(balance, 18)} FAB`);
    
    if (balance.lt(minStake)) {
      throw new Error('Insufficient FAB tokens');
    }
    
    // 2. Approve tokens
    console.log('Approving FAB tokens...');
    const approveTx = await fabToken.approve(NODE_REGISTRY, minStake);
    await approveTx.wait();
    console.log('✅ Tokens approved');
    
    // 3. Register with API URL (if provided)
    console.log('Registering as host...');
    const apiUrl = 'http://your-host.example.com:8080'; // Your API endpoint
    const registerTx = await nodeRegistry.registerNodeWithUrl(metadata, apiUrl);
    const receipt = await registerTx.wait();
    console.log('✅ Registration complete with API discovery!');
    console.log(`Transaction: ${receipt.transactionHash}`);
    console.log(`API URL: ${apiUrl}`);
    
    // 4. Verify
    const nodeInfo = await nodeRegistry.nodes(wallet.address);
    console.log('\nYour node info:');
    console.log(`- Active: ${nodeInfo.active}`);
    console.log(`- Staked: ${ethers.utils.formatUnits(nodeInfo.stakedAmount, 18)} FAB`);
    console.log(`- Metadata: ${nodeInfo.metadata}`);
    console.log(`- API URL: ${nodeInfo.apiUrl || 'Not set'}`);
    
  } catch (error) {
    console.error('Registration failed:', error.message);
  }
}

// Usage
registerHost(
  'YOUR_PRIVATE_KEY',
  'llama-2-7b,llama-2-13b,inference',
  'http://your-api.example.com:8080'  // Add your API URL
);
```

## Additional Operations

### Update Your API Endpoint

If you need to change your API URL (e.g., changed ports, domain, or IP):

```javascript
const newApiUrl = 'https://your-host.example.com:8443';
const updateTx = await nodeRegistry.updateApiUrl(newApiUrl);
await updateTx.wait();
console.log('✅ API URL updated!');
```

### Add More Stake (Optional)

You can stake additional FAB tokens to increase your node's stake:

```javascript
const additionalAmount = ethers.utils.parseUnits('500', 18);

// Approve additional tokens
await fabToken.approve(NODE_REGISTRY, additionalAmount);

// Add stake
const stakeTx = await nodeRegistry.stake(additionalAmount);
await stakeTx.wait();
```

### Update Metadata

Change your node's capabilities description:

```javascript
const newMetadata = 'llama-2-70b,gpt-4,claude-3,inference,training';
const updateTx = await nodeRegistry.updateMetadata(newMetadata);
await updateTx.wait();
```

### Unregister and Withdraw Stake

When you want to stop being a host:

```javascript
const unregisterTx = await nodeRegistry.unregisterNode();
await unregisterTx.wait();
// Your 1000 FAB tokens are automatically returned
```

## Important Notes

### Recent Updates (January 2025)
- **NEW**: Added API endpoint discovery - hosts can now register with their API URLs for automatic client discovery
- **Fixed**: Re-registration bug that prevented hosts from registering again after unregistering
- **New Contract Address**: NodeRegistry deployed at `0x039AB5d5e8D5426f9963140202F506A2Ce6988F9` (Note: API URL feature requires redeployment)
- **New JobMarketplace**: Updated to `0x001A47Bb8C6CaD9995639b8776AB5816Ab9Ac4E0` with refund fixes

### Gas Costs
- Registration: ~200,000 gas
- Unregistration: ~100,000 gas
- Metadata update: ~50,000 gas

### Staking Rules
- Minimum stake: 1000 FAB tokens
- Tokens are locked during registration
- All staked tokens returned on unregistration
- No slashing mechanism currently implemented

### Metadata Format
The metadata string should describe your capabilities. Suggested format:
```
"model1,model2,model3,capability1,capability2,location"
```

Examples:
- `"llama-2-7b,llama-2-13b,inference,base-sepolia"`
- `"gpt-4,claude-3,training,inference,us-east"`
- `"stable-diffusion,dalle-3,image-generation,europe"`

## Troubleshooting

### Common Errors

1. **"Already registered"**
   - You're already registered. The contract now properly handles re-registration.
   - **Fix Applied (Jan 2025)**: The registry now correctly checks if you're already registered and prevents duplicate registration attempts.

2. **"Transfer failed"**
   - Insufficient FAB tokens or approval not set correctly.

3. **"Empty metadata"**
   - Metadata string cannot be empty.

4. **"Not registered"**
   - Trying to perform operations that require registration.

### Checking Registration Status

```javascript
async function checkHostStatus(address) {
  const nodeInfo = await nodeRegistry.nodes(address);
  const isRegistered = nodeInfo.operator !== ethers.constants.AddressZero;
  
  if (isRegistered) {
    console.log('Host is registered');
    console.log('Staked:', ethers.utils.formatUnits(nodeInfo.stakedAmount, 18), 'FAB');
    console.log('Active:', nodeInfo.active);
    console.log('Metadata:', nodeInfo.metadata);
    console.log('API URL:', nodeInfo.apiUrl || 'Not set');
  } else {
    console.log('Host is not registered');
  }
}
```

## API Endpoint Discovery

The NodeRegistry now supports automatic API endpoint discovery, allowing clients to find your inference service without hardcoded URLs.

### For Hosts

1. **Register with API URL**: Use `registerNodeWithUrl()` to include your endpoint
2. **Update API URL**: Use `updateApiUrl()` to change your endpoint anytime
3. **API URL Format**: Can be HTTP or HTTPS, include port if non-standard
   - Examples: `http://192.168.1.100:8080`, `https://api.myhost.com`, `http://localhost:8083`

### For Clients/SDK

Query the registry to discover host endpoints:

```javascript
// Get a specific host's API URL
const apiUrl = await nodeRegistry.getNodeApiUrl(hostAddress);

// Get all info including API URL
const info = await nodeRegistry.getNodeFullInfo(hostAddress);
console.log(`Host ${hostAddress} API: ${info[4]}`);

// Discover all active hosts with their endpoints
const activeHosts = await nodeRegistry.getAllActiveNodes();
for (const host of activeHosts) {
  const url = await nodeRegistry.getNodeApiUrl(host);
  console.log(`${host}: ${url || 'No API URL set'}`);
}
```

## Integration with JobMarketplace

Once registered, you can:
1. Be selected for jobs by renters
2. Submit proofs of work
3. Earn payments for completed inference tasks
4. Build reputation in the system

The JobMarketplace contract (`0x001A47Bb8C6CaD9995639b8776AB5816Ab9Ac4E0`) verifies your registration before allowing you to claim jobs.

## Support

For technical support:
- GitHub Issues: https://github.com/fabstir/fabstir-llm-marketplace
- Documentation: [Technical Docs](./technical/contracts/NodeRegistry.md)
- Contract Source: [NodeRegistryFAB.sol](../src/NodeRegistryFAB.sol)

## Next Steps

After successful registration:
1. Monitor the marketplace for available jobs
2. Set up your GPU infrastructure
3. Implement the host client to handle inference requests
4. Start earning FAB tokens and ETH for completed jobs

---

*Last Updated: January 2025*