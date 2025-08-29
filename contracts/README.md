# Client ABI Files

This directory contains minimal ABI files for client applications to interact with the Fabstir smart contracts on Base Sepolia.

## Contract Addresses

```javascript
const contractAddresses = {
  jobMarketplace: "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304",
  paymentEscrow: "0xa4C5599Ea3617060ce86Ff0916409e1fb4a0d2c6", 
  nodeRegistry: "0x87516C13Ea2f99de598665e14cab64E191A0f8c4",
  hostEarnings: "0xbFfCd6BAaCCa205d471bC52Bd37e1957B1A43d4a",
  treasury: "0x4e770e723B95A0d8923Db006E49A8a3cb0BAA078" // EOA wallet, not a contract
}
```

## Available ABIs

- **JobMarketplaceFABWithS5-CLIENT-ABI.json** - Job posting and management with S5 CID support
- **PaymentEscrowWithEarnings-CLIENT-ABI.json** - Payment escrow and fee handling
- **NodeRegistryFAB-CLIENT-ABI.json** - Node registration and management
- **HostEarnings-CLIENT-ABI.json** - Host earnings accumulation and withdrawal

## Usage Example

```javascript
import { ethers } from 'ethers';
import JobMarketplaceABI from './JobMarketplaceFABWithS5-CLIENT-ABI.json';
import PaymentEscrowABI from './PaymentEscrowWithEarnings-CLIENT-ABI.json';
import NodeRegistryABI from './NodeRegistryFAB-CLIENT-ABI.json';
import HostEarningsABI from './HostEarnings-CLIENT-ABI.json';

// Initialize contracts
const provider = new ethers.JsonRpcProvider('https://sepolia.base.org');
const signer = provider.getSigner();

const contracts = {
  jobMarketplace: new ethers.Contract(
    contractAddresses.jobMarketplace,
    JobMarketplaceABI,
    signer
  ),
  paymentEscrow: new ethers.Contract(
    contractAddresses.paymentEscrow,
    PaymentEscrowABI,
    signer
  ),
  nodeRegistry: new ethers.Contract(
    contractAddresses.nodeRegistry,
    NodeRegistryABI,
    signer
  ),
  hostEarnings: new ethers.Contract(
    contractAddresses.hostEarnings,
    HostEarningsABI,
    signer
  )
};
```

## Key Functions

### JobMarketplace
- `postJobWithToken()` - Post a job with S5 CID for prompt
- `claimJob()` - Host claims a job
- `completeJob()` - Complete job with S5 CID for response
- `getJob()` - Get job details including CIDs

### NodeRegistry
- `registerNode()` - Register as a host with FAB stake
- `unregisterNode()` - Unregister and withdraw stake
- `isHostActive()` - Check if host is active
- `getNodeInfo()` - Get host details

### HostEarnings
- `getBalance()` - Check earnings balance
- `withdraw()` - Withdraw specific token earnings
- `withdrawAll()` - Withdraw all earnings

### PaymentEscrow
- `releaseEscrow()` - Release payment for completed job
- `getEscrow()` - Get escrow details

## Notes

- Treasury address is an EOA (wallet), not a smart contract
- All contracts use S5 CID storage for prompts/responses
- Minimum stake for node registration: 1000 FAB tokens
- Platform fee: 10% (1000 basis points)