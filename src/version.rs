// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.1.1-gpu-stark-proofs-2025-10-14";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.1.1";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 1;

/// Patch version number
pub const VERSION_PATCH: u32 = 1;

/// Build date
pub const BUILD_DATE: &str = "2025-10-14";

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
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    "Patch version bump (v8.1.0 -> v8.1.1) - Critical proof generation fix",
    "FIXED: Checkpoint submissions now generate real Risc0 STARK proofs (was submitting JSON metadata)",
    "Real Risc0 zkVM proofs replace mock proofs (200 bytes -> ~221KB proof size)",
    "CUDA GPU acceleration enabled for proof generation (4.4s CPU -> 0.5-2s GPU)",
    "Proof format changed from mock EZKL to Risc0 STARK receipts",
    "First proof generation requires JIT kernel compilation (2-5 min one-time delay)",
    "Binary size increased due to CUDA kernels (~100-200MB vs ~50MB mock)",
    "Blockchain transactions now contain real cryptographic proofs in submitProofOfWork",
    "No SDK changes required - proof generation is internal to node",
    "Backward compatible: nodes without GPU fall back to CPU (still production STARK proofs)",
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
        assert_eq!(VERSION_PATCH, 1);
        assert!(FEATURES.contains(&"multi-chain"));
        assert!(FEATURES.contains(&"dual-pricing"));
        assert!(FEATURES.contains(&"end-to-end-encryption"));
        assert!(FEATURES.contains(&"encrypted-sessions"));
        assert!(FEATURES.contains(&"gpu-stark-proofs"));
        assert!(FEATURES.contains(&"risc0-zkvm"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("8.1.1"));
        assert!(version.contains("2025-10-14"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.1.1-gpu-stark-proofs-2025-10-14");
        assert_eq!(VERSION_NUMBER, "8.1.1");
        assert_eq!(BUILD_DATE, "2025-10-14");
    }
}
