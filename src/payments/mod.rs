// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
pub mod fees;
pub mod revenue;
pub mod tracker;
pub mod withdrawal;

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
pub use fees::{
    FeeAllocation, FeeDistributionConfig, FeeDistributor, FeeRecipient, FeeStats, RecipientRole,
};
pub use revenue::{FeeStructure, JobMetrics, Revenue, RevenueCalculator, RevenueStats};
pub use tracker::{PaymentEvent, PaymentEventType, PaymentFilter, PaymentStats, PaymentTracker};
pub use withdrawal::{
    WithdrawalConfig, WithdrawalManager, WithdrawalRequest, WithdrawalStats, WithdrawalStatus,
};
