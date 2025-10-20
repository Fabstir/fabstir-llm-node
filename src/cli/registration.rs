// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use clap::Args;
use ethers::types::Address;
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::blockchain::multi_chain_registrar::{
    MultiChainRegistrar, NodeMetadata, RegistrationStatus,
};
use crate::config::chains::ChainRegistry;

/// Arguments for register-node command
#[derive(Args, Debug)]
pub struct RegisterNodeArgs {
    /// Chain ID to register on (e.g., 84532 for Base Sepolia)
    #[arg(long, conflicts_with = "all_chains")]
    pub chain: Option<u64>,

    /// Register on all available chains
    #[arg(long, conflicts_with = "chain")]
    pub all_chains: bool,

    /// Node name
    #[arg(long)]
    pub name: String,

    /// API URL for the node
    #[arg(long)]
    pub api_url: String,

    /// Comma-separated list of model IDs
    #[arg(long, value_delimiter = ',')]
    pub models: Vec<String>,

    /// Performance tier (standard/premium)
    #[arg(long, default_value = "standard")]
    pub performance_tier: String,

    /// Private key (can also be set via NODE_PRIVATE_KEY env var)
    #[arg(long, env = "NODE_PRIVATE_KEY")]
    pub private_key: Option<String>,

    /// Dry run mode - don't actually submit transactions
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for registration-status command
#[derive(Args, Debug)]
pub struct StatusArgs {
    /// Chain ID to check status on
    #[arg(long, conflicts_with = "all_chains")]
    pub chain: Option<u64>,

    /// Check status on all chains
    #[arg(long, conflicts_with = "chain")]
    pub all_chains: bool,

    /// Node address to check (defaults to current node)
    #[arg(long)]
    pub address: Option<String>,

    /// Private key (for checking own registration)
    #[arg(long, env = "NODE_PRIVATE_KEY")]
    pub private_key: Option<String>,
}

/// Arguments for update-registration command
#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Chain ID to update registration on
    #[arg(long)]
    pub chain: u64,

    /// New API URL
    #[arg(long)]
    pub api_url: Option<String>,

    /// New comma-separated list of model IDs
    #[arg(long, value_delimiter = ',')]
    pub models: Option<Vec<String>>,

    /// Private key
    #[arg(long, env = "NODE_PRIVATE_KEY")]
    pub private_key: Option<String>,

    /// Dry run mode
    #[arg(long)]
    pub dry_run: bool,
}

/// Register a node on specified chains
pub async fn register_node(args: RegisterNodeArgs) -> Result<()> {
    // Load environment variables from .env file if it exists
    dotenv::dotenv().ok();

    // Get private key
    let private_key = args
        .private_key
        .or_else(|| env::var("NODE_PRIVATE_KEY").ok())
        .ok_or_else(|| {
            anyhow!("Private key required. Use --private-key or set NODE_PRIVATE_KEY env var")
        })?;

    // Determine which chains to register on
    let chain_ids = if args.all_chains {
        println!("🌐 Registering on all available chains...");
        vec![84532] // Base Sepolia (add more when available)
    } else if let Some(chain_id) = args.chain {
        println!("🔗 Registering on chain {}...", chain_id);
        vec![chain_id]
    } else {
        return Err(anyhow!("Must specify either --chain or --all-chains"));
    };

    // Validate chain IDs
    for chain_id in &chain_ids {
        if *chain_id != 84532 && *chain_id != 5611 {
            return Err(anyhow!("Chain {} not supported. Supported chains: 84532 (Base Sepolia), 5611 (opBNB Testnet)", chain_id));
        }
    }

    // Create node metadata
    let metadata = NodeMetadata {
        name: args.name.clone(),
        version: "1.0.0".to_string(),
        api_url: args.api_url.clone(),
        capabilities: vec!["inference".to_string()],
        performance_tier: args.performance_tier.clone(),
    };

    println!("\n📋 Registration Details:");
    println!("  Name:             {}", metadata.name);
    println!("  API URL:          {}", metadata.api_url);
    println!("  Models:           {:?}", args.models);
    println!("  Performance Tier: {}", metadata.performance_tier);
    println!("  Chains:           {:?}", chain_ids);

    if args.dry_run {
        println!("\n🔍 DRY RUN MODE - No transactions will be submitted");
        println!(
            "✅ Registration would be submitted to chains: {:?}",
            chain_ids
        );
        return Ok(());
    }

    // Create registrar
    let chain_registry = Arc::new(ChainRegistry::new());
    let registrar = MultiChainRegistrar::new(chain_registry, &private_key, metadata).await?;

    // Register on each chain
    for chain_id in chain_ids {
        println!("\n🚀 Registering on chain {}...", chain_id);

        match registrar.register_on_chain(chain_id).await {
            Ok(tx_hash) => {
                println!("✅ Registration transaction submitted!");
                println!("   Transaction hash: {:?}", tx_hash);
                println!("   View on explorer:");
                if chain_id == 84532 {
                    println!("   https://sepolia.basescan.org/tx/{:?}", tx_hash);
                }
            }
            Err(e) => {
                println!("❌ Registration failed: {}", e);

                // Provide helpful error messages
                if e.to_string().contains("insufficient") || e.to_string().contains("balance") {
                    println!("   💡 You need 1000 FAB tokens for staking");
                    println!(
                        "   💡 Check your balance and ensure you have enough FAB and ETH for gas"
                    );
                } else if e.to_string().contains("already registered") {
                    println!("   💡 Node is already registered on this chain");
                }
            }
        }
    }

    Ok(())
}

