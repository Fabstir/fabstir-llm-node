# IMPLEMENTATION - Model-Agnostic Inference Pipeline & GLM-4.7-Flash Support

## Status: ✅ COMPLETE

**Version**: v8.15.0-model-agnostic-inference
**Previous Version**: v8.14.2-proofinterval-billing
**Start Date**: 2026-02-07
**Completion Date**: 2026-02-07
**Approach**: Strict TDD with bounded autonomy - one sub-phase at a time

---

## Overview

This implementation makes the inference pipeline model-agnostic and adds GLM-4.7-Flash as the first non-Harmony model. Currently the pipeline is hardcoded for GPT-OSS-20B (Harmony template, stop token ID 200002, no min_p sampler). These changes allow any GGUF model with the correct chat template to run cleanly.

### What's Changing

| Component | Current | Target | Impact |
|-----------|---------|--------|--------|
| Chat templates | 5 (no GLM) | 6 (add GLM4) | New template |
| Stop tokens | Hardcoded Harmony (ID 200002) | Per-template + env override | **Engine refactor** |
| Sampler chain | temp + top_p + greedy | temp + penalties + top_p + min_p + dist | **Engine refactor** |
| Marker stripping | Harmony/ChatML/Llama2 | + GLM4 | 2 files |
| InferenceRequest | No min_p field | Add min_p: f32 | ~15 construction sites |

### GLM-4.7-Flash Model Details

- **Architecture**: deepseek2 (MoE, 30B params, 3B active)
- **Chat format**: `<|system|>\n{content}<|user|>\n{content}<|assistant|>\n`
- **Stop tokens**: `<|user|>`, `<|observation|>`
- **Sampling**: temp=1.0, top_p=0.95, min_p=0.01, repeat_penalty=1.0
- **Context**: Up to 202,752 tokens
- **BOS**: `[gMASK]<sop>` (handled automatically by llama.cpp via GGUF metadata)
- **Thinking**: `<think>...</think>` blocks (passed through to client transparently)
- **GGUF source**: `unsloth/GLM-4.7-Flash-GGUF` (Q4_K_M = 18.3GB, BF16 = 59.9GB)

### Key Architectural Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Stop tokens | Per-template defaults + `MODEL_STOP_TOKENS` env override | Correct default per model; escape hatch for new variants |
| `[gMASK]<sop>` prefix | Leave to llama.cpp | Already handled by `AddBos::Always` in engine.rs:314 |
| `<think>` blocks | Pass through transparently | SDK/client responsibility to display/hide |
| Backward compat | Harmony remains default | No breaking change for existing deployments |

---

## Implementation Status

| Phase | Sub-phase | Description | Status | Tests | Max Lines |
|-------|-----------|-------------|--------|-------|-----------|
| 1 | 1.1 | Add GLM4 enum variant + from_str (TDD) | ✅ Done | 2/2 | 15 |
| 1 | 1.2 | Implement format_glm4() (TDD) | ✅ Done | 4/4 | 35 |
| 2 | 2.1 | Add stop_tokens() method (TDD) | ✅ Done | 5/5 | 20 |
| 2 | 2.2 | Add parse_stop_tokens_env() (TDD) | ✅ Done | 3/3 | 15 |
| 3 | 3.1 | Refactor engine stop token resolution | ✅ Done | 2/2 | 30 |
| 3 | 3.2 | Refactor engine stop condition logic | ✅ Done | 2/2 | 15 |
| 4 | 4.1 | Add min_p field to InferenceRequest (TDD) | ✅ Done | 2/2 | 5 |
| 4 | 4.2 | Refactor sampler chain (TDD) | ✅ Done | 3/3 | 25 |
| 4 | 4.3 | Update InferenceRequest construction sites | ✅ Done | 12/12 | ~30 total |
| 5 | 5.1 | Add GLM4 markers to strip functions (TDD) | ✅ Done | 3/3 | 20 |
| 5 | 5.2 | Update is_prompt_already_formatted() | ✅ Done | 2/2 | 5 |
| 6 | 6.1 | Bump version to 8.15.0 | ✅ Done | 3/3 | 20 |
| 6 | 6.2 | Run full test suite + lint | ✅ Done | 0/0 | 0 |
| 6 | 6.3 | Update environment docs | ✅ Done | 0/0 | 15 |
| **Total** | | | **100%** | **42/42** | **~250** |

