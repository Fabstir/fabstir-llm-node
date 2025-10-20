// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use super::types::SettlementError;
use crate::config::chains::ChainRegistry;
use anyhow::{anyhow, Result};
use ethers::types::{Address, U256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentConfig {
    pub host_earnings_percentage: u8,
    pub treasury_fee_percentage: u8,
    pub min_payment_threshold: U256,
    pub batch_payment_size: usize,
}

impl Default for PaymentConfig {
    fn default() -> Self {
        // Read from environment variables or use defaults
        let host_percentage = std::env::var("HOST_EARNINGS_PERCENTAGE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(90);

        let treasury_percentage = std::env::var("TREASURY_FEE_PERCENTAGE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);

        Self {
            host_earnings_percentage: host_percentage,
            treasury_fee_percentage: treasury_percentage,
            min_payment_threshold: U256::from(1_000_000_000_000_000u64), // 0.001 ETH/BNB
            batch_payment_size: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PaymentToken {
    Native,
    ERC20(Address),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentSplit {
    pub chain_id: u64,
    pub host_earnings: U256,
    pub treasury_fee: U256,
    pub user_refund: U256,
    pub total_payment: U256,
    pub native_token_symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefundCalculation {
    pub refund_amount: U256,
    pub tokens_unused: u64,
    pub original_deposit: U256,
    pub amount_spent: U256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainPaymentStats {
    pub chain_id: u64,
    pub total_payments: u64,
    pub total_volume: U256,
    pub total_fees: U256,
    pub total_refunds: U256,
    pub average_payment: U256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRecord {
    pub chain_id: u64,
    pub payment_split: PaymentSplit,
    pub token_type: PaymentToken,
    pub token_symbol: String,
    pub timestamp: u64,
}

pub struct PaymentDistributor {
    chain_registry: Arc<ChainRegistry>,
    config: PaymentConfig,
    treasury_balances: Arc<RwLock<HashMap<u64, U256>>>, // Per-chain treasury accumulation
    host_earnings: Arc<RwLock<HashMap<(u64, Address), U256>>>, // Per-chain, per-host earnings
    payment_stats: Arc<RwLock<HashMap<u64, ChainPaymentStats>>>, // Per-chain statistics
    payment_history: Arc<RwLock<Vec<PaymentRecord>>>,
}

impl PaymentDistributor {
    pub fn new(chain_registry: Arc<ChainRegistry>, config: PaymentConfig) -> Self {
        Self {
            chain_registry,
            config,
            treasury_balances: Arc::new(RwLock::new(HashMap::new())),
            host_earnings: Arc::new(RwLock::new(HashMap::new())),
            payment_stats: Arc::new(RwLock::new(HashMap::new())),
            payment_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Calculate payment split for a session
    pub async fn calculate_payment_split(
        &self,
        chain_id: u64,
        deposit: U256,
        tokens_used: u64,
        total_tokens: u64,
        price_per_token: U256,
    ) -> Result<PaymentSplit, SettlementError> {
        info!("[PAYMENT-SPLIT] 📊 === CALCULATING PAYMENT DISTRIBUTION ===");
        info!("[PAYMENT-SPLIT]   Chain: {}", chain_id);
        info!("[PAYMENT-SPLIT]   Deposit amount: {}", deposit);
        info!("[PAYMENT-SPLIT]   Tokens: {} used / {} total", tokens_used, total_tokens);
        info!("[PAYMENT-SPLIT]   Price per token: {}", price_per_token);

        let chain_config = self
            .chain_registry
            .get_chain(chain_id)
            .ok_or_else(|| {
                error!("[PAYMENT-SPLIT] ❌ Chain {} not found!", chain_id);
                SettlementError::UnsupportedChain(chain_id)
            })?;

        // Calculate total payment
        let total_payment = price_per_token * U256::from(tokens_used);
        info!("[PAYMENT-SPLIT]   Total payment due: {}", total_payment);

        // Calculate splits
        let host_earnings = total_payment * self.config.host_earnings_percentage / 100;
        let treasury_fee = total_payment * self.config.treasury_fee_percentage / 100;
        info!("[PAYMENT-SPLIT] 💵 Distribution:");
        info!("[PAYMENT-SPLIT]   - Host earnings ({}%): {}", self.config.host_earnings_percentage, host_earnings);
        info!("[PAYMENT-SPLIT]   - Treasury fee ({}%): {}", self.config.treasury_fee_percentage, treasury_fee);

        // Calculate refund
        let user_refund = if deposit > total_payment {
            let refund = deposit - total_payment;
            info!("[PAYMENT-SPLIT]   - User refund: {} (unused deposit)", refund);
            refund
        } else {
            info!("[PAYMENT-SPLIT]   - User refund: 0 (full deposit used)");
            U256::zero()
        };

        info!(
            "[PAYMENT-SPLIT] ✅ Payment split calculated for chain {}",
            chain_id
        );

        Ok(PaymentSplit {
            chain_id,
            host_earnings,
            treasury_fee,
            user_refund,
            total_payment,
            native_token_symbol: chain_config.native_token.symbol.clone(),
        })
    }

    /// Accumulate treasury fees per chain
    pub async fn accumulate_treasury_fee(&mut self, chain_id: u64, amount: U256) {
        let mut balances = self.treasury_balances.write().await;
        let balance = balances.entry(chain_id).or_insert(U256::zero());
        *balance = *balance + amount;

        info!(
            "Accumulated treasury fee on chain {}: {} (total: {})",
            chain_id, amount, balance
        );
    }

    /// Get treasury balance for a chain
    pub async fn get_treasury_balance(&self, chain_id: u64) -> U256 {
        *self
            .treasury_balances
            .read()
            .await
            .get(&chain_id)
            .unwrap_or(&U256::zero())
    }

    /// Withdraw treasury fees
    pub async fn withdraw_treasury_fees(&mut self, chain_id: u64) -> Result<U256> {
        let mut balances = self.treasury_balances.write().await;
        let amount = balances.remove(&chain_id).unwrap_or(U256::zero());

        if amount > U256::zero() {
            info!(
                "Withdrawing treasury fees from chain {}: {}",
                chain_id, amount
            );
        }

        Ok(amount)
    }

    /// Calculate refund for a user
    pub fn calculate_refund(
        &self,
        deposit: U256,
        tokens_used: u64,
        max_tokens: u64,
        price_per_token: U256,
    ) -> RefundCalculation {
        let amount_spent = price_per_token * U256::from(tokens_used);
        let refund_amount = if deposit > amount_spent {
            deposit - amount_spent
        } else {
            U256::zero()
        };

        RefundCalculation {
            refund_amount,
            tokens_unused: max_tokens.saturating_sub(tokens_used),
            original_deposit: deposit,
            amount_spent,
        }
    }

    /// Verify payment split is valid
    pub fn verify_payment_split(&self, split: &PaymentSplit) -> bool {
        // Verify that host earnings + treasury fee equals total payment
        let calculated_total = split.host_earnings + split.treasury_fee;

        if calculated_total != split.total_payment {
            warn!(
                "Payment split verification failed: {} + {} != {}",
                split.host_earnings, split.treasury_fee, split.total_payment
            );
            return false;
        }

        // Verify percentages
        let expected_host = split.total_payment * self.config.host_earnings_percentage / 100;
        let expected_treasury = split.total_payment * self.config.treasury_fee_percentage / 100;

        let host_match = split.host_earnings == expected_host;
        let treasury_match = split.treasury_fee == expected_treasury;

        host_match && treasury_match
    }

    /// Process a payment with different token types
    pub async fn process_payment(
        &self,
        chain_id: u64,
        token: PaymentToken,
        deposit: U256,
        tokens_used: u64,
        max_tokens: u64,
        price_per_token: U256,
    ) -> Result<PaymentRecord> {
        info!("[PAYMENT-DIST] 💰 === PROCESSING PAYMENT ===");
        info!("[PAYMENT-DIST]   Chain ID: {}", chain_id);
        info!("[PAYMENT-DIST]   Deposit: {}", deposit);
        info!("[PAYMENT-DIST]   Tokens used: {} / {}", tokens_used, max_tokens);
        info!("[PAYMENT-DIST]   Price per token: {}", price_per_token);

        let chain_config = self
            .chain_registry
            .get_chain(chain_id)
            .ok_or_else(|| {
                error!("[PAYMENT-DIST] ❌ Chain {} not supported!", chain_id);
                anyhow!("Unsupported chain: {}", chain_id)
            })?;

        let token_symbol = match &token {
            PaymentToken::Native => chain_config.native_token.symbol.clone(),
            PaymentToken::ERC20(addr)
                if *addr
                    == Address::from_slice(&[
                        0x03, 0x6C, 0xbD, 0x53, 0x84, 0x2c, 0x54, 0x26, 0x63, 0x4e, 0x79, 0x29,
                        0x54, 0x1e, 0xC2, 0x31, 0x8f, 0x3d, 0xCF, 0x7e,
                    ]) =>
            {
                "USDC".to_string()
            }
            PaymentToken::ERC20(_) => "ERC20".to_string(),
        };

        let payment_split = self
            .calculate_payment_split(chain_id, deposit, tokens_used, max_tokens, price_per_token)
            .await?;

        let record = PaymentRecord {
            chain_id,
            payment_split: payment_split.clone(),
            token_type: token,
            token_symbol,
            timestamp: chrono::Utc::now().timestamp() as u64,
        };

        // Store in history
        self.payment_history.write().await.push(record.clone());

        Ok(record)
    }

    /// Record payment for statistics
    pub async fn record_payment(&mut self, chain_id: u64, amount: U256, fees: U256) {
        let mut stats = self.payment_stats.write().await;
        let chain_stats = stats.entry(chain_id).or_insert(ChainPaymentStats {
            chain_id,
            total_payments: 0,
            total_volume: U256::zero(),
            total_fees: U256::zero(),
            total_refunds: U256::zero(),
            average_payment: U256::zero(),
        });

        chain_stats.total_payments += 1;
        chain_stats.total_volume = chain_stats.total_volume + amount;
        chain_stats.total_fees = chain_stats.total_fees + fees;

        if chain_stats.total_payments > 0 {
            chain_stats.average_payment = chain_stats.total_volume / chain_stats.total_payments;
        }
    }

    /// Get statistics for a specific chain
    pub async fn get_chain_statistics(&self, chain_id: u64) -> ChainPaymentStats {
        self.payment_stats
            .read()
            .await
            .get(&chain_id)
            .cloned()
            .unwrap_or(ChainPaymentStats {
                chain_id,
                total_payments: 0,
                total_volume: U256::zero(),
                total_fees: U256::zero(),
                total_refunds: U256::zero(),
                average_payment: U256::zero(),
            })
    }

    /// Get statistics for all chains
    pub async fn get_all_chain_statistics(&self) -> Vec<ChainPaymentStats> {
        self.payment_stats.read().await.values().cloned().collect()
    }

    /// Accumulate host earnings
    pub async fn accumulate_host_earnings(&mut self, chain_id: u64, host: Address, amount: U256) {
        let mut earnings = self.host_earnings.write().await;
        let key = (chain_id, host);
        let balance = earnings.entry(key).or_insert(U256::zero());
        *balance = *balance + amount;

        info!(
            "Accumulated host earnings on chain {} for {}: {} (total: {})",
            chain_id, host, amount, balance
        );
    }

    /// Get host earnings
    pub async fn get_host_earnings(&self, chain_id: u64, host: Address) -> U256 {
        *self
            .host_earnings
            .read()
            .await
            .get(&(chain_id, host))
            .unwrap_or(&U256::zero())
    }

    /// Withdraw host earnings
    pub async fn withdraw_host_earnings(&mut self, chain_id: u64, host: Address) -> Result<U256> {
        let mut earnings = self.host_earnings.write().await;
        let amount = earnings.remove(&(chain_id, host)).unwrap_or(U256::zero());

        if amount > U256::zero() {
            info!(
                "Withdrawing host earnings from chain {} for {}: {}",
                chain_id, host, amount
            );
        }

        Ok(amount)
    }
}
