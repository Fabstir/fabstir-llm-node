# IMPLEMENTATION - llama-cpp-2 Upgrade + Flash Attention Fallback

## Status: ⏳ IN PROGRESS

**Version**: v8.15.2-llama-upgrade-fa-fallback
**Previous Version**: v8.15.1-kv-cache-quantization
**Start Date**: 2026-02-07
**Approach**: Strict TDD with bounded autonomy - one sub-phase at a time

---

## Overview

GLM-4.7-Flash uses `deepseek2` architecture. When KV cache quantization (q8_0) is enabled, llama.cpp requires Flash Attention. But the FA CUDA kernel bundled with llama-cpp-2 v0.1.122 doesn't support deepseek2 — the FA tensor gets assigned to CPU while KV is on CUDA0, causing a device mismatch and `NullReturn` error. This makes `KV_CACHE_TYPE=q8_0` unusable on GLM-4.7-Flash.

### Root Cause (from production logs)

```
llama_context: layer 0 is assigned to device CUDA0 but the Flash Attention tensor
  is assigned to device CPU (usually due to missing support)
llama_context: Flash Attention was auto, set to disabled
llama_init_from_model: failed to initialize the context:
  quantized V cache was requested, but this requires Flash Attention
```

### What's Changing

| Component | Current (v8.15.1) | Target (v8.15.2) | Impact |
|-----------|-------------------|-------------------|--------|
| llama-cpp-2 | v0.1.122 | v0.1.130+ (latest) | Cargo dep upgrade |
| Flash Attention | Not configurable | `FLASH_ATTN` env var (auto/enabled/disabled) | New env var |
| Context creation | Hard fail on KV quant + no FA | Graceful fallback to f16 KV | Engine resilience |
| EngineConfig | No flash_attn field | `flash_attn_policy: Option<String>` | Struct change |

### Key Architectural Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Fallback strategy | Auto-retry with f16 KV on failure | Node stays up; logs warning so operator knows |
| Flash Attention env var | `FLASH_ATTN` (auto/enabled/disabled) | Matches llama.cpp CLI convention |
| Upgrade target | v0.1.130 minimum | Gets newer llama.cpp with better FA/deepseek2 support |
| Phase ordering | Fallback first, then upgrade | Fallback protects us even if upgrade doesn't fix FA |

### Existing API (v0.1.122, already available but unused)

```rust
// Already exists in llama-cpp-2 v0.1.122:
LlamaContextParams::with_flash_attention_policy(llama_flash_attn_type)
// Values: LLAMA_FLASH_ATTN_TYPE_AUTO (-1), DISABLED (0), ENABLED (1)
```

---

## Implementation Status

| Phase | Sub-phase | Description | Status | Tests | Max Lines |
|-------|-----------|-------------|--------|-------|-----------|
| 1 | 1.1 | Add `flash_attn_policy` to EngineConfig + env var | ⏳ | 0/0 | 10 |
| 1 | 1.2 | Wire flash_attn_policy into context creation | ⏳ | 0/0 | 15 |
| 1 | 1.3 | Add graceful fallback on context creation failure (TDD) | ⏳ | 3/3 | 40 |
| 1 | 1.4 | Update EngineConfig construction sites | ⏳ | 0/0 | ~20 total |
| 2 | 2.1 | Upgrade llama-cpp-2 in Cargo.toml | ⏳ | 0/0 | 2 |
| 2 | 2.2 | Fix compilation errors from API changes | ⏳ | 0/0 | ~50 |
| 2 | 2.3 | Run full test suite + verify | ⏳ | 0/0 | 0 |
| 3 | 3.1 | Bump version to 8.15.2 | ⏳ | 9/9 | 20 |
| 3 | 3.2 | Run full test suite + lint | ⏳ | 0/0 | 0 |
| 3 | 3.3 | Update environment docs | ⏳ | 0/0 | 15 |
| 4 | 4.1 | Build release tarball | ⏳ | 0/0 | 0 |
| **Total** | | | **0%** | **12 new + existing** | **~172** |

