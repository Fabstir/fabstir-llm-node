pub mod tracker;
pub mod revenue;
pub mod withdrawal;
pub mod fees;

// Re-export the internal modules that tests expect
pub mod payment_tracker {
    pub use super::tracker::*;
}

pub mod revenue_calculator {
    pub use super::revenue::*;
}

pub mod withdrawal_manager {
    pub use super::withdrawal::*;
}

pub mod fee_distributor {
    pub use super::fees::*;
}

// Also re-export main types at top level
pub use tracker::{PaymentTracker, PaymentEvent, PaymentFilter, PaymentStats, PaymentEventType};
pub use revenue::{RevenueCalculator, Revenue, RevenueStats, FeeStructure, JobMetrics};
pub use withdrawal::{WithdrawalManager, WithdrawalRequest, WithdrawalStatus, WithdrawalConfig, WithdrawalStats};
pub use fees::{FeeDistributor, FeeAllocation, FeeRecipient, RecipientRole, FeeDistributionConfig, FeeStats};