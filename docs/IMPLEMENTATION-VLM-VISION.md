# IMPLEMENTATION - VLM Sidecar for Vision with ONNX Fallback

## Status: COMPLETE

**Version**: v8.15.4-vlm-vision-ws-ocr
**Previous Version**: v8.15.2-repeat-penalty-window
**Start Date**: 2026-02-08
**Approach**: Strict TDD with bounded autonomy - one sub-phase at a time

---

## Overview

Replace the CPU-only PaddleOCR + Florence-2 ONNX pipeline with a modern VLM sidecar (Qwen3-VL via llama-server) for dramatically better OCR and image description quality. Keep existing ONNX pipeline as fallback for consumer GPU hosts that lack VRAM for a VLM.

### What's Changing

| Component | Current | Target | Impact |
|-----------|---------|--------|--------|
| OCR model | PaddleOCR v4 (CPU ONNX, ~13MB) | VLM sidecar (Qwen3-VL, ~5GB Q4) | **Quality leap** |
| Image description | Florence-2-large (CPU ONNX, ~2.6GB) | VLM sidecar (same model) | **Quality leap** |
| Fallback | None | Existing ONNX pipeline (CPU) | **New resilience** |
| Vision routing | Direct ONNX calls | VLM HTTP → ONNX fallback | **New module** |
| Model field | Hardcoded "paddleocr" / "florence-2" | Dynamic based on provider | **Response change** |

### Architecture

```
/v1/ocr or /v1/describe-image
            │
    ┌───────▼────────┐
    │  API Handler    │
    │  (ocr/describe) │
    └───────┬────────┘
            │
    ┌───────▼──────────────┐
    │  VisionModelManager  │
    │  has_vlm()?          │
    └───┬──────────┬───────┘
        │          │
       YES         NO
        │          │
  ┌─────▼─────┐  ┌▼────────────┐
  │ VlmClient │  │ ONNX Models │
  │ (reqwest)  │  │ PaddleOCR   │
  │ → llama-  │  │ Florence-2  │
  │   server   │  │ (CPU)       │
  └─────┬─────┘  └─────────────┘
        │
  ┌─────▼─────────────┐
  │ OpenAI-compatible  │
  │ /v1/chat/          │
  │ completions API    │
  └────────────────────┘
```

### Key Architectural Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| VLM serving | llama-server (GGUF) | Same tech stack as main LLM; OpenAI-compatible API |
| Fallback | Existing ONNX pipeline | Consumer GPUs (RTX 4090) can't spare VRAM; works on CPU |
| Selection | `VLM_ENDPOINT` env var | Host operator decides; automatic if not set |
| API format | OpenAI chat/completions | Standard; supports image_url with base64 |
| HTTP client | reqwest | Already in Cargo.toml; patterns exist in codebase |

### VRAM Budget

| Host Type | Main LLM | VLM | Total | GPU |
|-----------|----------|-----|-------|-----|
| High-end | GLM-4.7-Flash Q8_K (~70GB) | Qwen3-VL-8B Q4 (~5GB) | ~75GB | 96GB A6000 |
| Consumer | gpt-oss-20B Q8_K (~20GB) | None (ONNX CPU) | ~20GB | 24GB RTX 4090 |

---

## Implementation Status

| Phase | Sub-phase | Description | Status | Tests | Max Lines |
|-------|-----------|-------------|--------|-------|-----------|
| 1 | 1.1 | VlmClient struct + new() + health_check() (TDD) | ✅ Done | 5/5 | 60 |
| 1 | 1.2 | VlmClient::ocr() method (TDD) | ✅ Done | 8/8 | 80 |
| 1 | 1.3 | VlmClient::describe() method (TDD) | ✅ Done | 11/11 | 60 |
| 2 | 2.1 | Extend VisionModelConfig + VisionModelManager | ✅ Done | 8/8 | 40 |
| 3 | 3.1 | Add model param to response constructors | ✅ Done | 11/11 | 20 |
| 3 | 3.2 | Update OCR handler (VLM priority + ONNX fallback) | ✅ Done | 18/18 | 40 |
| 3 | 3.3 | Update describe-image handler (VLM priority + ONNX fallback) | ✅ Done | 19/19 | 40 |
| 4 | 4.1 | Add VLM env vars to main.rs + docker-compose | ✅ Done | 8/8 | 30 |
| 4 | 4.2 | Create download_vlm_model.sh | ✅ Done | syntax-ok | 80 |
| 5 | 5.1 | Bump version to 8.15.3 | ✅ Done | 8/8 | 20 |
| 5 | 5.2 | Run full test suite + lint | ✅ Done | 820/827 (7 pre-existing) | 0 |
| 6 | 6.1 | Tests for vision image extraction + prompt augmentation | ✅ Done | 6/6 | 80 |
| 6 | 6.2 | `process_vision_images()` helper function | ✅ Done | 2/2 | 50 |
| 6 | 6.3 | Integrate into encrypted_message handler | ✅ Done | 1/1 | 20 |
| 6 | 6.4 | Integrate into plaintext inference handler | ✅ Done | 1/1 | 20 |
| 6 | 6.5 | Full test suite + rebuild tarball | ✅ Done | 820/827 (7 pre-existing) | 0 |
| 7 | 7.1 | Tests for VLM token capture + billing | ✅ Done | 4/4 | 40 |
| 7 | 7.2 | Capture usage from VLM response | ✅ Done | 2/2 | 25 |
| 7 | 7.3 | Return + track VLM tokens in server.rs | ✅ Done | 1/1 | 30 |
| 7 | 7.4 | Full test suite + rebuild tarball | ✅ Done | 822/829 (7 pre-existing) | 0 |
| **Total** | | | **100%** | **83 total** | **~660** |

