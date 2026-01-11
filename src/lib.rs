// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
pub mod api;
pub mod blockchain;
pub mod cache;
pub mod checkpoint;
pub mod cli;
pub mod config;
pub mod contracts;
pub mod crypto;
pub mod embeddings;
pub mod ezkl;
pub mod host;
pub mod inference;
pub mod job_assignment_types;
pub mod job_claim;
pub mod job_processor;
pub mod models;
pub mod monitoring;
pub mod p2p;
pub mod p2p_config;
pub mod payment_claim;
pub mod payments;
pub mod performance;
pub mod qa;
pub mod result_submission;
pub mod results;
pub mod settlement;
pub mod storage;
pub mod rag;
pub mod search;
pub mod utils;
pub mod vector;
pub mod version;
pub mod vision;

// Re-export main types from new modules
pub use job_assignment_types::{AssignmentRecord, AssignmentStatus, JobClaimConfig};
pub use job_claim::{
    ClaimConfig, ClaimError, ClaimEvent, ClaimResult, JobClaimer,
    JobMarketplaceTrait as ClaimMarketplaceTrait, MockMarketplace,
};
pub use job_processor::{
    ContractClientTrait, JobEvent, JobProcessor, JobRequest, JobResult, JobStatus, LLMService,
    Message, NodeConfig, NodeConfig as JobNodeConfig,
};
pub use payment_claim::{
    EscrowManager, PaymentClaimer, PaymentConfig, PaymentError, PaymentEvent, PaymentSplitter,
    PaymentStatistics, PaymentStatus, PaymentSystemTrait,
};
pub use result_submission::{
    InferenceResult, JobMarketplaceTrait as SubmissionMarketplaceTrait, ProofData, ProofGenerator,
    ResultSubmitter, StorageClient, SubmissionConfig, SubmissionError,
};

// Re-export types from existing modules
pub use contracts::{
    ChainConfig, JobMonitor, PaymentVerifier, ProofSubmitter, Web3Client, Web3Config,
};
pub use inference::{EngineConfig, InferenceRequest, LlmEngine, ModelConfig};
pub use p2p::Node;
