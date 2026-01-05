# SDK Web Search Integration Guide

**Version**: v8.7.0+
**Status**: Production Ready
**Last Updated**: January 2026

## Overview

Starting with v8.7.0, Fabstir LLM Nodes support **host-side web search** - a powerful feature that allows hosts to perform web searches on behalf of clients. This enables LLMs to access real-time information without requiring clients to implement search functionality or consume their own bandwidth.

### Why Host-Side Web Search?

| Benefit | Description |
|---------|-------------|
| **Zero Client Setup** | DuckDuckGo works out of the box - no API keys needed |
| **Bandwidth Savings** | Host fetches search results, not the client |
| **Real-Time Data** | LLMs can answer questions about current events |
| **Decentralized** | Search is performed by the P2P host, not a central server |
| **Transparent Billing** | Search costs are part of the inference job |

### Supported Providers

| Provider | API Key Required | Quality | Notes |
|----------|------------------|---------|-------|
| **DuckDuckGo** | No | Good | Always available as fallback |
| **Brave** | Yes | Excellent | Best results, rate-limited |
| **Bing** | Yes | Very Good | Microsoft API |

---

## Feature Detection

Before using web search, verify the host supports it:

### Check Version Endpoint

```javascript
const response = await fetch('http://host:8080/v1/version');
const version = await response.json();

const supportsWebSearch = version.features.includes('host-side-web-search');
const supportsInferenceSearch = version.features.includes('inference-web-search');

console.log('Web search supported:', supportsWebSearch);
console.log('Inference search supported:', supportsInferenceSearch);
```

### Feature Flags to Check

```javascript
const WEB_SEARCH_FEATURES = [
  'host-side-web-search',    // /v1/search endpoint available
  'inference-web-search',    // web_search field in inference
  'brave-search-api',        // Brave provider available (if API key set)
  'duckduckgo-fallback',     // DuckDuckGo always available
  'search-caching',          // Results are cached
  'search-rate-limiting'     // Rate limiting is enforced
];
```

---

## Integration Path A: Direct Search API

Use `/v1/search` when you need search results directly - for search UIs, pre-fetching context, or research tools.

### Request Format

```typescript
interface SearchRequest {
  query: string;           // Required: Search query (max 500 chars)
  numResults?: number;     // Optional: 1-20, default 10
  chainId?: number;        // Optional: default 84532 (Base Sepolia)
  requestId?: string;      // Optional: for tracking
}
```

### Response Format

```typescript
interface SearchResponse {
  query: string;
  results: SearchResult[];
  resultCount: number;
  searchTimeMs: number;
  provider: 'brave' | 'duckduckgo' | 'bing';
  cached: boolean;
  chainId: number;
  chainName: string;
}

interface SearchResult {
  title: string;
  url: string;
  snippet: string;
  source: string;
  published_date?: string;  // May be null
}
```

### JavaScript Example

```javascript
async function search(query, numResults = 5) {
  const response = await fetch('http://host:8080/v1/search', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      query,
      numResults,
      chainId: 84532
    })
  });

  if (!response.ok) {
    const error = await response.json();
    throw new SearchError(response.status, error.error);
  }

  return response.json();
}

// Usage
const results = await search('latest AI news', 5);
console.log(`Found ${results.resultCount} results in ${results.searchTimeMs}ms`);
console.log(`Provider: ${results.provider}, Cached: ${results.cached}`);

results.results.forEach(r => {
  console.log(`- ${r.title}: ${r.url}`);
});
```

### Error Handling

```javascript
class SearchError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
    this.name = 'SearchError';
  }
}

async function searchWithRetry(query, maxRetries = 3) {
  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      return await search(query);
    } catch (error) {
      if (error.status === 429) {
        // Rate limited - wait and retry
        const waitMs = Math.pow(2, attempt) * 1000;
        console.log(`Rate limited, waiting ${waitMs}ms...`);
        await new Promise(r => setTimeout(r, waitMs));
        continue;
      }
      if (error.status === 503) {
        // Service unavailable - search disabled on this host
        throw new Error('Web search is disabled on this host');
      }
      throw error;
    }
  }
  throw new Error('Max retries exceeded');
}
```

---

## Integration Path B: Search-Augmented Inference

Use `web_search: true` in inference requests when you want the LLM to automatically use search results. The host handles everything - searching, formatting results, and injecting them into the prompt.

### Request Format