---

## Phase 1: Flash Attention Config + Graceful Fallback

### Sub-phase 1.1: Add `flash_attn_policy` to EngineConfig + env var

**Goal**: Add the field to EngineConfig and read it from `FLASH_ATTN` env var

**Status**: ⏳ Pending

**Files**:
- `src/inference/engine.rs` — add field to EngineConfig struct + Default impl
- `src/main.rs` — read `FLASH_ATTN` env var

**Max Lines Changed**: 10

**Tasks**:
- [ ] Add `pub flash_attn_policy: Option<String>` to `EngineConfig` (after `kv_cache_type_v`, line ~80)
- [ ] Add `flash_attn_policy: None` to `Default::default()` (line ~100)
- [ ] Read `FLASH_ATTN` env var in `main.rs` (after KV_CACHE_TYPE, line ~58)
- [ ] Set `flash_attn_policy` in EngineConfig construction in `main.rs` (line ~72)

**Verification**:
```bash
cargo check 2>&1 | grep "flash_attn_policy" | head -20
# Will show errors at construction sites — expected (fixed in 1.4)
```

---

### Sub-phase 1.2: Wire flash_attn_policy into context creation

**Goal**: Set `with_flash_attention_policy()` on LlamaContextParams when configured

**Status**: ⏳ Pending

**File**: `src/inference/engine.rs` (after KV cache type setting, line ~397)

**Max Lines Changed**: 15

**Dependencies**: Sub-phase 1.1

**Tasks**:
- [ ] Add `use llama_cpp_sys_2` to imports (for flash_attn_type constants)
- [ ] Add flash_attn_policy block after KV cache type setting (line ~397)
- [ ] Log the flash attention policy being used

**Implementation Target** (~12 lines):
```rust
// Set flash attention policy
if let Some(ref policy) = self.config.flash_attn_policy {
    let fa_type = match policy.to_lowercase().as_str() {
        "enabled" | "on" | "true" | "1" => llama_cpp_sys_2::LLAMA_FLASH_ATTN_TYPE_ENABLED,
        "disabled" | "off" | "false" | "0" => llama_cpp_sys_2::LLAMA_FLASH_ATTN_TYPE_DISABLED,
        _ => llama_cpp_sys_2::LLAMA_FLASH_ATTN_TYPE_AUTO,
    };
    ctx_params = ctx_params.with_flash_attention_policy(fa_type);
    tracing::info!("Flash attention policy set to: {}", policy);
} else if self.config.kv_cache_type_k.is_some() || self.config.kv_cache_type_v.is_some() {
    // Auto-enable when KV cache quantization is requested
    ctx_params = ctx_params.with_flash_attention_policy(
        llama_cpp_sys_2::LLAMA_FLASH_ATTN_TYPE_AUTO
    );
    tracing::info!("Flash attention policy auto (KV cache quantization requested)");
}
```

**Verification**:
```bash
cargo check 2>&1 | head -20
```

---

### Sub-phase 1.3: Add graceful fallback on context creation failure (TDD)

**Goal**: If context creation fails with quantized KV cache, retry with f16 and log a warning

**Status**: ⏳ Pending

**File**: `src/inference/engine.rs` (replace context creation at line ~399-402)

**Max Lines Changed**: 40 (new fallback logic + 3 tests)

**Dependencies**: Sub-phase 1.2

**Approach**: Test-Driven Development

**Tasks**:
- [ ] Write test `test_parse_flash_attn_policy_enabled` — verify "enabled" maps correctly
- [ ] Write test `test_parse_flash_attn_policy_disabled` — verify "disabled" maps correctly
- [ ] Write test `test_parse_flash_attn_policy_auto` — verify "auto" and unknown strings map to auto
- [ ] Add `has_kv_quantization` bool flag before context creation
- [ ] Wrap context creation in match with fallback arm
- [ ] Log warning when falling back to f16

