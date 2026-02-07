# Migration Guide: Required Environment Variables (v8.4.4+)

**Date**: February 1, 2026
**Breaking Change**: Environment variables for contract addresses are now REQUIRED

---

## Overview

Starting with version 8.4.4, the Fabstir LLM Node **requires** all contract addresses to be set via environment variables. The node will **fail to start** if any required variables are missing.

This change eliminates the risk of accidentally using deprecated pre-AUDIT-F4 contracts.

---

## What Changed

### Before (v8.4.3 and earlier)
- Contract addresses had hardcoded fallbacks
- Node would start even without `.env` file
- Could silently use wrong/deprecated contracts
- Configuration errors were hidden

### After (v8.4.4+)
- **All** contract addresses must be in `.env` file
- Node fails immediately if any are missing
- Clear error messages tell you which variable is missing
- No silent fallbacks to wrong contracts

---

## Required Environment Variables

All of these MUST be set in your `.env` file:

```bash
# RPC URL
BASE_SEPOLIA_RPC_URL=https://sepolia.base.org

# AUDIT-F4 Remediated Contracts (January 31, 2026)
CONTRACT_JOB_MARKETPLACE=0x95132177F964FF053C1E874b53CF74d819618E06
CONTRACT_PROOF_SYSTEM=0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31

# Other Required Contracts
CONTRACT_NODE_REGISTRY=0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22
CONTRACT_HOST_EARNINGS=0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0
CONTRACT_MODEL_REGISTRY=0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2

# Token Addresses
USDC_TOKEN=0x036CbD53842c5426634e7929541eC2318f3dCF7e
FAB_TOKEN=0xC78949004B4EB6dEf2D66e49Cd81231472612D62

# Optional (will warn if not set)
MULTICALL3_ADDRESS=0xcA11bde05977b3631167028862bE2a173976CA11
```

---

## Migration Steps

### Step 1: Check Your Current Configuration

```bash
# Navigate to your node directory
cd /path/to/fabstir-llm-node

# Check if .env exists
ls -la .env
```

### Step 2: Copy From .env.contracts

If you don't have a `.env` file, create one from `.env.contracts`:

```bash
# Copy all AUDIT-F4 addresses
cp .env.contracts .env
```

### Step 3: Add RPC URL

Add your RPC endpoint to `.env`:

```bash
# Add this line to .env
echo "BASE_SEPOLIA_RPC_URL=https://sepolia.base.org" >> .env
```

Or use your own RPC provider (Alchemy, Infura, etc.):

```bash
echo "BASE_SEPOLIA_RPC_URL=https://base-sepolia.g.alchemy.com/v2/YOUR_KEY" >> .env
```

### Step 4: Verify Configuration

Test that the node can start:

```bash
# Should start successfully
cargo run --release

# Or if using Docker
docker-compose up -d
```

### Step 5: Check Logs

Verify the node is using correct contracts:

```bash
# Look for contract addresses in logs
docker logs llm-node-prod | grep "0x9513"  # Should see JobMarketplace
docker logs llm-node-prod | grep "0xE8DC"  # Should see ProofSystem
```

---

## Error Messages

If you're missing a required variable, you'll see clear error messages:

### Missing JobMarketplace

```
thread 'main' panicked at src/blockchain/chain_config.rs:49:
CONTRACT_JOB_MARKETPLACE environment variable is required (AUDIT-F4 remediated contract): NotPresent
```

**Fix**: Add to `.env`:
```bash
CONTRACT_JOB_MARKETPLACE=0x95132177F964FF053C1E874b53CF74d819618E06
```

### Missing RPC URL

```
thread 'main' panicked at src/blockchain/chain_config.rs:41:
BASE_SEPOLIA_RPC_URL environment variable is required: NotPresent
```

**Fix**: Add to `.env`:
```bash
BASE_SEPOLIA_RPC_URL=https://sepolia.base.org
```

### Missing FAB Token (during registration)

```
thread 'main' panicked at src/blockchain/multi_chain_registrar.rs:115:
FAB_TOKEN environment variable is required for node registration: NotPresent
```

**Fix**: Add to `.env`:
```bash
FAB_TOKEN=0xC78949004B4EB6dEf2D66e49Cd81231472612D62
```

---

## Docker Users

If you're using Docker, ensure your `.env` file is in the same directory as `docker-compose.yml`:

```bash
# Check .env exists
ls -la .env

# Restart container to pick up new env vars
docker-compose down
docker-compose up -d

# Verify container started
docker ps | grep llm-node
```

---

## Testing Your Configuration

Quick test to verify all required variables are set:

```bash
# Check all required vars are present
grep "CONTRACT_JOB_MARKETPLACE" .env
grep "CONTRACT_PROOF_SYSTEM" .env
grep "CONTRACT_NODE_REGISTRY" .env
grep "CONTRACT_HOST_EARNINGS" .env
grep "CONTRACT_MODEL_REGISTRY" .env
grep "USDC_TOKEN" .env
grep "FAB_TOKEN" .env
grep "BASE_SEPOLIA_RPC_URL" .env

# All should return a line with the address
```

---

## Rollback

If you need to rollback to the previous version:

```bash
# Use v8.4.3 binary (has fallbacks)
git checkout v8.4.3
cargo build --release --features real-ezkl -j 4

# Or use pre-built tarball
wget https://github.com/Fabstir/fabstir-llm-node/releases/download/v8.4.3/fabstir-llm-node-v8.4.3.tar.gz
tar -xzf fabstir-llm-node-v8.4.3.tar.gz
```

**Note**: v8.4.3 still has hardcoded fallbacks to deprecated contracts, so this is NOT recommended.

---

## Why This Change?

### Security
- Prevents accidental use of deprecated contracts
- Forces explicit configuration
- No hidden fallbacks

### AUDIT-F4 Compliance
- Ensures AUDIT-F4 remediated contracts are used
- Prevents cross-model replay attacks
- Maintains signature verification integrity

### Operational Safety
- Fail-fast on misconfiguration
- Clear error messages
- Easier debugging

---

## Support

If you encounter issues:

1. Check `.env` file has all required variables
2. Verify addresses match those in `.env.contracts`
3. Ensure RPC URL is valid and accessible
4. Check logs for specific error messages

For help, open an issue at: https://github.com/Fabstir/fabstir-llm-node/issues
