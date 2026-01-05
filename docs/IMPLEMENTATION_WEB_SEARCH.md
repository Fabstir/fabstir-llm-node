# IMPLEMENTATION - Host-Side Web Search

## Status: PLANNING

**Status**: Phase 0 - Planning Complete
**Version**: v8.7.0-web-search (planned)
**Target Start Date**: TBD
**Approach**: Strict TDD bounded autonomy - one sub-phase at a time

---

## Overview

Implementation plan for host-side web search in Fabstir LLM Node. This enables hosts to perform web searches on behalf of clients, supporting use cases from simple lookups to deep research with hundreds of queries - without consuming client bandwidth or mobile data.

**Key Benefits for Decentralized P2P:**
- Hosts have datacenter-quality connections (fast, reliable, unlimited)
- Mobile/low-bandwidth clients send single request, receive synthesized response
- Deep research (100+ searches) impractical on client side
- Search cost built into inference pricing

**New Capabilities:**
- `POST /v1/search` - Standalone web search endpoint
- `web_search` flag in chat/inference requests
- `POST /v1/research` - Deep research with agentic loop
- WebSocket messages for search progress streaming

**Approach**: Strict TDD bounded autonomy - one sub-phase at a time

**Key Constraints:**
- **Privacy-respecting providers** - Brave Search, DuckDuckGo preferred
- **Graceful degradation** - If search fails, continue inference without results
- **Rate limiting** - Prevent abuse of search APIs
- **Caching** - Reduce API costs and latency for repeated queries
- **Host capability** - Search is optional; hosts advertise availability

**References:**
- Existing API patterns: `src/api/embed/`, `src/api/ocr/`
- Session management: `src/api/websocket/`
- Model manager pattern: `src/vision/model_manager.rs`

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           CLIENT (Mobile/Web)                            │
│                                                                          │
│  User: "What are the latest AI developments in 2025?"                   │
│                              │                                           │
│         { "prompt": "...", "web_search": true }                         │
└──────────────────────────────────┬──────────────────────────────────────┘
                                   │ Single request (~1KB)
                                   ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         P2P HOST NODE                                    │
│                                                                          │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    Search Module (src/search/)                    │   │
│  │                                                                   │   │
│  │  SearchService                                                   │   │
│  │    ├── BraveSearchProvider                                       │   │
│  │    ├── DuckDuckGoProvider                                        │   │
│  │    ├── BingSearchProvider                                        │   │
│  │    ├── SearchCache (TTL-based)                                   │   │
│  │    └── RateLimiter                                               │   │
│  └───────────────────────────────┬─────────────────────────────────┘   │
│                                  │                                      │
│  ┌───────────────────────────────┴─────────────────────────────────┐   │
│  │              Research Agent (src/research/) [Phase 4]            │   │
│  │                                                                   │   │
│  │  1. Generate research plan (LLM)                                 │   │
│  │  2. Execute searches (parallel batches)                          │   │
│  │  3. Synthesize results (LLM)                                     │   │
│  │  4. Identify gaps → more searches                                │   │
│  │  5. Final synthesis                                              │   │
│  └───────────────────────────────┬─────────────────────────────────┘   │
│                                  │                                      │
│  ┌───────────────────────────────┴─────────────────────────────────┐   │
│  │                    LLM Inference Engine                          │   │
│  │                    (llama-cpp-2 + CUDA)                          │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Dependencies Required

### Cargo.toml Updates
```toml
[dependencies]
# Existing dependencies (no changes needed):
# reqwest = { version = "0.11", features = ["json", "rustls-tls"] }
# tokio = { version = "1", features = ["full"] }
# serde = { version = "1", features = ["derive"] }
# serde_json = "1"
# tracing = "0.1"

# Rate limiting (NEW - may already have similar)
governor = "0.6"                              # Token bucket rate limiting

# URL handling (likely already present)
url = "2"                                     # URL parsing
```

**Note**: Most dependencies already present in the project. Governor adds robust rate limiting.

---

## Environment Variables

```bash
# Web Search Configuration
WEB_SEARCH_ENABLED=true                       # Enable/disable search feature
BRAVE_API_KEY=BSA...                          # Brave Search API key (recommended)
BING_API_KEY=...                              # Bing Search API key (optional)
SEARCH_CACHE_TTL_SECS=3600                    # Cache TTL (default: 1 hour)
SEARCH_RATE_LIMIT_PER_MINUTE=60               # Rate limit per minute
MAX_SEARCHES_PER_REQUEST=20                   # Max searches per single request
MAX_SEARCHES_PER_SESSION=200                  # Max searches per session
DEEP_RESEARCH_MAX_ITERATIONS=5                # Max research iterations
DEEP_RESEARCH_MAX_SEARCHES=100                # Max searches for deep research
```

---

## Module Structure

```
src/
├── search/
│   ├── mod.rs                 # Public exports
│   ├── types.rs               # SearchResult, SearchError, SearchResponse
│   ├── config.rs              # SearchConfig, environment loading
│   ├── provider.rs            # SearchProvider trait
│   ├── brave.rs               # Brave Search implementation
│   ├── duckduckgo.rs          # DuckDuckGo implementation (no API key)
│   ├── bing.rs                # Bing Search implementation
│   ├── cache.rs               # TTL-based result caching
│   ├── rate_limiter.rs        # Rate limiting per provider
│   ├── service.rs             # SearchService orchestration
│   └── query_extractor.rs     # Extract search queries from prompts
│
├── research/                  # Phase 4
│   ├── mod.rs
│   ├── agent.rs               # Agentic research loop
│   ├── planner.rs             # Generate research plans via LLM
│   └── synthesizer.rs         # Combine and summarize results
│
├── api/
│   ├── search/
│   │   ├── mod.rs
│   │   ├── handler.rs         # /v1/search endpoint
│   │   ├── request.rs         # SearchRequest validation
│   │   └── response.rs        # SearchApiResponse
│   └── research/              # Phase 4
│       ├── mod.rs
│       ├── handler.rs         # /v1/research endpoint
│       ├── request.rs
│       └── response.rs
```

