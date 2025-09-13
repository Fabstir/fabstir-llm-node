# Current Contract Addresses - Base Sepolia

Last Updated: January 13, 2025 (Model Governance System Added)

> **🚀 LATEST DEPLOYMENT**: Model Governance System + USDC Support Fixed (2025-09-13)
> - **JobMarketplace**: `0x001A47Bb8C6CaD9995639b8776AB5816Ab9Ac4E0` ✅ CURRENT - USDC support fixed
> - **ModelRegistry**: `0x92b2De840bB2171203011A6dBA928d855cA8183E` ✅ Model governance - ONLY 2 approved models
> - **NodeRegistryWithModels**: `0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218` ✅ Model-validated registration
> - **HostEarnings**: `0x908962e8c6CE72610021586f85ebDE09aAc97776` ✅ ETH & USDC accumulation
> - **ProofSystem**: `0x2ACcc60893872A499700908889B38C5420CBcFD1` ✅ FIXED internal verification
> - **Note**: Model governance ensures only approved models can be used in the marketplace

## ✅ Active Contracts - With Model Governance + Treasury/Host Accumulation (Current)

These contracts include model governance, all fixes, AND both treasury and host earnings accumulation for maximum gas savings:

| Contract | Address | Description |
|----------|---------|-------------|
| **JobMarketplace** | `0x001A47Bb8C6CaD9995639b8776AB5816Ab9Ac4E0` | ✅ CURRENT - USDC support fixed |
| **ModelRegistry** | `0x92b2De840bB2171203011A6dBA928d855cA8183E` | ✅ Model governance - ONLY 2 approved models |
| **NodeRegistryWithModels** | `0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218` | ✅ Node registration with model validation |
| **ProofSystem** | `0x2ACcc60893872A499700908889B38C5420CBcFD1` | ✅ FIXED internal verification for USDC |
| **HostEarnings** | `0x908962e8c6CE72610021586f85ebDE09aAc97776` | ✅ Host earnings accumulation (ETH & USDC) |

### 🎯 Approved Models for MVP Testing

| Model | HuggingFace Repo | File | SHA256 Hash |
|-------|------------------|------|-------------|
| **TinyVicuna-1B** | CohereForAI/TinyVicuna-1B-32k-GGUF | tiny-vicuna-1b.q4_k_m.gguf | 0x329d002bc20d4e7baae25df802c9678b5a4340b3ce91f23e6a0644975e95935f |
| **TinyLlama-1.1B** | TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF | tinyllama-1b.Q4_K_M.gguf | 0x45b71fe98efe5f530b825dce6f5049d738e9c16869f10be4370ab81a9912d4a6 |

## ⚠️ Previous Deployments with Issues

### Wrong NodeRegistry (Jan 5, 2025)
| Contract | Address | Issue |
|----------|---------|-------|
| **JobMarketplaceFABWithS5** | `0x55A702Ab5034810F5B9720Fe15f83CFcf914F56b` | Uses old buggy NodeRegistry, hosts can't re-register |

### Missing Treasury Accumulation (Sept 4, 2025)
| Contract | Address | Issue |
|----------|---------|-------|
| **JobMarketplaceFABWithS5** | `0x9A945fFBe786881AaD92C462Ad0bd8aC177A8069` | No treasury accumulation, direct transfers only |
| **HostEarnings** | `0x67D0dB226Cc9631e3F5369cfb8b0FBFcBA576aEC` | Works but paired with non-accumulating treasury |

### Missing Treasury Initialization (Sept 4, 2025)
| Contract | Address | Issue |
|----------|---------|-------|
| **JobMarketplaceFABWithS5** | `0xEB646BF2323a441698B256623F858c8787d70f9F` | Treasury not initialized, all transactions revert |
| **HostEarnings** | `0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E` | Works but paired with unusable marketplace |

### Missing Accumulation Logic (January 2025)
| Contract | Address | Issue |
|----------|---------|-------|
| **JobMarketplaceFABWithS5** | `0xD937c594682Fe74E6e3d06239719805C04BE804A` | No accumulation logic, direct payments only |

### Missing USDC Validations (December 2024)
| Contract | Address | Issue |
|----------|---------|-------|
| **JobMarketplaceFABWithS5** | `0xebD3bbc24355d05184C7Af753d9d631E2b3aAF7A` | No host/parameter validation for USDC sessions |

### Session Jobs with Payment Bug (November 2024)
| Contract | Address | Issue |
|----------|---------|-------|
| **JobMarketplaceFABWithS5** | `0x445882e14b22E921c7d4Fe32a7736a32197578AF` | transfer() fails silently |
| **ProofSystem** | `0x707B775933C4C4c89894EC516edad83b2De77A05` | Works but paired with buggy marketplace |

### Old Single-Prompt Contracts (DEPRECATED)
| Contract | Address | Issue |
|----------|---------|-------|
| **JobMarketplaceFABWithS5** (old) | `0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304` | No session job support |
| **HostEarnings** | `0xbFfCd6BAaCCa205d471bC52Bd37e1957B1A43d4a` | Not used for session jobs |
| **PaymentEscrowWithEarnings** | `0xa4C5599Ea3617060ce86Ff0916409e1fb4a0d2c6` | Not used for session jobs |

