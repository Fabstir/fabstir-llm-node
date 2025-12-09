# Current Contract Addresses - Multi-Chain Support

Last Updated: December 9, 2025

## 🌐 Multi-Chain Deployment Status

| Chain | Network | Status | Native Token | Contract Address |
|-------|---------|--------|--------------|------------------|
| **Base** | Sepolia (Testnet) | ✅ DEPLOYED | ETH | `0x0c942eADAF86855F69Ee4fa7f765bc6466f254A1` |
| **opBNB** | Testnet | ⏳ PLANNED | BNB | Post-MVP deployment |
| **Base** | Mainnet | ⏳ FUTURE | ETH | TBD |
| **opBNB** | Mainnet | ⏳ FUTURE | BNB | TBD |

> **🚀 LATEST DEPLOYMENT**: Flexible Pricing (Per-Model & Multi-Token)
>
> - **JobMarketplaceWithModels**: `0x0c942eADAF86855F69Ee4fa7f765bc6466f254A1` ✅ NEW - Per-model pricing, model-aware sessions (Dec 9, 2025)
> - **NodeRegistryWithModels**: `0x48aa4A8047A45862Da8412FAB71ef66C17c7766d` ✅ NEW - Per-model pricing, multi-token support (Dec 9, 2025)
> - **Features**: Per-model pricing (setModelPricing), multi-token pricing (setTokenPricing), model-aware sessions (createSessionJobForModel), batch price queries (getHostModelPrices)
> - **New Functions**: `setModelPricing()`, `clearModelPricing()`, `getModelPricing()`, `getHostModelPrices()`, `setTokenPricing()`, `createSessionJobForModel()`, `createSessionJobForModelWithToken()`
> - **Backward Compatible**: All existing SDK functions work unchanged

## Active Contracts

| Contract | Address | Description |
|----------|---------|-------------|
| **JobMarketplaceWithModels** | `0x0c942eADAF86855F69Ee4fa7f765bc6466f254A1` | ✅ ACTIVE - Flexible pricing, model-aware sessions (Dec 9, 2025) |
| **NodeRegistryWithModels** | `0x48aa4A8047A45862Da8412FAB71ef66C17c7766d` | ✅ ACTIVE - Per-model pricing, multi-token support (Dec 9, 2025) |
| **ModelRegistry** | `0x92b2De840bB2171203011A6dBA928d855cA8183E` | Model governance (3 approved models) |
| **ProofSystem** | `0x2ACcc60893872A499700908889B38C5420CBcFD1` | EZKL proof verification |
| **HostEarnings** | `0x908962e8c6CE72610021586f85ebDE09aAc97776` | Host earnings accumulation |

## Deprecated Contracts

| Contract | Address | Description |
|----------|---------|-------------|
| **JobMarketplaceWithModels** | `0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E` | ⚠️ DEPRECATED - S5 proof storage version (Oct 14, 2025) |
| **JobMarketplaceWithModels** | `0xe169A4B57700080725f9553E3Cc69885fea13629` | ⚠️ DEPRECATED - Old proof storage (Jan 28, 2025) |
| **NodeRegistryWithModels** | `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6` | ⚠️ DEPRECATED - Dual pricing without per-model (Jan 28, 2025) |

## Approved Models

| Model | HuggingFace Repo | File | Model ID |
|-------|------------------|------|----------|
| **TinyVicuna-1B** | CohereForAI/TinyVicuna-1B-32k-GGUF | tiny-vicuna-1b.q4_k_m.gguf | `0x0b75a206...` |
| **TinyLlama-1.1B** | TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF | tinyllama-1b.Q4_K_M.gguf | `0x14843424...` |
| **OpenAI-GPT-OSS-20B** | bartowski/openai_gpt-oss-20b-GGUF | openai_gpt-oss-20b-MXFP4.gguf | `0x7583557c...` |

> See [API Reference](docs/API_REFERENCE.md) for full model IDs.

## Token Contracts

| Token | Address | Description |
|-------|---------|-------------|
| **FAB Token** | `0xC78949004B4EB6dEf2D66e49Cd81231472612D62` | Governance and staking |
| **USDC** | `0x036CbD53842c5426634e7929541eC2318f3dCF7e` | Job payments |

## Platform Configuration

| Parameter | Value |
|-----------|-------|
| **Treasury** | `0xbeaBB2a5AEd358aA0bd442dFFd793411519Bdc11` |
| **Treasury Fee** | Configurable via TREASURY_FEE_PERCENTAGE env var |
| **Min Stake** | 1000 FAB tokens |
| **Min Deposit (ETH)** | 0.0002 ETH |
| **Min Deposit (USDC)** | 0.80 USDC |

## Chain-Specific Configuration

### Base Sepolia (ETH)
```javascript
const baseSepoliaConfig = {
  chainId: 84532,
  nativeToken: "ETH",
  contracts: {
    jobMarketplace: "0x0c942eADAF86855F69Ee4fa7f765bc6466f254A1", // Flexible pricing (Dec 9, 2025)
    nodeRegistry: "0x48aa4A8047A45862Da8412FAB71ef66C17c7766d", // Per-model pricing (Dec 9, 2025)
    modelRegistry: "0x92b2De840bB2171203011A6dBA928d855cA8183E",
    proofSystem: "0x2ACcc60893872A499700908889B38C5420CBcFD1",
    hostEarnings: "0x908962e8c6CE72610021586f85ebDE09aAc97776",
    fabToken: "0xC78949004B4EB6dEf2D66e49Cd81231472612D62",
    usdcToken: "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
    weth: "0x4200000000000000000000000000000000000006"
  },
  rpcUrl: "https://sepolia.base.org",
  explorer: "https://sepolia.basescan.org"
};
```

### opBNB Testnet (BNB) - Future Deployment
```javascript
const opBNBConfig = {
  chainId: 5611, // opBNB testnet
  nativeToken: "BNB",
  contracts: {
    // To be deployed post-MVP
    jobMarketplace: "TBD",
    // Supporting contracts will need deployment
  },
  rpcUrl: "https://opbnb-testnet-rpc.bnbchain.org",
  explorer: "https://testnet.opbnbscan.com"
};
```

## Network Information

- **Network**: Base Sepolia
- **Chain ID**: 84532
- **RPC URL**: https://sepolia.base.org
- **Block Explorer**: https://sepolia.basescan.org