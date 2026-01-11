// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.11.0-checkpoint-publishing-2026-01-11";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.11.0";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 11;

/// Patch version number
pub const VERSION_PATCH: u32 = 0;

/// Build date
pub const BUILD_DATE: &str = "2026-01-11";

/// Supported features in this version
pub const FEATURES: &[&str] = &[
    "multi-chain",
    "base-sepolia",
    "opbnb-testnet",
    "chain-aware-sessions",
    "auto-settlement",
    "websocket-compression",
    "rate-limiting",
    "job-auth",
    "dual-pricing",
    "native-stable-pricing",
    "price-precision-1000",
    "uups-upgradeable",
    "end-to-end-encryption",
    "ecdh-key-exchange",
    "xchacha20-poly1305",
    "encrypted-sessions",
    "session-key-management",
    "ecdsa-authentication",
    "perfect-forward-secrecy",
    "replay-protection",
    "gpu-stark-proofs",
    "risc0-zkvm",
    "cuda-acceleration",
    "zero-knowledge-proofs",
    "s5-proof-storage",
    "off-chain-proofs",
    "proof-hash-cid",
    "host-side-rag",
    "session-vector-storage",
    "384d-embeddings",
    "cosine-similarity-search",
    "chat-templates",
    "model-specific-formatting",
    "s5-vector-loading",
    "encrypted-vector-database-paths",
    "configurable-batch-size",
    "llama-batch-size-env",
    "async-checkpoints",
    "non-blocking-proof-submission",
    "harmony-chat-template",
    "gpt-oss-20b-support",
    "utf8-content-sanitization",
    "strip-chat-markers",
    "null-byte-sanitization",
    "cpu-ocr",
    "paddleocr-onnx",
    "cpu-vision",
    "florence-2-onnx",
    "image-to-text",
    "image-description",
    "vision-20mb-body-limit",
    // Web search (v8.7.0+)
    "host-side-web-search",
    "brave-search-api",
    "duckduckgo-fallback",
    "bing-search-api",
    "search-caching",
    "search-rate-limiting",
    "inference-web-search",
    // Web search streaming (v8.7.5+)
    "streaming-web-search",
    "websocket-web-search",
    // Auto-detect search intent (v8.7.8+)
    "auto-search-intent-detection",
    // SDK web_search field support (v8.7.9+)
    "sdk-web-search-field",
    // System prompt web search instructions (v8.7.10+)
    "web-search-system-prompt",
    // Search query extraction fix (v8.7.11+)
    "search-query-harmony-cleanup",
    // Improved search prompt (v8.7.12+)
    "search-prompt-v2",
    // Content fetching (v8.8.0+)
    "content-fetching",
    "html-extraction",
    "page-content-cache",
    "parallel-fetch",
    // Security audit proof signing (v8.9.0+)
    "proof-signing",
    "security-audit-compliance",
    "ecdsa-proof-signatures",
    "65-byte-signatures",
    // EIP-191 personal_sign (v8.9.1+)
    "eip191-personal-sign",
    // Content hash binding for proofs (v8.10.0+)
    "content-hash-binding",
    "real-prompt-hash",
    "real-response-hash",
    "proof-witness-content",
    "streaming-response-accumulation",
    // Checkpoint publishing for conversation recovery (v8.11.0+)
    "checkpoint-publishing",
    "conversation-recovery",
    "sdk-checkpoint-recovery",
    "s5-checkpoint-storage",
    "eip191-checkpoint-signatures",
    "sorted-json-keys",
    "session-resumption",
    "ttl-cleanup-policy",
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    // v8.11.0 - Checkpoint Publishing for Conversation Recovery (Jan 11, 2026)
    "FEAT: Checkpoint publishing to S5 for SDK conversation recovery",
    "FEAT: Signed checkpoint deltas with EIP-191 signatures",
    "FEAT: Checkpoint index with session metadata and proof hashes",
    "FEAT: Session resumption from existing S5 checkpoint data",
    "FEAT: TTL-based cleanup policy (7 days completed, 30 days timeout, immediate cancelled)",
    "FEAT: Streaming partial response support in checkpoints",
    "FEAT: JSON keys alphabetically sorted for SDK signature verification",
    // v8.10.5 - Remove Sensitive Logs (Jan 10, 2026)
    "PRIVACY: Removed logging of decrypted message content",
    "PRIVACY: Removed verbose diagnostic eprintln! statements from inference engine",
    "PRIVACY: Log only message lengths, not content",
    // v8.10.3 - Session Store Fix (Jan 10, 2026)
    "FIX: Sessions now created in session_store during session_init and encrypted_session_init",
    "FIX: Resolves 'Session X not found for search' errors in searchVectors and other RAG operations",
    // v8.10.2 - Security Audit Remediation (Jan 9, 2026)
    "CONTRACT: JobMarketplace proxy address changed to 0x3CaCbf3f448B420918A93a88706B26Ab27a3523E",
    "CONTRACT: Clean slate deployment for security audit compliance",
    // v8.10.1 - Incremental Content Hash
    "FIX: Intermediate checkpoints now use partial response hash (not placeholders)",
    "FEAT: All checkpoints use real content hashes during streaming",
    // v8.10.0 - Content Hash Binding
    "FEAT: Proof witness now includes real SHA256 hash of prompt (input_hash)",
    "FEAT: Proof witness now includes real SHA256 hash of response (output_hash)",
    "FEAT: Response tokens accumulated during streaming for final hash computation",
    "FEAT: Backward compatible - falls back to placeholder hashes if content hashes unavailable",
    "FEAT: Logs indicate whether real or placeholder hashes used in proof generation",
    // v8.9.1 - EIP-191 Fix
    "FIX: Proof signatures now use EIP-191 personal_sign prefix (\\x19Ethereum Signed Message:\\n32)",
    "FIX: Signature now matches contract's ecrecover verification",
    // v8.9.0 - Security Audit Proof Signing
    "BREAKING: submitProofOfWork now requires 5th parameter: 65-byte proof signature",
    "FEAT: Proof signing for security audit compliance - prevents token manipulation",
    "FEAT: Host wallet cryptographically signs proof data before submission",
    "FEAT: Signature formula: keccak256(abi.encodePacked(proofHash, hostAddress, tokensClaimed))",
    // v8.8.0 - Content fetching
    "FEAT: Web search now fetches actual page content from URLs, not just snippets",
    "FEAT: HTML content extraction using CSS selectors (article, main, .content, etc.)",
    "FEAT: Content caching with 30-minute TTL to reduce repeated fetches",
    "FEAT: Parallel fetching of up to 3 pages with configurable timeouts",
    "FEAT: SSRF protection - blocks localhost and private IP addresses",
    "FEAT: Graceful fallback to snippets when content fetch fails",
    "FEAT: New env vars: CONTENT_FETCH_ENABLED, CONTENT_FETCH_MAX_PAGES, CONTENT_FETCH_TIMEOUT_SECS",
    // v8.7.12 - Improved search prompt
    "FIX: Stronger system prompt to use [Web Search Results] and never claim 'cannot browse'",
    "FIX: Removed 'You are ChatGPT' and 'Knowledge cutoff' which confused the model",
    "FIX: Added explicit numbered instructions for handling search results",
    // v8.7.11 - Search query extraction fix
    "FIX: Search queries now extract last user message from Harmony chat format",
    "FIX: Strips <|start|>, <|end|>, <|message|> markers before sending to search engine",
    "FIX: Web search no longer returns irrelevant results about GPT-OSS/Harmony documentation",
    // v8.7.10 - System prompt web search instructions
    "FEAT: System prompt now instructs model to use [Web Search Results] when provided",
    "FIX: Model no longer claims 'I cannot browse the web' when search results are available",
    // v8.7.9 - SDK web_search field support
    "FEAT: Node now reads web_search, max_searches, search_queries from encrypted message JSON",
    "FEAT: SDK can explicitly enable web search via web_search: true at message level",
    // v8.7.8 - Auto-detect search intent
    "FEAT: Auto-detect search intent from prompt (triggers on 'search for', 'latest', 'current', etc.)",
    "FEAT: Web search now works without SDK explicitly setting web_search=true",
    // v8.7.5 - Streaming Web Search
    "FEAT: Added web search support to streaming inference (HTTP streaming and WebSocket)",
    "FEAT: WebSocket encrypted sessions now support web_search flag",
    "FEAT: Search context is prepended to prompt before streaming begins",
    // v8.7.0 - Web Search
    "FEAT: Added host-side web search for decentralized AI inference",
    "FEAT: Added POST /v1/search endpoint for direct web search",
    "FEAT: Added web_search, max_searches, search_queries fields to InferenceRequest",
    "FEAT: Added web_search_performed, search_queries_count, search_provider to InferenceResponse",
    "FEAT: Support Brave Search API, Bing Search API, and DuckDuckGo (no API key) providers",
    "FEAT: Added TTL-based search result caching (default 15 minutes)",
    "FEAT: Added search rate limiting (configurable via SEARCH_RATE_LIMIT_PER_MINUTE)",
    "FEAT: Added WEB_SEARCH_ENABLED, BRAVE_API_KEY, BING_API_KEY environment variables",
    "FEAT: Added WebSocket message types: SearchRequest, SearchStarted, SearchResults, SearchError",
    // Previous versions
    "FEAT: Added POST /v1/ocr endpoint for OCR using PaddleOCR (CPU-only)",
    "FEAT: Added POST /v1/describe-image endpoint for image description using Florence-2 (CPU-only)",
    "FEAT: Added GET /v1/models?type=vision to list available vision models",
    "FEAT: Added OCR_MODEL_PATH and FLORENCE_MODEL_PATH environment variables",
    "FEAT: Vision models run on CPU only (no GPU VRAM competition with LLM)",
];

