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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issues: Option<Vec<String>>,
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