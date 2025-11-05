// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.3.6-rag-response-type-field-complete-2025-11-05";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.3.6";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 3;

/// Patch version number
pub const VERSION_PATCH: u32 = 6;

/// Build date
pub const BUILD_DATE: &str = "2025-11-05";

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
    "host-side-rag",
    "session-vector-storage",
    "384d-embeddings",
    "cosine-similarity-search",
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    "Patch version bump (v8.3.4 -> v8.3.6) - RAG Response Type Field Complete",
    "FIXED: Added missing 'type' field to uploadVectorsResponse and searchVectorsResponse",
    "uploadVectorsResponse now includes type: 'uploadVectorsResponse'",
    "searchVectorsResponse now includes type: 'searchVectorsResponse'",
    "Enables proper SDK message routing and correlation",
    "Fixes SDK error: 'handleMessage called for type: undefined'",
    "/v1/version endpoint confirmed working (was already in code)",
    "All v8.3.x features intact (Host-Side RAG + Session Persistence)",
    "No contract changes - fully compatible with v8.2.0+",
];

/// Get formatted version string for logging
pub fn get_version_string() -> String {
    format!("Fabstir LLM Node {} ({})", VERSION_NUMBER, BUILD_DATE)
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
        assert_eq!(VERSION_MINOR, 3);
        assert_eq!(VERSION_PATCH, 6);
        assert!(FEATURES.contains(&"multi-chain"));
        assert!(FEATURES.contains(&"dual-pricing"));
        assert!(FEATURES.contains(&"end-to-end-encryption"));
        assert!(FEATURES.contains(&"encrypted-sessions"));
        assert!(FEATURES.contains(&"gpu-stark-proofs"));
        assert!(FEATURES.contains(&"risc0-zkvm"));
        assert!(FEATURES.contains(&"s5-proof-storage"));
        assert!(FEATURES.contains(&"off-chain-proofs"));
        assert!(FEATURES.contains(&"host-side-rag"));
        assert!(FEATURES.contains(&"session-vector-storage"));
        assert!(FEATURES.contains(&"384d-embeddings"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("8.3.6"));
        assert!(version.contains("2025-11-05"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.3.6-rag-response-type-field-complete-2025-11-05");
        assert_eq!(VERSION_NUMBER, "8.3.6");
        assert_eq!(BUILD_DATE, "2025-11-05");
    }
}
