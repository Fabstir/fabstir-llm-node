// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.4.1-s5-integration-tests-2025-11-15";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.4.1";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 4;

/// Patch version number
pub const VERSION_PATCH: u32 = 1;

/// Build date
pub const BUILD_DATE: &str = "2025-11-15";

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
    "chat-templates",
    "model-specific-formatting",
    "s5-vector-loading",
    "encrypted-vector-database-paths",
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    "Patch version bump (v8.4.0 -> v8.4.1) - S5 Integration Testing Complete",
    "TESTING: All 19 S5 vector loading integration tests passing (100%)",
    "TESTING: Phase 3 E2E tests (7/7), Phase 3.2 Encryption tests (4/4)",
    "TESTING: Phase 4 Error scenarios (6/6), Phase 4.5 Error handling (5/5)",
    "ENHANCEMENT: EnhancedS5Client now uses real S5 bridge HTTP API (not just mock storage)",
    "ENHANCEMENT: 7 new Enhanced S5.js bridge integration tests added",
    "ENHANCEMENT: Bridge unavailability testing verified (connection failure handling)",
    "DOCUMENTATION: Updated IMPLEMENTATION_S5_VECTOR_LOADING.md (production-ready status)",
    "DOCUMENTATION: Updated TESTING_ENHANCED_S5_INTEGRATION.md (19/19 tests passing)",
    "No breaking changes - fully backward compatible with v8.4.0",
    "All features from v8.4.0: S5 vector loading, encrypted vector_database paths, RAG support",
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
        assert_eq!(VERSION_MINOR, 4);
        assert_eq!(VERSION_PATCH, 1);
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
        assert!(FEATURES.contains(&"chat-templates"));
        assert!(FEATURES.contains(&"s5-vector-loading"));
        assert!(FEATURES.contains(&"encrypted-vector-database-paths"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("8.4.1"));
        assert!(version.contains("2025-11-15"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.4.1-s5-integration-tests-2025-11-15");
        assert_eq!(VERSION_NUMBER, "8.4.1");
        assert_eq!(BUILD_DATE, "2025-11-15");
    }
}