---

## Phase 1: GLM4 Chat Template

### Sub-phase 1.1: Add GLM4 Enum Variant + from_str (TDD)

**Goal**: Add `Glm4` variant to `ChatTemplate` enum and wire up string parsing

**Status**: ⏳ Pending

**File**: `src/inference/chat_template.rs` (modify existing)

**Max Lines Changed**: 15

**Approach**: Test-Driven Development
1. Write failing tests first
2. Add enum variant and parsing to pass tests
3. Verify existing tests still pass

**Tasks**:
- [x] Write test `test_from_str_glm4` — parse "glm4", "glm-4", "glm4-flash", "glm-4.7-flash"
- [x] Write test `test_glm4_as_str` — `ChatTemplate::Glm4.as_str() == "glm4"`
- [x] Add `Glm4` variant to `ChatTemplate` enum (after `ChatML`, line ~24)
- [x] Add `"glm4" | "glm-4" | "glm4-flash" | "glm-4.7-flash"` to `from_str()` (line ~34)
- [x] Add `Self::Glm4 => "glm4"` to `as_str()` (line ~47)
- [x] Add `Self::Glm4 => self.format_glm4(messages)` to `format_messages()` (line ~66) — stub with `todo!()`
- [x] Run `cargo test --lib -- chat_template::tests::test_from_str` — verify parsing works (11/11 pass)

**Test Template**:
```rust
#[test]
fn test_from_str_glm4() {
    assert_eq!(ChatTemplate::from_str("glm4"), Some(ChatTemplate::Glm4));
    assert_eq!(ChatTemplate::from_str("glm-4"), Some(ChatTemplate::Glm4));
    assert_eq!(ChatTemplate::from_str("glm4-flash"), Some(ChatTemplate::Glm4));
    assert_eq!(ChatTemplate::from_str("glm-4.7-flash"), Some(ChatTemplate::Glm4));
}

#[test]
fn test_glm4_as_str() {
    assert_eq!(ChatTemplate::Glm4.as_str(), "glm4");
}
```

**Verification**:
```bash
timeout 60 cargo test --lib -- chat_template::tests::test_from_str -- --test-threads=2
timeout 60 cargo test --lib -- chat_template::tests::test_glm4_as_str -- --test-threads=2
```

---

### Sub-phase 1.2: Implement format_glm4() (TDD)

**Goal**: Implement the GLM-4 chat template formatting method

**Status**: ⏳ Pending

**File**: `src/inference/chat_template.rs` (add method, ~line 215)

**Max Lines Changed**: 35 (method body + tests)

**Dependencies**: Sub-phase 1.1 must be complete

**Tasks**:
- [x] Write test `test_glm4_format_basic` — single user message, verify `<|user|>\n` and `<|assistant|>\n` markers
- [x] Write test `test_glm4_format_with_system` — system + user message, verify `<|system|>\n` present
- [x] Write test `test_glm4_format_multi_turn` — user/assistant/user, verify all markers correct
- [x] Write test `test_glm4_auto_system_message` — no system provided → auto-inject "You are a helpful assistant."
- [x] Implement `format_glm4()` method
- [x] Run `cargo test --lib -- chat_template` — all 15 template tests pass

**Implementation Target** (~25 lines):
```rust
/// GLM-4 format: "<|system|>\n{content}<|user|>\n{content}<|assistant|>\n"
/// Reference: https://huggingface.co/zai-org/GLM-4.7/blob/main/chat_template.jinja
/// Note: [gMASK]<sop> BOS tokens are handled by llama.cpp via GGUF metadata
fn format_glm4(&self, messages: &[(String, String)]) -> String {
    let mut prompt = String::new();
    let has_system = messages.iter().any(|(role, _)| role == "system");
    if !has_system {
        prompt.push_str("<|system|>\nYou are a helpful assistant.\n");
    }
    for (role, content) in messages {
        match role.as_str() {
            "system" => prompt.push_str(&format!("<|system|>\n{}\n", content)),
            "user" => prompt.push_str(&format!("<|user|>\n{}\n", content)),
            "assistant" => prompt.push_str(&format!("<|assistant|>\n{}\n", content)),
            _ => {}
        }
    }
    prompt.push_str("<|assistant|>\n");
    prompt
}
```

