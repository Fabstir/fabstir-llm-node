# Multi-Chain Configuration Guide

This guide explains how to configure the Fabstir LLM Node for multi-chain operation.

## Overview

The Fabstir LLM Node supports multiple blockchain networks simultaneously, allowing nodes to serve clients across different chains. Currently supported chains:

- **Base Sepolia** (Chain ID: 84532) - Ethereum L2 testnet
- **opBNB Testnet** (Chain ID: 5611) - BSC L2 testnet

## Configuration Files

### 1. Environment Variables (.env)

Create a `.env` file in the project root with your chain-specific configurations:

```bash
# Base Sepolia Configuration
BASE_SEPOLIA_RPC=https://sepolia.base.org
BASE_SEPOLIA_CHAIN_ID=84532
BASE_SEPOLIA_JOB_MARKETPLACE=0xaa38e7fcf5d7944ef7c836e8451f3bf93b98364f
BASE_SEPOLIA_NODE_REGISTRY=0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218
BASE_SEPOLIA_PAYMENT_ESCROW=0x908962e8c6CE72610021586f85ebDE09aAc97776

# opBNB Testnet Configuration
OPBNB_TESTNET_RPC=https://opbnb-testnet-rpc.bnbchain.org
OPBNB_TESTNET_CHAIN_ID=5611
OPBNB_TESTNET_JOB_MARKETPLACE=0x<your-contract>
OPBNB_TESTNET_NODE_REGISTRY=0x<your-contract>
OPBNB_TESTNET_PAYMENT_ESCROW=0x<your-contract>

# Default Chain (optional)
DEFAULT_CHAIN_ID=84532
```

### 2. Contract Addresses (.env.contracts)

The `.env.contracts` file serves as the single source of truth for contract addresses:

```bash
# S5-Enabled Contracts (Latest deployment)
NODE_REGISTRY_FAB_ADDRESS=0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218
JOB_MARKETPLACE_FAB_WITH_S5_ADDRESS=0xaa38e7fcf5d7944ef7c836e8451f3bf93b98364f
PAYMENT_ESCROW_WITH_EARNINGS_ADDRESS=0x908962e8c6CE72610021586f85ebDE09aAc97776
HOST_EARNINGS_ADDRESS=0x908962e8c6CE72610021586f85ebDE09aAc97776

# Supporting Contracts
PROOF_SYSTEM_ADDRESS=0x2ACcc60893872A499700908889B38C5420CBcFD1
REPUTATION_SYSTEM_ADDRESS=0x...
MULTICALL3_ADDRESS=0xcA11bde05977b3631167028862bE2a173976CA11
```

### 3. Test Accounts (.env.test.local)

For testing, configure test accounts with funds on both chains:

```bash
# Test User Account (Client)
TEST_USER_1_ADDRESS=0x...
TEST_USER_1_PRIVATE_KEY=0x...

# Test Host Account (Node Operator)
TEST_HOST_1_ADDRESS=0x...
TEST_HOST_1_PRIVATE_KEY=0x...
```

## Chain-Specific Settings

### Base Sepolia

```bash
# Native Token: ETH
# Block Time: ~2 seconds
# Gas Price: Usually 0.001-0.01 Gwei
# Faucet: https://www.coinbase.com/faucets/base-ethereum-goerli-faucet
```

### opBNB Testnet

```bash
# Native Token: BNB
# Block Time: ~1 second
# Gas Price: Usually 3-5 Gwei
# Faucet: https://www.bnbchain.org/en/testnet-faucet
```

## Node Registration

Register your node on each chain you want to support:

```bash
# Register on Base Sepolia
cargo run --bin fabstir-cli -- register \
  --chain-id 84532 \
  --host-address $TEST_HOST_1_ADDRESS \
  --private-key $TEST_HOST_1_PRIVATE_KEY

# Register on opBNB Testnet
cargo run --bin fabstir-cli -- register \
  --chain-id 5611 \
  --host-address $TEST_HOST_1_ADDRESS \
  --private-key $TEST_HOST_1_PRIVATE_KEY
```

