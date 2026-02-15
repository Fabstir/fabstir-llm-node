//! Content fetching module for web search enhancement
//!
//! Fetches actual page content from search result URLs to provide
//! LLM with real information instead of just snippets.
//!
//! ## Architecture
//!
//! ```text
//! Search Results (URLs) → ContentFetcher → HTML → ContentExtractor → Clean Text
//!                              ↓
//!                        ContentCache (30min TTL)
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! let config = ContentFetchConfig::from_env();
//! let fetcher = ContentFetcher::new(config);
//!
//! // Fetch content from multiple URLs in parallel
//! let urls = vec!["https://example.com".to_string()];
//! let contents = fetcher.fetch_multiple(&urls).await;
//! ```

pub mod cache;
pub mod config;
pub mod extractor;
pub mod fetcher;

pub use cache::{CachedContent, ContentCache, ContentCacheStats};
pub use config::ContentFetchConfig;
pub use extractor::extract_main_content;
pub use fetcher::{ContentFetcher, FetchError, PageContent};
