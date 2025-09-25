use ethers::types::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

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
    pub job_marketplace: Address,
    pub node_registry: Address,
    pub payment_escrow: Address,
    pub host_earnings: Address,
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
                // Using addresses from .env.contracts
                job_marketplace: Address::from_str("0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304")
                    .expect("Invalid job marketplace address"),
                node_registry: Address::from_str("0x87516C13Ea2f99de598665e14cab64E191A0f8c4")
                    .expect("Invalid node registry address"),
                payment_escrow: Address::from_str("0xa4C5599Ea3617060ce86Ff0916409e1fb4a0d2c6")
                    .expect("Invalid payment escrow address"),
                host_earnings: Address::from_str("0xbFfCd6BAaCCa205d471bC52Bd37e1957B1A43d4a")
                    .expect("Invalid host earnings address"),
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
                    .ok()
                    .and_then(|addr| Address::from_str(&addr).ok())
                    .unwrap_or_else(Address::zero),
                node_registry: std::env::var("OPBNB_NODE_REGISTRY")
                    .ok()
                    .and_then(|addr| Address::from_str(&addr).ok())
                    .unwrap_or_else(Address::zero),
                payment_escrow: std::env::var("OPBNB_PAYMENT_ESCROW")
                    .ok()
                    .and_then(|addr| Address::from_str(&addr).ok())
                    .unwrap_or_else(Address::zero),
                host_earnings: std::env::var("OPBNB_HOST_EARNINGS")
                    .ok()
                    .and_then(|addr| Address::from_str(&addr).ok())
                    .unwrap_or_else(Address::zero),
            },
            confirmation_blocks: 15, // BNB chains typically need more confirmations
        }
    }
}

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

    pub fn default_chain(&self) -> u64 {
        self.default_chain
    }

    pub fn list_supported_chains(&self) -> Vec<u64> {
        self.chains.keys().cloned().collect()
    }

    pub fn is_chain_supported(&self, chain_id: u64) -> bool {
        self.chains.contains_key(&chain_id)
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

    #[test]
    fn test_chain_config_creation() {
        let config = ChainConfig::base_sepolia();
        assert_eq!(config.chain_id, 84532);
        assert_eq!(config.name, "Base Sepolia");
    }

    #[test]
    fn test_registry_creation() {
        let registry = ChainRegistry::new();
        assert!(registry.is_chain_supported(84532));
        assert!(registry.is_chain_supported(5611));
    }
}