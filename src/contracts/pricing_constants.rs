// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/// Dual Pricing System Constants with PRICE_PRECISION Support
///
/// These constants define the pricing ranges for native tokens (ETH/BNB) and
/// stablecoins (USDC) as specified in the contract deployment from December 9, 2025.
///
/// **BREAKING CHANGE (December 2025):** Prices are now stored with a 1000x multiplier
/// (PRICE_PRECISION=1000) to support sub-$1/million token pricing for budget AI models.
///
/// Payment calculation formulas:
/// - max_tokens = (deposit * PRICE_PRECISION) / price_per_token
/// - host_payment = (tokens_used * price_per_token) / PRICE_PRECISION
///
/// See docs/compute-contracts-reference/BREAKING_CHANGES.md for full migration guide.
use ethers::types::{Address, U256};
use std::str::FromStr;

/// Price precision multiplier for sub-$1/million token pricing support.
/// All pricePerToken values are stored with this 1000x multiplier.
///
/// # Examples
/// - $5/million tokens → pricePerToken = 5000
/// - $0.06/million tokens (budget models) → pricePerToken = 60
pub const PRICE_PRECISION: u64 = 1000;

/// Convert USD per million tokens to pricePerToken format (with PRICE_PRECISION)
///
/// # Example
/// ```
/// let price = to_precision_format(5); // $5/million → 5000
/// ```
pub fn to_precision_format(usd_per_million: u64) -> u64 {
    usd_per_million * PRICE_PRECISION
}

/// Convert pricePerToken format back to USD per million tokens
///
/// # Example
/// ```
/// let usd = from_precision_format(5000); // 5000 → $5/million
/// ```
pub fn from_precision_format(price_per_token: u64) -> u64 {
    price_per_token / PRICE_PRECISION
}

/// Native Token (ETH/BNB) Pricing Constants
///
/// Updated December 2025 for PRICE_PRECISION=1000 support.
/// Prices calibrated for ~$4400 ETH.
pub mod native {
    use super::*;

    /// Minimum price per token in wei (with PRICE_PRECISION)
    /// ~$0.001/million tokens @ $4400 ETH
    pub const MIN_PRICE_PER_TOKEN: u64 = 227_273;

    /// Maximum price per token in wei (with PRICE_PRECISION)
    /// ~$100,000/million tokens @ $4400 ETH
    pub const MAX_PRICE_PER_TOKEN: u64 = 22_727_272_727_273_000;

    /// Range multiplier (MAX / MIN) - now 100,000,000x for sub-$1 support
    pub const RANGE_MULTIPLIER: u64 = 100_000_000;

    /// Decimals for native tokens
    pub const DECIMALS: u8 = 18;

    /// Get minimum price as U256
    pub fn min_price() -> U256 {
        U256::from(MIN_PRICE_PER_TOKEN)
    }

    /// Get maximum price as U256
    pub fn max_price() -> U256 {
        U256::from(MAX_PRICE_PER_TOKEN)
    }

    /// Validate a native token price is within range
    pub fn validate_price(price: U256) -> Result<(), String> {
        if price < min_price() {
            return Err(format!(
                "Native price {} below minimum {}",
                price, MIN_PRICE_PER_TOKEN
            ));
        }
        if price > max_price() {
            return Err(format!(
                "Native price {} above maximum {}",
                price, MAX_PRICE_PER_TOKEN
            ));
        }
        Ok(())
    }

    /// Default price for native tokens (geometric mean of range)
    /// sqrt(227,273 * 22,727,272,727,273,000) ≈ 2,272,727,273
    pub fn default_price() -> U256 {
        // Geometric mean: sqrt(min * max) ≈ 2,272,727,273
        U256::from(2_272_727_273u64)
    }
}

/// Stablecoin (USDC) Pricing Constants
///
/// Updated December 2025 for PRICE_PRECISION=1000 support.
/// Now supports sub-$1/million token pricing for budget models.
pub mod stable {
    use super::*;

    /// Minimum price per token (with PRICE_PRECISION)
    /// $0.001/million tokens
    pub const MIN_PRICE_PER_TOKEN: u64 = 1;

