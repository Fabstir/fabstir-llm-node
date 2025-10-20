// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use fabstir_llm_node::blockchain::multi_chain_registrar::RegistrationStatus;
use std::env;
use std::process::Command;

// Helper function to run CLI commands
fn run_cli_command(args: Vec<&str>) -> Result<String> {
    let output = Command::new("cargo")
        .args(&["run", "--bin", "fabstir-cli", "--"])
        .args(&args)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        return Err(anyhow::anyhow!("Command failed: {}", stderr));
    }

    Ok(stdout.to_string())
}

#[tokio::test]
async fn test_register_single_chain_cli() -> Result<()> {
    println!("📝 Testing single chain registration via CLI...");

    // Set up test environment
    env::set_var(
        "NODE_PRIVATE_KEY",
        "0xe7855c0ea54ccca55126d40f97d90868b2a73bad0363e92ccdec0c4fbd6c0ce2",
    );

    // Test registration on Base Sepolia
    let args = vec![
        "register-node",
        "--chain",
        "84532",
        "--name",
        "Test Node CLI",
        "--api-url",
        "http://localhost:8080",
        "--models",
        "model1,model2",
        "--performance-tier",
        "standard",
        "--dry-run", // Don't actually register
    ];

    let output = run_cli_command(args)?;

    // Check output contains expected messages
    assert!(output.contains("Registration would be submitted") || output.contains("Dry run mode"));
    assert!(output.contains("Base Sepolia") || output.contains("84532"));

    println!("✅ Single chain CLI test passed");
    Ok(())
}

#[tokio::test]
async fn test_register_all_chains_cli() -> Result<()> {
    println!("📝 Testing all chains registration via CLI...");

    env::set_var(
        "NODE_PRIVATE_KEY",
        "0xe7855c0ea54ccca55126d40f97d90868b2a73bad0363e92ccdec0c4fbd6c0ce2",
    );

    let args = vec![
        "register-node",
        "--all-chains",
        "--name",
        "Test Node Multi",
        "--api-url",
        "http://localhost:8080",
        "--models",
        "model1,model2",
        "--performance-tier",
        "premium",
        "--dry-run",
    ];

    let output = run_cli_command(args)?;

    // Should mention multiple chains
    assert!(output.contains("all chains") || output.contains("All chains"));

    println!("✅ All chains CLI test passed");
    Ok(())
}

#[tokio::test]
async fn test_status_command() -> Result<()> {
    println!("📝 Testing registration status command...");

    let args = vec![
        "registration-status",
        "--chain",
        "84532",
        "--address",
        "0x4594F755F593B517Bb3194F4DeC20C48a3f04504",
    ];

    let output = run_cli_command(args)?;

    // Should show status information
    assert!(
        output.contains("Status") || output.contains("status") || output.contains("Not registered")
    );

    println!("✅ Status command test passed");
    Ok(())
}

#[tokio::test]
async fn test_update_registration() -> Result<()> {
    println!("📝 Testing registration update command...");

    env::set_var(
        "NODE_PRIVATE_KEY",
        "0xe7855c0ea54ccca55126d40f97d90868b2a73bad0363e92ccdec0c4fbd6c0ce2",
    );

    let args = vec![
        "update-registration",
        "--chain",
        "84532",
        "--api-url",
        "http://localhost:8081",
        "--models",
        "model3,model4",
        "--dry-run",
    ];

    let output = run_cli_command(args)?;

    // Should mention update
    assert!(
        output.contains("Update") || output.contains("update") || output.contains("Would update")
    );

    println!("✅ Update registration test passed");
    Ok(())
}

#[tokio::test]
async fn test_invalid_chain_cli() -> Result<()> {
    println!("📝 Testing invalid chain ID handling...");

    env::set_var(
        "NODE_PRIVATE_KEY",
        "0xe7855c0ea54ccca55126d40f97d90868b2a73bad0363e92ccdec0c4fbd6c0ce2",
    );

    let args = vec![
        "register-node",
        "--chain",
        "999999", // Invalid chain ID
        "--name",
        "Test Node",
        "--api-url",
        "http://localhost:8080",
        "--models",
        "model1",
        "--performance-tier",
        "standard",
        "--dry-run",
    ];

    // This should fail
    let result = run_cli_command(args);

    match result {
        Err(_) => {} // Expected error
        Ok(output) => assert!(
            output.contains("Invalid") || output.contains("not supported"),
            "Expected error message about invalid chain, got: {}",
            output
        ),
    }

    println!("✅ Invalid chain handling test passed");
    Ok(())
}

// Test help command
#[tokio::test]
async fn test_help_command() -> Result<()> {
    println!("📝 Testing help command...");

    let args = vec!["--help"];
    let output = run_cli_command(args)?;

    // Should show available commands
    assert!(output.contains("register-node") || output.contains("Commands"));
    assert!(output.contains("registration-status"));
    assert!(output.contains("update-registration"));

    println!("✅ Help command test passed");
    Ok(())
}

// Test missing required arguments
#[tokio::test]
async fn test_missing_arguments() -> Result<()> {
    println!("📝 Testing missing arguments handling...");

    let args = vec![
        "register-node",
        "--chain",
        "84532",
        // Missing required arguments like --name, --api-url
    ];

    let result = run_cli_command(args);

    // Should fail with error about missing arguments
    assert!(result.is_err());

    println!("✅ Missing arguments test passed");
    Ok(())
}
