// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
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
    #[serde(skip_serializing_if = "Option::is_none", alias = "jobId")]
    pub job_id: Option<u64>, // Blockchain job ID for payment (accepts job_id or jobId)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", alias = "chainId")]
    pub chain_id: Option<u64>, // Chain ID for multi-chain support (accepts chain_id or chainId)
    /// Enable web search before inference (v8.7.0+)
    #[serde(default, alias = "webSearch")]
    pub web_search: bool,
    /// Maximum number of searches to perform (default 5, max 20)
    #[serde(default = "default_max_searches", alias = "maxSearches")]
    pub max_searches: u32,
    /// Custom search queries (optional, auto-extracted from prompt if not provided)
    #[serde(skip_serializing_if = "Option::is_none", alias = "searchQueries")]
    pub search_queries: Option<Vec<String>>,
    /// Thinking/reasoning mode (v8.17.0+)
    /// Values: "enabled", "disabled", "low", "medium", "high"
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thinking: Option<String>,
}

fn default_max_searches() -> u32 {
    5
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
    /// Whether web search was performed (v8.7.0+)
    #[serde(skip_serializing_if = "Option::is_none", alias = "webSearchPerformed")]
    pub web_search_performed: Option<bool>,
    /// Number of search queries executed
    #[serde(skip_serializing_if = "Option::is_none", alias = "searchQueriesCount")]
    pub search_queries_count: Option<u32>,
    /// Search provider used (if search was performed)
    #[serde(skip_serializing_if = "Option::is_none", alias = "searchProvider")]
    pub search_provider: Option<String>,
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

        if let Some(ref thinking) = self.thinking {
            let valid = ["enabled", "disabled", "low", "medium", "high"];
            if !valid.contains(&thinking.as_str()) {
                return Err(ApiError::ValidationError {
                    field: "thinking".to_string(),
                    message: format!(
                        "Invalid thinking mode '{}'. Valid: enabled, disabled, low, medium, high",
                        thinking
                    ),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thinking_field_deserializes_all_values() {
        for val in &["enabled", "high", "disabled", "low", "medium"] {
            let json = format!(
                r#"{{"model":"m","prompt":"p","max_tokens":10,"thinking":"{}"}}"#,
                val
            );
            let req: InferenceRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(req.thinking.as_deref(), Some(*val));
        }
        let json = r#"{"model":"m","prompt":"p","max_tokens":10,"thinking":null}"#;
        let req: InferenceRequest = serde_json::from_str(json).unwrap();
        assert!(req.thinking.is_none());
    }

    #[test]
    fn test_thinking_field_defaults_to_none() {
        let json = r#"{"model":"m","prompt":"p","max_tokens":10}"#;
        let req: InferenceRequest = serde_json::from_str(json).unwrap();
        assert!(req.thinking.is_none());
    }

    #[test]
    fn test_thinking_field_validation_rejects_invalid() {
        let json = r#"{"model":"m","prompt":"p","max_tokens":10,"thinking":"invalid_value"}"#;
        let req: InferenceRequest = serde_json::from_str(json).unwrap();
        let err = req.validate().unwrap_err();
        assert!(format!("{:?}", err).contains("thinking"));
    }

    #[test]
    fn test_thinking_field_validation_accepts_all_valid() {
        for val in &["enabled", "disabled", "low", "medium", "high"] {
            let json = format!(
                r#"{{"model":"m","prompt":"p","max_tokens":10,"thinking":"{}"}}"#,
                val
            );
            let req: InferenceRequest = serde_json::from_str(&json).unwrap();
            assert!(
                req.validate().is_ok(),
                "validate() failed for thinking={}",
                val
            );
        }
    }
}