```typescript
interface InferenceRequest {
  model: string;
  prompt: string;
  max_tokens: number;
  temperature?: number;

  // Web search fields (v8.7.0+)
  web_search?: boolean;           // Enable search (default: false)
  max_searches?: number;          // Max queries, 1-20 (default: 5)
  search_queries?: string[];      // Custom queries (optional)

  // Standard fields
  chain_id?: number;
  job_id?: number;
  session_id?: string;
  request_id?: string;
}
```

### Response Format

```typescript
interface InferenceResponse {
  model: string;
  content: string;
  tokens_used: number;
  finish_reason: string;
  request_id: string;
  chain_id: number;
  chain_name: string;
  native_token: string;

  // Web search metadata (v8.7.0+)
  web_search_performed?: boolean;   // null if not requested
  search_queries_count?: number;    // null if not searched
  search_provider?: string;         // null if not searched
}
```

### JavaScript Example

```javascript
async function inferenceWithSearch(prompt, options = {}) {
  const response = await fetch('http://host:8080/v1/inference', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      model: options.model || 'llama-2-7b',
      prompt,
      max_tokens: options.maxTokens || 500,
      temperature: options.temperature || 0.7,

      // Enable web search
      web_search: true,
      max_searches: options.maxSearches || 3,
      search_queries: options.searchQueries || null,

      chain_id: options.chainId || 84532
    })
  });

  if (!response.ok) {
    throw new Error(`Inference failed: ${response.status}`);
  }

  const result = await response.json();

  // Log search metadata
  if (result.web_search_performed) {
    console.log(`Search performed: ${result.search_queries_count} queries via ${result.search_provider}`);
  }

  return result;
}

// Usage - let host extract search query from prompt
const result1 = await inferenceWithSearch(
  'What are the latest developments in quantum computing?'
);

// Usage - provide custom search queries
const result2 = await inferenceWithSearch(
  'Compare and summarize these topics',
  {
    searchQueries: ['quantum computing 2026', 'AI breakthroughs 2026'],
    maxSearches: 2
  }
);
```

### When to Use Custom Search Queries

| Scenario | Recommendation |
|----------|----------------|
| Simple question | Let host extract from prompt |
| Multi-topic comparison | Provide custom queries |
| Specific sources needed | Provide targeted queries |
| Follow-up questions | Provide context-aware queries |

```javascript
// Let host extract (good for simple questions)
await inferenceWithSearch('What is the capital of France?');

// Custom queries (good for comparisons)
await inferenceWithSearch('Compare these AI models', {
  searchQueries: ['GPT-4 capabilities', 'Claude 3 capabilities', 'Gemini capabilities']
});

// Targeted queries (good for specific info)
await inferenceWithSearch('Summarize recent news', {
  searchQueries: ['site:reuters.com AI news January 2026']
});
```

---

## Integration Path C: WebSocket Search

For real-time applications, use WebSocket messages to perform searches within a conversation flow.

### Message Types

```typescript
// Client -> Server
interface SearchRequest {
  type: 'searchRequest';
  query: string;
  num_results?: number;    // 1-20, default 10
  request_id?: string;
}

// Server -> Client
interface SearchStarted {
  type: 'searchStarted';
  query: string;
  request_id?: string;
  provider: string;
}

// Server -> Client
interface SearchResults {
  type: 'searchResults';
  query: string;
  results: SearchResult[];
  result_count: number;
  search_time_ms: number;
  provider: string;
  cached: boolean;
  request_id?: string;
}

// Server -> Client
interface SearchError {
  type: 'searchError';
  error: string;
  error_code: string;
  request_id?: string;
}
```

### JavaScript Example

```javascript
class FabstirWebSocket {
  constructor(url) {
    this.ws = new WebSocket(url);
    this.pendingSearches = new Map();

    this.ws.onmessage = (event) => {
      const msg = JSON.parse(event.data);
      this.handleMessage(msg);
    };
  }

  handleMessage(msg) {
    switch (msg.type) {
      case 'searchStarted':
        console.log(`Search started: "${msg.query}" via ${msg.provider}`);
        break;

      case 'searchResults':
        const resolver = this.pendingSearches.get(msg.request_id);
        if (resolver) {
          resolver.resolve(msg);
          this.pendingSearches.delete(msg.request_id);
        }
        break;

      case 'searchError':
        const rejecter = this.pendingSearches.get(msg.request_id);
        if (rejecter) {
          rejecter.reject(new Error(`${msg.error_code}: ${msg.error}`));
          this.pendingSearches.delete(msg.request_id);
        }
        break;
    }
  }

  async search(query, numResults = 5) {
    const requestId = `search-${Date.now()}`;

    return new Promise((resolve, reject) => {
      this.pendingSearches.set(requestId, { resolve, reject });

      this.ws.send(JSON.stringify({
        type: 'searchRequest',
        query,
        num_results: numResults,
        request_id: requestId
      }));

      // Timeout after 30 seconds
      setTimeout(() => {
        if (this.pendingSearches.has(requestId)) {
          this.pendingSearches.delete(requestId);
          reject(new Error('Search timeout'));
        }
      }, 30000);
    });
  }
}

// Usage
const ws = new FabstirWebSocket('ws://host:8080/v1/ws');

// Wait for connection
await new Promise(r => ws.ws.onopen = r);

// Perform search
const results = await ws.search('AI news today', 5);
console.log(`Found ${results.result_count} results`);
```

