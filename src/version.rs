// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.21.1-think-tag-passthrough-2026-02-28";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.21.1";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 21;

/// Patch version number
pub const VERSION_PATCH: u32 = 1;

/// Build date
pub const BUILD_DATE: &str = "2026-02-28";

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
    // Settlement race condition fix (v8.12.6)
    "settlement-wait-loop",
    "proof-submission-cache",
    "s5-propagation-delay-handling",
    "submission-started-tracking",
    // AUDIT pre-report remediation (v8.13.0)
    "audit-f4-compliance",
    "model-id-signature",
    "cross-model-replay-protection",
    "session-model-query",
    "audit-remediation",
    // Model validation (v8.14.0)
    "model-validation",
    "dynamic-model-discovery",
    "sha256-model-verification",
    "host-authorization-cache",
    "startup-model-validation",
    "contract-model-queries",
    // Model-agnostic inference (v8.15.0)
    "glm4-chat-template",
    "configurable-stop-tokens",
    "min-p-sampling",
    "model-agnostic-inference",
    "per-template-stop-tokens",
    "probabilistic-sampling",
    // KV cache quantization (v8.15.1)
    "kv-cache-quantization",
    // Repeat penalty window fix (v8.15.2)
    "repeat-penalty-window-256",
    // VLM vision sidecar (v8.15.3)
    "vlm-vision-sidecar",
    "vlm-ocr",
    "vlm-image-description",
    "vlm-onnx-fallback",
    "openai-compatible-vlm",
    // WebSocket vision pre-processing (v8.15.4)
    "websocket-vision-preprocessing",
    "vlm-dual-ocr-describe",
    "vision-prompt-augmentation",
    // Session re-init fix (v8.15.5)
    "session-reinit-fix",
    // Image generation (v8.16.0)
    "image-generation",
    "diffusion-sidecar",
    "sglang-diffusion",
    "flux-klein-4b",
    "prompt-safety-classifier",
    "output-safety-classifier",
    "image-rate-limiter",
    "image-generation-billing",
    "image-content-hashes",
    "image-proof-extension",
    "websocket-image-generation",
    "http-image-generation",
    // Auto-route image intent (v8.16.1)
    "auto-image-routing",
    // Thinking/reasoning mode (v8.17.0)
    "thinking-mode",
    "per-request-thinking",
    "default-thinking-mode-env",
    // Thinking injection bugfix (v8.17.1)
    "thinking-post-processing",
    // Thinking "Off" conciseness directive (v8.17.2)
    "thinking-off-conciseness",
    // GLM-4 default thinking + off skip injection (v8.17.3)
    "glm4-default-thinking",
    // New JobMarketplace proxy (v8.17.4)
    "new-jobmarketplace-proxy",
    // Dispute window fix (v8.17.5)
    "dispute-window-fix",
    "contract-dispute-window-query",
    "dispute-window-buffer",
    // GLM-4 RAG context-aware system prompt (v8.17.6)
    "glm4-context-aware-system-prompt",
    // setTokenPricing after registration (v8.18.0)
    "set-token-pricing",
    "per-token-erc20-pricing",
    "token-pricing-usdc-env",
    // Stream cancellation (v8.19.0)
    "stream-cancel",
    "cancel-flag-inference",
    "tokio-select-streaming",
    "stream-end-reason",
    "stream-end-tokens-used",
    // True token-by-token streaming (v8.19.1)
    "true-streaming",
    "spawn-blocking-inference",
    // Per-model token pricing (v8.20.0)
    "model-token-pricing",
    "set-model-token-pricing",
    "clear-model-token-pricing",
    "per-model-per-token-pricing",
    // Content fetch PDF fix (v8.20.1)
    "binary-url-detection",
    "content-type-filtering",
    "safe-string-truncation",
    // Context usage reporting (v8.21.0)
    "context-usage-reporting",
    "finish-reason-length",
    "token-limit-exceeded",
    "stream-end-usage",
    // Think-tag passthrough (v8.21.1)
    "think-tag-passthrough",
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    // v8.21.1 - Think-tag passthrough (Feb 28, 2026)
    "FIX: Special tokens (e.g. <think>) now rendered in output (Special::Tokenize instead of Special::Plaintext)",
    // v8.21.0 - Context Usage Reporting (Feb 28, 2026)
    "FEAT: HTTP inference responses now include 'usage' object with prompt_tokens, completion_tokens, total_tokens, context_window_size",
    "FEAT: WebSocket stream_end messages now include 'usage' and 'finish_reason' fields",
    "FIX: finish_reason now correctly returns 'length' when max_tokens is hit (was always 'stop')",
    "FEAT: TOKEN_LIMIT_EXCEEDED structured error when prompt exceeds context window",
    // v8.20.1 - Content Fetch PDF Fix (Feb 27, 2026)
    "FIX: Content fetcher no longer panics on PDF/binary URLs (was crashing on arxiv.org PDFs)",
    "FIX: Binary URL detection skips .pdf, .zip, image, video, audio URLs before fetching",
    "FIX: Content-Type header check filters application/pdf, image/*, video/*, audio/*",
    "FIX: Body content sniff detects %PDF prefix as fallback for incorrect Content-Type headers",
    "FIX: truncate_content() now uses char-boundary-safe slicing (prevents panic on multi-byte data)",
    // v8.20.0 - Per-Model Token Pricing (Phase 18, Feb 26, 2026)
    "BREAKING: setTokenPricing(address,uint256) removed — use setModelTokenPricing(bytes32,address,uint256)",
    "FEAT: Per-model per-token pricing via setModelTokenPricing after registerNode()",
    "FEAT: Pricing set for each model × each token (native + USDC) in a loop",
    "FEAT: clearModelTokenPricing(bytes32,address) added to ABI for price removal",
    "FEAT: ModelTokenPricingUpdated event replaces TokenPricingUpdated (adds modelId field)",
    "CONTRACT: getNodePricing, updatePricingNative, updatePricingStable removed from ABI",
    // v8.19.1 - True Token-by-Token Streaming (Feb 25, 2026)
    "FEAT: Tokens now stream to client as generated (no more batch-then-deliver delay)",
    "FEAT: spawn_blocking + Handle::block_on for !Send llama-cpp inference on blocking thread pool",
    "FEAT: token_sender field on InferenceRequest for per-token channel delivery",
    "FEAT: Removed 10ms artificial streaming delay — tokens arrive at generation speed",
    "PERF: stream_cancel now stops actual GPU generation, not just delivery",
    // v8.19.0 - Stream Cancellation (Feb 25, 2026)
    "FEAT: Node handles stream_cancel WebSocket message to stop inference mid-stream",
    "FEAT: Cancel flag (AtomicBool) checked between tokens in generation loop",
    "FEAT: tokio::select! in streaming loops enables concurrent cancel detection",
    "FEAT: stream_end messages now include 'reason' (complete/cancelled/error) and 'tokens_used' fields",
    "FEAT: WebSocket socket split (sender/receiver) for concurrent read/write",
    // v8.18.0 - setTokenPricing After Registration (Feb 24, 2026)
    "FEAT: Node now calls setTokenPricing(USDC, price) after registerNode() (F202614977)",
    "FEAT: TOKEN_PRICING_USDC env var for custom USDC pricing (default: 10,000 = $10/million)",
    "FEAT: get_token_pricing_usdc() helper with env var + range validation + fallback",
    "FEAT: ABI updated with setTokenPricing, customTokenPricing, TokenPricingUpdated",
    "CONTRACT: NodeRegistry getNodePricing() now reverts for ERC20 without setTokenPricing",
    // v8.17.6 - GLM-4 RAG Context-Aware System Prompt (Feb 23, 2026)
    "FIX: GLM-4 default system prompt now instructs model to use provided reference material, search results, and document excerpts",
    "FIX: GLM-4 no longer claims 'I don't have access to external databases' when RAG context is in user message",
    "FEAT: GLM-4 auto-injected system prompt now includes current date (matching Harmony pattern)",
    // v8.17.5 - Dispute Window Fix (Feb 23, 2026)
    "FIX: Error string matching broadened from 'Must wait dispute window' to 'dispute window' (catches old and new contract)",
    "FIX: Dispute window now queried from contract disputeWindow() at startup (was hardcoded 30s)",
    "FIX: 5s safety buffer added to dispute window wait (accounts for block confirmation delay)",
    // v8.17.4 - New JobMarketplace Proxy (Feb 22, 2026)
    "CONTRACT: JobMarketplace proxy changed to 0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4 (fresh proxy deployment)",
    "CONTRACT: Old proxy 0x95132177F964FF053C1E874b53CF74d819618E06 deprecated (de-authorized)",
    "CONTRACT: Error string shortened: 'Only host can submit proof' -> 'Not host'",
    // v8.17.3 - GLM-4 Default Thinking + Off Skip Injection (Feb 18, 2026)
    "FEAT: GLM-4 Default mode now injects /think (thinking ON, matching centralised platforms)",
    "FIX: GLM-4 Off mode skips injection instead of /no_think (natural non-thinking, ~483 tokens)",
    // v8.17.2 - Thinking "Off" Conciseness Directive (Feb 18, 2026)
    "FIX: Thinking=disabled now injects conciseness directive for noticeably shorter responses on Harmony",
    // v8.17.1 - Thinking Injection Bugfix (Feb 18, 2026)
    "FIX: Thinking injection no longer destroys default system prompt on first message",
    "FIX: Empty-string DEFAULT_THINKING_MODE env var treated as unset (no injection)",
    "FIX: Post-processing replaces Reasoning level in formatted output, preserving Valid channels",
    // v8.17.0 - Thinking/Reasoning Mode (Feb 17, 2026)
    "FEAT: Per-request thinking/reasoning mode via 'thinking' field (enabled, disabled, low, medium, high)",
    "FEAT: Harmony template: maps thinking mode to Reasoning: none/low/medium/high in system prompt",
    "FEAT: GLM-4 template: maps thinking mode to /think or /no_think prefix on user message",
    "FEAT: DEFAULT_THINKING_MODE env var for global default thinking mode",
    "FEAT: Respects user-provided Reasoning: directive in system messages (no override)",
    // v8.16.1 - Auto-Route Image Intent (Feb 16, 2026)
    "FEAT: Node-side image intent detection (AUTO_IMAGE_ROUTING env var, default OFF)",
    "FEAT: Conservative keyword matching for generate/create/make/draw/paint/sketch/illustrate",
    "FEAT: Auto-routes detected image prompts to diffusion sidecar when available",
    "FEAT: Falls through to normal inference if diffusion sidecar unavailable",
    // v8.16.0 - Image Generation (Feb 14, 2026)
    "FEAT: Text-to-image generation via SGLang Diffusion sidecar (FLUX.2 Klein 4B)",
    "FEAT: DiffusionClient with OpenAI-compatible /v1/images/generations API",
    "FEAT: Three-layer content safety pipeline (keyword blocklist, LLM prompt classifier, VLM output classifier)",
    "FEAT: POST /v1/images/generate HTTP endpoint for image generation",
    "FEAT: WebSocket ImageGeneration/ImageGenerationResult message types",
    "FEAT: Image generation billing (megapixel-steps formula with model multiplier)",
    "FEAT: ImageContentHashes for SHA-256 proof witness binding",
    "FEAT: ImageGenerationRateLimiter with sliding window rate limiting",
    "FEAT: SafetyAttestation with cryptographic safety proof hashes",
    "FEAT: DIFFUSION_ENDPOINT and DIFFUSION_MODEL_NAME env vars for sidecar configuration",
    "FEAT: Docker diffusion-sidecar service in docker-compose.prod.yml",
    // v8.15.5 - Session Re-init Fix (Feb 13, 2026)
    "FIX: Second encrypted_session_init no longer wipes uploaded vectors and conversation history",
    "FEAT: New ensure_session_exists_with_chain() preserves existing session state on re-init",
    // v8.15.4 - WebSocket Vision Pre-Processing (Feb 8, 2026)
    "FEAT: WebSocket encrypted messages now route images to VLM sidecar for OCR + visual description",
    "FEAT: Dual OCR+describe pipeline: text extraction (4096 tokens) + brief visual description (100 tokens)",
    "FEAT: Prompt augmented with [Image Analysis]...[/Image Analysis] context before main LLM",
    "FEAT: Plaintext inference path also supports image routing to VLM sidecar",
    // v8.15.3 - VLM Vision Sidecar (Feb 8, 2026)
    "FEAT: Optional VLM sidecar (Qwen3-VL via llama-server) for high-quality OCR and image description",
    "FEAT: VLM_ENDPOINT and VLM_MODEL_NAME env vars for sidecar configuration",
    "FEAT: Automatic ONNX fallback when VLM unavailable or fails",
    "FEAT: Response model field now dynamic based on provider (VLM name or paddleocr/florence-2)",
    "FEAT: OcrResponse::new() and DescribeImageResponse::new() accept model parameter",
    // v8.15.2 - Repeat Penalty Window Fix (Feb 7, 2026)
    "FIX: Repeat penalty window increased from 64 to 256 tokens to prevent long repetition loops",
    "FIX: Models no longer get stuck in repeating patterns that exceed 64-token lookback",
    // v8.15.1 - KV Cache Quantization (Feb 7, 2026)
    "FEAT: KV cache quantization via KV_CACHE_TYPE env var (q8_0, q4_0, f16, bf16, f32)",
    "FEAT: EngineConfig gains kv_cache_type_k/v fields (Option<String>, default None)",
    // v8.15.0 - Model-Agnostic Inference Pipeline (Feb 7, 2026)
    "FEAT: GLM-4 chat template support (MODEL_CHAT_TEMPLATE=glm4)",
    "FEAT: Per-template stop tokens replace hardcoded Harmony token ID 200002",
    "FEAT: min_p sampler field added to InferenceRequest (default 0.0 = disabled)",
    "FEAT: Sampler chain now uses dist() for probabilistic sampling when temp > 0",
    "FEAT: MODEL_STOP_TOKENS env var for custom stop token override",
    "FEAT: repeat_penalty now wired into sampler chain (was ignored before)",
    // v8.14.1 - Dynamic Model Registry + submitProofOfWork Fix (Feb 5, 2026)
    "FIX: submitProofOfWork now uses 5 params (signature removed per Feb 4 contract update)",
    "FIX: Removed hardcoded ApprovedModels struct - now fully dynamic from contract",
    "FIX: validate_models_for_registration() queries ModelRegistry contract at startup",
    "FEAT: Any model registered on-chain works automatically without code changes",
    "FEAT: GPT-OSS-20B and future models supported without hardcoding",
    // v8.14.0 - Model Validation (Feb 5, 2026)
    "FEAT: Model validation enforces host authorization at startup (REQUIRE_MODEL_VALIDATION=true)",
    "FEAT: Dynamic model discovery from ModelRegistry contract (no hardcoded model list)",
    "FEAT: SHA256 hash verification of model files against on-chain hash",
    "FEAT: Host authorization caching for performance (nodeSupportsModel queries)",
    "FEAT: Node refuses to start if MODEL_PATH not authorized for host",
    "FEAT: Feature flag REQUIRE_MODEL_VALIDATION (default: false) for gradual rollout",
    // v8.13.0 - AUDIT Pre-Report Remediation (Feb 1, 2026)
    "BREAKING: Proof signatures now include modelId as 4th parameter (AUDIT-F4)",
    "BREAKING: Signature format changed from 84 bytes to 116 bytes",
    "FEAT: Node queries sessionModel(sessionId) from JobMarketplace before signing",
    "FEAT: Prevents cross-model replay attacks (cheap model proof on premium model)",
    "FEAT: For non-model sessions: modelId = bytes32(0)",
    "CONTRACT: Using remediated contracts at 0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4 (JobMarketplace)",
    "CONTRACT: Using remediated contracts at 0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31 (ProofSystem)",
    "SECURITY: Implements AUDIT-F4 recommendation from pre-report security audit",
    // v8.12.6 - Settlement Race Condition Fix (Jan 25, 2026)
    "FIX: Settlement now waits for in-flight proof submissions to complete before proceeding",
    "FIX: Prevents 'Session not active' errors when WebSocket disconnects during proof generation",
    "FEAT: Added ProofSubmissionCache for S5 propagation delay handling",
    "FEAT: Added submission_started_at field to JobTokenTracker for timeout calculation",
    "FEAT: Settlement polls for up to 120s waiting for submission_in_progress to become false",
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
        assert_eq!(VERSION_MINOR, 21);
        assert_eq!(VERSION_PATCH, 1);
        assert!(FEATURES.contains(&"multi-chain"));
        assert!(FEATURES.contains(&"dual-pricing"));
        // v8.17.4 new JobMarketplace proxy
        assert!(FEATURES.contains(&"new-jobmarketplace-proxy"));
        // v8.17.5 dispute window fix
        assert!(FEATURES.contains(&"dispute-window-fix"));
        assert!(FEATURES.contains(&"contract-dispute-window-query"));
        assert!(FEATURES.contains(&"dispute-window-buffer"));
        // v8.17.6 GLM-4 RAG context-aware system prompt
        assert!(FEATURES.contains(&"glm4-context-aware-system-prompt"));
        // v8.18.0 setTokenPricing
        assert!(FEATURES.contains(&"set-token-pricing"));
        assert!(FEATURES.contains(&"per-token-erc20-pricing"));
        assert!(FEATURES.contains(&"token-pricing-usdc-env"));
        // v8.19.0 stream-cancel
        assert!(FEATURES.contains(&"stream-cancel"));
        assert!(FEATURES.contains(&"cancel-flag-inference"));
        assert!(FEATURES.contains(&"stream-end-reason"));
        // v8.19.1 true streaming
        assert!(FEATURES.contains(&"true-streaming"));
        assert!(FEATURES.contains(&"spawn-blocking-inference"));
        // v8.20.0 per-model token pricing
        assert!(FEATURES.contains(&"model-token-pricing"));
        assert!(FEATURES.contains(&"set-model-token-pricing"));
        assert!(FEATURES.contains(&"clear-model-token-pricing"));
        assert!(FEATURES.contains(&"per-model-per-token-pricing"));
        // v8.20.1 content fetch PDF fix
        assert!(FEATURES.contains(&"binary-url-detection"));
        assert!(FEATURES.contains(&"content-type-filtering"));
        assert!(FEATURES.contains(&"safe-string-truncation"));
        // v8.21.0 context usage reporting
        assert!(FEATURES.contains(&"context-usage-reporting"));
        assert!(FEATURES.contains(&"finish-reason-length"));
        assert!(FEATURES.contains(&"token-limit-exceeded"));
        assert!(FEATURES.contains(&"stream-end-usage"));
        // v8.21.1 think-tag passthrough
        assert!(FEATURES.contains(&"think-tag-passthrough"));
        // v8.15.5 session re-init fix
        assert!(FEATURES.contains(&"session-reinit-fix"));
        // v8.15.0 model-agnostic inference features
        assert!(FEATURES.contains(&"glm4-chat-template"));
        assert!(FEATURES.contains(&"configurable-stop-tokens"));
        assert!(FEATURES.contains(&"min-p-sampling"));
        assert!(FEATURES.contains(&"model-agnostic-inference"));
        assert!(FEATURES.contains(&"per-template-stop-tokens"));
        assert!(FEATURES.contains(&"probabilistic-sampling"));
        // v8.15.1 KV cache quantization
        assert!(FEATURES.contains(&"kv-cache-quantization"));
        // v8.15.2 repeat penalty window
        assert!(FEATURES.contains(&"repeat-penalty-window-256"));
        // v8.15.3 VLM vision
        assert!(FEATURES.contains(&"vlm-vision-sidecar"));
        assert!(FEATURES.contains(&"vlm-ocr"));
        assert!(FEATURES.contains(&"vlm-onnx-fallback"));
        // v8.15.4 WebSocket vision pre-processing
        assert!(FEATURES.contains(&"websocket-vision-preprocessing"));
        assert!(FEATURES.contains(&"vlm-dual-ocr-describe"));
        assert!(FEATURES.contains(&"vision-prompt-augmentation"));
        // v8.16.0 image generation
        assert!(FEATURES.contains(&"image-generation"));
        assert!(FEATURES.contains(&"diffusion-sidecar"));
        assert!(FEATURES.contains(&"prompt-safety-classifier"));
        assert!(FEATURES.contains(&"output-safety-classifier"));
        assert!(FEATURES.contains(&"image-generation-billing"));
        assert!(FEATURES.contains(&"image-content-hashes"));
        // v8.16.1 auto-route image intent
        assert!(FEATURES.contains(&"auto-image-routing"));
        // v8.17.0 thinking mode
        assert!(FEATURES.contains(&"thinking-mode"));
        assert!(FEATURES.contains(&"per-request-thinking"));
        assert!(FEATURES.contains(&"default-thinking-mode-env"));
        // v8.17.1 thinking injection bugfix
        assert!(FEATURES.contains(&"thinking-post-processing"));
        // v8.17.2 thinking off conciseness
        assert!(FEATURES.contains(&"thinking-off-conciseness"));
        // v8.17.3 GLM-4 default thinking
        assert!(FEATURES.contains(&"glm4-default-thinking"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("8.21.1"));
        assert!(version.contains("2026-02-28"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.21.1-think-tag-passthrough-2026-02-28");
        assert_eq!(VERSION_NUMBER, "8.21.1");
        assert_eq!(BUILD_DATE, "2026-02-28");
    }

    #[test]
    fn test_model_validation_features() {
        assert!(FEATURES.contains(&"model-validation"));
        assert!(FEATURES.contains(&"dynamic-model-discovery"));
        assert!(FEATURES.contains(&"sha256-model-verification"));
        assert!(FEATURES.contains(&"host-authorization-cache"));
        assert!(FEATURES.contains(&"startup-model-validation"));
        assert!(FEATURES.contains(&"contract-model-queries"));
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
