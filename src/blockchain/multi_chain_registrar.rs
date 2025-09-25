use anyhow::{Result, anyhow};
use ethers::prelude::*;
use ethers::providers::{Provider, Http};
use ethers::middleware::SignerMiddleware;
use std::collections::HashMap;
use std::sync::Arc;
use std::str::FromStr;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use serde_json;

use crate::config::chains::{ChainRegistry, ChainConfig};
use crate::contracts::types::NodeRegistryWithModels;

// FAB Token constants
const FAB_TOKEN_ADDRESS: &str = "0xC78949004B4EB6dEf2D66e49Cd81231472612D62";
const MIN_STAKE_AMOUNT: &str = "1000"; // 1000 FAB tokens

// Registration status for each chain
#[derive(Debug, Clone)]
pub enum RegistrationStatus {
    NotRegistered,
    Pending { tx_hash: H256 },
    Confirmed { block_number: u64 },
    Failed { error: String },
}

// Node metadata for CLI registration
#[derive(Debug, Clone)]
pub struct NodeMetadata {
    pub name: String,
    pub version: String,
    pub api_url: String,
    pub capabilities: Vec<String>,
    pub performance_tier: String,
}

pub struct MultiChainRegistrar {
    chain_registry: Arc<ChainRegistry>,
    node_metadata: NodeMetadata,
    node_address: Address,
    providers: HashMap<u64, Arc<Provider<Http>>>,
    signers: HashMap<u64, Arc<SignerMiddleware<Provider<Http>, LocalWallet>>>,
    registration_status: Arc<RwLock<HashMap<u64, RegistrationStatus>>>,
}

impl MultiChainRegistrar {
    /// Create a new MultiChainRegistrar
    pub async fn new(
        chain_registry: Arc<ChainRegistry>,
        host_private_key: &str,
        metadata: NodeMetadata,
    ) -> Result<Self> {
        let wallet = host_private_key.parse::<LocalWallet>()
            .map_err(|e| anyhow!("Invalid private key: {}", e))?;

        let node_address = wallet.address();
        info!("Initializing MultiChainRegistrar for address: {}", node_address);

        let mut providers = HashMap::new();
        let mut signers = HashMap::new();
        let registration_status = Arc::new(RwLock::new(HashMap::new()));

        // Initialize providers and signers for each chain
        for chain_id in chain_registry.get_all_chain_ids() {
            if let Some(config) = chain_registry.get_chain(chain_id) {
                info!("Setting up provider for chain {} ({})", config.name, chain_id);

                let provider = Provider::<Http>::try_from(&config.rpc_url)
                    .map_err(|e| anyhow!("Failed to create provider for chain {}: {}", chain_id, e))?;
                let provider = Arc::new(provider);

                let chain_wallet = wallet.clone().with_chain_id(chain_id);
                let signer = Arc::new(SignerMiddleware::new(
                    provider.as_ref().clone(),
                    chain_wallet,
                ));

                providers.insert(chain_id, provider);
                signers.insert(chain_id, signer);

                // Initialize status
                registration_status.write().await.insert(chain_id, RegistrationStatus::NotRegistered);
            }
        }

        Ok(Self {
            chain_registry,
            node_metadata: metadata,
            node_address,
            providers,
            signers,
            registration_status,
        })
    }

    /// Check FAB token balance for registration
    async fn check_fab_balance(&self, chain_id: u64) -> Result<U256> {
        let provider = self.providers.get(&chain_id)
            .ok_or_else(|| anyhow!("No provider for chain {}", chain_id))?;

        // FAB Token ABI for balanceOf
        abigen!(
            FabToken,
            r#"[
                function balanceOf(address owner) view returns (uint256)
            ]"#
        );

        let fab_token_address = Address::from_str(FAB_TOKEN_ADDRESS)?;
        let fab_token = FabToken::new(fab_token_address, provider.clone());

        let balance = fab_token.balance_of(self.node_address).call().await?;
        debug!("FAB balance for {} on chain {}: {}", self.node_address, chain_id, balance);