---

## Phase 1: Foundation (2 hours)

### Sub-phase 1.1: Add Dependencies ✅

**Goal**: Add required crates and verify build

**Status**: COMPLETE (2025-01-05)

#### Tasks
- [x] Add `governor = "0.6"` to Cargo.toml (if not present)
- [x] Add `url = "2"` to Cargo.toml for URL parsing
- [x] Verify `reqwest`, `serde` dependencies present
- [x] Run `cargo check` to verify dependencies compile
- [x] Run existing tests to ensure no regressions (445 passed, 4 pre-existing failures)

**Test Files:**
- Run `cargo test --lib` - Ensure no regressions

**Implementation Files:**
- `Cargo.toml` - Added dependencies:
  - `governor = "0.6"` (line 89) - Rate limiting for search API requests
  - `url = "2"` (line 85) - URL parsing and validation

---

### Sub-phase 1.2: Create Module Structure

**Goal**: Create stub files for all new modules

**Status**: NOT STARTED

#### Tasks
- [ ] Create `src/search/mod.rs` with submodule declarations
- [ ] Create `src/search/types.rs` stub
- [ ] Create `src/search/config.rs` stub
- [ ] Create `src/search/provider.rs` stub
- [ ] Create `src/search/brave.rs` stub
- [ ] Create `src/search/duckduckgo.rs` stub
- [ ] Create `src/search/bing.rs` stub
- [ ] Create `src/search/cache.rs` stub
- [ ] Create `src/search/rate_limiter.rs` stub
- [ ] Create `src/search/service.rs` stub
- [ ] Create `src/search/query_extractor.rs` stub
- [ ] Create `src/api/search/mod.rs` stub
- [ ] Create `src/api/search/handler.rs` stub
- [ ] Create `src/api/search/request.rs` stub
- [ ] Create `src/api/search/response.rs` stub
- [ ] Add `pub mod search;` to `src/lib.rs`
- [ ] Add `pub mod search;` to `src/api/mod.rs`
- [ ] Run `cargo check` to verify module structure

**Files Created:**
- `src/search/mod.rs`
- `src/search/types.rs`
- `src/search/config.rs`
- `src/search/provider.rs`
- `src/search/brave.rs`
- `src/search/duckduckgo.rs`
- `src/search/bing.rs`
- `src/search/cache.rs`
- `src/search/rate_limiter.rs`
- `src/search/service.rs`
- `src/search/query_extractor.rs`
- `src/api/search/mod.rs`
- `src/api/search/handler.rs`
- `src/api/search/request.rs`
- `src/api/search/response.rs`

**Files Modified:**
- `src/lib.rs` - Add `pub mod search;`
- `src/api/mod.rs` - Add `pub mod search;`

---

### Sub-phase 1.3: Define Core Types

**Goal**: Define search types with serialization

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for SearchResult serialization/deserialization (5 tests)
- [ ] Write tests for SearchResponse serialization (3 tests)
- [ ] Write tests for SearchError variants (4 tests)
- [ ] Implement `SearchResult` struct
- [ ] Implement `SearchResponse` struct
- [ ] Implement `SearchError` enum with thiserror
- [ ] Implement `SearchQuery` struct for batch operations

**Test Files:**
- Inline tests in `src/search/types.rs` (~12 tests)

**Implementation Files:**
- `src/search/types.rs` (max 200 lines)
  ```rust
  use serde::{Deserialize, Serialize};
  use thiserror::Error;

  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct SearchResult {
      pub title: String,
      pub url: String,
      pub snippet: String,
      #[serde(skip_serializing_if = "Option::is_none")]
      pub published_date: Option<String>,
      pub source: String,  // "brave", "bing", "duckduckgo"
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct SearchResponse {
      pub query: String,
      pub results: Vec<SearchResult>,
      pub search_time_ms: u64,
      pub provider: String,
      pub cached: bool,
      pub result_count: usize,
  }

  #[derive(Debug, Error)]
  pub enum SearchError {
      #[error("Rate limited, retry after {retry_after_secs}s")]
      RateLimited { retry_after_secs: u64 },

      #[error("Search API error: {status} - {message}")]
      ApiError { status: u16, message: String },

      #[error("Search timeout after {timeout_ms}ms")]
      Timeout { timeout_ms: u64 },

      #[error("Provider unavailable: {provider}")]
      ProviderUnavailable { provider: String },

      #[error("No API key configured for {provider}")]
      NoApiKey { provider: String },

      #[error("Invalid query: {reason}")]
      InvalidQuery { reason: String },

      #[error("Search disabled on this host")]
      SearchDisabled,
  }

  #[derive(Debug, Clone)]
  pub struct SearchQuery {
      pub query: String,
      pub num_results: usize,
      pub request_id: Option<String>,
  }
  ```

---

### Sub-phase 1.4: Define Configuration

**Goal**: Define configuration with environment variable loading

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for SearchConfig defaults (3 tests)
- [ ] Write tests for environment variable loading (5 tests)
- [ ] Write tests for validation (3 tests)
- [ ] Implement `SearchConfig` struct
- [ ] Implement `SearchProviderConfig` struct
- [ ] Implement `from_env()` loading function
- [ ] Add validation for config values

**Test Files:**
- Inline tests in `src/search/config.rs` (~11 tests)