---

## Phase 1: VlmClient Module (TDD)

### Sub-phase 1.1: VlmClient Struct + new() + health_check() (TDD)

**Goal**: Create the VLM HTTP client module with construction and health checking

**Status**: ✅ Done

**Files**:
- `src/vision/vlm_client.rs` (**NEW**, max 60 lines)
- `src/vision/mod.rs` (add 1 line: `pub mod vlm_client;`)

**Max Lines Changed**: 60

**Dependencies**: None

**Tasks**:
- [x] Write test `test_vlm_client_new` — create client, verify endpoint/model stored
- [x] Write test `test_vlm_client_health_check_unreachable` — returns `false` for bad endpoint
- [x] Write test `test_vlm_client_default_timeout` — verify 30s timeout
- [x] Write test `test_vlm_client_model_name` — verify model_name() accessor
- [x] Implement `VlmClient` struct with `client: reqwest::Client`, `endpoint: String`, `model_name: String`
- [x] Implement `new(endpoint, model_name)` with reqwest Client builder (30s timeout)
- [x] Implement `health_check()` — GET `{endpoint}/health`, return `Ok(true)` on 200
- [x] Implement `model_name()` accessor
- [x] Add `pub mod vlm_client;` to `src/vision/mod.rs`

**Implementation Target** (~45 lines):
```rust
use anyhow::Result;
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Client for calling a VLM sidecar service via OpenAI-compatible API
pub struct VlmClient {
    client: Client,
    endpoint: String,
    model_name: String,
}

impl VlmClient {
    /// Create a new VLM client
    pub fn new(endpoint: &str, model_name: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        let endpoint = endpoint.trim_end_matches('/').to_string();
        info!("VLM client configured: endpoint={}, model={}", endpoint, model_name);

        Ok(Self {
            client,
            endpoint,
            model_name: model_name.to_string(),
        })
    }

    /// Get the model name
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Check if the VLM sidecar is healthy
    pub async fn health_check(&self) -> bool {
        match self.client.get(format!("{}/health", self.endpoint)).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                debug!("VLM health check failed: {}", e);
                false
            }
        }
    }
}
```

**Verification**:
```bash
timeout 60 cargo test --lib -- vision::vlm_client::tests -- --test-threads=1
```

---

### Sub-phase 1.2: VlmClient::ocr() Method (TDD)

**Goal**: Implement OCR via VLM using OpenAI-compatible chat/completions API

**Status**: ✅ Done

**File**: `src/vision/vlm_client.rs` (add ~80 lines)

**Max Lines Changed**: 80

**Dependencies**: Sub-phase 1.1 must be complete

**Tasks**:
- [x] Write test `test_ocr_request_format` — verify OpenAI request JSON structure
- [x] Write test `test_ocr_response_parsing` — parse `choices[0].message.content` from mock JSON
- [x] Write test `test_ocr_prompt_construction` — verify OCR system prompt
- [x] Implement request/response serde structs (`ChatRequest`, `ChatResponse`, `ChatMessage`, `ContentPart`)
- [x] Implement OCR prompt template (instruct VLM to extract text verbatim)
- [x] Implement `ocr(&self, base64_image: &str, format: &str) -> Result<VlmOcrResult>`

**Serde structs** (~25 lines):
```rust
#[derive(serde::Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(serde::Serialize)]
struct ChatMessage {
    role: String,
    content: serde_json::Value,  // String or Vec<ContentPart>
}

#[derive(serde::Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(serde::Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(serde::Deserialize)]
struct ChatResponseMessage {
    content: String,
}
```

