# Fabstir SDK API Reference

## Table of Contents
- [Overview](#overview)
- [Installation](#installation)
- [Core SDK](#core-sdk)
- [Authentication](#authentication)
- [Session Management](#session-management)
- [Web Search Integration](#web-search-integration)
- [Image Generation](#image-generation)
  - [OpenAI-Compatible Bridge](#openai-compatible-bridge)
- [Video Transcoding](#video-transcoding)
- [Payment Management](#payment-management)
- [Model Governance](#model-governance)
- [Host Management](#host-management)
- [Storage Management](#storage-management)
  - [User Settings Storage](#user-settings-storage)
- [RAG and Vector Databases](#rag-and-vector-databases)
  - [VectorRAGManager](#vectorragmanager)
  - [DocumentManager](#documentmanager)
  - [Embedding Services](#embedding-services)
  - [Permissions and Sharing](#permissions-and-sharing)
- [Treasury Management](#treasury-management)
- [Client Manager](#client-manager)
- [WebSocket Communication](#websocket-communication)
- [Contract Integration](#contract-integration)
  - [Delegate-Pays Authorization](#delegate-pays-authorization-sdk-level-sdk-core-1210)
- [Services](#services)
- [Error Handling](#error-handling)
- [Types and Interfaces](#types-and-interfaces)
- [Usage Examples](#usage-examples)
- [Multi-Agent Orchestration](#multi-agent-orchestration-fabstirorchestrator)

## Overview

The Fabstir SDK provides a comprehensive interface for interacting with the Fabstir P2P LLM marketplace. The SDK has been refactored into browser-compatible (`@fabstir/sdk-core`) and Node.js-specific (`@fabstir/sdk-node`) packages.

### Key Features
- Browser-compatible core functionality
- USDC and ETH payment support
- Session-based LLM interactions with context preservation
- **RAG (Retrieval-Augmented Generation)** with vector databases, document upload, and semantic search
- **Image Generation** via FLUX.2 diffusion models (encrypted WebSocket + HTTP paths)
- **Video Transcoding** with GPU acceleration, encrypted source/output support, and real-time progress
- Model governance and validation
- WebSocket real-time streaming
- S5 decentralized storage integration
- Base Account Kit for gasless transactions
- Multi-chain support (Base Sepolia, opBNB testnet)
- Chain-aware wallet providers (EOA and Smart Accounts)
- Multi-agent orchestration with LLM-driven task decomposition
- x402 HTTP payment protocol for agent-to-agent USDC micropayments
- A2A protocol for agent discovery and delegation

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
import { ChainRegistry, ChainId } from '@fabstir/sdk-core/config';

// Base Sepolia Configuration (Default)
// Contract addresses from ChainRegistry (populated from .env.test)
const chain = ChainRegistry.getChain(ChainId.BASE_SEPOLIA);
const baseSepolia = new FabstirSDKCore({
  rpcUrl: process.env.RPC_URL_BASE_SEPOLIA!,
  chainId: ChainId.BASE_SEPOLIA, // 84532
  contractAddresses: {
    jobMarketplace: chain.contracts.jobMarketplace,
    nodeRegistry: chain.contracts.nodeRegistry,
    proofSystem: chain.contracts.proofSystem,
    hostEarnings: chain.contracts.hostEarnings,
    usdcToken: chain.contracts.usdcToken,
    fabToken: chain.contracts.fabToken,
    modelRegistry: chain.contracts.modelRegistry
  }
});

// opBNB Testnet Configuration (post-MVP)
const opBNBChain = ChainRegistry.getChain(ChainId.OPBNB_TESTNET);
const opBNBTestnet = new FabstirSDKCore({
  rpcUrl: 'https://opbnb-testnet-rpc.bnbchain.org',
  chainId: ChainId.OPBNB_TESTNET, // 5611
  contractAddresses: {
    // Addresses will be in ChainRegistry when opBNB contracts are deployed
    ...opBNBChain.contracts
  }
});
```

**Contract Addresses:**
```bash
# Source of truth: .env.test - never hardcode addresses
cat .env.test | grep CONTRACT_
```

## Authentication

### authenticate

Authenticates the SDK with various providers.

```typescript
async authenticate(
  method?: 'metamask' | 'privatekey' | 'signer' | 'aa-signer',
  options?: any,
): Promise<void>
```

**Parameters:**
- `method`: Authentication method — `'metamask'` (default), `'privatekey'`, `'signer'`, or `'aa-signer'` (added in 1.19.0).
- `options`: Mode-specific options.
  - For `'privatekey'`: `{ privateKey: string }`
  - For `'signer'`: `{ signer: ethers.Signer }`
  - For `'aa-signer'`: `{ smartAccountAddress, eoaPrivateKey, sendUserOp, rpcUrl, chainId }` — see Method 3 below.

**Example 1: Private Key Authentication:**
```typescript
await sdk.authenticate('privatekey', { privateKey: '0x...' });
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

**Example 3: ERC-4337 Smart Account Authentication (`'aa-signer'` mode, since 1.19.0):**

For users whose Smart Account holds funds but whose EOA holds no ETH (email-only sign-in via Biconomy MEE, ZeroDev, Pimlico, etc.). All on-chain transactions route through a caller-supplied `sendUserOp` callback (bundler-agnostic); the EOA private key is used internally for off-chain signing only (`signMessage`, `signTypedData`, encrypted session init).

```typescript
import { FabstirSDKCore, type SendUserOpFn } from '@fabstir/sdk-core';

const sendUserOp: SendUserOpFn = async ({ to, data, value }) => {
  // Build + submit UserOp via your AA stack (Biconomy / ZeroDev / Pimlico / etc.)
  const { transactionHash } = await myBundler.sendUserOp({ to, data, value: value ?? 0n });
  return { transactionHash };  // sdk-core fetches the receipt itself
};

const sdk = new FabstirSDKCore({ /* ...config */ });
await sdk.authenticate('aa-signer', {
  smartAccountAddress: '0xSA...',  // 0x… — used as msg.sender for all chain tx
  eoaPrivateKey:        '0xEOA...', // 0x… — used ONLY for off-chain signing
  sendUserOp,
  rpcUrl:               'https://...',
  chainId:              84532,
});
```

**`SendUserOpFn` types** (importable from `@fabstir/sdk-core`):

```typescript
type SendUserOpCall   = { to: string; data: string; value?: bigint };  // value defaults to 0n
type SendUserOpResult = { transactionHash: string; [k: string]: unknown };  // extra fields permitted
type SendUserOpFn     = (call: SendUserOpCall) => Promise<SendUserOpResult>;
```

**Behavioral notes specific to `'aa-signer'` mode:**

- **Asymmetric address surface**: `sdk.getAddress()` returns the Smart Account address (the chain-side identity, used as `msg.sender` for every contract call). `sdk.getSigner()` returns the internal EOA `ethers.Wallet` for off-chain signing — `sdk.getSigner().getAddress()` returns the EOA address. **Do NOT** call `sendTransaction` on `sdk.getSigner()` — the EOA holds no ETH and the call will revert.
- **`tx.from` semantics**: when methods like `pm.depositNative` / `pm.sendEth` resolve, the returned `TransactionResponse.from` is the Smart Account address (matching `msg.sender` of the inner contract call), NOT the bundler relayer EOA that broadcast the on-chain tx. Use `tx.hash` for tx identity, not `tx.from`.
- **`switchChain()` not supported in 1.19.0**: throws `SDKError` with code `AA_SWITCH_CHAIN_UNSUPPORTED`. Workaround: `disconnect()` then re-`authenticate('aa-signer', { ...newChainOptions })`. Native AA-mode chain-switching is planned for v1.20.0.
- **Cross-RPC race tolerance**: when the caller's bundler RPC differs from sdk-core's `rpcUrl`, the SDK retries `getTransactionReceipt` up to 5× with 500ms backoff before throwing `AA_RECEIPT_NOT_VISIBLE`.

**SDKError codes from `authenticate('aa-signer', ...)`** (wrapped at the public-API layer as `AUTH_FAILED` with the inner code in the error message):
- `AA_SMART_ACCOUNT_MISSING`, `AA_EOA_KEY_MISSING`, `AA_SEND_USEROP_MISSING`, `AA_RPC_URL_MISSING`
- `AA_RECEIPT_NOT_VISIBLE` (cross-RPC race — bundler mined the tx but sdk-core's RPC hadn't seen it after 5 attempts)
- `AA_SIGN_TRANSACTION_UNSUPPORTED` (raised if anything tries to call `signTransaction` on the AA signer — use `sendTransaction` which routes through `sendUserOp`)

See `docs/fabstir-v2-reference/IMPLEMENTATION-AA-TRANSCODE-PAYMENT.md` Section A for the full design rationale.

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
  USDC: process.env.CONTRACT_USDC_TOKEN! // From .env.test
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
    USDC: process.env.CONTRACT_USDC_TOKEN! // From .env.test
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
      proofInterval: 100,
      proofTimeoutWindow: 300 // 5 min proof timeout
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
  proofTimeoutWindow?: number; // Proof timeout window in seconds (60-3600, default: 300)
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
    proofTimeoutWindow: 300, // 5 min proof timeout (60-3600, default: 300)
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
  onToken?: (token: string) => void,
  options?: PromptOptions
): Promise<string>
```

**PromptOptions:**
```typescript
interface PromptOptions {
  images?: ImageAttachment[];
  thinking?: ThinkingMode;
  onTokenUsage?: (usage: TokenUsageInfo) => void;
  onImageGenerated?: (result: ImageGenerationResult) => void;
  rawQuery?: string;
  signal?: AbortSignal;
  onContextWarning?: (usage: TokenUsageInfo) => void;
  contextWarningThreshold?: number;  // default 0.8
}
```

**TokenUsageInfo:**
```typescript
interface TokenUsageInfo {
  llmTokens: number;
  vlmTokens: number;
  imageGenTokens: number;
  totalTokens: number;
  promptTokens?: number;          // from node usage.prompt_tokens
  contextWindowSize?: number;     // from node usage.context_window_size
  contextUtilization?: number;    // 0.0-1.0 ratio
  finishReason?: 'stop' | 'length' | 'cancelled';
}
```

**Example:**
```typescript
const response = await sessionManager.sendPromptStreaming(
  sessionId,
  "Tell me a story",
  (token) => process.stdout.write(token),
  {
    onTokenUsage: (usage) => {
      console.log(`Context: ${Math.round((usage.contextUtilization ?? 0) * 100)}%`);
      if (usage.finishReason === 'length') {
        console.log('Response was truncated (max_tokens reached).');
      }
    },
    contextWarningThreshold: 0.75,
    onContextWarning: (usage) => {
      console.warn(`Context ${Math.round(usage.contextUtilization! * 100)}% full — trim history soon`);
    }
  }
);
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

### uploadVectors (Host-Side RAG)

Uploads vectors to the host node's session memory via WebSocket for RAG functionality.

```typescript
async uploadVectors(
  sessionId: string | BigInt,
  vectors: Vector[],
  replace?: boolean
): Promise<UploadVectorsResult>
```

**Parameters:**
- `sessionId`: Active session ID
- `vectors`: Array of vector objects with id, vector (384-d), and metadata
- `replace`: If true, replace all existing vectors (default: false)

**Returns:**
```typescript
interface UploadVectorsResult {
  uploaded: number;
  status: 'success' | 'error';
  error?: string;
}
```

**Example:**
```typescript
// Convert document chunks to vectors
const vectors = chunks.map((chunk, i) => ({
  id: `chunk-${i}`,
  vector: chunk.embedding,  // 384-dimensional array
  metadata: { text: chunk.text, index: i, source: 'doc.pdf' }
}));

// Upload to host (auto-batched at 1K vectors)
const result = await sessionManager.uploadVectors(sessionId, vectors);
console.log(`Uploaded ${result.uploaded} vectors`);
```

**Notes:**
- Vectors stored in session memory on host node (Rust)
- Auto-batched: 1000 vectors per WebSocket message
- Auto-cleanup: Vectors deleted when session ends
- No persistence: Host is stateless

### searchVectors (Host-Side RAG)

Searches for similar vectors on the host node via WebSocket.

```typescript
async searchVectors(
  sessionId: string | BigInt,
  queryVector: number[],
  k?: number,
  threshold?: number
): Promise<SearchResult[]>
```

**Parameters:**
- `sessionId`: Active session ID
- `queryVector`: Query embedding (384 dimensions)
- `k`: Number of results to return (default: 5, max: 20)
- `threshold`: Minimum similarity score (default: 0.2, range: 0.0-1.0)

**Returns:**
```typescript
interface SearchResult {
  id: string;
  score: number;           // Cosine similarity (0-1)
  metadata: Record<string, any>;
  vector?: number[];       // Optional: include full vector
}
```

**Example:**
```typescript
// Generate query embedding
const queryEmbedding = await embeddingService.embed('What is RAG?');

// Search with production-tested threshold
const results = await sessionManager.searchVectors(
  sessionId,
  queryEmbedding,
  5,    // topK: return top 5 results
  0.2   // threshold: 0.2 works best with all-MiniLM-L6-v2 (not 0.7!)
);

results.forEach(r => {
  console.log(`Score: ${r.score.toFixed(3)}, Text: ${r.metadata?.text}`);
});
```

**Threshold Selection Guide:**
- **0.0**: Accept all results (debugging only)
- **0.2**: Balanced filtering (recommended for production)
- **0.4**: Strict filtering (may miss relevant results)
- **0.7**: Too strict (returns 0 results with all-MiniLM-L6-v2)

**Performance:** ~100ms for 10K vectors (Rust implementation)

### askWithContext (RAG Helper)

Helper method that combines embedding generation, vector search, and context injection.

```typescript
async askWithContext(
  sessionId: string | BigInt,
  question: string,
  topK?: number
): Promise<string>
```

**Parameters:**
- `sessionId`: Active session ID
- `question`: User's question
- `topK`: Number of context chunks to retrieve (default: 5)

**Returns:** Enhanced prompt with RAG context injected

**Example:**
```typescript
// Automatic workflow: embed → search → format
const enhanced = await sessionManager.askWithContext(
  sessionId,
  'What are the key findings?',
  3  // Retrieve top 3 relevant chunks
);

// Send enhanced prompt to LLM
await sessionManager.sendPromptStreaming(sessionId, enhanced, (chunk) => {
  process.stdout.write(chunk.content);
});
```

**Context Format:**
```
Context:
[Document 1] <text from metadata>

[Document 2] <text from metadata>

Question: <original question>
```

**Notes:**
- Falls back to original question if no results found
- Automatically handles embedding generation
- Uses production-tested threshold (0.2)

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

### generateEmbeddings

Generates vector embeddings for document content using the host's embedding model. Part of the **deferred embeddings workflow** where documents are uploaded without embeddings and processed later during an active session.

```typescript
async generateEmbeddings(
  sessionId: bigint,
  fileContent: string,
  options?: {
    chunkSize?: number;
    chunkOverlap?: number;
    chainId?: number;
  }
): Promise<Vector[]>
```

**Parameters:**
- `sessionId` - Active session ID (must be started with a host)
- `fileContent` - Raw document text to chunk and embed
- `options.chunkSize` - Characters per chunk (default: 512)
- `options.chunkOverlap` - Overlap between chunks (default: 50)
- `options.chainId` - Blockchain chain ID (default: from session)

**Returns:** Array of vectors with embeddings and metadata

**Throws:**
- `SESSION_NOT_FOUND` - Session does not exist
- `SESSION_INACTIVE` - Session must be active
- `INVALID_CONTENT` - No valid chunks generated
- `EMBEDDING_TIMEOUT` - Operation exceeded 120 seconds
- `NETWORK_ERROR` - Failed to reach host embedding endpoint
- `EMBEDDING_FAILED` - Host returned error response

**Example:**
```typescript
// 1. Start session with host
const { sessionId } = await sessionManager.startSession({
  hostUrl: 'http://localhost:8080',
  jobId: 123n,
  modelName: 'llama-3',
  chainId: ChainId.BASE_SEPOLIA
});

// 2. Read document content
const fileContent = await file.text();

// 3. Generate embeddings via host
const vectors = await sessionManager.generateEmbeddings(
  sessionId,
  fileContent,
  {
    chunkSize: 512,      // Characters per chunk
    chunkOverlap: 50     // Overlap for context continuity
  }
);

console.log(`Generated ${vectors.length} embeddings`);
// vectors[0].values = [0.12, -0.45, ...] // 384D embedding
// vectors[0].metadata.text = "chunk text..."
// vectors[0].metadata.chunkIndex = 0
// vectors[0].metadata.model = "all-MiniLM-L6-v2"

// 4. Add to vector database
await vectorRAGManager.addVectors(databaseName, vectors);
```

**Architecture:**
- Calls host's `/v1/embed` endpoint (requires active session)
- Uses `all-MiniLM-L6-v2` model (384 dimensions)
- Automatic chunking with overlap for semantic continuity
- 120-second timeout for large documents
- Returns vectors ready to add to VectorRAGManager

**When to Use:**
- **Deferred embeddings workflow**: Upload documents without embeddings, process during session
- **Background processing**: Generate embeddings after document upload
- **Progress tracking**: Show real-time progress as chunks are embedded
- **Host-side processing**: Leverage host's GPU/CPU for embedding generation

**See Also:**
- `VectorRAGManager.getPendingDocuments()` - Get documents awaiting embeddings
- `VectorRAGManager.updateDocumentStatus()` - Update document embedding status

### getLastTokenUsage

Returns the token usage info from the last completed prompt for a session.

```typescript
getLastTokenUsage(sessionId: bigint): TokenUsageInfo | undefined
```

**Example:**
```typescript
const usage = sessionManager.getLastTokenUsage(sessionId);
if (usage) {
  console.log(`Last prompt used ${usage.totalTokens} tokens`);
  console.log(`Context: ${Math.round((usage.contextUtilization ?? 0) * 100)}%`);
}
```

### getContextInfo

Returns context window utilization info for a session. Returns `null` if no prompt has been sent yet or the node didn't report context usage.

```typescript
getContextInfo(sessionId: bigint): ContextInfo | null
```

**ContextInfo:**
```typescript
interface ContextInfo {
  promptTokens: number;
  completionTokens: number;
  contextWindowSize: number;
  utilization: number;              // 0.0-1.0
  finishReason: 'stop' | 'length' | 'cancelled' | null;
}
```

**Example:**
```typescript
const ctx = sessionManager.getContextInfo(sessionId);
if (ctx && ctx.utilization > 0.9) {
  // Trim conversation history before next message
}
```

### Checkpoint Recovery

The SDK provides methods to recover conversation state from checkpoint data when sessions are interrupted.

#### recoverFromBlockchainEvents (Recommended)

Recovers conversation from blockchain ProofSubmitted events. This decentralized method does NOT require the host to be online - deltaCIDs are permanently recorded on-chain.

```typescript
async recoverFromBlockchainEvents(
  jobId: bigint,
  options?: CheckpointQueryOptions
): Promise<BlockchainRecoveredConversation>
```

**Parameters:**
- `jobId`: The job/session ID to recover
- `options`: Optional query options (block range)
  - `fromBlock`: Starting block (default: auto-calculated for RPC limits)
  - `toBlock`: Ending block (default: 'latest')

**Returns:**
```typescript
interface BlockchainRecoveredConversation {
  messages: Message[];           // Recovered conversation messages
  tokenCount: number;            // Total tokens from last checkpoint
  checkpoints: BlockchainCheckpointEntry[];  // On-chain checkpoint data
}
```

**Example:**
```typescript
// After a session timeout or disconnect
const recovered = await sessionManager.recoverFromBlockchainEvents(jobId);

console.log(`Recovered ${recovered.messages.length} messages`);
console.log(`Total tokens: ${recovered.tokenCount}`);
console.log(`From ${recovered.checkpoints.length} on-chain checkpoints`);

// Display recovered conversation
recovered.messages.forEach(msg => {
  console.log(`[${msg.role}]: ${msg.content}`);
});
```

**Features:**
- **Decentralized**: Works without host being online
- **Encrypted**: Automatically decrypts deltas using user's recovery key
- **On-chain**: Data permanently recorded in ProofSubmitted events
- **Block Range**: Auto-calculates safe block range to avoid RPC limits

**Error Codes:**
- `DELTA_FETCH_FAILED`: Could not fetch delta from S5 storage
- `DECRYPTION_FAILED`: Decryption failed (wrong key or tampered data)
- `SIGNER_NOT_AVAILABLE`: SDK not properly authenticated

#### recoverFromCheckpoints (Deprecated)

> **Deprecated**: Use `recoverFromBlockchainEvents()` instead for decentralized recovery.

Recovers conversation via HTTP API from the host node. Requires host to be online.

```typescript
async recoverFromCheckpoints(
  sessionId: bigint
): Promise<RecoveredConversation>
```

This method is kept for backward compatibility with pre-Phase 9 sessions.

## Web Search Integration

The SDK provides automatic web search integration that enhances LLM responses with real-time information from the web. **Web search works seamlessly with both plaintext and encrypted sessions** (requires node v8.7.5+).

### Automatic Intent Detection

The SDK automatically detects when a prompt requires web search based on trigger patterns:

```typescript
// Triggers detected: "search for", "latest"
await sessionManager.sendPromptStreaming(sessionId, 'Search for latest NVIDIA GPU specs', onToken);
// → web_search: true (auto-enabled)

// No triggers detected
await sessionManager.sendPromptStreaming(sessionId, 'What is 2+2?', onToken);
// → web_search: false (no search overhead)
```

**Trigger Patterns:**
- Keywords: `search`, `find`, `look up`, `google`, `check online`
- Time references: `latest`, `recent`, `current`, `today`, `2025`, `2026`
- News patterns: `news about`, `what happened`

### Override Controls

Configure web search behavior on a per-prompt basis:

```typescript
// Force-enable web search (even without triggers)
await sessionManager.sendPromptStreaming(
  sessionId,
  'Tell me about quantum computing',
  onToken,
  { webSearch: { forceEnabled: true } }
);

// Force-disable web search (even with triggers)
await sessionManager.sendPromptStreaming(
  sessionId,
  'Search for the news',
  onToken,
  { webSearch: { forceDisabled: true } }
);

// Disable automatic detection
await sessionManager.sendPromptStreaming(
  sessionId,
  'Latest AI developments',
  onToken,
  { webSearch: { autoDetect: false } }
);

// Custom search configuration
await sessionManager.sendPromptStreaming(
  sessionId,
  'What is happening in tech?',
  onToken,
  {
    webSearch: {
      forceEnabled: true,
      maxSearches: 3,              // Limit number of searches
      queries: ['tech news 2026']  // Custom search queries
    }
  }
);
```

### searchDirect (HTTP Path)

Performs a direct web search via HTTP endpoint:

```typescript
const results = await sessionManager.searchDirect(
  sessionId,
  'NVIDIA RTX 5090 specifications',
  {
    numResults: 10,  // 1-20, default 10
    chainId: 84532   // Optional, uses session's chainId
  }
);

// Results structure
interface SearchApiResponse {
  results: Array<{
    title: string;
    url: string;
    content: string;
    score?: number;
  }>;
  query: string;
  search_provider: 'brave' | 'duckduckgo' | 'bing';
  timestamp: number;
}
```

### webSearch (WebSocket Path)

Performs a web search via WebSocket (real-time feedback):

```typescript
const results = await sessionManager.webSearch(
  sessionId,
  'latest AI breakthroughs'
);
// Returns same SearchApiResponse as searchDirect
```

### getWebSearchCapabilities

Check if a host supports web search:

```typescript
const hostManager = sdk.getHostManager();
const capabilities = await hostManager.getWebSearchCapabilities('http://host:8080');

interface WebSearchCapabilities {
  supportsWebSearch: boolean;        // Host has web search feature
  supportsInferenceSearch: boolean;  // Can inject search into inference
  supportsStreamingSearch: boolean;  // v8.7.5+ streaming support
  supportsWebSocketSearch: boolean;  // v8.7.5+ WebSocket integration
  provider: 'brave' | 'duckduckgo' | 'bing' | null;
  rateLimitPerMinute: number;
}

if (capabilities.supportsWebSearch) {
  console.log(`Provider: ${capabilities.provider}`);
  console.log(`Rate limit: ${capabilities.rateLimitPerMinute}/min`);
}
```

### searchWithRetry Utility

Retry search with exponential backoff:

```typescript
import { searchWithRetry } from '@fabstir/sdk-core';

const results = await searchWithRetry(
  () => sessionManager.searchDirect(sessionId, 'query'),
  {
    maxRetries: 3,           // Default: 3
    initialDelayMs: 1000,    // Default: 1000ms
    maxDelayMs: 30000        // Default: 30000ms
  }
);
```

### WebSearchError

Handle web search errors:

```typescript
import { WebSearchError } from '@fabstir/sdk-core';

try {
  await sessionManager.searchDirect(sessionId, 'query');
} catch (error) {
  if (error instanceof WebSearchError) {
    switch (error.code) {
      case 'rate_limited':
        // Wait for Retry-After header or backoff
        break;
      case 'timeout':
        // Search timed out (30s default)
        break;
      case 'provider_error':
        // Search provider returned error
        break;
      case 'not_supported':
        // Host doesn't support web search
        break;
    }
  }
}
```

### Encrypted Session Support

Web search works transparently with encrypted sessions (v8.7.5+ nodes):

```typescript
// Start encrypted session (default)
const { sessionId } = await sessionManager.startSession({
  hostUrl: 'http://localhost:8080',
  jobId: 123n,
  modelName: 'llama-3',
  chainId: ChainId.BASE_SEPOLIA
  // encryption: true is default
});

// Web search auto-detected, works with encryption
await sessionManager.sendPromptStreaming(
  sessionId,
  'Search for the latest GPU prices',
  onToken
);
// → Encrypted message with web_search: true at message level
```

**Important**: The `web_search`, `max_searches`, and `search_queries` fields are sent at the message level (not inside the encrypted payload), allowing the node to perform web search before processing the encrypted content.

## Image Generation

The SDK supports text-to-image generation using host-side diffusion models (FLUX.2 Klein 4B). Image generation uses the same E2E encrypted WebSocket channel as inference, with an HTTP path available for testing.

### generateImage (Encrypted WebSocket)

Generate an image via the encrypted WebSocket session (production path):

```typescript
const sessionManager = await sdk.getSessionManager();

const result = await sessionManager.generateImage(
  sessionId,
  'A cat astronaut floating in space',
  {
    size: '512x512',    // Default: '1024x1024'
    steps: 4,           // Default: 4 (range: 1-100)
    model: 'flux-1-schnell',  // Optional model override
    seed: 42,           // Optional seed for reproducibility
    negativePrompt: 'blurry, low quality',  // Optional
    safetyLevel: 'strict'  // 'strict' | 'moderate' | 'permissive'
  }
);

// Display image
const imgSrc = `data:image/png;base64,${result.image}`;

console.log(`Size: ${result.size}`);
console.log(`Steps: ${result.steps}`);
console.log(`Seed: ${result.seed}`);
console.log(`Processing: ${result.processingTimeMs}ms`);
console.log(`Billing: ${result.billing.generationUnits} units`);
```

**Parameters:**
- `sessionId` (string) - Active encrypted session ID
- `prompt` (string) - Text description (1-2000 characters)
- `options` (ImageGenerationOptions, optional) - Generation options

**Returns:** `Promise<ImageGenerationResult>`

```typescript
interface ImageGenerationResult {
  image: string;           // Base64-encoded PNG
  model: string;           // Model used
  size: string;            // e.g. '512x512'
  steps: number;           // Diffusion steps
  seed: number;            // Seed used
  processingTimeMs: number;
  safety: SafetyInfo;      // { promptSafe, outputSafe, safetyLevel }
  billing: BillingInfo;    // { generationUnits, modelMultiplier, megapixels, steps }
  provider: string;        // Host address
  chainId: number;
  chainName: string;
  nativeToken: string;
}
```

**Throws:** `ImageGenerationError` with codes:
- `VALIDATION_FAILED` - Invalid prompt, size, or steps
- `PROMPT_BLOCKED` - Safety classifier rejected the prompt
- `DIFFUSION_SERVICE_UNAVAILABLE` - Host diffusion sidecar is down
- `IMAGE_GENERATION_FAILED` - Generation failed (retryable)
- `RATE_LIMIT_EXCEEDED` - Client-side rate limit (5 req/min, retryable)
- `ENCRYPTION_FAILED` - Encryption/decryption error

### generateImageHttpRequest (HTTP Path)

Generate an image via HTTP POST (testing/CI path, no E2E encryption):

```typescript
const result = await sessionManager.generateImageHttpRequest(
  'http://host:8080',
  'A serene mountain lake at golden hour',
  { size: '512x512', steps: 4 }
);
```

**Parameters:**
- `hostUrl` (string) - Base URL of the host node
- `prompt` (string) - Text description (1-2000 characters)
- `options` (ImageGenerationOptions, optional) - Generation options

**Returns:** `Promise<ImageGenerationResult>` (same as `generateImage`)

### getImageGenerationCapabilities

Check if a host supports image generation:

```typescript
const hostManager = sdk.getHostManager();
const capabilities = await hostManager.getImageGenerationCapabilities(hostAddress, apiUrl);

interface ImageGenerationCapabilities {
  supportsImageGeneration: boolean;      // Host has diffusion sidecar
  supportsEncryptedWebSocket: boolean;   // Encrypted WebSocket path available
  supportsHttp: boolean;                 // HTTP path available
  hasSafetyClassifier: boolean;          // Prompt safety checking
  hasOutputClassifier: boolean;          // Output safety checking
  hasBilling: boolean;                   // Billing computation
  hasContentHashes: boolean;             // Content hash generation
}

if (capabilities.supportsImageGeneration) {
  console.log(`WebSocket: ${capabilities.supportsEncryptedWebSocket}`);
  console.log(`HTTP: ${capabilities.supportsHttp}`);
}
```

### ImageGenerationError

Handle image generation errors:

```typescript
import { ImageGenerationError } from '@fabstir/sdk-core';

try {
  await sessionManager.generateImage(sessionId, prompt);
} catch (error) {
  if (error instanceof ImageGenerationError) {
    switch (error.code) {
      case 'RATE_LIMIT_EXCEEDED':
        console.log(`Retry in ${Math.ceil(error.retryAfter! / 1000)}s`);
        break;
      case 'PROMPT_BLOCKED':
        console.log('Prompt rejected by safety classifier');
        break;
      case 'DIFFUSION_SERVICE_UNAVAILABLE':
        console.log('Diffusion sidecar not running on host');
        break;
      case 'IMAGE_GENERATION_FAILED':
        console.log('Generation failed, retryable:', error.isRetryable);
        break;
    }
  }
}
```

### Allowed Image Sizes

```typescript
import { ALLOWED_IMAGE_SIZES, isValidImageSize } from '@fabstir/sdk-core';

// ALLOWED_IMAGE_SIZES = ['256x256', '512x512', '768x768', '1024x1024', '1024x768', '768x1024']

if (isValidImageSize('512x512')) {
  // Valid size
}
```

### Automatic Image Intent Detection (v1.14.10+)

The SDK auto-detects image generation intent from natural language prompts in `sendPromptStreaming()`. When detected, it routes to `generateImage()` automatically — no UI toggle required.

```typescript
// Auto-detected: routes to generateImage(), fires callback
await sessionManager.sendPromptStreaming(
  sessionId,
  'Generate an image of a cat astronaut in 1024x1024',
  (token) => process.stdout.write(token),
  {
    onImageGenerated: (result) => {
      // result.image = base64 PNG
      const imgSrc = `data:image/png;base64,${result.image}`;
      console.log(`Generated ${result.size} in ${result.processingTimeMs}ms`);
    }
  }
);
// Returns "Image generated successfully" as text response

// NOT detected: goes to normal LLM inference
await sessionManager.sendPromptStreaming(sessionId, 'What is in this image?', onToken);
// → Normal LLM response (no false positive)
```

**Trigger patterns:** `generate an image of`, `draw a`, `create a picture of`, `paint a`, `sketch a`, `make an image of`, `render a` — including polite forms (`please generate`, `can you draw`).

**Not triggered by:** `describe the image`, `what is in this image`, `how to draw in CSS`, `image processing algorithm`, etc.

**Parameters auto-extracted from prompt:**
- Size: `"...in 1024x1024"` → `{ size: '1024x1024' }`
- Steps: `"...with 20 steps"` → `{ steps: 20 }`

**Fallback:** If image generation fails, the prompt silently falls back to normal LLM inference.

**Direct usage:**

```typescript
import { analyzePromptForImageIntent } from '@fabstir/sdk-core';

const result = analyzePromptForImageIntent('Generate an image of a cat in 512x512');
// { isImageIntent: true, cleanPrompt: 'a cat', extractedOptions: { size: '512x512' } }
```

### Billing Estimation

```typescript
import { estimateGenerationUnits, parseSize } from '@fabstir/sdk-core';

const { width, height } = parseSize('1024x1024');
const units = estimateGenerationUnits(width, height, 4);
// Formula: (width * height / 1_048_576) * (steps / 20) * modelMultiplier
// 1024x1024, 4 steps = 0.2 units
```

### OpenAI-Compatible Bridge

The `@fabstir/openai-bridge` package exposes Fabstir's AI infrastructure via standard OpenAI API endpoints. Any OpenAI SDK client (Cursor, Continue, OpenCode, LangChain) can use Fabstir hosts without code changes.

```bash
npx fabstir-openai-bridge --private-key $KEY --model "repo:file"
# Listens on http://localhost:3457
# Client sets OPENAI_BASE_URL=http://localhost:3457/v1
```

Supported endpoints:
- `POST /v1/chat/completions` — streaming and non-streaming text, tool use, vision
- `POST /v1/images/generations` — image generation (maps to `SessionManager.generateImage()`)
- `POST /v1/responses` — OpenAI Responses API format
- `GET /v1/models` — model listing

The bridge handles blockchain session management, host discovery, E2E encryption, and session re-keying automatically. See [`packages/openai-bridge/`](../packages/openai-bridge/) for CLI options and configuration.

## Video Transcoding

GPU-accelerated video transcoding via encrypted WebSocket. Source videos are uploaded to S5 (optionally encrypted), transcoded by the host's sidecar, and output CIDs returned for download.

### Get TranscodeManager

```typescript
const transcodeManager = sdk.getTranscodeManager();
```

### Feature Detection

```typescript
const hasTranscoding = await transcodeManager.isTranscodingAvailable(hostUrl);
const hasTrustless = await transcodeManager.isTrustlessAvailable(hostUrl);
```

### Price Estimation

```typescript
const estimate = await transcodeManager.estimateTranscodePrice(hostAddress, formatSpec, durationSeconds);
// { totalCost, breakdown: { duration, pricePerSecond, resolution, codec, quality } }
```

### Submit Transcode via SessionManager

`submitTranscode` is on the concrete `SessionManager` class (not `ISessionManager`), following the same pattern as `generateImage`:

```typescript
const sessionManager = sdk.getSessionManager() as any; // Cast to access submitTranscode

const handle = await sessionManager.submitTranscode(sessionId, sourceCid, formats, {
  isGpu: true,
  isEncrypted: true, // true if source is encrypted on S5
  onProgress: (progress, gopInfo) => {
    console.log(`${progress}% — GOP ${gopInfo?.currentGop}/${gopInfo?.totalGops}`);
  },
});

console.log(`Task: ${handle.taskId}`);
handle.cancel(); // Cancel mid-transcode

const result = await handle.result;
// { taskId, outputs: TranscodeOutputUnion[], billing: { units, tokens }, duration, qualityMetrics, proofTreeCID, proofTreeRootHash }
// Each output is either { id, ext, cid } (standard) or { id, hls: true, initSegmentCid, segments, ... } (HLS)
```

### TranscodeSubmitOptions

```typescript
interface TranscodeSubmitOptions {
  isEncrypted?: boolean;   // Source video encrypted on S5 (default: true)
  isGpu?: boolean;         // Use GPU acceleration (default: true)
  chainId?: number;        // Blockchain context
  onProgress?: (progress: number, gopInfo?: GOPInfo) => void;
  timeoutMs?: number;      // Default: 300000 (5 min)
  previewPercent?: number; // HLS: first N% of segments uploaded unencrypted (default: 0)
}
```

### Load-Balanced Transcode Submission

`submitTranscodeWithLoadBalancing` on `TranscodeManager` ranks available hosts, checks capacity, and submits to the best host with available slots. If the preferred host is full, it overflows to the next ranked host automatically.

```typescript
const transcodeManager = sdk.getTranscodeManager() as any; // Cast — not on ITranscodeManager interface
const handle = await transcodeManager.submitTranscodeWithLoadBalancing(sourceCid, formats, modelId, {
  hostSelectionMode: HostSelectionMode.AUTO,
  maxHostRetries: 5,
  isGpu: true,
  isEncrypted: true,
  depositAmount: '0.0002',
  onHostSelected: (address, url) => console.log(`Selected: ${address} @ ${url}`),
  onProgress: (progress, gopInfo) => console.log(`${progress}%`),
});
const result = await handle.result;
```

**Pending job tracking**: The load balancer tracks in-flight jobs per host so that rapid sequential submissions don't all land on the same host. `effectiveAvailable = capacity.available - pendingJobs` ensures overflow to secondary hosts even when the capacity endpoint hasn't yet updated.

**Per-transcode WebSocket connections**: Each `submitTranscode` call creates its own WebSocket connection and session key, enabling multiple concurrent transcode jobs without WebSocket message clobbering.

### TranscodeLoadBalancedOptions

```typescript
interface TranscodeLoadBalancedOptions extends TranscodeSubmitOptions {
  maxHostRetries?: number;       // Max hosts to try (default: 3)
  hostSelectionMode?: HostSelectionMode; // AUTO, CHEAPEST, RELIABLE, FASTEST
  onHostSelected?: (hostAddress: string, hostUrl: string) => void;
  depositAmount?: string;        // Session deposit (default: '0.0002')
  duration?: number;             // Session duration in seconds (default: 3600)
  proofInterval?: number;        // Proof interval (default: 100)
  encryption?: boolean;          // E2E encryption (default: true)
}
```

### TranscodeCapacity

Runtime capacity snapshot from a host's transcode sidecar, returned by `fetchTranscodeCapacity(hostUrl)`:

```typescript
interface TranscodeCapacity {
  active: number;           // Currently running transcode jobs
  max: number;              // Maximum concurrent slots
  queued: number;           // Jobs waiting in sidecar queue (node v8.27.0+)
  available: number;        // Slots available for new jobs
  sidecarConnected: boolean; // Whether transcoder sidecar is connected
}
```

**HTTP 429 handling**: When a host's sidecar is full, the capacity endpoint returns HTTP 429. The SDK maps this to `TranscodeError` with code `CAPACITY_FULL` (retryable), causing the load balancer to try the next host.

### VideoFormat

Formats use `VideoFormat[]` matching the node's ffmpeg parameters:

```typescript
const formats: VideoFormat[] = [{
  id: 1, ext: 'mp4', vcodec: 'h264_nvenc', acodec: 'aac',
  preset: 'fast', vf: 'scale=1920x1080', b_v: '5M',
  ar: '48k', ch: 2, dest: 's5',
  encrypt: true,        // Per-format encryption (encrypts output on S5)
  trim_percent: 15,     // Transcode only first N% of source duration
}];
```

**Streaming fields (Phase 1 — whole-file):**
- `encrypt` (boolean, optional): When `true`, the output is encrypted on S5. Used to produce encrypted full-length outputs alongside unencrypted previews.
- `trim_percent` (number, optional): Transcode only the first N% of the source video. The node sidecar computes `-t <duration * trim_percent / 100>` for ffmpeg. Used to produce free preview clips.

**HLS fields (Phase 2 — segmented):**
- `hls` (boolean, optional): When `true`, output fMP4 segments instead of a single file. `encrypt` and `trim_percent` are ignored for HLS formats.
- `hls_time` (number, optional): Target segment duration in seconds (default: 6).

### Streaming Content Pipeline

For streaming content with free previews, use `buildStreamingFormats()` to produce two outputs per resolution — an encrypted full-length version and an unencrypted trimmed preview:

```typescript
import { buildStreamingFormats, selectResolution, assembleContentMetadata } from '@fabstir/sdk-core';

// Build format array: 2 outputs per resolution (full + preview)
const formats = buildStreamingFormats(['720p', '1080p'], 'h264', 15);
// Returns 4 formats (ordered by the resolutions array):
//   id 1: 720p full (encrypt: true)
//   id 2: 720p preview (encrypt: false, trim_percent: 15)
//   id 3: 1080p full (encrypt: true)
//   id 4: 1080p preview (encrypt: false, trim_percent: 15)

// When previewPercent is 0, produces 1 output per resolution (no encrypt/trim flags)
const simpleFormats = buildStreamingFormats(['1080p'], 'h264', 0);
// Returns 1 format: id 1, 1080p (same as legacy single-output behavior)
```

**After transcode completes**, assemble metadata pairing preview and full CIDs:

```typescript
const result = await handle.result;
const metadata = assembleContentMetadata(result, formats, sourceCid, 15, jobId);
// metadata: {
//   sourceCid, transcodedAt, freePreviewPercent: 15, jobId,
//   sources: [
//     { resolution: '1080p', previewCid: '...', fullCid: '...', codec: 'h264', container: 'mp4', bitrateKbps: 5000 },
//     { resolution: '720p',  previewCid: '...', fullCid: '...', codec: 'h264', container: 'mp4', bitrateKbps: 2000 },
//   ]
// }
```

**Select best resolution** for playback based on device/bandwidth:

```typescript
const best = selectResolution(metadata.sources, { maxHeight: 720, bandwidthKbps: 3000 });
// Returns the TranscodedSource for the best matching resolution
```

**Types:**

```typescript
interface TranscodedSource {
  resolution: string;      // '480p' | '720p' | '1080p' | '2160p'
  previewCid: string;      // Unencrypted, first N% — freely accessible
  fullCid: string;         // Encrypted, complete — requires purchase/key
  codec: string;           // 'h264' | 'hevc' | 'av1'
  container: string;       // 'mp4' | 'webm'
  bitrateKbps: number;
  proofTreeCid?: string;
}

interface TranscodedContentMetadata {
  sourceCid: string;
  transcodedAt: number;        // Unix timestamp
  freePreviewPercent: number;  // e.g. 15
  sources: TranscodedSource[];
  jobId: number;               // On-chain job ID
}
```

### HLS Adaptive Bitrate Streaming

For adaptive bitrate streaming with per-segment encryption, use `buildHlsFormats()`. Unlike `buildStreamingFormats()` (which produces two outputs per resolution), HLS produces one format per resolution — the preview/paid split is handled per-segment via `previewPercent` at the request level.

```typescript
import { buildHlsFormats, buildMasterPlaylist, buildVariantPlaylist,
  assembleHlsContentMetadata, isHlsOutput } from '@fabstir/sdk-core';

// Build format array: 1 format per resolution with hls: true
const formats = buildHlsFormats(['720p', '1080p', '2160p'], 'av1', 6);
// Returns 3 formats:
//   id 1: 720p  (hls: true, hls_time: 6)
//   id 2: 1080p (hls: true, hls_time: 6)
//   id 3: 2160p (hls: true, hls_time: 6)

// Submit with previewPercent (first 15% of segments unencrypted)
const handle = await (sessionManager as any).submitTranscode(
  sessionId.toString(), sourceCid, formats, {
    isGpu: true, isEncrypted: true, previewPercent: 15,
    onProgress: (p, gop) => console.log(`${p}%`),
  },
);
const result = await handle.result;
```

**After transcode completes**, assemble HLS metadata:

```typescript
const metadata = assembleHlsContentMetadata(result, formats, sourceCid, 15, jobId);
// metadata: {
//   sourceCid, transcodedAt, freePreviewPercent: 15, jobId,
//   sources: [
//     { resolution: '720p', codec: 'av1', container: 'mp4', bitrateKbps: 1500,
//       initSegmentCid: 'z...', segments: [...], previewSegments: 15, totalDuration: 598 },
//     { resolution: '1080p', ... },
//     { resolution: '2160p', ... },
//   ]
// }
```

**Generate M3U8 playlists** client-side:

```typescript
// Master playlist (adaptive bitrate switching)
const master = buildMasterPlaylist(metadata.sources, res => `${res}/index.m3u8`);
// #EXTM3U
// #EXT-X-STREAM-INF:BANDWIDTH=1500000,RESOLUTION=1280x720
// 720p/index.m3u8
// #EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1920x1080
// 1080p/index.m3u8
// ...

// Variant playlist (single resolution, all segments)
const variant = buildVariantPlaylist(metadata.sources[0], cid => `https://s5.example/${cid}`);
// #EXTM3U
// #EXT-X-VERSION:7
// #EXT-X-TARGETDURATION:7
// #EXT-X-MAP:URI="https://s5.example/zInitSeg..."
// #EXTINF:6.006000,
// https://s5.example/zSeg0...
// ...
// #EXT-X-ENDLIST
```

**Discriminate HLS from standard outputs** using the type guard:

```typescript
for (const output of result.outputs) {
  if (isHlsOutput(output)) {
    // output: HlsOutput — has initSegmentCid, segments[], previewSegments, etc.
    console.log(`HLS: ${output.segments.length} segments`);
  } else {
    // output: TranscodedOutput — has cid (single file)
    console.log(`Standard: ${output.cid}`);
  }
}
```

**HLS Types:**

```typescript
interface HlsSegment {
  index: number;           // 0-based segment position
  cid: string;             // z-prefix (unencrypted) or u-prefix (encrypted)
  duration: number;        // Actual segment duration in seconds
  encrypted: boolean;      // true if segment is encrypted on S5
}

interface HlsOutput {
  id: number;
  hls: true;               // Literal true — discriminant for type guard
  initSegmentCid: string;  // fMP4 init segment (always unencrypted, z-prefix)
  segments: HlsSegment[];
  previewSegments: number; // Count of unencrypted preview segments
  totalSegments: number;
  totalDuration: number;   // Sum of all segment durations
}

type TranscodeOutputUnion = TranscodedOutput | HlsOutput;

function isHlsOutput(o: TranscodeOutputUnion): o is HlsOutput;

interface HlsTranscodedSource {
  resolution: string;      // '480p' | '720p' | '1080p' | '2160p'
  codec: string;           // 'h264' | 'av1'
  container: string;       // 'mp4'
  bitrateKbps: number;
  initSegmentCid: string;
  segments: HlsSegment[];
  previewSegments: number;
  totalDuration: number;
}

interface HlsContentMetadata {
  sourceCid: string;
  transcodedAt: number;
  freePreviewPercent: number;
  sources: HlsTranscodedSource[];
  jobId: number;
}
```

### Model IDs for Transcoding

Transcode model IDs are computed from the canonical JSON of the `VideoFormat[]` array — not from a model name string. Use the SDK utility:

```typescript
import { computeTranscodeModelId } from '@fabstir/sdk-core';

const modelId = computeTranscodeModelId(formats);
// Internally: sorts by id, maps to canonical field order, JSON.stringify, keccak256
// Deterministic — same formats always produce the same model ID
```

### Encrypted Source Upload

Upload source video encrypted to S5 using `uploadBlobEncrypted`, then construct the S5 encrypted CID (0xae format) which embeds the decryption key:

```typescript
const s5 = storageManager.getS5Client();
const result = await s5.fs.uploadBlobEncrypted(blob);
// result: { hash, size, encryptionKey, encryptedBlobHash, padding }

// Construct encrypted CID (0xae format — sidecar extracts key automatically)
const encCID = new Uint8Array([0xae, 0x01, 18, 0x1f, ...result.encryptedBlobHash, ...result.encryptionKey, ...paddingLE, ...plaintextCIDTail]);
const sourceCid = `s5://${base64url.encode(encCID)}`; // u-prefix
```

---

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
    proofTimeoutWindow?: number; // 60-3600 seconds, default: 300
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
    proofInterval: 100,
    proofTimeoutWindow: 300 // 5 min proof timeout (60-3600)
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
    proofTimeoutWindow?: number; // 60-3600 seconds, default: 300
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

### getAvailableModelsWithHosts

Gets all approved models with host availability and pricing information.

```typescript
async getAvailableModelsWithHosts(): Promise<ModelWithAvailability[]>
```

**Returns:**
```typescript
interface ModelWithAvailability {
  model: ModelInfo;
  hostCount: number;        // Number of hosts serving this model
  priceRange: {
    min: bigint;            // Lowest host price
    max: bigint;            // Highest host price
    avg: bigint;            // Average price
  };
  isAvailable: boolean;     // hostCount > 0
}
```

**Throws:**
- `Error` if HostManager not set via `setHostManager()`

**Example:**
```typescript
const modelManager = sdk.getModelManager();
modelManager.setHostManager(sdk.getHostManager());

const models = await modelManager.getAvailableModelsWithHosts();

// Show available models
models
  .filter(m => m.isAvailable)
  .forEach(m => {
    console.log(`${m.model.fileName}: ${m.hostCount} hosts`);
    console.log(`  Price range: ${m.priceRange.min} - ${m.priceRange.max}`);
  });
```

### getModelPriceRange

Gets price range for a specific model across all hosts.

```typescript
async getModelPriceRange(modelId: string): Promise<{
  min: bigint;
  max: bigint;
  avg: bigint;
  hostCount: number;
}>
```

**Parameters:**
- `modelId` - Model ID to get pricing for

**Returns:**
- Price range with min, max, avg, and host count
- All values are 0 if no hosts serve this model

**Example:**
```typescript
const modelManager = sdk.getModelManager();
modelManager.setHostManager(sdk.getHostManager());

const pricing = await modelManager.getModelPriceRange(modelId);

if (pricing.hostCount > 0) {
  console.log(`${pricing.hostCount} hosts available`);
  console.log(`Cheapest: ${pricing.min} (raw USDC units)`);
  console.log(`Average: ${pricing.avg}`);
  console.log(`Most expensive: ${pricing.max}`);
} else {
  console.log('No hosts available for this model');
}
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

### setModelTokenPricing

Sets per-model per-token pricing for a specific model. Phase 18 replacement for `updatePricingNative`, `updatePricingStable`, and `setModelPricing`.

```typescript
async setModelTokenPricing(modelId: string, tokenAddress: string, price: string): Promise<string>
```

**Parameters:**
- `modelId`: bytes32 model ID hash
- `tokenAddress`: Token address — USDC address for stablecoin, `0x0000...0000` for native ETH
- `price`: Price value — USDC uses PRICE_PRECISION (1000 = $1/million tokens), native uses wei

**Example:**
```typescript
const hostManager = sdk.getHostManager();

// Set USDC price to $5/million tokens (5 * 1000 = 5000)
const usdcAddr = process.env.CONTRACT_USDC_TOKEN;
const tx1 = await hostManager.setModelTokenPricing(modelId, usdcAddr, '5000');

// Set ETH price (in wei per million tokens)
const zeroAddr = '0x0000000000000000000000000000000000000000';
const tx2 = await hostManager.setModelTokenPricing(modelId, zeroAddr, '15909090909091');
```

### clearModelTokenPricing

Clears per-model pricing for a specific token. Must be called per-token (native and USDC separately).

```typescript
async clearModelTokenPricing(modelId: string, tokenAddress: string): Promise<string>
```

**Parameters:**
- `modelId`: bytes32 model ID hash
- `tokenAddress`: Token address to clear pricing for

**Example:**
```typescript
const hostManager = sdk.getHostManager();

// Clear USDC pricing for a model
const tx = await hostManager.clearModelTokenPricing(modelId, usdcAddr);
```

### getHostModelPrices

Fetches all per-model prices for a host for a specific token.

```typescript
async getHostModelPrices(hostAddress: string, tokenAddress: string): Promise<ModelPricing[]>
```

**Returns:** Array of `{ modelId: string, price: bigint }` — entries with `price === 0n` are filtered out.

**Example:**
```typescript
const hostManager = sdk.getHostManager();

// Get all USDC model prices
const usdcPrices = await hostManager.getHostModelPrices(hostAddress, usdcAddr);

// Get all native model prices
const nativePrices = await hostManager.getHostModelPrices(hostAddress, zeroAddr);
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
  V1 = 1,  // Initial version
  V2 = 2   // Model/host selection preferences
}
```

**Version 2 New Fields:**
```typescript
// Host selection mode (added in V2)
export enum HostSelectionMode {
  AUTO = 'auto',           // Weighted algorithm (default)
  CHEAPEST = 'cheapest',   // Lowest price first
  RELIABLE = 'reliable',   // Highest stake + reputation
  FASTEST = 'fastest',     // Lowest latency (placeholder)
  SPECIFIC = 'specific'    // Use preferredHostAddress
}

interface UserSettingsV2 {
  // ... V1 fields preserved ...

  // Model Preferences (NEW in V2)
  defaultModelId: string | null;     // User's default model

  // Host Preferences (NEW in V2)
  hostSelectionMode: HostSelectionMode;  // Selection algorithm
  preferredHostAddress: string | null;   // For SPECIFIC mode
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

### AI Preference Helper Methods

The StorageManager provides convenience methods for managing model and host selection preferences.

#### getDefaultModel(): Promise<ModelInfo | null>

Get the user's default model. Returns `null` if no default is set.

**Returns:**
- `ModelInfo` if a default model is set and exists
- `null` if no default is set or model doesn't exist

**Throws:**
- `Error` if ModelManager not set via `setModelManager()`

**Example:**
```typescript
const storageManager = await sdk.getStorageManager();
storageManager.setModelManager(sdk.getModelManager());

const defaultModel = await storageManager.getDefaultModel();
if (defaultModel) {
  console.log('Default model:', defaultModel.fileName);
} else {
  console.log('No default model set');
}
```

#### setDefaultModel(modelId: string | null): Promise<void>

Set the user's default model. Pass `null` to clear the default.

**Parameters:**
- `modelId` - Model ID to set as default, or `null` to clear

**Throws:**
- `Error` if ModelManager not set
- `Error` if model not found in registry

**Example:**
```typescript
const storageManager = await sdk.getStorageManager();
storageManager.setModelManager(sdk.getModelManager());

// Set a default model
await storageManager.setDefaultModel('0xabc123...'); // Model ID

// Clear the default
await storageManager.setDefaultModel(null);
```

#### getHostSelectionMode(): Promise<HostSelectionMode>

Get the user's host selection mode. Returns `AUTO` if not set.

**Returns:**
- `HostSelectionMode` - Current selection mode

**Example:**
```typescript
import { HostSelectionMode } from '@fabstir/sdk-core';

const storageManager = await sdk.getStorageManager();
const mode = await storageManager.getHostSelectionMode();

switch (mode) {
  case HostSelectionMode.AUTO:
    console.log('Using weighted algorithm');
    break;
  case HostSelectionMode.CHEAPEST:
    console.log('Selecting cheapest hosts');
    break;
  case HostSelectionMode.RELIABLE:
    console.log('Selecting most reliable hosts');
    break;
  case HostSelectionMode.SPECIFIC:
    console.log('Using specific preferred host');
    break;
}
```

#### setHostSelectionMode(mode: HostSelectionMode, preferredHostAddress?: string): Promise<void>

Set the user's host selection mode.

**Parameters:**
- `mode` - HostSelectionMode to set
- `preferredHostAddress` - Required for `SPECIFIC` mode

**Throws:**
- `Error` if `SPECIFIC` mode without `preferredHostAddress`

**Example:**
```typescript
import { HostSelectionMode } from '@fabstir/sdk-core';

const storageManager = await sdk.getStorageManager();

// Use cheapest hosts
await storageManager.setHostSelectionMode(HostSelectionMode.CHEAPEST);

// Use a specific host
await storageManager.setHostSelectionMode(
  HostSelectionMode.SPECIFIC,
  '0x1234567890123456789012345678901234567890'
);

// Back to auto
await storageManager.setHostSelectionMode(HostSelectionMode.AUTO);
```

#### clearAIPreferences(): Promise<void>

Reset all AI preferences to defaults without affecting other settings.

**Behavior:**
- `defaultModelId` → `null`
- `hostSelectionMode` → `AUTO`
- `preferredHostAddress` → `null`

**Example:**
```typescript
const storageManager = await sdk.getStorageManager();

// Reset AI preferences (keeps theme, payment token, etc.)
await storageManager.clearAIPreferences();
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

### S5 Connection Handling (v1.4.24+)

The StorageManager includes robust connection handling for mobile browsers where WebSocket connections frequently die due to background tab throttling, device sleep, or network switching.

#### Connection Status

##### getConnectionStatus(): S5ConnectionStatus

Get the current S5 WebSocket connection status.

**Returns:**
- `'connected'` - WebSocket is open and ready
- `'connecting'` - WebSocket is establishing connection
- `'disconnected'` - WebSocket is closed or in error state

**Example:**
```typescript
const storageManager = await sdk.getStorageManager();
const status = storageManager.getConnectionStatus();
console.log('S5 connection:', status); // 'connected' | 'connecting' | 'disconnected'
```

#### Sync Status

##### getSyncStatus(): SyncStatus

Get the current sync status for UI indicators.

**Returns:**
- `'synced'` - All operations completed successfully
- `'syncing'` - Currently processing queued operations
- `'pending'` - Operations queued, waiting for connection
- `'error'` - Last operation failed after retries

**Example:**
```typescript
const status = storageManager.getSyncStatus();
if (status === 'pending') {
  console.log(`${storageManager.getPendingOperationCount()} operations waiting`);
}
```

##### getPendingOperationCount(): number

Get the number of operations waiting in the queue.

**Returns:**
- `number` - Count of pending PUT/DELETE operations

##### getLastError(): Error | null

Get the last error if sync status is 'error'.

**Returns:**
- `Error` - The error that caused the 'error' status
- `null` - No error (status is not 'error')

##### onSyncStatusChange(callback): () => void

Subscribe to sync status changes. Callback fires immediately with current status.

**Parameters:**
- `callback` (function) - Called with new status on each change

**Returns:**
- `() => void` - Unsubscribe function

**Example:**
```typescript
const unsubscribe = storageManager.onSyncStatusChange((status) => {
  switch (status) {
    case 'synced':
      hideSpinner();
      showGreenCheck();
      break;
    case 'syncing':
      showSpinner();
      break;
    case 'pending':
      showYellowWarning(`${storageManager.getPendingOperationCount()} pending`);
      break;
    case 'error':
      showRedError(storageManager.getLastError()?.message);
      break;
  }
});

// Cleanup on component unmount
useEffect(() => unsubscribe, []);
```

#### Retry Operations

##### putWithRetry(path, data): Promise<void>

PUT operation with automatic retry and queue support.

**Behavior:**
- If connected: Executes with up to 5 retries (exponential backoff: 1s, 2s, 4s, 8s, 16s)
- If disconnected: Queues operation, resolves when connection restored and operation succeeds

**Parameters:**
- `path` (string) - S5 storage path
- `data` (any) - Data to store

**Throws:**
- `SDKError` with code `STORAGE_NOT_INITIALIZED` - StorageManager not initialized
- `SDKError` with code `STORAGE_RETRY_EXHAUSTED` - Failed after 5 retries
- `SDKError` with code `STORAGE_QUEUE_FULL` - Queue exceeded 100 operations

##### getWithRetry(path): Promise<any>

GET operation with automatic retry (no queue - reads need immediate response).

**Parameters:**
- `path` (string) - S5 storage path

**Returns:**
- `Promise<any>` - Retrieved data

**Throws:**
- `SDKError` with code `STORAGE_NOT_INITIALIZED` - StorageManager not initialized
- `SDKError` with code `STORAGE_RETRY_EXHAUSTED` - Failed after 5 retries

##### deleteWithRetry(path): Promise<boolean>

DELETE operation with automatic retry and queue support.

**Parameters:**
- `path` (string) - S5 storage path

**Returns:**
- `Promise<boolean>` - True if deleted

**Throws:**
- `SDKError` with code `STORAGE_NOT_INITIALIZED` - StorageManager not initialized
- `SDKError` with code `STORAGE_RETRY_EXHAUSTED` - Failed after 5 retries
- `SDKError` with code `STORAGE_QUEUE_FULL` - Queue exceeded 100 operations

#### Manual Sync

##### forceSync(): Promise<void>

Manually trigger reconnection and flush queued operations. Useful for UI "retry" buttons.

**Example:**
```typescript
// Retry button handler
retryButton.onclick = async () => {
  try {
    await storageManager.forceSync();
    console.log('Sync successful');
  } catch (error) {
    console.error('Sync failed:', error.message);
  }
};
```

#### Cleanup

##### cleanup(): void

Clean up connection handlers and reject pending operations. Call when disposing the SDK.

**Example:**
```typescript
// On app shutdown
storageManager.cleanup();
```

#### Auto-Reconnect Behavior

The SDK automatically attempts to reconnect in these scenarios:

1. **Tab becomes visible** - When user returns to the tab after it was backgrounded
2. **Network comes online** - When device regains network connectivity

When reconnection succeeds, queued operations are automatically flushed.

#### Complete UI Integration Example

```typescript
import { FabstirSDKCore, SyncStatus } from '@fabstir/sdk-core';

function ChatApp() {
  const [syncStatus, setSyncStatus] = useState<SyncStatus>('synced');
  const [pendingCount, setPendingCount] = useState(0);
  const storageManagerRef = useRef<StorageManager | null>(null);

  useEffect(() => {
    const init = async () => {
      const sdk = new FabstirSDKCore({ /* config */ });
      await sdk.authenticate('privatekey', { privateKey });
      const storageManager = await sdk.getStorageManager();
      storageManagerRef.current = storageManager;

      // Subscribe to sync status
      const unsubscribe = storageManager.onSyncStatusChange((status) => {
        setSyncStatus(status);
        setPendingCount(storageManager.getPendingOperationCount());
      });

      return () => {
        unsubscribe();
        storageManager.cleanup();
      };
    };

    init();
  }, []);

  const handleRetry = async () => {
    await storageManagerRef.current?.forceSync();
  };

  return (
    <div>
      {/* Sync status indicator */}
      <SyncIndicator status={syncStatus} pendingCount={pendingCount} />

      {syncStatus === 'error' && (
        <button onClick={handleRetry}>Retry Sync</button>
      )}

      {/* Chat UI */}
    </div>
  );
}

function SyncIndicator({ status, pendingCount }: { status: SyncStatus; pendingCount: number }) {
  switch (status) {
    case 'synced':
      return <span className="text-green-500">✓ Synced</span>;
    case 'syncing':
      return <span className="text-blue-500">⟳ Syncing...</span>;
    case 'pending':
      return <span className="text-yellow-500">⏳ {pendingCount} pending</span>;
    case 'error':
      return <span className="text-red-500">✗ Sync error</span>;
  }
}
```

#### Types

```typescript
// Connection status from S5.js
export type S5ConnectionStatus = 'connected' | 'connecting' | 'disconnected';

// Sync status for UI
export type SyncStatus = 'synced' | 'syncing' | 'pending' | 'error';
```

## RAG and Vector Databases

The SDK includes a complete **Retrieval-Augmented Generation (RAG)** system that enhances LLM responses by retrieving relevant context from user documents.

### Quick Start

```typescript
// 1. Get RAG managers
const vectorRAGManager = sdk.getVectorRAGManager();
const embeddingService = new HostAdapter({ hostUrl: 'http://host:8080' });
const documentManager = new DocumentManager({
  embeddingService,
  vectorManager: vectorRAGManager,
  databaseName: 'my-knowledge-base'
});

// 2. Create vector database
await vectorRAGManager.createSession('my-knowledge-base');

// 3. Upload document
const result = await documentManager.processDocument(file);
console.log(`Added ${result.chunks} chunks`);

// 4. Start RAG-enhanced session
sessionManager.setVectorRAGManager(vectorRAGManager);
sessionManager.setEmbeddingService(embeddingService);

const { sessionId } = await sessionManager.startSession({
  hostUrl: process.env.NEXT_PUBLIC_TEST_HOST_1_URL || 'http://localhost:8083',
  jobId: 123n,
  modelName: 'llama-3',
  chainId: ChainId.BASE_SEPOLIA,
  ragConfig: {
    enabled: true,
    databaseName: 'my-knowledge-base',
    topK: 5,
    threshold: 0.2  // Production-tested with all-MiniLM-L6-v2 embeddings
  }
});

// 5. Send prompts - RAG context automatically injected
await sessionManager.sendPrompt(sessionId, 'What does the document say about...?');
```

### VectorRAGManager

Manages vector databases and semantic search operations.

**Architecture**: VectorRAGManager uses a hybrid approach:
- **Storage**: S5VectorStore for decentralized persistence (S5 network)
- **Search**: In-memory native JavaScript vector database for fast semantic search
- **Deferred Embeddings**: Uploads documents without embeddings, generates them later during sessions

**Key Features**:
- ✅ Cross-device sync via S5 decentralized storage
- ✅ Chunked storage (10K vectors per chunk for efficiency)
- ✅ Virtual folder hierarchies for organization
- ✅ Pending/ready document tracking for deferred embeddings
- ✅ Browser and Node.js compatible

**Storage Layer**: See [S5VectorStore API](./S5_VECTOR_STORE_API.md) for detailed storage documentation.

#### Get VectorRAGManager

```typescript
const vectorRAGManager = sdk.getVectorRAGManager();
```

#### createSession

Creates a new vector database.

```typescript
async createSession(databaseName: string, config?: DatabaseConfig): Promise<string>
```

**Parameters:**
- `databaseName` - Unique identifier for the database
- `config` - Optional configuration (dimensions, metric, encryption)

**Returns:** Database session ID

**Example:**
```typescript
const sessionId = await vectorRAGManager.createSession('my-docs', {
  dimensions: 384,
  metric: 'cosine',
  encryption: true
});
```

#### addVector / addVectors

Adds vector(s) to database.

```typescript
async addVector(
  databaseName: string,
  id: string,
  vector: number[],
  metadata?: VectorMetadata
): Promise<void>

async addVectors(
  databaseName: string,
  vectors: VectorInput[]
): Promise<AddVectorResult>
```

**Example:**
```typescript
// Single vector
await vectorRAGManager.addVector('docs', 'doc-1', embedding, {
  content: 'Document text...',
  title: 'Getting Started',
  category: 'tutorial'
});

// Batch vectors
const result = await vectorRAGManager.addVectors('docs', [
  { id: 'doc-1', vector: emb1, metadata: { content: '...' } },
  { id: 'doc-2', vector: emb2, metadata: { content: '...' } }
]);
console.log(`Added ${result.added}, failed ${result.failed}`);
```

#### search

Searches for similar vectors.

```typescript
async search(
  databaseName: string,
  queryVector: number[],
  topK: number,
  options?: SearchOptions
): Promise<SearchResult[]>
```

**Parameters:**
- `queryVector` - Query embedding (384 dimensions)
- `topK` - Number of results to return
- `options.threshold` - Minimum similarity score (0-1)
- `options.filter` - Metadata filter (MongoDB-style)
- `options.includeVectors` - Include vector arrays in results

**Returns:**
```typescript
interface SearchResult {
  id: string;
  score: number;
  metadata: VectorMetadata;
  vector?: number[];
}
```

**Example:**
```typescript
const queryEmbedding = await embeddingService.embed('search query');
const results = await vectorRAGManager.search('docs', queryEmbedding, 5, {
  threshold: 0.2,  // Production-tested: 0.2 works best with all-MiniLM-L6-v2
  filter: {
    $and: [
      { category: 'tutorial' },
      { $gte: { createdAt: Date.now() - 86400000 * 30 } }
    ]
  }
});

results.forEach(r => {
  console.log(`${r.metadata.title} (score: ${r.score})`);
});
```

#### searchMultipleDatabases

Searches across multiple databases simultaneously.

```typescript
async searchMultipleDatabases(
  databaseNames: string[],
  queryVector: number[],
  options: SearchOptions
): Promise<SearchResult[]>
```

**Example:**
```typescript
const results = await vectorRAGManager.searchMultipleDatabases(
  ['user-docs', 'shared-docs', 'public-wiki'],
  queryEmbedding,
  { topK: 10, threshold: 0.2 }  // Production-tested threshold
);
```

#### deleteVector / deleteByMetadata

Deletes vectors from database.

```typescript
async deleteVector(databaseName: string, id: string): Promise<void>

async deleteByMetadata(
  databaseName: string,
  filter: MetadataFilter
): Promise<DeleteResult>
```

**Example:**
```typescript
// Delete single vector
await vectorRAGManager.deleteVector('docs', 'doc-1');

// Delete by metadata
const result = await vectorRAGManager.deleteByMetadata('docs', {
  $lt: { createdAt: Date.now() - 86400000 * 365 } // Older than 1 year
});
console.log(`Deleted ${result.deletedCount} old documents`);
```

#### Folder Operations

Organize vectors in virtual folder hierarchies.

```typescript
// Move to folder
await vectorRAGManager.moveToFolder('docs', 'doc-1', '/tutorials/python');

// Search in folder
const results = await vectorRAGManager.searchInFolder(
  'docs',
  '/tutorials',
  queryVector,
  5,
  { recursive: true }
);

// List folders
const folders = vectorRAGManager.listFolders('docs');
console.log('Folders:', folders);

// Folder statistics
const stats = vectorRAGManager.getFolderStatistics('docs', '/tutorials');
console.log('Vectors in folder:', stats.vectorCount);
```

#### Database Management

```typescript
// Get metadata
const metadata = vectorRAGManager.getDatabaseMetadata('docs');
console.log('Created:', metadata.createdAt);

// Get statistics
const stats = vectorRAGManager.getDatabaseStats('docs');
console.log('Total vectors:', stats.vectorCount);

// List all databases
const databases = vectorRAGManager.listDatabases();
databases.forEach(db => console.log(db.databaseName));

// Delete database
await vectorRAGManager.deleteDatabase('old-docs');

// Vacuum (reclaim storage)
const vacuumStats = await vectorRAGManager.vacuum('docs');
console.log(`Freed ${vacuumStats.spaceFreed} bytes`);
```

#### Deferred Embeddings

Methods for managing documents in the **deferred embeddings workflow**, where documents are uploaded without embeddings and processed later during an active session.

##### getPendingDocuments

Gets all documents awaiting embedding generation across all vector databases.

```typescript
async getPendingDocuments(sessionGroupId?: string): Promise<PendingDocument[]>
```

**Parameters:**
- `sessionGroupId` - Optional: Filter by session group (currently not implemented)

**Returns:** Array of pending documents with database context

```typescript
interface PendingDocument {
  id: string;
  fileName: string;
  fileSize: number;
  fileType: string;
  uploadedAt: number;
  status: 'pending' | 'processing' | 'failed';
  embeddingProgress?: number;  // 0-100 percentage
  embeddingError?: string;
  vectorCount?: number;
  databaseName: string;  // Which database this document belongs to
}
```

**Example:**
```typescript
// Get all pending documents
const pending = await vectorRAGManager.getPendingDocuments();
console.log(`Found ${pending.length} documents awaiting embeddings`);

pending.forEach(doc => {
  console.log(`${doc.fileName} (${doc.databaseName}): ${doc.status}`);
  if (doc.embeddingProgress !== undefined) {
    console.log(`  Progress: ${doc.embeddingProgress}%`);
  }
  if (doc.embeddingError) {
    console.log(`  Error: ${doc.embeddingError}`);
  }
});
```

**When to Use:**
- Display pending documents UI
- Start background embedding generation during session
- Show progress indicators for embedding processing
- Identify failed documents needing retry

**See Also:**
- `SessionManager.generateEmbeddings()` - Generate embeddings for pending documents
- `VectorRAGManager.updateDocumentStatus()` - Update document status after processing

##### updateDocumentStatus

Updates the embedding status of a document. Automatically moves document from `pendingDocuments[]` to `readyDocuments[]` when status is set to 'ready'.

```typescript
async updateDocumentStatus(
  documentId: string,
  status: 'pending' | 'processing' | 'ready' | 'failed',
  updates?: {
    vectorCount?: number;
    embeddingProgress?: number;
    embeddingError?: string;
  }
): Promise<void>
```

**Parameters:**
- `documentId` - Unique document identifier
- `status` - New embedding status
- `updates.vectorCount` - Number of vectors generated (for 'ready' status)
- `updates.embeddingProgress` - Progress percentage (0-100, for 'processing' status)
- `updates.embeddingError` - Error message (for 'failed' status)

**Example:**
```typescript
// Update to processing status
await vectorRAGManager.updateDocumentStatus(
  'doc-123',
  'processing',
  { embeddingProgress: 45 }
);

// Update to ready status (moves to readyDocuments[])
await vectorRAGManager.updateDocumentStatus(
  'doc-123',
  'ready',
  { vectorCount: 150 }
);

// Update to failed status
await vectorRAGManager.updateDocumentStatus(
  'doc-456',
  'failed',
  { embeddingError: 'Host disconnected during embedding generation' }
);

// Retry failed document (reset to pending)
await vectorRAGManager.updateDocumentStatus(
  'doc-456',
  'pending'
);
```

**Workflow:**
```typescript
// 1. Upload document (UI adds to pendingDocuments[])
const fileContent = await file.text();
const docId = `doc-${Date.now()}`;

// 2. Start session with host
const { sessionId } = await sessionManager.startSession({...});

// 3. Get pending documents
const pending = await vectorRAGManager.getPendingDocuments();

// 4. Process each pending document
for (const doc of pending) {
  try {
    // Update to processing
    await vectorRAGManager.updateDocumentStatus(
      doc.id,
      'processing',
      { embeddingProgress: 0 }
    );

    // Generate embeddings
    const vectors = await sessionManager.generateEmbeddings(
      sessionId,
      doc.fileContent
    );

    // Update progress
    await vectorRAGManager.updateDocumentStatus(
      doc.id,
      'processing',
      { embeddingProgress: 50 }
    );

    // Add to vector database
    await vectorRAGManager.addVectors(doc.databaseName, vectors);

    // Update to ready (moves to readyDocuments[])
    await vectorRAGManager.updateDocumentStatus(
      doc.id,
      'ready',
      { vectorCount: vectors.length }
    );
  } catch (error) {
    // Update to failed
    await vectorRAGManager.updateDocumentStatus(
      doc.id,
      'failed',
      { embeddingError: error.message }
    );
  }
}
```

**Status Transitions:**
- `pending` → `processing` → `ready` (success path)
- `pending` → `processing` → `failed` (error path)
- `failed` → `pending` (retry)
- `ready` documents are moved to `readyDocuments[]` array

**See Also:**
- `SessionManager.generateEmbeddings()` - Generate embeddings for document
- `VectorRAGManager.getPendingDocuments()` - Get all pending documents

### DocumentManager

Handles document processing, text extraction, chunking, and embedding generation for RAG.

**Supported File Types:**

| Format | Extension | Processing Method |
|--------|-----------|-------------------|
| Plain Text | `.txt` | Direct text extraction |
| Markdown | `.md` | Direct text extraction |
| HTML | `.html` | Tag stripping + text |
| PDF | `.pdf` | Client-side pdfjs extraction |
| PNG | `.png` | Host OCR + Description |
| JPEG | `.jpg`, `.jpeg` | Host OCR + Description |
| WebP | `.webp` | Host OCR + Description |
| GIF | `.gif` | Host OCR + Description |

#### Constructor

```typescript
import { DocumentManager } from '@fabstir/sdk-core';

const documentManager = new DocumentManager({
  embeddingService: hostAdapter  // Required for processDocument
});
```

**Options:**
- `embeddingService` - EmbeddingService instance (HostAdapter, OpenAIAdapter, or CohereAdapter)

#### processDocument

Processes a document or image: extracts text, chunks it, and generates embeddings.

**For images**, automatically uses HostAdapter's `processImage()` to call host OCR and description endpoints.

```typescript
async processDocument(
  file: File,
  options?: ChunkingOptions & UploadOptions
): Promise<ChunkResult[]>
```

**Options:**
- `chunkSize` - Tokens per chunk (default: 500)
- `overlap` - Token overlap between chunks (default: 50)
- `splitBySentence` - Split on sentence boundaries
- `splitByParagraph` - Split on paragraph boundaries
- `onProgress` - Progress callback

**Returns:**
```typescript
interface ChunkResult {
  chunk: DocumentChunk;
  embedding: number[];  // 384-dimensional vector
}

interface DocumentChunk {
  id: string;
  text: string;
  metadata: {
    documentId: string;
    documentName: string;
    documentType: DocumentType;
    index: number;
    startOffset: number;
    endOffset: number;
    tokenCount?: number;
  };
}
```

**Example (Text Document):**
```typescript
const chunks = await documentManager.processDocument(pdfFile, {
  chunkSize: 500,
  overlap: 50,
  onProgress: (progress) => {
    // progress.currentStep: "Extracted 1,234 words"
    console.log(`${progress.stage}: ${progress.progress}%`);
  }
});

console.log(`Processed ${chunks.length} chunks`);
```

**Example (Image - automatic OCR):**
```typescript
// Images work the same way - SDK handles OCR automatically
const chunks = await documentManager.processDocument(screenshotPng, {
  onProgress: (progress) => {
    // progress.currentStep: "Image processed: OCR confidence 95%"
    console.log(`${progress.stage}: ${progress.progress}%`);
  }
});

// chunks[0].chunk.text contains extracted OCR text
```

#### embedText

Embeds a single text string for RAG query embedding.

```typescript
async embedText(
  text: string,
  type?: 'document' | 'query'
): Promise<{ embedding: number[]; text: string; tokenCount: number }>
```

**Example:**
```typescript
// Embed user's question for vector search
const queryEmbedding = await documentManager.embedText(
  "What is the main topic?",
  'query'
);

// Use queryEmbedding.embedding for vector similarity search
const results = await sessionManager.searchVectors(
  sessionId,
  queryEmbedding.embedding,
  5,     // topK
  0.3    // threshold
);
```

#### uploadDocument

Uploads a document to S5 storage (requires initialization).

```typescript
async uploadDocument(
  file: File,
  databaseName: string,
  options?: UploadOptions
): Promise<UploadResult>
```

#### uploadBatch

Uploads multiple documents in parallel.

```typescript
async uploadBatch(
  files: File[],
  databaseName: string,
  options?: BatchOptions
): Promise<BatchResult[]>
```

**Example:**
```typescript
const results = await documentManager.uploadBatch(files, 'my-docs', {
  concurrency: 3,
  continueOnError: true,
  onProgress: ({ completed, total, currentFile }) => {
    console.log(`${completed}/${total}: ${currentFile}`);
  }
});

const successful = results.filter(r => r.success).length;
console.log(`Uploaded ${successful}/${results.length} files`);
```

#### deleteDocument

Deletes a document from registry and S5 storage.

```typescript
async deleteDocument(databaseName: string, documentId: string): Promise<void>
```

#### listDocuments

Lists all documents in a database.

```typescript
async listDocuments(databaseName: string): Promise<DocumentMetadata[]>
```

### Embedding Services

Three embedding adapters for different deployment scenarios.

#### HostAdapter (Recommended for Production)

Zero-cost, host-side embeddings using local models. Also supports **image processing** via OCR and description endpoints.

```typescript
import { HostAdapter } from '@fabstir/sdk-core/embeddings';

const hostAdapter = new HostAdapter({
  hostUrl: 'https://your-host:8080',
  chainId: 84532,  // Optional, defaults to Base Sepolia
  model: 'all-MiniLM-L6-v2',
  dimensions: 384
});
```

**Benefits:**
- Zero API costs
- 9x faster than target (11ms vs 100ms)
- No third-party data exposure
- GDPR compliant
- **Image OCR support** (PNG, JPEG, WebP, GIF)

##### processImage

Processes an image using host's `/v1/ocr` and `/v1/describe-image` endpoints in parallel.

```typescript
async processImage(file: File): Promise<ImageProcessingResult>
```

**Returns:**
```typescript
interface ImageProcessingResult {
  description: string;        // From /v1/describe-image (Florence-2)
  extractedText: string;      // From /v1/ocr (PaddleOCR)
  ocrConfidence: number;      // 0.0-1.0
  combinedText: string;       // "[Image Description]\n...\n\n[Extracted Text]\n..."
  processingTimeMs: number;
}
```

**Example:**
```typescript
const result = await hostAdapter.processImage(screenshotFile);

console.log(`OCR confidence: ${(result.ocrConfidence * 100).toFixed(0)}%`);
console.log(`Extracted text: ${result.extractedText}`);
console.log(`Description: ${result.description}`);

// Combined text is ready for chunking/embedding
console.log(`Combined: ${result.combinedText}`);
```

**Note:** `processImage()` is called automatically by `DocumentManager.processDocument()` when the file is an image type. You typically don't need to call it directly.

#### OpenAIAdapter

External API using OpenAI embeddings.

```typescript
import { OpenAIAdapter } from '@fabstir/sdk-core/embeddings';

const embeddingService = new OpenAIAdapter({
  apiKey: process.env.OPENAI_API_KEY,
  model: 'text-embedding-3-small',
  dimensions: 384,
  maxRetries: 3
});
```

**Cost:** ~$0.02 per 1M tokens

**Note:** OpenAIAdapter does not support image processing. Use HostAdapter for image RAG.

#### CohereAdapter

External API using Cohere embeddings.

```typescript
import { CohereAdapter } from '@fabstir/sdk-core/embeddings';

const embeddingService = new CohereAdapter({
  apiKey: process.env.COHERE_API_KEY,
  model: 'embed-english-v3.0',
  dimensions: 384
});
```

**Cost:** ~$0.10 per 1M tokens

**Note:** CohereAdapter does not support image processing. Use HostAdapter for image RAG.

### Permissions and Sharing

Control access to vector databases with fine-grained permissions.

#### PermissionManager

```typescript
import { PermissionManager } from '@fabstir/sdk-core/permissions';

const permissionManager = new PermissionManager(ownerAddress, {
  auditLogger: true
});

// Grant permissions
permissionManager.grant('docs', readerAddress, 'reader');
permissionManager.grant('docs', writerAddress, 'writer');

// Check permissions
const canWrite = permissionManager.check('docs', 'write', userAddress);

// Revoke permissions
permissionManager.revoke('docs', userAddress);

// List permissions
const permissions = permissionManager.listPermissions('docs');

// Get audit logs
const logs = permissionManager.getAuditLogs('docs');
```

**Permission Levels:**
- **reader** - Read-only access (search)
- **writer** - Read + write access (add/update/delete vectors)
- **owner** - Full control (manage permissions, delete database)

#### SharingManager

```typescript
import { SharingManager } from '@fabstir/sdk-core/sharing';

const sharingManager = new SharingManager({
  permissionManager: sdk.getPermissionManager()
});

// Create invitation
const invitation = await sharingManager.createInvitation({
  databaseName: 'docs',
  recipientAddress: '0x123...',
  role: 'reader',
  expiresIn: 86400000 // 24 hours
});

console.log('Invitation code:', invitation.invitationCode);

// Generate access token
const token = await sharingManager.generateAccessToken({
  databaseName: 'docs',
  role: 'reader',
  expiresIn: 3600000,   // 1 hour
  usageLimit: 100       // Max 100 uses
});

// Revoke invitation
await sharingManager.revokeInvitation(invitationId);
```

### RAG Configuration in Sessions

Enable RAG when starting LLM sessions.

```typescript
const { sessionId } = await sessionManager.startSession({
  hostUrl: process.env.NEXT_PUBLIC_TEST_HOST_1_URL || 'http://localhost:8083',
  jobId: 123n,
  modelName: 'llama-3',
  chainId: ChainId.BASE_SEPOLIA,
  ragConfig: {
    enabled: true,
    databaseName: 'my-knowledge-base',
    topK: 5,                    // Retrieve top 5 most similar chunks
    threshold: 0.2,             // Production-tested: 0.2 works with all-MiniLM-L6-v2
    conversationMemory: {
      enabled: true,
      maxMessages: 10,          // Store last 10 messages
      includeRecent: 5,         // Always include 5 most recent
      similarityThreshold: 0.8  // For historical retrieval
    }
  }
});
```

### Monitoring RAG Performance

```typescript
const session = sessionManager.getSession(sessionId.toString());
console.log('RAG Metrics:', {
  contextsRetrieved: session.ragMetrics.contextsRetrieved,
  avgSimilarity: session.ragMetrics.avgSimilarityScore,
  retrievalTime: session.ragMetrics.retrievalTimeMs,
  tokensAdded: session.ragMetrics.tokensAdded
});
```

### Comprehensive Documentation

For complete RAG documentation, see:

- **[RAG API Reference](./RAG_API_REFERENCE.md)** - Complete API documentation
- **[RAG Implementation Plan](./IMPLEMENTATION_CHAT_RAG.md)** - Implementation progress and architecture
- **[RAG Manual Testing](./RAG_MANUAL_TESTING_GUIDE.md)** - Testing host embedding endpoints
- **[RAG Node Integration](./node-reference/RAG_SDK_INTEGRATION.md)** - Host-side integration guide

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
  modelId: string;           // Required bytes32 model ID (Phase 18: modelless sessions removed)
  sessionConfig: {
    pricePerToken: bigint;
    duration: bigint;
    proofInterval: bigint;
    proofTimeoutWindow?: number; // Proof timeout in seconds (60-3600, default: 300)
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

**February 2026 Update:** Signature parameter removed. Authentication is now via `msg.sender == session.host` check on-chain.

```typescript
async submitCheckpointProof(
  sessionId: bigint,
  tokensClaimed: number,
  proofHash: string,
  proofCID: string,
  deltaCID: string = ''
): Promise<string>
```

**Parameters:**
- `sessionId` - The session/job ID
- `tokensClaimed` - Number of tokens being claimed
- `proofHash` - bytes32 keccak256 hash of the proof data
- `proofCID` - S5 CID pointing to the full proof data
- `deltaCID` - Optional S5 CID for delta/incremental proof data

#### completeSessionJob

Completes a session job.

```typescript
async completeSessionJob(
  sessionId: bigint,
  conversationCID: string
): Promise<string>
```

### Delegate-Pays Authorization (SDK-level, sdk-core 1.21.0+)

High-level API for the **delegate-pays** model: a generated hot EOA (the "delegate") spends a payer's **on-chain-capped USDC allowance** — no host pinning, no raw private-key custody. The user keeps control via a hard on-chain cap (`usdc.approve(jobMarketplace, cap)`); a looping agent physically cannot overspend it. These methods wrap the contract-level calls in [V2 Direct Payment Delegation](#v2-direct-payment-delegation-february-2026) below.

> **Delegated sessions are USDC-only** (V2 delegation rejects native token) and the **delegate never settles** — the host settles the job on WebSocket disconnect. Only the payer or host may complete a delegated session.
>
> **The `payer` MUST be an account distinct from the chat primary** (use the existing Coinbase sub-account). ERC-20 allowance is keyed `(owner=payer, spender=marketplace)` and shared across all of that payer's delegates, so reusing the chat primary would let a coding agent drain chat's allowance, and `revokeDelegate`'s `approve(0)` would break chat.

#### authenticateAsDelegate

Authenticates with a plain EOA delegate `signer` while recording the `payer` whose USDC funds sessions. Reuses the `'signer'` auth path (managers initialise as normal), then sets `authMode = 'delegate'` and propagates the payer to the PaymentManager. After this, `startSession` routes through the delegate path transparently — `SessionManager` is unchanged.

```typescript
await sdk.authenticateAsDelegate({ signer: ethers.Signer, payer: string }): Promise<void>
sdk.getDelegatePayer(): string | undefined   // the payer recorded in delegate mode
```

Throws `SDKError` `DELEGATE_SIGNER_MISSING` (no signer) or `DELEGATE_PAYER_MISSING` (missing/zero payer) before authenticating. `disconnect()` clears the delegate state.

#### createDelegateAuthorization

Authorizes a delegate to spend the payer's USDC up to `allowanceCap` (in **base units** — bigint, USDC has 6 decimals). Performs `approveToken(marketplace, cap)` **then** `authorizeDelegate(delegate, true)`. Signed by the bridge-payer (sub-account).

```typescript
await sdk.getPaymentManager().createDelegateAuthorization({
  delegate: string,
  allowanceCap: bigint,        // e.g. 50_000_000n = 50 USDC
  token?: string,              // defaults to the chain's USDC
  chainId?: number,
}): Promise<{ approveTxHash: string; authorizeTxHash: string }>
```

Throws `DELEGATE_USDC_REQUIRED` for a zero/native token (no `ZeroAddress` fallback).

#### getDelegateAuthorization

Reads whether `delegate` is authorized for `payer` and the live remaining allowance (the authoritative on-chain cap).

```typescript
await sdk.getPaymentManager().getDelegateAuthorization({
  payer: string,
  delegate: string,
  token?: string,
  chainId?: number,
}): Promise<{ authorized: boolean; remaining: bigint }>   // remaining = USDC base units
```

#### revokeDelegate

Revokes a delegate by bundling **both** on-chain actions so the UI cannot half-revoke: `authorizeDelegate(delegate, false)` **then** `approveToken(marketplace, 0n)`. Because it operates on the bridge-payer sub-account, zeroing its allowance never touches chat's `allowance[primary][marketplace]`.

```typescript
await sdk.getPaymentManager().revokeDelegate({
  delegate: string,
  token?: string,
  chainId?: number,
}): Promise<{ revokeTxHash: string; approveTxHash: string }>
```

If the `approve(0)` leg fails after deauthorization, throws `SDKError` `DELEGATE_REVOKE_INCOMPLETE` carrying the `revokeTxHash` in `details`.

#### setDelegatePayer / getDelegatePayer (PaymentManager)

```typescript
sdk.getPaymentManager().setDelegatePayer(payer: string): void   // enter delegate mode
sdk.getPaymentManager().getDelegatePayer(): string | undefined
```

#### Delegate session pre-flight & error codes

When `delegatePayer` is set, `createSessionJob` (used by `startSession`) takes a delegate branch that pre-checks **before** any chain write and maps contract reverts to typed `SDKError` codes:

| Code | Cause |
|------|-------|
| `DELEGATE_USDC_REQUIRED` | zero/native `paymentToken`, or contract `ERC20Only` revert |
| `DELEGATE_NOT_AUTHORIZED` | delegate not authorized for payer, or contract `NotDelegate` revert |
| `DELEGATE_ALLOWANCE_INSUFFICIENT` | remaining allowance < session amount (`details` has `remaining`/`needed`) |
| `DELEGATE_BALANCE_INSUFFICIENT` | payer USDC balance < session amount |
| `DELEGATE_BAD_PARAMS` | contract `BadDelegateParams` revert |
| `DELEGATE_SIGNER_MISSING` / `DELEGATE_PAYER_MISSING` | `authenticateAsDelegate` validation |
| `DELEGATE_REVOKE_INCOMPLETE` | `revokeDelegate`'s `approve(0)` leg failed |

#### Minimal flow

```typescript
// UI (signed by the bridge sub-account = payer):
await sdk.getPaymentManager().createDelegateAuthorization({ delegate: delegateAddress, allowanceCap: 50_000_000n });

// Daemon (delegate hot EOA):
await sdk.authenticateAsDelegate({ signer: delegateWallet, payer: subAccountAddress });
const { sessionId } = await (await sdk.getSessionManager()).startSession({ /* USDC paymentToken */ });
// … run prompts … then end the session; the HOST settles on disconnect.

// Revoke when done:
await sdk.getPaymentManager().revokeDelegate({ delegate: delegateAddress });
```

### V2 Direct Payment Delegation (February 2026)

These contract-level methods (on the JobMarketplace wrapper) back the [Delegate-Pays Authorization](#delegate-pays-authorization-sdk-level-sdk-core-1210) API above. Use the SDK-level API for app code; reach for these only for low-level/CLI use.

#### authorizeDelegate

Authorizes or revokes a delegate address for session creation on behalf of payer.

```typescript
async authorizeDelegate(delegate: string, authorized: boolean): Promise<string>
```

#### isDelegateAuthorized

Checks if a delegate is authorized for a payer.

```typescript
async isDelegateAuthorized(payer: string, delegate: string): Promise<boolean>
```

#### createSessionForModelAsDelegate

Creates a session as delegate for a payer (model-specific).

```typescript
async createSessionForModelAsDelegate(
  payer: string,
  modelId: string,
  host: string,
  paymentToken: string,
  amount: bigint,
  pricePerToken: bigint,
  maxDuration: number,
  proofInterval: number,
  proofTimeoutWindow: number
): Promise<SessionResult>
```

#### getMinTokensFee

Gets the minimum token fee for early cancellation.

```typescript
async getMinTokensFee(): Promise<bigint>
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

### HostSelectionService

Provides intelligent host selection with weighted scoring algorithm.

```typescript
import { HostSelectionService, HostSelectionMode } from '@fabstir/sdk-core';

const hostSelectionService = new HostSelectionService();
hostSelectionService.setHostManager(sdk.getHostManager());
```

#### selectHostForModel

Select the best host for a model based on user preferences.

```typescript
async selectHostForModel(
  modelId: string,
  mode: HostSelectionMode,
  preferredHostAddress?: string
): Promise<HostInfo | null>
```

**Parameters:**
- `modelId` - Model ID to find hosts for
- `mode` - Selection mode (AUTO, CHEAPEST, RELIABLE, FASTEST, SPECIFIC)
- `preferredHostAddress` - Required for SPECIFIC mode

**Returns:**
- `HostInfo` if a host is found
- `null` if no hosts available (for non-SPECIFIC modes)

**Throws:**
- `Error` for SPECIFIC mode when host unavailable or doesn't support model

**Example:**
```typescript
// Auto mode - weighted random selection
const host = await hostSelectionService.selectHostForModel(
  modelId,
  HostSelectionMode.AUTO
);

// Cheapest mode - prioritize low price
const cheapHost = await hostSelectionService.selectHostForModel(
  modelId,
  HostSelectionMode.CHEAPEST
);

// Specific mode - use preferred host (throws if unavailable)
const specificHost = await hostSelectionService.selectHostForModel(
  modelId,
  HostSelectionMode.SPECIFIC,
  '0x1234...'
);
```

#### getRankedHostsForModel

Get all hosts for a model with scores and ranking.

```typescript
async getRankedHostsForModel(
  modelId: string,
  mode: HostSelectionMode,
  limit?: number
): Promise<RankedHost[]>
```

**Parameters:**
- `modelId` - Model ID to find hosts for
- `mode` - Selection mode for scoring weights
- `limit` - Maximum hosts to return (default: 10)

**Returns:**
```typescript
interface RankedHost {
  host: HostInfo;
  score: number;          // 0-1 composite score
  factors: {
    stakeScore: number;   // 0-1 (higher stake = higher score)
    priceScore: number;   // 0-1 (lower price = higher score)
    uptimeScore: number;  // 0-1 (placeholder: 0.95)
    latencyScore: number; // 0-1 (placeholder: 0.9)
  };
}
```

**Example:**
```typescript
// Get top 5 hosts ranked for cheapest mode
const rankedHosts = await hostSelectionService.getRankedHostsForModel(
  modelId,
  HostSelectionMode.CHEAPEST,
  5
);

// Display options to user
rankedHosts.forEach((rh, i) => {
  console.log(`${i + 1}. ${rh.host.address}`);
  console.log(`   Score: ${(rh.score * 100).toFixed(1)}%`);
  console.log(`   Price: ${rh.host.minPricePerTokenStable}`);
  console.log(`   Stake: ${rh.host.stake}`);
});
```

#### calculateHostScore

Calculate a host's score for a given mode.

```typescript
calculateHostScore(host: HostInfo, mode: HostSelectionMode): number
```

**Weighted Scoring Algorithm:**

| Mode | Stake | Price | Uptime | Latency |
|------|-------|-------|--------|---------|
| AUTO | 35% | 30% | 20% | 15% |
| CHEAPEST | 15% | 70% | 10% | 5% |
| RELIABLE | 50% | 5% | 40% | 5% |
| FASTEST | 10% | 20% | 10% | 60% |

**Note:** Uptime (95%) and latency (0.9) are placeholder values until metrics system is implemented.

**Example:**
```typescript
const host = await hostManager.getHostInfo('0x1234...');
const autoScore = hostSelectionService.calculateHostScore(host, HostSelectionMode.AUTO);
const cheapScore = hostSelectionService.calculateHostScore(host, HostSelectionMode.CHEAPEST);

console.log(`Auto score: ${(autoScore * 100).toFixed(1)}%`);
console.log(`Cheap score: ${(cheapScore * 100).toFixed(1)}%`);
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
  DEPOSIT_ACCOUNT_UNAVAILABLE = 'DEPOSIT_ACCOUNT_UNAVAILABLE',

  // Transcoding
  CAPACITY_FULL = 'CAPACITY_FULL',             // Host transcode queue full (HTTP 429, retryable)
  TRANSCODE_FAILED = 'TRANSCODE_FAILED',       // Transcode operation failed
  TRANSCODE_TIMEOUT = 'TRANSCODE_TIMEOUT',     // Transcode timed out (retryable)
  SIDECAR_DISCONNECTED = 'SIDECAR_DISCONNECTED', // Transcoder sidecar not connected
  NO_AVAILABLE_HOSTS = 'NO_AVAILABLE_HOSTS'    // All hosts exhausted during load balancing
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

class ContextLimitError extends Error {
  readonly code: 'TOKEN_LIMIT_EXCEEDED';
  readonly promptTokens: number;
  readonly contextWindowSize: number;
  readonly excess: number;  // promptTokens - contextWindowSize
  constructor(message: string, promptTokens: number, contextWindowSize: number);
}
```

### Context Limit Error Handling

When the prompt exceeds the model's context window, the node returns `TOKEN_LIMIT_EXCEEDED` and the SDK throws `ContextLimitError`. The `excess` field tells the UI how many tokens to trim.

```typescript
import { ContextLimitError } from '@fabstir/sdk-core';

try {
  await sessionManager.sendPromptStreaming(sessionId, prompt, onToken, opts);
} catch (err) {
  if (err instanceof ContextLimitError) {
    console.log(`Over limit by ${err.excess} tokens`);
    const trimmed = trimOldestMessages(messages, err.excess);
    await sessionManager.sendPromptStreaming(sessionId, buildPrompt(trimmed), onToken, opts);
  } else throw err;
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
  proofTimeoutWindow?: number; // Proof timeout window (60-3600 seconds, default: 300)
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

// Document Types
type DocumentType = 'txt' | 'md' | 'html' | 'pdf' | 'docx' | 'png' | 'jpeg' | 'webp' | 'gif';

interface DocumentMetadata {
  id: string;
  name: string;
  type: DocumentType;
  size: number;
  uploadedAt: number;
  s5Path: string;
}

interface DocumentChunk {
  id: string;
  text: string;
  metadata: ChunkMetadata;
}

interface ChunkMetadata {
  documentId: string;
  documentName: string;
  documentType: DocumentType;
  index: number;
  startOffset: number;
  endOffset: number;
  tokenCount?: number;
}

interface ChunkResult {
  chunk: DocumentChunk;
  embedding: number[];  // 384-dimensional vector
}

// Image Processing Types (HostAdapter)
interface ImageProcessingResult {
  description: string;        // From /v1/describe-image (Florence-2)
  extractedText: string;      // From /v1/ocr (PaddleOCR)
  ocrConfidence: number;      // 0.0-1.0
  combinedText: string;       // Combined text for embedding
  processingTimeMs: number;
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
export const DEFAULT_S5_PORTAL = 'wss://z2DcjTLqfj6PTMsDbFfgtuHtYmrKeibFTkvqY8QZeyR3YmE@s5.platformlessai.ai/s5/p2p';
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
// Contract addresses from ChainRegistry (populated from .env.test)
const chain = ChainRegistry.getChain(ChainId.BASE_SEPOLIA);
const sdk = new FabstirSDKCore({
  rpcUrl: process.env.RPC_URL_BASE_SEPOLIA!,
  chainId: ChainId.BASE_SEPOLIA, // 84532
  contractAddresses: {
    jobMarketplace: chain.contracts.jobMarketplace,
    nodeRegistry: chain.contracts.nodeRegistry,
    proofSystem: chain.contracts.proofSystem,
    hostEarnings: chain.contracts.hostEarnings,
    usdcToken: chain.contracts.usdcToken,
    fabToken: chain.contracts.fabToken,
    modelRegistry: chain.contracts.modelRegistry
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

## Multi-Agent Orchestration (`@fabstir/orchestrator`)

The orchestrator package coordinates multiple SDK sessions for complex, multi-step tasks using LLM-driven planning.

### Installation

```bash
pnpm add @fabstir/orchestrator
```

### OrchestratorManager

The main entry point for multi-agent orchestration.

```typescript
import { OrchestratorManager } from '@fabstir/orchestrator';

const orchestrator = new OrchestratorManager({
  sdk,                          // FabstirSDKCore instance
  chainId: 84532,               // Base Sepolia
  privateKey: '0x...',          // Or use signer for Account Kit
  models: {
    fast: 'fast-model',         // For simple analysis tasks
    deep: 'deep-model',        // For reasoning/synthesis
    planning: 'deep-model',    // For task decomposition
  },
  maxConcurrentSessions: 3,
  budget: {
    maxDepositPerSubTask: '0.003',
    maxTotalDeposit: '0.01',
    maxSubTasks: 5,
  },
});

const result = await orchestrator.run('Analyze renewable energy pros and cons');
// result.synthesis — Final combined answer
// result.subTaskResults — Individual task outputs
// result.totalTokensUsed — Token usage across all tasks
await orchestrator.destroy();
```

### OrchestratorConfig

```typescript
interface OrchestratorConfig {
  sdk: FabstirSDKCore;
  chainId: number;
  privateKey?: string;           // EOA authentication
  signer?: SignerLike;           // Account Kit authentication
  models: {
    fast?: string;               // Quick tasks
    deep?: string;               // Complex reasoning
    planning?: string;           // Task decomposition
  };
  maxConcurrentSessions: number; // Pool size (>= 2)
  budget: BudgetConfig;
  proofGracePeriodMs?: number;   // Grace period for proof submission
  x402?: X402Config;             // Optional x402 payment config
}

interface BudgetConfig {
  maxDepositPerSubTask: string;  // USDC per task
  maxTotalDeposit: string;       // Total USDC budget
  maxSubTasks: number;           // Max parallel tasks
}
```

### Orchestration Patterns

Three reusable execution patterns:

```typescript
import { fanOut, pipeline, mapReduce } from '@fabstir/orchestrator';

// Fan-out: N independent tasks in parallel
const results = await fanOut(pool, tasks, options, signal);

// Pipeline: Sequential, each receives prior result
const result = await pipeline(pool, tasks, options, signal);

// Map-reduce: Parallel mappers + single reducer
const result = await mapReduce(pool, mapTasks, reduceTask, options, signal);
```

### Session Multiplexing

SessionPool automatically caches sessions per model. When a subtask finishes, its session is cached instead of destroyed. The next subtask using the same model reuses it — no new deposit required.

| Scenario | Sessions Before | After | Savings |
|----------|----------------|-------|---------|
| 1 plan + 3 tasks (2 fast, 1 deep) | 4 | 3 | 25% |
| 1 plan + 4 tasks (2 fast, 2 deep) | 5 | 3 | 40% |
| 1 plan + 5 tasks (all fast) | 6 | 2 | 67% |

### A2A Protocol (Agent-to-Agent)

#### Running as A2A Server

```typescript
import { OrchestratorA2AServer } from '@fabstir/orchestrator';

const server = new OrchestratorA2AServer(orchestrator, { port: 3100 });
await server.start();
// GET  /.well-known/agent.json  — Agent discovery
// POST /v1/orchestrate           — Execute orchestration
// DELETE /v1/orchestrate/:taskId — Cancel task
```

#### Discovering External Agents

```typescript
import { AgentDiscovery, A2AClientPool } from '@fabstir/orchestrator';

const discovery = new AgentDiscovery();
await discovery.register('https://other-agent.example.com');
const agents = discovery.findBySkill('code-review');
```

### x402 Payment Protocol

HTTP-native USDC micropayments between agents using EIP-3009 `transferWithAuthorization`.

#### Server (receiving payments)

```typescript
import { x402PaymentGate, X402PaymentValidator } from '@fabstir/orchestrator';

// Express middleware — returns 402 if payment missing
app.use('/v1/orchestrate', x402PaymentGate({
  pricing: { amount: '0.01', currency: 'USDC', network: 'base-sepolia' },
  payTo: '0xYourAddress',
}));
```

#### Client (sending payments)

```typescript
import { X402PaymentHandler, X402BudgetTracker } from '@fabstir/orchestrator';

const handler = new X402PaymentHandler(signerProvider, usdcAddress);
const budget = new X402BudgetTracker(1.0); // $1 max spend

// Automatically handles 402 responses
const header = await handler.createPaymentHeader(paymentRequirements);
```

## Support

- GitHub Issues: https://github.com/fabstir/fabstir-llm-sdk/issues
- Documentation: https://docs.fabstir.com
- Discord: https://discord.gg/fabstir

## License

MIT License - See LICENSE file for details.