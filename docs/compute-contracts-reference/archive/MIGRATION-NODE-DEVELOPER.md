# Migration Guide for Node Developers

**Date:** December 14, 2025
**Change:** UUPS Upgradeable Contract Migration + Minimum Deposit Reduction

---

## Summary

All Fabstir marketplace contracts have been upgraded to UUPS (Universal Upgradeable Proxy Standard) pattern. **You must update your node software to use the new contract addresses.**

Additionally, minimum session deposits have been reduced to ~$0.50.

---

## New Contract Addresses (Base Sepolia)

```javascript
// OLD ADDRESSES (DEPRECATED - DO NOT USE)
// jobMarketplace: "0x75C72e8C3eC707D8beF5Ba9b9C4f75CbB5bced97"
// nodeRegistry: "0x906F4A8Cb944E4fe12Fb85Be7E627CeDAA8B8999"
// etc.

// NEW ADDRESSES (UUPS Proxies) - USE THESE
const contracts = {
  jobMarketplace: "0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D",
  nodeRegistry: "0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22",
  modelRegistry: "0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2",
  proofSystem: "0x5afB91977e69Cc5003288849059bc62d47E7deeb",
  hostEarnings: "0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0",

  // Tokens (unchanged)
  fabToken: "0xC78949004B4EB6dEf2D66e49Cd81231472612D62",
  usdcToken: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
};

// Implementation addresses (for verification only)
const implementations = {
  jobMarketplace: "0xe0ee96FC4Cc7a05a6e9d5191d070c5d1d13f143F",
  nodeRegistry: "0x68298e2b74a106763aC99E3D973E98012dB5c75F",
  modelRegistry: "0xd7Df5c6D4ffe6961d47753D1dd32f844e0F73f50",
  proofSystem: "0x83eB050Aa3443a76a4De64aBeD90cA8d525E7A3A",
  hostEarnings: "0x588c42249F85C6ac4B4E27f97416C0289980aabB"
};
```

---

## What Changed?

### Contract Addresses
- All 5 core contracts have new addresses (they are now proxies)
- The proxy addresses are permanent - future upgrades won't change them

### Minimum Deposits Reduced
| Payment Type | Old Minimum | New Minimum |
|--------------|-------------|-------------|
| ETH | 0.0002 ETH (~$0.88) | 0.0001 ETH (~$0.50) |
| USDC | 0.80 USDC | 0.50 USDC |

### ABI Changes
- **Backward compatible** - existing functions unchanged
- **New function**: `updateTokenMinDeposit(address token, uint256 minDeposit)` (admin only)
- **New event**: `TokenMinDepositUpdated(address indexed token, uint256 oldMinDeposit, uint256 newMinDeposit)`

### New Feature: Emergency Pause
- JobMarketplace now has `pause()`/`unpause()` functions
- When paused: session creation and proof submission are blocked
- When paused: session completion and withdrawals still work (safety)

---

## Action Required

### 1. Update Contract Addresses
Replace all hardcoded contract addresses in your node software with the new ones above.

### 2. Re-register Your Node
Since this is a fresh deployment, you need to re-register:

```bash
# You'll need FAB tokens for staking (1000 FAB minimum)
# Then call registerNode with your models and pricing
```

### 3. Test Your Integration
- Verify you can query your node status: `nodeRegistry.isActiveNode(yourAddress)`
- Verify you can submit proofs to sessions
- Verify earnings accumulate in HostEarnings

---

## No Changes Required

- Core function signatures (same ABI for existing functions)
- Proof submission flow
- Session completion flow
- Pricing structure (PRICE_PRECISION=1000)
- Token addresses (FAB, USDC)

---

## Questions?

Contact the Fabstir team if you encounter any issues during migration.
