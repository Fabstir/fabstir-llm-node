use anyhow::Result;
use ethers::types::{Address, U256, H256};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SettlementStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Retrying,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementRequest {
    pub session_id: u64,
    pub chain_id: u64,
    pub priority: u8,
    pub retry_count: u8,
    pub status: SettlementStatus,
}

#[derive(Debug, Clone)]
pub struct GasEstimate {
    pub gas_limit: U256,
    pub gas_multiplier: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum SettlementError {
    #[error("Unsupported chain: {0}")]
    UnsupportedChain(u64),

    #[error("No RPC endpoint for chain: {0}")]
    NoRpcEndpoint(u64),

    #[error("Settlement failed on chain {chain}: {reason}")]
    SettlementFailed { chain: u64, reason: String },

    #[error("Insufficient balance on chain {chain}: need {required}, have {balance}")]
    InsufficientBalance {
        chain: u64,
        required: U256,
        balance: U256,
    },

    #[error("Session not found: {0}")]
    SessionNotFound(u64),

    #[error("Signer not found for chain: {0}")]
    SignerNotFound(u64),

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("Contract call failed: {0}")]
    ContractCallFailed(String),

    #[error("Queue is empty")]
    QueueEmpty,

    #[error("Maximum retries exceeded for session: {0}")]
    MaxRetriesExceeded(u64),
}

impl From<ethers::providers::ProviderError> for SettlementError {
    fn from(err: ethers::providers::ProviderError) -> Self {
        SettlementError::ProviderError(err.to_string())
    }
}

impl From<ethers::contract::ContractError<ethers::providers::Provider<ethers::providers::Http>>> for SettlementError {
    fn from(err: ethers::contract::ContractError<ethers::providers::Provider<ethers::providers::Http>>) -> Self {
        SettlementError::ContractCallFailed(err.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct SettlementResult {
    pub session_id: u64,
    pub chain_id: u64,
    pub tx_hash: H256,
    pub gas_used: U256,
    pub status: SettlementStatus,
}