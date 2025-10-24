// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Node Private Key Extraction (Phase 6, Sub-phase 6.1)
//!
//! This module handles extraction of the node's private key from environment variables.
//! The private key is used for ECDH key exchange during encrypted session initialization.
//!
//! ## Security Considerations
//!
//! - Private key is read from `HOST_PRIVATE_KEY` environment variable
//! - Must be 32-byte hex string with "0x" prefix
//! - Key is NEVER logged or persisted
//! - Key validation ensures correct format before use
//!
//! ## Usage
//!
//! ```no_run
//! use fabstir_llm_node::crypto::extract_node_private_key;
//!
//! // Extract private key from HOST_PRIVATE_KEY environment variable
//! match extract_node_private_key() {
//!     Ok(key_bytes) => {
//!         // Use key_bytes for ECDH operations
//!         println!("✅ Private key loaded successfully");
//!     }
//!     Err(e) => {
//!         eprintln!("❌ Failed to load private key: {}", e);
//!     }
//! }
//! ```

use anyhow::{anyhow, Result};
use std::env;
use tracing::info;

/// Extract node's private key from HOST_PRIVATE_KEY environment variable
///
/// This function reads the `HOST_PRIVATE_KEY` environment variable and extracts
/// the raw 32-byte private key. The key must be a hex string with "0x" prefix.
///
/// # Security
///
/// - The actual key is NEVER logged
/// - Only logs whether key was successfully loaded or not
/// - Key must be exactly 32 bytes (64 hex characters + "0x" prefix)
///
/// # Returns
///
/// - `Ok([u8; 32])` - Raw 32-byte private key
/// - `Err` - If key is missing, invalid format, or wrong length
///
/// # Errors
///
/// - `HOST_PRIVATE_KEY` environment variable not set
/// - Key doesn't start with "0x" prefix
/// - Key is not valid hex
/// - Key is not exactly 32 bytes
///
/// # Example
///
/// ```no_run
/// use fabstir_llm_node::crypto::extract_node_private_key;
///
/// let key = extract_node_private_key()?;
/// println!("Key length: {} bytes", key.len());
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn extract_node_private_key() -> Result<[u8; 32]> {
    // Read HOST_PRIVATE_KEY from environment
    let key_str = env::var("HOST_PRIVATE_KEY")
        .map_err(|_| anyhow!("HOST_PRIVATE_KEY environment variable not set"))?;

    // Trim whitespace
    let key_str = key_str.trim();

    // Validate that key is not empty
    if key_str.is_empty() {
        return Err(anyhow!("HOST_PRIVATE_KEY is empty"));
    }

    // Validate 0x prefix
    if !key_str.starts_with("0x") {
        return Err(anyhow!(
            "HOST_PRIVATE_KEY must start with '0x' prefix (Ethereum format)"
        ));
    }

    // Strip 0x prefix
    let hex_str = &key_str[2..];

    // Validate length (should be 64 hex chars = 32 bytes)
    if hex_str.len() != 64 {
        return Err(anyhow!(
            "HOST_PRIVATE_KEY must be exactly 64 hex characters (32 bytes), got {} characters",
            hex_str.len()
        ));
    }

    // Decode hex to bytes
    let key_bytes = hex::decode(hex_str)
        .map_err(|e| anyhow!("HOST_PRIVATE_KEY contains invalid hex characters: {}", e))?;

    // Validate decoded length (should be exactly 32 bytes)
    if key_bytes.len() != 32 {
        return Err(anyhow!(
            "Decoded key must be exactly 32 bytes, got {} bytes",
            key_bytes.len()
        ));
    }

    // Convert Vec<u8> to [u8; 32]
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&key_bytes);

    // Log success WITHOUT logging the actual key
    info!("✅ Node private key loaded successfully (32 bytes)");

    Ok(key_array)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_key_extraction() {
        // Test with valid 32-byte key
        env::set_var(
            "HOST_PRIVATE_KEY",
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        );

        let result = extract_node_private_key();
        assert!(result.is_ok());

        let key = result.unwrap();
        assert_eq!(key.len(), 32);

        env::remove_var("HOST_PRIVATE_KEY");
    }

    #[test]
    fn test_key_without_prefix_rejected() {
        // Test that keys without 0x prefix are rejected
        env::set_var(
            "HOST_PRIVATE_KEY",
            "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        );

        let result = extract_node_private_key();
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("0x"));

        env::remove_var("HOST_PRIVATE_KEY");
    }

    #[test]
    fn test_short_key_rejected() {
        // Test that short keys are rejected
        env::set_var("HOST_PRIVATE_KEY", "0x1234");

        let result = extract_node_private_key();
        assert!(result.is_err());

        env::remove_var("HOST_PRIVATE_KEY");
    }
}
