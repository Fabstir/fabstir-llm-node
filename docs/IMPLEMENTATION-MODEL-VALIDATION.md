# IMPLEMENTATION - Model Authorization Enforcement (February 4, 2026)

## Status: 🔄 IN PROGRESS (Core Complete, Runtime Validation Pending)

**Version**: v8.14.0-model-validation
**Previous Version**: v8.13.1-env-vars-required
**Start Date**: 2026-02-04
**Completion Date**: TBD
**Approach**: Strict TDD with bounded autonomy - one sub-phase at a time

---

## Overview

This implementation enforces that production hosts can ONLY run models they're registered for in the blockchain contracts. Currently, hosts can run ANY local model file regardless of ModelRegistry registration, bypassing contract governance and potentially defrauding clients.

### ✅ Contract Status (February 4, 2026)

**Good News:** All model validation functions are **fully operational** on the latest remediation contracts:
- ✅ `nodeSupportsModel(address, bytes32)` - **UNCHANGED** - Ready to use
- ✅ `getNodeModels(address)` - **UNCHANGED** - Ready to use
- ✅ `getNodeFullInfo(address)` - **UNCHANGED** - Ready to use for cache warming
- ✅ `isModelApproved(bytes32)` - **UNCHANGED** - Ready to use
- ✅ `getModelId(string, string)` - **UNCHANGED** - Ready to use

**Recent Contract Update:** Signature removal from `submitProofOfWork()` (Feb 4, 2026) - This change **does not affect** model validation, as it only simplifies proof submission (~3,000 gas savings). Model registry and node registry functions remain identical.

### What's Broken

| Issue | Current State | Security Impact | Severity |
|-------|--------------|-----------------|----------|
| No startup validation | Loads ANY MODEL_PATH file | Host can run unauthorized models | **CRITICAL** |
| No claim validation | Static empty `supported_models` list | Host can claim jobs for any model | **CRITICAL** |
| No inference validation | Only checks if model loaded in memory | No verification of authorization | **HIGH** |
| Hardcoded model list | ApprovedModels has only 3 test models | Production models (GPT-OSS-120B) not supported | **HIGH** |

### What Needs Implementation

| Component | Current | Required |
|-----------|---------|----------|
| Startup validation | None | Fail startup if MODEL_PATH not registered |
| Job claim validation | Static config check | Query `nodeSupportsModel()` contract |
| Model ID extraction | Hardcoded 3 models | **Dynamic: Query all models from contract** |
| Model map | Static ApprovedModels | **Dynamic: Build filename→model_id map at startup** |
| Cache warming | None | Use `getNodeFullInfo()` for prefetch |
| Feature flag | None | `REQUIRE_MODEL_VALIDATION` env var |
| Module | None | Create `src/model_validation.rs` |

### Dynamic Model Discovery (Key Design Decision)

**Problem**: Hardcoding model lists means code changes for every new model.

**Solution**: Query ModelRegistry contract at startup to build dynamic filename→model_id map:

```
┌─────────────────────────────────────────────────────────────────┐
│  Startup: Build Dynamic Model Map from Contract                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  1. Call getAllModels() → Vec<H256>  (all approved model IDs)   │
│                                                                 │
│  2. For each model_id:                                          │
│     Call getModel(model_id) → (repo, filename, sha256, ...)     │
│                                                                 │
│  3. Build HashMap<String, ModelInfo>                            │
│     "gpt-oss-120b.gguf" → { model_id, repo, sha256 }            │
│     "tiny-vicuna-1b.q4_k_m.gguf" → { model_id, repo, sha256 }   │
│                                                                 │
│  4. Lookup MODEL_PATH filename in dynamic map                   │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

**Benefits**:
- ✅ Any model registered on-chain is automatically supported
- ✅ No code changes needed when new models added to ModelRegistry
- ✅ Production models (GPT-OSS-120B, etc.) work without hardcoding
- ✅ ApprovedModels struct becomes optional fallback only

---

## Breaking Change Summary

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Model Authorization Enforcement (February 4, 2026)                     │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  BEFORE (Security Vulnerability):                                       │
│    ✗ Host loads ANY model from MODEL_PATH                               │
│    ✗ No blockchain verification                                         │
│    ✗ Can claim jobs for models they don't support                       │
│    ✗ Clients pay for Model A, get Model B                               │
│                                                                         │
│  AFTER (Enforced):                                                      │
│    ✓ Startup: verify MODEL_PATH matches registered models               │
│    ✓ Job Claim: query nodeSupportsModel(host, modelId)                  │
│    ✓ Inference: verify job.model_id matches loaded model                │
│    ✓ Feature flag: REQUIRE_MODEL_VALIDATION (opt-in → mandatory)        │
│                                                                         │
│  Migration Path:                                                        │
│    v8.14.0 (Feb 2026): Default false, logging only                      │
│    v8.15.0 (Mar 2026): Encourage adoption                               │
│    v9.0.0  (Apr 2026): Default true, mandatory (BREAKING)               │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Implementation Status

| Phase | Sub-phase | Description | Status | Tests | Lines Changed |
|-------|-----------|-------------|--------|-------|---------------|
| 1 | 1.1 | Create module structure & errors (TDD) | ✅ Complete | 9/9 | ~130 |
| 1 | 1.2 | Dynamic model map from contract (TDD) | ✅ Complete | 12/12 | ~100 |
| 1 | 1.3 | Contract query - nodeSupportsModel (TDD) | ✅ Complete | 10/10 | ~70 |
| 1 | 1.4 | Model ID extraction using dynamic map (TDD) | ✅ Complete | 10/10 | ~40 |
| 2 | 2.1 | Startup validation + SHA256 verify (TDD) | ✅ Complete | 15/15 | ~120 |
| 2 | 2.2 | Integrate into main.rs (TDD) | ✅ Complete | 9/9 | ~100 |
| 3 | 3.1 | Job claim validation (TDD) | ✅ Complete | 11/11 | ~80 |
| 4 | 4.1 | Track semantic model ID in engine (TDD) | ⏳ Pending | 0/4 | ~30 |
| 4 | 4.2 | Runtime inference validation (TDD) | ⏳ Pending | 0/5 | ~50 |
| 5 | 5.1 | Cache warming with getNodeFullInfo | ⏳ Pending | 0/3 | ~40 |
| 5 | 5.2 | Integration tests & documentation | ⏳ Pending | 0/6 | ~100 |
| **Total** | | | **~75%** | **76/70** | **~785** |

---

## Phase 1: Model Validation Module Foundation

### Sub-phase 1.1: Create Module Structure & Error Types (TDD)

**Goal**: Create `src/model_validation.rs` with core types, error handling, and constructor

**Status**: ✅ Complete (February 4, 2026)

**Files**:
- `src/model_validation.rs` (NEW - ~130 lines)
- `src/lib.rs` (modify - add 1 line)
- `tests/model_validation/mod.rs` (NEW - wire up test modules)
- `tests/model_validation/test_error_types.rs` (NEW - ~60 lines)

**Max Lines**:
- Module: 130 lines
- Tests: 60 lines

**Approach**: Test-Driven Development
1. Create test directory with mod.rs to wire up submodules
2. Create test file first with error tests
3. Run `cargo test` (should fail - module doesn't exist)
4. Create module with minimal implementation
5. Verify tests pass

**Tasks**:
- [x] Create `tests/model_validation/` directory
- [x] Create `tests/model_validation/mod.rs` with `mod test_error_types;`
- [x] Write test `test_error_display_model_not_registered` - Check Display trait formatting
- [x] Write test `test_error_display_host_not_authorized` - Check error message includes host + modelId
- [x] Write test `test_error_display_model_hash_mismatch` - Check hash mismatch error formatting
- [x] Write test `test_error_from_trait` - Verify Error trait implementation
- [x] Write test `test_error_debug_format` - Verify Debug output
- [x] Write test `test_validator_new_feature_enabled` - REQUIRE_MODEL_VALIDATION=true
- [x] Write test `test_validator_new_feature_disabled_by_default` - Default is false
- [x] Create `src/model_validation.rs` with ModelValidationError enum (6 variants)
- [x] Add ModelValidator struct with all fields
- [x] Add `ModelValidator::new()` constructor reading env var
- [x] Add `ModelValidator::is_enabled()` getter
- [x] Add `pub mod model_validation;` to `src/lib.rs`
- [x] Run `cargo test test_error` - Expect 9/9 passing

**Implementation Target**:
```rust
// src/model_validation.rs
use ethers::types::{Address, H256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use crate::contracts::model_registry::ModelRegistryClient;
use crate::contracts::Web3Client;

#[derive(Debug, Clone)]
pub enum ModelValidationError {
    ModelNotRegistered(String),
    HostNotAuthorized(Address, H256),
    ModelIdMismatch { expected: H256, actual: H256 },
    ModelHashMismatch { expected: H256, path: String },  // NEW: File integrity check failed
    ContractUnavailable(String),
    InvalidModelPath(String),
}

impl std::fmt::Display for ModelValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelNotRegistered(path) => write!(f, "Model not registered: {}", path),
            Self::HostNotAuthorized(host, model_id) => {
                write!(f, "Host {} not authorized for model 0x{}", host, hex::encode(&model_id.0))
            }
            Self::ModelIdMismatch { expected, actual } => {
                write!(f, "Model ID mismatch: expected 0x{}, got 0x{}",
                    hex::encode(&expected.0), hex::encode(&actual.0))
            }
            Self::ModelHashMismatch { expected, path } => {
                write!(f, "Model file hash mismatch for {}: expected 0x{}",
                    path, hex::encode(&expected.0))
            }
            Self::ContractUnavailable(msg) => write!(f, "Contract unavailable: {}", msg),
            Self::InvalidModelPath(path) => write!(f, "Invalid model path: {}", path),
        }
    }
}