**Implementation Files:**
- `src/search/config.rs` (max 200 lines)
  ```rust
  use std::env;

  #[derive(Debug, Clone)]
  pub struct SearchConfig {
      pub enabled: bool,
      pub providers: SearchProviderConfig,
      pub cache_ttl_secs: u64,
      pub max_searches_per_request: u32,
      pub max_searches_per_session: u32,
      pub rate_limit_per_minute: u32,
      pub default_num_results: usize,
      pub request_timeout_ms: u64,
  }

  #[derive(Debug, Clone)]
  pub struct SearchProviderConfig {
      pub brave_api_key: Option<String>,
      pub bing_api_key: Option<String>,
      pub preferred_provider: String,
  }

  impl SearchConfig {
      pub fn from_env() -> Self {
          Self {
              enabled: env::var("WEB_SEARCH_ENABLED")
                  .map(|v| v.to_lowercase() == "true")
                  .unwrap_or(false),
              providers: SearchProviderConfig {
                  brave_api_key: env::var("BRAVE_API_KEY").ok(),
                  bing_api_key: env::var("BING_API_KEY").ok(),
                  preferred_provider: env::var("SEARCH_PROVIDER")
                      .unwrap_or_else(|_| "brave".to_string()),
              },
              cache_ttl_secs: env::var("SEARCH_CACHE_TTL_SECS")
                  .ok()
                  .and_then(|v| v.parse().ok())
                  .unwrap_or(3600),
              max_searches_per_request: env::var("MAX_SEARCHES_PER_REQUEST")
                  .ok()
                  .and_then(|v| v.parse().ok())
                  .unwrap_or(20),
              max_searches_per_session: env::var("MAX_SEARCHES_PER_SESSION")
                  .ok()
                  .and_then(|v| v.parse().ok())
                  .unwrap_or(200),
              rate_limit_per_minute: env::var("SEARCH_RATE_LIMIT_PER_MINUTE")
                  .ok()
                  .and_then(|v| v.parse().ok())
                  .unwrap_or(60),
              default_num_results: 10,
              request_timeout_ms: 10000,
          }
      }

      pub fn validate(&self) -> Result<(), String> {
          if self.enabled && self.providers.brave_api_key.is_none()
              && self.providers.bing_api_key.is_none() {
              return Err("Search enabled but no API keys configured".to_string());
          }
          Ok(())
      }

      pub fn has_any_provider(&self) -> bool {
          self.providers.brave_api_key.is_some()
              || self.providers.bing_api_key.is_some()
      }
  }

  impl Default for SearchConfig {
      fn default() -> Self {
          Self {
              enabled: false,
              providers: SearchProviderConfig {
                  brave_api_key: None,
                  bing_api_key: None,
                  preferred_provider: "brave".to_string(),
              },
              cache_ttl_secs: 3600,
              max_searches_per_request: 20,
              max_searches_per_session: 200,
              rate_limit_per_minute: 60,
              default_num_results: 10,
              request_timeout_ms: 10000,
          }
      }
  }
  ```

---

## Phase 2: Search Providers (4 hours)

### Sub-phase 2.1: SearchProvider Trait

**Goal**: Define trait for search providers with async support

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for trait object usage (2 tests)
- [ ] Define `SearchProvider` async trait
- [ ] Add provider metadata methods
- [ ] Add availability check method

**Test Files:**
- Inline tests in `src/search/provider.rs` (~4 tests)

**Implementation Files:**
- `src/search/provider.rs` (max 100 lines)
  ```rust
  use async_trait::async_trait;
  use super::types::{SearchResult, SearchError};

  #[async_trait]
  pub trait SearchProvider: Send + Sync {
      /// Perform a web search
      async fn search(
          &self,
          query: &str,
          num_results: usize,
      ) -> Result<Vec<SearchResult>, SearchError>;

      /// Provider name for logging and billing
      fn name(&self) -> &'static str;

      /// Check if provider is available (has API key, etc.)
      fn is_available(&self) -> bool;

      /// Provider priority (lower = preferred)
      fn priority(&self) -> u8 {
          100
      }
  }
  ```

---

### Sub-phase 2.2: Brave Search Provider

**Goal**: Implement Brave Search API integration

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for BraveSearchProvider creation (3 tests)
- [ ] Write tests for successful search response parsing (4 tests)
- [ ] Write tests for error handling (rate limit, auth, timeout) (5 tests)
- [ ] Write tests for result transformation (3 tests)
- [ ] Implement `BraveSearchProvider` struct
- [ ] Implement API request construction
- [ ] Implement response parsing
- [ ] Add proper error handling for all status codes

**Test Files:**
- Inline tests in `src/search/brave.rs` (~15 tests)
- Mock tests using wiremock or similar

**Implementation Files:**
- `src/search/brave.rs` (max 250 lines)
  ```rust
  use reqwest::Client;
  use std::time::Duration;
  use async_trait::async_trait;
  use super::provider::SearchProvider;
  use super::types::{SearchResult, SearchError};

  const BRAVE_API_URL: &str = "https://api.search.brave.com/res/v1/web/search";

  pub struct BraveSearchProvider {
      api_key: String,
      client: Client,
  }

  impl BraveSearchProvider {
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
          let response = self.client
              .get(BRAVE_API_URL)
              .header("X-Subscription-Token", &self.api_key)
              .header("Accept", "application/json")
              .query(&[
                  ("q", query),
                  ("count", &num_results.min(20).to_string()),
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
              return Err(SearchError::RateLimited { retry_after_secs: 60 });
          }

          if status == 401 || status == 403 {
              return Err(SearchError::NoApiKey {
                  provider: "brave".to_string()
              });
          }

          if !status.is_success() {
              let message = response.text().await.unwrap_or_default();
              return Err(SearchError::ApiError {
                  status: status.as_u16(),
                  message,
              });
          }

          let data: BraveResponse = response.json().await.map_err(|e| {
              SearchError::ApiError {
                  status: 0,
                  message: format!("JSON parse error: {}", e),
              }
          })?;

          Ok(data.web.results.into_iter().map(|r| SearchResult {
              title: r.title,
              url: r.url,
              snippet: r.description,
              published_date: r.age,
              source: "brave".to_string(),
          }).collect())
      }

      fn name(&self) -> &'static str {
          "brave"
      }

      fn is_available(&self) -> bool {
          !self.api_key.is_empty()
      }

      fn priority(&self) -> u8 {
          10  // Preferred provider
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
  ```