**OCR method** (~30 lines):
```rust
/// Result from VLM-based OCR
pub struct VlmOcrResult {
    pub text: String,
    pub model: String,
    pub processing_time_ms: u64,
}

pub async fn ocr(&self, base64_image: &str, format: &str) -> Result<VlmOcrResult> {
    let start = std::time::Instant::now();
    let data_url = format!("data:image/{};base64,{}", format, base64_image);

    let request = ChatRequest {
        model: self.model_name.clone(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: serde_json::json!([
                {"type": "text", "text": "Extract all text from this image. Return only the extracted text, preserving the original layout and formatting as much as possible. If no text is found, respond with an empty string."},
                {"type": "image_url", "image_url": {"url": data_url}}
            ]),
        }],
        max_tokens: 4096,
        temperature: 0.1,
    };

    let response = self.client
        .post(format!("{}/v1/chat/completions", self.endpoint))
        .json(&request)
        .send()
        .await?;

    let chat_response: ChatResponse = response.json().await?;
    let text = chat_response.choices.first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    Ok(VlmOcrResult {
        text,
        model: self.model_name.clone(),
        processing_time_ms: start.elapsed().as_millis() as u64,
    })
}
```

**Verification**:
```bash
timeout 60 cargo test --lib -- vision::vlm_client::tests -- --test-threads=1
```

---

### Sub-phase 1.3: VlmClient::describe() Method (TDD)

**Goal**: Implement image description via VLM

**Status**: ✅ Done

**File**: `src/vision/vlm_client.rs` (add ~60 lines)

**Max Lines Changed**: 60

**Dependencies**: Sub-phase 1.2 must be complete (reuses ChatRequest/ChatResponse)

**Tasks**:
- [x] Write test `test_describe_prompt_brief` — verify brief prompt
- [x] Write test `test_describe_prompt_detailed` — verify detailed prompt
- [x] Write test `test_describe_response_parsing` — parse description from response
- [x] Implement prompt construction for detail levels ("brief", "detailed", "comprehensive")
- [x] Implement `describe(&self, base64_image, format, detail, prompt) -> Result<VlmDescribeResult>`

**Implementation Target** (~40 lines):
```rust
pub struct VlmDescribeResult {
    pub description: String,
    pub model: String,
    pub processing_time_ms: u64,
}

pub async fn describe(
    &self,
    base64_image: &str,
    format: &str,
    detail: &str,
    custom_prompt: Option<&str>,
) -> Result<VlmDescribeResult> {
    let start = std::time::Instant::now();
    let data_url = format!("data:image/{};base64,{}", format, base64_image);

    let text_prompt = custom_prompt.unwrap_or_else(|| match detail {
        "brief" => "Describe this image in one sentence.",
        "comprehensive" => "Provide a comprehensive, detailed analysis of this image. Describe all objects, people, text, colors, composition, and any notable details.",
        _ => "Describe this image in detail, including objects, scene, colors, and any text visible.",
    });

    let request = ChatRequest {
        model: self.model_name.clone(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: serde_json::json!([
                {"type": "text", "text": text_prompt},
                {"type": "image_url", "image_url": {"url": data_url}}
            ]),
        }],
        max_tokens: match detail {
            "brief" => 100,
            "comprehensive" => 500,
            _ => 300,
        },
        temperature: 0.3,
    };

    let response = self.client
        .post(format!("{}/v1/chat/completions", self.endpoint))
        .json(&request)
        .send()
        .await?;

    let chat_response: ChatResponse = response.json().await?;
    let description = chat_response.choices.first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    Ok(VlmDescribeResult {
        description,
        model: self.model_name.clone(),
        processing_time_ms: start.elapsed().as_millis() as u64,
    })
}
```

**Verification**:
```bash
timeout 60 cargo test --lib -- vision::vlm_client::tests -- --test-threads=1
```

---

## Phase 2: VisionModelManager Integration

### Sub-phase 2.1: Extend VisionModelConfig + VisionModelManager

**Goal**: Add VLM client as optional provider in the existing vision manager

**Status**: ✅ Done

**File**: `src/vision/model_manager.rs` (modify, max 40 lines added)

**Max Lines Changed**: 40

**Dependencies**: Phase 1 must be complete

**Tasks**:
- [x] Write test `test_config_with_vlm` — config with VLM endpoint creates VlmClient
- [x] Write test `test_config_without_vlm` — config without VLM endpoint has no VlmClient
- [x] Write test `test_has_vlm` — `has_vlm()` returns correct state
- [x] Write test `test_list_models_includes_vlm` — `list_models()` includes VLM when available
- [x] Add `vlm_endpoint: Option<String>` + `vlm_model_name: Option<String>` to `VisionModelConfig`
- [x] Add `vlm_client: Option<Arc<VlmClient>>` to `VisionModelManager`
- [x] Add `get_vlm_client()`, `has_vlm()` methods
- [x] Update `VisionModelManager::new()` to create VlmClient when endpoint is set
- [x] Update `list_models()` to include VLM info

