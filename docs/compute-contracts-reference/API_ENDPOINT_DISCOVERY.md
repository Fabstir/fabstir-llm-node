# API Endpoint Discovery Solution

## Problem Solved
Previously, there was no way for clients to automatically discover host API endpoints. The SDK had to rely on hardcoded URLs or environment variables. This has been fixed by adding API URL storage directly to the NodeRegistryFAB contract.

## Contract Changes Made

### Updated Node Structure
```solidity
struct Node {
    address operator;
    uint256 stakedAmount;
    bool active;
    string metadata;     // Existing: capabilities like "llama-2-7b,inference"
    string apiUrl;       // NEW: API endpoint like "http://host.com:8080"
}
```

### New Functions Added

#### Registration with URL
```solidity
// Register new node with both metadata and API URL
function registerNodeWithUrl(string memory metadata, string memory apiUrl) external

// Backward compatible - sets apiUrl to empty string
function registerNode(string memory metadata) external
```

#### Update API URL
```solidity
// Hosts can update their API URL without re-registering
function updateApiUrl(string memory newApiUrl) external
```

#### Query Functions
```solidity
// Get just the API URL for a host
function getNodeApiUrl(address operator) external view returns (string memory)

// Get all node information including API URL
function getNodeFullInfo(address operator) external view returns (
    address nodeOperator,
    uint256 stakedAmount,
    bool active,
    string memory metadata,
    string memory apiUrl
)
```

## SDK Integration Guide

### 1. Basic Discovery Pattern

```javascript
const { ethers } = require("ethers");

class HostDiscovery {
  constructor(registryAddress, provider) {
    this.registry = new ethers.Contract(
      registryAddress,
      [
        "function getNodeApiUrl(address) view returns (string)",
        "function getAllActiveNodes() view returns (address[])",
        "function getNodeFullInfo(address) view returns (address, uint256, bool, string, string)"
      ],
      provider
    );
  }
  
  async getHostEndpoint(hostAddress) {
    try {
      const apiUrl = await this.registry.getNodeApiUrl(hostAddress);
      if (apiUrl && apiUrl !== "") {
        return apiUrl;
      }
    } catch (error) {
      console.error(`Failed to get API URL for ${hostAddress}:`, error);
    }
    return null;
  }
  
  async discoverAllHosts() {
    const hosts = {};
    const activeNodes = await this.registry.getAllActiveNodes();
    
    for (const nodeAddress of activeNodes) {
      const apiUrl = await this.getHostEndpoint(nodeAddress);
      if (apiUrl) {
        hosts[nodeAddress] = {
          address: nodeAddress,
          apiUrl: apiUrl
        };
      }
    }
    
    return hosts;
  }
}
```

### 2. Complete Integration Example

```javascript
// Initialize discovery
const discovery = new HostDiscovery(
  "0x039AB5d5e8D5426f9963140202F506A2Ce6988F9", // NodeRegistry address
  provider
);

// When user selects a host for a job
async function connectToHost(hostAddress) {
  // 1. Get the host's API endpoint from the contract
  const apiUrl = await discovery.getHostEndpoint(hostAddress);
  
  if (!apiUrl) {
    throw new Error(`Host ${hostAddress} has not set their API URL`);
  }
  
  // 2. Connect to the host's API
  const response = await fetch(`${apiUrl}/api/v1/inference`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      model: 'llama-2-7b',
      prompt: 'Hello, world!',
      jobId: 123
    })
  });
  
  return response.json();
}

// Discover all available hosts with their endpoints
async function listAvailableHosts() {
  const hosts = await discovery.discoverAllHosts();
  
  console.log("Available Hosts:");
  for (const [address, info] of Object.entries(hosts)) {
    console.log(`- ${address}`);
    console.log(`  API: ${info.apiUrl}`);
    
    // Get additional info if needed
    const fullInfo = await registry.getNodeFullInfo(address);
    console.log(`  Capabilities: ${fullInfo[3]}`);
    console.log(`  Staked: ${ethers.utils.formatUnits(fullInfo[1], 18)} FAB`);
  }
  
  return hosts;
}
```

### 3. Fallback Strategy (Migration Period)

During the migration period, some hosts may not have set their API URLs yet:

```javascript
async function getHostEndpointWithFallback(hostAddress) {
  // 1. Try contract first
  const contractUrl = await discovery.getHostEndpoint(hostAddress);
  if (contractUrl) return contractUrl;
  
  // 2. Fallback to environment variables
  const envUrl = process.env[`HOST_${hostAddress.toUpperCase()}_URL`];
  if (envUrl) return envUrl;
  
  // 3. Fallback to hardcoded map (temporary)
  const KNOWN_HOSTS = {
    "0x4594F755F593B517Bb3194F4DeC20C48a3f04504": "http://localhost:8080",
    "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7": "http://localhost:8083"
  };
  
  return KNOWN_HOSTS[hostAddress] || null;
}
```

## For Host Operators

### Registering with API URL

New hosts should register with their API URL:

```javascript
const registry = new ethers.Contract(registryAddress, ABI, signer);

// Register with API URL
await registry.registerNodeWithUrl(
  "llama-2-7b,gpt-4,inference",           // capabilities
  "https://my-api.example.com:8443"       // API endpoint
);
```

### Updating API URL for Existing Hosts

Hosts already registered can add their API URL:

```javascript
// Add or update API URL
await registry.updateApiUrl("https://my-api.example.com:8443");

// Can be called anytime to update the URL
await registry.updateApiUrl("https://new-endpoint.example.com:9000");
```

## Important Notes

1. **Contract must be redeployed** - The current deployed NodeRegistry doesn't have these changes yet
2. **Backward compatible** - Existing registrations will work, they just have empty API URLs
3. **Gas costs** - Storing URLs on-chain costs more gas, but provides reliable discovery
4. **URL validation** - The contract only checks that URLs are non-empty, not that they're valid
5. **Security** - Hosts are responsible for securing their API endpoints

## Migration Plan

1. **Deploy new NodeRegistryFAB** with API URL support
2. **Migrate existing hosts** - They can re-register or we add migration function
3. **Update JobMarketplace** to use new registry
4. **SDK updates** - Use new discovery instead of hardcoded URLs
5. **Host updates** - Hosts call `updateApiUrl()` to add their endpoints

## Contract Addresses

- **Current NodeRegistry** (without API URLs): `0x039AB5d5e8D5426f9963140202F506A2Ce6988F9`
- **New NodeRegistry** (with API URLs): To be deployed

## Testing

Run the example script to see how discovery works:

```bash
node scripts/node-api-discovery-example.js
```

This will demonstrate:
- How to register with API URLs
- How to discover host endpoints
- How to update API URLs
- SDK integration patterns

## Benefits

1. **Automatic discovery** - No more hardcoding URLs
2. **Dynamic updates** - Hosts can change URLs without SDK updates
3. **Single source of truth** - Contract stores all host information
4. **Decentralized** - No central registry service needed
5. **Fallback compatible** - Can still use env vars during migration