---

### Sub-phase 2.3: DuckDuckGo Provider

**Goal**: Implement DuckDuckGo search (no API key required)

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for DuckDuckGoProvider creation (2 tests)
- [ ] Write tests for HTML parsing (4 tests)
- [ ] Write tests for rate limiting handling (2 tests)
- [ ] Implement `DuckDuckGoProvider` struct
- [ ] Implement HTML scraping approach (DDG has no official API)
- [ ] Add respectful rate limiting

**Test Files:**
- Inline tests in `src/search/duckduckgo.rs` (~8 tests)

**Implementation Files:**
- `src/search/duckduckgo.rs` (max 200 lines)
  ```rust
  use reqwest::Client;
  use std::time::Duration;
  use async_trait::async_trait;
  use super::provider::SearchProvider;
  use super::types::{SearchResult, SearchError};

  const DDG_HTML_URL: &str = "https://html.duckduckgo.com/html/";

  pub struct DuckDuckGoProvider {
      client: Client,
  }

  impl DuckDuckGoProvider {
      pub fn new() -> Self {
          let client = Client::builder()
              .timeout(Duration::from_secs(10))
              .user_agent("Mozilla/5.0 (compatible; FabstirBot/1.0)")
              .build()
              .expect("Failed to create HTTP client");

          Self { client }
      }
  }

  #[async_trait]
  impl SearchProvider for DuckDuckGoProvider {
      async fn search(
          &self,
          query: &str,
          num_results: usize,
      ) -> Result<Vec<SearchResult>, SearchError> {
          let response = self.client
              .post(DDG_HTML_URL)
              .form(&[("q", query)])
              .send()
              .await
              .map_err(|e| SearchError::ApiError {
                  status: 0,
                  message: e.to_string(),
              })?;

          let html = response.text().await.map_err(|e| {
              SearchError::ApiError {
                  status: 0,
                  message: e.to_string(),
              }
          })?;

          // Parse HTML for results (simplified)
          let results = parse_ddg_html(&html, num_results);

          Ok(results)
      }

      fn name(&self) -> &'static str {
          "duckduckgo"
      }

      fn is_available(&self) -> bool {
          true  // No API key needed
      }

      fn priority(&self) -> u8 {
          50  // Fallback provider
      }
  }

  fn parse_ddg_html(html: &str, max_results: usize) -> Vec<SearchResult> {
      // Simple regex-based parsing
      // In production, use scraper crate for robust HTML parsing
      let mut results = Vec::new();

      // Parse result divs - this is simplified
      // Real implementation would use proper HTML parsing

      results.truncate(max_results);
      results
  }
  ```

---

### Sub-phase 2.4: Bing Search Provider (Optional)

**Goal**: Implement Bing Search API as alternative

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for BingSearchProvider creation (3 tests)
- [ ] Write tests for response parsing (3 tests)
- [ ] Write tests for error handling (3 tests)
- [ ] Implement `BingSearchProvider` struct
- [ ] Implement Bing Web Search API v7

**Test Files:**
- Inline tests in `src/search/bing.rs` (~9 tests)

**Implementation Files:**
- `src/search/bing.rs` (max 200 lines)
  ```rust
  // Similar structure to Brave provider
  // Bing API: https://api.bing.microsoft.com/v7.0/search
  ```

---

## Phase 3: Search Service (3 hours)

### Sub-phase 3.1: Search Cache

**Goal**: Implement TTL-based caching for search results

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for cache insertion and retrieval (4 tests)
- [ ] Write tests for TTL expiration (3 tests)
- [ ] Write tests for cache key generation (2 tests)
- [ ] Write tests for cache size limits (2 tests)
- [ ] Implement `SearchCache` struct with HashMap
- [ ] Implement TTL-based expiration
- [ ] Add cache statistics method
- [ ] Add cache clear method

**Test Files:**
- Inline tests in `src/search/cache.rs` (~11 tests)

**Implementation Files:**
- `src/search/cache.rs` (max 200 lines)
  ```rust
  use std::collections::HashMap;
  use std::sync::RwLock;
  use std::time::{Duration, Instant};
  use super::types::SearchResult;

  pub struct SearchCache {
      cache: RwLock<HashMap<String, CachedEntry>>,
      ttl: Duration,
      max_entries: usize,
  }

  struct CachedEntry {
      results: Vec<SearchResult>,
      provider: String,
      inserted_at: Instant,
  }

  impl SearchCache {
      pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
          Self {
              cache: RwLock::new(HashMap::new()),
              ttl: Duration::from_secs(ttl_secs),
              max_entries,
          }
      }

      pub fn get(&self, query: &str) -> Option<(Vec<SearchResult>, String)> {
          let cache = self.cache.read().ok()?;
          let key = Self::cache_key(query);
          let entry = cache.get(&key)?;

          if entry.inserted_at.elapsed() > self.ttl {
              return None;  // Expired
          }

          Some((entry.results.clone(), entry.provider.clone()))
      }

      pub fn insert(&self, query: &str, results: &[SearchResult], provider: &str) {
          let mut cache = match self.cache.write() {
              Ok(c) => c,
              Err(_) => return,
          };

          // Evict oldest if at capacity
          if cache.len() >= self.max_entries {
              self.evict_oldest(&mut cache);
          }

          let key = Self::cache_key(query);
          cache.insert(key, CachedEntry {
              results: results.to_vec(),
              provider: provider.to_string(),
              inserted_at: Instant::now(),
          });
      }

      pub fn clear(&self) {
          if let Ok(mut cache) = self.cache.write() {
              cache.clear();
          }
      }

      pub fn stats(&self) -> CacheStats {
          let cache = self.cache.read().unwrap();
          let total = cache.len();
          let expired = cache.values()
              .filter(|e| e.inserted_at.elapsed() > self.ttl)
              .count();
          CacheStats { total, expired, max: self.max_entries }
      }

      fn cache_key(query: &str) -> String {
          query.to_lowercase().trim().to_string()
      }

      fn evict_oldest(&self, cache: &mut HashMap<String, CachedEntry>) {
          if let Some(oldest_key) = cache.iter()
              .min_by_key(|(_, v)| v.inserted_at)
              .map(|(k, _)| k.clone())
          {
              cache.remove(&oldest_key);
          }
      }
  }

  #[derive(Debug)]
  pub struct CacheStats {
      pub total: usize,
      pub expired: usize,
      pub max: usize,
  }
  ```