    /// Maximum price per token (with PRICE_PRECISION)
    /// $100,000/million tokens
    pub const MAX_PRICE_PER_TOKEN: u64 = 100_000_000;

    /// Range multiplier (MAX / MIN) - now 100,000,000x for sub-$1 support
    pub const RANGE_MULTIPLIER: u64 = 100_000_000;

    /// Decimals for USDC
    pub const DECIMALS: u8 = 6;

    /// Get minimum price as U256
    pub fn min_price() -> U256 {
        U256::from(MIN_PRICE_PER_TOKEN)
    }

    /// Get maximum price as U256
    pub fn max_price() -> U256 {
        U256::from(MAX_PRICE_PER_TOKEN)
    }

    /// Validate a stable token price is within range
    pub fn validate_price(price: U256) -> Result<(), String> {
        if price < min_price() {
            return Err(format!(
                "Stable price {} below minimum {}",
                price, MIN_PRICE_PER_TOKEN
            ));
        }
        if price > max_price() {
            return Err(format!(
                "Stable price {} above maximum {}",
                price, MAX_PRICE_PER_TOKEN
            ));
        }
        Ok(())
    }

    /// Default price for stable tokens (geometric mean of range)
    /// sqrt(1 * 100,000,000) = 10,000 → $10/million tokens
    pub fn default_price() -> U256 {
        // Geometric mean: sqrt(min * max) = sqrt(1 * 100,000,000) = 10,000
        U256::from(10_000u64)
    }
}

/// Token Addresses
pub mod tokens {
    use super::*;

    /// USDC token address on Base Sepolia
    pub fn usdc_address() -> Address {
        Address::from_str("0x036CbD53842c5426634e7929541eC2318f3dCF7e")
            .expect("Invalid USDC address")
    }

    /// Zero address (represents native token in dual pricing calls)
    pub fn native_address() -> Address {
        Address::zero()
    }

