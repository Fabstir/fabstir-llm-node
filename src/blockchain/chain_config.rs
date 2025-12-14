// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use ethers::types::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

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
    pub symbol: String,
    pub decimals: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractAddresses {
    pub job_marketplace: String,
    pub node_registry: String,
    pub proof_system: String,
    pub host_earnings: String,
    pub model_registry: String,
    pub usdc_token: String,
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
                // Updated December 14, 2025 for UUPS Upgradeable proxy contracts (v8.5.0)
                job_marketplace: "0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D".to_string(),
                node_registry: "0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22".to_string(),
                proof_system: "0x5afB91977e69Cc5003288849059bc62d47E7deeb".to_string(),
                host_earnings: "0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0".to_string(),
                model_registry: "0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2".to_string(),
                usdc_token: "0x036CbD53842c5426634e7929541eC2318f3dCF7e".to_string(),
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
                // opBNB contracts - OPTIONAL, will be skipped if not deployed (zero addresses)
                job_marketplace: std::env::var("OPBNB_JOB_MARKETPLACE")
                    .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string()),
                node_registry: std::env::var("OPBNB_NODE_REGISTRY")
                    .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string()),
                proof_system: std::env::var("OPBNB_PROOF_SYSTEM")
                    .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string()),
                host_earnings: std::env::var("OPBNB_HOST_EARNINGS")
                    .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string()),
                model_registry: std::env::var("OPBNB_MODEL_REGISTRY")
                    .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string()),
                usdc_token: std::env::var("OPBNB_USDC_TOKEN")
                    .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string()),
            },
            confirmation_blocks: 15, // BNB chains typically need more confirmations
        }
    }

    pub fn is_deployed(&self) -> bool {
        // Check if contracts are deployed (not zero addresses)
        self.contracts.job_marketplace != "0x0000000000000000000000000000000000000000"
            && self.contracts.node_registry != "0x0000000000000000000000000000000000000000"
    }

    pub fn get_job_marketplace_address(&self) -> Result<Address, Box<dyn std::error::Error>> {
        Ok(Address::from_str(&self.contracts.job_marketplace)?)
    }

    pub fn get_node_registry_address(&self) -> Result<Address, Box<dyn std::error::Error>> {
        Ok(Address::from_str(&self.contracts.node_registry)?)
    }
}

pub struct ChainRegistry {
    chains: HashMap<u64, ChainConfig>,
    default_chain: u64,
}

impl ChainRegistry {
    pub fn new() -> Self {
        let mut chains = HashMap::new();
        let base_sepolia = ChainConfig::base_sepolia();
        let opbnb_testnet = ChainConfig::opbnb_testnet();

        chains.insert(base_sepolia.chain_id, base_sepolia);

        // Only include opBNB if contracts are deployed
        if opbnb_testnet.is_deployed() {
            chains.insert(opbnb_testnet.chain_id, opbnb_testnet);
        }

        ChainRegistry {
            chains,
            default_chain: 84532, // Base Sepolia as default
        }
    }

    pub fn get_chain(&self, chain_id: u64) -> Option<&ChainConfig> {
        self.chains.get(&chain_id)
    }

    pub fn get_default_chain(&self) -> &ChainConfig {
        self.chains
            .get(&self.default_chain)
            .expect("Default chain should always exist")
    }

    pub fn get_all_chains(&self) -> Vec<&ChainConfig> {
        self.chains.values().collect()
    }

    pub fn get_default_chain_id(&self) -> u64 {
        self.default_chain
    }
}

impl Default for ChainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===========================================
    // Contract Address Tests (Sub-phase 1.2)
    // Updated December 14, 2025 for UUPS Upgradeable proxy contracts
    // ===========================================

    /// New JobMarketplace contract address (UUPS Proxy)
    const NEW_JOB_MARKETPLACE: &str = "0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D";
    /// New NodeRegistry contract address (UUPS Proxy)
    const NEW_NODE_REGISTRY: &str = "0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22";

    #[test]
    fn test_job_marketplace_address_updated() {
        let config = ChainConfig::base_sepolia();
        assert_eq!(
            config.contracts.job_marketplace, NEW_JOB_MARKETPLACE,
            "JobMarketplace should be updated to new PRICE_PRECISION contract"
        );
    }

    #[test]
    fn test_node_registry_address_updated() {
        let config = ChainConfig::base_sepolia();
        assert_eq!(
            config.contracts.node_registry, NEW_NODE_REGISTRY,
            "NodeRegistry should be updated to new PRICE_PRECISION contract"
        );
    }

    #[test]
    fn test_job_marketplace_address_valid() {
        let config = ChainConfig::base_sepolia();
        let addr = config.get_job_marketplace_address();
        assert!(addr.is_ok(), "JobMarketplace address should be valid");
        assert_eq!(
            addr.unwrap(),
            Address::from_str(NEW_JOB_MARKETPLACE).unwrap()
        );
    }

    #[test]
    fn test_node_registry_address_valid() {
        let config = ChainConfig::base_sepolia();
        let addr = config.get_node_registry_address();
        assert!(addr.is_ok(), "NodeRegistry address should be valid");
        assert_eq!(
            addr.unwrap(),
            Address::from_str(NEW_NODE_REGISTRY).unwrap()
        );
    }

    // ===========================================
    // Existing Tests (kept for compatibility)
    // ===========================================

    #[test]
    fn test_chain_config_base_sepolia() {
        let config = ChainConfig::base_sepolia();
        assert_eq!(config.chain_id, 84532);
        assert_eq!(config.name, "Base Sepolia");
        assert_eq!(config.native_token.symbol, "ETH");
        assert!(config.is_deployed());
    }

    #[test]
    fn test_chain_registry() {
        let registry = ChainRegistry::new();

        // Base Sepolia should always be available
        let base = registry.get_chain(84532);
        assert!(base.is_some());
        assert_eq!(base.unwrap().name, "Base Sepolia");

        // Default chain should be Base Sepolia
        assert_eq!(registry.get_default_chain_id(), 84532);
    }

    #[test]
    fn test_other_contracts_updated() {
        let config = ChainConfig::base_sepolia();
        // All contracts updated to UUPS proxies (December 14, 2025)
        assert_eq!(
            config.contracts.proof_system,
            "0x5afB91977e69Cc5003288849059bc62d47E7deeb"
        );
        assert_eq!(
            config.contracts.host_earnings,
            "0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0"
        );
        assert_eq!(
            config.contracts.model_registry,
            "0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2"
        );
        // USDC token address unchanged
        assert_eq!(
            config.contracts.usdc_token,
            "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
        );
    }
}