/// Check registration status
pub async fn check_status(args: StatusArgs) -> Result<()> {
    dotenv::dotenv().ok();

    // Determine which chains to check
    let chain_ids = if args.all_chains {
        println!("🔍 Checking status on all chains...");
        vec![84532, 5611] // All supported chains
    } else if let Some(chain_id) = args.chain {
        vec![chain_id]
    } else {
        vec![84532] // Default to Base Sepolia
    };

    // If checking own status, use private key to create registrar
    if let Some(private_key) = args
        .private_key
        .or_else(|| env::var("NODE_PRIVATE_KEY").ok())
    {
        let chain_registry = Arc::new(ChainRegistry::new());
        let metadata = NodeMetadata {
            name: "Status Check".to_string(),
            version: "1.0.0".to_string(),
            api_url: "http://localhost".to_string(),
            capabilities: vec![],
            performance_tier: "standard".to_string(),
        };

        let registrar = MultiChainRegistrar::new(chain_registry, &private_key, metadata).await?;

        println!("\n📊 Registration Status:");
        for chain_id in chain_ids {
            print!("  Chain {}: ", chain_id);

            match registrar.get_registration_status(chain_id).await? {
                RegistrationStatus::NotRegistered => {
                    println!("❌ Not registered");
                }
                RegistrationStatus::Pending { tx_hash } => {
                    println!("⏳ Pending (tx: {:?})", tx_hash);
                }
                RegistrationStatus::Confirmed { block_number } => {
                    println!("✅ Registered (block: {})", block_number);
                }
                RegistrationStatus::Failed { ref error } => {
                    println!("❌ Failed: {}", error);
                }
            }

            // Also check if registered on-chain
            if registrar.verify_registration_on_chain(chain_id).await? {
                println!("     ✅ Verified on-chain");
            }
        }
    } else if let Some(address_str) = args.address {
        // Check status for specific address (read-only)
        let _address =
            Address::from_str(&address_str).map_err(|_| anyhow!("Invalid address format"))?;

        println!("\n📊 Registration Status for {}:", address_str);
        println!("  (Read-only check not yet implemented for external addresses)");
    } else {
        return Err(anyhow!(
            "Must provide either --private-key, --address, or NODE_PRIVATE_KEY env var"
        ));
    }

    Ok(())
}

/// Update existing registration
pub async fn update_registration(args: UpdateArgs) -> Result<()> {
    dotenv::dotenv().ok();

    let private_key = args
        .private_key
        .or_else(|| env::var("NODE_PRIVATE_KEY").ok())
        .ok_or_else(|| anyhow!("Private key required"))?;

    println!("🔄 Updating registration on chain {}...", args.chain);

    if let Some(ref api_url) = args.api_url {
        println!("  New API URL: {}", api_url);
    }
    if let Some(ref models) = args.models {
        println!("  New Models: {:?}", models);
    }

    if args.dry_run {
        println!("\n🔍 DRY RUN MODE - No transactions will be submitted");
        println!("✅ Would update registration on chain {}", args.chain);
        return Ok(());
    }

    // Create registrar for update
    let chain_registry = Arc::new(ChainRegistry::new());
    let metadata = NodeMetadata {
        name: "Updated Node".to_string(),
        version: "1.0.0".to_string(),
        api_url: args
            .api_url
            .unwrap_or_else(|| "http://localhost:8080".to_string()),
        capabilities: vec!["inference".to_string()],
        performance_tier: "standard".to_string(),
    };

    let registrar = MultiChainRegistrar::new(chain_registry, &private_key, metadata).await?;

    // Note: Actual update logic would need to be implemented in MultiChainRegistrar
    // For now, this is a placeholder
    println!("⚠️  Update functionality not yet fully implemented");
    println!("    Would update registration on chain {}", args.chain);

    Ok(())
}