**Implementation Target** (~20 lines for fallback):
```rust
let has_kv_quantization = self.config.kv_cache_type_k.is_some()
    || self.config.kv_cache_type_v.is_some();

let mut context = match model.model.new_context(&model.backend, ctx_params) {
    Ok(ctx) => ctx,
    Err(e) if has_kv_quantization => {
        tracing::warn!(
            "⚠️ Context creation failed with quantized KV cache: {:?}. \
             Retrying with f16 KV cache (Flash Attention may not support this model architecture)...",
            e
        );
        let fallback_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(context_size as u32))
            .with_n_batch(self.config.batch_size as u32);
        model
            .model
            .new_context(&model.backend, fallback_params)
            .map_err(|e2| anyhow!(
                "Failed to create context even with f16 fallback: {:?} (original error: {:?})",
                e2, e
            ))?
    }
    Err(e) => return Err(anyhow!("Failed to create context: {:?}", e)),
};
```

**Test Template** (~18 lines):
```rust
#[test]
fn test_parse_flash_attn_policy_enabled() {
    // Verify the mapping logic (we test the string matching, not the llama_cpp_sys constant)
    let policy = "enabled";
    let result = match policy.to_lowercase().as_str() {
        "enabled" | "on" | "true" | "1" => "enabled",
        "disabled" | "off" | "false" | "0" => "disabled",
        _ => "auto",
    };
    assert_eq!(result, "enabled");
}

#[test]
fn test_parse_flash_attn_policy_disabled() {
    for input in &["disabled", "off", "false", "0"] {
        let result = match input.to_lowercase().as_str() {
            "enabled" | "on" | "true" | "1" => "enabled",
            "disabled" | "off" | "false" | "0" => "disabled",
            _ => "auto",
        };
        assert_eq!(result, "disabled", "Failed for input: {}", input);
    }
}

#[test]
fn test_parse_flash_attn_policy_auto() {
    for input in &["auto", "AUTO", "anything", ""] {
        let result = match input.to_lowercase().as_str() {
            "enabled" | "on" | "true" | "1" => "enabled",
            "disabled" | "off" | "false" | "0" => "disabled",
            _ => "auto",
        };
        assert_eq!(result, "auto", "Failed for input: {}", input);
    }
}
```

**Verification**:
```bash
timeout 60 cargo test --lib -- engine::tests::test_parse_flash_attn -- --test-threads=1
```

---

### Sub-phase 1.4: Update EngineConfig construction sites

**Goal**: Add `flash_attn_policy: None` to all EngineConfig construction sites

**Status**: ⏳ Pending

**Files** (from grep — 8 files with `EngineConfig {`):
- [ ] `src/main.rs` — 1 site (set from env var, not None)
- [ ] `src/inference/engine.rs` — Default impl (already done in 1.1)
- [ ] `src/api/websocket/inference.rs` — 1 site
- [ ] `src/job_processor.rs` — 1 site
- [ ] `tests/inference/test_engine.rs` — multiple sites
- [ ] `tests/inference/test_real_engine.rs` — multiple sites
- [ ] `tests/api/test_real_inference.rs` — 1 site
- [ ] `examples/demo_fixed.rs` — 1 site

**Note**: `tests/test_gpt_oss_20b_inference.rs` uses OLD EngineConfig API — don't touch.

**Max Lines Changed**: ~20 total (1 line per construction site)

**Pattern**: Add `flash_attn_policy: None,` after each `kv_cache_type_v:` line.

**Verification**:
```bash
cargo check 2>&1 | grep "flash_attn_policy"
# Should show 0 errors
timeout 120 cargo test --lib -- --test-threads=1
```

---

## Phase 2: Upgrade llama-cpp-2

### Sub-phase 2.1: Upgrade llama-cpp-2 in Cargo.toml

**Goal**: Bump the dependency to get a newer bundled llama.cpp

**Status**: ⏳ Pending

**File**: `Cargo.toml`

**Max Lines Changed**: 2

