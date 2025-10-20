// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use super::types::{GasEstimate, SettlementError};
use ethers::types::U256;
use std::collections::HashMap;

pub struct GasEstimator {
    // Chain-specific gas configurations
    gas_limits: HashMap<u64, HashMap<String, U256>>,
    gas_multipliers: HashMap<u64, f64>,
}

impl GasEstimator {
    pub fn new() -> Self {
        let mut gas_limits = HashMap::new();
        let mut gas_multipliers = HashMap::new();

        // Base Sepolia (84532) gas limits
        let mut base_limits = HashMap::new();
        base_limits.insert("settle_session".to_string(), U256::from(200_000));
        base_limits.insert("submit_proof".to_string(), U256::from(150_000));
        base_limits.insert("claim_payment".to_string(), U256::from(100_000));
        gas_limits.insert(84532, base_limits);
        gas_multipliers.insert(84532, 1.1); // 10% buffer for Base Sepolia

        // opBNB Testnet (5611) gas limits - typically needs more gas
        let mut opbnb_limits = HashMap::new();
        opbnb_limits.insert("settle_session".to_string(), U256::from(300_000));
        opbnb_limits.insert("submit_proof".to_string(), U256::from(200_000));
        opbnb_limits.insert("claim_payment".to_string(), U256::from(150_000));
        gas_limits.insert(5611, opbnb_limits);
        gas_multipliers.insert(5611, 1.2); // 20% buffer for opBNB

        Self {
            gas_limits,
            gas_multipliers,
        }
    }

    pub fn estimate_gas(
        &self,
        chain_id: u64,
        operation: &str,
    ) -> Result<GasEstimate, SettlementError> {
        let chain_limits = self
            .gas_limits
            .get(&chain_id)
            .ok_or(SettlementError::UnsupportedChain(chain_id))?;

        let gas_limit = chain_limits
            .get(operation)
            .cloned()
            .unwrap_or_else(|| U256::from(250_000)); // Default gas limit

        let gas_multiplier = self.gas_multipliers.get(&chain_id).copied().unwrap_or(1.15); // Default 15% buffer

        Ok(GasEstimate {
            gas_limit,
            gas_multiplier,
        })
    }

    pub fn estimate_with_buffer(
        &self,
        chain_id: u64,
        operation: &str,
    ) -> Result<U256, SettlementError> {
        let estimate = self.estimate_gas(chain_id, operation)?;

        // Apply the multiplier to the gas limit
        let buffered = estimate.gas_limit.as_u64() as f64 * estimate.gas_multiplier;
        Ok(U256::from(buffered as u64))
    }

    pub fn get_chain_multiplier(&self, chain_id: u64) -> f64 {
        self.gas_multipliers.get(&chain_id).copied().unwrap_or(1.15)
    }

    pub fn set_chain_multiplier(&mut self, chain_id: u64, multiplier: f64) {
        self.gas_multipliers.insert(chain_id, multiplier);
    }

    pub fn update_gas_limit(&mut self, chain_id: u64, operation: &str, new_limit: U256) {
        self.gas_limits
            .entry(chain_id)
            .or_insert_with(HashMap::new)
            .insert(operation.to_string(), new_limit);
    }
}

impl Default for GasEstimator {
    fn default() -> Self {
        Self::new()
    }
}