## 📦 Token Contracts

| Token | Address | Description |
|-------|---------|-------------|
| **FAB Token** | `0xC78949004B4EB6dEf2D66e49Cd81231472612D62` | Governance and staking token |
| **USDC** | `0x036CbD53842c5426634e7929541eC2318f3dCF7e` | Base Sepolia USDC for job payments |

## 🏦 Platform Configuration

| Parameter | Value | Description |
|-----------|-------|-------------|
| **Treasury** | `0x4e770e723B95A0d8923Db006E49A8a3cb0BAA078` | Receives 10% platform fees |
| **Platform Fee** | 1000 basis points (10%) | Applied to all job completions |
| **Min Stake** | 1000 FAB tokens | Required for node registration |

## ❌ Deprecated Contracts (DO NOT USE)

These contracts are from earlier deployments and are no longer compatible with the current system:

| Contract | Address | Issue |
|----------|---------|-------|
| JobMarketplaceFABWithEarnings | `0x1A173A3703858D2F5EA4Bf48dDEb53FD4278187D` | No S5 CID support - hosts receive placeholder text |
| NodeRegistry (Original) | `0xF6420Cc8d44Ac92a6eE29A5E8D12D00aE91a73B3` | Used ETH staking instead of FAB |
| NodeRegistryFAB (Buggy) | `0x87516C13Ea2f99de598665e14cab64E191A0f8c4` | Re-registration bug - couldn't register after unregister |
| JobMarketplace (Original) | `0x6C4283A2aAee2f94BcD2EB04e951EfEa1c35b0B6` | No earnings accumulation |
| PaymentEscrow (Original) | `0x3b96fBD7b463e94463Ae4d0f2629e08cf1F25894` | No earnings support |

## 🔄 System Evolution

1. **Phase 1**: Original system with ETH staking and direct payments
2. **Phase 2**: FAB token integration and USDC support  
3. **Phase 3**: Earnings accumulation system (40-46% gas savings)
4. **Phase 4** (Current): S5 CID storage system
   - Fixes critical issue: hosts now receive actual prompts via S5 CIDs
   - Unlimited prompt/response size (not limited by gas)
   - Maintains gas efficiency of earnings system
   - Clean job ID sequence (starts from 1)

## 🚀 For Your Client Application

Update your configuration with the NEW contracts with accumulation:

```javascript
const config = {
  // Model Governance + USDC Support Fixed (CURRENT - 2025-09-13)
  jobMarketplace: '0x001A47Bb8C6CaD9995639b8776AB5816Ab9Ac4E0',  // ✅ CURRENT - USDC support fixed
  modelRegistry: '0x92b2De840bB2171203011A6dBA928d855cA8183E',   // ✅ Model governance - ONLY 2 approved models
  nodeRegistry: '0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218',    // ✅ NodeRegistryWithModels - Use this
  proofSystem: '0x2ACcc60893872A499700908889B38C5420CBcFD1',      // ✅ FIXED internal verification
  hostEarnings: '0x908962e8c6CE72610021586f85ebDE09aAc97776',    // ✅ Host earnings accumulation

  // Tokens
  fabToken: '0xC78949004B4EB6dEf2D66e49Cd81231472612D62',
  usdcToken: '0x036CbD53842c5426634e7929541eC2318f3dCF7e',

  // Platform
  treasury: '0xbeaBB2a5AEd358aA0bd442dFFd793411519Bdc11',

  // Approved Models (MVP Testing Only)
  approvedModels: {
    tinyVicuna: {
      repo: 'CohereForAI/TinyVicuna-1B-32k-GGUF',
      file: 'tiny-vicuna-1b.q4_k_m.gguf'
    },
    tinyLlama: {
      repo: 'TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF',
      file: 'tinyllama-1b.Q4_K_M.gguf'
    }
  },

  // Network
  chainId: 84532, // Base Sepolia
  rpcUrl: 'https://base-sepolia.g.alchemy.com/v2/YOUR_API_KEY'
};
```

## 📝 Important Notes

- **Model Governance**: Only approved GGUF models can be used (TinyVicuna-1B and TinyLlama-1.1B for MVP)
- **Gas Savings**: ~80% reduction in gas costs through dual accumulation (treasury + host)
- **Host Registration**: Will require approved model IDs once NodeRegistryWithModels is deployed
- **Host Withdrawals**: Hosts can withdraw accumulated earnings at their convenience
- **Treasury Withdrawals**: Treasury can batch withdraw all accumulated fees with one transaction
- **Job IDs**: Start from 1 in JobMarketplaceLite
- **Payment Support**: Both ETH and USDC payments with accumulation
- **Staking**: Requires FAB tokens, not ETH
- **Contract Size**: JobMarketplaceLite is 5KB (vs 26KB original)
- **Verification**: All contracts verified on [Base Sepolia Explorer](https://sepolia.basescan.org)