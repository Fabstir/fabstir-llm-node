use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use ethers::types::{Address, H256, U256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Revenue {
    pub job_id: H256,
    pub gross_amount: U256,
    pub marketplace_fee: U256,
    pub network_fee: U256,
    pub bonus_amount: U256,
    pub penalty_amount: U256,
    pub net_amount: U256,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevenueStats {
    pub total_jobs: u64,
    pub total_gross_revenue: U256,
    pub total_net_revenue: U256,
    pub total_fees_paid: U256,
    pub average_job_revenue: U256,
    pub revenue_by_model: HashMap<String, U256>,
    pub revenue_by_period: HashMap<String, U256>,
}

#[derive(Debug, Clone)]
pub struct FeeStructure {
    pub marketplace_fee_percent: u8,
    pub network_fee_fixed: U256,
    pub bonus_threshold_tokens: u32,
    pub bonus_percent: u8,
    pub penalty_threshold_ms: u64,
    pub penalty_percent: u8,
}

impl Default for FeeStructure {
    fn default() -> Self {
        Self {
            marketplace_fee_percent: 5,
            network_fee_fixed: U256::from(1_000_000_000_000_000u64), // 0.001 ETH
            bonus_threshold_tokens: 1000,
            bonus_percent: 10,
            penalty_threshold_ms: 5000,
            penalty_percent: 5,
        }
    }
}

pub struct JobMetrics {
    pub tokens_generated: u32,
    pub inference_time_ms: u64,
    pub model_id: String,
    pub completed_at: DateTime<Utc>,
}

pub struct RevenueCalculator {
    fee_structure: FeeStructure,
    revenue_history: Arc<RwLock<Vec<Revenue>>>,
    revenue_by_job: Arc<RwLock<HashMap<H256, Revenue>>>,
}

impl RevenueCalculator {
    pub fn new(fee_structure: FeeStructure) -> Self {
        Self {
            fee_structure,
            revenue_history: Arc::new(RwLock::new(Vec::new())),
            revenue_by_job: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn calculate_revenue(
        &self,
        job_id: H256,
        base_amount: U256,
        metrics: JobMetrics,
    ) -> Result<Revenue> {
        let marketplace_fee = self.calculate_marketplace_fee(base_amount);
        let network_fee = self.fee_structure.network_fee_fixed;
        let bonus_amount = self.calculate_bonus(&metrics, base_amount);
        let penalty_amount = self.calculate_penalty(&metrics, base_amount);

        let total_fees = marketplace_fee + network_fee + penalty_amount;
        let total_additions = bonus_amount;

        let net_amount = if total_fees > base_amount + total_additions {
            U256::zero()
        } else {
            base_amount + total_additions - total_fees
        };

        let revenue = Revenue {
            job_id,
            gross_amount: base_amount,
            marketplace_fee,
            network_fee,
            bonus_amount,
            penalty_amount,
            net_amount,
            timestamp: metrics.completed_at,
        };

        Ok(revenue)
    }

    pub async fn record_revenue(&self, revenue: Revenue) -> Result<()> {
        self.revenue_history.write().await.push(revenue.clone());
        self.revenue_by_job
            .write()
            .await
            .insert(revenue.job_id, revenue);
        Ok(())
    }

    pub async fn get_revenue_stats(&self) -> Result<RevenueStats> {
        let history = self.revenue_history.read().await;

        let total_jobs = history.len() as u64;
        let total_gross_revenue = history
            .iter()
            .map(|r| r.gross_amount)
            .fold(U256::zero(), |acc, amt| acc + amt);

        let total_net_revenue = history
            .iter()
            .map(|r| r.net_amount)
            .fold(U256::zero(), |acc, amt| acc + amt);

        let total_fees_paid = history
            .iter()
            .map(|r| r.marketplace_fee + r.network_fee)
            .fold(U256::zero(), |acc, amt| acc + amt);

        let average_job_revenue = if total_jobs > 0 {
            total_net_revenue / U256::from(total_jobs)
        } else {
            U256::zero()
        };

        // Group revenue by model
        let mut revenue_by_model = HashMap::new();
        let jobs = self.revenue_by_job.read().await;
        for (_, revenue) in jobs.iter() {
            // Since we don't store model_id in Revenue, we'll use a placeholder
            let model_id = "default".to_string();
            *revenue_by_model.entry(model_id).or_insert(U256::zero()) += revenue.net_amount;
        }

        // Group revenue by period (simplified - just today and yesterday)
        let mut revenue_by_period = HashMap::new();
        let now = Utc::now();
        let today_start = now.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc();
        let yesterday_start = (now - Duration::days(1))
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();

        let today_revenue = history
            .iter()
            .filter(|r| r.timestamp >= today_start)
            .map(|r| r.net_amount)
            .fold(U256::zero(), |acc, amt| acc + amt);

        let yesterday_revenue = history
            .iter()
            .filter(|r| r.timestamp >= yesterday_start && r.timestamp < today_start)
            .map(|r| r.net_amount)
            .fold(U256::zero(), |acc, amt| acc + amt);

        revenue_by_period.insert("today".to_string(), today_revenue);
        revenue_by_period.insert("yesterday".to_string(), yesterday_revenue);

        Ok(RevenueStats {
            total_jobs,
            total_gross_revenue,
            total_net_revenue,
            total_fees_paid,
            average_job_revenue,
            revenue_by_model,
            revenue_by_period,
        })
    }

    pub async fn get_revenue_by_period(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Revenue>> {
        let history = self.revenue_history.read().await;
        Ok(history
            .iter()
            .filter(|r| r.timestamp >= start && r.timestamp <= end)
            .cloned()
            .collect())
    }

    pub async fn get_pending_revenue(&self) -> Result<U256> {
        let history = self.revenue_history.read().await;
        // For simplicity, assume all recorded revenue is pending
        Ok(history
            .iter()
            .map(|r| r.net_amount)
            .fold(U256::zero(), |acc, amt| acc + amt))
    }

    fn calculate_marketplace_fee(&self, amount: U256) -> U256 {
        amount * U256::from(self.fee_structure.marketplace_fee_percent) / U256::from(100)
    }

    fn calculate_bonus(&self, metrics: &JobMetrics, base_amount: U256) -> U256 {
        if metrics.tokens_generated > self.fee_structure.bonus_threshold_tokens {
            base_amount * U256::from(self.fee_structure.bonus_percent) / U256::from(100)
        } else {
            U256::zero()
        }
    }

    fn calculate_penalty(&self, metrics: &JobMetrics, base_amount: U256) -> U256 {
        if metrics.inference_time_ms > self.fee_structure.penalty_threshold_ms {
            base_amount * U256::from(self.fee_structure.penalty_percent) / U256::from(100)
        } else {
            U256::zero()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_metrics() -> JobMetrics {
        JobMetrics {
            tokens_generated: 500,
            inference_time_ms: 2000,
            model_id: "llama2-7b".to_string(),
            completed_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_basic_revenue_calculation() {
        let calculator = RevenueCalculator::new(FeeStructure::default());
        let job_id = H256::random();
        let base_amount = U256::from(100_000_000_000_000_000u64); // 0.1 ETH
        let metrics = create_test_metrics();

        let revenue = calculator
            .calculate_revenue(job_id, base_amount, metrics)
            .await
            .unwrap();

        assert_eq!(revenue.job_id, job_id);
        assert_eq!(revenue.gross_amount, base_amount);
        assert_eq!(
            revenue.marketplace_fee,
            U256::from(5_000_000_000_000_000u64)
        ); // 5%
        assert_eq!(revenue.network_fee, U256::from(1_000_000_000_000_000u64)); // 0.001 ETH
        assert_eq!(revenue.bonus_amount, U256::zero());
        assert_eq!(revenue.penalty_amount, U256::zero());
        assert_eq!(revenue.net_amount, U256::from(94_000_000_000_000_000u64)); // 0.094 ETH
    }
}