**Tasks**:
- [ ] Change `llama-cpp-2 = { version = "0.1.55", features = ["cuda"] }` to `llama-cpp-2 = { version = "0.1.130", features = ["cuda"] }`
- [ ] Run `cargo update -p llama-cpp-2` to update Cargo.lock

**Verification**:
```bash
cargo update -p llama-cpp-2 2>&1
grep "llama-cpp-2" Cargo.lock | head -5
```

---

### Sub-phase 2.2: Fix compilation errors from API changes

**Goal**: Adapt engine.rs to any API changes in the newer version

**Status**: ⏳ Pending

**File**: `src/inference/engine.rs` (primary), possibly others

**Max Lines Changed**: ~50

**Dependencies**: Sub-phase 2.1

**Known risk areas** (from changelog research):
- **v0.1.128 lifetime changes**: `LlamaContext<'a>` may require explicit lifetimes on `RealLlamaModel`
- **Sampler API**: Method signatures for `temp()`, `penalties()`, `top_p()`, `min_p()`, `dist()`, `greedy()`, `chain_simple()` may have changed
- **Token API**: `token_to_str()` may be refactored for stateful UTF-8 parsing
- **Error types**: Different error wrapper types

**Tasks**:
- [ ] Run `cargo check` and collect all errors
- [ ] Fix each error, starting with imports
- [ ] Fix lifetime annotations if needed (RealLlamaModel struct)
- [ ] Fix sampler chain calls if signatures changed
- [ ] Fix token-to-string calls if API changed
- [ ] Ensure all existing functionality is preserved

**Approach**: Iterative — run `cargo check`, fix one category of errors, repeat.

**Verification**:
```bash
cargo check 2>&1 | grep "^error" | wc -l
# Target: 0 errors
```

---

### Sub-phase 2.3: Run full test suite + verify

**Goal**: Confirm nothing regressed

**Status**: ⏳ Pending

**Dependencies**: Sub-phase 2.2

**Tasks**:
- [ ] Run `cargo test --lib -- --test-threads=1` — all existing tests pass
- [ ] Run `cargo test --test inference_tests -- --test-threads=1` — inference tests pass
- [ ] Document any new warnings or behavior changes

**Verification**:
```bash
timeout 300 cargo test --lib -- --test-threads=1 2>&1 | tail -5
timeout 120 cargo test --test inference_tests -- --test-threads=1 2>&1 | tail -5
```

---

## Phase 3: Version Bump + Docs

### Sub-phase 3.1: Bump version to 8.15.2

**Goal**: Update version files for the upgrade release

**Status**: ⏳ Pending

**Files**:
- `/workspace/VERSION`
- `/workspace/src/version.rs`

**Max Lines Changed**: 20

**Tasks**:
- [ ] Update `VERSION` to `8.15.2-llama-upgrade-fa-fallback`
- [ ] Update `src/version.rs` VERSION to `"v8.15.2-llama-upgrade-fa-fallback-2026-02-07"`
- [ ] Update `src/version.rs` VERSION_NUMBER to `"8.15.2"`
- [ ] Update `src/version.rs` VERSION_PATCH to `2`
- [ ] Add FEATURES entries for flash_attn + fallback
- [ ] Add BREAKING_CHANGES entry for llama-cpp-2 upgrade
- [ ] Update test assertions in `#[cfg(test)]` module
- [ ] Run `cargo test --lib -- version` — 9/9 pass

**Verification**:
```bash
timeout 60 cargo test --lib -- version::tests -- --test-threads=1
```

---

### Sub-phase 3.2: Run full test suite + lint

**Goal**: Final verification

**Status**: ⏳ Pending

**Tasks**:
- [ ] Run `cargo check` — compiles cleanly
- [ ] Run `timeout 300 cargo test --lib -- --test-threads=1` — all tests pass
- [ ] Document total passing test count

---

### Sub-phase 3.3: Update environment docs

**Goal**: Document new `FLASH_ATTN` env var