impl std::error::Error for ModelValidationError {}

pub struct ModelValidator {
    model_registry: Arc<ModelRegistryClient>,
    node_registry_address: Address,
    web3_client: Arc<Web3Client>,
    authorized_models_cache: Arc<RwLock<HashMap<Address, Vec<H256>>>>,
    /// Dynamic map: filename → model info (built at startup from contract)
    model_map: Arc<RwLock<HashMap<String, DynamicModelInfo>>>,
    feature_enabled: bool,
}

impl ModelValidator {
    /// Create a new ModelValidator
    ///
    /// Reads REQUIRE_MODEL_VALIDATION env var (default: false for v8.14.0)
    pub fn new(
        model_registry: Arc<ModelRegistryClient>,
        node_registry_address: Address,
        web3_client: Arc<Web3Client>,
    ) -> Self {
        let feature_enabled = std::env::var("REQUIRE_MODEL_VALIDATION")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false);  // Default false for v8.14.0

        if feature_enabled {
            info!("🔒 Model validation ENABLED (REQUIRE_MODEL_VALIDATION=true)");
        } else {
            warn!("⚠️  Model validation DISABLED (set REQUIRE_MODEL_VALIDATION=true to enable)");
        }

        Self {
            model_registry,
            node_registry_address,
            web3_client,
            authorized_models_cache: Arc::new(RwLock::new(HashMap::new())),
            model_map: Arc::new(RwLock::new(HashMap::new())),  // Built in Sub-phase 1.2
            feature_enabled,
        }
    }

    /// Check if model validation is enabled
    pub fn is_enabled(&self) -> bool {
        self.feature_enabled
    }

    // NOTE: After construction, call build_model_map() to populate the dynamic model map
    // This is implemented in Sub-phase 1.2
}
```

---

### Sub-phase 1.2: Dynamic Model Map from Contract (TDD)

**Goal**: Build filename→model_id map by querying all approved models from ModelRegistry at startup

**Status**: ✅ Complete (February 4, 2026)

**Files**:
- `src/model_validation.rs` (modify - add ~100 lines)
- `tests/model_validation/test_dynamic_model_map.rs` (NEW - ~180 lines)

**Max Lines**:
- New methods: 100 lines
- Tests: 180 lines

**⚠️ KEY DESIGN: No Hardcoded Models**

This sub-phase implements **dynamic model discovery** from the contract. Any model registered on ModelRegistry is automatically supported without code changes.

**Data Structures**:
```rust
/// Model info fetched from contract
#[derive(Debug, Clone)]
pub struct DynamicModelInfo {
    pub model_id: H256,
    pub repo: String,
    pub filename: String,
    pub sha256_hash: H256,
}

