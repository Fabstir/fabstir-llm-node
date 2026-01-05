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

pub mod types;
pub mod config;
pub mod provider;
pub mod brave;
pub mod duckduckgo;
pub mod bing;
pub mod cache;
pub mod rate_limiter;
pub mod service;
pub mod query_extractor;

// Re-export commonly used types
pub use types::{SearchResult, SearchResponse, SearchError};
pub use config::SearchConfig;
pub use service::SearchService;
