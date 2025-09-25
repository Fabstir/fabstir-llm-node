# Node Multi-Chain/Multi-Wallet Update Specification

## Overview

Update the Fabstir LLM Node to support multiple blockchain networks while maintaining automatic payment settlement on WebSocket disconnect. The node must handle different chains (Base Sepolia, opBNB) with their respective contract addresses, RPC endpoints, and native tokens.

**Note**: Wallet type (EOA vs Smart Account) is irrelevant to the node - it only cares about blockchain interactions and session management.

## Current State vs Required Changes

### Current Implementation (v5)
- **Single Chain**: Base Sepolia hardcoded
- **Fixed Contracts**: Hardcoded addresses in environment variables
- **Auto-Settlement**: Calls `completeSessionJob()` on WebSocket disconnect
- **Native Token**: Assumes ETH for gas fees
- **RPC Endpoint**: Single RPC URL for Base Sepolia

### Required Changes
- **Multi-Chain Support**: Base Sepolia + opBNB initially
- **Dynamic Contracts**: Per-chain contract addresses
- **Chain-Aware Settlement**: Call `completeSessionJob()` on correct chain
- **Native Token Handling**: ETH on Base, BNB on opBNB
- **Multi-RPC Management**: Different RPC endpoints per chain
- **Session Chain Tracking**: Remember which chain each session belongs to

## Architecture Design

### 1. Chain Configuration Structure

Create a chain configuration registry that the node loads at startup:

```rust
// src/config/chains.rs
use ethers::types::{Address, U256};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainConfig {
    pub chain_id: u64,
    pub name: String,
    pub rpc_url: String,
    pub native_token: TokenInfo,
    pub contracts: ContractAddresses,
    pub confirmation_blocks: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenInfo {
    pub symbol: String,  // "ETH" or "BNB"
    pub decimals: u8,    // 18 for both
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractAddresses {
    pub job_marketplace: Address,
    pub node_registry: Address,
    pub proof_system: Address,
    pub host_earnings: Address,
    pub model_registry: Address,
    pub usdc_token: Address,
}

impl ChainConfig {
    pub fn base_sepolia() -> Self {
        ChainConfig {
            chain_id: 84532,
            name: "Base Sepolia".to_string(),
            rpc_url: std::env::var("BASE_SEPOLIA_RPC_URL")
                .unwrap_or_else(|_| "https://sepolia.base.org".to_string()),
            native_token: TokenInfo {
                symbol: "ETH".to_string(),
                decimals: 18,
            },
            contracts: ContractAddresses {
                job_marketplace: "0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944".parse().unwrap(),
                node_registry: "0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218".parse().unwrap(),
                proof_system: "0x2ACcc60893872A499700908889B38C5420CBcFD1".parse().unwrap(),
                host_earnings: "0x908962e8c6CE72610021586f85ebDE09aAc97776".parse().unwrap(),
                model_registry: "0x92b2De840bB2171203011A6dBA928d855cA8183E".parse().unwrap(),
                usdc_token: "0x036CbD53842c5426634e7929541eC2318f3dCF7e".parse().unwrap(),
            },
            confirmation_blocks: 3,
        }
    }

    pub fn opbnb_testnet() -> Self {
        ChainConfig {
            chain_id: 5611,
            name: "opBNB Testnet".to_string(),
            rpc_url: std::env::var("OPBNB_TESTNET_RPC_URL")
                .unwrap_or_else(|_| "https://opbnb-testnet-rpc.bnbchain.org".to_string()),
            native_token: TokenInfo {
                symbol: "BNB".to_string(),
                decimals: 18,
            },
            contracts: ContractAddresses {
                // These will be deployed and configured
                job_marketplace: std::env::var("OPBNB_JOB_MARKETPLACE")
                    .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000")
                    .parse().unwrap(),
                node_registry: std::env::var("OPBNB_NODE_REGISTRY")
                    .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000")
                    .parse().unwrap(),
                // ... other contracts
            },
            confirmation_blocks: 15, // BNB chains typically need more confirmations
        }
    }
}

// Chain registry
pub struct ChainRegistry {
    chains: HashMap<u64, ChainConfig>,
    default_chain: u64,
}

impl ChainRegistry {
    pub fn new() -> Self {
        let mut chains = HashMap::new();
        chains.insert(84532, ChainConfig::base_sepolia());
        chains.insert(5611, ChainConfig::opbnb_testnet());

        ChainRegistry {
            chains,
            default_chain: 84532, // Base Sepolia as default
        }
    }

    pub fn get_chain(&self, chain_id: u64) -> Option<&ChainConfig> {
        self.chains.get(&chain_id)
    }
}
```

### 2. Session Chain Tracking