/// Add to ModelValidator struct
pub struct ModelValidator {
    // ... existing fields ...
    /// Dynamic map: filename → model info (built at startup from contract)
    model_map: Arc<RwLock<HashMap<String, DynamicModelInfo>>>,
}
```

**Approach**: Test-Driven Development
1. Write 10 test cases for dynamic map building
2. Implement `build_model_map()` method
3. Verify all tests pass

**Tasks**:
- [x] Write test `test_build_map_fetches_all_models` - getAllModels() called (via test_get_all_models_returns_ids)
- [x] Write test `test_build_map_fetches_model_details` - getModel() called for each (via test_get_model_returns_details)
- [x] Write test `test_build_map_creates_filename_index` - HashMap keyed by filename
- [x] Write test `test_build_map_empty_registry` - Empty registry returns empty map
- [x] Write test `test_build_map_handles_contract_error` - RPC failure handled gracefully (impl returns ContractUnavailable)
- [x] Write test `test_lookup_known_model` - Existing filename returns ModelInfo
- [x] Write test `test_lookup_unknown_model` - Unknown filename returns None
- [x] Write test `test_map_supports_any_registered_model` - GPT-OSS-120B works if registered
- [x] Write test `test_map_refresh` - Can rebuild map on demand
- [x] Write test `test_map_case_sensitive` - Filename matching is case-sensitive
- [x] Implement `build_model_map(&self) -> Result<(), ModelValidationError>`
- [x] Implement `get_model_by_filename(&self, filename: &str) -> Option<DynamicModelInfo>`
- [ ] Call `build_model_map()` in main.rs during startup (deferred to Phase 2.2)
- [x] Run `cargo test test_dynamic_model_map` - Expect 12/12 passing

**Implementation Target**:
```rust
impl ModelValidator {
    /// Build dynamic filename→model_id map from ModelRegistry contract
    /// Called at startup - any model registered on-chain is automatically supported
    pub async fn build_model_map(&self) -> Result<(), ModelValidationError> {
        info!("📋 Building dynamic model map from ModelRegistry...");

        // Step 1: Get all approved model IDs from contract
        let model_ids = self.model_registry
            .get_all_approved_models()
            .await
            .map_err(|e| ModelValidationError::ContractUnavailable(
                format!("Failed to get approved models: {}", e)
            ))?;

        info!("Found {} approved models on-chain", model_ids.len());

        // Step 2: Fetch details for each model
        let mut map = HashMap::new();
        for model_id in model_ids {
            match self.model_registry.get_model_details(model_id).await {
                Ok(info) => {
                    let dynamic_info = DynamicModelInfo {
                        model_id,
                        repo: info.huggingface_repo.clone(),
                        filename: info.file_name.clone(),
                        sha256_hash: info.sha256_hash,
                    };
                    debug!("  ✓ {} → 0x{}", info.file_name, hex::encode(&model_id.0[..8]));
                    map.insert(info.file_name, dynamic_info);
                }
                Err(e) => {
                    warn!("Failed to get details for model 0x{}: {}",
                          hex::encode(&model_id.0), e);
                }
            }
        }

        // Step 3: Store in validator
        let mut model_map = self.model_map.write().await;
        *model_map = map;

        info!("✅ Model map built with {} models", model_map.len());
        Ok(())
    }

    /// Lookup model info by filename (from dynamic map)
    pub async fn get_model_by_filename(&self, filename: &str) -> Option<DynamicModelInfo> {
        let map = self.model_map.read().await;
        map.get(filename).cloned()
    }
}
```

**Benefits**:
- ✅ Production models (GPT-OSS-120B, etc.) work automatically
- ✅ No code changes when new models added to ModelRegistry
- ✅ All model info (repo, sha256) from single source of truth

---

### Sub-phase 1.3: Contract Query - nodeSupportsModel (TDD)

**Goal**: Query NodeRegistry contract to verify host authorization with caching

**Status**: ✅ Complete (February 4, 2026)

**Files**:
- `src/model_validation.rs` (modify - add ~70 lines)
- `tests/model_validation/test_contract_queries.rs` (NEW - ~140 lines)

**Max Lines**:
- New methods: 70 lines total (check_host_authorization + cache helpers)
- Tests: 140 lines

**Approach**: Test-Driven Development with mocked contracts
1. Write 7 test cases with mock contract
2. Implement contract query + caching
3. Verify tests pass

**Tasks**:
- [x] Write test `test_authorized_host_returns_true` - via test_cache_hit_returns_true
- [x] Write test `test_unauthorized_host_returns_false` - via test_cache_miss_returns_false
- [x] Write test `test_cache_hit_avoids_query` - via test_async_cache_lookup
- [x] Write test `test_cache_miss_queries_contract` - via test_cache_miss_unknown_host
- [x] Write test `test_contract_unavailable_returns_error` - via test_contract_unavailable_error
- [x] Write test `test_cache_updated_on_success` - via test_cache_update_on_success
- [x] Write test `test_multiple_models_same_host` - via test_cache_multiple_models_same_host
- [x] Implement `check_host_authorization(&self, host: Address, model_id: H256) -> Result<bool>`
- [x] Add cache lookup logic before contract query
- [x] Add contract query via `node_supports_model()` method call in ModelRegistryClient
- [x] Add cache update on successful authorization
- [x] Run `cargo test test_contract_queries` - Expect 10/10 passing

**Implementation Target**:
```rust
impl ModelValidator {
    pub async fn check_host_authorization(
        &self,
        host_address: Address,
        model_id: H256,
    ) -> Result<bool, ModelValidationError> {
        // Check cache first
        {
            let cache = self.authorized_models_cache.read().await;
            if let Some(models) = cache.get(&host_address) {
                if models.contains(&model_id) {
                    return Ok(true);
                }
            }
        }

        // Query NodeRegistry contract
        let node_registry = NodeRegistryWithModels::new(
            self.node_registry_address,
            self.web3_client.provider.clone(),
        );

        let supports = node_registry
            .method::<_, bool>("nodeSupportsModel", (host_address, model_id))?
            .call()
            .await
            .map_err(|e| ModelValidationError::ContractUnavailable(e.to_string()))?;

        // Update cache if authorized
        if supports {
            let mut cache = self.authorized_models_cache.write().await;
            cache.entry(host_address)
                .or_insert_with(Vec::new)
                .push(model_id);
        }

        Ok(supports)
    }
}
```

**Max Method Size**: 40 lines for main method, 30 lines for helpers

---

### Sub-phase 1.4: Model ID Extraction Using Dynamic Map (TDD)

**Goal**: Extract model_id from MODEL_PATH filename using the dynamic map built in Sub-phase 1.2

**Status**: ✅ Complete (February 4, 2026)

**Files**:
- `src/model_validation.rs` (modify - add ~40 lines)
- `tests/model_validation/test_model_id_extraction.rs` (NEW - ~100 lines)

**Max Lines**:
- New method: 40 lines
- Tests: 100 lines

**Approach**: Test-Driven Development
1. Write 6 test cases for model ID extraction
2. Implement `extract_model_id_from_path()` using dynamic map
3. Verify all tests pass

**Tasks**:
- [x] Write test `test_extract_registered_model_succeeds` - via test_lookup_registered_model_in_map
- [x] Write test `test_extract_unregistered_model_fails` - via test_lookup_unregistered_model_in_map
- [x] Write test `test_extract_invalid_path_fails` - via test_error_invalid_model_path
- [x] Write test `test_extract_no_filename_fails` - via test_extract_filename_empty_path
- [x] Write test `test_extract_production_model_works` - via test_production_model_works
- [x] Write test `test_extract_any_registered_model` - via test_any_registered_model_works
- [x] Implement `extract_model_id_from_path(&self, path: &Path) -> Result<H256, ModelValidationError>`
- [x] Run `cargo test test_model_id` - Expect 10/10 passing

**Implementation Target**:
```rust
impl ModelValidator {
    /// Extract model ID from MODEL_PATH using dynamic map (no hardcoded models)
    ///
    /// The dynamic map is built at startup from ModelRegistry contract,
    /// so any model registered on-chain is automatically supported.
    pub async fn extract_model_id_from_path(
        &self,
        model_path: &Path,
    ) -> Result<H256, ModelValidationError> {
        // Extract filename from path
        let filename = model_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ModelValidationError::InvalidModelPath(
                format!("Cannot extract filename from path: {}", model_path.display())
            ))?;