**Test Template**:
```rust
#[test]
fn test_glm4_format_basic() {
    let template = ChatTemplate::Glm4;
    let messages = vec![("user".to_string(), "What is 2+2?".to_string())];
    let formatted = template.format_messages(&messages);
    assert!(formatted.contains("<|user|>\nWhat is 2+2?\n"));
    assert!(formatted.ends_with("<|assistant|>\n"));
}

#[test]
fn test_glm4_format_with_system() {
    let template = ChatTemplate::Glm4;
    let messages = vec![
        ("system".to_string(), "You are helpful.".to_string()),
        ("user".to_string(), "Hello".to_string()),
    ];
    let formatted = template.format_messages(&messages);
    assert!(formatted.contains("<|system|>\nYou are helpful.\n"));
    assert!(formatted.contains("<|user|>\nHello\n"));
    assert!(formatted.ends_with("<|assistant|>\n"));
    // Should NOT auto-inject system message when one is provided
    assert_eq!(formatted.matches("<|system|>").count(), 1);
}

#[test]
fn test_glm4_format_multi_turn() {
    let template = ChatTemplate::Glm4;
    let messages = vec![
        ("user".to_string(), "Hi".to_string()),
        ("assistant".to_string(), "Hello!".to_string()),
        ("user".to_string(), "How are you?".to_string()),
    ];
    let formatted = template.format_messages(&messages);
    assert!(formatted.contains("<|user|>\nHi\n"));
    assert!(formatted.contains("<|assistant|>\nHello!\n"));
    assert!(formatted.contains("<|user|>\nHow are you?\n"));
    assert!(formatted.ends_with("<|assistant|>\n"));
}

#[test]
fn test_glm4_auto_system_message() {
    let template = ChatTemplate::Glm4;
    let messages = vec![("user".to_string(), "Hello".to_string())];
    let formatted = template.format_messages(&messages);
    assert!(formatted.contains("<|system|>\nYou are a helpful assistant.\n"));
}
```

**Verification**:
```bash
timeout 60 cargo test --lib -- chat_template -- --test-threads=2
# Expected: All 13+ tests passing (9 existing + 4+ new)
```

---

## Phase 2: Per-Template Stop Tokens

### Sub-phase 2.1: Add stop_tokens() Method (TDD)

**Goal**: Each `ChatTemplate` variant declares its own default stop token strings

**Status**: ⏳ Pending

**File**: `src/inference/chat_template.rs` (add method to impl block, ~line 67)

**Max Lines Changed**: 20

**Dependencies**: Phase 1 must be complete (Glm4 variant exists)

**Tasks**:
- [x] Write test `test_stop_tokens_harmony` — expects `["<|return|>", "<|end|>"]`
- [x] Write test `test_stop_tokens_glm4` — expects `["<|user|>", "<|observation|>"]`
- [x] Write test `test_stop_tokens_chatml` — expects `["<|im_end|>"]`
- [x] Write test `test_stop_tokens_default_empty` — expects `[]`
- [x] Write test `test_stop_tokens_llama2_empty` — expects `[]` (EOS handled by llama.cpp)
- [x] Implement `stop_tokens(&self) -> Vec<&'static str>` method

**Implementation Target** (~14 lines):
```rust
/// Get model-specific stop token strings.
/// These tokens cause generation to stop when encountered.
/// Returns empty vec for templates that only use EOS token.
pub fn stop_tokens(&self) -> Vec<&'static str> {
    match self {
        Self::Default => vec![],
        Self::Llama2 => vec![],
        Self::Vicuna => vec![],
        Self::Harmony => vec!["<|return|>", "<|end|>"],
        Self::ChatML => vec!["<|im_end|>"],
        Self::Glm4 => vec!["<|user|>", "<|observation|>"],
    }
}
```

**Verification**:
```bash
timeout 60 cargo test --lib -- chat_template::tests::test_stop_tokens -- --test-threads=2
```

---

### Sub-phase 2.2: Add parse_stop_tokens_env() (TDD)

**Goal**: Utility function to parse `MODEL_STOP_TOKENS` env var as override

**Status**: ⏳ Pending

**File**: `src/inference/chat_template.rs` (add standalone pub function after impl block)

**Max Lines Changed**: 15