**Key Code Changes**:
```rust
// In VisionModelConfig
pub vlm_endpoint: Option<String>,
pub vlm_model_name: Option<String>,

// In VisionModelManager
vlm_client: Option<Arc<VlmClient>>,

// In VisionModelManager::new()
let vlm_client = if let Some(ref endpoint) = config.vlm_endpoint {
    let model_name = config.vlm_model_name.as_deref().unwrap_or("qwen3-vl");
    match VlmClient::new(endpoint, model_name) {
        Ok(client) => {
            tracing::info!("✅ VLM client configured: {}", endpoint);
            Some(Arc::new(client))
        }
        Err(e) => {
            tracing::warn!("⚠️ Failed to create VLM client: {}", e);
            None
        }
    }
} else {
    None
};
```

**Verification**:
```bash
timeout 60 cargo test --lib -- vision::model_manager::tests -- --test-threads=1
```

---

## Phase 3: API Handler Updates

### Sub-phase 3.1: Add model param to Response Constructors

**Goal**: Allow response structs to report which model was used (VLM name vs "paddleocr"/"florence-2")

**Status**: ✅ Done

**Files**:
- `src/api/ocr/response.rs` (modify `OcrResponse::new()`, max 10 lines)
- `src/api/describe_image/response.rs` (modify `DescribeImageResponse::new()`, max 10 lines)

**Max Lines Changed**: 20

**Dependencies**: None

**Tasks**:
- [x] Add `model: &str` param to `OcrResponse::new()` (line 54), replace hardcoded `"paddleocr"`
- [x] Add `model: &str` param to `DescribeImageResponse::new()` (line 63), replace hardcoded `"florence-2"`
- [x] Update existing test assertions and callers
- [x] Write test `test_ocr_response_custom_model` — model field matches param
- [x] Write test `test_describe_response_custom_model` — model field matches param

**OcrResponse::new() change** (line 54-78):
```rust
// OLD: pub fn new(text, confidence, regions, processing_time_ms, chain_id) -> Self
// NEW:
pub fn new(
    text: String,
    confidence: f32,
    regions: Vec<TextRegion>,
    processing_time_ms: u64,
    chain_id: u64,
    model: &str,  // NEW param
) -> Self {
    // ...
    Self {
        // ...
        model: model.to_string(),  // was: "paddleocr".to_string()
        // ...
    }
}
```

**Verification**:
```bash
timeout 60 cargo test --lib -- api::ocr::response -- --test-threads=1
timeout 60 cargo test --lib -- api::describe_image::response -- --test-threads=1
```

---

### Sub-phase 3.2: Update OCR Handler (VLM Priority + ONNX Fallback)

**Goal**: OCR handler tries VLM first, falls back to ONNX if unavailable or fails

**Status**: ✅ Done

**File**: `src/api/ocr/handler.rs` (modify, max 40 lines added)

**Max Lines Changed**: 40

**Dependencies**: Sub-phases 2.1 and 3.1 must be complete

**Tasks**:
- [x] Write test `test_ocr_handler_vlm_model_field` — response has VLM model name when VLM used
- [x] Write test `test_ocr_handler_onnx_model_field` — response has "paddleocr" when ONNX used
- [x] Write test `test_ocr_handler_fallback_on_vlm_error` — falls back to ONNX on VLM failure
- [x] Insert VLM check between steps 2 and 3 (after getting manager, before getting OCR model)
- [x] Add VLM call with error handling → return early on success
- [x] On VLM failure → log warning, continue to existing ONNX path
- [x] Pass model name to `OcrResponse::new()`

**New handler flow** (inserted between existing lines 57 and 59):
```rust
    // 2b. Try VLM first (if available)
    if let Some(vlm_client) = manager.get_vlm_client() {
        let image_data = request.image.as_ref().ok_or_else(|| {
            (StatusCode::BAD_REQUEST, "image is required".to_string())
        })?;

        match vlm_client.ocr(image_data, &request.format).await {
            Ok(vlm_result) => {
                info!("VLM OCR complete: {} chars, {}ms (model: {})",
                    vlm_result.text.len(), vlm_result.processing_time_ms, vlm_result.model);

                let response = OcrResponse::new(
                    vlm_result.text, 1.0, vec![], vlm_result.processing_time_ms,
                    request.chain_id, &vlm_result.model,
                );
                return Ok(Json(response));
            }
            Err(e) => {
                warn!("VLM OCR failed, falling back to ONNX: {}", e);
                // Fall through to existing ONNX pipeline
            }
        }
    }

    // 3. Get OCR model (existing ONNX fallback)
    // ... existing code continues ...
```

**Verification**:
```bash
timeout 60 cargo test --lib -- api::ocr -- --test-threads=1
```

---

### Sub-phase 3.3: Update Describe-Image Handler (VLM Priority + ONNX Fallback)

**Goal**: Describe handler tries VLM first, falls back to Florence-2 if unavailable or fails

**Status**: ✅ Done

**File**: `src/api/describe_image/handler.rs` (modify, max 40 lines added)

