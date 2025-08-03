pub mod config;
pub mod p2p;
pub mod api;
pub mod contracts;
pub mod inference;
pub mod job_processor;
pub mod job_claim;
pub mod result_submission;
pub mod payment_claim;
pub mod results;
pub mod payments;
pub mod host;
pub mod qa;
pub mod storage;
pub mod vector;
pub mod ezkl;
pub mod models;

// Re-export main types from new modules
pub use job_processor::{
    JobProcessor, JobStatus, JobRequest, JobResult, NodeConfig, NodeConfig as JobNodeConfig,
    LLMService, JobEvent, ContractClientTrait
};
pub use job_claim::{
    JobClaimer, ClaimError, ClaimResult, ClaimEvent, ClaimConfig,
    JobMarketplaceTrait as ClaimMarketplaceTrait
};
pub use result_submission::{
    ResultSubmitter, SubmissionError, InferenceResult, SubmissionConfig,
    StorageClient, JobMarketplaceTrait as SubmissionMarketplaceTrait,
    ProofGenerator, ProofData
};
pub use payment_claim::{
    PaymentClaimer, PaymentError, PaymentStatus, PaymentEvent, PaymentConfig,
    PaymentSplitter, EscrowManager, PaymentStatistics, PaymentSystemTrait
};

// Re-export types from existing modules  
pub use contracts::{Web3Client, Web3Config, ChainConfig, JobMonitor, PaymentVerifier, ProofSubmitter};
pub use inference::{LlmEngine, EngineConfig, ModelConfig, InferenceRequest};
pub use p2p::Node;
