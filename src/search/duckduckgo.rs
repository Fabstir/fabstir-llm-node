// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! DuckDuckGo search provider
//!
//! Implements web search using DuckDuckGo's HTML interface.
//! No API key required, serves as a fallback provider.

use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;

use super::provider::SearchProvider;
use super::types::{SearchError, SearchResult};

const DDG_HTML_URL: &str = "https://html.duckduckgo.com/html/";

/// DuckDuckGo search provider (no API key required)
pub struct DuckDuckGoProvider {
    client: Client,
}

impl DuckDuckGoProvider {
    /// Create a new DuckDuckGo provider
    pub fn new() -> Self {
        // Use a realistic browser User-Agent to avoid being blocked
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .expect("Failed to create HTTP client");

        Self { client }
    }
}

impl Default for DuckDuckGoProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchProvider for DuckDuckGoProvider {
    async fn search(
        &self,
        query: &str,
        num_results: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let response = self
            .client
            .post(DDG_HTML_URL)
            .form(&[("q", query)])
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

        if !response.status().is_success() {
            return Err(SearchError::ApiError {
                status: response.status().as_u16(),
                message: "DuckDuckGo request failed".to_string(),
            });
        }

        let html = response.text().await.map_err(|e| SearchError::ApiError {
            status: 0,
            message: e.to_string(),
        })?;

        // Parse HTML for results
        let results = parse_ddg_html(&html, num_results);

        Ok(results)
    }

    fn name(&self) -> &'static str {
        "duckduckgo"
    }

    fn is_available(&self) -> bool {
        true // No API key needed
    }

    fn priority(&self) -> u8 {
        50 // Fallback provider
    }
}

/// Parse DuckDuckGo HTML response to extract search results
///
/// This is a simplified parser. In production, consider using
/// the `scraper` crate for more robust HTML parsing.
fn parse_ddg_html(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    // Simple regex-based parsing for result links
    // DuckDuckGo HTML results are in <a class="result__a"> tags
    // with <a class="result__snippet"> for descriptions

    // Look for result blocks
    for (i, part) in html.split("class=\"result__a\"").enumerate().skip(1) {
        if results.len() >= max_results {
            break;
        }

        // Extract URL from href
        let url = if let Some(href_start) = part.find("href=\"") {
            let url_start = href_start + 6;
            if let Some(href_end) = part[url_start..].find('"') {
                let raw_url = &part[url_start..url_start + href_end];
                // DDG uses redirect URLs, extract the actual URL
                extract_ddg_url(raw_url)
            } else {
                continue;
            }
        } else {
            continue;
        };

        // Extract title (text between > and </a>)
        let title = if let Some(title_start) = part.find('>') {
            if let Some(title_end) = part[title_start + 1..].find("</a>") {
                html_decode(&part[title_start + 1..title_start + 1 + title_end])
            } else {
                format!("Result {}", i)
            }
        } else {
            format!("Result {}", i)
        };

        // Extract snippet
        let snippet = if let Some(snippet_pos) = part.find("class=\"result__snippet\"") {
            if let Some(snippet_start) = part[snippet_pos..].find('>') {
                let start = snippet_pos + snippet_start + 1;
                if let Some(snippet_end) = part[start..].find("</a>") {
                    html_decode(&part[start..start + snippet_end])
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        if !url.is_empty() && !title.is_empty() {
            results.push(SearchResult {
                title,
                url,
                snippet,
                published_date: None,
                source: "duckduckgo".to_string(),
            });
        }
    }

    results
}

/// Extract actual URL from DuckDuckGo's redirect URL
fn extract_ddg_url(redirect_url: &str) -> String {
    // DDG URLs look like: //duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&...
    if let Some(uddg_pos) = redirect_url.find("uddg=") {
        let url_start = uddg_pos + 5;
        let url_end = redirect_url[url_start..]
            .find('&')
            .unwrap_or(redirect_url.len() - url_start);
        let encoded_url = &redirect_url[url_start..url_start + url_end];
        // URL decode
        url_decode(encoded_url)
    } else if redirect_url.starts_with("http") {
        redirect_url.to_string()
    } else {
        String::new()
    }
}

/// Simple URL decoding
fn url_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }

    result
}

/// Simple HTML entity decoding
fn html_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        // Remove any remaining HTML tags
        .split('<')
        .map(|part| {
            if let Some(pos) = part.find('>') {
                &part[pos + 1..]
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ddg_provider_creation() {
        let provider = DuckDuckGoProvider::new();
        assert_eq!(provider.name(), "duckduckgo");
        assert!(provider.is_available());
        assert_eq!(provider.priority(), 50);
    }

    #[test]
    fn test_ddg_provider_default() {
        let provider = DuckDuckGoProvider::default();
        assert!(provider.is_available());
    }

    #[test]
    fn test_url_decode() {
        assert_eq!(url_decode("https%3A%2F%2Fexample.com"), "https://example.com");
        assert_eq!(url_decode("hello+world"), "hello world");
    }

    #[test]
    fn test_html_decode() {
        assert_eq!(html_decode("Hello &amp; World"), "Hello & World");
        // Encoded angle brackets that become tags are stripped
        // This is intentional - we want plain text from HTML
        assert_eq!(html_decode("use &lt;b&gt;"), "use");
        assert_eq!(html_decode("<b>bold</b> text"), "bold text");
        // Plain text is preserved
        assert_eq!(html_decode("plain text"), "plain text");
    }

    #[test]
    fn test_extract_ddg_url() {
        let redirect = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=abc";
        assert_eq!(extract_ddg_url(redirect), "https://example.com");

        let direct = "https://example.com";
        assert_eq!(extract_ddg_url(direct), "https://example.com");
    }

    #[test]
    fn test_parse_empty_html() {
        let results = parse_ddg_html("", 10);
        assert!(results.is_empty());
    }
}
