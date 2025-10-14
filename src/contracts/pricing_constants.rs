/// Dual Pricing System Constants
///
/// These constants define the pricing ranges for native tokens (ETH/BNB) and
/// stablecoins (USDC) as specified in the contract deployment from January 28, 2025.
///
/// The pricing system separates native token pricing from stablecoin pricing to
/// account for different decimal places and economic models.

use ethers::types::{Address, U256};
use std::str::FromStr;

/// Native Token (ETH/BNB) Pricing Constants
pub mod native {
    use super::*;

    /// Minimum price per token in wei
    /// ~$0.00001 @ $4400 ETH
    pub const MIN_PRICE_PER_TOKEN: u64 = 2_272_727_273;

    /// Maximum price per token in wei
    /// ~$0.1 @ $4400 ETH
    pub const MAX_PRICE_PER_TOKEN: u64 = 22_727_272_727_273;

    /// Range multiplier (MAX / MIN)
    pub const RANGE_MULTIPLIER: u64 = 10_000;

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
                price,
                MIN_PRICE_PER_TOKEN
            ));
        }
        if price > max_price() {
            return Err(format!(
                "Native price {} above maximum {}",
                price,
                MAX_PRICE_PER_TOKEN
            ));
        }
        Ok(())
    }

    /// Default price for native tokens (middle of range)
    /// ~$0.00005 @ $4400 ETH
    pub fn default_price() -> U256 {
        U256::from(11_363_636_363_636u64) // ~Geometric mean of min and max
    }
}

/// Stablecoin (USDC) Pricing Constants
pub mod stable {
    use super::*;

    /// Minimum price per token
    /// 0.00001 USDC per token
    pub const MIN_PRICE_PER_TOKEN: u64 = 10;

    /// Maximum price per token
    /// 0.1 USDC per token
    pub const MAX_PRICE_PER_TOKEN: u64 = 100_000;

    /// Range multiplier (MAX / MIN)
    pub const RANGE_MULTIPLIER: u64 = 10_000;

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
                price,
                MIN_PRICE_PER_TOKEN
            ));
        }
        if price > max_price() {
            return Err(format!(
                "Stable price {} above maximum {}",
                price,
                MAX_PRICE_PER_TOKEN
            ));
        }
        Ok(())
    }

    /// Default price for stable tokens (middle of range)
    /// ~0.00032 USDC per token
    pub fn default_price() -> U256 {
        U256::from(316u64) // ~Geometric mean of min and max
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_pricing_range() {
        // Use floating point to verify 10,000x range (integer division truncates to 9999)
        let ratio = native::MAX_PRICE_PER_TOKEN as f64 / native::MIN_PRICE_PER_TOKEN as f64;
        assert!(
            (ratio - 10_000.0).abs() < 0.1,
            "Expected ~10000x range, got {}",
            ratio
        );
    }

    #[test]
    fn test_stable_pricing_range() {
        assert_eq!(
            stable::MAX_PRICE_PER_TOKEN / stable::MIN_PRICE_PER_TOKEN,
            10_000
        );
    }

    #[test]
    fn test_native_validation() {
        // Valid prices
        assert!(native::validate_price(native::min_price()).is_ok());
        assert!(native::validate_price(native::max_price()).is_ok());
        assert!(native::validate_price(native::default_price()).is_ok());

        // Invalid prices
        assert!(native::validate_price(U256::from(1000)).is_err());
        assert!(native::validate_price(U256::from(100_000_000_000_000u64)).is_err());
    }

    #[test]
    fn test_stable_validation() {
        // Valid prices
        assert!(stable::validate_price(stable::min_price()).is_ok());
        assert!(stable::validate_price(stable::max_price()).is_ok());
        assert!(stable::validate_price(stable::default_price()).is_ok());

        // Invalid prices
        assert!(stable::validate_price(U256::from(5)).is_err());
        assert!(stable::validate_price(U256::from(200_000)).is_err());
    }

    #[test]
    fn test_usdc_address() {
        let addr = tokens::usdc_address();
        assert_eq!(
            addr,
            Address::from_str("0x036CbD53842c5426634e7929541eC2318f3dCF7e").unwrap()
        );
    }
}