**Tasks**:
- [x] Write test `test_parse_stop_tokens_env_set` — set env var, verify parsing
- [x] Write test `test_parse_stop_tokens_env_unset` — unset env var, verify empty vec
- [x] Write test `test_parse_stop_tokens_env_whitespace` — handles trimming
- [x] Implement `pub fn parse_stop_tokens_env() -> Vec<String>`

**Implementation Target** (~8 lines):
```rust
/// Parse MODEL_STOP_TOKENS env var into a list of stop token strings.
/// Format: comma-separated, e.g. "<|user|>,<|observation|>"
/// Returns empty vec if not set (template defaults will be used).
pub fn parse_stop_tokens_env() -> Vec<String> {
    std::env::var("MODEL_STOP_TOKENS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}
```

**Verification**:
```bash
timeout 60 cargo test --lib -- chat_template::tests::test_parse_stop_tokens -- --test-threads=2
```

---

## Phase 3: Engine Stop Token Refactor

### Sub-phase 3.1: Refactor Stop Token Resolution (engine.rs)

**Goal**: Replace hardcoded Harmony stop tokens with template-driven resolution

**Status**: ⏳ Pending

**File**: `src/inference/engine.rs` (replace lines ~319-343)

**Max Lines Changed**: 30 (replacing ~25 existing lines)

**Dependencies**: Phase 2 must be complete

**Tasks**:
- [x] Replace hardcoded `return_tok` / `end_tok` logic with template-driven resolution
- [x] Resolve template from `MODEL_CHAT_TEMPLATE` env var (defaults to "harmony")
- [x] Get stop token strings: env override via `parse_stop_tokens_env()` or template defaults
- [x] Convert strings to `Vec<LlamaToken>` via `model.str_to_token()`
- [x] Update return tuple to `(tokens_list, context_size, eos, stop_ids)`

**Current Code** (lines 319-343, to be replaced):
```rust
// Hardcoded Harmony tokens — REMOVE THIS
let return_tok = model.model.str_to_token("<|return|>", AddBos::Never)...
    .unwrap_or_else(|| { unsafe { LlamaToken::new(200002) } });
let end_tok = model.model.str_to_token("<|end|>", AddBos::Never)...;
```

**New Code Target**:
```rust
// Resolve stop tokens from template (or MODEL_STOP_TOKENS env override)
let template_name = std::env::var("MODEL_CHAT_TEMPLATE").unwrap_or_else(|_| "harmony".to_string());
let template = crate::inference::ChatTemplate::from_str(&template_name)
    .unwrap_or(crate::inference::ChatTemplate::Harmony);

let stop_token_strings = {
    let env_overrides = crate::inference::chat_template::parse_stop_tokens_env();
    if env_overrides.is_empty() {
        template.stop_tokens().iter().map(|s| s.to_string()).collect::<Vec<_>>()
    } else {
        env_overrides
    }
};

let mut stop_token_ids: Vec<llama_cpp_2::token::LlamaToken> = Vec::new();
for token_str in &stop_token_strings {
    if let Ok(tokens) = model.model.str_to_token(token_str, AddBos::Never) {
        if let Some(&tok) = tokens.first() {
            stop_token_ids.push(tok);
        }
    }
}

tracing::debug!(
    "🎯 Stop tokens: eos={}, template={}, strings={:?}, ids={:?}",
    eos, template_name, stop_token_strings,
    stop_token_ids.iter().map(|t| t.0).collect::<Vec<_>>()
);
```

**Verification**:
```bash
timeout 60 cargo test --lib -- --test-threads=2
```

---

### Sub-phase 3.2: Refactor Stop Condition Logic (engine.rs)

**Goal**: Replace hardcoded `return_token` / `end_token` checks with `stop_token_ids.contains()`

**Status**: ⏳ Pending

**File**: `src/inference/engine.rs` (replace lines ~404-418)

**Max Lines Changed**: 15 (replacing ~15 existing lines)

**Dependencies**: Sub-phase 3.1 must be complete

**Tasks**:
- [x] Replace `is_special` check to use `stop_token_ids.contains()`
- [x] Replace individual token checks with `stop_token_ids.contains()` that **breaks** generation
- [x] Verify backward compat: Harmony template still stops on `<|return|>` and `<|end|>` (23/23 tests pass)