**Max Lines Changed**: 40

**Dependencies**: Sub-phases 2.1 and 3.1 must be complete

**Tasks**:
- [x] Write test `test_describe_handler_vlm_model_field` — response has VLM model name
- [x] Write test `test_describe_handler_onnx_model_field` — response has "florence-2"
- [x] Write test `test_describe_handler_fallback_on_vlm_error` — falls back to Florence-2
- [x] Insert VLM check between steps 2 and 3
- [x] Add VLM call with error handling → return early on success
- [x] On VLM failure → log warning, continue to existing Florence-2 path
- [x] Pass model name to `DescribeImageResponse::new()`

**New handler flow** (inserted between existing lines 62 and 64):
```rust
    // 2b. Try VLM first (if available)
    if let Some(vlm_client) = manager.get_vlm_client() {
        let image_data = request.image.as_ref().ok_or_else(|| {
            (StatusCode::BAD_REQUEST, "image is required".to_string())
        })?;

        match vlm_client.describe(
            image_data, &request.format, &request.detail, request.prompt.as_deref()
        ).await {
            Ok(vlm_result) => {
                info!("VLM describe complete: {} chars, {}ms (model: {})",
                    vlm_result.description.len(), vlm_result.processing_time_ms, vlm_result.model);

                let analysis = ImageAnalysis {
                    width: 0, height: 0,  // VLM doesn't return dimensions
                    dominant_colors: vec![], scene_type: None,
                };
                let response = DescribeImageResponse::new(
                    vlm_result.description, vec![], analysis,
                    vlm_result.processing_time_ms, request.chain_id, &vlm_result.model,
                );
                return Ok(Json(response));
            }
            Err(e) => {
                warn!("VLM describe failed, falling back to Florence-2: {}", e);
            }
        }
    }

    // 3. Get Florence model (existing ONNX fallback)
    // ... existing code continues ...
```

**Verification**:
```bash
timeout 60 cargo test --lib -- api::describe_image -- --test-threads=1
```

---

## Phase 4: Environment & Configuration

### Sub-phase 4.1: Add VLM Env Vars to main.rs + docker-compose

**Goal**: Wire VLM configuration from environment to VisionModelConfig

**Status**: ✅ Done

**Files**:
- `src/main.rs` (add ~10 lines near vision model config)
- `docker-compose.prod.yml` (add ~30 lines for VLM sidecar + env vars)

**Max Lines Changed**: 30 (main.rs) + additional docker-compose

**Dependencies**: Phase 2 must be complete

**Tasks**:
- [x] Add `VLM_ENDPOINT` and `VLM_MODEL_NAME` env var reading to `src/main.rs`
- [x] Pass to `VisionModelConfig { vlm_endpoint, vlm_model_name, ... }`
- [x] Write test `test_vlm_env_vars_optional` — node starts without VLM env vars
- [x] Write test `test_vlm_config_from_env` — env vars flow to config
- [x] Add VLM sidecar service to `docker-compose.prod.yml` (optional, commented-out or profile-gated)
- [x] Add `VLM_ENDPOINT` and `VLM_MODEL_NAME` to llm-node environment

**main.rs addition** (near existing vision model config, ~10 lines):
```rust
let vlm_endpoint = std::env::var("VLM_ENDPOINT").ok();
let vlm_model_name = std::env::var("VLM_MODEL_NAME").ok();

if let Some(ref endpoint) = vlm_endpoint {
    info!("VLM endpoint configured: {}", endpoint);
} else {
    info!("No VLM_ENDPOINT set, using ONNX vision models only");
}

let vision_config = VisionModelConfig {
    ocr_model_dir: Some("./models/paddleocr-english-onnx".to_string()),
    florence_model_dir: Some("./models/florence-2-onnx".to_string()),
    vlm_endpoint,
    vlm_model_name,
};
```

**docker-compose.prod.yml VLM service** (~25 lines):
```yaml
  # VLM Vision Service (optional - for high-VRAM hosts only)
  # Uncomment to enable VLM-based OCR and image description
  # qwen3-vl:
  #   image: ghcr.io/ggerganov/llama.cpp:server-cuda
  #   container_name: qwen3-vl
  #   restart: unless-stopped
  #   runtime: nvidia
  #   environment:
  #     NVIDIA_VISIBLE_DEVICES: all
  #   volumes:
  #     - ${VLM_MODEL_PATH:-./models/qwen3-vl}:/models:ro
  #   command: >
  #     --model /models/${VLM_MODEL_FILE:-Qwen3-VL-8B-Q4_K_M.gguf}
  #     --host 0.0.0.0 --port 8081
  #     --ctx-size 4096 --n-gpu-layers 99
  #   healthcheck:
  #     test: ["CMD", "curl", "-f", "http://localhost:8081/health"]
  #     interval: 30s
  #     timeout: 10s
  #     start_period: 120s
  #     retries: 3
  #   networks:
  #     - fabstir-network
```

