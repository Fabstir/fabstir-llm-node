// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.5.2-harmony-chat-template-2025-12-18";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.5.2";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 5;

/// Patch version number
pub const VERSION_PATCH: u32 = 2;

/// Build date
pub const BUILD_DATE: &str = "2025-12-18";

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
    "price-precision-1000",
    "uups-upgradeable",
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
    "configurable-batch-size",
    "llama-batch-size-env",
    "async-checkpoints",
    "non-blocking-proof-submission",
    "harmony-chat-template",
    "gpt-oss-20b-support",
    "utf8-content-sanitization",
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    "FIX: WebSocket inference now uses ChatTemplate system for proper prompt formatting",
    "FIX: GPT-OSS-20B Harmony format now correctly applied via WebSocket API",
    "FIX: UTF-8 content sanitization prevents corrupted output in WebSocket streams",
    "FIX: Enhanced diagnostic logging for invalid UTF-8 tokens",
    "ENV: MODEL_CHAT_TEMPLATE defaults to 'harmony' for GPT-OSS-20B compatibility",
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
        assert_eq!(VERSION_MINOR, 5);
        assert_eq!(VERSION_PATCH, 2);
        assert!(FEATURES.contains(&"multi-chain"));
        assert!(FEATURES.contains(&"dual-pricing"));
        assert!(FEATURES.contains(&"price-precision-1000"));
        assert!(FEATURES.contains(&"uups-upgradeable"));
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
        assert!(FEATURES.contains(&"configurable-batch-size"));
        assert!(FEATURES.contains(&"llama-batch-size-env"));
        assert!(FEATURES.contains(&"async-checkpoints"));
        assert!(FEATURES.contains(&"non-blocking-proof-submission"));
        assert!(FEATURES.contains(&"harmony-chat-template"));
        assert!(FEATURES.contains(&"gpt-oss-20b-support"));
        assert!(FEATURES.contains(&"utf8-content-sanitization"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("8.5.2"));
        assert!(version.contains("2025-12-18"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.5.2-harmony-chat-template-2025-12-18");
        assert_eq!(VERSION_NUMBER, "8.5.2");
        assert_eq!(BUILD_DATE, "2025-12-18");
    }
}
