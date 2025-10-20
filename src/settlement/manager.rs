// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use super::gas_estimator::GasEstimator;
use super::queue::{SettlementQueue, SettlementRequest};
use super::types::{SettlementError, SettlementResult, SettlementStatus};
use crate::config::chains::{ChainConfig, ChainRegistry};
use anyhow::{anyhow, Result};
use ethers::{
    prelude::*,
    providers::{Http, Provider},
    signers::LocalWallet,
    types::{Address, H256, U256},
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

// Type aliases for clarity
type ChainProvider = Arc<Provider<Http>>;
type ChainSigner = Arc<SignerMiddleware<Arc<Provider<Http>>, LocalWallet>>;

pub struct SettlementManager {
    chain_registry: Arc<ChainRegistry>,
    providers: HashMap<u64, ChainProvider>,
    signers: HashMap<u64, ChainSigner>,
    gas_estimator: GasEstimator,
    settlement_queue: Arc<RwLock<SettlementQueue>>,
    host_address: Address,
}

impl SettlementManager {
    pub async fn new(chain_registry: Arc<ChainRegistry>, host_private_key: &str) -> Result<Self> {
        let mut providers = HashMap::new();
        let mut signers = HashMap::new();

        // Parse the wallet once
        let wallet = host_private_key
            .parse::<LocalWallet>()
            .map_err(|e| anyhow!("Failed to parse private key: {}", e))?;
        let host_address = wallet.address();

        info!(
            "Initializing SettlementManager for address: {}",
            host_address
        );

        // Initialize providers and signers for each supported chain
        for chain_id in chain_registry.list_supported_chains() {
            let chain_config = chain_registry
                .get_chain(chain_id)
                .ok_or_else(|| anyhow!("Chain {} not found in registry", chain_id))?;

            // Create provider
            let provider = Provider::<Http>::try_from(&chain_config.rpc_url)
                .map_err(|e| anyhow!("Failed to create provider for chain {}: {}", chain_id, e))?;
            let provider = Arc::new(provider);

            // Create chain-specific wallet
            let chain_wallet = wallet.clone().with_chain_id(chain_id);

            // Create signer
            let signer = SignerMiddleware::new(provider.clone(), chain_wallet);
            let signer = Arc::new(signer);

            providers.insert(chain_id, provider.clone());
            signers.insert(chain_id, signer);

            info!(
                "Initialized chain {} ({}) with RPC: {}",
                chain_config.name, chain_id, chain_config.rpc_url
            );
        }

        Ok(Self {
            chain_registry,
            providers,
            signers,
            gas_estimator: GasEstimator::new(),
            settlement_queue: Arc::new(RwLock::new(SettlementQueue::new())),
            host_address,
        })
    }

    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    pub fn get_provider(&self, chain_id: u64) -> Option<ChainProvider> {
        self.providers.get(&chain_id).cloned()
    }

    pub fn get_signer(&self, chain_id: u64) -> Option<ChainSigner> {
        self.signers.get(&chain_id).cloned()
    }

    pub async fn check_provider_health(&self, chain_id: u64) -> Result<()> {
        let provider = self
            .get_provider(chain_id)
            .ok_or_else(|| anyhow!("No provider for chain {}", chain_id))?;

        // Try to get block number as health check
        let block_number = provider
            .get_block_number()
            .await
            .map_err(|e| anyhow!("Provider health check failed for chain {}: {}", chain_id, e))?;

        info!(
            "Chain {} health check passed, block: {}",
            chain_id, block_number
        );
        Ok(())
    }

    pub async fn check_balance(&self, chain_id: u64) -> Result<U256> {
        let provider = self
            .get_provider(chain_id)
            .ok_or_else(|| anyhow!("No provider for chain {}", chain_id))?;

        let balance = provider
            .get_balance(self.host_address, None)
            .await
            .map_err(|e| anyhow!("Failed to get balance for chain {}: {}", chain_id, e))?;

        Ok(balance)
    }

    pub async fn estimate_settlement_gas(
        &self,
        chain_id: u64,
        session_id: u64,
    ) -> Result<U256, SettlementError> {
        // Get gas estimate for settlement operation
        let gas_limit = self
            .gas_estimator
            .estimate_with_buffer(chain_id, "settle_session")?;

        // Get current gas price from provider
        if let Some(provider) = self.get_provider(chain_id) {
            let gas_price = provider
                .get_gas_price()
                .await
                .map_err(|e| SettlementError::ProviderError(e.to_string()))?;

            // Calculate total cost
            let total_cost = gas_limit * gas_price;

            info!(
                "Gas estimate for session {} on chain {}: limit={}, price={}, total={}",
                session_id, chain_id, gas_limit, gas_price, total_cost
            );

            Ok(total_cost)
        } else {
            Err(SettlementError::NoRpcEndpoint(chain_id))
        }
    }

    pub async fn queue_settlement(&self, request: SettlementRequest) -> Result<()> {
        let mut queue = self.settlement_queue.write().await;
        queue.add(request).await;
        Ok(())
    }

    pub async fn get_next_settlement(&self) -> Option<SettlementRequest> {
        let mut queue = self.settlement_queue.write().await;
        queue.get_next().await
    }

    pub async fn process_settlement_queue(&self) -> Result<Vec<SettlementResult>> {
        let mut results = Vec::new();
        let mut queue = self.settlement_queue.write().await;

        // Process up to 10 settlements at once
        for _ in 0..10 {
            if let Some(request) = queue.get_next().await {
                // Update status to processing
                queue
                    .update_status(request.session_id, SettlementStatus::Processing)
                    .await;

                // Here we would actually process the settlement
                // For now, just create a mock result
                let result = SettlementResult {
                    session_id: request.session_id,
                    chain_id: request.chain_id,
                    tx_hash: H256::zero(), // Would be actual tx hash
                    gas_used: U256::from(150_000),
                    status: SettlementStatus::Completed,
                };

                results.push(result);

                // Update status to completed
                queue
                    .update_status(request.session_id, SettlementStatus::Completed)
                    .await;
            } else {
                break;
            }
        }

        Ok(results)
    }

    pub async fn get_queue_size(&self) -> usize {
        self.settlement_queue.read().await.size().await
    }

    pub async fn get_pending_count(&self) -> usize {
        self.settlement_queue.read().await.pending_count().await
    }

    /// Settle a session on the blockchain
    /// This will be called when a WebSocket disconnects
    pub async fn settle_session(
        &self,
        session_id: u64,
        chain_id: u64,
    ) -> Result<H256, SettlementError> {
        info!("[SETTLEMENT] 🔄 Starting settlement process for session {} on chain {}", session_id, chain_id);

        let chain_config = self
            .chain_registry
            .get_chain(chain_id)
            .ok_or_else(|| {
                error!("[SETTLEMENT] ❌ Chain {} not found in registry", chain_id);
                SettlementError::UnsupportedChain(chain_id)
            })?;

        info!("[SETTLEMENT] ✓ Chain config found: {} ({})", chain_config.name, chain_config.native_token.symbol);

        let signer = self
            .get_signer(chain_id)
            .ok_or_else(|| {
                error!("[SETTLEMENT] ❌ No signer configured for chain {}", chain_id);
                SettlementError::SignerNotFound(chain_id)
            })?;

        info!(
            "[SETTLEMENT] ✓ Signer ready for chain {} - host address: {}",
            chain_id, self.host_address
        );

        // Get gas estimate
        info!("[SETTLEMENT] 📊 Estimating gas for settlement transaction...");
        let gas_limit = self
            .gas_estimator
            .estimate_with_buffer(chain_id, "settle_session")
            .map_err(|e| {
                error!("[SETTLEMENT] ❌ Gas estimation failed: {:?}", e);
                e
            })?;

        info!("[SETTLEMENT] ✓ Gas limit estimated: {}", gas_limit);

        // Check host balance
        if let Some(provider) = self.get_provider(chain_id) {
            match provider.get_balance(self.host_address, None).await {
                Ok(balance) => {
                    info!("[SETTLEMENT] 💰 Host balance on chain {}: {}", chain_id, balance);
                    if balance < gas_limit {
                        warn!("[SETTLEMENT] ⚠️ Host balance may be insufficient for gas costs");
                    }
                }
                Err(e) => {
                    warn!("[SETTLEMENT] ⚠️ Failed to check host balance: {}", e);
                }
            }
        }

        // Here we would build and send the actual transaction
        // For now, return a mock transaction hash
        warn!("[SETTLEMENT] ⚠️ MOCK: Settlement transaction not yet implemented - returning mock hash");
        warn!("[SETTLEMENT] ⚠️ TODO: Integrate with smart contract to trigger actual payment distribution");
        warn!("[SETTLEMENT] ⚠️ Expected flow: Call contract.settleSession(session_id) -> Distribute payments");

        let mock_hash = H256::from_low_u64_be(session_id);
        info!("[SETTLEMENT] 🎯 Mock settlement completed with hash: {:?}", mock_hash);

        Ok(mock_hash)
    }
}
