// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Host-side web search module
//!
//! Provides web search capabilities for P2P hosts, enabling:
//! - Single search requests via `/v1/search` endpoint
//! - Search-augmented chat via `web_search` flag in inference requests
//! - Deep research with agentic loops (future)
//!
//! Key features:
//! - Multiple search providers (Brave, DuckDuckGo, Bing)
//! - TTL-based result caching
//! - Rate limiting per provider
//! - Graceful degradation on provider failures

pub mod bing;
pub mod brave;
pub mod cache;
pub mod config;
pub mod content;
pub mod duckduckgo;
pub mod provider;
pub mod query_extractor;
pub mod rate_limiter;
pub mod service;
pub mod types;

// Re-export commonly used types
pub use config::SearchConfig;
pub use service::SearchService;
pub use types::{
    SearchError, SearchResponse, SearchResponseWithContent, SearchResult, SearchResultWithContent,
};

// Re-export content fetching types (Phase 9)
pub use content::{ContentCacheStats, ContentFetchConfig, ContentFetcher, FetchError, PageContent};