/// Get formatted version string for logging
pub fn get_version_string() -> String {
    format!("Fabstir LLM Node {} ({})", VERSION_NUMBER, BUILD_DATE)
}

/// Get full version info for API responses
pub fn get_version_info() -> serde_json::Value {
    serde_json::json!({
        "version": VERSION_NUMBER,
        "build": VERSION,
        "date": BUILD_DATE,
        "features": FEATURES,
        "chains": SUPPORTED_CHAINS,
        "breaking_changes": BREAKING_CHANGES,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constants() {
        assert_eq!(VERSION_MAJOR, 8);
        assert_eq!(VERSION_MINOR, 11);
        assert_eq!(VERSION_PATCH, 0);
        assert!(FEATURES.contains(&"multi-chain"));
        assert!(FEATURES.contains(&"dual-pricing"));
        assert!(FEATURES.contains(&"cpu-ocr"));
        assert!(FEATURES.contains(&"cpu-vision"));
        assert!(FEATURES.contains(&"vision-20mb-body-limit"));
        assert!(FEATURES.contains(&"host-side-web-search"));
        assert!(FEATURES.contains(&"brave-search-api"));
        assert!(FEATURES.contains(&"inference-web-search"));
        assert!(FEATURES.contains(&"streaming-web-search"));
        assert!(FEATURES.contains(&"websocket-web-search"));
        assert!(FEATURES.contains(&"auto-search-intent-detection"));
        assert!(FEATURES.contains(&"sdk-web-search-field"));
        assert!(FEATURES.contains(&"web-search-system-prompt"));
        assert!(FEATURES.contains(&"search-query-harmony-cleanup"));
        assert!(FEATURES.contains(&"search-prompt-v2"));
        // v8.8.0 content fetching features
        assert!(FEATURES.contains(&"content-fetching"));
        assert!(FEATURES.contains(&"html-extraction"));
        assert!(FEATURES.contains(&"page-content-cache"));
        assert!(FEATURES.contains(&"parallel-fetch"));
        // v8.9.0 proof signing features
        assert!(FEATURES.contains(&"proof-signing"));
        assert!(FEATURES.contains(&"security-audit-compliance"));
        assert!(FEATURES.contains(&"ecdsa-proof-signatures"));
        assert!(FEATURES.contains(&"65-byte-signatures"));
        // v8.9.1 EIP-191 fix
        assert!(FEATURES.contains(&"eip191-personal-sign"));
        // v8.10.0 content hash binding features
        assert!(FEATURES.contains(&"content-hash-binding"));
        assert!(FEATURES.contains(&"real-prompt-hash"));
        assert!(FEATURES.contains(&"real-response-hash"));
        assert!(FEATURES.contains(&"proof-witness-content"));
        assert!(FEATURES.contains(&"streaming-response-accumulation"));
        // v8.11.0 checkpoint publishing features
        assert!(FEATURES.contains(&"checkpoint-publishing"));
        assert!(FEATURES.contains(&"conversation-recovery"));
        assert!(FEATURES.contains(&"sdk-checkpoint-recovery"));
        assert!(FEATURES.contains(&"s5-checkpoint-storage"));
        assert!(FEATURES.contains(&"eip191-checkpoint-signatures"));
        assert!(FEATURES.contains(&"sorted-json-keys"));
        assert!(FEATURES.contains(&"session-resumption"));
        assert!(FEATURES.contains(&"ttl-cleanup-policy"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("8.11.0"));
        assert!(version.contains("2026-01-11"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.11.0-checkpoint-publishing-2026-01-11");
        assert_eq!(VERSION_NUMBER, "8.11.0");
        assert_eq!(BUILD_DATE, "2026-01-11");
    }
}
