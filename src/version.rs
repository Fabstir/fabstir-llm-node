// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.1.2-proof-s5-storage-2025-10-15";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.1.2";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 1;

/// Patch version number
pub const VERSION_PATCH: u32 = 2;

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
    "Patch version bump (v8.1.1 -> v8.1.2) - Off-chain proof storage integration",
    "CONTRACT UPDATE: submitProofOfWork now requires new contract (0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E)",
    "Proof submission changed from full proof (221KB) to hash+CID (~300 bytes)",
    "Transaction size reduced 737x (221KB -> 300 bytes) to fit RPC limits",
    "Proofs now stored in S5 decentralized storage (off-chain)",
    "On-chain: SHA256 hash (32 bytes) + S5 CID (string) only",
    "Old contract (0xe169A4B57700080725f9553E3Cc69885fea13629) deprecated",
    "S5 integration pending - currently uses placeholder CID",
    "Full S5 upload will be enabled once s5 crate is available",
    "No SDK changes required - proof retrieval will use S5 CID from events",
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
        assert_eq!(VERSION_PATCH, 2);
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
        assert!(version.contains("8.1.2"));
        assert!(version.contains("2025-10-15"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.1.2-proof-s5-storage-2025-10-15");
        assert_eq!(VERSION_NUMBER, "8.1.2");
        assert_eq!(BUILD_DATE, "2025-10-15");
    }
}