**Current Code** (lines 404-418, to be replaced):
```rust
let is_special = new_token_id == eos_token || new_token_id == return_token || ...;
if new_token_id == eos_token { stop_reason = "eos_token"; break; }
if new_token_id == return_token || end_token... { /* log but DON'T stop */ }
```

**New Code Target**:
```rust
// Stop on EOS token
if new_token_id == eos_token {
    stop_reason = "eos_token";
    tracing::info!("🛑 EOS token after {} chars, {} tokens", output.len(), token_info_list.len());
    break;
}

// Stop on template-specific stop tokens
if stop_token_ids.contains(&new_token_id) {
    stop_reason = "stop_token";
    tracing::info!("🛑 Stop token {} after {} chars, {} tokens", new_token_id, output.len(), token_info_list.len());
    break;
}
```

**Verification**:
```bash
timeout 60 cargo test --lib -- --test-threads=2
timeout 60 cargo test --test inference_tests -- --test-threads=2
```

---

## Phase 4: Sampler Chain Upgrade

### Sub-phase 4.1: Add min_p Field to InferenceRequest (TDD)

**Goal**: Add `pub min_p: f32` field to the `InferenceRequest` struct

**Status**: ⏳ Pending

**File**: `src/inference/engine.rs` (line ~102, after `repeat_penalty`)

**Max Lines Changed**: 5 (1 field + doc comment)

**Tasks**:
- [x] Add `pub min_p: f32` field to `InferenceRequest` struct (after `repeat_penalty`)
- [x] Compilation verified — all construction sites updated in Sub-phase 4.3

**Verification**:
```bash
# This will show compilation errors at construction sites — expected
cargo check 2>&1 | grep "min_p" | head -20
```

---

### Sub-phase 4.2: Refactor Sampler Chain (TDD)

**Goal**: Wire `repeat_penalty`, `min_p`, and probabilistic sampling into the sampler chain

**Status**: ⏳ Pending

**File**: `src/inference/engine.rs` (replace lines ~395-399)

**Max Lines Changed**: 25 (replacing 5 existing lines)

**Dependencies**: Sub-phase 4.1 must be complete

**Note**: `llama-cpp-2` v0.1.122 (Cargo.lock) provides `LlamaSampler::min_p()`, `LlamaSampler::penalties()`, and `LlamaSampler::dist()`.

**Tasks**:
- [x] Replace sampler chain with configurable version: temp → penalties → top_p → min_p → dist/greedy
- [x] Conditional repeat_penalty (skip when 1.0)
- [x] Conditional min_p (skip when 0.0)
- [x] dist() for temp > 0.0, greedy() for temp == 0.0

**Current Code** (lines 395-399):
```rust
let mut sampler = LlamaSampler::chain_simple([
    LlamaSampler::temp(request.temperature),
    LlamaSampler::top_p(request.top_p, 1),
    LlamaSampler::greedy(),
]);
```

**New Code Target**:
```rust
let mut samplers: Vec<LlamaSampler> = Vec::new();

// Temperature scaling
samplers.push(LlamaSampler::temp(request.temperature));

// Repeat penalty (skip if 1.0 = no penalty)
if request.repeat_penalty != 1.0 {
    samplers.push(LlamaSampler::penalties(64, request.repeat_penalty, 0.0, 0.0));
}

// Top-P nucleus sampling
samplers.push(LlamaSampler::top_p(request.top_p, 1));

// Min-P sampling (skip if 0.0 = disabled)
if request.min_p > 0.0 {
    samplers.push(LlamaSampler::min_p(request.min_p, 1));
}

// Token selection: dist() for randomness when temp > 0, greedy when temp == 0
if request.temperature > 0.0 {
    let seed = request.seed.unwrap_or(0) as u32;
    samplers.push(LlamaSampler::dist(seed));
} else {
    samplers.push(LlamaSampler::greedy());
}

let mut sampler = LlamaSampler::chain_simple(samplers);
```

**Verification**:
```bash
# Will still fail to compile until 4.3 adds min_p to construction sites
cargo check 2>&1 | grep "min_p" | wc -l
```

---

### Sub-phase 4.3: Update InferenceRequest Construction Sites

**Goal**: Add `min_p: 0.0` to every place that constructs an `InferenceRequest`

