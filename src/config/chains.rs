use ethers::types::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

// Re-export provider types
pub use super::provider::{MultiChainProvider, PoolStats, ProviderHealth, RotationStats};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainConfig {
    pub chain_id: u64,
    pub name: String,
    pub rpc_url: String,
    pub native_token: TokenInfo,
    pub contracts: ContractAddresses,
    pub confirmation_blocks: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_multiplier: Option<f64>,
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
    pub payment_escrow: Option<Address>, // Deprecated, kept for compatibility
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
                // Load from environment variables
                job_marketplace: std::env::var("CONTRACT_JOB_MARKETPLACE")
                    .ok()
                    .and_then(|addr| Address::from_str(&addr).ok())
                    .expect("CONTRACT_JOB_MARKETPLACE environment variable must be set"),
                node_registry: std::env::var("CONTRACT_NODE_REGISTRY")
                    .ok()
                    .and_then(|addr| Address::from_str(&addr).ok())
                    .expect("CONTRACT_NODE_REGISTRY environment variable must be set"),
                payment_escrow: std::env::var("CONTRACT_PAYMENT_ESCROW")
                    .ok()
                    .and_then(|addr| Address::from_str(&addr).ok()), // Deprecated, optional
                host_earnings: std::env::var("CONTRACT_HOST_EARNINGS")
                    .ok()
                    .and_then(|addr| Address::from_str(&addr).ok())
                    .expect("CONTRACT_HOST_EARNINGS environment variable must be set"),
            },
            confirmation_blocks: 3,
            gas_multiplier: std::env::var("BASE_GAS_MULTIPLIER")
                .ok()
                .and_then(|v| v.parse().ok()),
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
                    .and_then(|addr| Address::from_str(&addr).ok()), // Deprecated, optional
                host_earnings: std::env::var("OPBNB_HOST_EARNINGS")
                    .ok()
                    .and_then(|addr| Address::from_str(&addr).ok())
                    .unwrap_or_else(Address::zero),
            },
            confirmation_blocks: 15, // BNB chains typically need more confirmations
            gas_multiplier: std::env::var("OPBNB_GAS_MULTIPLIER")
                .ok()
                .and_then(|v| v.parse().ok()),
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

    pub fn get_all_chain_ids(&self) -> Vec<u64> {
        self.chains.keys().cloned().collect()
    }
}

impl Default for ChainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ChainConfigLoader {
    config_file: Option<String>,
}

