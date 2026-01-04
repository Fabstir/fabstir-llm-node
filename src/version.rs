// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Version information for the Fabstir LLM Node

/// Full version string with feature description
pub const VERSION: &str = "v8.6.19-vision-body-limit-fix-2026-01-04";

/// Semantic version number
pub const VERSION_NUMBER: &str = "8.6.19";

/// Major version number
pub const VERSION_MAJOR: u32 = 8;

/// Minor version number
pub const VERSION_MINOR: u32 = 6;

/// Patch version number
pub const VERSION_PATCH: u32 = 19;

/// Build date
pub const BUILD_DATE: &str = "2026-01-04";

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
];

/// Supported chain IDs
pub const SUPPORTED_CHAINS: &[u64] = &[
    84532, // Base Sepolia
    5611,  // opBNB Testnet
];

/// Breaking changes from previous version
pub const BREAKING_CHANGES: &[&str] = &[
    "FEAT: Added POST /v1/ocr endpoint for OCR using PaddleOCR (CPU-only)",
    "FEAT: Added POST /v1/describe-image endpoint for image description using Florence-2 (CPU-only)",
    "FEAT: Added GET /v1/models?type=vision to list available vision models",
    "FEAT: Added OCR_MODEL_PATH and FLORENCE_MODEL_PATH environment variables",
    "FEAT: Vision models run on CPU only (no GPU VRAM competition with LLM)",
    "FIX: Added vision routes to ApiServer.create_router() (v8.6.1)",
    "FIX: Switched to English PP-OCRv5 models for accurate English text OCR (v8.6.3)",
    "FIX: Fixed recognition height to 48 (ONNX model requirement) (v8.6.5)",
    "FIX: Added word spacing post-processing for English OCR output (v8.6.6)",
    "FIX: Increased body limit to 20MB for vision endpoints to support large images (v8.6.7)",
    "FIX: Added embed_tokens.onnx support for Florence-2 decoder (v8.6.8)",
    "FIX: Use correct Florence-2 task tokens: <cap> and <dcap> (v8.6.11)",
    "FIX: Remove trailing EOS from Florence input tokens for autoregressive generation (v8.6.12)",
    "FIX: Manual task token construction [BOS, <dcap>], debug logging, encoder validation (v8.6.13)",
    "FIX: Use <cap> token instead of <dcap> - ONNX model handles simple caption better (v8.6.14)",
    "FIX: Use BOS-only start without task tokens - ONNX export may have broken task token handling (v8.6.15)",
    "FIX: Use natural language prompts instead of task tokens - 'A photo of', 'The image shows' work correctly (v8.6.16)",
    "FIX: Add repetition masking to Florence decoder - prevents 'shows shows shows...' loops (v8.6.17)",
    "FIX: Use ImageNet normalization and CenterCrop for Florence preprocessing (was CLIP/Letterbox) (v8.6.18)",
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
        assert_eq!(VERSION_MINOR, 6);
        assert_eq!(VERSION_PATCH, 18);
        assert!(FEATURES.contains(&"multi-chain"));
        assert!(FEATURES.contains(&"dual-pricing"));
        assert!(FEATURES.contains(&"cpu-ocr"));
        assert!(FEATURES.contains(&"cpu-vision"));
        assert!(FEATURES.contains(&"vision-20mb-body-limit"));
        assert!(SUPPORTED_CHAINS.contains(&84532));
    }

    #[test]
    fn test_version_string() {
        let version = get_version_string();
        assert!(version.contains("8.6.18"));
        assert!(version.contains("2026-01-03"));
    }

    #[test]
    fn test_version_format() {
        assert_eq!(VERSION, "v8.6.18-florence-preprocessing-fix-2026-01-03");
        assert_eq!(VERSION_NUMBER, "8.6.18");
        assert_eq!(BUILD_DATE, "2026-01-03");
    }
}
