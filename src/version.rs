// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.12.5-s5-portal-migration-2026-01-23";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.12.5";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 12;

/// Patch version number
pub const VERSION_PATCH: u32 = 5;

/// Build date
pub const BUILD_DATE: &str = "2026-01-23";

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
    // HTTP checkpoint endpoint (v8.11.1+)
    "http-checkpoint-endpoint",
    "checkpoint-index-api",
    // Encrypted checkpoint deltas (v8.12.0+)
    "encrypted-checkpoint-deltas",
    "checkpoint-encryption",
    "ecdh-checkpoint-keys",
    "xchacha20-checkpoint-encryption",
    "recovery-public-key",
    "forward-secrecy-checkpoints",
    "ephemeral-keypairs",
    "harmony-message-parsing",
    "clean-checkpoint-messages",
    // Crypto params fix (v8.12.2)
    "sdk-compatible-ecdh",
    "sha256-shared-secret",
    // deltaCID on-chain support (v8.12.4)
    "delta-cid-on-chain",
    "checkpoint-blockchain-events",
    "decentralized-checkpoint-recovery",
    // S5 portal migration (v8.12.5)
    "platformless-ai-s5-portal",
    "sia-decentralized-storage",
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    // v8.12.5 - S5 Portal Migration (Jan 23, 2026)
    "CONFIG: Default S5 portal changed from s5.vup.cx to s5.platformlessai.ai",
    "CONFIG: S5 storage backend now uses Sia decentralized storage",
    "CONFIG: Updated default P2P peers to node.sfive.net, s5.garden, s5.vup.cx",
    "DEPLOY: Existing hosts must restart S5 bridge to use new portal",
    // v8.12.4 - deltaCID On-Chain Support (Jan 14, 2026)
    "FEAT: submitProofOfWork now includes 6th parameter: deltaCID",
    "FEAT: deltaCID stored on-chain for decentralized checkpoint recovery",
    "FEAT: ProofSubmitted event now emits deltaCID for SDK querying",
    "BREAKING: Contract ABI change - 6th string parameter added to submitProofOfWork",
    // v8.12.3 - Production Cleanup (Jan 13, 2026)
    "CLEANUP: Removed verbose debug logging from session_init (no more raw JSON in logs)",
    "PRIVACY: Session init no longer logs potentially sensitive decrypted payload data",
    // v8.12.2 - Crypto Params Fix (Jan 13, 2026)
    "FIX: ECDH key derivation now matches SDK spec - sha256(x_coordinate) before HKDF",
    "FIX: SDK can now decrypt encrypted checkpoint deltas (Poly1305 auth succeeds)",
    "CRYPTO: shared_secret = sha256(ecdh_result.x_coordinate) [was: raw x_coordinate]",
    "TEST: Added test_sdk_compatible_key_derivation() to verify crypto compatibility",
    // v8.12.1 - Checkpoint Bug Fixes (Jan 13, 2026)
    "FIX: Checkpoint messages now properly parsed from Harmony format (no more raw tags)",
    "FIX: recoveryPublicKey from session init now properly wired to checkpoint encryption",
    "FEAT: New harmony.rs module parses Harmony-formatted prompts into clean messages",
    "FEAT: extract_last_user_message() extracts just the last user message for checkpoints",
    // v8.12.0 - Encrypted Checkpoint Deltas (Jan 13, 2026)
    "FEAT: Checkpoint deltas can now be encrypted using user's recovery public key",
    "FEAT: ECDH key exchange with ephemeral keypairs for forward secrecy",
    "FEAT: XChaCha20-Poly1305 authenticated encryption for checkpoint content",
    "FEAT: Session init accepts optional recoveryPublicKey from SDK v1.8.7+",
    "FEAT: CheckpointEntry has optional 'encrypted' marker for SDK detection",
    "FEAT: Backward compatible - plaintext deltas when no recovery key provided",
    "PRIVACY: Only user with matching private key can decrypt checkpoint content",
    // v8.11.12 - Unified S5 Deployment (Jan 12, 2026)
    "DEPLOY: New unified docker-compose.prod.yml includes S5 bridge + Rust node",
    "DEPLOY: S5 bridge now starts automatically with docker-compose up",
    "DEPLOY: Rust node uses ENHANCED_S5_URL=http://s5-bridge:5522 (Docker networking)",
    "DEPLOY: Tarball now includes services/s5-bridge/ directory",
    "DEPLOY: New .env.prod.example with all required configuration",
    // v8.11.11 - S5 Backend Init Logging (Jan 12, 2026)
    "CRITICAL FIX: Node now logs which S5 backend is used on startup with [S5-INIT] prefix",
    "CRITICAL FIX: Shows warning if MockS5Backend is used (uploads won't reach network!)",
    "CRITICAL FIX: Shows ENHANCED_S5_URL env var value when using EnhancedS5Backend",
    "CRITICAL FIX: MockS5Backend::put() now logs warning for each upload that won't reach network",
    "DEBUG: Startup clearly shows: 'Using EnhancedS5Backend' or 'Using MockS5Backend'",
    // v8.11.10 - S5 Debug Logging (Jan 12, 2026)
    "DEBUG: Added comprehensive S5 upload logging with [S5-UPLOAD], [S5-RUST], [S5-HTTP] prefixes",
    "DEBUG: S5 bridge now logs portal account status, request IDs, and upload duration",
    "DEBUG: Rust node logs CID length, networkUploaded flag, and bridge debug info",
    "FIX: S5 bridge returns HTTP 503 if no portal accounts configured (prevents silent failures)",
    "FIX: S5 bridge startup now clearly logs whether uploads will go to S5 network",
    // v8.11.9 - BlobIdentifier CID Format (Jan 12, 2026)
    "BREAKING: CIDs now use BlobIdentifier format (58-70 chars) instead of raw hash (53 chars)",
    "FIX: S5 bridge uses BlobIdentifier class with file size for portal compatibility",
    "FIX: MockS5Backend generates BlobIdentifier CIDs (prefix + multihash + hash + size)",
    "FIX: is_valid_s5_cid() now accepts 58-70 char BlobIdentifier format",
    "FIX: Old 53-char raw hash format is DEPRECATED - S5 portals reject it",
    // v8.11.8 - S5 Advanced API CID Fix (Jan 12, 2026)
    "FIX: S5 bridge now uses Advanced API (FS5Advanced.pathToCID + formatCID) for proper CIDs",
    "FIX: MockS5Backend generates S5 CID format (blake3 + base32 = 53 chars) for testing",
    "FIX: Rust node reads 'cid' field from S5 bridge response (no more manual CID formatting)",
    "FIX: Removed all IPFS format (bafkrei/bafybei) references - S5 uses simpler raw base32 format",
    // v8.11.7 - CID Format Fix (Jan 12, 2026)
    "FIX: deltaCid now returns proper S5 CID format (53 chars: b + 52 base32) instead of raw hex hash",
    "FIX: S5 uses blake3 hashing (NOT sha256) with raw base32 encoding (NOT IPFS CID structure)",
    "FIX: CID format is 'b' prefix + 52 lowercase base32 chars = 53 total characters",
    "FIX: IMPORTANT - S5 does NOT use IPFS format (bafkrei/bafybei are WRONG for S5)",
    "DEBUG: Added tracing logs to EnhancedS5Backend::put() and put_file() for debugging",
    // v8.11.6 - S5 Storage Cleanup (Jan 12, 2026)
    "CLEANUP: Removed RealS5Backend (~285 lines) - redundant with EnhancedS5Backend",
    "CLEANUP: Removed S5Storage impl from EnhancedS5Client (~82 lines) - all usage goes through EnhancedS5Backend",
    "CLEANUP: Removed S5ClientConfig struct - only used by deleted RealS5Backend",
    "CLEANUP: Removed S5Backend::Real variant - not used in production",
    "ARCH: Now only two S5Storage implementations: MockS5Backend (testing) and EnhancedS5Backend (production)",
    // v8.11.5 - Real S5 CID Format (Jan 11, 2026)
    "FIX: deltaCid now returns real S5 CID from bridge (53 char base32 format) instead of fake hex hash",
    "FIX: S5 put_file() now returns the actual CID from the S5 bridge response",
    // v8.11.4 - Dead Code Cleanup (Jan 11, 2026)
    "CLEANUP: Removed ~850 lines of dead code from http_server.rs",
    "CLEANUP: http_server.rs now only contains AppState struct",
    "CLEANUP: All HTTP handlers consolidated in server.rs",
    // v8.11.1 - HTTP Checkpoint Endpoint (Jan 11, 2026)
    "FEAT: Added GET /v1/checkpoints/{session_id} HTTP endpoint",
    "FEAT: SDK can now retrieve checkpoint index without direct S5 access",
    "FEAT: CheckpointManager accessor methods for host_address and s5_storage",
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
        assert_eq!(VERSION_MINOR, 12);
        assert_eq!(VERSION_PATCH, 5);
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
        // v8.12.0 encrypted checkpoint deltas features
        assert!(FEATURES.contains(&"encrypted-checkpoint-deltas"));
        assert!(FEATURES.contains(&"checkpoint-encryption"));
        assert!(FEATURES.contains(&"ecdh-checkpoint-keys"));
        assert!(FEATURES.contains(&"xchacha20-checkpoint-encryption"));
        assert!(FEATURES.contains(&"recovery-public-key"));
        assert!(FEATURES.contains(&"forward-secrecy-checkpoints"));
        assert!(FEATURES.contains(&"ephemeral-keypairs"));
        // v8.12.4 deltaCID on-chain features
        assert!(FEATURES.contains(&"delta-cid-on-chain"));
        assert!(FEATURES.contains(&"checkpoint-blockchain-events"));
        assert!(FEATURES.contains(&"decentralized-checkpoint-recovery"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("8.12.5"));
        assert!(version.contains("2026-01-23"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.12.5-s5-portal-migration-2026-01-23");
        assert_eq!(VERSION_NUMBER, "8.12.5");
        assert_eq!(BUILD_DATE, "2026-01-23");
    }

    #[test]
    fn test_crypto_params_fix_features() {
        assert!(FEATURES.contains(&"sdk-compatible-ecdh"));
        assert!(FEATURES.contains(&"sha256-shared-secret"));
    }

    #[test]
    fn test_http_checkpoint_features() {
        assert!(FEATURES.contains(&"http-checkpoint-endpoint"));
        assert!(FEATURES.contains(&"checkpoint-index-api"));
    }

    #[test]
    fn test_encrypted_checkpoint_features() {
        assert!(FEATURES.contains(&"encrypted-checkpoint-deltas"));
        assert!(FEATURES.contains(&"checkpoint-encryption"));
        assert!(FEATURES.contains(&"recovery-public-key"));
    }

    #[test]
    fn test_harmony_parsing_features() {
        assert!(FEATURES.contains(&"harmony-message-parsing"));
        assert!(FEATURES.contains(&"clean-checkpoint-messages"));
    }

    #[test]
    fn test_s5_portal_migration_features() {
        assert!(FEATURES.contains(&"platformless-ai-s5-portal"));
        assert!(FEATURES.contains(&"sia-decentralized-storage"));
    }
}
