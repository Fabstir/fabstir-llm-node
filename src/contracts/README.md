# Contracts Module

This module provides integration with smart contracts on Base L2 for the Fabstir LLM Node.

## Components

### Web3 Client (`client.rs`)
- Ethereum provider management with ethers-rs
- Wallet and transaction handling
- Network switching (Base mainnet/testnet)
- Gas estimation and EIP-1559 support
- Block monitoring and event filtering

### Job Monitor (`monitor.rs`)
- Monitors JobMarketplace contract for job events
- Tracks job lifecycle (Posted → Claimed → Completed)
- Filters eligible jobs based on node capabilities
- Event replay from checkpoints
- Concurrent event processing

### Payment Verifier (`payments.rs`)
- Verifies escrow deposits for jobs
- Monitors payment releases and disputes
- Multi-token support (ETH, USDC, etc.)
- Fee calculation and splitting
- Payment history tracking

### Proof Submitter (`proofs.rs`)
- Generates and submits EZKL proofs
- Monitors proof verification status
- Handles proof challenges
- Batch proof submission
- IPFS storage optimization

## Usage

```rust
use fabstir_llm_node::contracts::{Web3Client, Web3Config, JobMonitor, JobMonitorConfig};

// Create Web3 client
let config = Web3Config {
    rpc_url: "https://sepolia.base.org".to_string(),
    chain_id: 84532, // Base Sepolia
    ..Default::default()
};
let web3_client = Arc::new(Web3Client::new(config).await?);

// Monitor jobs
let monitor_config = JobMonitorConfig {
    marketplace_address: "0x...".parse()?,
    registry_address: "0x...".parse()?,
    ..Default::default()
};
let mut monitor = JobMonitor::new(monitor_config, web3_client).await?;
let mut events = monitor.start().await;

// Process events
while let Some(event) = events.recv().await {
    match event {
        JobEvent::JobPosted { job_id, .. } => {
            // Check if eligible and claim
        }
        JobEvent::JobCompleted { job_id, .. } => {
            // Submit proof and claim payment
        }
        _ => {}
    }
}
```

## Testing

The contract module includes comprehensive tests that require either:
1. A local Ethereum node (Anvil/Hardhat) running on port 8545
2. Access to Base Sepolia testnet

Tests are located in `tests/contracts/` and cover all major functionality.