# API Endpoint Discovery Solution

*Last Updated: January 2025*

## Problem Solved
Previously, there was no way for clients to automatically discover host API endpoints. The SDK had to rely on hardcoded URLs or environment variables. This has been implemented in the NodeRegistryWithModels contract.

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

#### Registration with URL and Models
```solidity
// Register new node with metadata, API URL, and supported models
function registerNode(
    string memory metadata,
    string memory apiUrl,
    bytes32[] memory supportedModels
) external
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

// Get all node information including API URL and models
function getNodeFullInfo(address operator) external view returns (
    address nodeOperator,
    uint256 stakedAmount,
    bool active,
    string memory metadata,
    string memory apiUrl,
    bytes32[] memory supportedModels
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
        "function getNodeFullInfo(address) view returns (address, uint256, bool, string, string, bytes32[])"
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
  "0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218", // NodeRegistryWithModels address
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

### Registering with API URL and Models

New hosts must register with metadata, API URL, and supported models:

```javascript
const registry = new ethers.Contract(registryAddress, ABI, signer);

// Current approved model IDs from ModelRegistry
const TINY_VICUNA = "0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced";
const TINY_LLAMA = "0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca";

// Register with all required parameters
await registry.registerNode(
  JSON.stringify({                         // Structured metadata
    hardware: { gpu: "rtx-4090", vram: 24 },
    capabilities: ["inference", "streaming"],
    location: "us-west"
  }),
  "https://my-api.example.com:8443",      // API endpoint
  [TINY_VICUNA, TINY_LLAMA]                // Supported models (must be approved)
);
```

### Updating API URL or Supported Models

Hosts can update their configuration after registration:

```javascript
// Update API URL
await registry.updateApiUrl("https://new-api.example.com:8443");

// Update supported models (must be approved models)
await registry.updateSupportedModels([TINY_VICUNA, TINY_LLAMA]);

// Update metadata
await registry.updateMetadata(JSON.stringify({
  hardware: { gpu: "rtx-4090-upgraded", vram: 48 },
  capabilities: ["inference", "streaming", "batch"],
  location: "eu-central"
}));
```

## Important Notes

1. **Already deployed** - NodeRegistryWithModels at `0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218` includes API URL support
2. **Model validation** - Hosts must also register with supported models from ModelRegistry
3. **Gas costs** - Storing URLs on-chain costs more gas, but provides reliable discovery
4. **URL validation** - The contract only checks that URLs are non-empty, not that they're valid
5. **Security** - Hosts are responsible for securing their API endpoints

## Implementation Status

1. **NodeRegistryWithModels deployed** - Includes full API URL support
2. **JobMarketplaceWithModels integrated** - Uses the new registry
3. **Model validation active** - Hosts must support approved models
4. **SDK ready** - Can use discovery functions immediately
5. **Host registration** - New hosts use `registerNode()` with API URL

## Contract Addresses

- **NodeRegistryWithModels**: `0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218` (ACTIVE - with API URLs)
- **ModelRegistry**: `0x92b2De840bB2171203011A6dBA928d855cA8183E` (Model governance)
- **JobMarketplaceWithModels**: `0xaa38e7fcf5d7944ef7c836e8451f3bf93b98364f` (Multi-chain support)
- **HostEarnings**: `0x908962e8c6CE72610021586f85ebDE09aAc97776` (Earnings accumulation)

## Testing

The API discovery is now fully integrated into NodeRegistryWithModels:
- Hosts register with API URLs and supported models
- Clients can discover host endpoints dynamically
- Hosts can update their API URLs anytime
- Full integration with model validation system

## Benefits

1. **Automatic discovery** - No more hardcoding URLs
2. **Dynamic updates** - Hosts can change URLs without SDK updates
3. **Single source of truth** - Contract stores all host information
4. **Decentralized** - No central registry service needed
5. **Fallback compatible** - Can still use env vars during migration