**Status**: ⏳ Pending

**Max Lines Changed**: 15

**Tasks**:
- [ ] Add `FLASH_ATTN` env var to CLAUDE.md Environment Variables section
- [ ] Update CLAUDE.md version reference to v8.15.2
- [ ] Add `FLASH_ATTN` to docker-compose.prod.yml environment block (optional, passed through via env_file)
- [ ] Update this implementation doc status to ✅ COMPLETE

---

## Phase 4: Build + Deploy

### Sub-phase 4.1: Build release tarball

**Goal**: Create deployment artifact for local node

**Status**: ⏳ Pending

**Tasks**:
- [ ] Run `cargo build --release --features real-ezkl -j 4`
- [ ] Verify version: `strings target/release/fabstir-llm-node | grep "v8.15.2"`
- [ ] Create tarball: `cp target/release/fabstir-llm-node ./fabstir-llm-node && tar -czvf fabstir-llm-node-v8.15.2-llama-upgrade-fa-fallback.tar.gz fabstir-llm-node scripts/download_florence_model.sh scripts/download_ocr_models.sh scripts/download_embedding_model.sh scripts/setup_models.sh`

---

## Dependency Graph

```
Phase 1.1 (EngineConfig field) ──> 1.2 (wire flash_attn) ──> 1.3 (fallback logic)
                                                                      │
Phase 1.4 (construction sites) ← depends on 1.1                      │
                                                                      ▼
Phase 2.1 (Cargo upgrade) ──> 2.2 (fix compilation) ──> 2.3 (test suite)
                                                                      │
                                                                      ▼
Phase 3.1 (version) ──> 3.2 (full tests) ──> 3.3 (docs)
                                                    │
                                                    ▼
                                              Phase 4.1 (tarball)
```

**Note**: Phase 1 (fallback) is independent of Phase 2 (upgrade). If the upgrade proves too disruptive, Phase 1 alone provides resilience — the node auto-falls back to f16 KV cache instead of crashing.

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| v0.1.128 lifetime changes break RealLlamaModel | Phase 2 blocked | Add lifetime annotations; fallback to Phase 1 only |
| Newer llama.cpp still doesn't support FA for deepseek2 | KV quant still fails | Phase 1 fallback auto-recovers to f16 |
| Sampler API changed | Compilation errors | Iterative fix; sampler calls are contained in engine.rs |
| `with_flash_attention_policy()` removed in newer API | Phase 1.2 broken | Use raw `llama_cpp_sys_2` bindings directly |
| Build time increase with newer llama.cpp | Slower CI | Use `-j 4` as before |
| Blackwell compute cap 12.0 not supported | FA still fails | Phase 1 fallback handles this gracefully |

---

## Success Criteria

**Complete** when:

1. **Fallback**: Node with `KV_CACHE_TYPE=q8_0` auto-falls back to f16 with warning (not crash)
2. **Flash Attention**: `FLASH_ATTN=enabled/disabled/auto` env var works
3. **Upgrade**: llama-cpp-2 updated to v0.1.130+
4. **Tests**: All existing tests pass + 3 new tests for flash_attn policy parsing
5. **Deploy**: Tarball built and tested on local GLM-4.7-Flash node
6. **Backward compat**: All existing deployments unaffected (FLASH_ATTN defaults to auto)

---

## Related Documentation

- `docs/IMPLEMENTATION-KV-CACHE-QUANTIZATION.md` — v8.15.1 KV cache feature (predecessor)
- `docs/IMPLEMENTATION-MODEL-AGNOSTIC-INFERENCE.md` — v8.15.0 GLM-4 support
- [llama-cpp-2 crate](https://crates.io/crates/llama-cpp-2) — Rust bindings
- [llama-cpp-2 docs](https://docs.rs/crate/llama-cpp-2/latest) — API documentation
- llama-cpp-2 v0.1.122 source: `/usr/local/cargo/registry/src/index.crates.io-*/llama-cpp-2-0.1.122/`