Track which chain each session belongs to:

```rust
// src/session/manager.rs
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub session_id: u64,
    pub chain_id: u64,  // Track which chain this session is on
    pub user_address: Address,
    pub host_address: Address,
    pub deposit_amount: U256,
    pub payment_token: Address,
    pub tokens_used: u64,
    pub start_time: u64,
}

pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<u64, SessionInfo>>>,
    chain_registry: Arc<ChainRegistry>,
}

impl SessionManager {
    pub async fn create_session(
        &self,
        session_id: u64,
        chain_id: u64,
        user: Address,
        host: Address,
        deposit: U256,
        token: Address,
    ) -> Result<(), SessionError> {
        // Verify chain is supported
        if self.chain_registry.get_chain(chain_id).is_none() {
            return Err(SessionError::UnsupportedChain(chain_id));
        }

        let session = SessionInfo {
            session_id,
            chain_id,
            user_address: user,
            host_address: host,
            deposit_amount: deposit,
            payment_token: token,
            tokens_used: 0,
            start_time: chrono::Utc::now().timestamp() as u64,
        };

        self.sessions.write().await.insert(session_id, session);
        Ok(())
    }

    pub async fn get_session_chain(&self, session_id: u64) -> Option<u64> {
        self.sessions.read().await
            .get(&session_id)
            .map(|s| s.chain_id)
    }
}
```

### 3. Multi-Chain Payment Settlement

Update the automatic settlement to use the correct chain:

```rust
// src/blockchain/settlement.rs
use ethers::prelude::*;

pub struct SettlementManager {
    chain_registry: Arc<ChainRegistry>,
    session_manager: Arc<SessionManager>,
    // Store providers per chain
    providers: HashMap<u64, Arc<Provider<Http>>>,
    // Store signers per chain (with host private key)
    signers: HashMap<u64, SignerMiddleware<Provider<Http>, LocalWallet>>,
}

impl SettlementManager {
    pub fn new(
        chain_registry: Arc<ChainRegistry>,
        session_manager: Arc<SessionManager>,
        host_private_key: &str,
    ) -> Result<Self, Box<dyn Error>> {
        let mut providers = HashMap::new();
        let mut signers = HashMap::new();

        // Initialize providers and signers for each chain
        for (chain_id, config) in chain_registry.chains.iter() {
            let provider = Provider::<Http>::try_from(&config.rpc_url)?;
            let provider = Arc::new(provider);

            let wallet = host_private_key.parse::<LocalWallet>()?
                .with_chain_id(*chain_id);

            let signer = SignerMiddleware::new(
                provider.clone(),
                wallet,
            );

            providers.insert(*chain_id, provider);
            signers.insert(*chain_id, signer);
        }

        Ok(SettlementManager {
            chain_registry,
            session_manager,
            providers,
            signers,
        })
    }

    /// Called automatically when WebSocket disconnects
    pub async fn settle_session(&self, session_id: u64) -> Result<TxHash, SettlementError> {
        // Get session chain
        let chain_id = self.session_manager.get_session_chain(session_id)
            .await
            .ok_or(SettlementError::SessionNotFound)?;

        // Get chain config
        let chain_config = self.chain_registry.get_chain(chain_id)
            .ok_or(SettlementError::ChainNotSupported)?;

        // Get appropriate signer for this chain
        let signer = self.signers.get(&chain_id)
            .ok_or(SettlementError::SignerNotFound)?;

        // Build contract instance
        let job_marketplace = JobMarketplace::new(
            chain_config.contracts.job_marketplace,
            Arc::new(signer.clone()),
        );

        info!(
            "Settling session {} on chain {} ({}) using native token {}",
            session_id,
            chain_config.name,
            chain_id,
            chain_config.native_token.symbol
        );

        // Call completeSessionJob - host pays gas in native token (ETH/BNB)
        let tx = job_marketplace
            .complete_session_job(U256::from(session_id))
            .send()
            .await?
            .await?;

        info!(
            "Session {} settled on chain {} with tx: {:?}",
            session_id, chain_config.name, tx.transaction_hash
        );

        Ok(tx.transaction_hash)
    }
}
```

### 4. WebSocket Handler Updates

Update WebSocket handler to track chain information:

