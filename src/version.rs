// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.1.6-websocket-error-logging-2025-10-15";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.1.6";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 1;

/// Patch version number
pub const VERSION_PATCH: u32 = 6;

/// Build date
pub const BUILD_DATE: &str = "2025-10-15";

/// Supported features in this version
pub const FEATURES: &[&str] = &[
    "multi-chain",
    "base-sepolia",
    "opbnb-testnet",
    "chain-aware-sessions",
    "auto-settlement",
    "websocket-compression",
    "rate-limiting",
    "job-auth",
    "dual-pricing",
    "native-stable-pricing",
    "end-to-end-encryption",
    "ecdh-key-exchange",
    "xchacha20-poly1305",
    "encrypted-sessions",
    "session-key-management",
    "ecdsa-authentication",
    "perfect-forward-secrecy",
    "replay-protection",
    "gpu-stark-proofs",
    "risc0-zkvm",
    "cuda-acceleration",
    "zero-knowledge-proofs",
    "s5-proof-storage",
    "off-chain-proofs",
    "proof-hash-cid",
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    "Patch version bump (v8.1.5 -> v8.1.6) - WebSocket error logging added",
    "DIAGNOSTIC FIX: Added error logging to encrypted message WebSocket sends",
    "Helps diagnose why encrypted LLM responses not reaching client",
    "Logs success/failure for each encrypted_chunk sent via WebSocket",
    "Logs success/failure for final encrypted_response message",
    "No functional changes - purely diagnostic logging enhancement",
    "No contract changes - fully compatible with v8.1.2+",
    "Includes all v8.1.4 fixes (real Risc0 STARK proofs, encryption)",
];

/// Get formatted version string for logging
pub fn get_version_string() -> String {
    format!(
        "Fabstir LLM Node {} ({})",
        VERSION_NUMBER,
        BUILD_DATE
    )
}

/// Get full version info for API responses
pub fn get_version_info() -> serde_json::Value {
    serde_json::json!({
        "version": VERSION_NUMBER,
        "build": VERSION,
        "date": BUILD_DATE,
        "features": FEATURES,
        "chains": SUPPORTED_CHAINS,
        "breaking_changes": BREAKING_CHANGES,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constants() {
        assert_eq!(VERSION_MAJOR, 8);
        assert_eq!(VERSION_MINOR, 1);
        assert_eq!(VERSION_PATCH, 6);
        assert!(FEATURES.contains(&"multi-chain"));
        assert!(FEATURES.contains(&"dual-pricing"));
        assert!(FEATURES.contains(&"end-to-end-encryption"));
        assert!(FEATURES.contains(&"encrypted-sessions"));
        assert!(FEATURES.contains(&"gpu-stark-proofs"));
        assert!(FEATURES.contains(&"risc0-zkvm"));
        assert!(FEATURES.contains(&"s5-proof-storage"));
        assert!(FEATURES.contains(&"off-chain-proofs"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("8.1.6"));
        assert!(version.contains("2025-10-15"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.1.6-websocket-error-logging-2025-10-15");
        assert_eq!(VERSION_NUMBER, "8.1.6");
        assert_eq!(BUILD_DATE, "2025-10-15");
    }
}