**Verification**:
```bash
timeout 60 cargo test --lib -- --test-threads=1
# Verify node starts without VLM env vars:
timeout 10 cargo run -- --help 2>&1 | head -5
```

---

### Sub-phase 4.2: Create download_vlm_model.sh

**Goal**: Script to download Qwen3-VL GGUF model for sidecar

**Status**: ✅ Done

**File**: `scripts/download_vlm_model.sh` (**NEW**, max 80 lines)

**Max Lines Changed**: 80

**Dependencies**: None (independent)

**Tasks**:
- [x] Create script following pattern of `scripts/download_florence_model.sh`
- [x] Download Qwen3-VL-8B Q4_K_M GGUF from HuggingFace
- [x] Target directory: `models/qwen3-vl/`
- [x] Include file size verification
- [x] Create VERSION file

**Verification**:
```bash
bash -n scripts/download_vlm_model.sh  # Syntax check only
```

---

## Phase 5: Version & Lint

### Sub-phase 5.1: Bump Version to 8.15.3

**Goal**: Update version files for VLM vision release

**Status**: ✅ Done

**Files**:
- `/workspace/VERSION`
- `/workspace/src/version.rs`

**Max Lines Changed**: 20

**Tasks**:
- [x] Update `VERSION` to `8.15.3-vlm-vision`
- [x] Update `src/version.rs` VERSION to `"v8.15.3-vlm-vision-2026-02-08"`
- [x] Update VERSION_NUMBER to `"8.15.3"`
- [x] Update VERSION_PATCH to `3`
- [x] Update test assertions in `#[cfg(test)]` module
- [x] Run `cargo test --lib -- version` — all pass

**Verification**:
```bash
timeout 60 cargo test --lib -- version -- --test-threads=1
```

---

### Sub-phase 5.2: Run Full Test Suite + Lint

**Goal**: Verify everything works together

**Status**: ✅ Done

**Tasks**:
- [x] Run `cargo fmt -- --check` — formatting clean
- [x] Run `cargo check` — compiles cleanly
- [x] Run `timeout 120 cargo test --lib -- --test-threads=1` — all pass
- [x] Document total passing test count

**Verification**:
```bash
cargo fmt -- --check
timeout 120 cargo test --lib -- --test-threads=1
```

---

## Dependency Graph

```
Phase 1.1 (VlmClient struct) ──> Phase 1.2 (ocr) ──> Phase 1.3 (describe)
                                                              │
Phase 3.1 (response model param) ──────────────────┐         │
                                                    ├──> Phase 3.2 (OCR handler)
Phase 2.1 (manager integration) ◄── Phase 1 ───────┤    Phase 3.3 (describe handler)
                                                    │         │
                                                    └─────────┘
Phase 4.1 (env vars) ◄── Phase 2.1
Phase 4.2 (download script) — independent
Phase 5 (version/lint) ◄── all above
```

**Parallelizable**: Phase 3.1 and Phase 4.2 can be done independently.

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| llama-server image URL format differs | VLM calls fail silently | Test with actual llama-server; verify `image_url` content part support |
| ONNX fallback masks VLM config errors | Host thinks VLM is working | Log clear warning when VLM fails, include model name in response |
| reqwest timeout too short for large images | VLM OCR times out | 30s default; configurable via `VLM_TIMEOUT` env var if needed |
| Qwen3-VL GGUF not yet available | No model to download | Check HuggingFace; fall back to Qwen2.5-VL GGUF |
| `model` param change breaks existing callers | Compilation errors | Mechanical: add `"paddleocr"` / `"florence-2"` at all call sites |

---

## Success Criteria

**Complete** when:

1. **VLM**: `VLM_ENDPOINT=http://localhost:8081` routes OCR/describe calls to VLM sidecar
2. **Fallback**: Without `VLM_ENDPOINT`, node uses existing ONNX models on CPU
3. **Resilience**: VLM failure → automatic ONNX fallback with warning log
4. **Response**: `model` field shows actual model used (e.g., "qwen3-vl" or "paddleocr")
5. **Tests**: All new tests pass (27+ expected)
6. **Backward compat**: All existing tests still pass
7. **Lint**: `cargo fmt` and `cargo clippy` clean

---

## Phase 6: WebSocket Vision Pre-Processing

**Problem**: VLM sidecar (Qwen3-VL) is running and healthy, but images sent via WebSocket chat sessions are silently ignored. The main text LLM hallucinates because it never sees the image data. Vision only works through HTTP endpoints (`/v1/ocr`, `/v1/describe-image`) — the WebSocket flow has zero image routing.

**Solution**: Pre-process images in the WebSocket `encrypted_message` handler by calling the VLM sidecar, then prepend the vision analysis as text context (`[Image Analysis]...[/Image Analysis]`) to the prompt before sending it to the main LLM.