### Error Codes

| Code | Description | Action |
|------|-------------|--------|
| `search_disabled` | Web search disabled on host | Use different host or disable search |
| `invalid_query` | Query empty or too long | Fix query (max 500 chars) |
| `rate_limited` | Too many requests | Wait and retry with backoff |
| `provider_error` | Search provider failed | Retry or use fallback |
| `timeout` | Search took too long | Retry with shorter query |
| `no_providers` | No search providers available | Host misconfiguration |

---

## TypeScript Interfaces

Add these to your SDK's type definitions:

```typescript
// Request types
export interface SearchApiRequest {
  query: string;
  numResults?: number;
  chainId?: number;
  requestId?: string;
}

export interface InferenceRequestWithSearch {
  model: string;
  prompt: string;
  max_tokens: number;
  temperature?: number;
  stream?: boolean;
  request_id?: string;
  job_id?: number;
  session_id?: string;
  chain_id?: number;

  // Web search (v8.7.0+)
  web_search?: boolean;
  max_searches?: number;
  search_queries?: string[];
}

// Response types
export interface SearchApiResponse {
  query: string;
  results: SearchResultItem[];
  resultCount: number;
  searchTimeMs: number;
  provider: 'brave' | 'duckduckgo' | 'bing';
  cached: boolean;
  chainId: number;
  chainName: string;
}

export interface SearchResultItem {
  title: string;
  url: string;
  snippet: string;
  source: string;
  published_date?: string | null;
}

export interface InferenceResponseWithSearch {
  model: string;
  content: string;
  tokens_used: number;
  finish_reason: string;
  request_id: string;
  chain_id: number;
  chain_name: string;
  native_token: string;

  // Web search metadata (v8.7.0+)
  web_search_performed?: boolean | null;
  search_queries_count?: number | null;
  search_provider?: string | null;
}

// WebSocket message types
export type WebSearchMessageType =
  | 'searchRequest'
  | 'searchStarted'
  | 'searchResults'
  | 'searchError';

export interface WebSearchRequest {
  type: 'searchRequest';
  query: string;
  num_results?: number;
  request_id?: string;
}

export interface WebSearchStarted {
  type: 'searchStarted';
  query: string;
  request_id?: string;
  provider: string;
}

export interface WebSearchResults {
  type: 'searchResults';
  query: string;
  results: SearchResultItem[];
  result_count: number;
  search_time_ms: number;
  provider: string;
  cached: boolean;
  request_id?: string;
}

export interface WebSearchError {
  type: 'searchError';
  error: string;
  error_code: 'search_disabled' | 'invalid_query' | 'rate_limited' |
              'provider_error' | 'timeout' | 'no_providers';
  request_id?: string;
}
```

---

## Best Practices

### 1. Check for Search Support

```javascript
async function createClient(hostUrl) {
  const version = await fetch(`${hostUrl}/v1/version`).then(r => r.json());

  return {
    hostUrl,
    supportsSearch: version.features.includes('host-side-web-search'),
    supportsInferenceSearch: version.features.includes('inference-web-search'),
    version: version.version
  };
}
```

### 2. Handle Search Failures Gracefully

Search failures should not break inference - the LLM can still respond without search results:

```javascript
async function smartInference(prompt, options = {}) {
  try {
    // Try with search
    return await inferenceWithSearch(prompt, { ...options, web_search: true });
  } catch (error) {
    if (error.message.includes('search')) {
      console.warn('Search failed, falling back to inference without search');
      // Retry without search
      return await inference(prompt, { ...options, web_search: false });
    }
    throw error;
  }
}
```

