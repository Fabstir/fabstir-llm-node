use ethers::prelude::*;
use serde::{Deserialize, Serialize};

// Contract ABIs - these would normally be generated from the contract JSON
abigen!(
    NodeRegistry,
    r#"[
        {
            "inputs": [{"internalType": "address", "name": "host", "type": "address"}],
            "name": "getHost",
            "outputs": [
                {"internalType": "bool", "name": "isActive", "type": "bool"},
                {"internalType": "string[]", "name": "capabilities", "type": "string[]"},
                {"internalType": "uint256", "name": "stake", "type": "uint256"}
            ],
            "stateMutability": "view",
            "type": "function"
        }
    ]"#
);

abigen!(
    JobMarketplace,
    r#"[
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "uint256", "name": "jobId", "type": "uint256"},
                {"indexed": true, "internalType": "address", "name": "client", "type": "address"},
                {"indexed": false, "internalType": "bytes32", "name": "modelCommitment", "type": "bytes32"},
                {"indexed": false, "internalType": "uint256", "name": "maxPrice", "type": "uint256"},
                {"indexed": false, "internalType": "uint256", "name": "deadline", "type": "uint256"}
            ],
            "name": "JobPosted",
            "type": "event"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "uint256", "name": "jobId", "type": "uint256"},
                {"indexed": true, "internalType": "address", "name": "host", "type": "address"}
            ],
            "name": "JobClaimed",
            "type": "event"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "uint256", "name": "jobId", "type": "uint256"},
                {"indexed": false, "internalType": "bytes32", "name": "outputHash", "type": "bytes32"}
            ],
            "name": "JobCompleted",
            "type": "event"
        },
        {
            "inputs": [{"internalType": "uint256", "name": "jobId", "type": "uint256"}],
            "name": "getJob",
            "outputs": [
                {"internalType": "address", "name": "client", "type": "address"},
                {"internalType": "address", "name": "host", "type": "address"},
                {"internalType": "bytes32", "name": "modelCommitment", "type": "bytes32"},
                {"internalType": "uint256", "name": "maxPrice", "type": "uint256"},
                {"internalType": "uint256", "name": "deadline", "type": "uint256"},
                {"internalType": "uint8", "name": "status", "type": "uint8"}
            ],
            "stateMutability": "view",
            "type": "function"
        }
    ]"#
);

abigen!(
    PaymentEscrow,
    r#"[
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "uint256", "name": "jobId", "type": "uint256"},
                {"indexed": true, "internalType": "address", "name": "recipient", "type": "address"},
                {"indexed": false, "internalType": "uint256", "name": "amount", "type": "uint256"}
            ],
            "name": "PaymentReleased",
            "type": "event"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "uint256", "name": "jobId", "type": "uint256"},
                {"indexed": false, "internalType": "string", "name": "reason", "type": "string"}
            ],
            "name": "DisputeRaised",
            "type": "event"
        },
        {
            "inputs": [{"internalType": "uint256", "name": "jobId", "type": "uint256"}],
            "name": "getDeposit",
            "outputs": [
                {"internalType": "address", "name": "depositor", "type": "address"},
                {"internalType": "uint256", "name": "amount", "type": "uint256"},
                {"internalType": "address", "name": "token", "type": "address"},
                {"internalType": "uint8", "name": "status", "type": "uint8"}
            ],
            "stateMutability": "view",
            "type": "function"
        }
    ]"#
);