```rust
// src/websocket/handler.rs
impl WebSocketHandler {
    async fn handle_session_init(&mut self, msg: SessionInitMessage) {
        let job_id = msg.job_id;
        let chain_id = msg.chain_id.unwrap_or(84532); // Default to Base Sepolia

        // Verify job exists on specified chain
        let chain_config = self.chain_registry.get_chain(chain_id)
            .expect("Invalid chain");

        let provider = Provider::<Http>::try_from(&chain_config.rpc_url).unwrap();
        let job_marketplace = JobMarketplace::new(
            chain_config.contracts.job_marketplace,
            Arc::new(provider),
        );

        // Verify job on blockchain
        let job = job_marketplace.get_job(job_id).await.unwrap();

        // Create session with chain tracking
        self.session_manager.create_session(
            job_id,
            chain_id,
            job.user,
            job.host,
            job.deposit,
            job.payment_token,
        ).await.unwrap();

        info!("Session {} initialized on chain {}", job_id, chain_config.name);
    }

    async fn handle_disconnect(&mut self, session_id: u64) {
        info!("WebSocket disconnected for session {}", session_id);

        // Trigger automatic settlement on the correct chain
        match self.settlement_manager.settle_session(session_id).await {
            Ok(tx_hash) => {
                info!("Session {} settled with tx: {:?}", session_id, tx_hash);
            }
            Err(e) => {
                error!("Failed to settle session {}: {:?}", session_id, e);
                // Could implement retry logic here
            }
        }
    }
}
```

### 5. Node Registration on Multiple Chains

Support registering the node on multiple chains:

```rust
// src/blockchain/registration.rs
pub async fn register_on_all_chains(
    chain_registry: &ChainRegistry,
    host_private_key: &str,
    api_url: &str,
) -> Result<(), Box<dyn Error>> {
    for (chain_id, config) in chain_registry.chains.iter() {
        info!("Registering node on chain {} ({})", config.name, chain_id);

        let provider = Provider::<Http>::try_from(&config.rpc_url)?;
        let wallet = host_private_key.parse::<LocalWallet>()?
            .with_chain_id(*chain_id);
        let signer = SignerMiddleware::new(provider, wallet);

        let node_registry = NodeRegistry::new(
            config.contracts.node_registry,
            Arc::new(signer),
        );

        // Register node with API URL
        let tx = node_registry
            .register_node(api_url.to_string())
            .send()
            .await?;

        info!(
            "Node registered on {} with tx: {:?}",
            config.name, tx.tx_hash()
        );

        // Wait for confirmation
        tx.await?.unwrap();
    }

    Ok(())
}
```

### 6. Environment Configuration

Update `.env` configuration for multi-chain support:

```bash
# Chain RPC Endpoints
BASE_SEPOLIA_RPC_URL=https://sepolia.base.org
OPBNB_TESTNET_RPC_URL=https://opbnb-testnet-rpc.bnbchain.org

# Host Configuration (same key works on all chains)
HOST_PRIVATE_KEY=0x...

# Default chain for new sessions if not specified
DEFAULT_CHAIN_ID=84532  # Base Sepolia

# Chain-specific contract overrides (optional)
# If not set, uses defaults from code
OPBNB_JOB_MARKETPLACE=0x...
OPBNB_NODE_REGISTRY=0x...
OPBNB_HOST_EARNINGS=0x...

# Gas price multipliers per chain (optional)
BASE_GAS_MULTIPLIER=1.1
OPBNB_GAS_MULTIPLIER=1.2
```

### 7. API Updates

Update the HTTP API to support chain specification:

```rust
// GET /v1/models?chain_id=84532
// Returns models available on specific chain

// POST /v1/inference
{
  "chain_id": 84532,  // Optional, defaults to BASE_SEPOLIA
  "job_id": 123,
  "model": "llama",
  "prompt": "..."
}

// GET /v1/session/{job_id}/info
// Returns:
{
  "session_id": 123,
  "chain_id": 84532,
  "chain_name": "Base Sepolia",
  "status": "active",
  "tokens_used": 450,
  "native_token": "ETH"
}
```

## Implementation Checklist

### Phase 1: Chain Configuration (Priority 1)
- [ ] Create `ChainConfig` structure
- [ ] Implement `ChainRegistry` with Base Sepolia and opBNB configs
- [ ] Load chain configs from environment/config file
- [ ] Add chain validation utilities

### Phase 2: Session Management (Priority 2)
- [ ] Update `SessionInfo` to include `chain_id`
- [ ] Modify session creation to accept chain parameter
- [ ] Update session storage to track chain
- [ ] Add chain querying methods

### Phase 3: Payment Settlement (Priority 3)
- [ ] Create `SettlementManager` with multi-chain support
- [ ] Update WebSocket disconnect handler to use correct chain
- [ ] Implement chain-specific gas estimation
- [ ] Add settlement retry logic with exponential backoff

### Phase 4: Node Registration (Priority 4)
- [ ] Implement multi-chain registration
- [ ] Add registration status tracking per chain
- [ ] Create registration CLI command for all chains
- [ ] Add health check per chain