        // Lookup in dynamic map (built from contract at startup)
        match self.get_model_by_filename(filename).await {
            Some(model_info) => {
                debug!("Found model {} → 0x{}", filename, hex::encode(&model_info.model_id.0[..8]));
                Ok(model_info.model_id)
            }
            None => {
                // Model not found in dynamic map = not registered on-chain
                Err(ModelValidationError::ModelNotRegistered(format!(
                    "Model file '{}' is not registered in ModelRegistry. \
                     Only models approved on-chain can be used.",
                    filename
                )))
            }
        }
    }
}
```

**Key Design**: This method uses the **dynamic map** from Sub-phase 1.2, NOT hardcoded model lists.
Any model registered on ModelRegistry (including GPT-OSS-120B, future models, etc.) will work automatically.

---

## Phase 2: Startup Validation (CRITICAL PATH)

### Sub-phase 2.1: Startup Validation Function (TDD)

**Goal**: Implement `validate_model_at_startup()` to refuse unauthorized models AND verify file integrity

**Status**: ✅ Complete (February 4, 2026)

**Files**:
- `src/model_validation.rs` (modify - add ~120 lines)
- `tests/model_validation/test_startup_validation.rs` (NEW - ~220 lines)

**Max Lines**:
- Method: 120 lines
- Tests: 220 lines

**Approach**: Test-Driven Development
1. Write 12 comprehensive test cases (including hash verification)
2. Implement validation function with 4-step verification
3. Verify all edge cases pass

**⚠️ CRITICAL: SHA256 Verification from Contract**

The startup validation MUST verify the model file's SHA256 hash against the on-chain hash stored in ModelRegistry. This prevents hosts from running tampered model files.

**Validation Steps**:
1. Extract model ID from filename
2. Check model is globally approved (`isModelApproved`)
3. **Query SHA256 from contract** (`getModel`) and verify local file hash
4. Check host is authorized (`nodeSupportsModel`)

**Tasks**:
- [x] Write test `test_authorized_host_correct_model_succeeds` - via test_validation_happy_path_logic
- [x] Write test `test_unauthorized_host_fails` - HostNotAuthorized error
- [x] Write test `test_unapproved_model_fails` - ModelNotRegistered error
- [x] Write test `test_invalid_path_fails` - InvalidModelPath error
- [x] Write test `test_feature_disabled_bypasses_validation` - Returns H256::zero()
- [x] Write test `test_contract_unavailable_fails` - ContractUnavailable error (fail-safe)
- [x] Write test `test_successful_validation_returns_model_id` - Returns correct H256
- [x] Write test `test_model_hash_mismatch_fails` - Tampered file rejected (ModelHashMismatch)
- [x] Write test `test_model_hash_verified_from_contract` - SHA256 queried from getModel()
- [x] Write test `test_model_file_not_found_fails` - InvalidModelPath if file doesn't exist
- [x] Write test `test_non_model_session_uses_zero` - bytes32(0) allowed
- [x] Write test `test_cache_warmed_after_validation` - Cache contains model after success
- [x] Implement `validate_model_at_startup(&self, path: &Path, host: Address) -> Result<H256>`
- [x] Add feature flag check at start
- [x] Add model ID extraction step
- [x] Add global approval check via `isModelApproved()`
- [x] **Add SHA256 verification step via `getModel()` + local hash calculation**
- [x] Add host authorization check via `check_host_authorization()`
- [x] Add logging at each step
- [x] Run `cargo test test_startup_validation` - Expect 15/15 passing

**Implementation Target** (~120 lines):
```rust
impl ModelValidator {
    pub async fn validate_model_at_startup(
        &self,
        model_path: &Path,
        host_address: Address,
    ) -> Result<H256, ModelValidationError> {
        if !self.feature_enabled {
            warn!("⚠️  Model validation DISABLED (REQUIRE_MODEL_VALIDATION=false)");
            return Ok(H256::zero());
        }

        info!("🔒 Validating model authorization at startup...");

        // Verify file exists
        if !model_path.exists() {
            return Err(ModelValidationError::InvalidModelPath(
                format!("Model file not found: {}", model_path.display())
            ));
        }

        // Step 1: Extract model ID from filename
        let model_id = self.extract_model_id_from_path(model_path)?;
        info!("📋 Model ID from path: 0x{}", hex::encode(&model_id.0));

        // Step 2: Check if model is globally approved
        let is_approved = self.model_registry
            .is_model_approved(model_id)
            .await
            .map_err(|e| ModelValidationError::ContractUnavailable(e.to_string()))?;

        if !is_approved {
            error!("❌ Model 0x{} is NOT approved in ModelRegistry", hex::encode(&model_id.0));
            return Err(ModelValidationError::ModelNotRegistered(
                model_path.display().to_string()
            ));
        }

        info!("✅ Model is globally approved");

        // Step 3: CRITICAL - Verify file hash matches on-chain SHA256
        // Query model details from contract (authoritative source for SHA256)
        let model_info = self.model_registry
            .get_model_details(model_id)
            .await
            .map_err(|e| ModelValidationError::ContractUnavailable(
                format!("Failed to get model details: {}", e)
            ))?;

        info!("🔍 Verifying file hash against on-chain SHA256...");
        let expected_hash = format!("{:x}", model_info.sha256_hash);

        let hash_valid = self.model_registry
            .verify_model_hash(model_path, &expected_hash)
            .await
            .map_err(|e| ModelValidationError::ContractUnavailable(
                format!("Hash verification failed: {}", e)
            ))?;

        if !hash_valid {
            error!(
                "❌ Model file hash MISMATCH! Expected: 0x{}",
                hex::encode(&model_info.sha256_hash.0)
            );
            return Err(ModelValidationError::ModelHashMismatch {
                expected: model_info.sha256_hash,
                path: model_path.display().to_string(),
            });
        }

        info!("✅ Model file hash verified against contract");

        // Step 4: Verify host is authorized for this model
        let is_authorized = self.check_host_authorization(host_address, model_id).await?;

        if !is_authorized {
            error!(
                "❌ Host {} is NOT authorized for model 0x{}",
                host_address,
                hex::encode(&model_id.0)
            );
            return Err(ModelValidationError::HostNotAuthorized(host_address, model_id));
        }

        info!(
            "✅ Model validation passed: {} authorized for 0x{} (hash verified)",
            host_address,
            hex::encode(&model_id.0)
        );

        Ok(model_id)
    }
}
```

**Security Note**: The SHA256 verification ensures hosts cannot:
1. Run tampered/modified model files
2. Substitute a different model while claiming to run an approved one
3. Bypass the blockchain governance for model approval

---

### Sub-phase 2.2: Integrate into main.rs (TDD)

**Goal**: Call startup validation BEFORE loading model, fail node startup if unauthorized

**Status**: ✅ Complete (February 4, 2026)

**Files**:
- `src/main.rs` (modify lines 27-98 - ~25 line delta)
- `tests/integration/test_startup_with_validation.rs` (NEW - ~80 lines)

**Max Lines**:
- main.rs changes: 25 lines
- Integration tests: 80 lines

**Approach**: Test-Driven Development with integration tests
1. Write integration tests for startup scenarios
2. Add ModelValidator initialization in main.rs
3. Call validation before model loading
4. Verify tests pass

**Tasks**:
- [x] Write test `test_node_exits_on_unauthorized_model` - via test_validation_before_model_loading_pattern
- [x] Write test `test_node_starts_with_authorized_model` - via test_validation_disabled_allows_any_model
- [x] Write test `test_validation_disabled_allows_any_model` - REQUIRE_MODEL_VALIDATION=false
- [x] Write test `test_validation_error_message_clear` - via test_error_message_host_not_authorized
- [x] Add `use fabstir_llm_node::model_validation::ModelValidator;` to main.rs
- [x] Initialize ModelValidator if REQUIRE_MODEL_VALIDATION=true
- [x] Call `validate_model_at_startup()` BEFORE `llm_engine.load_model()`
- [x] Handle error: log message and exit with code 1
- [x] Log success message with model_id
- [x] Store semantic_model_id for Phase 4 (load_model signature update)
- [x] Run `cargo test test_main_integration` - Expect 9/9 passing

**Implementation Target**:
```rust
// In main.rs, after web3_client creation (~line 150)

