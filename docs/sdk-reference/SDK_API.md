# Fabstir SDK API Reference

## Table of Contents
- [Overview](#overview)
- [Installation](#installation)
- [Core SDK](#core-sdk)
- [Authentication](#authentication)
- [Session Management](#session-management)
- [Payment Management](#payment-management)
- [Model Governance](#model-governance)
- [Host Management](#host-management)
- [Storage Management](#storage-management)
  - [User Settings Storage](#user-settings-storage)
- [Treasury Management](#treasury-management)
- [Client Manager](#client-manager)
- [WebSocket Communication](#websocket-communication)
- [Contract Integration](#contract-integration)
- [Services](#services)
- [Error Handling](#error-handling)
- [Types and Interfaces](#types-and-interfaces)
- [Usage Examples](#usage-examples)

## Overview

The Fabstir SDK provides a comprehensive interface for interacting with the Fabstir P2P LLM marketplace. The SDK has been refactored into browser-compatible (`@fabstir/sdk-core`) and Node.js-specific (`@fabstir/sdk-node`) packages.

### Key Features
- Browser-compatible core functionality
- USDC and ETH payment support
- Session-based LLM interactions with context preservation
- Model governance and validation
- WebSocket real-time streaming
- S5 decentralized storage integration
- Base Account Kit for gasless transactions
- Multi-chain support (Base Sepolia, opBNB testnet)
- Chain-aware wallet providers (EOA and Smart Accounts)

## Multi-Chain Support

The SDK now provides comprehensive multi-chain support, allowing seamless operation across different blockchain networks.

### Supported Chains

| Chain | Chain ID | Native Token | Status | Min Deposit |
|-------|----------|--------------|--------|-------------|
| Base Sepolia | 84532 | ETH | Production | 0.0002 ETH |
| opBNB Testnet | 5611 | BNB | Development | 0.001 BNB |

### Chain Configuration

Each chain has its own configuration including contract addresses, RPC endpoints, and network-specific parameters:

```typescript
interface ChainConfig {
  chainId: number;
  name: string;
  nativeToken: 'ETH' | 'BNB';
  rpcUrl: string;
  contracts: {
    jobMarketplace: string;
    nodeRegistry: string;
    proofSystem: string;
    hostEarnings: string;
    modelRegistry: string;
    usdcToken: string;
    fabToken?: string;
  };
  minDeposit: string;
  blockExplorer: string;
}
```

### Default Chain Behavior

- The SDK defaults to Base Sepolia (chainId: 84532) when no chain is specified
- All operations are chain-aware and validate the target chain
- Smart contract addresses are automatically selected based on the active chain

## Installation

### Browser/React Applications

```bash
npm install @fabstir/sdk-core
```

### Node.js Applications

```bash
npm install @fabstir/sdk-core @fabstir/sdk-node
```

### Development Setup (npm link)

```bash
# In sdk-core directory
cd packages/sdk-core
pnpm build
npm link

# In your application
npm link @fabstir/sdk-core
```

## Core SDK

### Imports

The SDK exports the main class and utility functions:

```typescript
// Main SDK class
import { FabstirSDKCore } from '@fabstir/sdk-core';

// Wallet utilities for Base Account Kit integration
import {
  ensureSubAccount,
  createSubAccountSigner
} from '@fabstir/sdk-core';

// Types
import type {
  SubAccountOptions,
  SubAccountResult,
  SubAccountSignerOptions
} from '@fabstir/sdk-core';
```

### FabstirSDKCore

The main SDK class for browser environments.

```typescript
import { FabstirSDKCore } from '@fabstir/sdk-core';
```

#### Constructor

```typescript
new FabstirSDKCore(config?: FabstirSDKCoreConfig)
```

**Configuration:**
```typescript
interface FabstirSDKCoreConfig {
  rpcUrl: string;                     // REQUIRED: Blockchain RPC URL
  chainId?: number;                   // Optional: Chain ID (default: 84532 - Base Sepolia)
  contractAddresses: {                // REQUIRED: All 7 contracts
    jobMarketplace: string;           // REQUIRED
    nodeRegistry: string;             // REQUIRED
    proofSystem: string;              // REQUIRED
    hostEarnings: string;             // REQUIRED
    usdcToken: string;                // REQUIRED
    fabToken: string;                 // REQUIRED (was optional, now required)
    modelRegistry: string;            // REQUIRED (was optional, now required)
  };
  s5Config?: {                        // Optional: S5 storage config
    portalUrl?: string;
    seedPhrase?: string;              // Will be auto-generated if not provided
  };
}
```

**⚠️ IMPORTANT:** The SDK will throw clear errors if any required contract addresses are missing.

**Example (REQUIRED Configuration):**
```typescript
const sdk = new FabstirSDKCore({
  rpcUrl: process.env.NEXT_PUBLIC_RPC_URL_BASE_SEPOLIA!,
  contractAddresses: {
    // ALL 7 REQUIRED - SDK will throw error if any missing
    jobMarketplace: process.env.NEXT_PUBLIC_CONTRACT_JOB_MARKETPLACE!,
    nodeRegistry: process.env.NEXT_PUBLIC_CONTRACT_NODE_REGISTRY!,
    proofSystem: process.env.NEXT_PUBLIC_CONTRACT_PROOF_SYSTEM!,
    hostEarnings: process.env.NEXT_PUBLIC_CONTRACT_HOST_EARNINGS!,
    usdcToken: process.env.NEXT_PUBLIC_CONTRACT_USDC_TOKEN!,
    fabToken: process.env.NEXT_PUBLIC_CONTRACT_FAB_TOKEN!,
    modelRegistry: process.env.NEXT_PUBLIC_CONTRACT_MODEL_REGISTRY!
  }
});
```

**Multi-Chain Configuration Examples:**

```typescript
// Base Sepolia Configuration (Default)
const baseSepolia = new FabstirSDKCore({
  rpcUrl: 'https://sepolia.base.org',
  chainId: 84532, // Optional, this is the default
  contractAddresses: {
    jobMarketplace: '0xaa38e7fcf5d7944ef7c836e8451f3bf93b98364f',
    nodeRegistry: '0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218',
    proofSystem: '0x2ACcc60893872A499700908889B38C5420CBcFD1',
    hostEarnings: '0x908962e8c6CE72610021586f85ebDE09aAc97776',
    usdcToken: '0x036CbD53842c5426634e7929541eC2318f3dCF7e',
    fabToken: '0xC78949004B4EB6dEf2D66e49Cd81231472612D62',
    modelRegistry: '0x92b2De840bB2171203011A6dBA928d855cA8183E'
  }
});

// opBNB Testnet Configuration
const opBNBTestnet = new FabstirSDKCore({
  rpcUrl: 'https://opbnb-testnet-rpc.bnbchain.org',
  chainId: 5611, // Required for opBNB
  contractAddresses: {
    // Note: These are placeholder addresses for opBNB testnet
    jobMarketplace: '0x0000000000000000000000000000000000000001',
    nodeRegistry: '0x0000000000000000000000000000000000000002',
    proofSystem: '0x0000000000000000000000000000000000000003',
    hostEarnings: '0x0000000000000000000000000000000000000004',
    usdcToken: '0x0000000000000000000000000000000000000006',
    modelRegistry: '0x0000000000000000000000000000000000000005',
    fabToken: '0x0000000000000000000000000000000000000007'
  }
});
```

**Current Contract Addresses (Base Sepolia):**
```
JobMarketplace: 0xaa38e7fcf5d7944ef7c836e8451f3bf93b98364f
NodeRegistry: 0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218
ProofSystem: 0x2ACcc60893872A499700908889B38C5420CBcFD1
HostEarnings: 0x908962e8c6CE72610021586f85ebDE09aAc97776
USDCToken: 0x036CbD53842c5426634e7929541eC2318f3dCF7e
FABToken: 0xC78949004B4EB6dEf2D66e49Cd81231472612D62
ModelRegistry: 0x92b2De840bB2171203011A6dBA928d855cA8183E
```

## Authentication

### authenticate

Authenticates the SDK with various providers.

```typescript
// Method 1: Authenticate with private key
async authenticate(privateKey: string): Promise<void>

// Method 2: Authenticate with method name and options
async authenticate(method: string, options: AuthOptions): Promise<void>
```

**Parameters:**
- `privateKey`: Private key string (Method 1)
- `method`: Authentication method - "signer" or "privateKey" (Method 2)
- `options`: Authentication options including signer (Method 2)

**Example 1: Private Key Authentication:**
```typescript
await sdk.authenticate('0x...');
```

**Example 2: Custom Signer Authentication (Base Account Kit):**
```typescript
import { ensureSubAccount, createSubAccountSigner } from '@fabstir/sdk-core';
import { createBaseAccountSDK } from "@base-org/account";

// 1. Setup Base Account Kit
const baseAccountSDK = createBaseAccountSDK({
  appName: "Your App Name",
  appChainIds: [84532], // Base Sepolia
  subAccounts: {
    unstable_enableAutoSpendPermissions: true
  }
});

// 2. Login with passkey (one-time popup)
const result = await baseAccountSDK.loginWithPasskey();
const smartWallet = result.address;
const baseProvider = result.provider;

// 3. Get or create sub-account with spend permissions
const subAccountResult = await ensureSubAccount(
  baseProvider,
  smartWallet as `0x${string}`,
  {
    tokenAddress: process.env.NEXT_PUBLIC_CONTRACT_USDC_TOKEN!,
    tokenDecimals: 6,
    maxAllowance: "1000000",  // $1M max
    periodDays: 365           // Valid for 1 year
  }
);

const subAccount = subAccountResult.address;

// 4. Create custom signer for popup-free transactions
const baseSigner = createSubAccountSigner({
  provider: baseProvider,
  subAccount: subAccount,
  primaryAccount: smartWallet,
  chainId: 84532
});

// 5. Authenticate SDK with custom signer
await sdk.authenticate("signer", {
  signer: baseSigner,
});
```

### Base Account Kit Integration

#### Primary/Sub-Account Model
- **Primary Account**: Smart wallet with passkey authentication, holds main USDC balance
- **Sub-Account**: Session-specific account with spend permissions for popup-free transactions
- **Spend Permissions**: Configured allowances let sub-account pull funds from primary automatically
- **wallet_sendCalls**: EIP-5792 batch transactions enable atomic operations without popups

#### Architecture Benefits
- ✅ **One Popup Only**: Passkey authentication is the only popup required
- ✅ **Popup-Free Transactions**: All approvals, deposits, and session operations work without popups
- ✅ **Automatic Allowances**: Spend permissions auto-configured during sub-account creation
- ✅ **Reusable Sub-Accounts**: Same sub-account works across multiple sessions
- ✅ **SDK Utilities**: `ensureSubAccount()` and `createSubAccountSigner()` handle complexity
- ✅ **Gasless Experience**: No ETH needed for gas fees (Base Account Kit handles gas)

#### How It Works
1. **Initial Setup**: User authenticates once with passkey (creates/accesses smart wallet)
2. **Sub-Account Creation**: SDK creates sub-account with configured spend permissions
3. **Spend Permissions**: Sub-account can pull up to configured amount from primary account
4. **Popup-Free Operations**: All subsequent transactions use `wallet_sendCalls` (no popups)
5. **Atomic Batching**: Multiple operations bundled into single atomic transaction
6. **Session Reuse**: Same sub-account used for future sessions (no re-setup needed)

## Base Account Kit Wallet Utilities

The SDK provides comprehensive utilities for integrating Base Account Kit with popup-free transactions. These utilities simplify sub-account management and custom signer creation.

### ensureSubAccount

Creates or retrieves a sub-account with spend permissions configured.

```typescript
import { ensureSubAccount } from '@fabstir/sdk-core';

async ensureSubAccount(
  provider: any,
  primaryAccount: `0x${string}`,
  options: SubAccountOptions
): Promise<SubAccountResult>
```

**Parameters:**
```typescript
interface SubAccountOptions {
  tokenAddress: string;      // USDC or other ERC20 token
  tokenDecimals: number;      // Token decimals (6 for USDC)
  maxAllowance: string;       // Max allowance amount (e.g., "1000000" for $1M)
  periodDays: number;         // Permission period in days (e.g., 365)
}

interface SubAccountResult {
  address: string;            // Sub-account address
  isExisting: boolean;        // Whether sub-account already existed
}
```

**Example:**
```typescript
import { createBaseAccountSDK } from "@base-org/account";
import { ensureSubAccount } from "@fabstir/sdk-core";

// 1. Setup Base Account Kit
const baseAccountSDK = createBaseAccountSDK({
  appName: "Fabstir LLM",
  appChainIds: [84532],
  subAccounts: {
    unstable_enableAutoSpendPermissions: true
  }
});

const provider = await baseAccountSDK.getProvider();
const accounts = await provider.request({
  method: "eth_requestAccounts",
  params: []
});

const smartWallet = accounts[0]; // Primary account
const contracts = {
  USDC: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
};

// 2. Ensure sub-account exists with spend permissions
const subAccountResult = await ensureSubAccount(provider, smartWallet, {
  tokenAddress: contracts.USDC,
  tokenDecimals: 6,
  maxAllowance: "1000000",  // $1M USDC max
  periodDays: 365           // 1 year permission
});

console.log('Sub-account:', subAccountResult.address);
console.log('Already existed:', subAccountResult.isExisting);
```

**How It Works:**
1. Checks for existing sub-accounts for the domain/origin
2. If exists, returns existing sub-account address
3. If not, creates new sub-account via `wallet_addSubAccount`
4. Configures spend permission with specified token, allowance, and period
5. Returns sub-account address and whether it was newly created

### createSubAccountSigner

Creates a custom ethers-compatible signer that uses `wallet_sendCalls` for popup-free transactions.

```typescript
import { createSubAccountSigner } from '@fabstir/sdk-core';

function createSubAccountSigner(
  options: SubAccountSignerOptions
): Signer
```

**Parameters:**
```typescript
interface SubAccountSignerOptions {
  provider: any;              // Base Account Kit provider
  subAccount: string;         // Sub-account address (from address)
  primaryAccount: string;     // Primary smart wallet (for signatures)
  chainId: number;           // Chain ID for wallet_sendCalls
}
```

**Returns:** Custom signer compatible with ethers.js that:
- Uses `wallet_sendCalls` (EIP-5792) instead of `eth_sendTransaction`
- Polls `wallet_getCallsStatus` for transaction confirmation
- Returns proper `TransactionResponse` objects
- Avoids popup prompts after initial spend permission

**Example:**
```typescript
import { createSubAccountSigner } from "@fabstir/sdk-core";
import { FabstirSDKCore } from "@fabstir/sdk-core";

// Create custom signer
const baseSigner = createSubAccountSigner({
  provider: baseProvider,
  subAccount: subAccountAddress,
  primaryAccount: smartWallet,
  chainId: 84532  // Base Sepolia
});

// Authenticate SDK with custom signer
const sdk = new FabstirSDKCore({ /* config */ });
await sdk.authenticate("signer", {
  signer: baseSigner,
});

// Now all SDK operations use popup-free transactions!
const sessionManager = sdk.getSessionManager();
const { sessionId } = await sessionManager.startSession(/* ... */);
// ✅ No MetaMask popup!
```

**Key Features:**
- **Popup-Free**: Transactions execute without approval prompts
- **Atomic Batching**: Uses EIP-5792 `wallet_sendCalls` with atomic capability
- **Confirmation Polling**: Automatically waits for transaction confirmation
- **Ethers Compatible**: Works seamlessly with all ethers-based code
- **S5 Seed Integration**: Detects cached seeds to avoid signature popups

**Internal Implementation:**
```typescript
// The signer intercepts sendTransaction calls
async sendTransaction(tx: TransactionRequest): Promise<TransactionResponse> {
  // Convert to wallet_sendCalls format
  const calls = [{
    to: tx.to,
    data: tx.data,
    value: tx.value ? `0x${BigInt(tx.value).toString(16)}` : undefined,
  }];

  // Use wallet_sendCalls with sub-account
  const bundleId = await provider.request({
    method: 'wallet_sendCalls',
    params: [{
      version: '2.0.0',
      chainId: CHAIN_HEX,
      from: subAccount,
      calls: calls,
      capabilities: {
        atomic: { required: true },
      },
    }],
  });

  // Poll for confirmation
  const realTxHash = await pollForConfirmation(bundleId);

  // Return proper TransactionResponse
  return await ethersProvider.getTransaction(realTxHash);
}
```

### Complete Popup-Free Flow Example

```typescript
import { FabstirSDKCore, ensureSubAccount, createSubAccountSigner } from "@fabstir/sdk-core";
import { createBaseAccountSDK } from "@base-org/account";

async function setupPopupFreeTransactions() {
  // 1. Initialize SDK
  const sdk = new FabstirSDKCore({
    rpcUrl: process.env.NEXT_PUBLIC_RPC_URL_BASE_SEPOLIA!,
    contractAddresses: { /* all contract addresses */ }
  });

  // 2. Setup Base Account Kit
  const baseAccountSDK = createBaseAccountSDK({
    appName: "Your App",
    appChainIds: [84532],
    subAccounts: {
      unstable_enableAutoSpendPermissions: true  // Enable auto-spend
    }
  });

  const baseProvider = await baseAccountSDK.getProvider();
  const result = await baseProvider.request({
    method: "eth_requestAccounts",
    params: []
  });

  const smartWallet = result[0]; // Primary account with funds

  // 3. Ensure sub-account with spend permissions (SDK utility)
  const contracts = {
    USDC: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
  };

  const subAccountResult = await ensureSubAccount(baseProvider, smartWallet, {
    tokenAddress: contracts.USDC,
    tokenDecimals: 6,
    maxAllowance: "1000000",  // $1M max allowance
    periodDays: 365           // 1 year
  });

  const subAccount = subAccountResult.address;
  console.log('Sub-account ready:', subAccount);

  // 4. Create custom signer (SDK utility)
  const baseSigner = createSubAccountSigner({
    provider: baseProvider,
    subAccount: subAccount,
    primaryAccount: smartWallet,
    chainId: 84532
  });

  // 5. Authenticate SDK with custom signer
  await sdk.authenticate("signer", {
    signer: baseSigner,
  });

  console.log('SDK authenticated with popup-free signer');

  // 6. All operations now work without popups!
  const sessionManager = sdk.getSessionManager();
  const paymentManager = sdk.getPaymentManager();

  // Check allowance (no popup)
  const allowance = await paymentManager.checkAllowance(
    subAccount,
    contracts.JOB_MARKETPLACE,
    contracts.USDC
  );

  // Approve if needed (popup-free!)
  if (allowance < BigInt(1000000)) {
    await paymentManager.approveToken(
      contracts.JOB_MARKETPLACE,
      BigInt(1000000),
      contracts.USDC
    );
    console.log('✅ Approval complete (no popup!)');
  }

  // Start session (popup-free!)
  const { sessionId } = await sessionManager.startSession(
    modelHash,
    hostAddress,
    {
      depositAmount: "1.0",
      pricePerToken: 200,
      duration: 3600,
      proofInterval: 100
    }
  );

  console.log('✅ Session started (no popup!)');

  // Send prompts (popup-free!)
  const response = await sessionManager.sendPrompt(
    sessionId,
    "Tell me a story"
  );

  console.log('Response:', response);

  // Everything works without popups after initial setup! 🎉
}
```

**Benefits of This Approach:**
- ✅ **One-Time Setup**: Only ONE popup for initial spend permission
- ✅ **Persistent Permissions**: Works across browser sessions
- ✅ **Fully Composable**: Works with all SDK methods
- ✅ **Native Integration**: No custom transaction handling needed
- ✅ **Security**: Spend permissions limit risk exposure

## Chain Management

The SDK provides methods to manage and switch between different blockchain networks.

### initialize

Initialize the SDK with a wallet provider for multi-chain support.

```typescript
async initialize(walletProvider: IWalletProvider): Promise<void>
```

**Parameters:**
- `walletProvider`: Wallet provider implementing IWalletProvider interface

**Example:**
```typescript
import { EOAProvider } from '@fabstir/sdk-core';

const provider = new EOAProvider(window.ethereum);
await sdk.initialize(provider);
```

### getCurrentChainId

Returns the current active chain ID.

```typescript
getCurrentChainId(): number
```

**Returns:** Current chain ID (e.g., 84532 for Base Sepolia)

**Example:**
```typescript
const chainId = sdk.getCurrentChainId();
console.log('Current chain:', chainId); // 84532
```

### getCurrentChain

Returns the full configuration for the current chain.

```typescript
getCurrentChain(): ChainConfig
```

**Returns:** Complete chain configuration object

**Example:**
```typescript
const chain = sdk.getCurrentChain();
console.log('Chain name:', chain.name); // "Base Sepolia"
console.log('Native token:', chain.nativeToken); // "ETH"
console.log('Min deposit:', chain.minDeposit); // "0.0002"
```

### switchChain

Switch to a different supported chain.

```typescript
async switchChain(chainId: number): Promise<void>
```

**Parameters:**
- `chainId`: Target chain ID to switch to

**Throws:**
- `UnsupportedChainError`: If chain is not supported
- Error if wallet provider doesn't support chain switching

**Example:**
```typescript
// Switch from Base Sepolia to opBNB testnet
await sdk.switchChain(5611);

// Managers automatically reinitialize for the new chain
const paymentManager = sdk.getPaymentManager();
// Now operates on opBNB
```

## Wallet Providers

The SDK supports multiple wallet provider types through the IWalletProvider interface.

### IWalletProvider Interface

```typescript
interface IWalletProvider {
  // Connection management
  connect(chainId?: number): Promise<void>;
  disconnect(): Promise<void>;
  isConnected(): boolean;

  // Account management
  getAddress(): Promise<string>;
  getDepositAccount(): Promise<string>;

  // Chain management
  getCurrentChainId(): Promise<number>;
  switchChain(chainId: number): Promise<void>;
  getSupportedChains(): number[];

  // Transactions
  sendTransaction(tx: TransactionRequest): Promise<TransactionResponse>;
  signMessage(message: string): Promise<string>;

  // Capabilities
  getCapabilities(): WalletCapabilities;
}
```

### EOAProvider (MetaMask/Rainbow)

Standard Ethereum wallet provider for browser extensions.

```typescript
import { EOAProvider } from '@fabstir/sdk-core';

const provider = new EOAProvider(window.ethereum);
await provider.connect(84532); // Connect to Base Sepolia

const capabilities = provider.getCapabilities();
// {
//   supportsGaslessTransactions: false,
//   supportsChainSwitching: true,
//   supportsSmartAccounts: false,
//   requiresDepositAccount: false
// }
```

### SmartAccountProvider

Smart contract wallet provider with gasless transaction support.

```typescript
import { SmartAccountProvider } from '@fabstir/sdk-core';

const provider = new SmartAccountProvider({
  bundlerUrl: 'https://bundler.base.org',
  paymasterUrl: 'https://paymaster.base.org'
});

await provider.connect();

const capabilities = provider.getCapabilities();
// {
//   supportsGaslessTransactions: true,
//   supportsChainSwitching: false,
//   supportsSmartAccounts: true,
//   requiresDepositAccount: true
// }
```

### WalletProviderFactory

Factory for auto-detecting and creating wallet providers.

```typescript
import { WalletProviderFactory } from '@fabstir/sdk-core';

// Auto-detect available provider
const provider = await WalletProviderFactory.createProvider('eoa');

// Or specify type
const eoaProvider = await WalletProviderFactory.createProvider('eoa', window.ethereum);
const smartProvider = await WalletProviderFactory.createProvider('smart-account', config);
```

## Session Management

The SessionManager handles LLM session lifecycle, streaming responses, and context preservation.

### Get SessionManager

```typescript
const sessionManager = sdk.getSessionManager();
```

### startSession

Creates a new LLM session with blockchain job creation.

```typescript
async startSession(
  model: string,
  provider: string,
  config: SessionConfig
): Promise<{
  sessionId: bigint;
  jobId: bigint;
}>
```

**Parameters:**
```typescript
interface SessionConfig {
  depositAmount: string;     // USDC amount as string (e.g., "1.0" for $1)
  pricePerToken: number;     // Price per token (e.g., 200)
  duration: number;          // Session duration in seconds (e.g., 3600)
  proofInterval: number;     // Checkpoint interval in tokens (e.g., 100)
  encryption?: boolean;      // Enable end-to-end encryption (default: true)
```

**Example:**
```typescript
const { sessionId, jobId } = await sessionManager.startSession(
  '0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced', // Model hash
  '0x4594F755F593B517Bb3194F4DeC20C48a3f04504', // Provider address
  {
    depositAmount: "1.0",    // $1 USDC minimum per session
    pricePerToken: 200,      // 0.0002 USDC per token (0.02 cents)
    duration: 3600,          // 1 hour session timeout
    proofInterval: 100,      // Checkpoint every 100 tokens
    encryption: true         // Enable E2EE (default, can be omitted)
  },
  'http://localhost:8080'   // Optional: Host WebSocket endpoint
);
```

**Encryption Notes:**
- **Default Behavior**: Sessions use end-to-end encryption by default (Phase 6.2)
- **Zero Cost**: Encryption is client-side with negligible performance impact (~1-2ms per message)
- **Privacy First**: All messages and session data encrypted using ephemeral-static ECDH + XChaCha20-Poly1305
- **Opt-Out**: Set `encryption: false` to explicitly disable (for debugging/testing only)

```typescript
// Explicitly disable encryption (not recommended)
const { sessionId } = await sessionManager.startSession(model, provider, {
  ...config,
  encryption: false  // Opt-out of encryption
});
```

### sendPrompt

Sends a prompt to the LLM and receives response.

```typescript
async sendPrompt(
  sessionId: bigint,
  prompt: string
): Promise<string>
```

**Example:**
```typescript
const response = await sessionManager.sendPrompt(
  sessionId,
  "What is the capital of France?"
);
```

### sendPromptStreaming

Sends a prompt and receives streaming response via WebSocket.

```typescript
async sendPromptStreaming(
  sessionId: bigint,
  prompt: string,
  onToken?: (token: string) => void
): Promise<string>
```

**Example:**
```typescript
const response = await sessionManager.sendPromptStreaming(
  sessionId,
  "Tell me a story",
  (token) => {
    // Handle each token as it arrives
    process.stdout.write(token);
  }
);
console.log('\nFull response:', response);
```

### submitCheckpoint

Submits a checkpoint proof for token usage.

```typescript
async submitCheckpoint(
  sessionId: bigint,
  proof: CheckpointProof
): Promise<string>
```

**Parameters:**
```typescript
interface CheckpointProof {
  checkpointNumber: number;
  tokensUsed: number;
  proofData: string;        // 64-byte proof minimum
  timestamp: number;
}
```

### completeSession

Completes a session and triggers payment distribution.

```typescript
async completeSession(
  sessionId: bigint,
  totalTokens: number,
  finalProof: string
): Promise<string>
```

### getSessionHistory

Retrieves conversation history for a session.

```typescript
async getSessionHistory(
  sessionId: bigint
): Promise<{
  prompts: string[];
  responses: string[];
  timestamps: number[];
  tokenCounts: number[];
}>
```

### resumeSession

Resumes a paused session.

```typescript
async resumeSession(sessionId: bigint): Promise<void>
```

### pauseSession

Pauses an active session.

```typescript
async pauseSession(sessionId: bigint): Promise<void>
```

## Payment Management

Handles ETH and USDC payments for jobs.

### Get PaymentManager

```typescript
const paymentManager = sdk.getPaymentManager();
```

### createSessionJobWithUSDC

Creates a session job with USDC payment.

```typescript
async createSessionJobWithUSDC(
  provider: string,
  amount: string,         // Amount in USDC (e.g., "2" for $2)
  config: {
    pricePerToken: number;
    duration: number;
    proofInterval: number;
  }
): Promise<{
    sessionId: bigint;
    txHash: string;
}>
```

**Example:**
```typescript
const result = await paymentManager.createSessionJobWithUSDC(
  '0x4594F755F593B517Bb3194F4DeC20C48a3f04504',
  '2', // $2 USDC
  {
    pricePerToken: 200,
    duration: 3600,
    proofInterval: 100
  }
);
```

### createSessionJobWithETH

Creates a session job with ETH payment.

```typescript
async createSessionJobWithETH(
  provider: string,
  amount: string,         // Amount in ETH (e.g., "0.001")
  config: {
    pricePerToken: number;
    duration: number;
    proofInterval: number;
  }
): Promise<{
    sessionId: bigint;
    txHash: string;
}>
```

### Token Balance and Allowance Methods

The PaymentManager provides utility methods for reading token balances and managing approvals without requiring direct contract interactions.

#### getTokenBalance

Gets ERC20 token balance for an address.

```typescript
async getTokenBalance(
  address: string,
  tokenAddress: string
): Promise<bigint>
```

**Parameters:**
- `address`: Address to check balance for
- `tokenAddress`: ERC20 token contract address

**Returns:** Token balance as bigint (in token's smallest unit)

**Example:**
```typescript
const usdcAddress = process.env.NEXT_PUBLIC_CONTRACT_USDC_TOKEN!;
const balance = await paymentManager.getTokenBalance(userAddress, usdcAddress);
console.log(`USDC Balance: $${ethers.formatUnits(balance, 6)}`);
```

#### getNativeBalance

Gets native token (ETH/BNB) balance for an address.

```typescript
async getNativeBalance(address: string): Promise<bigint>
```

**Parameters:**
- `address`: Address to check balance for

**Returns:** Native balance as bigint (in wei)

**Example:**
```typescript
const ethBalance = await paymentManager.getNativeBalance(userAddress);
console.log(`ETH Balance: ${ethers.formatEther(ethBalance)}`);
```

#### checkAllowance

Checks ERC20 token allowance for a spender.

```typescript
async checkAllowance(
  owner: string,
  spender: string,
  tokenAddress: string
): Promise<bigint>
```

**Parameters:**
- `owner`: Token owner address
- `spender`: Spender address (e.g., JobMarketplace contract)
- `tokenAddress`: ERC20 token contract address

**Returns:** Allowance amount as bigint (in token's smallest unit)

**Example:**
```typescript
const usdcAddress = process.env.NEXT_PUBLIC_CONTRACT_USDC_TOKEN!;
const marketplaceAddress = sdk.contractManager.getAddress('jobMarketplace');

const allowance = await paymentManager.checkAllowance(
  userAddress,
  marketplaceAddress,
  usdcAddress
);

if (allowance < ethers.parseUnits("10.0", 6)) {
  console.log('Need to approve more USDC');
}
```

#### approveToken (Token Approval)

Approves ERC20 token spending for a spender.

```typescript
async approveToken(
  spender: string,
  amount: bigint,
  tokenAddress: string
): Promise<ethers.TransactionReceipt>
```

**Parameters:**
- `spender`: Spender address (e.g., JobMarketplace contract)
- `amount`: Amount to approve as bigint (in token's smallest unit)
- `tokenAddress`: ERC20 token contract address

**Returns:** Transaction receipt

**Example:**
```typescript
const usdcAddress = process.env.NEXT_PUBLIC_CONTRACT_USDC_TOKEN!;
const marketplaceAddress = sdk.contractManager.getAddress('jobMarketplace');

// Approve $100 USDC
const receipt = await paymentManager.approveToken(
  marketplaceAddress,
  ethers.parseUnits("100.0", 6),
  usdcAddress
);

console.log('Approval confirmed:', receipt.transactionHash);
```

#### sendToken

Sends ERC20 tokens to another address.

```typescript
async sendToken(
  to: string,
  amount: bigint,
  tokenAddress: string
): Promise<ethers.TransactionReceipt>
```

**Parameters:**
- `to`: Recipient address
- `amount`: Amount to send as bigint (in token's smallest unit)
- `tokenAddress`: ERC20 token contract address

**Returns:** Transaction receipt

**Example:**
```typescript
const usdcAddress = process.env.NEXT_PUBLIC_CONTRACT_USDC_TOKEN!;
const recipientAddress = '0x123...';

// Send $50 USDC
const receipt = await paymentManager.sendToken(
  recipientAddress,
  ethers.parseUnits("50.0", 6),
  usdcAddress
);

console.log('Transfer confirmed:', receipt.transactionHash);
```

**Complete Balance & Approval Flow:**
```typescript
// 1. Check current token balance
const usdcAddress = process.env.NEXT_PUBLIC_CONTRACT_USDC_TOKEN!;
const balance = await paymentManager.getTokenBalance(userAddress, usdcAddress);
console.log('Balance:', ethers.formatUnits(balance, 6), 'USDC');

// 2. Check current allowance
const marketplaceAddress = sdk.contractManager.getAddress('jobMarketplace');
const currentAllowance = await paymentManager.checkAllowance(
  userAddress,
  marketplaceAddress,
  usdcAddress
);

// 3. Approve if needed
const requiredAmount = ethers.parseUnits("10.0", 6);
if (currentAllowance < requiredAmount) {
  console.log('Approving USDC...');
  const receipt = await paymentManager.approveToken(
    marketplaceAddress,
    requiredAmount,
    usdcAddress
  );
  console.log('Approved:', receipt.transactionHash);
}

// 4. Send tokens if needed
const recipientAddress = '0x...';
const sendAmount = ethers.parseUnits("5.0", 6);
const receipt = await paymentManager.sendToken(
  recipientAddress,
  sendAmount,
  usdcAddress
);
console.log('Sent:', receipt.transactionHash);

// 5. Create session (will use approved tokens)
const { sessionId } = await sessionManager.startSession(
  modelHash,
  hostAddress,
  {
    depositAmount: "1.0",
    pricePerToken: 200,
    duration: 3600,
    proofInterval: 100
  }
);
```

### depositNative

Deposits native tokens (ETH/BNB) for gasless session operations.

```typescript
async depositNative(
  amount: string,
  chainId?: number
): Promise<TransactionResponse>
```

**Parameters:**
- `amount`: Amount to deposit in native token (e.g., "0.001" for 0.001 ETH/BNB)
- `chainId`: Optional chain ID (uses current chain if not specified)

**Example:**
```typescript
// Deposit ETH on Base Sepolia
const tx = await paymentManager.depositNative("0.001");
await tx.wait(3);

// Deposit BNB on opBNB
await sdk.switchChain(5611);
const bnbTx = await paymentManager.depositNative("0.002");
await bnbTx.wait(3);
```

### withdrawNative

Withdraws native tokens from deposited balance.

```typescript
async withdrawNative(
  amount: string,
  chainId?: number
): Promise<TransactionResponse>
```

**Parameters:**
- `amount`: Amount to withdraw in native token
- `chainId`: Optional chain ID (uses current chain if not specified)

**Example:**
```typescript
// Withdraw ETH from Base Sepolia
const tx = await paymentManager.withdrawNative("0.0005");
await tx.wait(3);
```

### getDepositBalance

Gets the deposited balance for the current account.

```typescript
async getDepositBalance(
  chainId?: number
): Promise<{
  native: string;
  usdc: string;
}>
```

**Returns:** Balance object with native and USDC amounts

**Example:**
```typescript
const balances = await paymentManager.getDepositBalance();
console.log('ETH deposited:', balances.native);
console.log('USDC deposited:', balances.usdc);
```

### Chain-Specific Minimum Deposits

Different chains have different minimum deposit requirements:

| Chain | Minimum Deposit | Native Token |
|-------|----------------|--------------|
| Base Sepolia | 0.0002 ETH | ETH |
| opBNB Testnet | 0.001 BNB | BNB |

### Payment Distribution Model

The SDK implements a transparent payment distribution system:

#### Deposit Model
- **Minimum Deposit**: $1.00 USDC per session (reduced from $2.00)
- **Actual Usage**: Typically $0.02-0.03 per session
- **Refunds**: Unused funds remain in sub-account for future sessions
- **Auto-reuse**: Subsequent sessions use existing balance (no new deposit needed)

#### Distribution Split
When a session ends and checkpoint is submitted:
- **Host (Provider)**: Receives 90% of consumed tokens value
- **Treasury**: Receives 10% as platform fee
- **User**: Gets refund of unused deposit to sub-account

#### Example Payment Flow
```typescript
// User deposits $1.00 for session
// Session uses 150 tokens at 0.0002 USDC per token = $0.03
// Distribution:
// - Host receives: $0.027 (90% of $0.03)
// - Treasury receives: $0.003 (10% of $0.03)
// - User refund: $0.97 (stays in sub-account)
// User can run ~32 more sessions without new deposit
```

#### Checkpoint Submission
```typescript
async submitCheckpoint(
  sessionId: bigint,
  tokensGenerated: number
): Promise<string>
```

**Important Notes:**
- Minimum 100 tokens must be submitted per checkpoint
- 5-second wait required before submission (ProofSystem rate limit)
- Checkpoint triggers automatic payment distribution

## Model Governance

The ModelManager handles model validation and governance.

### Get ModelManager

```typescript
const modelManager = sdk.getModelManager();
```

**Note:** As of the latest SDK update, ModelManager is now directly accessible via the `getModelManager()` getter method on the SDK instance. This requires prior authentication.

### getModelId

Generates deterministic model ID from Hugging Face repo and file.

```typescript
async getModelId(
  huggingfaceRepo: string,
  fileName: string
): Promise<string>
```

**Example:**
```typescript
const modelId = await modelManager.getModelId(
  'meta-llama/Llama-2-7b-hf',
  'model.safetensors'
);
```

### isModelApproved

Checks if a model is approved for use.

```typescript
async isModelApproved(modelId: string): Promise<boolean>
```

### getModelDetails

Gets detailed information about a model.

```typescript
async getModelDetails(modelId: string): Promise<ModelInfo | null>
```

**Returns:**
```typescript
interface ModelInfo {
  id: string;
  huggingfaceRepo: string;
  fileName: string;
  modelHash: string;
  size: bigint;
  approved: boolean;
  metadata?: {
    name?: string;
    description?: string;
    tags?: string[];
  };
}
```

### getAllApprovedModels

Gets all approved models in the registry.

```typescript
async getAllApprovedModels(): Promise<ModelInfo[]>
```

### validateModel

Validates a model specification and optionally verifies file hash.

```typescript
async validateModel(
  modelSpec: ModelSpec,
  fileContent?: ArrayBuffer
): Promise<ModelValidation>
```

**Parameters:**
```typescript
interface ModelSpec {
  huggingfaceRepo: string;
  fileName: string;
  modelHash: string;
}

interface ModelValidation {
  isValid: boolean;
  isApproved: boolean;
  hashMatches?: boolean;
  errors: string[];
}
```

### verifyModelHash

Verifies a model file's hash.

```typescript
async verifyModelHash(
  fileContent: ArrayBuffer,
  expectedHash: string
): Promise<boolean>
```

## Host Management

The HostManager provides comprehensive host management with model governance support.

### Get HostManager

```typescript
const hostManager = sdk.getHostManager();
```

### registerHostWithModels

Registers a host with supported models.

```typescript
async registerHostWithModels(
  request: HostRegistrationWithModels
): Promise<string>
```

**Parameters:**
```typescript
interface HostRegistrationWithModels {
  metadata: HostMetadata;
  supportedModels: ModelSpec[];
  stake?: string;                     // Optional stake amount
  apiUrl?: string;                    // Host API endpoint
  minPricePerTokenNative: string;     // Minimum price for native token payments (wei)
  minPricePerTokenStable: string;     // Minimum price for stablecoin payments (raw USDC)
}

interface HostMetadata {
  hardware: {
    gpu: string;
    vram: number;
    ram: number;
  };
  capabilities: string[];   // e.g., ['streaming', 'batch']
  location: string;
  maxConcurrent: number;
  costPerToken: number;
}
```

**Pricing Ranges:**
- **Native Token (ETH/BNB):**
  - MIN: `2272727273` wei (~$0.00001 @ $4400 ETH)
  - MAX: `22727272727273` wei (~$0.1 @ $4400 ETH)
  - DEFAULT: `11363636363636` wei (~$0.00005 @ $4400 ETH)
  - Range: 10,000x

- **Stablecoin (USDC):**
  - MIN: `10` (0.00001 USDC per token)
  - MAX: `100000` (0.1 USDC per token)
  - DEFAULT: `316` (0.000316 USDC per token)
  - Range: 10,000x

**Example:**
```typescript
await hostManager.registerHostWithModels({
  apiUrl: 'http://localhost:8080',
  supportedModels: ['model-hash-here'],
  metadata: {
    hardware: { gpu: 'RTX 4090', vram: 24, ram: 64 },
    capabilities: ['inference', 'streaming'],
    location: 'us-west',
    maxConcurrent: 10,
    costPerToken: 0.000316
  },
  minPricePerTokenNative: '11363636363636',  // ~$0.00005 @ $4400 ETH
  minPricePerTokenStable: '316'              // 0.000316 USDC
});
```

### findHostsForModel

Finds hosts that support a specific model.

```typescript
async findHostsForModel(modelId: string): Promise<HostInfo[]>
```

**Returns:**
```typescript
interface HostInfo {
  address: string;
  metadata: HostMetadata;
  supportedModels: string[];
  isActive: boolean;
  stake: bigint;
  reputation: number;
  apiUrl?: string;
  minPricePerTokenNative: bigint;   // Minimum price for native token (wei)
  minPricePerTokenStable: bigint;   // Minimum price for stablecoins (raw USDC)
}
```

### updateHostModels

Updates the models supported by the current host.

```typescript
async updateHostModels(newModels: ModelSpec[]): Promise<string>
```

### getHostStatus

Gets comprehensive status of a host.

```typescript
async getHostStatus(hostAddress: string): Promise<{
  isRegistered: boolean;
  isActive: boolean;
  supportedModels: string[];
  stake: bigint;
  metadata?: HostMetadata;
  apiUrl?: string;
  minPricePerTokenNative: bigint;   // Minimum price for native token (wei)
  minPricePerTokenStable: bigint;   // Minimum price for stablecoins (raw USDC)
}>
```

**Note:** This method returns registration, model information, and dual pricing. To get earnings, use `getHostEarnings()` separately.

### discoverAllActiveHostsWithModels

Discovers all active hosts with their supported models.

```typescript
async discoverAllActiveHostsWithModels(): Promise<HostInfo[]>
```

### hostSupportsModel

Checks if a host supports a specific model.

```typescript
async hostSupportsModel(
  hostAddress: string,
  modelId: string
): Promise<boolean>
```

### updateApiUrl

Updates the host's API endpoint URL.

```typescript
async updateApiUrl(apiUrl: string): Promise<string>
```

### getHostEarnings

Gets accumulated earnings for a specific host and token.

```typescript
async getHostEarnings(
  hostAddress: string,
  tokenAddress: string
): Promise<bigint>
```

**Parameters:**
- `hostAddress`: Host address to check earnings for
- `tokenAddress`: Token address (use `ethers.ZeroAddress` or `'0x0000000000000000000000000000000000000000'` for native ETH/BNB)

**Returns:** Accumulated earnings as bigint (in token's smallest unit)

**Example:**
```typescript
const hostManager = sdk.getHostManager();

// Get ETH earnings
const ETH_ADDRESS = ethers.ZeroAddress;
const ethEarnings = await hostManager.getHostEarnings(hostAddress, ETH_ADDRESS);
console.log('ETH Earnings:', ethers.formatEther(ethEarnings));

// Get USDC earnings
const usdcAddress = process.env.NEXT_PUBLIC_CONTRACT_USDC_TOKEN!;
const usdcEarnings = await hostManager.getHostEarnings(hostAddress, usdcAddress);
console.log('USDC Earnings:', ethers.formatUnits(usdcEarnings, 6));
```

### withdrawEarnings

Withdraws accumulated earnings for a host.

```typescript
async withdrawEarnings(tokenAddress: string): Promise<string>
```

### updatePricingNative

Updates the native token pricing for the current host.

```typescript
async updatePricingNative(newPrice: string): Promise<string>
```

**Parameters:**
- `newPrice`: New minimum price in wei (string)
- Must be within range: `2272727273` to `22727272727273`

**Example:**
```typescript
const hostManager = sdk.getHostManager();

// Update native pricing to ~$0.00007 @ $4400 ETH
const txHash = await hostManager.updatePricingNative('15909090909091');
console.log('Native pricing updated:', txHash);
```

### updatePricingStable

Updates the stablecoin pricing for the current host.

```typescript
async updatePricingStable(newPrice: string): Promise<string>
```

**Parameters:**
- `newPrice`: New minimum price in raw USDC units (string)
- Must be within range: `10` to `100000`

**Example:**
```typescript
const hostManager = sdk.getHostManager();

// Update stable pricing to 0.0005 USDC
const txHash = await hostManager.updatePricingStable('500');
console.log('Stable pricing updated:', txHash);
```

## Storage Management

Handles S5 decentralized storage operations with deterministic seed generation.

### Get StorageManager

```typescript
const storageManager = await sdk.getStorageManager();
```

**Note:** S5 seed phrases are now automatically generated deterministically from your wallet signature. The SDK:
- Generates a unique 15-word seed phrase per wallet
- Caches the seed in localStorage for performance
- No longer requires manual seed phrase configuration

### storeConversation

Stores a conversation in S5.

```typescript
async storeConversation(
  sessionId: string,
  messages: Array<{
    role: 'user' | 'assistant' | 'system';
    content: string;
    timestamp: number;
  }>
): Promise<string>
```

### retrieveConversation

Retrieves a stored conversation.

```typescript
async retrieveConversation(sessionId: string): Promise<{
  messages: Array<{
    role: string;
    content: string;
    timestamp: number;
  }>;
  metadata?: any;
}>
```

### storeSessionMetadata

Stores session metadata.

```typescript
async storeSessionMetadata(
  sessionId: string,
  metadata: {
    model: string;
    provider: string;
    startTime: number;
    config: SessionConfig;
  }
): Promise<string>
```

### User Settings Storage

The StorageManager provides persistent user settings storage via S5 decentralized storage. Settings sync automatically across devices and are cached in-memory for performance.

**Features:**
- **Cross-device synchronization** - Settings stored on S5 are accessible from any device
- **In-memory caching** - 5-minute TTL reduces S5 reads and improves performance
- **Offline mode support** - Returns stale cache when network is unavailable
- **Schema versioning** - Automatic migration system for future schema changes
- **Last-write-wins** - Simple conflict resolution for concurrent updates

#### saveUserSettings(settings: UserSettings): Promise<void>

Save complete user settings object to S5 storage. This overwrites any existing settings.

**Parameters:**
- `settings` (UserSettings) - Complete settings object with all required fields

**Required Fields:**
- `version` - Schema version (use `UserSettingsVersion.V1`)
- `lastUpdated` - Unix timestamp in milliseconds
- `selectedModel` - Currently selected model name

**Throws:**
- `SDKError` with code `STORAGE_NOT_INITIALIZED` - StorageManager not initialized
- `SDKError` with code `INVALID_SETTINGS` - Missing required fields (version, lastUpdated)
- `SDKError` with code `STORAGE_SAVE_ERROR` - Failed to save to S5

**Cache Behavior:**
- Updates in-memory cache immediately after successful save
- Cache is valid for 5 minutes

**Example:**
```typescript
import { UserSettings, UserSettingsVersion } from '@fabstir/sdk-core';

const storageManager = await sdk.getStorageManager();

const settings: UserSettings = {
  version: UserSettingsVersion.V1,
  lastUpdated: Date.now(),
  selectedModel: 'tiny-vicuna-1b.q4_k_m.gguf',
  preferredPaymentToken: 'USDC',
  theme: 'dark',
  lastUsedModels: ['tiny-vicuna-1b.q4_k_m.gguf', 'mistral-7b.q4_k_m.gguf'],
  advancedSettingsExpanded: false
};

try {
  await storageManager.saveUserSettings(settings);
  console.log('Settings saved successfully');
} catch (error) {
  if (error.code === 'STORAGE_SAVE_ERROR') {
    console.error('Failed to save settings:', error.message);
  }
}
```

#### getUserSettings(): Promise<UserSettings | null>

Load user settings from S5 storage. Returns cached value if available within 5-minute TTL.

**Returns:**
- `UserSettings` object if settings exist
- `null` if no settings found (first-time user)

**Throws:**
- `SDKError` with code `STORAGE_NOT_INITIALIZED` - StorageManager not initialized
- `SDKError` with code `INVALID_SETTINGS_STRUCTURE` - Settings schema is invalid or migration failed
- `SDKError` with code `STORAGE_LOAD_ERROR` - Failed to load from S5 and no cache available

**Cache Behavior:**
- Returns cached value if age < 5 minutes
- Fetches from S5 if cache expired or missing
- Caches result (including null for first-time users)

**Offline Mode:**
- Returns stale cache on network errors (even if expired)
- Throws error only if no cache available

**Example:**
```typescript
const storageManager = await sdk.getStorageManager();

try {
  const settings = await storageManager.getUserSettings();

  if (settings) {
    console.log('Last used model:', settings.selectedModel);
    console.log('Theme:', settings.theme || 'default');
    console.log('Payment token:', settings.preferredPaymentToken || 'not set');
  } else {
    console.log('First-time user, no settings found');
    // Initialize with defaults
    await storageManager.saveUserSettings({
      version: UserSettingsVersion.V1,
      lastUpdated: Date.now(),
      selectedModel: 'tiny-vicuna-1b.q4_k_m.gguf', // Default model
      theme: 'auto'
    });
  }
} catch (error) {
  if (error.code === 'INVALID_SETTINGS_STRUCTURE') {
    console.error('Settings corrupted, resetting:', error.message);
    await storageManager.clearUserSettings();
  }
}
```

#### updateUserSettings(partial: PartialUserSettings): Promise<void>

Update specific settings without overwriting the entire object. Merges partial update with existing settings.

**Parameters:**
- `partial` (PartialUserSettings) - Partial settings object (version and lastUpdated excluded)

**Behavior:**
- If settings exist: Merges partial with current settings
- If no settings exist: Creates new settings with partial values
- Always updates `lastUpdated` timestamp
- Preserves `version` field

**Throws:**
- `SDKError` with code `STORAGE_NOT_INITIALIZED` - StorageManager not initialized
- `SDKError` with code `STORAGE_UPDATE_ERROR` - Failed to update settings

**Example:**
```typescript
const storageManager = await sdk.getStorageManager();

// Update only model preference
await storageManager.updateUserSettings({
  selectedModel: 'mistral-7b.q4_k_m.gguf'
});

// Update multiple fields
await storageManager.updateUserSettings({
  theme: 'dark',
  preferredPaymentToken: 'ETH',
  advancedSettingsExpanded: true
});

// Add to recently used models list
const settings = await storageManager.getUserSettings();
const recentModels = settings?.lastUsedModels || [];
await storageManager.updateUserSettings({
  lastUsedModels: [...new Set([newModel, ...recentModels])].slice(0, 5)
});
```

#### clearUserSettings(): Promise<void>

Delete all user settings from S5 storage. Used for "Reset Preferences" functionality.

**Behavior:**
- Deletes settings file from S5
- Invalidates in-memory cache
- No error if settings don't exist

**Throws:**
- `SDKError` with code `STORAGE_NOT_INITIALIZED` - StorageManager not initialized
- `SDKError` with code `STORAGE_CLEAR_ERROR` - Failed to clear settings

**Example:**
```typescript
const storageManager = await sdk.getStorageManager();

// Reset all preferences
try {
  await storageManager.clearUserSettings();
  console.log('Settings reset successfully');

  // Settings are now null
  const settings = await storageManager.getUserSettings();
  console.log(settings); // null
} catch (error) {
  console.error('Failed to reset settings:', error.message);
}
```

#### Cache Behavior

User settings use an in-memory cache with 5-minute TTL for optimal performance:

**Cache Lifecycle:**
1. **First load** - Fetches from S5, caches result
2. **Subsequent loads** - Returns cached value if age < 5 minutes
3. **After 5 minutes** - Cache expired, fetches from S5
4. **After save** - Cache updated immediately
5. **After clear** - Cache invalidated (set to null)

**Cross-Device Sync:**
- Device A saves settings → S5 updated immediately
- Device B reads settings → May see stale cache for up to 5 minutes
- After cache expires → Device B sees Device A's changes

**Manual Cache Bypass:**
```typescript
// Clear cache to force reload from S5
await storageManager.clearUserSettings();
await storageManager.saveUserSettings(newSettings); // This also updates cache
```

#### Offline Mode

The SDK gracefully handles offline scenarios:

**Network Error Handling:**
```typescript
try {
  const settings = await storageManager.getUserSettings();
  // Success: either from cache or S5
} catch (error) {
  // Only throws if network error AND no cache available
  console.error('Offline and no cached settings');
}
```

**Behavior:**
- Network available → Fetch from S5, update cache
- Network error + cache available → Return stale cache (warn in console)
- Network error + no cache → Throw STORAGE_LOAD_ERROR

**Detected Network Errors:**
- Contains "network"
- Contains "timeout"
- Contains "Connection refused"
- Contains "ECONNREFUSED"

#### Schema Versioning

The SDK supports schema migrations for future UserSettings versions:

**Current Version:**
```typescript
export enum UserSettingsVersion {
  V1 = 1  // Initial version
}
```

**Migration System:**
- Automatic migration on load via `getUserSettings()`
- Migrations are transparent to the application
- Always migrates to latest version
- Migration failures throw `INVALID_SETTINGS_STRUCTURE`

**Future Migration Example (V1 → V2):**
```typescript
// If V2 adds new fields, migration automatically:
// 1. Detects V1 settings
// 2. Adds new V2 fields with defaults
// 3. Updates version to V2
// 4. Returns migrated settings
```

**Handling Migration Errors:**
```typescript
try {
  const settings = await storageManager.getUserSettings();
} catch (error) {
  if (error.code === 'INVALID_SETTINGS_STRUCTURE') {
    // Corrupt or unsupported version
    console.error('Settings migration failed:', error.message);

    // Reset to defaults
    await storageManager.clearUserSettings();
    await storageManager.saveUserSettings({
      version: UserSettingsVersion.V1,
      lastUpdated: Date.now(),
      selectedModel: 'default-model'
    });
  }
}
```

### Encrypted Storage (Phase 5.3)

The SDK provides convenience methods for encrypted conversation storage with end-to-end encryption. Conversations can be encrypted with the host's public key and stored on S5 decentralized storage.

**Features:**
- **End-to-end encryption** - Conversations encrypted with host's public key
- **Automatic decryption** - SDK handles decryption transparently on load
- **Backward compatible** - Falls back to plaintext for non-encrypted conversations
- **Sender verification** - ECDSA signatures allow conversation ownership verification
- **Metadata tracking** - Tracks encryption status, version, and timestamps

#### saveConversation(conversation, options?): Promise<string>

Save a conversation with optional encryption to S5 storage.

**Parameters:**
- `conversation` (ConversationData) - Conversation data to save
- `options` (optional) - Encryption options
  - `hostPubKey` (string) - Host's public key for encryption (required if encrypt=true)
  - `encrypt` (boolean) - Whether to encrypt the conversation (default: false)

**Returns:**
- `Promise<string>` - CID (Content Identifier) of stored conversation

**Throws:**
- `SDKError` with code `STORAGE_NOT_INITIALIZED` - StorageManager not initialized
- `SDKError` with code `INVALID_HOST_PUBLIC_KEY` - Invalid or missing host public key when encrypt=true
- `SDKError` with code `ENCRYPTION_ERROR` - Failed to encrypt conversation
- `SDKError` with code `STORAGE_SAVE_ERROR` - Failed to save to S5

**Example (Plaintext Storage):**
```typescript
const sdk = new FabstirSDKCore({ /* config */ });
await sdk.authenticate('privatekey', { privateKey });

// Save conversation without encryption
const conversation = {
  sessionId: 'sess-123',
  messages: [
    { role: 'user', content: 'Hello!', timestamp: Date.now() },
    { role: 'assistant', content: 'Hi there!', timestamp: Date.now() }
  ],
  metadata: {
    model: 'llama-3',
    startTime: Date.now()
  }
};

const cid = await sdk.saveConversation(conversation);
console.log('Conversation saved:', cid);
```

**Example (Encrypted Storage):**
```typescript
const sdk = new FabstirSDKCore({ /* config */ });
await sdk.authenticate('privatekey', { privateKey });

// 1. Get host's public key
const hostAddress = '0x1234...';
const hostPubKey = await sdk.getHostPublicKey(hostAddress);

// 2. Save with encryption
const conversation = {
  sessionId: 'sess-123',
  messages: [
    { role: 'user', content: 'Sensitive message', timestamp: Date.now() },
    { role: 'assistant', content: 'Response', timestamp: Date.now() }
  ]
};

const cid = await sdk.saveConversation(conversation, {
  hostPubKey,
  encrypt: true
});

console.log('Encrypted conversation saved:', cid);
```

#### loadConversation(conversationId): Promise<ConversationData>

Load a conversation from S5 storage with automatic decryption.

**Parameters:**
- `conversationId` (string) - Conversation ID or CID to load

**Returns:**
- `Promise<ConversationData>` - Decrypted conversation data

**Throws:**
- `SDKError` with code `STORAGE_NOT_INITIALIZED` - StorageManager not initialized
- `SDKError` with code `CONVERSATION_NOT_FOUND` - Conversation does not exist
- `SDKError` with code `DECRYPTION_ERROR` - Failed to decrypt (wrong key or corrupted data)
- `SDKError` with code `STORAGE_LOAD_ERROR` - Failed to load from S5

**Behavior:**
- Automatically detects encrypted vs plaintext conversations
- Decrypts using client's private key (if encrypted)
- Falls back to plaintext if decryption fails
- Verifies sender signature if metadata present

**Example:**
```typescript
const sdk = new FabstirSDKCore({ /* config */ });
await sdk.authenticate('privatekey', { privateKey });

// Load conversation (handles both encrypted and plaintext)
try {
  const conversation = await sdk.loadConversation('conv-123');

  console.log('Session ID:', conversation.sessionId);
  console.log('Messages:', conversation.messages.length);
  console.log('Metadata:', conversation.metadata);
} catch (error) {
  if (error.code === 'CONVERSATION_NOT_FOUND') {
    console.error('Conversation does not exist');
  } else if (error.code === 'DECRYPTION_ERROR') {
    console.error('Cannot decrypt - wrong key or corrupted');
  }
}
```

#### getHostPublicKey(hostAddress): Promise<string>

Get the public key of a registered host for encryption.

**Parameters:**
- `hostAddress` (string) - Host's Ethereum address

**Returns:**
- `Promise<string>` - Host's public key (hex string)

**Throws:**
- `SDKError` with code `HOST_NOT_FOUND` - Host not registered
- `SDKError` with code `PUBLIC_KEY_NOT_AVAILABLE` - Host has not set public key

**Example:**
```typescript
const sdk = new FabstirSDKCore({ /* config */ });
await sdk.authenticate('privatekey', { privateKey });

// Get host public key for encryption
const hostAddress = '0x1234...';
const hostPubKey = await sdk.getHostPublicKey(hostAddress);

// Use for encrypted storage
const cid = await sdk.saveConversation(conversation, {
  hostPubKey,
  encrypt: true
});
```

#### Complete Encrypted Workflow Example

```typescript
import { FabstirSDKCore, ChainId } from '@fabstir/sdk-core';
import { ethers } from 'ethers';

// 1. Initialize SDK
const sdk = new FabstirSDKCore({
  chainId: ChainId.BASE_SEPOLIA,
  rpcUrl: 'https://base-sepolia.g.alchemy.com/v2/YOUR_KEY',
  contractAddresses: { /* ... */ }
});

// 2. Authenticate
const wallet = ethers.Wallet.createRandom();
await sdk.authenticate('privatekey', { privateKey: wallet.privateKey });

// 3. Start encrypted session
const sessionManager = await sdk.getSessionManager();
const hostAddress = '0x1234...'; // Discovered via HostManager

await sessionManager.startSession({
  hostAddress,
  hostUrl: 'ws://host:8080/ws',
  jobId: 123n,
  modelName: 'llama-3',
  chainId: 84532,
  encryption: true  // Enable encryption
});

// 4. Send encrypted messages
await sessionManager.sendMessage('What is the weather?');

// Wait for response...
// Messages are encrypted in transit

// 5. Save encrypted conversation
const conversation = sessionManager.getConversation();
const hostPubKey = await sdk.getHostPublicKey(hostAddress);

const cid = await sdk.saveConversation(conversation, {
  hostPubKey,
  encrypt: true
});

console.log('Encrypted conversation saved with CID:', cid);

// 6. Load encrypted conversation later
const loaded = await sdk.loadConversation(cid);
console.log('Loaded messages:', loaded.messages.length);
```

## Treasury Management

Manages treasury operations and fee distribution.

### Get TreasuryManager

```typescript
const treasuryManager = sdk.getTreasuryManager();
```

### getTreasuryInfo

Gets treasury information.

```typescript
async getTreasuryInfo(): Promise<TreasuryInfo>
```

**Returns:**
```typescript
interface TreasuryInfo {
  address: string;
  balance: string;
  feePercentage: number;
  totalCollected: string;
}
```

### getTreasuryBalance

Gets treasury balance for a specific token.

```typescript
async getTreasuryBalance(tokenAddress: string): Promise<string>
```

### getAccumulatedNative

Gets accumulated native token (ETH/BNB) treasury balance from JobMarketplace contract.

```typescript
async getAccumulatedNative(): Promise<bigint>
```

**Returns:** Accumulated native treasury balance as bigint (in wei)

**Example:**
```typescript
const treasuryManager = sdk.getTreasuryManager();

const nativeBalance = await treasuryManager.getAccumulatedNative();
console.log('Treasury ETH:', ethers.formatEther(nativeBalance));
```

**Note:** This method automatically handles fallback to old contract method names (`accumulatedTreasuryETH`) for backward compatibility.

### withdrawTreasuryFunds

Withdraws funds from treasury (admin only).

```typescript
async withdrawTreasuryFunds(
  tokenAddress: string,
  amount: string
): Promise<string>
```

## Client Manager

The ClientManager provides client-side operations for model selection, host discovery, and job management.

### Get ClientManager

```typescript
const clientManager = sdk.getClientManager();
```

**Note:** ClientManager requires authentication before use. Added in latest SDK update.

### selectHostForModel

Selects the best host for a specific model based on requirements.

```typescript
async selectHostForModel(
  modelId: string,
  requirements?: {
    maxCostPerToken?: number;
    minReputation?: number;
    requiredCapabilities?: string[];
    preferredLocation?: string;
  }
): Promise<HostInfo | null>
```

**Example:**
```typescript
const host = await clientManager.selectHostForModel(
  '0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced',
  {
    maxCostPerToken: 300,
    minReputation: 80,
    requiredCapabilities: ['streaming']
  }
);
```

### getModelAvailability

Gets availability information for a specific model across the network.

```typescript
async getModelAvailability(modelId: string): Promise<{
  totalHosts: number;
  activeHosts: number;
  averageCostPerToken: number;
  locations: string[];
}>
```

### estimateJobCost

Estimates the cost for a job based on expected token usage.

```typescript
async estimateJobCost(
  modelId: string,
  expectedTokens: number,
  hostAddress?: string
): Promise<{
  estimatedCost: string;
  pricePerToken: number;
  hostAddress: string;
}>
```

**Example:**
```typescript
const estimate = await clientManager.estimateJobCost(
  modelId,
  1000, // Expected 1000 tokens
  hostAddress
);
console.log(`Estimated cost: $${estimate.estimatedCost}`);
```

### createInferenceJob

Creates an inference job with automatic model and host validation.

```typescript
async createInferenceJob(
  modelSpec: {
    repo: string;
    fileName: string;
  },
  hostAddress: string,
  config: SessionConfig
): Promise<{
  sessionId: bigint;
  jobId: bigint;
  txHash: string;
}>
```

### getHostsByModel

Gets all hosts that support a specific model.

```typescript
async getHostsByModel(modelId: string): Promise<HostInfo[]>
```

**Example:**
```typescript
const hosts = await clientManager.getHostsByModel(modelId);
console.log(`Found ${hosts.length} hosts supporting this model`);

// Sort by cost
const sortedHosts = hosts.sort((a, b) =>
  a.metadata.costPerToken - b.metadata.costPerToken
);
```

## WebSocket Communication

Real-time communication for streaming responses.

### WebSocketClient

```typescript
import { WebSocketClient } from '@fabstir/sdk-core';
```

### Constructor

```typescript
new WebSocketClient(url: string, options?: WebSocketOptions)
```

**Options:**
```typescript
interface WebSocketOptions {
  reconnect?: boolean;
  reconnectInterval?: number;
  maxReconnectAttempts?: number;
  heartbeatInterval?: number;
}
```

### connect

Establishes WebSocket connection.

```typescript
async connect(): Promise<void>
```

### sendMessage

Sends a message through WebSocket and waits for response.

```typescript
async sendMessage(message: WebSocketMessage): Promise<string>
```

### onMessage

Registers a message handler and returns unsubscribe function.

```typescript
onMessage(handler: (data: any) => void): () => void
```

### disconnect

Closes the WebSocket connection gracefully.

```typescript
async disconnect(): Promise<void>
```

### isConnected

Check if WebSocket is connected.

```typescript
isConnected(): boolean
```

### getReadyState

Get WebSocket connection state.

```typescript
getReadyState(): number
```

**Example:**
```typescript
const ws = new WebSocketClient('ws://localhost:8080');
await ws.connect();

// Register message handler (returns unsubscribe function)
const unsubscribe = ws.onMessage((data) => {
  console.log('Received:', data);
});

// Send message and wait for response
const response = await ws.sendMessage({
  type: 'inference',
  sessionId: '123',
  prompt: 'Hello'
});

// Check connection status
if (ws.isConnected()) {
  console.log('WebSocket is connected');
}

// Clean up
unsubscribe();
await ws.disconnect();
```

## Contract Integration

### SessionJobManager

Direct contract interaction for session jobs.

```typescript
const sessionJobManager = sdk.getSessionJobManager();
```

#### createSessionJob

Creates a session job directly on the blockchain.

```typescript
async createSessionJob(params: SessionJobParams): Promise<SessionResult>
```

**Parameters:**
```typescript
interface SessionJobParams {
  provider: string;
  signer: ethers.Signer;
  tokenAddress: string;     // USDC address
  depositAmount: string;     // Amount in smallest units
  sessionConfig: {
    pricePerToken: bigint;
    duration: bigint;
    proofInterval: bigint;
  };
}

interface SessionResult {
  sessionId: bigint;
  txHash: string;
  receipt: TransactionReceipt;
}
```

#### submitCheckpointProof

Submits checkpoint proof as provider.

```typescript
async submitCheckpointProof(
  sessionId: bigint,
  checkpointNumber: number,
  tokensUsed: number,
  proofData: string
): Promise<string>
```

#### completeSessionJob

Completes a session job.

```typescript
async completeSessionJob(
  sessionId: bigint,
  conversationCID: string
): Promise<string>
```

## Services

### ProofVerifier

Verifies cryptographic proofs.

```typescript
const proofVerifier = new ProofVerifier();
```

#### verifyCheckpointProof

Verifies a checkpoint proof.

```typescript
async verifyCheckpointProof(
  proof: string,
  expectedData: {
    sessionId: bigint;
    checkpointNumber: number;
    tokensUsed: number;
  }
): Promise<boolean>
```

#### generateProof

Generates a proof for checkpoint.

```typescript
async generateProof(
  data: {
    sessionId: bigint;
    checkpointNumber: number;
    tokensUsed: number;
    timestamp: number;
  }
): Promise<string>
```

### EnvironmentDetector

Detects runtime environment capabilities.

```typescript
import { EnvironmentDetector } from '@fabstir/sdk-core';

const detector = new EnvironmentDetector();
const capabilities = detector.getCapabilities();

if (capabilities.hasP2P) {
  // P2P features available
}
if (capabilities.hasWebSockets) {
  // WebSocket features available
}
```

## Error Handling

The SDK uses typed errors with specific codes.

### Error Codes

```typescript
enum SDKErrorCode {
  // Authentication
  AUTH_FAILED = 'AUTH_FAILED',
  AUTH_REQUIRED = 'AUTH_REQUIRED',
  INVALID_SIGNER = 'INVALID_SIGNER',

  // Managers
  MANAGER_NOT_INITIALIZED = 'MANAGER_NOT_INITIALIZED',
  MANAGER_NOT_AUTHENTICATED = 'MANAGER_NOT_AUTHENTICATED',

  // Transactions
  INSUFFICIENT_BALANCE = 'INSUFFICIENT_BALANCE',
  TRANSACTION_FAILED = 'TRANSACTION_FAILED',
  APPROVAL_FAILED = 'APPROVAL_FAILED',

  // Sessions
  SESSION_NOT_FOUND = 'SESSION_NOT_FOUND',
  SESSION_EXPIRED = 'SESSION_EXPIRED',
  INVALID_SESSION_STATE = 'INVALID_SESSION_STATE',

  // Models
  MODEL_NOT_APPROVED = 'MODEL_NOT_APPROVED',
  MODEL_VALIDATION_FAILED = 'MODEL_VALIDATION_FAILED',
  INVALID_MODEL_HASH = 'INVALID_MODEL_HASH',

  // Storage
  STORAGE_ERROR = 'STORAGE_ERROR',
  S5_CONNECTION_FAILED = 'S5_CONNECTION_FAILED',
  STORAGE_NOT_INITIALIZED = 'STORAGE_NOT_INITIALIZED',
  INVALID_SETTINGS = 'INVALID_SETTINGS',
  INVALID_SETTINGS_STRUCTURE = 'INVALID_SETTINGS_STRUCTURE',
  STORAGE_SAVE_ERROR = 'STORAGE_SAVE_ERROR',
  STORAGE_LOAD_ERROR = 'STORAGE_LOAD_ERROR',
  STORAGE_UPDATE_ERROR = 'STORAGE_UPDATE_ERROR',
  STORAGE_CLEAR_ERROR = 'STORAGE_CLEAR_ERROR',

  // WebSocket
  WEBSOCKET_CONNECTION_FAILED = 'WEBSOCKET_CONNECTION_FAILED',
  WEBSOCKET_MESSAGE_FAILED = 'WEBSOCKET_MESSAGE_FAILED',

  // Proofs
  INVALID_PROOF = 'INVALID_PROOF',
  PROOF_VERIFICATION_FAILED = 'PROOF_VERIFICATION_FAILED',

  // Client Manager
  CLIENT_MANAGER_ERROR = 'CLIENT_MANAGER_ERROR',
  HOST_NOT_FOUND = 'HOST_NOT_FOUND',
  HOST_SELECTION_FAILED = 'HOST_SELECTION_FAILED',

  // Configuration
  INVALID_CONFIGURATION = 'INVALID_CONFIGURATION',
  MISSING_CONTRACT_ADDRESS = 'MISSING_CONTRACT_ADDRESS',

  // Multi-chain
  UNSUPPORTED_CHAIN = 'UNSUPPORTED_CHAIN',
  CHAIN_MISMATCH = 'CHAIN_MISMATCH',
  INSUFFICIENT_DEPOSIT = 'INSUFFICIENT_DEPOSIT',
  NODE_CHAIN_MISMATCH = 'NODE_CHAIN_MISMATCH',
  DEPOSIT_ACCOUNT_UNAVAILABLE = 'DEPOSIT_ACCOUNT_UNAVAILABLE'
}
```

### Multi-Chain Error Types

```typescript
class UnsupportedChainError extends Error {
  constructor(chainId: number, supportedChains: number[]);
}

class ChainMismatchError extends Error {
  constructor(expected: number, actual: number, operation: string);
}

class InsufficientDepositError extends Error {
  constructor(required: string, available: string, chainId: number);
}

class NodeChainMismatchError extends Error {
  constructor(nodeChainId: number, sdkChainId: number);
}
```

### Error Handling Example

```typescript
try {
  await sdk.authenticate(privateKey);
  const sessionManager = sdk.getSessionManager();

  const { sessionId } = await sessionManager.startSession(
    modelId,
    provider,
    config
  );
} catch (error) {
  switch (error.code) {
    case SDKErrorCode.AUTH_FAILED:
      console.error('Authentication failed:', error.message);
      break;
    case SDKErrorCode.INSUFFICIENT_BALANCE:
      console.error('Insufficient USDC balance');
      break;
    case SDKErrorCode.MODEL_NOT_APPROVED:
      console.error('Model not approved for use');
      break;
    default:
      console.error('Unexpected error:', error);
  }
}
```

## Types and Interfaces

### Core Types

```typescript
// SDK Configuration
interface FabstirSDKCoreConfig {
  rpcUrl?: string;
  chainId?: number;
  contractAddresses?: ContractAddresses;
  s5Config?: S5Config;
}

// Session Management
interface SessionConfig {
  depositAmount: string;  // USDC amount as decimal string
  pricePerToken: number;  // Price per token in smallest units
  duration: number;       // Session duration in seconds
  proofInterval: number;  // Checkpoint interval in tokens
}

interface SessionJob {
  id: bigint;
  jobId: bigint;
  client: string;
  provider: string;
  model: string;
  depositAmount: bigint;
  pricePerToken: bigint;
  tokensUsed: bigint;
  status: 'active' | 'completed' | 'failed';
  startTime: bigint;
  endTime?: bigint;
  checkpoints: CheckpointProof[];
}

interface CheckpointProof {
  checkpointNumber: number;
  tokensUsed: number;
  proofData: string;
  timestamp: number;
}

// Model Governance
interface ModelInfo {
  id: string;
  huggingfaceRepo: string;
  fileName: string;
  modelHash: string;
  size: bigint;
  approved: boolean;
  metadata?: ModelMetadata;
}

interface ModelSpec {
  huggingfaceRepo: string;
  fileName: string;
  modelHash: string;
}

interface ModelValidation {
  isValid: boolean;
  isApproved: boolean;
  hashMatches?: boolean;
  errors: string[];
}

// Host Management
interface HostInfo {
  address: string;
  metadata: HostMetadata;
  supportedModels: string[];
  isActive: boolean;
  stake: bigint;
  reputation: number;
  apiUrl?: string;
}

interface HostMetadata {
  hardware: {
    gpu: string;
    vram: number;
    ram: number;
  };
  capabilities: string[];
  location: string;
  maxConcurrent: number;
  costPerToken: number;
}

// Chat/Conversation Types
interface ChatMessage {
  role: 'user' | 'assistant' | 'system';
  content: string;
  timestamp: number;
  tokens?: number;
}

interface ConversationData {
  sessionId: string;
  messages: ChatMessage[];
  metadata: {
    model: string;
    provider: string;
    totalTokens: number;
    totalCost: number;
  };
}
```

## Usage Examples

### Complete USDC Payment Flow with Context Preservation

This example shows the complete flow with popup-free transactions using Base Account Kit:

```typescript
import { FabstirSDKCore, ensureSubAccount, createSubAccountSigner } from '@fabstir/sdk-core';
import { createBaseAccountSDK } from "@base-org/account";
import { ethers } from 'ethers';

async function chatWithContext() {
  // 1. Initialize SDK with ALL required contracts
  const sdk = new FabstirSDKCore({
    rpcUrl: process.env.NEXT_PUBLIC_RPC_URL_BASE_SEPOLIA!,
    contractAddresses: {
      // ALL 5 REQUIRED
      jobMarketplace: process.env.NEXT_PUBLIC_CONTRACT_JOB_MARKETPLACE!,
      nodeRegistry: process.env.NEXT_PUBLIC_CONTRACT_NODE_REGISTRY!,
      proofSystem: process.env.NEXT_PUBLIC_CONTRACT_PROOF_SYSTEM!,
      hostEarnings: process.env.NEXT_PUBLIC_CONTRACT_HOST_EARNINGS!,
      usdcToken: process.env.NEXT_PUBLIC_CONTRACT_USDC_TOKEN!
    }
  });

  // 2. Setup Base Account Kit for popup-free transactions
  const baseAccountSDK = createBaseAccountSDK({
    appName: "Fabstir Chat",
    appChainIds: [84532],
    subAccounts: {
      unstable_enableAutoSpendPermissions: true
    }
  });

  // 3. Authenticate user with passkey (one-time popup)
  const result = await baseAccountSDK.loginWithPasskey();
  const smartWallet = result.address;
  const baseProvider = result.provider;

  // 4. Create sub-account with spend permissions (SDK utility)
  const subAccountResult = await ensureSubAccount(
    baseProvider,
    smartWallet as `0x${string}`,
    {
      tokenAddress: process.env.NEXT_PUBLIC_CONTRACT_USDC_TOKEN!,
      tokenDecimals: 6,
      maxAllowance: "1000000",    // $1M max allowance
      periodDays: 365             // Valid for 1 year
    }
  );

  const subAccount = subAccountResult.address;
  console.log(subAccountResult.isExisting
    ? 'Using existing sub-account'
    : 'Created new sub-account');

  // 5. Create custom signer for popup-free transactions (SDK utility)
  const baseSigner = createSubAccountSigner({
    provider: baseProvider,
    subAccount: subAccount,
    primaryAccount: smartWallet,
    chainId: 84532
  });

  // 6. Authenticate SDK with custom signer
  await sdk.authenticate("signer", {
    signer: baseSigner,
  });

  // 7. Get managers
  const sessionManager = sdk.getSessionManager();
  const storageManager = await sdk.getStorageManager();
  const paymentManager = sdk.getPaymentManager();

  // 8. Check and approve USDC if needed (no popup!)
  const usdcAddress = process.env.NEXT_PUBLIC_CONTRACT_USDC_TOKEN!;
  const marketplaceAddress = sdk.contractManager.getAddress('jobMarketplace');

  const allowance = await paymentManager.checkAllowance(
    subAccount,
    marketplaceAddress,
    usdcAddress
  );

  const requiredAmount = ethers.parseUnits("10.0", 6);
  if (allowance < requiredAmount) {
    console.log('Approving USDC...');
    await paymentManager.approveToken(
      marketplaceAddress,
      requiredAmount,
      usdcAddress
    );
    console.log('USDC approved - no popup required!');
  }

  // 9. Start session with USDC payment (no popup!)
  const { sessionId } = await sessionManager.startSession(
    '0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced',
    '0x4594F755F593B517Bb3194F4DeC20C48a3f04504',
    {
      depositAmount: "1.0",        // $1 USDC minimum deposit
      pricePerToken: 200,          // 0.0002 USDC per token
      duration: 3600,              // 1 hour timeout
      proofInterval: 100           // Checkpoint every 100 tokens
    }
  );

  console.log('Session started (no popup!):', sessionId.toString());

  // 10. Conversation with context preservation
  const conversation: ChatMessage[] = [];

  // First prompt
  let prompt = "What is the capital of France?";
  let response = await sessionManager.sendPrompt(sessionId, prompt);

  conversation.push(
    { role: 'user', content: prompt, timestamp: Date.now() },
    { role: 'assistant', content: response, timestamp: Date.now(), tokens: 15 }
  );

  // Second prompt with context
  const context = conversation.map(msg =>
    `${msg.role === 'user' ? 'User' : 'Assistant'}: ${msg.content}`
  ).join('\n');

  prompt = "Tell me more about that city";
  const fullPrompt = `${context}\nUser: ${prompt}\nAssistant:`;
  response = await sessionManager.sendPrompt(sessionId, fullPrompt);

  conversation.push(
    { role: 'user', content: prompt, timestamp: Date.now() },
    { role: 'assistant', content: response, timestamp: Date.now(), tokens: 95 }
  );

  // 11. Store conversation in S5
  const cid = await storageManager.storeConversation(
    sessionId.toString(),
    conversation
  );
  console.log('Conversation stored:', cid);

  // 12. Submit checkpoint proof
  const totalTokens = conversation.reduce((sum, msg) => sum + (msg.tokens || 0), 0);
  const checkpointProof = {
    checkpointNumber: 1,
    tokensUsed: totalTokens,
    proofData: '0x' + '00'.repeat(64), // 64-byte proof
    timestamp: Date.now()
  };

  await sessionManager.submitCheckpoint(sessionId, checkpointProof);

  // 13. Complete session (no popup!)
  const finalProof = '0x' + 'ff'.repeat(64);
  const txHash = await sessionManager.completeSession(
    sessionId,
    totalTokens,
    finalProof
  );

  console.log('Session completed (no popup!):', txHash);
}
```

**Key Benefits:**
- ✅ Only ONE popup for passkey authentication
- ✅ NO popups for approvals, deposits, or session operations
- ✅ Automatic spend permissions with configurable limits
- ✅ Sub-account reused across sessions
- ✅ Full SDK integration with custom signer

### Using ClientManager for Model and Host Selection

```typescript
async function selectOptimalHostForJob() {
  const sdk = new FabstirSDKCore(config);
  await sdk.authenticate(privateKey);

  const clientManager = sdk.getClientManager();
  const modelManager = sdk.getModelManager();

  // Find best host for a specific model
  const modelId = '0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced';

  // Select host based on requirements
  const host = await clientManager.selectHostForModel(modelId, {
    maxCostPerToken: 250,
    requiredCapabilities: ['streaming'],
    minReputation: 75
  });

  if (!host) {
    throw new Error('No suitable host found for model');
  }

  // Estimate job cost
  const estimate = await clientManager.estimateJobCost(
    modelId,
    500, // Expected 500 tokens
    host.address
  );

  console.log(`Selected host: ${host.address}`);
  console.log(`Estimated cost: $${estimate.estimatedCost}`);
  console.log(`Price per token: ${estimate.pricePerToken}`);

  // Create inference job
  const result = await clientManager.createInferenceJob(
    { repo: 'CohereForAI/TinyVicuna-1B-32k-GGUF', fileName: 'tiny-vicuna-1b.q4_k_m.gguf' },
    host.address,
    {
      depositAmount: "1.0",
      pricePerToken: estimate.pricePerToken,
      duration: 3600,
      proofInterval: 100
    }
  );

  return result;
}
```

### Model Discovery and Validation

```typescript
async function discoverAndValidateModels() {
  const sdk = new FabstirSDKCore(config);
  await sdk.authenticate(privateKey);

  const modelManager = sdk.getModelManager();
  const hostManager = sdk.getHostManager();

  // 1. Get all approved models
  const approvedModels = await modelManager.getAllApprovedModels();
  console.log(`Found ${approvedModels.length} approved models`);

  // 2. Find a specific model
  const modelId = await modelManager.getModelId(
    'meta-llama/Llama-2-7b-hf',
    'model.safetensors'
  );

  // 3. Check if model is approved
  const isApproved = await modelManager.isModelApproved(modelId);
  if (!isApproved) {
    throw new Error('Model not approved for use');
  }

  // 4. Get model details
  const modelInfo = await modelManager.getModelDetails(modelId);
  console.log('Model info:', modelInfo);

  // 5. Find hosts supporting this model
  const hosts = await hostManager.findHostsForModel(modelId);
  console.log(`Found ${hosts.length} hosts supporting this model`);

  // 6. Select best host based on criteria
  const bestHost = hosts.reduce((best, host) => {
    if (!best || host.metadata.costPerToken < best.metadata.costPerToken) {
      return host;
    }
    return best;
  }, hosts[0]);

  console.log('Selected host:', bestHost.address);
  console.log('Cost per token:', bestHost.metadata.costPerToken);
  console.log('Hardware:', bestHost.metadata.hardware);

  return { modelId, hostAddress: bestHost.address };
}
```

### Streaming Responses with WebSocket

```typescript
async function streamingChat() {
  const sdk = new FabstirSDKCore();
  await sdk.authenticate(privateKey);

  const sessionManager = sdk.getSessionManager();

  // Start session
  const { sessionId } = await sessionManager.startSession(
    modelId,
    providerAddress,
    config
  );

  // Send prompt with streaming
  const response = await sessionManager.sendPromptStreaming(
    sessionId,
    "Write a story about a robot",
    (token) => {
      // Handle each token as it arrives
      process.stdout.write(token);
    }
  );

  console.log('\n\nStreaming complete!');
  console.log('Total response length:', response.length);
}
```

### Host Registration with Models

```typescript
async function registerAsHost() {
  const sdk = new FabstirSDKCore();
  await sdk.authenticate(hostPrivateKey);

  const hostManager = sdk.getHostManager();
  const modelManager = sdk.getModelManager();

  // Define supported models
  const supportedModels: ModelSpec[] = [
    {
      huggingfaceRepo: 'meta-llama/Llama-2-7b-hf',
      fileName: 'model.safetensors',
      modelHash: '0x...'
    },
    {
      huggingfaceRepo: 'mistralai/Mistral-7B-v0.1',
      fileName: 'model.safetensors',
      modelHash: '0x...'
    }
  ];

  // Validate models are approved
  for (const model of supportedModels) {
    const validation = await modelManager.validateModel(model);
    if (!validation.isApproved) {
      throw new Error(`Model not approved: ${model.huggingfaceRepo}`);
    }
  }

  // Register host with models
  const txHash = await hostManager.registerHostWithModels({
    metadata: {
      hardware: {
        gpu: 'NVIDIA RTX 4090',
        vram: 24,
        ram: 64
      },
      capabilities: ['streaming', 'batch', 'context-8k'],
      location: 'us-east',
      maxConcurrent: 5,
      costPerToken: 150 // 150 units per token
    },
    supportedModels,
    stake: '100', // 100 FAB tokens
    apiUrl: 'https://my-llm-node.example.com'
  });

  console.log('Host registered:', txHash);

  // Update API URL if needed
  await hostManager.updateApiUrl('https://new-api.example.com');

  // Check earnings
  const status = await hostManager.getHostStatus(hostAddress);
  console.log('Earnings:', status.earnings);

  // Withdraw earnings
  if (parseFloat(status.earnings) > 0) {
    const withdrawTx = await hostManager.withdrawEarnings(USDC_ADDRESS);
    console.log('Earnings withdrawn:', withdrawTx);
  }
}
```

### Error Recovery and Retry Logic

```typescript
async function robustSession() {
  const sdk = new FabstirSDKCore();
  const maxRetries = 3;

  // Authenticate with retry
  for (let i = 0; i < maxRetries; i++) {
    try {
      await sdk.authenticate(privateKey);
      break;
    } catch (error) {
      if (i === maxRetries - 1) throw error;
      await new Promise(resolve => setTimeout(resolve, 1000 * (i + 1)));
    }
  }

  const sessionManager = sdk.getSessionManager();
  let sessionId: bigint;

  // Start session with proper error handling
  try {
    const result = await sessionManager.startSession(
      modelId,
      providerAddress,
      config
    );
    sessionId = result.sessionId;
  } catch (error) {
    if (error.code === SDKErrorCode.INSUFFICIENT_BALANCE) {
      // Handle insufficient balance
      console.error('Please fund your account with USDC');
      return;
    }
    if (error.code === SDKErrorCode.MODEL_NOT_APPROVED) {
      // Try alternative model
      const alternativeModel = await findAlternativeModel();
      const result = await sessionManager.startSession(
        alternativeModel,
        providerAddress,
        config
      );
      sessionId = result.sessionId;
    } else {
      throw error;
    }
  }

  // Send prompts with retry on WebSocket failures
  for (let i = 0; i < maxRetries; i++) {
    try {
      const response = await sessionManager.sendPrompt(
        sessionId,
        "Hello, how are you?"
      );
      console.log('Response:', response);
      break;
    } catch (error) {
      if (error.code === SDKErrorCode.WEBSOCKET_CONNECTION_FAILED && i < maxRetries - 1) {
        // Wait and retry
        await new Promise(resolve => setTimeout(resolve, 2000));
        continue;
      }
      throw error;
    }
  }
}
```

## Constants

```typescript
// Network Configuration
export const BASE_SEPOLIA_CHAIN_ID = 84532;
export const BASE_SEPOLIA_CHAIN_HEX = "0x14a34";

// Payment Configuration
export const MIN_USDC_DEPOSIT = "1";           // $1 minimum (reduced from $2)
export const DEFAULT_PRICE_PER_TOKEN = 200;    // 200 units per token
export const DEFAULT_SESSION_DURATION = 3600;  // 1 hour
export const DEFAULT_PROOF_INTERVAL = 1000;    // 1000 tokens (production default, balances security and gas costs)

// Proof Requirements
export const MIN_PROOF_LENGTH = 64;            // 64 bytes minimum
export const PROOF_VERIFICATION_GAS = 200000;  // Gas for proof verification

// Rate Limiting
export const TOKEN_GENERATION_RATE = 10;       // 10 tokens per second
export const TOKEN_BURST_MULTIPLIER = 2;       // 2x burst allowed

// Payment Distribution (from smart contracts)
export const HOST_PAYMENT_PERCENTAGE = 90;     // 90% to host
export const TREASURY_FEE_PERCENTAGE = 10;     // 10% to treasury

// WebSocket Configuration
export const WS_RECONNECT_INTERVAL = 5000;     // 5 seconds
export const WS_MAX_RECONNECT_ATTEMPTS = 5;
export const WS_HEARTBEAT_INTERVAL = 30000;    // 30 seconds

// S5 Storage Configuration
export const DEFAULT_S5_PORTAL = 'wss://z2DWuPbL5pweybXnEB618pMnV58ECj2VPDNfVGm3tFqBvjF@s5.ninja/s5/p2p';
export const S5_UPLOAD_TIMEOUT = 60000;        // 60 seconds
export const S5_DOWNLOAD_TIMEOUT = 30000;      // 30 seconds
```

## Troubleshooting

## Multi-Chain Usage Examples

### Multi-Chain Initialization

```typescript
import { FabstirSDKCore, EOAProvider, WalletProviderFactory } from '@fabstir/sdk-core';
import { ChainId } from '@fabstir/sdk-core/types';

// Initialize SDK with specific chain
const sdk = new FabstirSDKCore({
  rpcUrl: 'https://sepolia.base.org',
  chainId: ChainId.BASE_SEPOLIA, // 84532
  contractAddresses: {
    // Base Sepolia addresses
    jobMarketplace: '0xaa38e7fcf5d7944ef7c836e8451f3bf93b98364f',
    nodeRegistry: '0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218',
    proofSystem: '0x2ACcc60893872A499700908889B38C5420CBcFD1',
    hostEarnings: '0x908962e8c6CE72610021586f85ebDE09aAc97776',
    usdcToken: '0x036CbD53842c5426634e7929541eC2318f3dCF7e',
    fabToken: '0xC78949004B4EB6dEf2D66e49Cd81231472612D62',
    modelRegistry: '0x92b2De840bB2171203011A6dBA928d855cA8183E'
  }
});

// Initialize with wallet provider
const provider = new EOAProvider(window.ethereum);
await sdk.initialize(provider);
await sdk.authenticate('privatekey', { privateKey: process.env.PRIVATE_KEY });
```

### Chain Switching Example

```typescript
// Start on Base Sepolia
console.log('Current chain:', sdk.getCurrentChainId()); // 84532

// Create a session on Base Sepolia
const sessionManager = sdk.getSessionManager();
const baseSession = await sessionManager.startSession(
  modelId,
  hostAddress,
  {
    chainId: ChainId.BASE_SEPOLIA,
    depositAmount: "0.001", // ETH
    pricePerToken: 200,
    duration: 3600,
    proofInterval: 100
  }
);

// Switch to opBNB testnet
await sdk.switchChain(ChainId.OPBNB_TESTNET); // 5611
console.log('Switched to:', sdk.getCurrentChain().name); // "opBNB Testnet"

// Create a session on opBNB
const opbnbSession = await sessionManager.startSession(
  modelId,
  hostAddress,
  {
    chainId: ChainId.OPBNB_TESTNET,
    depositAmount: "0.002", // BNB
    pricePerToken: 200,
    duration: 3600,
    proofInterval: 100
  }
);
```

### Deposit Flow on Multiple Chains

```typescript
// Deposit ETH on Base Sepolia
const paymentManager = sdk.getPaymentManager();

// Check minimum deposit for Base Sepolia
const baseChain = sdk.getCurrentChain();
console.log('Min deposit:', baseChain.minDeposit); // 0.0002 ETH

// Deposit ETH
const ethTx = await paymentManager.depositNative("0.001");
await ethTx.wait(3);

// Check balance
let balance = await paymentManager.getDepositBalance();
console.log('ETH deposited:', balance.native);

// Switch to opBNB and deposit BNB
await sdk.switchChain(ChainId.OPBNB_TESTNET);

const opbnbChain = sdk.getCurrentChain();
console.log('Min deposit:', opbnbChain.minDeposit); // 0.001 BNB

// Deposit BNB
const bnbTx = await paymentManager.depositNative("0.002");
await bnbTx.wait(3);

balance = await paymentManager.getDepositBalance();
console.log('BNB deposited:', balance.native);
```

### Chain-Aware Node Discovery

```typescript
const clientManager = sdk.getClientManager();

// Discover nodes for Base Sepolia
const baseNodes = await clientManager.discoverNodes(ChainId.BASE_SEPOLIA);
console.log('Base Sepolia nodes:', baseNodes.length);

// Check if a node supports specific chain
const nodeChains = await clientManager.getNodeChains('http://node1.base');
if (nodeChains.includes(ChainId.BASE_SEPOLIA)) {
  console.log('Node supports Base Sepolia');
}

// Discover nodes for opBNB
const opbnbNodes = await clientManager.discoverNodes(ChainId.OPBNB_TESTNET);
console.log('opBNB nodes:', opbnbNodes.length);
```

### Error Handling for Multi-Chain

```typescript
import { UnsupportedChainError, ChainMismatchError } from '@fabstir/sdk-core/errors';

try {
  // Try to switch to unsupported chain
  await sdk.switchChain(999999);
} catch (error) {
  if (error instanceof UnsupportedChainError) {
    console.error('Chain not supported:', error.chainId);
    console.log('Supported chains:', error.supportedChains);
  }
}

try {
  // Try to create session on wrong chain
  await sessionManager.startSession(modelId, hostAddress, {
    chainId: ChainId.BASE_SEPOLIA,
    // ... config
  });
} catch (error) {
  if (error instanceof ChainMismatchError) {
    console.error(`Expected chain ${error.expected}, got ${error.actual}`);
  }
}
```

### Cross-Chain Session Management

```typescript
// Track sessions across chains
const sessions = new Map();

// Create session on Base Sepolia
await sdk.switchChain(ChainId.BASE_SEPOLIA);
const baseSessionId = await sessionManager.startSession(modelId, host, {
  chainId: ChainId.BASE_SEPOLIA,
  depositAmount: "0.001",
  pricePerToken: 200,
  duration: 3600,
  proofInterval: 100
});
sessions.set(ChainId.BASE_SEPOLIA, baseSessionId);

// Create session on opBNB
await sdk.switchChain(ChainId.OPBNB_TESTNET);
const opbnbSessionId = await sessionManager.startSession(modelId, host, {
  chainId: ChainId.OPBNB_TESTNET,
  depositAmount: "0.002",
  pricePerToken: 200,
  duration: 3600,
  proofInterval: 100
});
sessions.set(ChainId.OPBNB_TESTNET, opbnbSessionId);

// Resume session on specific chain
await sdk.switchChain(ChainId.BASE_SEPOLIA);
const baseSession = await sessionManager.resumeSession(
  sessions.get(ChainId.BASE_SEPOLIA)
);
```

### Common Issues

#### 1. "chainId must be a hex encoded integer"
**Solution:** Use `CHAIN_HEX = "0x14a34"` instead of decimal chain ID in wallet_sendCalls.

#### 2. "Insufficient USDC balance"
**Solution:**
- Check sub-account balance, not primary account
- Ensure $2 minimum deposit amount
- Fund sub-account from primary account if needed

#### 3. "Invalid proof" error
**Solution:**
- Ensure proof is minimum 64 bytes
- Use proper proof format: `'0x' + '00'.repeat(64)`
- Wait for token accumulation before submitting proof

#### 4. WebSocket connection failures
**Solution:**
- Check if host API URL is accessible
- Verify WebSocket endpoint format (ws:// or wss://)
- Implement retry logic with exponential backoff

#### 5. Model not approved
**Solution:**
- Use `modelManager.getAllApprovedModels()` to find approved models
- Verify model hash matches registry
- Contact governance for model approval

#### 6. Transaction timeout
**Solution:**
- Use `tx.wait(3)` for proper confirmations
- Don't use arbitrary setTimeout delays
- Check network congestion and gas prices

### Browser vs Node.js Differences

| Feature | Browser | Node.js |
|---------|---------|---------|
| P2P Networking | ❌ Not available | ✅ Full libp2p support |
| WebSocket | ✅ Native support | ✅ With ws package |
| S5 Storage | ✅ With IndexedDB | ✅ With polyfill |
| Crypto operations | ✅ Web Crypto API | ✅ Node crypto |
| File system | ❌ Not available | ✅ Full access |

### Debug Mode

Enable debug logging:

```typescript
const sdk = new FabstirSDKCore({
  debug: true,
  logLevel: 'verbose'
});

// Or set environment variable
process.env.FABSTIR_SDK_DEBUG = 'true';
```

## Gas Payment Responsibilities

Understanding who pays gas fees in the Fabstir marketplace:

### Payment Model

| Operation | Who Pays | Estimated Gas | Description |
|-----------|----------|---------------|-------------|
| Session Creation | User | ~200,000 gas | Initial session setup and deposit |
| Checkpoint Proofs | Host | ~30,000 gas each | Every `proofInterval` tokens |
| Session Completion | User | ~100,000 gas | Final settlement and refunds |
| Abandoned Session Claim | Host | ~100,000 gas | After 24 hour timeout |

### Economic Implications

1. **User Incentive Issue**: Users may abandon sessions to avoid paying completion gas
2. **Host Cost Consideration**: Hosts must factor checkpoint gas costs into pricing
3. **Future Solutions**: Exploring gasless completion via account abstraction

### Payment Distribution

Based on treasury configuration (.env.test):
- **Host**: 90% of payment (HOST_EARNINGS_PERCENTAGE)
- **Treasury**: 10% of payment (TREASURY_FEE_PERCENTAGE)
- **User**: Refund of unused deposit

## Support

- GitHub Issues: https://github.com/fabstir/fabstir-llm-sdk/issues
- Documentation: https://docs.fabstir.com
- Discord: https://discord.gg/fabstir

## License

MIT License - See LICENSE file for details.