---

### Sub-phase 3.2: Rate Limiter

**Goal**: Implement rate limiting for search requests

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for rate limiter allowing requests (3 tests)
- [ ] Write tests for rate limiter blocking excess (3 tests)
- [ ] Write tests for rate limiter reset (2 tests)
- [ ] Implement `SearchRateLimiter` using governor
- [ ] Add per-provider rate limiting
- [ ] Add global rate limiting

**Test Files:**
- Inline tests in `src/search/rate_limiter.rs` (~8 tests)

**Implementation Files:**
- `src/search/rate_limiter.rs` (max 150 lines)
  ```rust
  use governor::{Quota, RateLimiter as GovRateLimiter};
  use governor::clock::DefaultClock;
  use governor::state::{InMemoryState, NotKeyed};
  use std::num::NonZeroU32;
  use std::sync::Arc;
  use super::types::SearchError;

  pub struct SearchRateLimiter {
      limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
      requests_per_minute: u32,
  }

  impl SearchRateLimiter {
      pub fn new(requests_per_minute: u32) -> Self {
          let quota = Quota::per_minute(
              NonZeroU32::new(requests_per_minute).unwrap_or(NonZeroU32::new(60).unwrap())
          );
          let limiter = Arc::new(GovRateLimiter::direct(quota));

          Self {
              limiter,
              requests_per_minute,
          }
      }

      pub fn check(&self) -> Result<(), SearchError> {
          match self.limiter.check() {
              Ok(_) => Ok(()),
              Err(_) => Err(SearchError::RateLimited {
                  retry_after_secs: 60
              }),
          }
      }

      pub async fn wait(&self) {
          self.limiter.until_ready().await;
      }

      pub fn requests_per_minute(&self) -> u32 {
          self.requests_per_minute
      }
  }
  ```

---

### Sub-phase 3.3: Search Service

**Goal**: Orchestrate providers, cache, and rate limiting

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for SearchService creation with config (3 tests)
- [ ] Write tests for single search with cache miss (3 tests)
- [ ] Write tests for single search with cache hit (2 tests)
- [ ] Write tests for provider failover (3 tests)
- [ ] Write tests for batch search (4 tests)
- [ ] Write tests for rate limiting integration (2 tests)
- [ ] Implement `SearchService` struct
- [ ] Implement single `search()` method
- [ ] Implement `batch_search()` for multiple queries
- [ ] Add provider selection and failover logic

**Test Files:**
- Inline tests in `src/search/service.rs` (~17 tests)

**Implementation Files:**
- `src/search/service.rs` (max 350 lines)
  ```rust
  use std::sync::Arc;
  use std::time::Instant;
  use tracing::{debug, warn, info};
  use super::config::SearchConfig;
  use super::provider::SearchProvider;
  use super::brave::BraveSearchProvider;
  use super::duckduckgo::DuckDuckGoProvider;
  use super::cache::SearchCache;
  use super::rate_limiter::SearchRateLimiter;
  use super::types::{SearchResult, SearchResponse, SearchError};

  pub struct SearchService {
      providers: Vec<Box<dyn SearchProvider>>,
      cache: SearchCache,
      rate_limiter: SearchRateLimiter,
      config: SearchConfig,
  }

  impl SearchService {
      pub fn new(config: SearchConfig) -> Self {
          let mut providers: Vec<Box<dyn SearchProvider>> = Vec::new();

          // Add Brave if configured
          if let Some(ref api_key) = config.providers.brave_api_key {
              providers.push(Box::new(BraveSearchProvider::new(api_key.clone())));
          }

          // Always add DuckDuckGo as fallback
          providers.push(Box::new(DuckDuckGoProvider::new()));

          // Sort by priority
          providers.sort_by_key(|p| p.priority());

          let cache = SearchCache::new(config.cache_ttl_secs, 1000);
          let rate_limiter = SearchRateLimiter::new(config.rate_limit_per_minute);

          Self {
              providers,
              cache,
              rate_limiter,
              config,
          }
      }

      pub async fn search(
          &self,
          query: &str,
          num_results: Option<usize>,
      ) -> Result<SearchResponse, SearchError> {
          if !self.config.enabled {
              return Err(SearchError::SearchDisabled);
          }

          let num_results = num_results.unwrap_or(self.config.default_num_results);

          // Check cache first
          if let Some((results, provider)) = self.cache.get(query) {
              debug!("Cache hit for query: {}", query);
              return Ok(SearchResponse {
                  query: query.to_string(),
                  results: results.clone(),
                  search_time_ms: 0,
                  provider,
                  cached: true,
                  result_count: results.len(),
              });
          }

          // Rate limit check
          self.rate_limiter.check()?;

          let start = Instant::now();

          // Try providers in order
          for provider in &self.providers {
              if !provider.is_available() {
                  continue;
              }

              debug!("Trying search provider: {}", provider.name());

              match provider.search(query, num_results).await {
                  Ok(results) => {
                      let elapsed_ms = start.elapsed().as_millis() as u64;

                      // Cache successful results
                      self.cache.insert(query, &results, provider.name());

                      info!(
                          "Search complete: {} results from {} in {}ms",
                          results.len(),
                          provider.name(),
                          elapsed_ms
                      );

                      return Ok(SearchResponse {
                          query: query.to_string(),
                          results: results.clone(),
                          search_time_ms: elapsed_ms,
                          provider: provider.name().to_string(),
                          cached: false,
                          result_count: results.len(),
                      });
                  }
                  Err(e) => {
                      warn!(
                          "Search provider {} failed: {}, trying next",
                          provider.name(),
                          e
                      );
                      continue;
                  }
              }
          }

          Err(SearchError::ProviderUnavailable {
              provider: "all".to_string(),
          })
      }

      pub async fn batch_search(
          &self,
          queries: Vec<String>,
          num_results_per_query: Option<usize>,
      ) -> Vec<Result<SearchResponse, SearchError>> {
          let futures: Vec<_> = queries
              .into_iter()
              .map(|q| self.search(&q, num_results_per_query))
              .collect();

          futures::future::join_all(futures).await
      }

      pub fn is_enabled(&self) -> bool {
          self.config.enabled
      }

      pub fn available_providers(&self) -> Vec<&str> {
          self.providers
              .iter()
              .filter(|p| p.is_available())
              .map(|p| p.name())
              .collect()
      }

      pub fn cache_stats(&self) -> super::cache::CacheStats {
          self.cache.stats()
      }
  }
  ```

