// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v7.0.21-dispute-window-fix-2025-09-29";

/// Semantic version number
pub const VERSION_NUMBER: &str = "7.0.21";

/// Major version number
pub const VERSION_MAJOR: u32 = 7;

/// Minor version number
pub const VERSION_MINOR: u32 = 0;

/// Patch version number
pub const VERSION_PATCH: u32 = 21;

/// Build date
pub const BUILD_DATE: &str = "2025-09-29";

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
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    "chain_id required in all API requests",
    "WebSocket session_init requires chain_id",
    "Contract addresses are per-chain",
    "Node registration is per-chain",
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
        assert_eq!(VERSION_MAJOR, 7);
        assert_eq!(VERSION_PATCH, 21);
        assert!(FEATURES.contains(&"multi-chain"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("7.0.21"));
        assert!(version.contains("2025-09-29"));
    }
}