### Phase 5: Testing (Priority 5)
- [ ] Unit tests for chain configuration
- [ ] Integration tests for multi-chain settlement
- [ ] Test settlement with ETH on Base
- [ ] Test settlement with BNB on opBNB
- [ ] Test chain switching scenarios

## Gas Considerations

### Native Token Handling
The node must handle different native tokens for gas:

| Chain | Native Token | Used For |
|-------|--------------|----------|
| Base Sepolia | ETH | Gas for `completeSessionJob()` |
| opBNB Testnet | BNB | Gas for `completeSessionJob()` |

### Gas Estimation
```rust
// Different chains may need different gas limits
let gas_limit = match chain_id {
    84532 => U256::from(200_000),  // Base Sepolia
    5611 => U256::from(300_000),   // opBNB (may need more)
    _ => U256::from(250_000),      // Default
};
```

### Host Wallet Requirements
The host must maintain native token balance on each chain:
- **Base Sepolia**: Needs ETH for gas
- **opBNB**: Needs BNB for gas

Consider implementing:
- Balance monitoring per chain
- Low balance alerts
- Automatic top-up notifications

## Security Considerations

1. **Private Key Management**: Same host key works across chains, but ensure secure storage
2. **RPC Endpoint Security**: Use authenticated RPC endpoints in production
3. **Chain Verification**: Always verify chain ID before transactions
4. **Replay Protection**: Ensure transactions include correct chain ID
5. **Contract Verification**: Validate contract addresses before interacting

## Migration Guide

### From Single-Chain to Multi-Chain

1. **Update Dependencies**:
   ```toml
   [dependencies]
   ethers = { version = "2.0", features = ["ws", "rustls"] }
   ```

2. **Update Configuration**:
   - Add RPC URLs for each chain
   - Configure contract addresses per chain
   - Set default chain ID

3. **Database Migration** (if using persistent storage):
   ```sql
   ALTER TABLE sessions ADD COLUMN chain_id BIGINT DEFAULT 84532;
   CREATE INDEX idx_sessions_chain ON sessions(chain_id);
   ```

4. **Update WebSocket Protocol**:
   - Accept `chain_id` in session_init
   - Include chain info in responses

5. **Test on Each Chain**:
   - Deploy test contracts on opBNB
   - Test session creation and settlement
   - Verify gas consumption

## Monitoring and Observability

### Metrics to Track per Chain

```rust
// Prometheus metrics
static SESSION_SETTLEMENTS: Lazy<CounterVec> = Lazy::new(|| {
    register_counter_vec!(
        "node_session_settlements_total",
        "Total session settlements",
        &["chain", "status"]
    ).unwrap()
});

static SETTLEMENT_GAS_USED: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        "node_settlement_gas_used",
        "Gas used for settlements",
        &["chain", "token"]
    ).unwrap()
});

static CHAIN_BALANCE: Lazy<GaugeVec> = Lazy::new(|| {
    register_gauge_vec!(
        "node_wallet_balance",
        "Node wallet balance per chain",
        &["chain", "token"]
    ).unwrap()
});
```

### Logging

```rust
// Structured logging with chain context
info!(
    chain_id = %chain_id,
    chain_name = %chain_config.name,
    native_token = %chain_config.native_token.symbol,
    session_id = %session_id,
    "Settling session on chain"
);
```

## Error Handling

### Chain-Specific Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("Unsupported chain: {0}")]
    UnsupportedChain(u64),

    #[error("No RPC endpoint for chain: {0}")]
    NoRpcEndpoint(u64),

    #[error("Contract not deployed on chain {chain}: {contract}")]
    ContractNotDeployed { chain: u64, contract: String },

    #[error("Insufficient {token} balance on chain {chain}")]
    InsufficientBalance { chain: u64, token: String },

    #[error("Settlement failed on chain {chain}: {reason}")]
    SettlementFailed { chain: u64, reason: String },
}
```

## Future Enhancements

1. **Dynamic Chain Addition**: Add new chains without recompiling
2. **Cross-Chain Settlement**: Allow settlement on different chain than session
3. **Gas Token Abstraction**: Support gas payment in stablecoins
4. **Multi-Chain Analytics**: Aggregate metrics across all chains
5. **Automated Chain Failover**: Switch to backup RPC on failure

## Summary

This multi-chain update enables the Fabstir LLM Node to:
1. Support multiple blockchains (Base Sepolia, opBNB)
2. Track which chain each session belongs to
3. Automatically settle payments on the correct chain
4. Handle different native tokens (ETH vs BNB)
5. Maintain host earnings across multiple chains

The key principle is that the node handles ALL the complexity of multi-chain operations, making it transparent to users regardless of their wallet type or chain choice.