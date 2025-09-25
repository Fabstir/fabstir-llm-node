use crate::job_processor::Message;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRequest {
    pub model: String,
    pub prompt: String,
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default)]
    pub conversation_context: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<u64>, // Blockchain job ID for payment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>, // Chain ID for multi-chain support
}

fn default_temperature() -> f32 {
    0.7
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResponse {
    pub model: String,
    pub content: String,
    pub tokens_used: u32,
    pub finish_reason: String,
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsResponse {
    pub models: Vec<ModelInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issues: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub job_id: u64,
    pub chain_id: Option<u64>,
    pub user_address: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub tokens_used: u64,
    pub status: SessionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfoResponse {
    pub session_id: u64,
    pub chain_id: u64,
    pub chain_name: String,
    pub native_token: String,
    pub status: String,
    pub tokens_used: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainInfo {
    pub chain_id: u64,
    pub name: String,
    pub native_token: String,
    pub rpc_url: String,
    pub contracts: crate::blockchain::ContractAddresses,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainsResponse {
    pub chains: Vec<ChainInfo>,
    pub default_chain: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainStatistics {
    pub chain_id: u64,
    pub chain_name: String,
    pub total_sessions: u64,
    pub active_sessions: u64,
    pub total_tokens_processed: u64,
    pub total_settlements: u64,
    pub failed_settlements: u64,
    pub average_settlement_time_ms: u64,
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainStatsResponse {
    pub chains: Vec<ChainStatistics>,
    pub total: TotalStatistics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotalStatistics {
    pub total_sessions: u64,
    pub active_sessions: u64,
    pub total_tokens_processed: u64,
}

impl InferenceRequest {
    pub fn validate(&self) -> Result<(), crate::api::ApiError> {
        use crate::api::ApiError;

        if self.model.is_empty() {
            return Err(ApiError::ValidationError {
                field: "model".to_string(),
                message: "Model name cannot be empty".to_string(),
            });
        }

        if self.prompt.is_empty() {
            return Err(ApiError::ValidationError {
                field: "prompt".to_string(),
                message: "Prompt cannot be empty".to_string(),
            });
        }

        if self.max_tokens == 0 {
            return Err(ApiError::ValidationError {
                field: "max_tokens".to_string(),
                message: "max_tokens must be greater than 0".to_string(),
            });
        }

        if self.temperature < 0.0 || self.temperature > 2.0 {
            return Err(ApiError::ValidationError {
                field: "temperature".to_string(),
                message: "Temperature must be between 0.0 and 2.0".to_string(),
            });
        }

        Ok(())
    }
}
