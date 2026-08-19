// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.47.0-vault-session-hardening-2026-08-19";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.47.0";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 47;

/// Patch version number
pub const VERSION_PATCH: u32 = 0;

/// Build date
pub const BUILD_DATE: &str = "2026-08-19";

/// Supported features in this version
pub const FEATURES: &[&str] = &[
    // v8.46.1 context-window clamp on the generation budget.
    "context-window-clamp",
    // v8.46.0 Qwen3.8-27B support: llama-cpp-2 0.1.146 -> 0.1.154. The 0.1.146
    // llama.cpp has no nextn/MTP handling in its qwen35 path, so a Qwen3.8 GGUF
    // (block_count 65 = 64 trunk + 1 MTP block, plus blk.64.nextn.* tensors)
    // fails to load outright. 0.1.154's qwen35 loader splits n_layer from
    // n_layer_all and consumes the MTP block. Qwen3.6 is unaffected.
    "qwen38-27b",
    "llama-cpp-2-0-1-154",
    "qwen35-nextn-mtp-block",
    // v8.46.0 Qwen reasoning-effort control on the ChatML path. Qwen3.8's own
    // template defaults to reasoning_effort=xhigh; we format ChatML ourselves so
    // the model otherwise gets no instruction and falls back to long thinking,
    // which a per-token-billed host pays for on every reply.
    "chatml-reasoning-effort",
    "qwen-thinking-control",
    // v8.45.0 mode 13 "Convert to HDR (EXR)" (EXECUTION-MODE13-HDR.md): the
    // ltx-sdr2hdr-hdr template (HDR IC-LoRA + LTXVHDRDecodePostprocess, plain
    // VAEDecode LAW, exposure 7.1 = the proven look, internal writer OFF,
    // preview = tonemapped slot 0, exr_output = hdr_linear slot 1 unencoded);
    // colour_encoding "scene-linear-rec709" for this template; allowlist v23.
    "ltx-sdr2hdr",
    // v8.44.0 deep-conform input (EXECUTION-DEEP-CONFORM.md): jobs may carry
    // `inputWire` ("exrseq-display"/"exrseq-linear") with videos[0] a flat tar
    // of 16-bit EXR frames — the control clip without 8-bit quantisation or
    // 4:2:0 chroma subsampling. The patcher swaps the pinned VHS_LoadVideo for
    // the float sequence reader (in-place, censused, fail-closed); bounds gain
    // deepVideoMaxBytes (allowlist v22); frame count must equal billed EXACTLY.
    "ltx-deep-conform-input",
    // v8.42.0 WP-N1+WP-N2 moderation drop: operator-loadable hash lists
    // (MODERATION_LIST_FILE / MODERATION_OWNHASH_FILE /
    // MODERATION_PDQ_MAX_DISTANCE, loaded once at startup into a genuine
    // Loaded snapshot serving both the frames and asset moderation paths;
    // broken files degrade fail-closed with /health + metrics + boot-log
    // visibility; #!allow-empty exclusive directive); the moderation
    // {verdict, reason} field on transcode_complete; hold codes
    // CONTENT_BLOCKED/CONTENT_FLAGGED/MODERATION_UNAVAILABLE in the public
    // guide; match sentinel renamed to the list-neutral "hash-list-match";
    // per-hit provenance logging; named 20 MB moderation body limit as the
    // PART-A §3.2 batching counterpart (blocked-is-sticky across batches).
    "moderation-operator-lists",
    "moderation-verdict-on-complete",
    "moderation-hold-codes",
    "hash-list-match-sentinel",
    "moderation-degraded-health",
    "moderation-batched-frames",
    // v8.39.1 LTX panic safety: a panic inside the generation core used to
    // unwind straight out of the spawned task, skipping the single-exit
    // cleanup — the clip's pending proof was never forfeited, pending_count
    // stayed at 1 forever, and the later WS-disconnect path then took
    // defer_completion() (true) and returned WITHOUT calling
    // completeSessionJob, stranding the session escrow until the user paid an
    // on-chain triggerSessionTimeout reclaim. The core now runs under
    // catch_unwind: a caught panic emits a terminal GENERATION_FAILED frame
    // and falls through to the same single-exit cleanup as every other exit.
    // PANIC ONLY — SIGKILL, OOM-kill, SIGTERM (docker stop) and container
    // restarts lose LtxTracker's in-memory state and strand the same escrow;
    // closing those is separate, larger work. No wire, template, bundle,
    // commitment or attestation change.
    // v8.39.3 honest settlement logs: the LLM token tracker is legitimately
    // absent for LTX sessions (they claim tokens via submitProofOfWork), so its
    // absence is no longer logged as an ERROR claiming payment may be affected.
    "ltx-tracker-log-honesty",
    // v8.47.0 vault-session hardening (FC1.6):
    //  - plaintext `session_init` REFUSES a vault-paid job. That path carries no
    //    authenticated client identity, so checking a claimed address there
    //    would be theatre; vault money requires an encrypted session. Crypto-
    //    native sessions and un-configured nodes are unaffected.
    //  - the depositor read behind the gate now retries (3 attempts, 250ms unit
    //    backoff) and caches per jobId. The gate denies when the depositor
    //    cannot be read, which previously made every session init hostage to a
    //    public RPC answering first time; a depositor is fixed at creation, so
    //    a cache hit can never be stale. Genuine failures still deny.
    "fc1-plaintext-vault-refusal",
    "fc1-depositor-read-resilience",
    // v8.39.2 OQ-L24: all LTX WebSocket writes bounded (see BREAKING_CHANGES).
    "ltx-ws-write-bound",
    "oq-l24-wedged-client",
    "ltx-panic-safety",
    "ltx-panic-forfeits-pending-proof",
    // v8.39.0 FC1.6 vault-session auth: POST /v1/session-auth accepts a backend-
    // signed FC1-SESSION-AUTH digest (keccak256("FC1-SESSION-AUTH:<sessionId>:
    // <clientAddress lowercase>"), no EIP-191 prefix) and pre-authorises ONE
    // client address for a vault-paid session; the WS session gate then admits
    // the on-chain depositor OR that authorised client. Enables browser/helper
    // executors to attach to sessions the fiat vault deposited. Plus GET
    // /v1/health as an alias of /health — the SDK browser build probes
    // /v1/health (discovery + per-prompt host-health), so hosts without the
    // alias read as unreachable to browser clients.
    "fc1-session-auth",
    "vault-delegated-sessions",
    "v1-health-alias",
    // v8.36.1 over-length control clips accepted: the frame-count gate keeps only
    // its LOWER bound (under-length = overbilling risk, still fail-closed); clips
    // longer than the job crop server-side by construction (the trio's
    // frame_load_cap is patched to the billed count; iclora's Video Slice takes
    // the first duration seconds), so a 14.76 s clip is now a valid input to a
    // 14 s job with no client-side re-encode. Clip fps must still match exactly.
    // No wire/template/bundle change.
    // v8.36.0 BL4 video-edit trio (allow-list bundle v7): three pinned templates
    // ltx-outpaint-hdr / ltx-edit-hdr / ltx-restore-hdr on one shared spine
    // (dev-fp8 + distilled-384 + mode IC-LoRA, 8-step ManualSigmas, local Gemma
    // encoder, radiance gamma pair, source-audio passthrough). Outpaint fits and
    // black-letterboxes the control clip to the job's committed w/h (the outpaint
    // LoRA fills pure-black regions); edit/restore centre-crop conform. All three
    // take ONE control video and NO images (inputCommitment v3 with empty
    // imageHashes). The video binder widens to the union of LoadVideo (`file`)
    // and VHS_LoadVideo (`video`), id-ordered, count fail-closed; the billed
    // frame count patches into VHS_LoadVideo.frame_load_cap (loaded == billed by
    // construction; skip/select/force_rate are frozen neutral and golden-tested).
    // Graphs proved free on the host ComfyUI before hash pinning. No wire change.
    "ltx-video-edit",
    // v8.35.1 BL3/U7 hardening: server-side control-video frame-count gate
    // (billed == rendered enforced on the node, not just the helper) + hard-fail
    // on ComfyUI partial-graph node_errors at submit. No wire change.
    // v8.35.0 LTX IC-LoRA union control ("Restyle Clip", allow-list bundle v6):
    // the fourth pinned template `ltx-iclora-hdr` takes ONE reference still +
    // ONE control VIDEO (the first video input across the seam) and generates a
    // styled AV clip whose motion follows the control clip (MoGe depth guide);
    // audio is sampled jointly and ships in the output mp4. Seam: LtxJob gains
    // `videos` (S5 capability CIDs); bundle entries gain videoInputs/
    // videoSemantics and bounds gain videoMaxBytes (128 MiB) / videoFormats
    // (mp4, enforced by ISO-BMFF sniff on the decrypted bytes); inputCommitment
    // v3 appends `bytes32[] videoHashes` (keccak256 of plaintext), selected by
    // videoInputs > 0 (vectors-iclora.json is the cross-language fixture); the
    // patcher's seed handle widens to RandomNoise OR plain KSampler (iclora's
    // validated graph has no RandomNoise) and binds videos[i] to LoadVideo
    // nodes in node-id order, count fail-closed. The three pre-existing
    // templateHashes are byte-unchanged in bundle v6.
    "ltx-iclora",
    // v8.34.0 LTX user-selectable clip duration + fps correction (allow-list
    // bundle v5). Clients pick 5..=15 s at the LTX 2.3 native rates [24,25,48,50]
    // (the bundle previously advertised a never-supported 30 and omitted 48/50);
    // frames = fps·secs + 1, bounds.frames widened to {121,751}. The patcher
    // drives the pinned graphs' existing `Duration` PrimitiveInt (× Frame Rate + 1
    // → EmptyLTXVLatentVideo.length), so rendered length == billed frames by
    // construction; NO template file edited (zero templateHash movement). The new
    // validate_duration enforces exact whole seconds in range; the patcher fails
    // closed on fps==0 / frames==0.
    "ltx-duration",
    // v8.33.0 allow-list bundle v4: the full LTX 2.3 resolution ladder up to 4K
    // (landscape 768×512…3840×2160, portrait mirrors, 1024×1024 square) and a
    // 32 MiB input-image cap for 4K stills. Bundle-only change: bundleHash and
    // allowListVersion move; NO template/commitment/wire change. NOTE: a 4K
    // 121-frame clip is ~1,003,623 tokens ≈ $0.91 gross at price 904 — the SDK
    // must size deposits from ltxTokens(job), the $0.50 floor cannot cover it.
    "ltx-resolution-ladder-4k",
    // v8.32.0 LTX M1 economics: one submitProofOfWork per clip (5-param v8.14.0
    // form, host-wallet auth) through a ProofSubmit seam on CheckpointManager —
    // success strictly gated on a confirmed status-1 receipt. tokensClaimed ==
    // wire billing.tokens (§B). Disconnect-race machine on LtxTracker (pending
    // COUNT + deferred completion + dispute-window wait); mid-render disconnect
    // deterministically abandons (interrupt, forfeit, settle at 0).
    "ltx-payout",
    "ltx-proof-submit",
    "ltx-deferred-settlement",
    // v8.32.1 post-implementation review hardening: atomic accept gate
    // (mark_proof_pending rejects while a completeSessionJob is in flight —
    // the completing latch, self-expiring), take-at-wake deferral (peek →
    // dispute-window sleep → atomic take), post-render disconnect gates on the
    // stage sends (never bill a clip whose ltx_complete provably cannot land),
    // bounded recomputing "Too many" retry (3 attempts — lastProofTime moves
    // when sibling clips land), VRAM permit released before settlement sleeps.
    "ltx-payout-race-hardening",
    // v8.31.2 LTX real-template text-to-video (patch on the M0 sidecar): patch by the
    // template's own node names, accept a video output, advisory frame count, and pull
    // rendered files over ComfyUI's /view HTTP endpoint (no shared output volume).
    "ltx-real-template-t2v",
    "ltx-video-output",
    "ltx-http-view-fetch",
    // v8.31.3: DISABLE_LLM runs the node sidecar-only (no LLM GGUF load).
    "disable-llm-sidecar-only",
    // v8.31.4: publish the LTX allow-list bundle to S5 at startup, log the bundleCID.
    "ltx-bundle-s5-publish",
    // v8.31.5 LTX image-to-video (M1a): image-conditioned templates. Fetch the input
    // image from the S5 portal by capability CID (blake3-gated), decrypt, hash, upload
    // to ComfyUI, patch the LoadImage node(s), and bind imageHashes into a v2
    // inputCommitment. t2v stays byte-identical (empty imageHashes ⇒ M0 seven-field).
    "ltx-i2v",
    "ltx-s5-blob-fetch",
    "inputcommitment-v2",
    // v8.31.6: portal blob-download CID now uses the BlobIdentifier blake3 multihash
    // 0x1e (was 0x1f), matching s5.js BlobIdentifier(hash,0).toBase58() — fixes the
    // i2v input-image portal fetch 404.
    "ltx-i2v-blobcid-fix",
    // v8.31.7: input-image blob is fetched through the local S5 bridge's
    // downloadByCID (P2P) via ENHANCED_S5_URL, not a raw portal HTTP GET (which is
    // not a supported transport). Bridge must peer with the client's portal.
    "ltx-i2v-bridge-fetch",
    // v8.31.8 LTX first-last-frame to video (flf2v): two input images
    // (firstFrame/lastFrame) in a v3 bundle; patcher drives a CLIPTextEncode
    // positive prompt (.text) in addition to PrimitiveStringMultiline (.value).
    "ltx-flf2v",
    // v8.31.0 LTX 2.3 generation sidecar (M0)
    "ltx-video-sidecar",
    "comfyui-generation",
    "hdr-exr-output",
    "keyless-attestation",
    "megapixel-frame-billing",
    "fixed-field-commitments",
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
    // Think-tag normalize (v8.21.2)
    "think-tag-normalize",
    // Configurable penalties (v8.21.3)
    "configurable-penalties",
    "repeat-penalty-env",
    "frequency-penalty-env",
    "presence-penalty-env",
    // Sampler chain persistence (v8.21.5)
    "sampler-chain-persistence",
    // GLM-4 system prompt fix (v8.22.0)
    "glm4-system-prompt-fix",
    // Sampler reset after thinking (v8.22.1)
    "sampler-reset-after-think",
    // Fix </thought> detection for sampler reset (v8.22.2)
    "sampler-reset-thought-tag",
    // GLM-4 <|endoftext|> stop token to match Ollama (v8.22.3)
    "glm4-endoftext-stop",
    // Encrypted multi-turn conversation context (v8.22.5)
    "encrypted-multi-turn-context",
    "session-conversation-history",
    "extract-latest-user-message",
    // UTF-8 byte buffering (v8.23.0)
    "utf8-byte-buffering",
    "token-to-bytes",
    "max-consecutive-invalid-check",
    // Sequential transaction queue (v8.24.0)
    "tx-queue",
    "nonce-collision-prevention",
    "per-chain-fifo-queue",
    // Transcoder sidecar (v8.25.0)
    "transcoder-sidecar",
    "video-audio-transcoding",
    "transcoder-rest-client",
    "transcoder-jwt-auth",
    "transcoder-billing",
    "transcoder-rate-limiter",
    "websocket-transcode-handler",
    "transcode-progress-streaming",
    "http-transcode-endpoints",
    "docker-transcoder-sidecar",
    // Transcoding trustless verification (v8.26.0)
    "transcoding-quality-metrics",
    "transcoding-gop-proofs",
    "transcoding-merkle-tree",
    "transcoding-proof-checkpoints",
    "transcoding-job-validation",
    // Proof pipeline wired (v8.26.1)
    "proof-pipeline-wired",
    // Encrypted transcode source (v8.26.2)
    "encrypted-transcode-source",
    // Trim percent passthrough (v8.26.3)
    "trim-percent-passthrough",
    // Transcode capacity reporting & admission control (v8.26.4)
    "transcode-capacity",
    // Sidecar-based capacity tracking (v8.27.0)
    "sidecar-capacity",
    // Checkpoint lock contention fix (v8.27.1)
    "checkpoint-lock-split",
    // Default real proofs (v8.27.2)
    "default-real-proofs",
    // HLS pass-through (v8.28.0)
    "hls-passthrough",
    "hls-adaptive-bitrate",
    "preview-percent",
    "per-segment-encryption",
    // Qwen3.6-35B-A3B support (v8.29.0)
    "qwen35moe-architecture",
    "qwen36-35b-a3b",
    "llama-cpp-2-0-1-146",
    "cuda-13-runtime",
    "nccl-cumem-disable-wsl2",
    // TEE / confidential inference — mock backend, Phase 1-4 (v8.30.0)
    "tee-confidential-inference",
    "tee-attestation-mock",
    "encrypted-model-at-rest",
    "attested-dek-release",
    "tmpfs-weight-decrypt",
    "verify-then-load",
    "model-hash-binding",
    "policy-signer-binding",
    "tee-capability",
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    // v8.46.1 - Context-window clamp (Aug 19, 2026)
    "FIX: max_tokens is clamped to the context room that remains (context_size - prompt_tokens) instead of being taken at face value. The generation loop runs to prompt + max_tokens, so a request crossing context_size previously failed the decode MID-STREAM, after the session's escrow was open and tokens had been billed. It now ends cleanly with finish_reason=length. Requests that already fit are byte-for-byte unaffected",
    "NOTE: this clamps rather than rejects on purpose — a client that always sends a large max_tokens (the product UI sends 16000) is not misbehaving, and most such requests stop at EOS long before the wall. Rejecting up front would refuse requests that succeed today",
    // v8.46.0 - Qwen3.8-27B support + ChatML reasoning-effort control (Aug 18, 2026)
    "FEAT: llama-cpp-2 bumped 0.1.146 -> 0.1.154 — the qwen35 loader now splits n_layer from n_layer_all and consumes the NextN/MTP block, which is what Qwen3.8-27B needs (block_count 65 = 64 trunk + 1 MTP, plus blk.64.nextn.* tensors). On 0.1.146 that GGUF does not load at all: block 64 fell on the recurrent side of the full_attention_interval test and a required ssm tensor was missing",
    "FEAT: MODEL_CHAT_TEMPLATE=chatml now honours thinking mode. DEFAULT_THINKING_MODE / per-request thinking maps to Qwen's own reasoning_effort sentences (low, xhigh) injected at the head of the system message; medium injects nothing, matching Qwen's template; disabled prefills an empty <think></think> on the assistant turn, which is how Qwen suppresses reasoning",
    "NOTE: unset thinking mode leaves the ChatML prompt byte-for-byte as it was before v8.46.0 — Qwen3.6 hosts see no behaviour change until they opt in",
    "NOTE: Qwen3.8-27B is dense 27B, arch qwen35, same tokenizer and ChatML template as Qwen3.6-27B (eos 248046 / bos 248044 / pad 248055). Q8_0 is ~29 GB of weights; KV is only ~2 GiB at 32k because 48 of the 64 layers are gated-delta-net and keep a fixed recurrent state instead of a growing cache",
    // v8.43.0 - EXR masters (Aug 10, 2026)
    "FEAT: opt-in 16-bit EXR master delivery for every mode — new output kind `exr-frames`. Every pinned template (allowlist v19) carries a RadianceSaveEXR sink titled exr_output (16-bit half, ZIP, linearised on write); the patcher REMOVES it for legacy jobs (the one sanctioned structural edit) and REQUIRES it when exr-frames is requested",
    "FEAT: exr-frames delivery convention — frames[0] = preview mp4, frames[1..] = the EXR sequence in filename order; EXR count MUST equal billed frames (fail closed); manifest colour_encoding = linear-rec709 (legacy single-artefact jobs keep linear-HDR-from-LogC3 byte-for-byte)",
    "NOTE: legacy `exr-sequence` output kind is UNCHANGED (single H.264) — deployed helpers keep working without modification",
    // v8.42.0 - Operator-loadable moderation lists + verdict on completion (Jul 31, 2026)
    "FEAT: MODERATION_LIST_FILE (sha256:/pdq: entries) installs a genuine Loaded snapshot at startup — unlisted content can now CLEAR through /v1/moderate/frames; MODERATION_OWNHASH_FILE adds the definitive re-upload halt (block-only: it cannot clear); MODERATION_PDQ_MAX_DISTANCE tunes the near-match threshold (default 31, >256 or unparseable = boot-fatal; empty = unset)",
    "FEAT: broken list files DEGRADE fail-closed, never kill the node — ERROR log + 'moderation list degraded' /health issue + moderation_holds_total movement; #!allow-empty is the only legal empty list and only as the sole non-comment line",
    "FEAT: transcode_complete carries top-level moderation {verdict, reason} (omitted when no verdict); hold codes CONTENT_BLOCKED/CONTENT_FLAGGED/MODERATION_UNAVAILABLE documented in WEBSOCKET_API_SDK_GUIDE.md",
    "BREAKING: blocked-verdict reason value renamed \"csam-match\" -> \"hash-list-match\" (opaque display string; nothing should branch on it)",
    "FEAT: /v1/moderate nest body limit named at 20 MB — the counterpart of PART-A §3.2's 200-keyframes-per-POST batching; blocked verdicts are sticky across batches (set_if_not_downgrade), pinned by test",
    // v8.41.0 - CrossView novel-view mode (Jul 30, 2026)
    "FEAT: ltx-crossview-hdr template (allowlist v17) — novel view synthesis of a control clip via Cseti CrossView-Warp IC-LoRA (Apache-2.0) + DepthAnything v2 Small (Apache; Large is CC-BY-NC and must NOT be shipped). Single pass at the picked resolution; chain the upscale mode for 2x",
    "FEAT: optional `azimuth`/`elevation`/`distance` on ltx_generate, patched by class onto CrossViewWarp; ranges [-65,65]/[-25,40]/[0.5,2.0] (trained yellow-zone envelope); absent = pinned mild pose (20/0/1.0); REJECTED on templates with no camera node",
    "FEAT: `Frame Count` titled patch handle — one INT feeds both VHS frame_load_cap and the latent length, so billed == loaded == rendered by construction",
    // v8.40.0 - IC-LoRA guide strength (Jul 28, 2026)
    "FEAT: optional `strength` on ltx_generate — overrides LTXAddVideoICLoRAGuide.strength ((0,1]; the pinned graphs carry 1.0 = maximum source adherence). Lowering it hands the prompt authority over the source, which is what object edits (recolour/replace) need. Patched by CLASS so the retitled ingredients guide is covered; absent = pinned constant, wire and output byte-identical to v8.39.x",
    "GUARD: a strength sent for a template with no IC-LoRA guide node (t2v/i2v/flf2v/iclora/upscale) is REJECTED at validation — a paid render must never bill with its one requested knob silently ignored",
    // v8.39.1 - LTX panic safety (Jul 25, 2026)
    "FIX: a panic in the LTX generation core no longer strands session escrow — the core runs under catch_unwind and every exit funnels through the single-exit cleanup, forfeiting the clip's pending proof",
    "FIX: a caught panic now sends a terminal GENERATION_FAILED frame instead of leaving the client waiting for LTX_JOB_TIMEOUT_SECS",
    "SCOPE: panic-induced stranding only — SIGKILL/OOM/SIGTERM/restart lose in-memory pending state and are NOT covered",
    // v8.39.3 - honest settlement logging (Jul 26, 2026)
    "FIX: settlement no longer logs \"❌ Job N has NO TRACKER — payment calculation may be affected!\" on every successful LTX render. job_trackers counts LLM INFERENCE tokens; an LTX session legitimately never appears there because it claims tokens via submitProofOfWork, and completeSessionJob(jobId, conversationCID) takes no token count — so payment was never affected. The manager now records which jobs settled via an LTX proof and logs the three cases distinctly: LLM-tracked, LTX-proof-settled (info/debug), and genuinely-nothing-billed (warn)",
    // v8.39.2 - OQ-L24 wedged-client write bound (Jul 25, 2026)
    "FIX: OQ-L24 — every LTX WebSocket write is now BOUNDED (LTX_WS_WRITE_TIMEOUT_SECS, default 300s). A client that held the socket open but stopped reading TCP parked the write for ever, filling the 32-slot progress channel so the generation core could never return: the VRAM permit was never released and the pending proof never forfeited, stranding session escrow until the user paid a triggerSessionTimeout reclaim. No panic required, fully client-triggered",
    "FIX: the accept-path ack write is bounded too — it runs after the permit and pending mark are taken but BEFORE the task is spawned, so parking there leaked the permit with no owner (MAX_CONCURRENT_GENERATIONS defaults to 1, disabling LTX until restart); a gone client now forfeits the pending, drops the task and takes the settlement path",
    // v8.39.0 - FC1.6 vault-session auth + /v1/health alias (Jul 23, 2026)
    "FEAT: POST /v1/session-auth — backend-signed FC1-SESSION-AUTH digest pre-authorises a delegated client address for a vault-paid session (scheme fc1-session-auth-v1)",
    "FEAT: WS session gate admits the on-chain depositor OR the pre-authorised client for vault-paid sessions",
    "FEAT: GET /v1/health alias of /health — SDK browser builds probe /v1/health; hosts without the alias read as unreachable to browser clients",
    // v8.34.0 - LTX Duration + fps correction (Jul 3, 2026)
    "FEAT: User-selectable LTX clip duration 5..=15s — frames = fps·secs + 1 (allow-list bundle v5)",
    "FEAT: Corrected advertised fps to LTX 2.3 native rates [24,25,48,50] (dropped never-supported 30, added 48/50)",
    "FEAT: bounds.frames widened to {121,751}; new validate_duration enforces exact whole seconds in range",
    "FEAT: Patcher drives the pinned graphs' existing Duration handle so rendered length == billed frames (no template/hash change)",
    "BUNDLE: allowListVersion 4 -> 5; bundleHash moves; the three templateHashes are UNCHANGED",
    // v8.29.0 - Qwen3.6-35B-A3B Support (May 21, 2026)
    "FEAT: llama-cpp-2 bumped 0.1.122 -> 0.1.146 — adds qwen35moe (Qwen3.6-35B-A3B) architecture support",
    "BUILD: pinned llama-cpp-2 =0.1.146 in Cargo.toml (Cargo.lock is gitignored)",
    "BUILD: Dockerfile.production base -> nvidia/cuda:13.0.0-runtime (matches NCCL 2.27+cuda13; libnccl2 from base image)",
    "BUILD: build.rs emits cargo:rustc-link-lib=nccl — upstream llama-cpp-sys-2 0.1.146 omits it though bundled llama.cpp now calls ncclCommInitAll/ncclAllReduce",
    "DEPLOY: WSL2 single-GPU hosts require NCCL_CUMEM_ENABLE=0 + shm_size/ipc:host in docker-compose (cuMem VMM unsupported under WSL2 GPU passthrough)",
    "DEVCONTAINER: .devcontainer/Dockerfile now installs rzup + cargo-risczero for --features real-ezkl builds",
    // v8.28.0 - HLS Pass-Through (Apr 11, 2026)
    "FEAT: VideoFormat.hls and VideoFormat.hls_time fields for HLS adaptive bitrate streaming",
    "FEAT: previewPercent request field for per-segment encryption boundary",
    "FEAT: submit_transcode() conditionally includes preview_percent query param",
    // v8.27.2 - Default Real Proofs (Apr 3, 2026)
    "BREAKING: real-ezkl feature now included in default features — cargo build --release produces real STARK proofs",
    "BREAKING: Mock proofs require explicit opt-in via --no-default-features --features inference",
    // v8.27.1 - Checkpoint Lock Contention Fix (Mar 27, 2026)
    "FIX: publish_checkpoint() no longer holds write lock during S5 uploads",
    "FIX: set_recovery_public_key() no longer blocked by concurrent checkpoint uploads",
    "FIX: session_init_ack timeout during orchestration synthesise phase resolved",
    // v8.27.0 - Sidecar Capacity Integration (Mar 22, 2026)
    "FEAT: Real sidecar status via GET /status — replaces local atomic counter",
    "FEAT: CachedSidecarStatus with 2s TTL and stale-on-error fallback",
    "FEAT: /v1/transcode/capacity returns queued_jobs from sidecar",
    "FEAT: MAX_CONCURRENT_TRANSCODES moved to sidecar env (docker-compose)",
    "BREAKING: Removed TranscodeSlotGuard, try_acquire/release_transcode_slot, has_transcode_capacity",
    // v8.26.4 - Transcode Capacity Reporting (Mar 21, 2026)
    "FEAT: GET /v1/transcode/capacity endpoint for SDK host selection and load balancing",
    "FEAT: TRANSCODE_CAPACITY_FULL error code when all NVENC slots in use (WS + HTTP)",
    "FEAT: MAX_CONCURRENT_TRANSCODES env var (default 3) for GPU session limit",
    "FEAT: RAII TranscodeSlotGuard ensures slot release on task completion, error, or panic",
    // v8.26.3 - Trim Percent Passthrough (Mar 21, 2026)
    "FEAT: VideoFormat.trim_percent field passes through to transcoder sidecar for preview trimming",
    // v8.26.2 - Encrypted Transcode Source (Mar 21, 2026)
    "FIX: S5 bridge /api/locations/:hash now uses Host header instead of hardcoded localhost",
    "FEAT: S5 bridge GET /s5/download/:hash route for raw base64url hash blob download",
    // v8.26.1 - Proof Pipeline Wired (Mar 19, 2026)
    "FEAT: proofTreeCID and proofTreeRootHash populated in transcode_complete when jobId provided",
    "FEAT: STARK proof generated, Merkle tree built and uploaded to S5 on transcode completion",
    // v8.26.0 - Transcoding Trustless Verification (Mar 19, 2026)
    "FEAT: Quality metrics (PSNR/SSIM) parsing from ffmpeg for transcode verification",
    "FEAT: GOP-level progress tracking with estimated GOP counts in progress messages",
    "FEAT: Keccak256 Merkle tree over GOP proofs for cryptographic verification",
    "FEAT: GOP proof builder with Risc0 STARK integration (reuses 4-hash witness)",
    "FEAT: Transcoding checkpoint submission with billing token conversion",
    "FEAT: Format spec hashing for contract-compatible modelId generation",
    "FEAT: Sidecar cancel endpoint support for transcode cancellation",
    "FEAT: isEncrypted default changed from false to true for transcoding",
    "FEAT: transcode_complete message now includes proofTreeCID, proofTreeRootHash, qualityMetrics fields",
    "FEAT: transcode_progress message now includes gopInfo when duration is known",
    // v8.25.0 - Transcoder Sidecar Integration (Mar 19, 2026)
    "FEAT: Transcoder sidecar integration for video/audio transcoding via REST API",
    "FEAT: TranscoderClient with JWT auth, submit/poll, health check",
    "FEAT: Transcoding billing (duration × resolution × codec × encryption factors)",
    "FEAT: Per-session transcoding rate limiter (default 3 per 5-min window)",
    "FEAT: WebSocket transcode handler with background progress streaming via mpsc + tokio::select!",
    "FEAT: POST /v1/transcode and GET /v1/transcode/:task_id HTTP endpoints",
    "FEAT: Docker transcoder-sidecar service in docker-compose.prod.yml",
    "FEAT: TRANSCODER_ENDPOINT and FABSTIR_TRANSCODER_JWT env vars (pre-shared JWT token)",
    // v8.24.0 - Sequential Transaction Queue (Mar 11, 2026)
    "FEAT: Per-chain FIFO transaction queue prevents nonce collisions across checkpoint/settlement/registration",
    "FEAT: Automatic nonce retry with exponential backoff for transient nonce errors",
    "FEAT: CheckpointManager now uses enqueue_transaction instead of send_transaction (3 call sites migrated)",
    "FIX: Removed manual nonce retry in complete_session_job — queue handles it automatically",
    // v8.23.0 - UTF-8 Byte Buffering (Mar 10, 2026)
    "FIX: Multi-byte UTF-8 characters split across BPE tokens no longer vanish from output",
    "FIX: Generation loop uses token_to_bytes + byte buffer instead of token_to_str",
    "FIX: MAX_CONSECUTIVE_INVALID now enforced — breaks generation after 10 invalid UTF-8 bytes",
    // v8.22.5 - Encrypted multi-turn conversation context (Mar 6, 2026)
    "FIX: Encrypted WebSocket sessions now maintain server-side conversation history for proper multi-turn formatting",
    "FIX: GLM-4 (and other models) no longer see conversation history as a single user message",
    "FIX: extract_latest_user_message() strips SDK's inline history when session context is available",
    // v8.22.4 - Disable GLM-4 auto /think injection (Mar 1, 2026)
    "FIX: Removed auto /think injection on GLM-4 — caused degenerate meta-reasoning loops on multi-turn",
    // v8.22.3 - GLM-4 <|endoftext|> stop token (Mar 1, 2026)
    "FIX: Added <|endoftext|> (EOS) to GLM-4 stop tokens — matches Ollama template",
    "FIX: Sampler reset uses contains() for robust thinking close tag detection",
    // v8.22.1 - Sampler Reset After Thinking (Mar 1, 2026)
    "FIX: Sampler penalties reset after </think> block — prevents thinking tokens from poisoning answer generation",
    // v8.22.0 - GLM-4 System Prompt Fix (Mar 1, 2026)
    "FIX: GLM-4 default system prompt simplified — removed RAG instruction that caused hallucinated 'reference material'",
    // v8.21.5 - Sampler Chain Persistence (Mar 1, 2026)
    "FIX: Sampler chain now persists across tokens — penalties actually apply",
    // v8.21.3 - Configurable Penalties (Feb 28, 2026)
    "FEAT: REPEAT_PENALTY env var for configurable repeat penalty (default: 1.1)",
    "FEAT: FREQUENCY_PENALTY env var for frequency-based token penalty (default: 0.0)",
    "FEAT: PRESENCE_PENALTY env var for presence-based token penalty (default: 0.0)",
    "FEAT: PENALTY_LAST_N env var for penalty lookback window (default: 256)",
    // v8.21.2 - Think-tag normalize (Feb 28, 2026)
    "FIX: <thought> special token normalized to <think> for consistent thinking tags",
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
        assert_eq!(VERSION_MINOR, 47);
        assert_eq!(VERSION_PATCH, 0);
        assert!(FEATURES.contains(&"multi-chain"));
        assert!(FEATURES.contains(&"dual-pricing"));
        // v8.36.0 BL4 video-edit trio (bundle v7: outpaint/edit/restore)
        assert!(FEATURES.contains(&"ltx-video-edit"));
        // v8.35.0 LTX IC-LoRA union control (bundle v6, videos on the seam)
        assert!(FEATURES.contains(&"ltx-iclora"));
        // v8.34.0 LTX duration + fps correction (bundle v5)
        assert!(FEATURES.contains(&"ltx-duration"));
        // v8.32.0 LTX M1 economics (submitProofOfWork per clip)
        assert!(FEATURES.contains(&"ltx-payout"));
        assert!(FEATURES.contains(&"ltx-proof-submit"));
        assert!(FEATURES.contains(&"ltx-deferred-settlement"));
        // v8.32.1 review hardening (atomic accept gate, delivery gates)
        assert!(FEATURES.contains(&"ltx-payout-race-hardening"));
        // v8.33.0 bundle v4 resolution ladder
        assert!(FEATURES.contains(&"ltx-resolution-ladder-4k"));
        // v8.31.0 LTX 2.3 generation sidecar (M0)
        assert!(FEATURES.contains(&"ltx-video-sidecar"));
        assert!(FEATURES.contains(&"comfyui-generation"));
        assert!(FEATURES.contains(&"hdr-exr-output"));
        assert!(FEATURES.contains(&"keyless-attestation"));
        assert!(FEATURES.contains(&"megapixel-frame-billing"));
        assert!(FEATURES.contains(&"fixed-field-commitments"));
        // v8.31.5 LTX image-to-video (M1a)
        assert!(FEATURES.contains(&"ltx-i2v"));
        assert!(FEATURES.contains(&"ltx-s5-blob-fetch"));
        assert!(FEATURES.contains(&"inputcommitment-v2"));
        assert!(FEATURES.contains(&"ltx-i2v-blobcid-fix"));
        assert!(FEATURES.contains(&"ltx-i2v-bridge-fetch"));
        assert!(FEATURES.contains(&"ltx-flf2v"));
        // v8.30.0 TEE / confidential inference (mock backend, Phase 1-4)
        assert!(FEATURES.contains(&"tee-confidential-inference"));
        assert!(FEATURES.contains(&"tee-attestation-mock"));
        assert!(FEATURES.contains(&"encrypted-model-at-rest"));
        assert!(FEATURES.contains(&"attested-dek-release"));
        assert!(FEATURES.contains(&"tmpfs-weight-decrypt"));
        assert!(FEATURES.contains(&"verify-then-load"));
        assert!(FEATURES.contains(&"model-hash-binding"));
        assert!(FEATURES.contains(&"tee-capability"));
        // v8.29.0 Qwen3.6 support
        assert!(FEATURES.contains(&"qwen35moe-architecture"));
        assert!(FEATURES.contains(&"llama-cpp-2-0-1-146"));
        assert!(FEATURES.contains(&"cuda-13-runtime"));
        assert!(FEATURES.contains(&"nccl-cumem-disable-wsl2"));
        // v8.27.1 checkpoint lock fix
        assert!(FEATURES.contains(&"checkpoint-lock-split"));
        // v8.27.0 sidecar capacity
        assert!(FEATURES.contains(&"sidecar-capacity"));
        // v8.26.4 transcode capacity
        assert!(FEATURES.contains(&"transcode-capacity"));
        // v8.26.3 trim percent passthrough
        assert!(FEATURES.contains(&"trim-percent-passthrough"));
        // v8.26.2 encrypted transcode source
        assert!(FEATURES.contains(&"encrypted-transcode-source"));
        // v8.26.1 proof pipeline wired
        assert!(FEATURES.contains(&"proof-pipeline-wired"));
        // v8.26.0 transcoder-trustless
        assert!(FEATURES.contains(&"transcoding-quality-metrics"));
        assert!(FEATURES.contains(&"transcoding-gop-proofs"));
        assert!(FEATURES.contains(&"transcoding-merkle-tree"));
        assert!(FEATURES.contains(&"transcoding-proof-checkpoints"));
        assert!(FEATURES.contains(&"transcoding-job-validation"));
        // v8.25.0 transcoder-sidecar
        assert!(FEATURES.contains(&"transcoder-sidecar"));
        assert!(FEATURES.contains(&"video-audio-transcoding"));
        assert!(FEATURES.contains(&"transcoder-rest-client"));
        assert!(FEATURES.contains(&"transcoder-billing"));
        assert!(FEATURES.contains(&"websocket-transcode-handler"));
        assert!(FEATURES.contains(&"http-transcode-endpoints"));
        // v8.24.0 tx-queue
        assert!(FEATURES.contains(&"tx-queue"));
        assert!(FEATURES.contains(&"nonce-collision-prevention"));
        assert!(FEATURES.contains(&"per-chain-fifo-queue"));
        // v8.23.0 utf8 byte buffering
        assert!(FEATURES.contains(&"utf8-byte-buffering"));
        assert!(FEATURES.contains(&"token-to-bytes"));
        assert!(FEATURES.contains(&"max-consecutive-invalid-check"));
        // v8.22.5 encrypted multi-turn context
        assert!(FEATURES.contains(&"encrypted-multi-turn-context"));
        assert!(FEATURES.contains(&"session-conversation-history"));
        assert!(FEATURES.contains(&"extract-latest-user-message"));
        // v8.22.3 GLM-4 endoftext stop token
        assert!(FEATURES.contains(&"glm4-endoftext-stop"));
        // v8.22.2 sampler reset thought tag fix
        assert!(FEATURES.contains(&"sampler-reset-thought-tag"));
        // v8.22.1 sampler reset after think
        assert!(FEATURES.contains(&"sampler-reset-after-think"));
        // v8.22.0 GLM-4 system prompt fix
        assert!(FEATURES.contains(&"glm4-system-prompt-fix"));
        // v8.21.5 sampler chain persistence
        assert!(FEATURES.contains(&"sampler-chain-persistence"));
        // v8.21.3 configurable penalties
        assert!(FEATURES.contains(&"configurable-penalties"));
        assert!(FEATURES.contains(&"repeat-penalty-env"));
        assert!(FEATURES.contains(&"frequency-penalty-env"));
        assert!(FEATURES.contains(&"presence-penalty-env"));
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
        // v8.21.2 think-tag normalize
        assert!(FEATURES.contains(&"think-tag-normalize"));
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
        assert!(version.contains("8.46.1"));
        assert!(version.contains("2026-08-19"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.47.0-vault-session-hardening-2026-08-19");
        assert_eq!(VERSION_NUMBER, "8.47.0");
        assert_eq!(BUILD_DATE, "2026-08-19");
    }

    #[test]
    fn test_moderation_lists_features() {
        assert!(FEATURES.contains(&"moderation-operator-lists"));
        assert!(FEATURES.contains(&"moderation-verdict-on-complete"));
        assert!(FEATURES.contains(&"moderation-hold-codes"));
        assert!(FEATURES.contains(&"hash-list-match-sentinel"));
        assert!(FEATURES.contains(&"moderation-degraded-health"));
        assert!(FEATURES.contains(&"moderation-batched-frames"));
    }

    #[test]
    fn test_ltx_tracker_log_features() {
        assert!(FEATURES.contains(&"ltx-tracker-log-honesty"));
    }

    #[test]
    fn test_ltx_ws_write_bound_features() {
        assert!(FEATURES.contains(&"ltx-ws-write-bound"));
        assert!(FEATURES.contains(&"oq-l24-wedged-client"));
    }

    #[test]
    fn test_ltx_panic_safety_features() {
        assert!(FEATURES.contains(&"ltx-panic-safety"));
        assert!(FEATURES.contains(&"ltx-panic-forfeits-pending-proof"));
    }

    #[test]
    fn test_fc16_session_auth_features() {
        assert!(FEATURES.contains(&"fc1-session-auth"));
        assert!(FEATURES.contains(&"vault-delegated-sessions"));
        assert!(FEATURES.contains(&"v1-health-alias"));
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
