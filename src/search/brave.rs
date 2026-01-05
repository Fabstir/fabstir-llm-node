// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Brave Search API provider
//!
//! Implements web search using the Brave Search API.
//! Brave is the preferred provider due to privacy focus and good free tier.

use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;

use super::provider::SearchProvider;
use super::types::{SearchError, SearchResult};

const BRAVE_API_URL: &str = "https://api.search.brave.com/res/v1/web/search";

/// Brave Search API provider
pub struct BraveSearchProvider {
    api_key: String,
    client: Client,
}

impl BraveSearchProvider {
    /// Create a new Brave Search provider
    ///
    /// # Arguments
    /// * `api_key` - Brave Search API key
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self { api_key, client }
    }
}

#[async_trait]
impl SearchProvider for BraveSearchProvider {
    async fn search(
        &self,
        query: &str,
        num_results: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let response = self
            .client
            .get(BRAVE_API_URL)
            .header("X-Subscription-Token", &self.api_key)
            .header("Accept", "application/json")
            .query(&[("q", query), ("count", &num_results.min(20).to_string())])
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    SearchError::Timeout { timeout_ms: 10000 }
                } else {
                    SearchError::ApiError {
                        status: 0,
                        message: e.to_string(),
                    }
                }
            })?;

        let status = response.status();

        if status == 429 {
            return Err(SearchError::RateLimited {
                retry_after_secs: 60,
            });
        }

        if status == 401 || status == 403 {
            return Err(SearchError::NoApiKey {
                provider: "brave".to_string(),
            });
        }

        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(SearchError::ApiError {
                status: status.as_u16(),
                message,
            });
        }

        let data: BraveResponse = response.json().await.map_err(|e| SearchError::ApiError {
            status: 0,
            message: format!("JSON parse error: {}", e),
        })?;

        Ok(data
            .web
            .results
            .into_iter()
            .map(|r| SearchResult {
                title: r.title,
                url: r.url,
                snippet: r.description,
                published_date: r.age,
                source: "brave".to_string(),
            })
            .collect())
    }

    fn name(&self) -> &'static str {
        "brave"
    }

    fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    fn priority(&self) -> u8 {
        10 // Preferred provider
    }
}

#[derive(Debug, serde::Deserialize)]
struct BraveResponse {
    web: BraveWebResults,
}

#[derive(Debug, serde::Deserialize)]
struct BraveWebResults {
    results: Vec<BraveResult>,
}

#[derive(Debug, serde::Deserialize)]
struct BraveResult {
    title: String,
    url: String,
    description: String,
    age: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brave_provider_creation() {
        let provider = BraveSearchProvider::new("test-api-key".to_string());
        assert_eq!(provider.name(), "brave");
        assert!(provider.is_available());
        assert_eq!(provider.priority(), 10);
    }

    #[test]
    fn test_brave_provider_empty_key() {
        let provider = BraveSearchProvider::new(String::new());
        assert!(!provider.is_available());
    }

    #[test]
    fn test_brave_response_deserialization() {
        let json = r#"{
            "web": {
                "results": [
                    {
                        "title": "Test Title",
                        "url": "https://example.com",
                        "description": "Test description",
                        "age": "2 days ago"
                    }
                ]
            }
        }"#;

        let response: BraveResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.web.results.len(), 1);
        assert_eq!(response.web.results[0].title, "Test Title");
    }

    #[test]
    fn test_brave_response_no_age() {
        let json = r#"{
            "web": {
                "results": [
                    {
                        "title": "Test",
                        "url": "https://example.com",
                        "description": "Test"
                    }
                ]
            }
        }"#;

        let response: BraveResponse = serde_json::from_str(json).unwrap();
        assert!(response.web.results[0].age.is_none());
    }
}