---

## Phase 4: API Integration (3 hours)

### Sub-phase 4.1: Request/Response Types

**Goal**: Define API request and response types

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for SearchApiRequest serialization (4 tests)
- [ ] Write tests for SearchApiRequest validation (5 tests)
- [ ] Write tests for SearchApiResponse serialization (3 tests)
- [ ] Implement `SearchApiRequest` struct with validation
- [ ] Implement `SearchApiResponse` struct

**Test Files:**
- Inline tests in `src/api/search/request.rs` (~9 tests)
- Inline tests in `src/api/search/response.rs` (~3 tests)

**Implementation Files:**
- `src/api/search/request.rs` (max 150 lines)
  ```rust
  use serde::{Deserialize, Serialize};

  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct SearchApiRequest {
      /// Search query string
      pub query: String,

      /// Number of results to return (1-20, default 10)
      #[serde(default = "default_num_results")]
      pub num_results: usize,

      /// Chain ID for billing context
      #[serde(default = "default_chain_id")]
      pub chain_id: u64,

      /// Optional request ID for tracking
      #[serde(skip_serializing_if = "Option::is_none")]
      pub request_id: Option<String>,
  }

  fn default_num_results() -> usize { 10 }
  fn default_chain_id() -> u64 { 84532 }

  impl SearchApiRequest {
      pub fn validate(&self) -> Result<(), String> {
          if self.query.trim().is_empty() {
              return Err("Query cannot be empty".to_string());
          }
          if self.query.len() > 500 {
              return Err("Query too long (max 500 chars)".to_string());
          }
          if self.num_results < 1 || self.num_results > 20 {
              return Err("num_results must be between 1 and 20".to_string());
          }
          Ok(())
      }
  }
  ```

- `src/api/search/response.rs` (max 100 lines)
  ```rust
  use serde::{Deserialize, Serialize};
  use crate::search::types::SearchResult;

  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct SearchApiResponse {
      pub query: String,
      pub results: Vec<SearchResult>,
      pub result_count: usize,
      pub search_time_ms: u64,
      pub provider: String,
      pub cached: bool,
      pub chain_id: u64,
      pub chain_name: String,
  }

  impl SearchApiResponse {
      pub fn new(
          query: String,
          results: Vec<SearchResult>,
          search_time_ms: u64,
          provider: String,
          cached: bool,
          chain_id: u64,
      ) -> Self {
          let chain_name = match chain_id {
              84532 => "Base Sepolia".to_string(),
              5611 => "opBNB Testnet".to_string(),
              _ => "Unknown".to_string(),
          };

          Self {
              result_count: results.len(),
              query,
              results,
              search_time_ms,
              provider,
              cached,
              chain_id,
              chain_name,
          }
      }
  }
  ```

---

### Sub-phase 4.2: Search Handler

**Goal**: Implement POST /v1/search HTTP handler

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for successful search (3 tests)
- [ ] Write tests for validation errors (4 tests)
- [ ] Write tests for search disabled (2 tests)
- [ ] Write tests for provider errors (2 tests)
- [ ] Implement `search_handler` function
- [ ] Add AppState integration for SearchService
- [ ] Add proper error response codes

**Test Files:**
- `tests/api/test_search_endpoint.rs` (max 300 lines)

**Implementation Files:**
- `src/api/search/handler.rs` (max 200 lines)
  ```rust
  use axum::{
      extract::State,
      http::StatusCode,
      Json,
  };
  use tracing::{debug, warn, info};
  use crate::api::http_server::AppState;
  use super::request::SearchApiRequest;
  use super::response::SearchApiResponse;

  /// POST /v1/search - Perform web search
  ///
  /// # Request
  /// - `query`: Search query string (required, max 500 chars)
  /// - `numResults`: Number of results (1-20, default 10)
  /// - `chainId`: Chain ID for billing (default 84532)
  ///
  /// # Response
  /// - `results`: Array of search results
  /// - `searchTimeMs`: Time taken for search
  /// - `provider`: Search provider used
  /// - `cached`: Whether result was from cache
  ///
  /// # Errors
  /// - 400 Bad Request: Invalid query or parameters
  /// - 503 Service Unavailable: Search disabled or no providers
  /// - 429 Too Many Requests: Rate limited
  pub async fn search_handler(
      State(state): State<AppState>,
      Json(request): Json<SearchApiRequest>,
  ) -> Result<Json<SearchApiResponse>, (StatusCode, String)> {
      debug!("Search request: {:?}", request.query);

      // Validate request
      if let Err(e) = request.validate() {
          warn!("Search validation failed: {}", e);
          return Err((StatusCode::BAD_REQUEST, e));
      }

      // Get search service
      let search_service = state.search_service.read().await;
      let search_service = search_service.as_ref().ok_or_else(|| {
          (StatusCode::SERVICE_UNAVAILABLE, "Search service not available".to_string())
      })?;

      if !search_service.is_enabled() {
          return Err((
              StatusCode::SERVICE_UNAVAILABLE,
              "Web search is disabled on this host".to_string(),
          ));
      }

      // Perform search
      let result = search_service
          .search(&request.query, Some(request.num_results))
          .await
          .map_err(|e| {
              match &e {
                  crate::search::types::SearchError::RateLimited { .. } => {
                      (StatusCode::TOO_MANY_REQUESTS, e.to_string())
                  }
                  crate::search::types::SearchError::SearchDisabled => {
                      (StatusCode::SERVICE_UNAVAILABLE, e.to_string())
                  }
                  _ => {
                      (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                  }
              }
          })?;

      info!(
          "Search complete: {} results for '{}' in {}ms (cached: {})",
          result.result_count,
          request.query,
          result.search_time_ms,
          result.cached
      );

      Ok(Json(SearchApiResponse::new(
          result.query,
          result.results,
          result.search_time_ms,
          result.provider,
          result.cached,
          request.chain_id,
      )))
  }
  ```