abigen!(
    ProofSystem,
    r#"[
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "uint256", "name": "jobId", "type": "uint256"},
                {"indexed": true, "internalType": "address", "name": "submitter", "type": "address"},
                {"indexed": false, "internalType": "bytes32", "name": "proofHash", "type": "bytes32"}
            ],
            "name": "ProofSubmitted",
            "type": "event"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "uint256", "name": "jobId", "type": "uint256"},
                {"indexed": false, "internalType": "bool", "name": "isValid", "type": "bool"}
            ],
            "name": "ProofVerified",
            "type": "event"
        },
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "uint256", "name": "jobId", "type": "uint256"},
                {"indexed": true, "internalType": "address", "name": "challenger", "type": "address"},
                {"indexed": false, "internalType": "string", "name": "reason", "type": "string"}
            ],
            "name": "ProofChallenged",
            "type": "event"
        },
        {
            "inputs": [{"internalType": "uint256", "name": "jobId", "type": "uint256"}],
            "name": "getProof",
            "outputs": [
                {"internalType": "address", "name": "submitter", "type": "address"},
                {"internalType": "bytes32", "name": "proofHash", "type": "bytes32"},
                {"internalType": "uint256", "name": "timestamp", "type": "uint256"},
                {"internalType": "uint8", "name": "status", "type": "uint8"}
            ],
            "stateMutability": "view",
            "type": "function"
        }
    ]"#
);

// Multicall contract for batch operations
abigen!(
    Multicall3,
    r#"[
        {
            "inputs": [
                {
                    "components": [
                        {"internalType": "address", "name": "target", "type": "address"},
                        {"internalType": "bool", "name": "allowFailure", "type": "bool"},
                        {"internalType": "bytes", "name": "callData", "type": "bytes"}
                    ],
                    "internalType": "struct Multicall3.Call3[]",
                    "name": "calls",
                    "type": "tuple[]"
                }
            ],
            "name": "aggregate3",
            "outputs": [
                {
                    "components": [
                        {"internalType": "bool", "name": "success", "type": "bool"},
                        {"internalType": "bytes", "name": "returnData", "type": "bytes"}
                    ],
                    "internalType": "struct Multicall3.Result[]",
                    "name": "returnData",
                    "type": "tuple[]"
                }
            ],
            "stateMutability": "payable",
            "type": "function"
        }
    ]"#
);

// Job status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Posted = 0,
    Claimed = 1,
    Completed = 2,
    Cancelled = 3,
    Disputed = 4,
}

impl From<u8> for JobStatus {
    fn from(value: u8) -> Self {
        match value {
            0 => JobStatus::Posted,
            1 => JobStatus::Claimed,
            2 => JobStatus::Completed,
            3 => JobStatus::Cancelled,
            4 => JobStatus::Disputed,
            _ => JobStatus::Posted,
        }
    }
}

// Payment status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentStatus {
    Locked = 0,
    Released = 1,
    Refunded = 2,
    Disputed = 3,
}

impl From<u8> for PaymentStatus {
    fn from(value: u8) -> Self {
        match value {
            0 => PaymentStatus::Locked,
            1 => PaymentStatus::Released,
            2 => PaymentStatus::Refunded,
            3 => PaymentStatus::Disputed,
            _ => PaymentStatus::Locked,
        }
    }
}

// Proof status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofStatus {
    NotSubmitted = 0,
    Submitted = 1,
    Verified = 2,
    Challenged = 3,
    Invalid = 4,
}

impl From<u8> for ProofStatus {
    fn from(value: u8) -> Self {
        match value {
            0 => ProofStatus::NotSubmitted,
            1 => ProofStatus::Submitted,
            2 => ProofStatus::Verified,
            3 => ProofStatus::Challenged,
            4 => ProofStatus::Invalid,
            _ => ProofStatus::NotSubmitted,
        }
    }
}

// Contract deployment addresses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractAddresses {
    pub node_registry: Address,
    pub job_marketplace: Address,
    pub payment_escrow: Address,
    pub reputation_system: Address,
    pub proof_system: Address,
}

// Job metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMetadata {
    pub model: String,
    pub prompt: String,
    pub parameters: serde_json::Value,
}

// Payment info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentInfo {
    pub job_id: U256,
    pub amount: U256,
    pub token_symbol: String,
    pub status: PaymentStatus,
    pub client: Address,
}

// Block range for filtering
#[derive(Debug, Clone)]
pub struct BlockRange {
    pub from: Option<BlockNumber>,
    pub to: Option<BlockNumber>,
}