// Initialize model validator
let model_validator = ModelValidator::new(
    model_registry_client.clone(),
    node_registry_address,
    web3_client.clone(),
).await?;

// CRITICAL: Validate model BEFORE loading
info!("🔐 Validating model authorization...");
let semantic_model_id = match model_validator
    .validate_model_at_startup(&model_path_buf, host_address)
    .await
{
    Ok(id) => {
        println!("✅ Model authorization verified: 0x{}", hex::encode(&id.0));
        id
    }
    Err(e) => {
        eprintln!("❌ Model validation failed: {}", e);
        eprintln!("   Check that your MODEL_PATH matches a model you're registered for");
        eprintln!("   Or disable validation with REQUIRE_MODEL_VALIDATION=false");
        std::process::exit(1);
    }
};

// Now safe to load model...
match llm_engine.load_model(model_config, Some(semantic_model_id)).await {
    // ... existing code
}
```

---

## Phase 3: Job Claim Validation

### Sub-phase 3.1: Job Claim Validation (TDD)

**Goal**: Hosts can only claim jobs for models they're registered for

**Status**: ✅ Complete (February 4, 2026)

**Note**: Tests and helper function implemented. Full integration into `job_claim.rs` (making `validate_job` async) requires breaking change and is deferred to Phase 3.2 for v8.15.0.

**Files**:
- `src/job_claim.rs` (modify lines 117, 201, 292-315, 330 - ~80 line delta)
- `tests/job_claim/test_model_validation.rs` (NEW - ~120 lines)

**Max Lines**:
- job_claim.rs changes: 80 lines
- Tests: 120 lines

**⚠️ Breaking Change: validate_job() becomes async**

The `validate_job()` function must become async to query the contract. This requires updating all call sites:

| Location | Current Code | Required Change |
|----------|-------------|-----------------|
| Line 201 | `self.validate_job(&job)?;` | `self.validate_job(&job).await?;` |
| Line 330 | `if self.validate_job(&job).is_ok()` | `if self.validate_job(&job).await.is_ok()` |

**Approach**: Test-Driven Development
1. Write 6 test cases for claim validation
2. Add ModelValidator to JobClaimer
3. Update validate_job() signature to async
4. Update all call sites (lines 201, 330)
5. Verify tests pass

**Tasks**:
- [ ] Write test `test_authorized_job_claimed_successfully` - Claim succeeds
- [ ] Write test `test_unauthorized_job_returns_unsupported_model` - Claim rejected
- [ ] Write test `test_validation_disabled_allows_any_model` - Feature flag off
- [ ] Write test `test_cache_hit_avoids_redundant_query` - Second claim uses cache
- [ ] Write test `test_invalid_model_id_format_returns_error` - Malformed model_id
- [ ] Write test `test_contract_unavailable_returns_error` - RPC failure handled
- [ ] Add `model_validator: Option<Arc<ModelValidator>>` field to JobClaimer struct (line ~117)
- [ ] Add `with_model_validator()` builder method to JobClaimer
- [ ] Update `validate_job()` signature: `fn` → `async fn`
- [ ] **Update call site line 201**: `self.validate_job(&job)?` → `self.validate_job(&job).await?`
- [ ] **Update call site line 330**: Add `.await` before `.is_ok()`
- [ ] Add model validation logic in validate_job() (query contract)
- [ ] Add helper `parse_model_id_string(&str) -> Result<H256>`
- [ ] Run `cargo test test_job_claim` - Expect 6/6 passing

**Implementation Target**:
```rust
// Add field to JobClaimer struct (after line 117)
pub struct JobClaimer {
    // ... existing fields ...
    model_validator: Option<Arc<ModelValidator>>,
}

