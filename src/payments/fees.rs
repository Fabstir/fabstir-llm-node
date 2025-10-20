// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use chrono::{DateTime, Utc};
use ethers::types::{Address, H256, U256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeAllocation {
    pub job_id: H256,
    pub total_fee: U256,
    pub marketplace_share: U256,
    pub network_share: U256,
    pub referrer_share: U256,
    pub burn_amount: U256,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeRecipient {
    pub address: Address,
    pub share_percentage: u8,
    pub role: RecipientRole,
    pub accumulated_fees: U256,
    pub last_claim: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum RecipientRole {
    MarketplaceOperator,
    NetworkMaintainer,
    Referrer,
    Treasury,
    BurnAddress,
}

#[derive(Debug, Clone)]
pub struct FeeDistributionConfig {
    pub marketplace_percentage: u8,
    pub network_percentage: u8,
    pub referrer_percentage: u8,
    pub burn_percentage: u8,
    pub minimum_claim_amount: U256,
    pub auto_distribute_threshold: U256,
}

impl Default for FeeDistributionConfig {
    fn default() -> Self {
        Self {
            marketplace_percentage: 40,
            network_percentage: 30,
            referrer_percentage: 20,
            burn_percentage: 10,
            minimum_claim_amount: U256::from(1_000_000_000_000_000u64), // 0.001 ETH
            auto_distribute_threshold: U256::from(100_000_000_000_000_000u64), // 0.1 ETH
        }
    }
}

#[derive(Debug, Clone)]
pub struct FeeStats {
    pub total_fees_collected: U256,
    pub total_distributed: U256,
    pub pending_distribution: U256,
    pub fees_by_role: HashMap<RecipientRole, U256>,
    pub distribution_count: u64,
}

pub struct FeeDistributor {
    config: FeeDistributionConfig,
    contract_client: Arc<dyn ContractClient>,
    recipients: Arc<RwLock<HashMap<RecipientRole, FeeRecipient>>>,
    fee_allocations: Arc<RwLock<Vec<FeeAllocation>>>,
    pending_fees: Arc<RwLock<HashMap<Address, U256>>>,
}

#[async_trait::async_trait]
pub trait ContractClient: Send + Sync {
    async fn distribute_fee(&self, recipient: Address, amount: U256) -> Result<H256>;

    async fn batch_distribute(&self, distributions: Vec<(Address, U256)>) -> Result<Vec<H256>>;

    async fn burn_tokens(&self, amount: U256) -> Result<H256>;

    async fn get_fee_balance(&self) -> Result<U256>;
}

impl FeeDistributor {
    pub fn new(config: FeeDistributionConfig, contract_client: Arc<dyn ContractClient>) -> Self {
        Self {
            config,
            contract_client,
            recipients: Arc::new(RwLock::new(HashMap::new())),
            fee_allocations: Arc::new(RwLock::new(Vec::new())),
            pending_fees: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn allocate_fee(
        &self,
        job_id: H256,
        total_fee: U256,
        referrer: Option<Address>,
    ) -> Result<FeeAllocation> {
        let (marketplace_share, network_share, referrer_share, burn_amount) =
            self.calculate_shares(total_fee, referrer.is_some());

        let allocation = FeeAllocation {
            job_id,
            total_fee,
            marketplace_share,
            network_share,
            referrer_share,
            burn_amount,
            timestamp: Utc::now(),
        };

        // Update pending fees
        let mut pending = self.pending_fees.write().await;
        let recipients = self.recipients.read().await;

        // Add marketplace share
        if let Some(marketplace) = recipients.get(&RecipientRole::MarketplaceOperator) {
            *pending.entry(marketplace.address).or_insert(U256::zero()) += marketplace_share;
        }

        // Add network share
        if let Some(network) = recipients.get(&RecipientRole::NetworkMaintainer) {
            *pending.entry(network.address).or_insert(U256::zero()) += network_share;
        }

        // Add referrer share if applicable
        if let Some(referrer_addr) = referrer {
            if referrer_share > U256::zero() {
                *pending.entry(referrer_addr).or_insert(U256::zero()) += referrer_share;
            }
        }

        // Store allocation
        self.fee_allocations.write().await.push(allocation.clone());

        Ok(allocation)
    }

    pub async fn distribute_pending_fees(&self) -> Result<Vec<H256>> {
        let mut pending = self.pending_fees.write().await;
        let mut tx_hashes = Vec::new();

        // Collect distributions
        let mut distributions = Vec::new();
        for (address, amount) in pending.iter() {
            if *amount >= self.config.minimum_claim_amount {
                distributions.push((*address, *amount));
            }
        }

        // Execute distributions
        if !distributions.is_empty() {
            let hashes = self
                .contract_client
                .batch_distribute(distributions.clone())
                .await?;
            tx_hashes.extend(hashes);

            // Clear distributed amounts
            for (address, _) in distributions {
                pending.remove(&address);
            }
        }

        // Handle burn
        let allocations = self.fee_allocations.read().await;
        let total_burn = allocations
            .iter()
            .map(|a| a.burn_amount)
            .fold(U256::zero(), |acc, amt| acc + amt);

        if total_burn > U256::zero() {
            let burn_hash = self.contract_client.burn_tokens(total_burn).await?;
            tx_hashes.push(burn_hash);
        }

        Ok(tx_hashes)
    }

    pub async fn claim_fees(&self, recipient: Address) -> Result<H256> {
        let mut pending = self.pending_fees.write().await;
        let amount = pending.get(&recipient).cloned().unwrap_or_default();

        if amount < self.config.minimum_claim_amount {
            anyhow::bail!("Amount below minimum claim threshold");
        }

        let tx_hash = self
            .contract_client
            .distribute_fee(recipient, amount)
            .await?;
        pending.remove(&recipient);

        // Update recipient's last claim time
        let mut recipients = self.recipients.write().await;
        for (_, fee_recipient) in recipients.iter_mut() {
            if fee_recipient.address == recipient {
                fee_recipient.last_claim = Some(Utc::now());
                break;
            }
        }

        Ok(tx_hash)
    }

    pub async fn register_recipient(
        &self,
        role: RecipientRole,
        address: Address,
        share_percentage: u8,
    ) -> Result<()> {
        let recipient = FeeRecipient {
            address,
            share_percentage,
            role: role.clone(),
            accumulated_fees: U256::zero(),
            last_claim: None,
        };

        self.recipients.write().await.insert(role, recipient);
        Ok(())
    }

    pub async fn get_pending_fees(&self, recipient: Address) -> Result<U256> {
        Ok(self
            .pending_fees
            .read()
            .await
            .get(&recipient)
            .cloned()
            .unwrap_or_default())
    }

    pub async fn get_fee_stats(&self) -> Result<FeeStats> {
        let allocations = self.fee_allocations.read().await;
        let pending = self.pending_fees.read().await;
        let recipients = self.recipients.read().await;

        let total_fees_collected = allocations
            .iter()
            .map(|a| a.total_fee)
            .fold(U256::zero(), |acc, amt| acc + amt);

        let pending_distribution = pending.values().fold(U256::zero(), |acc, amt| acc + amt);

        let total_distributed = total_fees_collected - pending_distribution;

        let mut fees_by_role = HashMap::new();
        fees_by_role.insert(
            RecipientRole::MarketplaceOperator,
            allocations
                .iter()
                .map(|a| a.marketplace_share)
                .fold(U256::zero(), |acc, amt| acc + amt),
        );
        fees_by_role.insert(
            RecipientRole::NetworkMaintainer,
            allocations
                .iter()
                .map(|a| a.network_share)
                .fold(U256::zero(), |acc, amt| acc + amt),
        );
        fees_by_role.insert(
            RecipientRole::Referrer,
            allocations
                .iter()
                .map(|a| a.referrer_share)
                .fold(U256::zero(), |acc, amt| acc + amt),
        );
        fees_by_role.insert(
            RecipientRole::BurnAddress,
            allocations
                .iter()
                .map(|a| a.burn_amount)
                .fold(U256::zero(), |acc, amt| acc + amt),
        );

        Ok(FeeStats {
            total_fees_collected,
            total_distributed,
            pending_distribution,
            fees_by_role,
            distribution_count: recipients.len() as u64,
        })
    }

    pub async fn auto_distribute_if_needed(&self) -> Result<Option<Vec<H256>>> {
        let pending = self.pending_fees.read().await;
        let total_pending = pending.values().fold(U256::zero(), |acc, amt| acc + amt);

        if total_pending >= self.config.auto_distribute_threshold {
            drop(pending);
            let tx_hashes = self.distribute_pending_fees().await?;
            Ok(Some(tx_hashes))
        } else {
            Ok(None)
        }
    }

    fn calculate_shares(&self, total: U256, has_referrer: bool) -> (U256, U256, U256, U256) {
        if has_referrer {
            let marketplace =
                total * U256::from(self.config.marketplace_percentage) / U256::from(100);
            let network = total * U256::from(self.config.network_percentage) / U256::from(100);
            let referrer = total * U256::from(self.config.referrer_percentage) / U256::from(100);
            let burn = total * U256::from(self.config.burn_percentage) / U256::from(100);

            // Ensure total adds up
            let sum = marketplace + network + referrer + burn;
            let remainder = if sum < total {
                total - sum
            } else {
                U256::zero()
            };

            (marketplace + remainder, network, referrer, burn)
        } else {
            // Redistribute referrer share proportionally when no referrer
            let total_percent = self.config.marketplace_percentage
                + self.config.network_percentage
                + self.config.burn_percentage;

            let marketplace = total
                * U256::from(
                    self.config.marketplace_percentage
                        + self.config.referrer_percentage * self.config.marketplace_percentage
                            / total_percent,
                )
                / U256::from(100);
            let network = total
                * U256::from(
                    self.config.network_percentage
                        + self.config.referrer_percentage * self.config.network_percentage
                            / total_percent,
                )
                / U256::from(100);
            let burn = total
                * U256::from(
                    self.config.burn_percentage
                        + self.config.referrer_percentage * self.config.burn_percentage
                            / total_percent,
                )
                / U256::from(100);

            // Ensure total adds up
            let sum = marketplace + network + burn;
            let remainder = if sum < total {
                total - sum
            } else {
                U256::zero()
            };

            (marketplace + remainder, network, U256::zero(), burn)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockContractClient {
        distributed_fees: Arc<RwLock<Vec<(Address, U256)>>>,
        burned_amount: Arc<RwLock<U256>>,
        fee_balance: Arc<RwLock<U256>>,
    }

    impl MockContractClient {
        fn new() -> Self {
            Self {
                distributed_fees: Arc::new(RwLock::new(Vec::new())),
                burned_amount: Arc::new(RwLock::new(U256::zero())),
                fee_balance: Arc::new(RwLock::new(U256::zero())),
            }
        }
    }

    #[async_trait::async_trait]
    impl ContractClient for MockContractClient {
        async fn distribute_fee(&self, recipient: Address, amount: U256) -> Result<H256> {
            self.distributed_fees
                .write()
                .await
                .push((recipient, amount));
            Ok(H256::random())
        }

        async fn batch_distribute(&self, distributions: Vec<(Address, U256)>) -> Result<Vec<H256>> {
            let mut tx_hashes = vec![];
            for (recipient, amount) in distributions {
                tx_hashes.push(self.distribute_fee(recipient, amount).await?);
            }
            Ok(tx_hashes)
        }

        async fn burn_tokens(&self, amount: U256) -> Result<H256> {
            *self.burned_amount.write().await += amount;
            Ok(H256::random())
        }

        async fn get_fee_balance(&self) -> Result<U256> {
            Ok(*self.fee_balance.read().await)
        }
    }

    #[tokio::test]
    async fn test_fee_distributor_creation() {
        let client = Arc::new(MockContractClient::new());
        let distributor = FeeDistributor::new(FeeDistributionConfig::default(), client);

        assert_eq!(distributor.config.marketplace_percentage, 40);
        assert_eq!(distributor.config.network_percentage, 30);
        assert_eq!(distributor.config.referrer_percentage, 20);
        assert_eq!(distributor.config.burn_percentage, 10);
    }
}