## WebSocket Session Configuration

WebSocket sessions are chain-aware. Clients specify the chain during session initialization:

```json
{
  "type": "session_init",
  "session_id": "unique-session-id",
  "chain_id": 84532,
  "job_id": "0x...",
  "user_address": "0x..."
}
```

## API Chain Selection

### HTTP Requests

Include chain_id in request body:

```json
POST /inference
{
  "chain_id": 84532,
  "prompt": "Hello, world!",
  "job_id": "0x...",
  "max_tokens": 100
}
```

### WebSocket Messages

All WebSocket messages include chain context:

```json
{
  "type": "prompt",
  "chain_id": 84532,
  "content": "Tell me about blockchain"
}
```

## Settlement Configuration

Automatic settlement happens per-chain when sessions end:

```bash
# Settlement thresholds (per chain)
MIN_SETTLEMENT_AMOUNT=1000000000000000  # 0.001 ETH/BNB
SETTLEMENT_BATCH_SIZE=10                # Process 10 sessions at once
SETTLEMENT_INTERVAL_MS=60000            # Check every minute
```

## Monitoring Configuration

Track metrics per chain:

```bash
# Prometheus metrics endpoint
METRICS_PORT=9090

# Per-chain metrics available:
# - fabstir_sessions_by_chain
# - fabstir_settlements_by_chain
# - fabstir_gas_used_by_chain
# - fabstir_rpc_calls_by_chain
```

## Common Configuration Issues

### 1. RPC Connection Failures

```bash
# Test RPC connectivity
curl -X POST $BASE_SEPOLIA_RPC \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

### 2. Contract Address Mismatch

Ensure contract addresses match the deployment on each chain. Check:
- `.env.contracts` for single-chain addresses
- Environment variables for multi-chain overrides

### 3. Insufficient Gas

Each chain has different gas requirements:
- Base Sepolia: Lower gas prices, higher limits
- opBNB Testnet: Higher gas prices, lower limits

### 4. Chain ID Conflicts

Always use the correct chain ID:
- Base Sepolia: 84532
- opBNB Testnet: 5611

## Environment Variable Priority

1. Command-line arguments (highest priority)
2. `.env.local` (local overrides)
3. `.env.test.local` (test configuration)
4. `.env.contracts` (contract addresses)
5. `.env` (base configuration)
6. Hardcoded defaults (lowest priority)

## Docker Configuration

For Docker deployments, mount configuration files:

```yaml
version: '3.8'
services:
  fabstir-node:
    image: fabstir/llm-node:latest
    volumes:
      - ./.env:/app/.env
      - ./.env.contracts:/app/.env.contracts
    environment:
      - DEFAULT_CHAIN_ID=84532
      - P2P_PORT=9000
      - API_PORT=8080
```

## Security Considerations

1. **Private Keys**: Never commit private keys to version control
2. **RPC Endpoints**: Use authenticated RPC endpoints in production
3. **Contract Verification**: Always verify contract addresses before deployment
4. **Chain Validation**: Validate chain IDs in all incoming requests
5. **Gas Limits**: Set appropriate gas limits per chain to prevent drain attacks

## Testing Multi-Chain Setup

Test your configuration:

```bash
# Run multi-chain tests
cargo test --test chains_tests

# Test specific chain
CHAIN_ID=84532 cargo test test_base_sepolia
CHAIN_ID=5611 cargo test test_opbnb

# Test chain switching
cargo test test_chain_switching
```

## Next Steps

After configuration:

1. Register your node on desired chains
2. Fund your node wallet with native tokens (ETH/BNB)
3. Monitor registration status
4. Start accepting jobs from multiple chains

For more details, see:
- [Deployment Guide](DEPLOYMENT.md)
- [Troubleshooting Guide](TROUBLESHOOTING.md)
- [API Documentation](API.md)