    /// Get USDC token pricing from env var with fallback to stable::default_price().
    /// Reads `TOKEN_PRICING_USDC` env var. Falls back to default (10,000) if:
    /// - env var not set
    /// - env var not a valid integer
    /// - value is outside stable pricing range
    pub fn get_token_pricing_usdc() -> U256 {
        let default = stable::default_price();
        match std::env::var("TOKEN_PRICING_USDC") {
            Ok(val) => match val.parse::<u64>() {
                Ok(v) => {
                    let price = U256::from(v);
                    if stable::validate_price(price).is_ok() {
                        price
                    } else {
                        tracing::warn!(
                            "TOKEN_PRICING_USDC={} out of range, using default {}",
                            v,
                            default
                        );
                        default
                    }
                }
                Err(_) => {
                    tracing::warn!(
                        "TOKEN_PRICING_USDC='{}' invalid, using default {}",
                        val,
                        default
                    );
                    default
                }
            },
            Err(_) => default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===========================================
    // PRICE_PRECISION Tests (Sub-phase 1.1)
    // ===========================================

    #[test]
    fn test_price_precision_constant_exists() {
        // PRICE_PRECISION must be 1000 as per new contract spec
        assert_eq!(PRICE_PRECISION, 1000);
    }

    #[test]
    fn test_price_precision_is_u64() {
        // Verify type compatibility
        let precision: u64 = PRICE_PRECISION;
        assert_eq!(precision, 1000u64);
    }

    // ===========================================
    // Native Token Pricing Tests (New Values)
    // ===========================================

    #[test]
    fn test_native_min_price_value() {
        // New MIN: 227,273 wei (~$0.001/million @ $4400 ETH)
        assert_eq!(native::MIN_PRICE_PER_TOKEN, 227_273);
    }

    #[test]
    fn test_native_max_price_value() {
        // New MAX: 22,727,272,727,273,000 wei (~$100,000/million @ $4400 ETH)
        assert_eq!(native::MAX_PRICE_PER_TOKEN, 22_727_272_727_273_000);
    }

    #[test]
    fn test_native_pricing_range() {
        // Verify ~100,000,000,000x range (new wider range for sub-$1 pricing)
        let ratio = native::MAX_PRICE_PER_TOKEN as f64 / native::MIN_PRICE_PER_TOKEN as f64;
        // 22,727,272,727,273,000 / 227,273 ≈ 99,999,880,000
        // Allow 1% tolerance due to integer rounding in constants
        let expected = 100_000_000_000.0;
        let tolerance = expected * 0.01;
        assert!(
            (ratio - expected).abs() < tolerance,
            "Expected ~100 billion x range, got {}",
            ratio
        );
    }

    #[test]
    fn test_native_validation() {
        // Valid prices
        assert!(native::validate_price(native::min_price()).is_ok());
        assert!(native::validate_price(native::max_price()).is_ok());
        assert!(native::validate_price(native::default_price()).is_ok());
        // Mid-range value
        assert!(native::validate_price(U256::from(1_000_000u64)).is_ok());

        // Invalid prices - below minimum
        assert!(native::validate_price(U256::from(100)).is_err());
        assert!(native::validate_price(U256::from(227_272)).is_err()); // Just below min

        // Invalid prices - above maximum
        assert!(native::validate_price(U256::from(22_727_272_727_273_001u64)).is_err());
    }

    #[test]
    fn test_native_default_price() {
        // Default should be geometric mean of min and max
        let default = native::default_price();
        assert!(default > native::min_price());
        assert!(default < native::max_price());
        // Geometric mean of 227,273 and 22,727,272,727,273,000 ≈ 2,272,727,273 (roughly)
        // This is approximately sqrt(min * max)
    }

    // ===========================================
    // Stable Token Pricing Tests (New Values)
    // ===========================================

    #[test]
    fn test_stable_min_price_value() {
        // New MIN: 1 ($0.001/million tokens)
        assert_eq!(stable::MIN_PRICE_PER_TOKEN, 1);
    }

    #[test]
    fn test_stable_max_price_value() {
        // New MAX: 100,000,000 ($100,000/million tokens)
        assert_eq!(stable::MAX_PRICE_PER_TOKEN, 100_000_000);
    }

    #[test]
    fn test_stable_pricing_range() {
        // Verify 100,000,000x range (new wider range)
        assert_eq!(
            stable::MAX_PRICE_PER_TOKEN / stable::MIN_PRICE_PER_TOKEN,
            100_000_000
        );
    }

    #[test]
    fn test_stable_validation() {
        // Valid prices
        assert!(stable::validate_price(stable::min_price()).is_ok());
        assert!(stable::validate_price(stable::max_price()).is_ok());
        assert!(stable::validate_price(stable::default_price()).is_ok());
        // Mid-range value ($5/million = 5000 with PRICE_PRECISION)
        assert!(stable::validate_price(U256::from(5000u64)).is_ok());

        // Invalid prices - below minimum (0 is invalid)
        assert!(stable::validate_price(U256::from(0)).is_err());

        // Invalid prices - above maximum
        assert!(stable::validate_price(U256::from(100_000_001u64)).is_err());
    }

    #[test]
    fn test_stable_default_price() {
        // Default should be geometric mean of min and max
        let default = stable::default_price();
        assert!(default > stable::min_price());
        assert!(default < stable::max_price());
        // Geometric mean of 1 and 100,000,000 = 10,000 (sqrt(1 * 100,000,000))
    }

    // ===========================================
    // Helper Function Tests
    // ===========================================

    #[test]
    fn test_to_precision_format() {
        // Convert USD/million to pricePerToken format
        // $5/million -> 5 * 1000 = 5000
        assert_eq!(to_precision_format(5), 5000);
        assert_eq!(to_precision_format(1), 1000);
        assert_eq!(to_precision_format(100), 100_000);
    }

    #[test]
    fn test_from_precision_format() {
        // Convert pricePerToken back to USD/million
        // 5000 -> 5000 / 1000 = $5/million
        assert_eq!(from_precision_format(5000), 5);
        assert_eq!(from_precision_format(1000), 1);
        assert_eq!(from_precision_format(100_000), 100);
    }

    #[test]
    fn test_precision_format_roundtrip() {
        // Verify roundtrip conversion
        for usd in [1u64, 5, 10, 50, 100, 1000] {
            assert_eq!(from_precision_format(to_precision_format(usd)), usd);
        }
    }

    // ===========================================
    // Payment Calculation Tests with PRICE_PRECISION
    // ===========================================

    #[test]
    fn test_max_tokens_calculation() {
        // NEW formula: max_tokens = (deposit * PRICE_PRECISION) / price_per_token
        let deposit = U256::from(10_000_000u64); // 10 USDC (6 decimals)
        let price_per_token = U256::from(5000u64); // $5/million with PRICE_PRECISION

        let max_tokens = (deposit * U256::from(PRICE_PRECISION)) / price_per_token;
        // (10_000_000 * 1000) / 5000 = 2,000,000 tokens
        assert_eq!(max_tokens, U256::from(2_000_000u64));
    }

    #[test]
    fn test_host_payment_calculation() {
        // NEW formula: host_payment = (tokens_used * price_per_token) / PRICE_PRECISION
        let tokens_used = U256::from(1_000_000u64); // 1 million tokens
        let price_per_token = U256::from(5000u64); // $5/million with PRICE_PRECISION

        let host_payment = (tokens_used * price_per_token) / U256::from(PRICE_PRECISION);
        // (1_000_000 * 5000) / 1000 = 5,000,000 USDC units = $5
        assert_eq!(host_payment, U256::from(5_000_000u64));
    }

    #[test]
    fn test_sub_dollar_pricing() {
        // Test budget model pricing: $0.06/million (Llama 3.2 3B)
        // With PRICE_PRECISION: 0.06 * 1000 = 60
        let price_per_token = U256::from(60u64);
        let tokens_used = U256::from(1_000_000u64);

        let host_payment = (tokens_used * price_per_token) / U256::from(PRICE_PRECISION);
        // (1_000_000 * 60) / 1000 = 60,000 USDC units = $0.06
        assert_eq!(host_payment, U256::from(60_000u64));
    }

    // ===========================================
    // Token Address Tests
    // ===========================================

    #[test]
    fn test_usdc_address() {
        let addr = tokens::usdc_address();
        assert_eq!(
            addr,
            Address::from_str("0x036CbD53842c5426634e7929541eC2318f3dCF7e").unwrap()
        );
    }

    #[test]
    fn test_native_address_is_zero() {
        assert_eq!(tokens::native_address(), Address::zero());
    }

    // ===========================================
    // Token Pricing Helper Tests (Sub-phase 2.1)
    // ===========================================

    #[test]
    fn test_get_token_pricing_usdc_default() {
        // No env var set → should return stable::default_price() = 10,000
        std::env::remove_var("TOKEN_PRICING_USDC");
        let price = tokens::get_token_pricing_usdc();
        assert_eq!(price, stable::default_price());
        assert_eq!(price, U256::from(10_000u64));
    }

    #[test]
    fn test_get_token_pricing_usdc_from_env() {
        // TOKEN_PRICING_USDC=5000 → 5,000
        std::env::set_var("TOKEN_PRICING_USDC", "5000");
        let price = tokens::get_token_pricing_usdc();
        assert_eq!(price, U256::from(5_000u64));
        std::env::remove_var("TOKEN_PRICING_USDC");
    }

    #[test]
    fn test_get_token_pricing_usdc_invalid_env() {
        // TOKEN_PRICING_USDC=abc → fallback to default 10,000
        std::env::set_var("TOKEN_PRICING_USDC", "abc");
        let price = tokens::get_token_pricing_usdc();
        assert_eq!(price, stable::default_price());
        std::env::remove_var("TOKEN_PRICING_USDC");
    }

    #[test]
    fn test_get_token_pricing_usdc_out_of_range() {
        // TOKEN_PRICING_USDC=999999999 → above MAX, fallback to default 10,000
        std::env::set_var("TOKEN_PRICING_USDC", "999999999");
        let price = tokens::get_token_pricing_usdc();
        assert_eq!(price, stable::default_price());
        std::env::remove_var("TOKEN_PRICING_USDC");
    }
}
