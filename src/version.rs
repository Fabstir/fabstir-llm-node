// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v7.0.29-dual-pricing-support-2025-01-28";

/// Semantic version number
pub const VERSION_NUMBER: &str = "7.0.29";

/// Major version number
pub const VERSION_MAJOR: u32 = 7;

/// Minor version number
pub const VERSION_MINOR: u32 = 0;

/// Patch version number
pub const VERSION_PATCH: u32 = 29;

/// Build date
pub const BUILD_DATE: &str = "2025-01-28";

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
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    "New contract addresses (NodeRegistry: 0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6, JobMarketplace: 0xe169A4B57700080725f9553E3Cc69885fea13629)",
    "Dual pricing system - registerNode requires minPricePerTokenNative and minPricePerTokenStable",
    "getNodeFullInfo now returns 8 fields (added minPriceNative and minPriceStable)",
    "getNodePricing requires token address parameter",
    "Old contracts (0xC8dDD546e0993eEB4Df03591208aEDF6336342D7, 0x462050a4a551c4292586D9c1DE23e3158a9bF3B3) are deprecated",
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
        assert_eq!(VERSION_PATCH, 29);
        assert!(FEATURES.contains(&"multi-chain"));
        assert!(FEATURES.contains(&"dual-pricing"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("7.0.29"));
        assert!(version.contains("2025-01-28"));
    }
}