### Architecture

```
WebSocket encrypted_message / plaintext inference
            │
    ┌───────▼────────────┐
    │  Decrypt payload /  │
    │  Extract prompt     │
    └───────┬────────────┘
            │
    ┌───────▼────────────┐
    │  Has "images"?     │
    └───┬──────────┬─────┘
       YES         NO
        │          │
  ┌─────▼─────┐   │
  │ VlmClient │   │
  │ .describe()│   │
  │ → sidecar  │   │
  └─────┬─────┘   │
  ┌─────▼─────┐   │
  │ Augment   │   │
  │ prompt w/ │   │
  │ [Image    │   │
  │  Analysis]│   │
  └─────┬─────┘   │
        └────┬────┘
    ┌────────▼────────┐
    │ InferenceRequest │
    │ (text prompt)    │
    │ → Main LLM      │
    └─────────────────┘
```

**SDK payload format** (in encrypted_message):
```json
{
  "prompt": "Describe what you see in the attached image",
  "images": [{ "data": "<base64>", "format": "png" }]
}
```

---

### Sub-phase 6.1: Tests for Vision Image Extraction + Prompt Augmentation (RED)

**Goal**: Write failing tests for extracting images from decrypted JSON and augmenting prompts

**Status**: ✅ Done

**File**: `tests/vision_websocket_tests.rs` (**NEW**, max 80 lines)

**Max Lines Changed**: 80

**Dependencies**: Phase 1 VlmClient must exist (already done)

**Tasks**:
- [x] Write test `test_extract_images_from_decrypted_json` — parse `images` array from serde_json::Value
- [x] Write test `test_extract_images_missing_field` — returns empty vec when no `images` field
- [x] Write test `test_extract_images_empty_array` — returns empty vec when images array is empty
- [x] Write test `test_augment_prompt_with_vision` — verify `[Image Analysis]...[/Image Analysis]` format
- [x] Write test `test_augment_prompt_no_descriptions` — returns original prompt when descriptions is empty
- [x] Write test `test_augment_prompt_multiple_images` — descriptions joined with newlines

**Verification**:
```bash
timeout 60 cargo test --test vision_websocket_tests -- --test-threads=1
```

---

### Sub-phase 6.2: `process_vision_images()` Helper Function (GREEN)

**Goal**: Implement the helper that calls VLM sidecar for each image and returns an augmented prompt

**Status**: ✅ Done

**File**: `src/api/server.rs` (add helper function, max 50 lines)

**Max Lines Changed**: 50

**Dependencies**: Sub-phase 6.1 tests must exist

**Tasks**:
- [x] Implement `pub fn augment_prompt_with_vision(descriptions: &[String], user_prompt: &str) -> String` pure function
- [x] Implement `async fn process_vision_images(server: &ApiServer, images: &[serde_json::Value], user_prompt: &str) -> String`
- [x] Verify Sub-phase 6.1 tests pass

**Verification**:
```bash
timeout 60 cargo test --test vision_websocket_tests -- --test-threads=1
cargo check
```

---

### Sub-phase 6.3: Integrate into Encrypted Message Handler

**Goal**: Add image pre-processing in the `encrypted_message` path of `handle_websocket()`

**Status**: ✅ Done

**File**: `src/api/server.rs` (~line 1884, max 20 lines added)

**Max Lines Changed**: 20

**Dependencies**: Sub-phase 6.2

**Tasks**:
- [x] After `plaintext_prompt` extraction, add image detection + VLM call
- [x] Verify compilation: `cargo check`

---

### Sub-phase 6.4: Integrate into Plaintext Inference Handler

**Goal**: Add image pre-processing in the plaintext inference path of `handle_websocket()`

**Status**: ✅ Done

**File**: `src/api/server.rs` (~line 2380, max 20 lines added)

**Max Lines Changed**: 20

**Dependencies**: Sub-phase 6.2

**Tasks**:
- [x] In plaintext inference path, add same image detection + VLM call
- [x] Verify compilation: `cargo check`

---

### Sub-phase 6.5: Full Test Suite + Rebuild Tarball

**Goal**: Verify all tests pass, rebuild tarball once

**Status**: ✅ Done

**Tasks**:
- [x] Run `cargo fmt`
- [x] Run `timeout 120 cargo test --lib -- --test-threads=2` — all pass
- [x] Rebuild tarball

---

## Phase 7: VLM Token Billing

**Problem**: The VLM sidecar processes images via OCR (up to 4096 tokens) and describe (up to 100 tokens) on GPU, but these tokens are never counted for billing. Only main LLM output tokens are tracked. The host bears GPU compute cost for vision processing but isn't compensated. The VLM sidecar (llama-server) already returns token usage in its OpenAI-compatible response (`usage.total_tokens`), but the `ChatResponse` struct ignores it.