---

### Sub-phase 4.3: Update AppState

**Goal**: Add SearchService to AppState

**Status**: NOT STARTED

#### Tasks
- [ ] Add `search_service` field to AppState
- [ ] Add setter method for search service
- [ ] Update AppState::new_for_test() to include search service
- [ ] Add /v1/search route to create_app()

**Implementation Files:**
- `src/api/http_server.rs` (modify)
  ```rust
  #[derive(Clone)]
  pub struct AppState {
      // ... existing fields ...
      pub search_service: Arc<RwLock<Option<Arc<SearchService>>>>,
  }
  ```

- `src/api/server.rs` (modify)
  ```rust
  impl ApiServer {
      pub async fn set_search_service(&self, service: Arc<SearchService>) {
          let mut guard = self.state.search_service.write().await;
          *guard = Some(service);
      }
  }
  ```

---

### Sub-phase 4.4: Chat Integration

**Goal**: Add web_search flag to chat/inference requests

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for chat with web_search=false (2 tests)
- [ ] Write tests for chat with web_search=true (3 tests)
- [ ] Write tests for search result injection into prompt (2 tests)
- [ ] Add `web_search` field to ChatRequest
- [ ] Add `max_searches` field to ChatRequest
- [ ] Modify chat handler to perform search before inference
- [ ] Implement prompt augmentation with search results

**Test Files:**
- Tests in existing chat handler test file (~7 tests)

**Implementation Files:**
- `src/api/chat/request.rs` (modify)
  ```rust
  pub struct ChatRequest {
      // ... existing fields ...

      /// Enable web search before inference
      #[serde(default)]
      pub web_search: bool,

      /// Maximum searches to perform (default 5)
      #[serde(default = "default_max_searches")]
      pub max_searches: u32,

      /// Custom search queries (optional, auto-extracted if not provided)
      #[serde(skip_serializing_if = "Option::is_none")]
      pub search_queries: Option<Vec<String>>,
  }
  ```

---

## Phase 5: WebSocket Integration (2 hours)

### Sub-phase 5.1: WebSocket Message Types

**Goal**: Add search-related WebSocket messages

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for SearchRequest message serialization (2 tests)
- [ ] Write tests for SearchResults message serialization (2 tests)
- [ ] Write tests for SearchProgress message serialization (2 tests)
- [ ] Add `SearchRequest` client message type
- [ ] Add `SearchResults` server message type
- [ ] Add `SearchStarted` server message type
- [ ] Add `SearchError` server message type

**Test Files:**
- Inline tests in WebSocket message files (~6 tests)

**Implementation Files:**
- `src/api/websocket/messages.rs` (modify)
  ```rust
  #[derive(Debug, Serialize, Deserialize)]
  #[serde(tag = "type")]
  pub enum ClientMessage {
      // ... existing variants ...

      /// Request a web search
      SearchRequest {
          query: String,
          num_results: Option<u32>,
          request_id: String,
      },

      /// Chat with web search enabled
      ChatWithSearch {
          messages: Vec<Message>,
          web_search: bool,
          max_searches: Option<u32>,
          request_id: String,
      },
  }

  #[derive(Debug, Serialize, Deserialize)]
  #[serde(tag = "type")]
  pub enum ServerMessage {
      // ... existing variants ...

      /// Search results
      SearchResults {
          request_id: String,
          query: String,
          results: Vec<SearchResult>,
          search_time_ms: u64,
          provider: String,
          cached: bool,
      },

      /// Search started notification
      SearchStarted {
          request_id: String,
          query: String,
      },

      /// Search error
      SearchError {
          request_id: String,
          error: String,
      },
  }
  ```

---

### Sub-phase 5.2: WebSocket Handler Integration

**Goal**: Handle search messages in WebSocket

**Status**: NOT STARTED

#### Tasks
- [ ] Write tests for WebSocket search request handling (3 tests)
- [ ] Write tests for WebSocket search error handling (2 tests)
- [ ] Add search request handling to WebSocket message handler
- [ ] Send SearchStarted before search
- [ ] Send SearchResults or SearchError after completion

**Test Files:**
- Tests in WebSocket handler test file (~5 tests)

**Implementation Files:**
- `src/api/websocket/handler.rs` (modify)

---

## Phase 6: Deep Research Agent (4 hours) - OPTIONAL

### Sub-phase 6.1: Research Module Structure

**Goal**: Create research agent module structure

**Status**: NOT STARTED (OPTIONAL PHASE)

#### Tasks
- [ ] Create `src/research/mod.rs`
- [ ] Create `src/research/agent.rs` stub
- [ ] Create `src/research/planner.rs` stub
- [ ] Create `src/research/synthesizer.rs` stub
- [ ] Add `pub mod research;` to `src/lib.rs`

---

### Sub-phase 6.2: Research Planner

**Goal**: Use LLM to generate research plans

**Status**: NOT STARTED (OPTIONAL PHASE)