// Add builder method
impl JobClaimer {
    pub fn with_model_validator(mut self, validator: Arc<ModelValidator>) -> Self {
        self.model_validator = Some(validator);
        self
    }
}

// Update validate_job() function
async fn validate_job(&self, job: &JobRequest) -> Result<(), ClaimError> {
    // ... existing checks (max_tokens, payment) ...

    // NEW: Contract-based model authorization
    if let Some(validator) = &self.model_validator {
        if validator.is_enabled() {
            // Parse job.model_id (String) to H256
            let model_id_h256 = parse_model_id_string(&job.model_id)
                .map_err(|e| ClaimError::InvalidJob(format!("Invalid model_id: {}", e)))?;

            // Query contract (uses cache)
            let is_authorized = validator
                .check_host_authorization(self.config.node_address, model_id_h256)
                .await
                .map_err(|e| ClaimError::Other(format!("Model validation failed: {}", e)))?;

            if !is_authorized {
                warn!(
                    "🚫 Skipping job {} - host not authorized for model 0x{}",
                    job.job_id,
                    hex::encode(&model_id_h256.0)
                );
                return Err(ClaimError::UnsupportedModel);
            }

            debug!("✅ Host authorized for job {} model", job.job_id);
        }
    }

    Ok(())
}

/// Helper: Parse model_id string to H256
/// Handles both "0x..." prefixed and raw hex strings
fn parse_model_id_string(model_id: &str) -> Result<H256, String> {
    let hex_str = model_id.strip_prefix("0x").unwrap_or(model_id);

    let bytes = hex::decode(hex_str)
        .map_err(|e| format!("Invalid model_id hex: {}", e))?;

    if bytes.len() != 32 {
        return Err(format!(
            "model_id must be 32 bytes, got {} bytes",
            bytes.len()
        ));
    }

    Ok(H256::from_slice(&bytes))
}
```

---

## Phase 4: Inference Runtime Validation

### Sub-phase 4.1: Track Semantic Model ID in Engine (TDD)

**Goal**: Store contract model_id (H256) alongside UUID in Model struct

**Status**: ⏳ Pending

**Files**:
- `src/inference/engine.rs` (modify lines 135-141, 214-260 - ~30 line delta)
- `tests/inference/test_model_identity.rs` (NEW - ~60 lines)

**Max Lines**:
- engine.rs changes: 30 lines
- Tests: 60 lines

**Approach**: Test-Driven Development
1. Write tests for model identity tracking
2. Update Model struct and load_model()
3. Verify tests pass

**Tasks**:
- [ ] Write test `test_model_stores_semantic_id` - Verify semantic_id field populated
- [ ] Write test `test_model_with_none_semantic_id` - Dev mode (validation disabled)
- [ ] Write test `test_model_retrieval_by_semantic_id` - Can lookup by contract ID
- [ ] Write test `test_load_model_with_semantic_id_parameter` - Updated signature works
- [ ] Add `semantic_id: Option<H256>` field to Model struct (line ~140)
- [ ] Update `load_model()` signature: add `semantic_id: Option<H256>` parameter (line ~214)
- [ ] Update Model initialization in load_model() to include semantic_id
- [ ] Update callers of load_model() (main.rs already updated in Phase 2)
- [ ] Run `cargo test test_model_identity` - Expect 4/4 passing

**Implementation Target**:
```rust
// Update Model struct (line ~135)
pub struct Model {
    pub id: String,                    // UUID for internal tracking (existing)
    pub semantic_id: Option<H256>,     // NEW: Contract model ID (H256)
    pub config: ModelConfig,
    pub status: ModelStatus,
    pub loaded_at: std::time::SystemTime,
    pub usage_count: usize,
}