**Solution**: Capture VLM token usage from the OpenAI-compatible response and add it to the job's token tracker so hosts are compensated for vision processing.

### Token Flow After Fix

```
Images → VLM OCR (N tokens) + VLM Describe (M tokens)
       → track_tokens(job_id, N+M)              ← NEW
       → Augmented prompt → Main LLM (K tokens)
       → track_tokens(job_id, K)                 ← existing
       → Total billed: N + M + K
```

---

### Sub-phase 7.1: Tests for VLM Token Capture + Billing (RED)

**Goal**: Write tests that validate token usage is captured from VLM responses and flows to billing

**Status**: ✅ Done

**File**: `tests/vision_websocket_tests.rs` (append, max 40 lines) + `src/vision/vlm_client.rs` (unit tests)

**Max Lines Changed**: 40

**Dependencies**: Phase 6 complete

**Tasks**:
- [x] Write test `test_vlm_ocr_result_has_tokens_used` — `VlmOcrResult` struct has `tokens_used: u32` field
- [x] Write test `test_vlm_describe_result_has_tokens_used` — `VlmDescribeResult` struct has `tokens_used: u32` field
- [x] Write test `test_chat_usage_deserialization` — `ChatUsage` struct deserializes from OpenAI JSON
- [x] Write test `test_chat_response_with_usage` — `ChatResponse` captures optional `usage` field

---

### Sub-phase 7.2: Capture Usage from VLM Response (GREEN)

**Goal**: Update `VlmClient` to deserialize token usage from VLM sidecar response

**Status**: ✅ Done

**File**: `src/vision/vlm_client.rs` (modify, max 25 lines)

**Max Lines Changed**: 25

**Dependencies**: Sub-phase 7.1 tests must exist

**Tasks**:
- [x] Add `ChatUsage` struct: `{ prompt_tokens: u32, completion_tokens: u32, total_tokens: u32 }`
- [x] Add `usage: Option<ChatUsage>` field to `ChatResponse`
- [x] Add `pub tokens_used: u32` field to `VlmOcrResult`
- [x] Add `pub tokens_used: u32` field to `VlmDescribeResult`
- [x] In `ocr()`: extract `total_tokens` from response usage, default 0
- [x] In `describe()`: extract `total_tokens` from response usage, default 0
- [x] Verify Sub-phase 7.1 tests pass (GREEN)

---

### Sub-phase 7.3: Return + Track VLM Tokens in server.rs

**Goal**: Change `process_vision_images()` to return VLM token count and track it for billing

**Status**: ✅ Done

**File**: `src/api/server.rs` (modify, max 30 lines changed)

**Max Lines Changed**: 30

**Dependencies**: Sub-phase 7.2 must be complete

**Tasks**:
- [x] Change `process_vision_images()` return type from `String` to `(String, u64)`
- [x] Accumulate `tokens_used` from each `ocr()` + `describe()` call
- [x] Return `(augmented_prompt, total_vlm_tokens)`
- [x] At encrypted call site (~line 2013): destructure tuple, track VLM tokens via `checkpoint_manager.track_tokens()` or `token_tracker.track_tokens()`
- [x] At plaintext call site (~line 2535): same destructure + track
- [x] Add log: `"VLM vision processing used {} tokens for job {}", vlm_tokens, job_id`
- [x] Verify compilation: `cargo check`

---

### Sub-phase 7.4: Full Test Suite + Rebuild Tarball

**Goal**: Verify all tests pass, rebuild tarball once

**Status**: ✅ Done

**Tasks**:
- [x] Run `cargo fmt`
- [x] Run `timeout 120 cargo test --lib -- --test-threads=2` — 822 pass (7 pre-existing failures)
- [x] Run `timeout 60 cargo test --test vision_websocket_tests -- --test-threads=1` — all 12 pass
- [x] Rebuild tarball

**Verification**:
```bash
cargo fmt
timeout 120 cargo test --lib -- --test-threads=2
cargo build --release --features real-ezkl -j 4
cp target/release/fabstir-llm-node ./fabstir-llm-node
tar -czvf fabstir-llm-node-v8.15.4.tar.gz fabstir-llm-node docker-compose.prod.yml docker/ scripts/*.sh
```

---

## Related Documentation

- [Qwen3-VL Models](https://huggingface.co/collections/Qwen/qwen3-vl-6849976983f77a6ca16e8f13)
- [llama.cpp Server](https://github.com/ggerganov/llama.cpp/tree/master/examples/server)
- [OpenAI Vision API](https://platform.openai.com/docs/guides/vision)
- `docs/IMPLEMENTATION-MODEL-AGNOSTIC-INFERENCE.md` — Format reference
- `docs/IMPLEMENTATION-KV-CACHE-QUANTIZATION.md` — Previous implementation