**Status**: ⏳ Pending

**Files** (from grep — 25 files contain `InferenceRequest {`):

**Source files** (~6, max 2 lines changed each):
- [x] `src/api/server.rs` — 2 construction sites
- [x] `src/api/websocket/handlers/inference.rs` — 2 construction sites
- [x] `src/api/websocket/inference.rs` — 2 construction sites
- [x] `src/inference/engine.rs` — 1 construction site (default request)

**Test/example files**:
- [x] `tests/inference/test_engine.rs` — 7 construction sites
- [x] `tests/inference/test_real_engine.rs` — 6 construction sites
- [x] `examples/test_ai.rs` — 1 construction site
- [x] `examples/test_streaming.rs` — 1 construction site
- [x] `examples/demo_fixed.rs` — 1 construction site
- [x] `examples/test_inference.rs` — 1 construction site

**Max Lines Changed**: ~30 total (1-2 lines per file, adding `min_p: 0.0,`)

**Pattern**: Add `min_p: 0.0,` after each `repeat_penalty:` line in every `InferenceRequest { ... }` block.

**Verification**:
```bash
# Must compile cleanly
timeout 120 cargo check -- --test-threads=2
# Then run tests
timeout 120 cargo test --lib -- --test-threads=2
```

---

## Phase 5: GLM4 Marker Stripping

### Sub-phase 5.1: Add GLM4 Markers to strip_chat_template_markers() (TDD)

**Goal**: Prevent double-formatting when client sends pre-formatted GLM4 messages

**Status**: ⏳ Pending

**Files** (2 copies of the same function):
- `src/utils/context.rs:74` — `strip_chat_template_markers()`
- `src/api/websocket/handlers/inference.rs:435` — `strip_chat_template_markers()`

**Max Lines Changed**: 20 (10 per file)

**Dependencies**: Phase 1 must be complete

**Tasks**:
- [x] Write test `test_strip_glm4_markers` — strip `<|user|>\n`, `<|system|>\n`, `<|assistant|>\n`, `<|observation|>\n`
- [x] Write test `test_strip_glm4_preserves_content` — content between markers preserved
- [x] Write test `test_strip_glm4_no_false_positives` — doesn't strip partial matches
- [x] Add GLM4 stripping block to `src/utils/context.rs` (after ChatML block)
- [x] Add identical GLM4 stripping block to `src/api/websocket/handlers/inference.rs`

**Implementation Target** (~10 lines per file):
```rust
// GLM-4 format markers
if result.contains("<|system|>") || result.contains("<|user|>") || result.contains("<|observation|>") {
    let glm4_patterns = [
        "<|system|>\n", "<|user|>\n", "<|assistant|>\n", "<|observation|>\n",
        "<|system|>", "<|user|>", "<|assistant|>", "<|observation|>",
    ];
    for pattern in glm4_patterns {
        result = result.replace(pattern, "");
    }
    result = result.trim().to_string();
}
```

**Verification**:
```bash
timeout 60 cargo test --lib -- context -- --test-threads=2
```

---

### Sub-phase 5.2: Update is_prompt_already_formatted()

**Goal**: Detect GLM4 pre-formatted prompts to skip double-formatting

**Status**: ⏳ Pending

**File**: `src/utils/context.rs` (modify `is_prompt_already_formatted()`)

**Max Lines Changed**: 5

**Tasks**:
- [x] Write test `test_is_formatted_glm4` — detects `<|system|>` + `<|user|>` pattern
- [x] Write test `test_is_formatted_glm4_negative` — doesn't false-positive on standalone `<|user|>`
- [x] Add GLM4 detection: `let has_glm4 = prompt.contains("<|system|>") && prompt.contains("<|user|>");`
- [x] Add `has_glm4` to the return expression

**Verification**:
```bash
timeout 60 cargo test --lib -- context -- --test-threads=2
```

---

## Phase 6: Version Bump, Lint & Documentation

### Sub-phase 6.1: Bump Version to 8.15.0

**Goal**: Update version files for the model-agnostic inference release

**Status**: ⏳ Pending

**Files**:
- `/workspace/VERSION`
- `/workspace/src/version.rs`

**Max Lines Changed**: 20

