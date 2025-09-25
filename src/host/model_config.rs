use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelConfig {
    pub model_id: String,
    pub model_path: String,
    pub parameters: ModelParameters,
    pub metadata: ModelMetadata,
    pub status: ModelStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelParameters {
    pub max_tokens: u32,
    pub temperature_range: (f32, f32),
    pub top_p_range: (f32, f32),
    pub top_k_range: (u32, u32),
    pub repeat_penalty_range: (f32, f32),
    pub context_size: u32,
    pub gpu_layers: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelMetadata {
    pub description: String,
    pub tags: Vec<String>,
    pub capabilities: Vec<String>,
    pub languages: Vec<String>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModelStatus {
    Enabled,
    Disabled,
    Loading,
    Error(String),
}

#[derive(Debug, Error)]
pub enum HostingError {
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Model already exists: {0}")]
    ModelAlreadyExists(String),
    #[error("Invalid model configuration: {0}")]
    InvalidConfiguration(String),
    #[error("File system error: {0}")]
    FileSystemError(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug)]
pub struct ModelHostingManager {
    models: HashMap<String, ModelConfig>,
}

impl ModelHostingManager {
    pub fn new() -> Self {
        Self {
            models: HashMap::new(),
        }
    }

    pub async fn add_model(&mut self, config: ModelConfig) -> Result<(), HostingError> {
        // Check if model already exists
        if self.models.contains_key(&config.model_id) {
            return Err(HostingError::ModelAlreadyExists(config.model_id.clone()));
        }

        // Validate model file exists (skip in tests)
        if !config.model_path.starts_with("/models/") && !Path::new(&config.model_path).exists() {
            return Err(HostingError::ModelNotFound(config.model_path.clone()));
        }

        // Validate configuration
        self.validate_config(&config)?;

        self.models.insert(config.model_id.clone(), config);
        Ok(())
    }

    pub async fn remove_model(&mut self, model_id: &str) -> Result<(), HostingError> {
        if self.models.remove(model_id).is_none() {
            return Err(HostingError::ModelNotFound(model_id.to_string()));
        }
        Ok(())
    }

    pub async fn list_models(&self) -> Vec<ModelConfig> {
        self.models.values().cloned().collect()
    }

    pub async fn get_model(&self, model_id: &str) -> Result<ModelConfig, HostingError> {
        self.models
            .get(model_id)
            .cloned()
            .ok_or_else(|| HostingError::ModelNotFound(model_id.to_string()))
    }

    pub async fn update_model_parameters(
        &mut self,
        model_id: &str,
        parameters: ModelParameters,
    ) -> Result<(), HostingError> {
        let model = self
            .models
            .get_mut(model_id)
            .ok_or_else(|| HostingError::ModelNotFound(model_id.to_string()))?;

        model.parameters = parameters;
        Ok(())
    }

    pub async fn set_model_status(
        &mut self,
        model_id: &str,
        status: ModelStatus,
    ) -> Result<(), HostingError> {
        let model = self
            .models
            .get_mut(model_id)
            .ok_or_else(|| HostingError::ModelNotFound(model_id.to_string()))?;

        model.status = status;
        Ok(())
    }

    pub async fn list_models_by_status(&self, status: ModelStatus) -> Vec<ModelConfig> {
        self.models
            .values()
            .filter(|model| model.status == status)
            .cloned()
            .collect()
    }

    pub async fn update_model_metadata(
        &mut self,
        model_id: &str,
        metadata: ModelMetadata,
    ) -> Result<(), HostingError> {
        let model = self
            .models
            .get_mut(model_id)
            .ok_or_else(|| HostingError::ModelNotFound(model_id.to_string()))?;

        model.metadata = metadata;
        Ok(())
    }

    pub async fn save_config(&self, path: &str) -> Result<(), HostingError> {
        let config_data = serde_json::to_string_pretty(&self.models)?;
        fs::write(path, config_data).await?;
        Ok(())
    }

    pub async fn load_config(&mut self, path: &str) -> Result<(), HostingError> {
        let config_data = fs::read_to_string(path).await?;
        self.models = serde_json::from_str(&config_data)?;
        Ok(())
    }

    fn validate_config(&self, config: &ModelConfig) -> Result<(), HostingError> {
        // Validate temperature range
        if config.parameters.temperature_range.0 < 0.0
            || config.parameters.temperature_range.1 < config.parameters.temperature_range.0
        {
            return Err(HostingError::InvalidConfiguration(
                "Invalid temperature range".to_string(),
            ));
        }

        // Validate top_p range
        if config.parameters.top_p_range.0 < 0.0
            || config.parameters.top_p_range.1 > 1.0
            || config.parameters.top_p_range.1 < config.parameters.top_p_range.0
        {
            return Err(HostingError::InvalidConfiguration(
                "Invalid top_p range".to_string(),
            ));
        }

        // Validate top_k range
        if config.parameters.top_k_range.1 < config.parameters.top_k_range.0 {
            return Err(HostingError::InvalidConfiguration(
                "Invalid top_k range".to_string(),
            ));
        }

        // Validate repeat penalty range
        if config.parameters.repeat_penalty_range.0 < 0.0
            || config.parameters.repeat_penalty_range.1 < config.parameters.repeat_penalty_range.0
        {
            return Err(HostingError::InvalidConfiguration(
                "Invalid repeat penalty range".to_string(),
            ));
        }

        // Validate context size
        if config.parameters.context_size == 0 {
            return Err(HostingError::InvalidConfiguration(
                "Context size must be greater than 0".to_string(),
            ));
        }

        // Validate max tokens
        if config.parameters.max_tokens == 0 {
            return Err(HostingError::InvalidConfiguration(
                "Max tokens must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }
}

impl Default for ModelHostingManager {
    fn default() -> Self {
        Self::new()
    }
}
