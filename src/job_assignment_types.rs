use ethers::types::Address;
use serde::{Serialize, Deserialize};

// Job assignment configuration
#[derive(Debug, Clone)]
pub struct JobClaimConfig {
    pub max_concurrent_jobs: usize,
    pub claim_timeout_ms: u64,
    pub enable_auto_claim: bool,
}

// Assignment tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentRecord {
    pub job_id: String,
    pub host_address: Address,
    pub assigned_at: u64,
    pub status: AssignmentStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AssignmentStatus {
    Pending,
    Confirmed,
    Reassigned,
    Completed,
    Failed,
}