#### Tasks
- [ ] Write tests for research plan generation (4 tests)
- [ ] Write tests for query extraction (3 tests)
- [ ] Implement `ResearchPlanner` struct
- [ ] Implement LLM prompt for query generation
- [ ] Parse LLM output into search queries

---

### Sub-phase 6.3: Research Agent

**Goal**: Implement agentic research loop

**Status**: NOT STARTED (OPTIONAL PHASE)

#### Tasks
- [ ] Write tests for research agent creation (2 tests)
- [ ] Write tests for single iteration (3 tests)
- [ ] Write tests for multi-iteration with gap detection (4 tests)
- [ ] Write tests for max iteration limit (2 tests)
- [ ] Implement `ResearchAgent` struct
- [ ] Implement research loop with progress callbacks
- [ ] Implement gap detection via LLM
- [ ] Implement final synthesis

---

### Sub-phase 6.4: Research API Endpoint

**Goal**: Implement POST /v1/research endpoint

**Status**: NOT STARTED (OPTIONAL PHASE)

#### Tasks
- [ ] Write tests for research endpoint (5 tests)
- [ ] Implement `research_handler` function
- [ ] Add streaming progress via WebSocket

---

## Phase 7: Integration & Documentation (2 hours)

### Sub-phase 7.1: Main.rs Integration

**Goal**: Initialize SearchService in main.rs

**Status**: NOT STARTED

#### Tasks
- [ ] Add SearchService initialization to main.rs
- [ ] Load SearchConfig from environment
- [ ] Log search availability at startup
- [ ] Handle missing API keys gracefully

**Implementation Files:**
- `src/main.rs` (modify)
  ```rust
  // Initialize Search Service (optional)
  println!("Initializing search service...");

  let search_config = SearchConfig::from_env();

  if search_config.enabled {
      match search_config.validate() {
          Ok(_) => {
              let search_service = SearchService::new(search_config);
              let providers = search_service.available_providers();
              println!("Search enabled with providers: {:?}", providers);
              api_server.set_search_service(Arc::new(search_service)).await;
          }
          Err(e) => {
              println!("Search configuration invalid: {}", e);
              println!("/v1/search will return 503");
          }
      }
  } else {
      println!("Web search disabled (set WEB_SEARCH_ENABLED=true to enable)");
  }
  ```

---

### Sub-phase 7.2: Update API Documentation

**Goal**: Document new endpoints in API.md

**Status**: NOT STARTED

#### Tasks
- [ ] Add POST /v1/search documentation
- [ ] Add web_search parameter to chat documentation
- [ ] Add WebSocket search message documentation
- [ ] Add configuration documentation
- [ ] Add request/response examples

---

### Sub-phase 7.3: Update Version

**Goal**: Update version information

**Status**: NOT STARTED

#### Tasks
- [ ] Update VERSION file to `8.7.0-web-search`
- [ ] Update src/version.rs with new version constants
- [ ] Add new features to FEATURES array (web-search, brave-search, research-agent)
- [ ] Update BREAKING_CHANGES if needed

---

### Sub-phase 7.4: Host Capability Advertisement

**Goal**: Advertise search capability to network

**Status**: NOT STARTED

#### Tasks
- [ ] Add `web_search_enabled` to host capabilities
- [ ] Add `search_providers` list to capabilities
- [ ] Update /v1/info endpoint with search status
- [ ] Document capability advertisement

---

## Test Summary

| Phase | Test File | Test Count |
|-------|-----------|------------|
| 1.3 | types.rs | ~12 |
| 1.4 | config.rs | ~11 |
| 2.1 | provider.rs | ~4 |
| 2.2 | brave.rs | ~15 |
| 2.3 | duckduckgo.rs | ~8 |
| 2.4 | bing.rs | ~9 |
| 3.1 | cache.rs | ~11 |
| 3.2 | rate_limiter.rs | ~8 |
| 3.3 | service.rs | ~17 |
| 4.1 | request.rs, response.rs | ~12 |
| 4.2 | test_search_endpoint.rs | ~11 |
| 4.4 | chat integration | ~7 |
| 5.1-5.2 | websocket | ~11 |
| 6.x | research (optional) | ~18 |
| **Total** | | **~144 tests** |

---

## Performance Targets

| Operation | Target | Notes |
|-----------|--------|-------|
| Single search | <2s | Including network latency |
| Cached search | <10ms | In-memory cache hit |
| Batch search (10 queries) | <5s | Parallel execution |
| Deep research (50 searches) | <60s | With synthesis |

---

## Security Considerations

| Concern | Mitigation |
|---------|------------|
| API key exposure | Store in environment variables, never log |
| Search query injection | Validate and sanitize queries |
| Rate limit abuse | Per-session and global rate limits |
| Cost attacks | Max searches per request/session limits |
| Privacy | Use privacy-respecting providers (Brave, DDG) |

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Brave API rate limits | Fallback to DuckDuckGo |
| Search API costs | Aggressive caching, rate limiting |
| Provider unavailability | Multiple provider failover |
| Large result payloads | Limit snippet length, result count |
| Slow searches | Timeout handling, graceful degradation |

---

## Pricing Integration Notes

Web searches should be factored into job pricing:

```rust
// Suggested pricing model
pub struct SearchPricing {
    /// Token equivalent per search (e.g., 100 tokens)
    pub tokens_per_search: u64,

    /// Whether search cost is separate or included in token price
    pub separate_billing: bool,
}

// In job completion
pub struct JobMetrics {
    pub tokens_generated: u64,
    pub searches_performed: u32,
    pub total_token_equivalent: u64,  // tokens + (searches * tokens_per_search)
}
```

---

## Future Enhancements (Out of Scope)

1. **Search result verification** - Hash results for proof submission
2. **Specialized search** - News, images, academic papers
3. **Search history** - Per-session search history for context
4. **Custom search providers** - Allow hosts to add custom providers
5. **Search result ranking** - Re-rank results based on relevance