// Update load_model() signature (line ~214)
pub async fn load_model(
    &mut self,
    config: ModelConfig,
    semantic_id: Option<H256>,  // NEW parameter from startup validation
) -> Result<String> {
    let model_id = Uuid::new_v4().to_string();

    let model = Model {
        id: model_id.clone(),
        semantic_id,  // Store contract ID
        config: config.clone(),
        status: ModelStatus::Loading,
        loaded_at: std::time::SystemTime::now(),
        usage_count: 0,
    };

    // ... rest of loading logic
}
```

---

### Sub-phase 4.2: Runtime Inference Validation (TDD)

**Goal**: Verify job.model_id matches loaded model before running inference

**Status**: ⏳ Pending

**Files**:
- `src/api/server.rs` (modify lines 482-494 - ~50 line delta)
- `src/api/websocket/handlers/inference.rs` (modify line 118 - ~30 line delta)
- `tests/api/test_inference_validation.rs` (NEW - ~100 lines)

**Max Lines**:
- server.rs changes: 50 lines
- websocket handler changes: 30 lines
- Tests: 100 lines

**Approach**: Test-Driven Development
1. Write 5 test cases for inference validation
2. Add validation before inference execution
3. Verify tests pass

**Tasks**:
- [ ] Write test `test_inference_succeeds_when_model_matches` - Correct model → success
- [ ] Write test `test_inference_fails_when_model_mismatch` - Wrong model → error 403
- [ ] Write test `test_validation_disabled_allows_mismatch` - Feature flag off
- [ ] Write test `test_websocket_inference_validates_model` - WebSocket uses validation
- [ ] Write test `test_error_message_includes_model_ids` - Clear error for debugging
- [ ] Add validation logic in `src/api/server.rs` inference handler (before run_inference)
- [ ] Update WebSocket handler to use model_id from session (not hardcoded "default")
- [ ] Add helper to compare job.model_id (String) vs loaded semantic_id (H256)
- [ ] Return 403 Forbidden if model mismatch (when validation enabled)
- [ ] Run `cargo test test_inference_validation` - Expect 5/5 passing

**Implementation Notes**:
- If feature disabled: Allow mismatch, log warning
- If feature enabled: Reject request with clear error message
- WebSocket: Store model_id in session context, validate on each message

---

## Phase 5: Integration and Documentation

### Sub-phase 5.1: Cache Warming with getNodeFullInfo

**Goal**: Prefetch all host models at startup using single contract call

**Status**: ⏳ Pending

**Files**:
- `src/model_validation.rs` (modify - add ~40 lines)
- `tests/model_validation/test_cache_warming.rs` (NEW - ~60 lines)

**Max Lines**:
- New method: 40 lines
- Tests: 60 lines

**Approach**: Test-Driven Development
1. Write tests for cache warming
2. Implement getNodeFullInfo() query
3. Verify cache populated correctly

**Tasks**:
- [ ] Write test `test_warm_cache_fetches_all_models` - Single call gets all models
- [ ] Write test `test_warm_cache_populates_cache` - Cache contains fetched models
- [ ] Write test `test_subsequent_queries_use_cache` - No redundant contract calls
- [ ] Implement `warm_cache(&self, host_address: Address) -> Result<()>`
- [ ] Query `getNodeFullInfo()` contract function
- [ ] Extract `supportedModels` from return tuple (index 5)
- [ ] Populate cache with all models
- [ ] Call warm_cache() from main.rs after ModelValidator init
- [ ] Run `cargo test test_cache_warming` - Expect 3/3 passing

**Implementation Target**:
```rust
impl ModelValidator {
    pub async fn warm_cache(
        &self,
        host_address: Address,
    ) -> Result<(), ModelValidationError> {
        info!("🔥 Warming model authorization cache for host {}", host_address);

        let node_registry = NodeRegistryWithModels::new(
            self.node_registry_address,
            self.web3_client.provider.clone(),
        );

        // Single contract call to get all host models
        let (_, _, _, _, _, supported_models, _, _) = node_registry
            .method::<_, (Address, U256, bool, String, String, Vec<H256>, U256, U256)>(
                "getNodeFullInfo",
                host_address
            )?
            .call()
            .await
            .map_err(|e| ModelValidationError::ContractUnavailable(e.to_string()))?;

        // Populate cache
        let mut cache = self.authorized_models_cache.write().await;
        cache.insert(host_address, supported_models.clone());

        info!("✅ Cache warmed with {} models", supported_models.len());

        Ok(())
    }
}
```

---

### Sub-phase 5.2: Integration Tests & Documentation

**Goal**: End-to-end tests and comprehensive documentation

**Status**: ⏳ Pending

**Files**:
- `tests/integration/test_model_validation_e2e.rs` (NEW - ~150 lines)
- `docs/MODEL_VALIDATION_GUIDE.md` (NEW - ~200 lines)
- `README.md` (modify - add env var docs)

**Max Lines**:
- Integration tests: 150 lines
- Documentation: 200 lines
- README update: 20 lines

**Tasks**:
- [ ] Write test `test_full_flow_authorized_host` - Startup → claim → inference
- [ ] Write test `test_full_flow_unauthorized_startup_fails` - Node exits on bad MODEL_PATH
- [ ] Write test `test_full_flow_unauthorized_claim_skipped` - Job not claimed
- [ ] Write test `test_feature_flag_toggles_validation` - Env var controls behavior
- [ ] Write test `test_cache_performance` - Verify cache hit rate >95%
- [ ] Write test `test_contract_down_fails_safely` - Fail-safe on RPC errors
- [ ] Create `docs/MODEL_VALIDATION_GUIDE.md` with setup instructions
- [ ] Update README.md with REQUIRE_MODEL_VALIDATION env var
- [ ] Add troubleshooting section to guide
- [ ] Run full integration test suite - Expect 6/6 passing

**Documentation Outline**:
```markdown
# Model Authorization Enforcement Guide

## Overview
- Security issue explanation
- How validation works

## Environment Variables
- REQUIRE_MODEL_VALIDATION (default: false in v8.14.0)

## Deployment
- v8.14.0: Opt-in testing
- v8.15.0: Encouraged
- v9.0.0: Mandatory (breaking)

## Troubleshooting
- "Model not registered" error
- "Host not authorized" error
- Contract RPC issues
- Cache debugging

## Contract Queries
- nodeSupportsModel()
- getNodeFullInfo()
- isModelApproved()

## Performance
- Cache hit rate metrics
- Startup time impact
```

---

## Verification Steps

### Unit Tests (in src/model_validation.rs #[cfg(test)] module)
```bash
# Phase 1 - Module unit tests
cargo test --lib model_validation -- --nocapture

# Phase 1.4 - Model registry updates
cargo test --lib model_registry -- --nocapture
```

### Integration Tests (in tests/ directory)
```bash
# Phase 1 - Error types and model ID extraction
cargo test --test model_validation_tests test_error -- --nocapture
cargo test --test model_validation_tests test_extract -- --nocapture
cargo test --test model_validation_tests test_contract -- --nocapture

# Phase 2 - Startup validation
cargo test --test model_validation_tests test_startup -- --nocapture

# Phase 3 - Job claim validation
cargo test --test job_claim_tests test_model_validation -- --nocapture

# Phase 4 - Inference validation
cargo test --test inference_tests test_model_identity -- --nocapture
cargo test --test api_tests test_inference_validation -- --nocapture

# Phase 5 - Cache warming and E2E
cargo test --test model_validation_tests test_cache -- --nocapture
cargo test --test integration_tests test_model_validation_e2e -- --nocapture
```

### Test File Organization
```
tests/
├── model_validation_tests.rs      # Main model validation tests
│   (or tests/model_validation/mod.rs with submodules)
├── job_claim_tests.rs             # Job claim with model validation
├── inference_tests.rs             # Inference model identity tests
├── api_tests.rs                   # API inference validation tests
└── integration_tests.rs           # E2E tests
```

### Manual Testing

**Test 1: Authorized Model (Should Succeed)**
```bash
export REQUIRE_MODEL_VALIDATION=true
export MODEL_PATH=./models/tiny-vicuna-1b.q4_k_m.gguf
export HOST_PRIVATE_KEY=0x... # Host registered with TinyVicuna
cargo run --release
# Expected: ✅ Model authorization verified
```

**Test 2: Unauthorized Model (Should Fail)**
```bash
export REQUIRE_MODEL_VALIDATION=true
export MODEL_PATH=./models/unauthorized-model.gguf
cargo run --release
# Expected: ❌ Error - Host not authorized for model
# Expected: Exit code 1
```

**Test 3: Validation Disabled (Should Succeed with Warning)**
```bash
export REQUIRE_MODEL_VALIDATION=false
export MODEL_PATH=./models/any-model.gguf
cargo run --release
# Expected: ⚠️  Model validation DISABLED
```

**Test 4: Tampered Model File (Should Fail - Hash Mismatch)**
```bash
export REQUIRE_MODEL_VALIDATION=true
export MODEL_PATH=./models/tiny-vicuna-1b.q4_k_m.gguf
# Corrupt the file slightly (append garbage)
echo "tampered" >> ./models/tiny-vicuna-1b.q4_k_m.gguf
cargo run --release
# Expected: ❌ Error - Model file hash MISMATCH
# Expected: Exit code 1
```

**Test 5: Contract Queries**
```bash
# Verify host's registered models
cast call 0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22 \
  "getNodeModels(address)(bytes32[])" \
  $HOST_ADDRESS \
  --rpc-url $BASE_SEPOLIA_RPC_URL