### 3. Be Aware of Caching

Search results are cached for 1 hour by default. Check the `cached` field:

```javascript
const results = await search('AI news');

if (results.cached) {
  console.log('Results from cache - may not be latest');
  // Consider adding time context to prompt
} else {
  console.log('Fresh results');
}
```

### 4. Optimize Queries

| Do | Don't |
|----|-------|
| Keep queries under 200 chars | Use entire prompt as query |
| Use specific keywords | Use vague/broad terms |
| Provide multiple targeted queries | Use one long query |
| Match language to content | Mix languages unnecessarily |

### 5. Rate Limiting Awareness

```javascript
const RATE_LIMIT = 60; // requests per minute (default)
const requestTimes = [];

function canMakeRequest() {
  const now = Date.now();
  const oneMinuteAgo = now - 60000;

  // Remove old requests
  while (requestTimes.length && requestTimes[0] < oneMinuteAgo) {
    requestTimes.shift();
  }

  return requestTimes.length < RATE_LIMIT;
}

async function rateLimitedSearch(query) {
  if (!canMakeRequest()) {
    throw new Error('Local rate limit reached');
  }

  requestTimes.push(Date.now());
  return await search(query);
}
```

---

## Migration Guide

If your SDK already has client-side search, here's how to migrate to host-side search:

### Before (Client-Side Search)

```javascript
// Old approach - client does search
const searchResults = await clientSearch(query); // Client bandwidth
const context = formatSearchResults(searchResults);
const response = await inference({
  prompt: `Context:\n${context}\n\nQuestion: ${prompt}`,
  max_tokens: 500
});
```

### After (Host-Side Search)

```javascript
// New approach - host does search
const response = await inference({
  prompt: prompt,  // No need to include search context
  max_tokens: 500,
  web_search: true,
  max_searches: 3
});

// Search metadata in response
if (response.web_search_performed) {
  console.log(`Host searched via ${response.search_provider}`);
}
```

### SDK Changes Required

1. **Add new request fields**: `web_search`, `max_searches`, `search_queries`
2. **Add new response fields**: `web_search_performed`, `search_queries_count`, `search_provider`
3. **Add `/v1/search` endpoint support** (optional, for direct search)
4. **Add WebSocket search message types** (if using WebSocket)
5. **Update TypeScript types** (see interfaces above)

---

## Testing

### Test Direct Search

```bash
curl -X POST http://localhost:8080/v1/search \
  -H 'Content-Type: application/json' \
  -d '{"query": "test query", "numResults": 3}'
```

### Test Inference with Search

```bash
curl -X POST http://localhost:8080/v1/inference \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "llama-2-7b",
    "prompt": "What is the capital of France?",
    "max_tokens": 100,
    "web_search": true
  }'
```

### Expected Response

```json
{
  "model": "llama-2-7b",
  "content": "Based on the search results, Paris is the capital of France...",
  "tokens_used": 45,
  "finish_reason": "complete",
  "web_search_performed": true,
  "search_queries_count": 1,
  "search_provider": "duckduckgo"
}
```

---

## Troubleshooting

| Issue | Cause | Solution |
|-------|-------|----------|
| `results: []` | Provider blocked or changed | Check host logs, try different query |
| `503 Service Unavailable` | Search disabled | Check `WEB_SEARCH_ENABLED` on host |
| `429 Too Many Requests` | Rate limited | Implement exponential backoff |
| `web_search_performed: null` | `web_search` not set | Add `web_search: true` to request |
| Search results not in response | Different response path | Check `web_search_performed` field |

---

## Environment Variables (Host Configuration)

SDK developers should be aware of these host-side settings:

| Variable | Default | Description |
|----------|---------|-------------|
| `WEB_SEARCH_ENABLED` | `true` | Enable/disable search |
| `BRAVE_API_KEY` | - | Brave Search API key |
| `BING_API_KEY` | - | Bing Search API key |
| `SEARCH_CACHE_TTL_SECS` | `3600` | Cache TTL (1 hour) |
| `SEARCH_RATE_LIMIT_PER_MINUTE` | `60` | Rate limit |
| `MAX_SEARCHES_PER_REQUEST` | `20` | Max queries per request |

---

## Questions?

For SDK integration support:
- Review the [Node API Documentation](../API.md#web-search-v870)
- Check the [WebSocket API Guide](./WEBSOCKET_API_SDK_GUIDE.md)
- File issues at: https://github.com/anthropics/claude-code/issues