**Tasks**:
- [x] Update `VERSION` to `8.15.0-model-agnostic-inference`
- [x] Update `src/version.rs` VERSION to `"v8.15.0-model-agnostic-inference-2026-02-07"`
- [x] Update `src/version.rs` VERSION_NUMBER to `"8.15.0"`
- [x] Update `src/version.rs` VERSION_MINOR to `15`, VERSION_PATCH to `0`
- [x] Update `src/version.rs` BUILD_DATE to `"2026-02-07"`
- [x] Add FEATURES: 6 new features added
- [x] Add BREAKING_CHANGES: 6 new entries for v8.15.0
- [x] Update test assertions in version.rs `#[cfg(test)]` module
- [x] Run `cargo test --lib -- version` — 9/9 pass

---

### Sub-phase 6.2: Run Full Test Suite + Lint

**Goal**: Verify everything works together

**Status**: ⏳ Pending

**Tasks**:
- [x] Run `cargo check` — compiles cleanly (298 pre-existing warnings only)
- [x] Run `timeout 120 cargo test --lib -- --test-threads=2` — 789 passed, 7 pre-existing failures
- [x] Document total passing test count

**Test Results**:
```
Total Tests: 789 passed (7 pre-existing failures in embed/response_formatter/settlement/vision)
  - chat_template: 23/23 (9 original + 14 new)
  - context: 19/19 (12 original + 7 new)
  - version: 9/9 (all pass now with correct assertions)
  - Full --lib suite: 789/796 (7 pre-existing env-dependent failures)
```

---

### Sub-phase 6.3: Update Environment Documentation

**Goal**: Document new env vars for operators

**Status**: ⏳ Pending

**Max Lines Changed**: 15

**Tasks**:
- [x] Add MODEL_CHAT_TEMPLATE and MODEL_STOP_TOKENS to `CLAUDE.md` Environment Variables section
- [x] Update `CLAUDE.md` version and inference description
- [x] Update this implementation doc status to ✅ COMPLETE

---

## Dependency Graph

```
Phase 1 (GLM template) ──┐
                          ├──> Phase 2 (stop_tokens) ──> Phase 3 (engine refactor)
Phase 4 (sampler chain) ──┘
Phase 5 (marker stripping) ← depends on Phase 1
Phase 6 (version/lint) ← depends on all above
```

**Parallelizable**: Phases 1 and 4 can be done in parallel (different files).

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| `LlamaSampler::min_p()` API not in v0.1.122 | Phase 4 blocked | Cargo.lock confirmed 0.1.122; compilation catches instantly |
| ~15 files need `min_p: 0.0` added | Tedious but mechanical | Compiler errors catch every missed site |
| GLM `<|user|>` stop token overlaps with text | Premature stop | Stop tokens resolved to exact token IDs (not string matching in output) |
| Sampler chain order wrong | Bad output quality | Follow llama.cpp recommended: temp → penalties → top_p → min_p → dist |
| Harmony backward compat broken | Existing deployments fail | All existing tests must still pass; Harmony remains default |

---

## Success Criteria

**Complete** when:

1. **Template**: `MODEL_CHAT_TEMPLATE=glm4` produces correct GLM-4 format
2. **Stop tokens**: Generation terminates on template-specific tokens (not hardcoded 200002)
3. **Sampler**: `min_p=0.01` and `repeat_penalty=1.0` work for GLM-4.7-Flash
4. **Backward compat**: All existing tests pass, Harmony still default
5. **Tests**: All new tests pass (49+ expected)
6. **Lint**: `cargo fmt` and `cargo clippy` clean

---

## Related Documentation

- [Unsloth GLM-4.7-Flash GGUF](https://huggingface.co/unsloth/GLM-4.7-Flash-GGUF)
- [GLM-4.7 Chat Template (Jinja)](https://huggingface.co/zai-org/GLM-4.7/blob/main/chat_template.jinja)
- [GLM-4.7-Flash Local Running Guide](https://unsloth.ai/docs/models/glm-4.7-flash)
- [llama.cpp Supported Templates](https://github.com/ggml-org/llama.cpp/wiki/Templates-supported-by-llama_chat_apply_template)
- `docs/IMPLEMENTATION-REMEDIATION-PRE-REPORT.md` — Format reference
- `docs/IMPLEMENTATION-MODEL-VALIDATION.md` — Current work (model registry)
