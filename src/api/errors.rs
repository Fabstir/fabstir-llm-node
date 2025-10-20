// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorResponse {
    pub error_type: String,
    pub message: String,
    pub request_id: Option<String>,
    pub details: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum ApiError {
    NotFound(String),
    MethodNotAllowed(String),
    InvalidRequest(String),
    ValidationError {
        field: String,
        message: String,
    },
    Unauthorized(String),
    RateLimitExceeded {
        retry_after: u64,
    },
    ServiceUnavailable(String),
    ModelNotFound {
        model: String,
        available_models: Vec<String>,
    },
    InternalError(String),
    CircuitBreakerOpen,
    Timeout,
}

impl ApiError {
    pub fn to_response(&self, request_id: Option<String>) -> ErrorResponse {
        let (error_type, message, details) = match self {
            ApiError::NotFound(msg) => ("not_found", msg.clone(), None),
            ApiError::MethodNotAllowed(msg) => ("method_not_allowed", msg.clone(), None),
            ApiError::InvalidRequest(msg) => ("invalid_request", msg.clone(), None),
            ApiError::ValidationError { field, message } => {
                let mut details = HashMap::new();
                details.insert(
                    "field".to_string(),
                    serde_json::Value::String(field.clone()),
                );
                ("validation_error", message.clone(), Some(details))
            }
            ApiError::Unauthorized(msg) => ("unauthorized", msg.clone(), None),
            ApiError::RateLimitExceeded { retry_after } => {
                let mut details = HashMap::new();
                details.insert(
                    "retry_after".to_string(),
                    serde_json::Value::Number((*retry_after).into()),
                );
                (
                    "rate_limit_exceeded",
                    "Rate limit exceeded".to_string(),
                    Some(details),
                )
            }
            ApiError::ServiceUnavailable(msg) => ("service_unavailable", msg.clone(), None),
            ApiError::ModelNotFound {
                model,
                available_models,
            } => {
                let mut details = HashMap::new();
                details.insert(
                    "available_models".to_string(),
                    serde_json::Value::Array(
                        available_models
                            .iter()
                            .map(|m| serde_json::Value::String(m.clone()))
                            .collect(),
                    ),
                );
                (
                    "model_not_found",
                    format!("Model '{}' not found", model),
                    Some(details),
                )
            }
            ApiError::InternalError(msg) => ("internal_error", msg.clone(), None),
            ApiError::CircuitBreakerOpen => (
                "service_unavailable",
                "Circuit breaker is open".to_string(),
                None,
            ),
            ApiError::Timeout => ("timeout", "Request timed out".to_string(), None),
        };

        ErrorResponse {
            error_type: error_type.to_string(),
            message,
            request_id,
            details,
            chain_id: None,
        }
    }

    pub fn to_response_with_chain(
        &self,
        request_id: Option<String>,
        chain_id: u64,
        chain_name: &str,
        native_token: &str,
    ) -> ErrorResponse {
        let mut response = self.to_response(request_id);
        response.chain_id = Some(chain_id);

        // Add chain context to details
        if let Some(ref mut details) = response.details {
            details.insert(
                "chain_id".to_string(),
                serde_json::Value::Number(chain_id.into()),
            );
            details.insert(
                "chain_name".to_string(),
                serde_json::Value::String(chain_name.to_string()),
            );
            details.insert(
                "native_token".to_string(),
                serde_json::Value::String(native_token.to_string()),
            );
        } else {
            let mut details = HashMap::new();
            details.insert(
                "chain_id".to_string(),
                serde_json::Value::Number(chain_id.into()),
            );
            details.insert(
                "chain_name".to_string(),
                serde_json::Value::String(chain_name.to_string()),
            );
            details.insert(
                "native_token".to_string(),
                serde_json::Value::String(native_token.to_string()),
            );
            response.details = Some(details);
        }

        // Update message to include chain context if relevant
        if !response.message.contains(chain_name) {
            response.message = format!("{} on {}", response.message, chain_name);
        }

        response
    }

    pub fn status_code(&self) -> u16 {
        match self {
            ApiError::NotFound(_) => 404,
            ApiError::MethodNotAllowed(_) => 405,
            ApiError::InvalidRequest(_) | ApiError::ValidationError { .. } => 400,
            ApiError::Unauthorized(_) => 401,
            ApiError::RateLimitExceeded { .. } => 429,
            ApiError::ServiceUnavailable(_) | ApiError::CircuitBreakerOpen => 503,
            ApiError::ModelNotFound { .. } => 404,
            ApiError::InternalError(_) => 500,
            ApiError::Timeout => 504,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::NotFound(msg) => write!(f, "Not found: {}", msg),
            ApiError::MethodNotAllowed(msg) => write!(f, "Method not allowed: {}", msg),
            ApiError::InvalidRequest(msg) => write!(f, "Invalid request: {}", msg),
            ApiError::ValidationError { field, message } => {
                write!(f, "Validation error for {}: {}", field, message)
            }
            ApiError::Unauthorized(msg) => write!(f, "Unauthorized: {}", msg),
            ApiError::RateLimitExceeded { retry_after } => write!(
                f,
                "Rate limit exceeded, retry after {} seconds",
                retry_after
            ),
            ApiError::ServiceUnavailable(msg) => write!(f, "Service unavailable: {}", msg),
            ApiError::ModelNotFound { model, .. } => write!(f, "Model '{}' not found", model),
            ApiError::InternalError(msg) => write!(f, "Internal error: {}", msg),
            ApiError::CircuitBreakerOpen => write!(f, "Circuit breaker is open"),
            ApiError::Timeout => write!(f, "Request timed out"),
        }
    }
}

impl std::error::Error for ApiError {}