        Ok(balance)
    }

    /// Approve FAB tokens for staking
    async fn approve_fab_tokens(&self, chain_id: u64, registry_address: Address) -> Result<H256> {
        let signer = self.signers.get(&chain_id)
            .ok_or_else(|| anyhow!("No signer for chain {}", chain_id))?;

        // FAB Token ABI for approve
        abigen!(
            FabToken,
            r#"[
                function approve(address spender, uint256 amount) returns (bool)
            ]"#
        );

        let fab_token_address = Address::from_str(FAB_TOKEN_ADDRESS)?;
        let fab_token = FabToken::new(fab_token_address, signer.clone());

        let stake_amount = ethers::utils::parse_units(MIN_STAKE_AMOUNT, 18)?;
        let stake_amount_u256: U256 = match stake_amount {
            ethers::utils::ParseUnits::U256(val) => val,
            ethers::utils::ParseUnits::I256(_) => {
                return Err(anyhow!("Unexpected negative value for stake amount"));
            }
        };

        info!("Approving {} FAB tokens for registry {}", MIN_STAKE_AMOUNT, registry_address);
        let approve_call = fab_token.approve(registry_address, stake_amount_u256);
        let pending_tx = approve_call.send().await?;
        let tx_hash = pending_tx.tx_hash();

        // Wait for confirmation
        let receipt = pending_tx.await?;
        if receipt.is_none() {
            return Err(anyhow!("FAB approval transaction failed"));
        }

        Ok(tx_hash)
    }

    /// Register node on a specific chain
    pub async fn register_on_chain(&self, chain_id: u64) -> Result<H256> {
        let chain_config = self.chain_registry.get_chain(chain_id)
            .ok_or_else(|| anyhow!("Chain {} not supported", chain_id))?;

        info!("Starting registration on {} (chain {})", chain_config.name, chain_id);

        // Check FAB balance
        let balance = self.check_fab_balance(chain_id).await?;
        let required_stake = ethers::utils::parse_units(MIN_STAKE_AMOUNT, 18)?;
        let required_stake_u256: U256 = match required_stake {
            ethers::utils::ParseUnits::U256(val) => val,
            ethers::utils::ParseUnits::I256(_) => {
                return Err(anyhow!("Unexpected negative value for required stake"));
            }
        };

        if balance < required_stake_u256 {
            return Err(anyhow!(
                "Insufficient FAB balance. Required: {} FAB, Available: {} FAB",
                MIN_STAKE_AMOUNT,
                ethers::utils::format_units(balance, 18)?
            ));
        }

        // Get signer for this chain
        let signer = self.signers.get(&chain_id)
            .ok_or_else(|| anyhow!("No signer available for chain {}", chain_id))?;

        // Approve FAB tokens
        let registry_address = chain_config.contracts.node_registry;
        self.approve_fab_tokens(chain_id, registry_address).await?;

        // Build metadata JSON (following HOST_REGISTRATION_GUIDE.md format)
        let metadata_json = serde_json::json!({
            "name": self.node_metadata.name,
            "version": self.node_metadata.version,
            "hardware": {
                "gpu": "RTX 4090", // Default GPU for CLI registration
                "vram": 24,
                "cpu": "AMD Ryzen 9 5950X",
                "ram": 64
            },
            "capabilities": self.node_metadata.capabilities,
            "location": "us-west",
            "performance_tier": self.node_metadata.performance_tier,
            "maxConcurrentJobs": 5
        }).to_string();

        // Get approved model IDs
        // For MVP, using the two approved models from HOST_REGISTRATION_GUIDE.md
        let model_ids: Vec<[u8; 32]> = vec![
            // TinyVicuna-1B
            H256::from_str("0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced")?.into(),
            // TinyLlama-1.1B
            H256::from_str("0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca")?.into(),
        ];

        info!(
            "Calling registerNode on chain {} with metadata: {} and API URL: {}",
            chain_id, metadata_json, self.node_metadata.api_url
        );

        // Update status to pending
        let mut status = self.registration_status.write().await;

        // Use raw contract call approach to avoid lifetime issues
        use ethers::abi::{Function, Param, ParamType, Token};
        use ethers::types::Bytes;

        // Define the function ABI
        let register_function = Function {
            name: "registerNode".to_string(),
            inputs: vec![
                Param {
                    name: "metadata".to_string(),
                    kind: ParamType::String,
                    internal_type: None,
                },
                Param {
                    name: "apiUrl".to_string(),
                    kind: ParamType::String,
                    internal_type: None,
                },
                Param {
                    name: "modelIds".to_string(),
                    kind: ParamType::Array(Box::new(ParamType::FixedBytes(32))),
                    internal_type: None,
                },
            ],
            outputs: vec![],
            constant: None,
            state_mutability: ethers::abi::StateMutability::NonPayable,
        };

        // Encode the function call
        let tokens = vec![
            Token::String(metadata_json.clone()),
            Token::String(self.node_metadata.api_url.clone()),
            Token::Array(model_ids.iter().map(|id| Token::FixedBytes(id.to_vec())).collect()),
        ];

        let encoded = register_function.encode_input(&tokens)
            .map_err(|e| anyhow!("Failed to encode function call: {}", e))?;

        // Create transaction request
        let tx_request = ethers::types::TransactionRequest::new()
            .to(registry_address)
            .data(Bytes::from(encoded));

        // Send transaction
        let pending_tx = signer.send_transaction(tx_request, None).await
            .map_err(|e| anyhow!("Failed to send registration transaction: {}", e))?;

        let tx_hash = pending_tx.tx_hash();

        info!("Registration transaction sent on chain {}: {:?}", chain_id, tx_hash);

        status.insert(chain_id, RegistrationStatus::Pending { tx_hash });
        drop(status);

        // Wait for confirmation (but don't block indefinitely)
        // We'll spawn a task that just monitors the transaction hash
        let registration_status = self.registration_status.clone();
        let chain_id_copy = chain_id;
        let provider_clone = self.providers.get(&chain_id).cloned();

        if let Some(provider) = provider_clone {
            tokio::spawn(async move {
                // Wait a bit, then check the transaction
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

                match provider.get_transaction_receipt(tx_hash).await {
                    Ok(Some(receipt)) => {
                        info!("Registration confirmed on chain {} at block {}",
                              chain_id_copy, receipt.block_number.unwrap_or_default());

                        let mut status = registration_status.write().await;
                        status.insert(chain_id_copy, RegistrationStatus::Confirmed {
                            block_number: receipt.block_number.unwrap_or_default().as_u64(),
                        });
                    },
                    Ok(None) => {
                        debug!("Registration transaction on chain {} still pending", chain_id_copy);
                        // Transaction is still pending, leave status as Pending
                    },
                    Err(e) => {
                        error!("Failed to check registration receipt on chain {}: {}", chain_id_copy, e);
                        // Leave status as Pending, don't mark as failed just because we couldn't check
                    }
                }
            });
        }

        Ok(tx_hash)
    }

    /// Verify if node is registered on a specific chain
    pub async fn verify_registration_on_chain(&self, chain_id: u64) -> Result<bool> {
        let chain_config = self.chain_registry.get_chain(chain_id)
            .ok_or_else(|| anyhow!("Chain {} not supported", chain_id))?;

        let provider = self.providers.get(&chain_id)
            .ok_or_else(|| anyhow!("No provider for chain {}", chain_id))?;

        let node_registry = NodeRegistryWithModels::new(
            chain_config.contracts.node_registry,
            provider.clone(),
        );

        // Check if node is active
        let is_active = node_registry
            .is_active_node(self.node_address)
            .call()
            .await
            .unwrap_or(false);

        debug!("Node {} registration status on chain {}: {}",
               self.node_address, chain_id, is_active);

        Ok(is_active)
    }

    /// Get registration status for a specific chain
    pub async fn get_registration_status(&self, chain_id: u64) -> Result<RegistrationStatus> {
        let status = self.registration_status.read().await;
        status.get(&chain_id)
            .cloned()
            .ok_or_else(|| anyhow!("No registration status for chain {}", chain_id))
    }

    /// Get registration status for all chains
    pub async fn get_all_registration_status(&self) -> Result<HashMap<u64, RegistrationStatus>> {
        let status = self.registration_status.read().await;
        Ok(status.clone())
    }

    /// Register on all supported chains
    pub async fn register_on_all_chains(&self) -> Result<Vec<(u64, Result<H256>)>> {
        let mut results = Vec::new();

        for chain_id in self.chain_registry.get_all_chain_ids() {
            info!("Attempting registration on chain {}", chain_id);
            let result = self.register_on_chain(chain_id).await;
            results.push((chain_id, result));
        }

        Ok(results)
    }
}