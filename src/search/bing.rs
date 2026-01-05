// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Bing Search API provider
//!
//! Implements web search using Microsoft Bing Web Search API v7.
//! Alternative provider to Brave Search.

use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;

use super::provider::SearchProvider;
use super::types::{SearchError, SearchResult};

const BING_API_URL: &str = "https://api.bing.microsoft.com/v7.0/search";

/// Bing Search API provider
pub struct BingSearchProvider {
    api_key: String,
    client: Client,
}

impl BingSearchProvider {
    /// Create a new Bing Search provider
    ///
    /// # Arguments
    /// * `api_key` - Bing Search API key
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self { api_key, client }
    }
}

#[async_trait]
impl SearchProvider for BingSearchProvider {
    async fn search(
        &self,
        query: &str,
        num_results: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let response = self
            .client
            .get(BING_API_URL)
            .header("Ocp-Apim-Subscription-Key", &self.api_key)
            .query(&[
                ("q", query),
                ("count", &num_results.min(50).to_string()),
                ("responseFilter", "Webpages"),
            ])
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
                provider: "bing".to_string(),
            });
        }

        if !status.is_success() {
            let message = response.text().await.unwrap_or_default();
            return Err(SearchError::ApiError {
                status: status.as_u16(),
                message,
            });
        }

        let data: BingResponse = response.json().await.map_err(|e| SearchError::ApiError {
            status: 0,
            message: format!("JSON parse error: {}", e),
        })?;

        let results = data
            .web_pages
            .map(|pages| {
                pages
                    .value
                    .into_iter()
                    .map(|r| SearchResult {
                        title: r.name,
                        url: r.url,
                        snippet: r.snippet,
                        published_date: r.date_last_crawled,
                        source: "bing".to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(results)
    }

    fn name(&self) -> &'static str {
        "bing"
    }

    fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    fn priority(&self) -> u8 {
        20 // Secondary provider after Brave
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BingResponse {
    web_pages: Option<BingWebPages>,
}

#[derive(Debug, serde::Deserialize)]
struct BingWebPages {
    value: Vec<BingResult>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BingResult {
    name: String,
    url: String,
    snippet: String,
    date_last_crawled: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bing_provider_creation() {
        let provider = BingSearchProvider::new("test-api-key".to_string());
        assert_eq!(provider.name(), "bing");
        assert!(provider.is_available());
        assert_eq!(provider.priority(), 20);
    }

    #[test]
    fn test_bing_provider_empty_key() {
        let provider = BingSearchProvider::new(String::new());
        assert!(!provider.is_available());
    }

    #[test]
    fn test_bing_response_deserialization() {
        let json = r#"{
            "webPages": {
                "value": [
                    {
                        "name": "Test Title",
                        "url": "https://example.com",
                        "snippet": "Test description",
                        "dateLastCrawled": "2025-01-05"
                    }
                ]
            }
        }"#;

        let response: BingResponse = serde_json::from_str(json).unwrap();
        assert!(response.web_pages.is_some());
        let pages = response.web_pages.unwrap();
        assert_eq!(pages.value.len(), 1);
        assert_eq!(pages.value[0].name, "Test Title");
    }

    #[test]
    fn test_bing_response_empty_pages() {
        let json = r#"{}"#;

        let response: BingResponse = serde_json::from_str(json).unwrap();
        assert!(response.web_pages.is_none());
    }
}