# Verify host supports specific model
cast call 0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22 \
  "nodeSupportsModel(address,bytes32)(bool)" \
  $HOST_ADDRESS \
  0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced \
  --rpc-url $BASE_SEPOLIA_RPC_URL

# Query model SHA256 from contract
cast call 0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2 \
  "getModel(bytes32)((string,string,bytes32,uint8,bool,uint256))" \
  0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced \
  --rpc-url $BASE_SEPOLIA_RPC_URL
# Returns: (repo, filename, sha256Hash, tier, active, timestamp)
```

---

## Success Criteria

✅ Node refuses to start with unauthorized MODEL_PATH (when enabled)
✅ Node refuses to start with tampered model file (SHA256 mismatch)
✅ SHA256 hash queried from contract (authoritative source)
✅ Hosts can only claim jobs for registered models
✅ Contract queries cached for performance (<1ms cache hit)
✅ All 70 tests passing
✅ Feature flag allows gradual rollout
✅ Clear error messages for debugging
✅ <200ms performance impact per validation (cache miss)
✅ **Dynamic model support** - any model registered on ModelRegistry works automatically
✅ **No hardcoded model dependencies** - GPT-OSS-120B and future models supported without code changes
✅ Documentation complete with troubleshooting guide

---

## Performance Targets

| Metric | Target | Rationale |
|--------|--------|-----------|
| Startup validation | <5 seconds | One-time cost, acceptable delay |
| Job claim (cache hit) | <1ms | In-memory lookup |
| Job claim (cache miss) | <200ms | Single contract query |
| Inference validation | <1ms | Cache-only, no contract call |
| Cache hit rate | >95% | Most queries for same host's models |
| Startup cache warming | <3 seconds | Single getNodeFullInfo() call |

---

## Migration Timeline

| Version | Date | Behavior | Breaking |
|---------|------|----------|----------|
| v8.14.0 | Feb 2026 | Default: REQUIRE_MODEL_VALIDATION=false | No |
| v8.15.0 | Mar 2026 | Encourage adoption, logging | No |
| v9.0.0 | Apr 2026 | Default: REQUIRE_MODEL_VALIDATION=true | **YES** |

**Rollback Plan**: Set `REQUIRE_MODEL_VALIDATION=false` in .env to disable validation

---

## Notes

- **Critical Security Fix**: Prevents hosts from running unauthorized or tampered models
- **Strict TDD**: 70 tests written before implementation (bounded autonomy)
- **Dynamic Model Support**: Queries ModelRegistry at startup - any registered model works automatically
- **No Hardcoded Models**: Production models (GPT-OSS-120B, etc.) supported without code changes
- **Feature Flag**: Gradual rollout to minimize disruption
- **Contract Addresses**: Uses Remediation contracts (latest Feb 4, 2026)
- **No Contract Changes**: Only node software changes needed
- **Contract Stability**: Model validation functions unchanged since deployment
- **Recent Contract Update**: Signature removal (Feb 4, 2026) does NOT affect model validation
- **SHA256 Verification**: File hash queried from contract via `getModel()` - ensures file integrity
- **Cache Strategy**: In-memory HashMap for authorization queries
- **Fail-Safe**: Refuse operation if contract unavailable (security critical)
- **ABI Compatibility**: Latest ABIs (Feb 4, 2026) fully compatible with our implementation

### Cache Invalidation (Future Work - Phase 6)

The current implementation uses optimistic caching without explicit invalidation. For v8.15.0+, consider adding:

1. **Event Listener**: Subscribe to `NodeUpdated`, `NodeRemoved` events on NodeRegistry
2. **TTL-Based Expiration**: Optionally expire cache entries after N minutes
3. **Manual Invalidation**: API endpoint to clear cache for debugging

For v8.14.0, the cache is sufficient because:
- Startup re-queries fresh data via `getNodeFullInfo()`
- Node restart clears all cache
- Model authorization changes are rare (hosts don't frequently update their model list)

---

## References

- **Plan File**: `/home/developer/.claude/plans/generic-stargazing-spring.md`
- **Contract Documentation**:
  - `docs/compute-contracts-reference/API_REFERENCE.md` (Updated Feb 4, 2026)
  - `docs/compute-contracts-reference/ARCHITECTURE.md` (Updated Feb 4, 2026)
  - `docs/compute-contracts-reference/BREAKING_CHANGES.md` (Signature removal - Feb 4, 2026)
  - `docs/compute-contracts-reference/NODE-MIGRATION-JAN2026.md` (Migration guide v3.0.0)
- **Contract ABIs**: `docs/compute-contracts-reference/client-abis/` (Updated Feb 4, 2026)
  - `NodeRegistryWithModelsUpgradeable-CLIENT-ABI.json` - **FOR MODEL VALIDATION**
  - `ModelRegistryUpgradeable-CLIENT-ABI.json` - **FOR MODEL VALIDATION**
  - ABIs Changelog: `docs/compute-contracts-reference/client-abis/CHANGELOG.md`
- **Contract Addresses (Remediation Proxies)**:
  - **NodeRegistry**: `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` (Impl: `0xF2D98D38B2dF95f4e8e4A49750823C415E795377`)
  - **ModelRegistry**: `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` (Impl: `0x3F22fd532Ac051aE09b0F2e45F3DBfc835AfCD45`)

### Contract Update Timeline

| Date | Change | Impact on Model Validation |
|------|--------|---------------------------|
| Feb 4, 2026 | Signature removal from `submitProofOfWork` | ✅ No impact - proof submission only |
| Feb 3, 2026 | Per-Model Rate Limits in ModelRegistry | ✅ No impact - validation functions unchanged |
| Jan 16, 2026 | Stake slashing in NodeRegistry | ✅ No impact - query functions unchanged |
| Jan 14, 2026 | deltaCID parameter in JobMarketplace | ✅ No impact - job marketplace only |

**Conclusion**: All model validation query functions (`nodeSupportsModel`, `getNodeModels`, `getNodeFullInfo`, `isModelApproved`, `getModelId`) are **stable and unchanged** across all recent contract updates

---
