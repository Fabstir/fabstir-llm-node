pub mod registration;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Fabstir LLM Node CLI
#[derive(Parser, Debug)]
#[command(name = "fabstir-cli")]
#[command(version = "1.0.0")]
#[command(about = "CLI tools for Fabstir LLM Node management", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Register a node on one or more chains
    RegisterNode(registration::RegisterNodeArgs),

    /// Check registration status
    RegistrationStatus(registration::StatusArgs),

    /// Update existing registration
    UpdateRegistration(registration::UpdateArgs),
}

/// Execute CLI command
pub async fn execute(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::RegisterNode(args) => registration::register_node(args).await,
        Commands::RegistrationStatus(args) => registration::check_status(args).await,
        Commands::UpdateRegistration(args) => registration::update_registration(args).await,
    }
}