impl ChainConfigLoader {
    pub fn new() -> Self {
        ChainConfigLoader { config_file: None }
    }

    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if !std::path::Path::new(path).exists() {
            return Err(format!("Config file not found: {}", path).into());
        }
        Ok(ChainConfigLoader {
            config_file: Some(path.to_string()),
        })
    }

    pub async fn load_base_sepolia(&self) -> Result<ChainConfig, Box<dyn std::error::Error>> {
        if let Some(ref file_path) = self.config_file {
            return self.load_from_file(file_path, "base_sepolia");
        }

        let chain_id = std::env::var("BASE_SEPOLIA_CHAIN_ID")
            .unwrap_or_else(|_| "84532".to_string())
            .parse::<u64>()?;

        let rpc_url = std::env::var("BASE_SEPOLIA_RPC_URL")
            .unwrap_or_else(|_| "https://sepolia.base.org".to_string());

        self.validate_rpc_url(&rpc_url)?;

        let confirmations = std::env::var("BASE_SEPOLIA_CONFIRMATIONS")
            .unwrap_or_else(|_| "3".to_string())
            .parse::<u64>()?;

        let gas_multiplier = std::env::var("BASE_GAS_MULTIPLIER")
            .ok()
            .and_then(|v| v.parse().ok());

        Ok(ChainConfig {
            chain_id,
            name: "Base Sepolia".to_string(),
            rpc_url,
            native_token: TokenInfo {
                symbol: "ETH".to_string(),
                decimals: 18,
            },
            contracts: self.load_base_sepolia_contracts(),
            confirmation_blocks: confirmations,
            gas_multiplier,
        })
    }

    pub async fn load_opbnb_testnet(&self) -> Result<ChainConfig, Box<dyn std::error::Error>> {
        if let Some(ref file_path) = self.config_file {
            return self.load_from_file(file_path, "opbnb_testnet");
        }

        let chain_id = std::env::var("OPBNB_TESTNET_CHAIN_ID")
            .unwrap_or_else(|_| "5611".to_string())
            .parse::<u64>()?;

        let rpc_url = std::env::var("OPBNB_TESTNET_RPC_URL")
            .unwrap_or_else(|_| "https://opbnb-testnet-rpc.bnbchain.org".to_string());

        self.validate_rpc_url(&rpc_url)?;

        let confirmations = std::env::var("OPBNB_TESTNET_CONFIRMATIONS")
            .unwrap_or_else(|_| "15".to_string())
            .parse::<u64>()?;

        let gas_multiplier = std::env::var("OPBNB_GAS_MULTIPLIER")
            .ok()
            .and_then(|v| v.parse().ok());

        Ok(ChainConfig {
            chain_id,
            name: "opBNB Testnet".to_string(),
            rpc_url,
            native_token: TokenInfo {
                symbol: "BNB".to_string(),
                decimals: 18,
            },
            contracts: self.load_opbnb_contracts(),
            confirmation_blocks: confirmations,
            gas_multiplier,
        })
    }

    fn load_base_sepolia_contracts(&self) -> ContractAddresses {
        ContractAddresses {
            job_marketplace: std::env::var("CONTRACT_JOB_MARKETPLACE")
                .ok()
                .and_then(|addr| Address::from_str(&addr).ok())
                .expect("CONTRACT_JOB_MARKETPLACE environment variable must be set"),
            node_registry: std::env::var("CONTRACT_NODE_REGISTRY")
                .ok()
                .and_then(|addr| Address::from_str(&addr).ok())
                .expect("CONTRACT_NODE_REGISTRY environment variable must be set"),
            payment_escrow: std::env::var("CONTRACT_PAYMENT_ESCROW")
                .ok()
                .and_then(|addr| Address::from_str(&addr).ok()), // Deprecated, optional
            host_earnings: std::env::var("CONTRACT_HOST_EARNINGS")
                .ok()
                .and_then(|addr| Address::from_str(&addr).ok())
                .expect("CONTRACT_HOST_EARNINGS environment variable must be set"),
        }
    }

    fn load_opbnb_contracts(&self) -> ContractAddresses {
        ContractAddresses {
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
                .and_then(|addr| Address::from_str(&addr).ok()), // Deprecated, optional
            host_earnings: std::env::var("OPBNB_HOST_EARNINGS")
                .ok()
                .and_then(|addr| Address::from_str(&addr).ok())
                .unwrap_or_else(Address::zero),
        }
    }

    pub fn validate_rpc_url(&self, url: &str) -> Result<(), Box<dyn std::error::Error>> {
        if url.is_empty() {
            return Err("RPC URL cannot be empty".into());
        }

        if !url.starts_with("http://")
            && !url.starts_with("https://")
            && !url.starts_with("ws://")
            && !url.starts_with("wss://")
        {
            return Err("RPC URL must start with http://, https://, ws://, or wss://".into());
        }

        Ok(())
    }

    fn load_from_file(
        &self,
        path: &str,
        section: &str,
    ) -> Result<ChainConfig, Box<dyn std::error::Error>> {
        let contents = std::fs::read_to_string(path)?;
        let config: toml::Value = toml::from_str(&contents)?;

        let chain_section = config
            .get(section)
            .ok_or_else(|| format!("Section '{}' not found in config file", section))?;

        let chain_id = chain_section
            .get("chain_id")
            .and_then(|v| v.as_integer())
            .ok_or("chain_id not found")? as u64;

        let rpc_url = chain_section
            .get("rpc_url")
            .and_then(|v| v.as_str())
            .ok_or("rpc_url not found")?
            .to_string();

        let confirmations = chain_section
            .get("confirmations")
            .and_then(|v| v.as_integer())
            .unwrap_or(if section == "base_sepolia" { 3 } else { 15 })
            as u64;

        let gas_multiplier = chain_section
            .get("gas_multiplier")
            .and_then(|v| v.as_float());

        if section == "base_sepolia" {
            Ok(ChainConfig {
                chain_id,
                name: "Base Sepolia".to_string(),
                rpc_url,
                native_token: TokenInfo {
                    symbol: "ETH".to_string(),
                    decimals: 18,
                },
                contracts: self.load_base_sepolia_contracts(),
                confirmation_blocks: confirmations,
                gas_multiplier,
            })
        } else {
            Ok(ChainConfig {
                chain_id,
                name: "opBNB Testnet".to_string(),
                rpc_url,
                native_token: TokenInfo {
                    symbol: "BNB".to_string(),
                    decimals: 18,
                },
                contracts: self.load_opbnb_contracts(),
                confirmation_blocks: confirmations,
                gas_multiplier,
            })
        }
    }

    pub async fn build_registry(&self) -> Result<ChainRegistry, Box<dyn std::error::Error>> {
        let mut chains = HashMap::new();

        let base_config = self.load_base_sepolia().await?;
        chains.insert(base_config.chain_id, base_config);

        let opbnb_config = self.load_opbnb_testnet().await?;
        chains.insert(opbnb_config.chain_id, opbnb_config);

        let default_chain = std::env::var("DEFAULT_CHAIN_ID")
            .unwrap_or_else(|_| "84532".to_string())
            .parse::<u64>()?;

        Ok(ChainRegistry {
            chains,
            default_chain,
        })
    }
}

impl Default for ChainConfigLoader {
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
