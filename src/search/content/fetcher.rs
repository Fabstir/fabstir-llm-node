//! HTTP content fetching with parallel requests and timeouts
//!
//! Fetches web page content from URLs returned by search results.

use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};
use url::Url;

use super::cache::ContentCache;
use super::config::ContentFetchConfig;
use super::extractor::extract_main_content;

/// Fetched page content
#[derive(Debug, Clone)]
pub struct PageContent {
    pub url: String,
    pub title: String,
    pub text: String,
}

/// Content fetch error types
#[derive(Debug, Clone)]
pub enum FetchError {
    /// Request timed out
    Timeout(String),
    /// HTTP request error
    HttpError(String),
    /// HTTP non-success status
    HttpStatus(u16, String),
    /// No content could be extracted
    NoContent(String),
    /// URL is unsafe (localhost, private IP)
    UnsafeUrl(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout(url) => write!(f, "Timeout fetching: {}", url),
            Self::HttpError(msg) => write!(f, "HTTP error: {}", msg),
            Self::HttpStatus(code, url) => write!(f, "HTTP {} for: {}", code, url),
            Self::NoContent(url) => write!(f, "No content extracted from: {}", url),
            Self::UnsafeUrl(url) => write!(f, "Unsafe URL blocked: {}", url),
        }
    }
}

impl std::error::Error for FetchError {}

/// Content fetcher with caching and parallel requests
pub struct ContentFetcher {
    client: Client,
    cache: Arc<ContentCache>,
    config: ContentFetchConfig,
}

