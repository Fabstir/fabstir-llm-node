// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.3.13-harmony-channels-2025-11-08";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.3.13";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 3;

/// Patch version number
pub const VERSION_PATCH: u32 = 13;

/// Build date
pub const BUILD_DATE: &str = "2025-11-08";

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
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    "Patch version bump (v8.3.12 -> v8.3.13) - CORRECT Harmony Format Implementation",
    "CRITICAL FIX: Implemented PROPER Harmony format per official OpenAI spec",
    "v8.3.12 was WRONG - used DEFAULT format instead of required Harmony format",
    "Official spec: https://cookbook.openai.com/articles/openai-harmony",
    "Added <|channel|>final<|message|> for user-facing responses",
    "Added 'Reasoning: medium' and channel list to system message",
    "Fixed stop token: now uses <|return|> (200002) instead of <|end|> (200007)",
    "Harmony format is REQUIRED per OpenAI: 'will not work correctly otherwise'",
    "All features from v8.3.11: Risc0 proofs, multi-chain, encryption",
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
        assert_eq!(VERSION_PATCH, 13);
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
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("8.3.13"));
        assert!(version.contains("2025-11-08"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.3.13-harmony-channels-2025-11-08");
        assert_eq!(VERSION_NUMBER, "8.3.13");
        assert_eq!(BUILD_DATE, "2025-11-08");
    }
}
