// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.0.0-encryption-support-2025-10-13";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.0.0";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 0;

/// Patch version number
pub const VERSION_PATCH: u32 = 0;

/// Build date
pub const BUILD_DATE: &str = "2025-10-13";

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
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    "Major version bump (v7 -> v8) due to encryption feature addition",
    "New encrypted WebSocket message types: encrypted_session_init, encrypted_message, encrypted_chunk, encrypted_response",
    "HOST_PRIVATE_KEY environment variable required for encryption support (optional for plaintext-only mode)",
    "Session key management with TTL-based expiration",
    "New encryption error codes: ENCRYPTION_NOT_SUPPORTED, DECRYPTION_FAILED, INVALID_SIGNATURE, SESSION_KEY_NOT_FOUND",
    "SDK Phase 6.2+ required for encrypted sessions",
    "Plaintext sessions still supported for backward compatibility (deprecated)",
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
        assert_eq!(VERSION_MINOR, 0);
        assert_eq!(VERSION_PATCH, 0);
        assert!(FEATURES.contains(&"multi-chain"));
        assert!(FEATURES.contains(&"dual-pricing"));
        assert!(FEATURES.contains(&"end-to-end-encryption"));
        assert!(FEATURES.contains(&"encrypted-sessions"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("8.0.0"));
        assert!(version.contains("2025-10-13"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.0.0-encryption-support-2025-10-13");
        assert_eq!(VERSION_NUMBER, "8.0.0");
        assert_eq!(BUILD_DATE, "2025-10-13");
    }
}