impl ContentFetcher {
    /// Create a new content fetcher
    pub fn new(config: ContentFetchConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_per_page_secs))
            .user_agent("Mozilla/5.0 (compatible; FabstirBot/1.0; +https://fabstir.com)")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .expect("Failed to create HTTP client");

        let cache = Arc::new(ContentCache::new(
            config.cache_ttl_secs,
            config.max_cache_entries,
        ));

        Self {
            client,
            cache,
            config,
        }
    }

    /// Fetch content from a single URL
    pub async fn fetch_content(&self, url: &str) -> Result<PageContent, FetchError> {
        // Validate URL safety
        if !Self::is_safe_url(url) {
            return Err(FetchError::UnsafeUrl(url.to_string()));
        }

        // Check cache first
        if let Some(cached) = self.cache.get(url) {
            debug!("Content cache hit for: {}", url);
            return Ok(PageContent {
                url: cached.url,
                title: cached.title,
                text: cached.text,
            });
        }

        debug!("Fetching content from: {}", url);

        // Fetch page
        let response = self.client.get(url).send().await.map_err(|e| {
            if e.is_timeout() {
                FetchError::Timeout(url.to_string())
            } else {
                FetchError::HttpError(e.to_string())
            }
        })?;

        let status = response.status();
        if !status.is_success() {
            return Err(FetchError::HttpStatus(status.as_u16(), url.to_string()));
        }

        let html = response
            .text()
            .await
            .map_err(|e| FetchError::HttpError(e.to_string()))?;

        // Extract content
        let text = extract_main_content(&html, self.config.max_chars_per_page);

        if text.len() < 100 {
            return Err(FetchError::NoContent(url.to_string()));
        }

        // Extract title from HTML
        let title = Self::extract_title(&html).unwrap_or_else(|| url.to_string());

        // Cache the result
        self.cache.insert(url, title.clone(), text.clone());

        info!("Fetched {} chars from: {}", text.len(), url);

        Ok(PageContent {
            url: url.to_string(),
            title,
            text,
        })
    }

    /// Fetch content from multiple URLs in parallel
    ///
    /// Returns results in same order as input URLs.
    /// Failed fetches return Err, but don't stop other fetches.
    pub async fn fetch_multiple(&self, urls: &[String]) -> Vec<Result<PageContent, FetchError>> {
        use futures::future::join_all;
        use tokio::time::timeout;

        let total_timeout = Duration::from_secs(self.config.total_timeout_secs);
        let max_pages = self.config.max_pages;

        // Limit number of URLs to fetch
        let urls_to_fetch: Vec<_> = urls.iter().take(max_pages).collect();

        if urls_to_fetch.is_empty() {
            return vec![];
        }

        let futures: Vec<_> = urls_to_fetch
            .iter()
            .map(|url| self.fetch_content(url))
            .collect();

        // Apply total timeout to all fetches
        match timeout(total_timeout, join_all(futures)).await {
            Ok(results) => results,
            Err(_) => {
                warn!("Total fetch timeout exceeded");
                vec![Err(FetchError::Timeout("total".to_string())); urls_to_fetch.len()]
            }
        }
    }

    /// Check if URL is safe to fetch (not localhost/private IP)
    pub fn is_safe_url(url: &str) -> bool {
        let parsed = match Url::parse(url) {
            Ok(u) => u,
            Err(_) => return false,
        };

        // Only allow http/https
        if !["http", "https"].contains(&parsed.scheme()) {
            return false;
        }

        // Block localhost and private IPs
        if let Some(host) = parsed.host_str() {
            let host_lower = host.to_lowercase();
            if host_lower == "localhost"
                || host_lower == "127.0.0.1"
                || host_lower.starts_with("192.168.")
                || host_lower.starts_with("10.")
                || host_lower.starts_with("172.16.")
                || host_lower.starts_with("172.17.")
                || host_lower.starts_with("172.18.")
                || host_lower.starts_with("172.19.")
                || host_lower.starts_with("172.20.")
                || host_lower.starts_with("172.21.")
                || host_lower.starts_with("172.22.")
                || host_lower.starts_with("172.23.")
                || host_lower.starts_with("172.24.")
                || host_lower.starts_with("172.25.")
                || host_lower.starts_with("172.26.")
                || host_lower.starts_with("172.27.")
                || host_lower.starts_with("172.28.")
                || host_lower.starts_with("172.29.")
                || host_lower.starts_with("172.30.")
                || host_lower.starts_with("172.31.")
                || host_lower.starts_with("0.0.0.0")
                || host_lower.starts_with("169.254.")
            // Link-local
            {
                return false;
            }
        }

        true
    }

    /// Extract title from HTML
    fn extract_title(html: &str) -> Option<String> {
        use scraper::{Html, Selector};

        let document = Html::parse_document(html);
        let selector = Selector::parse("title").ok()?;

        document
            .select(&selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> super::cache::ContentCacheStats {
        self.cache.stats()
    }

    /// Check if content fetching is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the configuration
    pub fn config(&self) -> &ContentFetchConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_safe_url_valid() {
        assert!(ContentFetcher::is_safe_url("https://example.com/page"));
        assert!(ContentFetcher::is_safe_url("http://bbc.com/news"));
        assert!(ContentFetcher::is_safe_url(
            "https://www.google.com/search?q=test"
        ));
    }

    #[test]
    fn test_is_safe_url_blocks_localhost() {
        assert!(!ContentFetcher::is_safe_url("http://localhost/admin"));
        assert!(!ContentFetcher::is_safe_url("http://localhost:8080/api"));
        assert!(!ContentFetcher::is_safe_url("https://localhost/"));
    }

    #[test]
    fn test_is_safe_url_blocks_loopback() {
        assert!(!ContentFetcher::is_safe_url("http://127.0.0.1/admin"));
        assert!(!ContentFetcher::is_safe_url("http://127.0.0.1:8080"));
    }

    #[test]
    fn test_is_safe_url_blocks_private_ips() {
        assert!(!ContentFetcher::is_safe_url("http://192.168.1.1/router"));
        assert!(!ContentFetcher::is_safe_url("http://10.0.0.1/internal"));
        assert!(!ContentFetcher::is_safe_url("http://172.16.0.1/private"));
        assert!(!ContentFetcher::is_safe_url("http://172.31.255.255/"));
    }

    #[test]
    fn test_is_safe_url_blocks_other_schemes() {
        assert!(!ContentFetcher::is_safe_url("ftp://example.com/file"));
        assert!(!ContentFetcher::is_safe_url("file:///etc/passwd"));
        assert!(!ContentFetcher::is_safe_url("javascript:alert(1)"));
    }

    #[test]
    fn test_extract_title() {
        let html = "<html><head><title>Test Page Title</title></head><body></body></html>";
        let title = ContentFetcher::extract_title(html);
        assert_eq!(title, Some("Test Page Title".to_string()));
    }

    #[test]
    fn test_extract_title_missing() {
        let html = "<html><body>No title here</body></html>";
        let title = ContentFetcher::extract_title(html);
        assert!(title.is_none());
    }

    #[test]
    fn test_extract_title_with_whitespace() {
        let html = "<html><head><title>  Spaced Title  </title></head></html>";
        let title = ContentFetcher::extract_title(html);
        assert_eq!(title, Some("Spaced Title".to_string()));
    }

    #[tokio::test]
    async fn test_fetcher_creation() {
        let config = ContentFetchConfig::default();
        let fetcher = ContentFetcher::new(config);
        assert!(fetcher.is_enabled());
    }

    #[tokio::test]
    async fn test_fetch_unsafe_url_blocked() {
        let config = ContentFetchConfig::default();
        let fetcher = ContentFetcher::new(config);

        let result = fetcher.fetch_content("http://localhost/admin").await;
        assert!(matches!(result, Err(FetchError::UnsafeUrl(_))));
    }
}
