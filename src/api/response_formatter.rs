// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use crate::api::{ErrorResponse, InferenceResponse, StreamingResponse};
use crate::blockchain::ChainRegistry;
use std::sync::Arc;

/// Formatter for adding chain context to all API responses
pub struct ResponseFormatter {
    chain_id: u64,
    chain_name: String,
    native_token: String,
}

impl ResponseFormatter {
    /// Create a new ResponseFormatter for a specific chain
    pub fn new(chain_id: u64) -> Self {
        let registry = ChainRegistry::new();
        let chain = registry
            .get_chain(chain_id)
            .unwrap_or_else(|| registry.get_default_chain());

        Self {
            chain_id: chain.chain_id,
            chain_name: chain.name.clone(),
            native_token: chain.native_token.symbol.clone(),
        }
    }

    /// Create from a chain registry
    pub fn from_registry(registry: &ChainRegistry, chain_id: Option<u64>) -> Self {
        let chain_id = chain_id.unwrap_or_else(|| registry.get_default_chain_id());
        let chain = registry
            .get_chain(chain_id)
            .unwrap_or_else(|| registry.get_default_chain());

        Self {
            chain_id: chain.chain_id,
            chain_name: chain.name.clone(),
            native_token: chain.native_token.symbol.clone(),
        }
    }

    /// Format an inference response with chain context
    pub fn format_inference_response(&self, mut response: InferenceResponse) -> InferenceResponse {
        if response.chain_id.is_none() {
            response.chain_id = Some(self.chain_id);
        }
        if response.chain_name.is_none() {
            response.chain_name = Some(self.chain_name.clone());
        }
        if response.native_token.is_none() {
            response.native_token = Some(self.native_token.clone());
        }
        response
    }

    /// Format a streaming response with chain context
    pub fn format_streaming_response(&self, mut response: StreamingResponse) -> StreamingResponse {
        if response.chain_id.is_none() {
            response.chain_id = Some(self.chain_id);
        }
        if response.chain_name.is_none() {
            response.chain_name = Some(self.chain_name.clone());
        }
        if response.native_token.is_none() {
            response.native_token = Some(self.native_token.clone());
        }
        response
    }

    /// Format an error response with chain context
    pub fn format_error_response(&self, mut response: ErrorResponse) -> ErrorResponse {
        response.chain_id = Some(self.chain_id);

        // Add chain details if not already present
        if let Some(ref mut details) = response.details {
            details
                .entry("chain_id".to_string())
                .or_insert_with(|| serde_json::Value::Number(self.chain_id.into()));
            details
                .entry("chain_name".to_string())
                .or_insert_with(|| serde_json::Value::String(self.chain_name.clone()));
            details
                .entry("native_token".to_string())
                .or_insert_with(|| serde_json::Value::String(self.native_token.clone()));
        } else {
            let mut details = std::collections::HashMap::new();
            details.insert(
                "chain_id".to_string(),
                serde_json::Value::Number(self.chain_id.into()),
            );
            details.insert(
                "chain_name".to_string(),
                serde_json::Value::String(self.chain_name.clone()),
            );
            details.insert(
                "native_token".to_string(),
                serde_json::Value::String(self.native_token.clone()),
            );
            response.details = Some(details);
        }

        // Add chain name to message if not present
        if !response.message.contains(&self.chain_name) {
            response.message = format!("{} on {}", response.message, self.chain_name);
        }

        response
    }

    /// Get the chain ID
    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    /// Get the chain name
    pub fn chain_name(&self) -> &str {
        &self.chain_name
    }

    /// Get the native token symbol
    pub fn native_token(&self) -> &str {
        &self.native_token
    }
}

/// Trait for types that can be formatted with chain context
pub trait ChainFormattable {
    fn with_chain_context(self, formatter: &ResponseFormatter) -> Self;
}

impl ChainFormattable for InferenceResponse {
    fn with_chain_context(self, formatter: &ResponseFormatter) -> Self {
        formatter.format_inference_response(self)
    }
}

impl ChainFormattable for StreamingResponse {
    fn with_chain_context(self, formatter: &ResponseFormatter) -> Self {
        formatter.format_streaming_response(self)
    }
}

impl ChainFormattable for ErrorResponse {
    fn with_chain_context(self, formatter: &ResponseFormatter) -> Self {
        formatter.format_error_response(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_formatter_creation() {
        let formatter = ResponseFormatter::new(84532);
        assert_eq!(formatter.chain_id(), 84532);
        assert_eq!(formatter.chain_name(), "Base Sepolia");
        assert_eq!(formatter.native_token(), "ETH");
    }

    #[test]
    fn test_inference_response_formatting() {
        let formatter = ResponseFormatter::new(84532);

        let response = InferenceResponse {
            model: "test".to_string(),
            content: "test".to_string(),
            tokens_used: 1,
            finish_reason: "stop".to_string(),
            request_id: "test".to_string(),
            chain_id: None,
            chain_name: None,
            native_token: None,
            web_search_performed: None,
            search_queries_count: None,
            search_provider: None,
            usage: None,
        };

        let formatted = formatter.format_inference_response(response);
        assert_eq!(formatted.chain_id, Some(84532));
        assert_eq!(formatted.chain_name, Some("Base Sepolia".to_string()));
        assert_eq!(formatted.native_token, Some("ETH".to_string()));
    }

    #[test]
    fn test_streaming_response_formatting() {
        let formatter = ResponseFormatter::new(84532);

        let response = StreamingResponse {
            content: "test".to_string(),
            tokens: 1,
            finish_reason: None,
            chain_id: None,
            chain_name: None,
            native_token: None,
        };

        let formatted = formatter.format_streaming_response(response);
        assert_eq!(formatted.chain_id, Some(84532));
        assert_eq!(formatted.chain_name, Some("Base Sepolia".to_string()));
        assert_eq!(formatted.native_token, Some("ETH".to_string()));
    }
}
