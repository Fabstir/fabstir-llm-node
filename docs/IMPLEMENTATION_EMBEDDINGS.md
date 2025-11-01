# Host-Side Embedding Implementation Plan (Sub-phase 4.2)

## Overview

This implementation plan adds a production `/v1/embed` endpoint to fabstir-llm-node, enabling **zero-cost, host-side text embeddings** for SDK clients. This replaces expensive external API dependencies (OpenAI, Cohere) with self-hosted ONNX-based sentence transformers, following strict TDD (Test-Driven Development) with bounded autonomy.

## Core Requirements

- **Endpoint**: `POST /v1/embed` - Generate 384-dimensional embeddings
- **Model**: all-MiniLM-L6-v2 ONNX (90 MB, 384 dimensions)
- **Batch Size**: 1-96 texts per request
- **Performance**: <100ms per embedding (CPU), <50ms (GPU optional)
- **Cost**: $0.00 (zero cost to users)
- **Multi-Chain**: Support chain_id parameter (Base Sepolia 84532, opBNB 5611)
- **Compatibility**: Integrates with existing Axum HTTP server
- **Vector DB Requirement**: Must output exactly 384 dimensions

## Architecture Integration

### Integration with Existing Infrastructure

```
┌──────────────────────────────────────────────────────────┐
│  Existing Axum HTTP Server (port 8080)                   │
│  ┌────────────────────────────────────────────────────┐  │
│  │  POST /v1/inference    (LLM text generation)       │  │
│  │  • Model: llama-3, tinyllama                       │  │
│  │  • llama-cpp-2 with CUDA                           │  │
│  │  • WebSocket + HTTP support                        │  │
│  └────────────────────────────────────────────────────┘  │
│  ┌────────────────────────────────────────────────────┐  │
│  │  POST /v1/embed  ← NEW (this implementation)      │  │
│  │  • Model: all-MiniLM-L6-v2 (384-dim)             │  │
│  │  • ONNX Runtime (ort crate)                       │  │
│  │  • HTTP only (no streaming needed)                │  │
│  │  • Multi-model support                            │  │
│  └────────────────────────────────────────────────────┘  │
│                                                           │
│  Shared Infrastructure:                                   │
│  • AppState for model management                         │
│  • ApiError for error handling                           │
│  • ChainRegistry for multi-chain support                 │
│  • Rate limiting middleware                               │
│  • Prometheus metrics                                     │
└──────────────────────────────────────────────────────────┘
```

### Key Design Decisions

1. **384-Dimension Hard Requirement**
   - Vector DB requires exactly 384 dimensions
   - Runtime validation with DimensionMismatch error
   - Allows any model that outputs 384-dim (future-proof)

2. **Multi-Model Manager Pattern**
   - Load multiple embedding models on startup
   - Clients specify model via `model` parameter
   - Default model: all-MiniLM-L6-v2
   - Discovery via `GET /v1/models?type=embedding`

3. **Separate from Inference Pipeline**
   - Embeddings use ONNX Runtime (`ort` crate)
   - LLM inference uses llama-cpp-2
   - No CUDA conflicts (ONNX uses CPU by default)

## Phase 1: Dependencies and Module Structure

### Sub-phase 1.1: Add Dependencies ✅
**Goal**: Add ONNX Runtime and tokenization dependencies without breaking existing build

**Tasks**:
- [x] Add `ort = { version = "2.0.0-rc.10", features = ["download-binaries"] }` to Cargo.toml
- [x] Add `tokenizers = "0.20"` to Cargo.toml
- [x] Add `ndarray = "0.16"` to Cargo.toml
- [x] hf-hub already present at v0.3 (no update needed)
- [x] Run `cargo check --all-features` to verify no dependency conflicts with llama-cpp-2
- [x] Run existing tests to verify no regressions: `cargo test --lib` (212/214 passed, 2 pre-existing failures)

**Test Files** (TDD - Written First):
- `tests/dependencies_test.rs` - Verify ONNX runtime loads (5 tests)
  - test_ort_available() ✅
  - test_tokenizers_available() ✅
  - test_ndarray_available() ✅
  - test_no_llama_conflicts() ✅
  - test_dependency_versions_documented() ✅

**Success Criteria**:
- [x] Cargo build succeeds with new dependencies
- [x] All existing tests still pass (212/214 tests pass, 2 pre-existing failures unrelated to our changes)
- [x] ONNX Runtime compiles successfully (ort v2.0.0-rc.10)
- [x] No CUDA conflicts between ort and llama-cpp-2

**Deliverables**:
- ✅ Updated `Cargo.toml` with 3 new dependencies (lines 47-52)
- ✅ 5 passing dependency verification tests
- ✅ Clean dependency tree (no conflicts)

**Actual Time**: 1 hour

**Notes**:
- Used ort v2.0.0-rc.10 (latest RC, production-ready)
- tokenizers v0.20.4 (latest stable)
- ndarray v0.16.1 (latest stable)
- No dependency conflicts found via `cargo tree -d`
- All dependencies compile cleanly

---

### Sub-phase 1.2: Create Module Structure ✅
**Goal**: Create embedding module structure following existing patterns

**Tasks**:
- [x] Create `src/api/embed/mod.rs` module
- [x] Create `src/api/embed/request.rs` for EmbedRequest type
- [x] Create `src/api/embed/response.rs` for EmbedResponse type
- [x] Create `src/api/embed/handler.rs` for HTTP handler (stub)
- [x] Create `src/embeddings/onnx_model.rs` for ONNX model wrapper (stub)
- [x] Create `src/embeddings/model_manager.rs` for multi-model management (stub)
- [x] Update `src/api/mod.rs` to export embed module
- [x] Update `src/embeddings/mod.rs` to export ONNX types
- [x] Run `cargo test --test api_tests test_embed` (8/8 tests pass)

**Test Files** (TDD - Written First):
- `tests/api/test_embed_module.rs` - Module structure tests (8 tests)
  - test_embed_module_exists() ✅
  - test_request_response_types_exported() ✅
  - test_handler_accessible() ✅
  - test_request_deserialization() ✅
  - test_request_defaults() ✅
  - test_response_serialization() ✅
  - test_embedding_module_types_accessible() ✅
  - test_embedding_result_structure() ✅

**Success Criteria**:
- [x] All modules compile without errors
- [x] Module structure follows existing API patterns
- [x] Types are properly exported
- [x] No circular dependencies

**Deliverables**:
- ✅ 6 new module files created (`src/api/embed/{mod,request,response,handler}.rs`, `src/embeddings/{onnx_model,model_manager}.rs`)
- ✅ Module structure matches existing API patterns (copied from websocket module structure)
- ✅ 8/8 passing module structure tests
- ✅ Request/Response types with serde defaults and camelCase serialization
- ✅ Stub implementations with TODO comments for future sub-phases

**Actual Time**: 1 hour

**Notes**:
- Followed strict TDD: Created test file FIRST (`tests/api/test_embed_module.rs`), then implementation
- Created stub implementations with extensive TODO comments for Phases 3-4
- Request type includes serde defaults: model="all-MiniLM-L6-v2", chain_id=84532
- Response type uses camelCase serialization (tokenCount, totalTokens) for API consistency
- Handler stub returns zero embeddings for now (will implement in Phase 4)
- ONNX model and manager are stubs (will implement in Phase 3)
- All 8 TDD tests pass on first run after implementation

---

## Phase 2: Request/Response Types

### Sub-phase 2.1: Define Request Type ⏳
**Goal**: Create EmbedRequest struct with validation following ApiError patterns

**Tasks**:
- [ ] Define `EmbedRequest` struct in `src/api/embed/request.rs`
  - [ ] `texts: Vec<String>` field (1-96 items)
  - [ ] `model: Option<String>` field (defaults to "all-MiniLM-L6-v2")
  - [ ] `chain_id: Option<u64>` field (defaults to 84532)
- [ ] Implement `Default` trait for optional fields
- [ ] Implement `validate()` method with clear error messages
  - [ ] Validate texts count (1-96)
  - [ ] Validate each text length (1-8192 characters)
  - [ ] Validate chain_id (84532 or 5611)
  - [ ] Validate model name (if specified)
- [ ] Implement `From<ApiError>` conversions
- [ ] Add serde derives for JSON serialization

**Test Files** (TDD - Write First):
- `tests/api/test_embed_request.rs` - 12 test cases
  - test_valid_request_single_text()
  - test_valid_request_batch()
  - test_default_model_applied()
  - test_default_chain_id_applied()
  - test_empty_texts_rejected()
  - test_too_many_texts_rejected() (>96)
  - test_text_too_long_rejected() (>8192 chars)
  - test_invalid_chain_id_rejected()
  - test_whitespace_only_text_rejected()
  - test_json_serialization()
  - test_json_deserialization()
  - test_validation_error_messages_clear()

**Success Criteria**:
- [ ] All 12 validation tests pass
- [ ] Error messages are clear and actionable
- [ ] JSON serialization works correctly
- [ ] Follows existing ApiError patterns

**Deliverables**:
- `src/api/embed/request.rs` (~200 lines)
- 12 passing TDD tests
- Clear validation error messages

**Estimated Time**: 2 hours

---

### Sub-phase 2.2: Define Response Type ⏳
**Goal**: Create EmbedResponse struct following multi-chain patterns

**Tasks**:
- [ ] Define `EmbeddingResult` struct in `src/api/embed/response.rs`
  - [ ] `embedding: Vec<f32>` field (384 floats)
  - [ ] `text: String` field (original input)
  - [ ] `token_count: u32` field
- [ ] Define `EmbedResponse` struct
  - [ ] `embeddings: Vec<EmbeddingResult>` field
  - [ ] `model: String` field
  - [ ] `provider: String` field (always "host")
  - [ ] `total_tokens: u32` field
  - [ ] `cost: f64` field (always 0.0)
  - [ ] `chain_id: u64` field
  - [ ] `chain_name: String` field
  - [ ] `native_token: String` field
- [ ] Implement `add_chain_context()` helper method
- [ ] Add serde derives with camelCase rename (tokenCount, totalTokens)
- [ ] Implement `From<Vec<EmbeddingResult>>` for builder pattern

**Test Files** (TDD - Write First):
- `tests/api/test_embed_response.rs` - 8 test cases
  - test_response_structure()
  - test_embedding_result_structure()
  - test_chain_context_included()
  - test_token_count_aggregation()
  - test_cost_always_zero()
  - test_provider_always_host()
  - test_json_serialization_camelcase()
  - test_embedding_vector_length_384()

**Success Criteria**:
- [ ] All 8 structure tests pass
- [ ] JSON uses camelCase (tokenCount, not token_count)
- [ ] Chain context matches existing patterns
- [ ] Cost field always 0.0 for host embeddings

**Deliverables**:
- `src/api/embed/response.rs` (~150 lines)
- 8 passing TDD tests
- Consistent with existing API response patterns

**Estimated Time**: 2 hours

---

## Phase 3: ONNX Model Infrastructure

### Sub-phase 3.1: ONNX Model Wrapper ⏳
**Goal**: Implement single embedding model using ONNX Runtime

**Tasks**:
- [ ] Create `OnnxEmbeddingModel` struct in `src/embeddings/onnx_model.rs`
  - [ ] `session: Arc<ort::Session>` field
  - [ ] `tokenizer: Arc<tokenizers::Tokenizer>` field
  - [ ] `dimensions: usize` field (must be 384)
  - [ ] `max_length: usize` field (256 for MiniLM)
- [ ] Implement `new()` async constructor
  - [ ] Load ONNX model from file path
  - [ ] Load tokenizer from file path
  - [ ] Validate model outputs correct dimensions
  - [ ] Configure ONNX optimization level (Level3)
  - [ ] Set thread count (4 threads)
- [ ] Implement `embed_single()` method
  - [ ] Tokenize input text
  - [ ] Create ONNX input tensors (input_ids, attention_mask)
  - [ ] Run inference via ort::Session
  - [ ] Extract embeddings from output tensor
  - [ ] Apply mean pooling over sequence dimension
  - [ ] Return 384-dimensional vector
- [ ] Implement `embed_batch()` method for batch processing
  - [ ] Tokenize all texts together
  - [ ] Create batched tensors
  - [ ] Single ONNX inference call
  - [ ] Extract all embeddings
- [ ] Implement `count_tokens()` method
  - [ ] Use tokenizer to count tokens
  - [ ] Return u32 count
- [ ] Add error handling for model loading failures
- [ ] Add logging for model operations (without logging embeddings)

**Test Files** (TDD - Write First):
- `tests/embeddings/test_onnx_model.rs` - 10 test cases
  - test_model_loads_successfully()
  - test_model_validates_dimensions()
  - test_embed_single_returns_384_dims()
  - test_embed_batch_returns_correct_count()
  - test_embeddings_are_deterministic()
  - test_different_texts_different_embeddings()
  - test_token_counting()
  - test_empty_text_handling()
  - test_long_text_truncation()
  - test_invalid_model_path_error()

**Success Criteria**:
- [ ] All 10 model tests pass
- [ ] Embeddings are deterministic (same input → same output)
- [ ] Model validates 384 dimensions at load time
- [ ] Batch processing faster than sequential
- [ ] Clear error messages for model loading failures

**Deliverables**:
- `src/embeddings/onnx_model.rs` (~400 lines)
- 10 passing TDD tests
- ONNX Runtime integration working

**Estimated Time**: 4 hours

**Note**: For initial testing, download pre-converted ONNX model:
```bash
# Download all-MiniLM-L6-v2 ONNX model
mkdir -p models/all-MiniLM-L6-v2-onnx
cd models/all-MiniLM-L6-v2-onnx
wget https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx
wget https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json
```

---

### Sub-phase 3.2: Multi-Model Manager ⏳
**Goal**: Manage multiple embedding models with default selection

**Tasks**:
- [ ] Create `EmbeddingModelManager` struct in `src/embeddings/model_manager.rs`
  - [ ] `models: HashMap<String, Arc<OnnxEmbeddingModel>>` field
  - [ ] `default_model: String` field
- [ ] Create `EmbeddingModelConfig` struct
  - [ ] `name: String`
  - [ ] `model_path: String`
  - [ ] `tokenizer_path: String`
  - [ ] `dimensions: usize` (must be 384)
- [ ] Implement `new()` async constructor
  - [ ] Accept `Vec<EmbeddingModelConfig>`
  - [ ] Load all models in parallel using tokio::spawn
  - [ ] Log success/failure for each model
  - [ ] Continue if some models fail to load
  - [ ] Error if NO models load successfully
- [ ] Implement `get_model()` method
  - [ ] Accept optional model name
  - [ ] Return default if name not specified
  - [ ] Return error if model not found
- [ ] Implement `list_models()` method
  - [ ] Return Vec<ModelInfo> with name, dimensions, available status
- [ ] Implement `default_model_name()` getter
- [ ] Add thread-safe Arc<RwLock<>> wrapper

**Test Files** (TDD - Write First):
- `tests/embeddings/test_model_manager.rs` - 9 test cases
  - test_manager_loads_single_model()
  - test_manager_loads_multiple_models()
  - test_get_default_model()
  - test_get_model_by_name()
  - test_get_nonexistent_model_error()
  - test_list_all_models()
  - test_parallel_model_loading()
  - test_partial_load_failure_acceptable()
  - test_all_models_fail_returns_error()

**Success Criteria**:
- [ ] All 9 manager tests pass
- [ ] Multiple models load in parallel
- [ ] Default model selection works
- [ ] Graceful handling of partial failures
- [ ] Thread-safe concurrent access

**Deliverables**:
- `src/embeddings/model_manager.rs` (~350 lines)
- 9 passing TDD tests
- Multi-model support working

**Estimated Time**: 3 hours

---

## Phase 4: HTTP Endpoint Handler

### Sub-phase 4.1: Handler Implementation ⏳
**Goal**: Implement POST /v1/embed HTTP handler

**Tasks**:
- [ ] Create `embed_handler()` in `src/api/embed/handler.rs`
  - [ ] Accept `State<AppState>` and `Json<EmbedRequest>`
  - [ ] Return `Result<Json<EmbedResponse>, ApiError>`
- [ ] Implement handler logic:
  - [ ] Extract and validate request using `request.validate()`
  - [ ] Get chain context from ChainRegistry
  - [ ] Validate chain_id (84532 or 5611)
  - [ ] Get embedding model manager from AppState
  - [ ] Select model (default or specified)
  - [ ] Validate model dimensions == 384
  - [ ] Call `model.embed_batch(&request.texts)`
  - [ ] Count tokens for each text
  - [ ] Build EmbedResponse with results
  - [ ] Add chain context (chain_id, chain_name, native_token)
  - [ ] Return JSON response
- [ ] Implement error handling:
  - [ ] Empty texts → ApiError::ValidationError
  - [ ] Too many texts (>96) → ApiError::ValidationError
  - [ ] Text too long (>8192) → ApiError::ValidationError
  - [ ] Invalid chain_id → ApiError::InvalidRequest
  - [ ] Model not found → ApiError::ModelNotFound
  - [ ] Dimension mismatch → ApiError::ValidationError
  - [ ] Model not loaded → ApiError::ServiceUnavailable
  - [ ] Inference error → ApiError::InternalError
- [ ] Add logging:
  - [ ] Log request received (texts count, model, chain_id)
  - [ ] Log processing time
  - [ ] Log success (tokens processed)
  - [ ] Log errors (without logging text content)

**Test Files** (TDD - Write First):
- `tests/api/test_embed_handler.rs` - 15 test cases
  - test_handler_single_text_success()
  - test_handler_batch_success()
  - test_handler_default_model_applied()
  - test_handler_custom_model_specified()
  - test_handler_chain_context_added()
  - test_handler_empty_texts_error()
  - test_handler_too_many_texts_error()
  - test_handler_text_too_long_error()
  - test_handler_invalid_chain_error()
  - test_handler_model_not_found_error()
  - test_handler_dimension_mismatch_error()
  - test_handler_model_not_loaded_error()
  - test_handler_token_counting_accurate()
  - test_handler_cost_always_zero()
  - test_handler_processing_time_logged()

**Success Criteria**:
- [ ] All 15 handler tests pass
- [ ] Request validation works correctly
- [ ] Chain context added properly
- [ ] Error handling covers all cases
- [ ] Logging provides useful information
- [ ] Processing time < 100ms per embedding

**Deliverables**:
- `src/api/embed/handler.rs` (~300 lines)
- 15 passing TDD tests
- Production-ready handler

**Estimated Time**: 4 hours

---

### Sub-phase 4.2: Route Registration ⏳
**Goal**: Register /v1/embed route in ApiServer

**Tasks**:
- [ ] Add `embedding_model_manager: Arc<RwLock<Option<Arc<EmbeddingModelManager>>>>` to ApiServer struct
- [ ] Update `ApiServer::new()` to accept embedding config
- [ ] Load embedding models during server initialization
- [ ] Add `get_embedding_manager()` getter method
- [ ] Register route in `create_router()`:
  ```rust
  .route("/v1/embed", post(embed_handler))
  ```
- [ ] Update `AppState` to include embedding_model_manager
- [ ] Add embedding model loading to `main.rs`
  - [ ] Read EMBEDDING_MODEL_PATH from environment
  - [ ] Default to "./models/all-MiniLM-L6-v2-onnx"
  - [ ] Log model loading status
  - [ ] Continue startup even if embedding models fail to load

**Test Files** (TDD - Write First):
- `tests/api/test_route_registration.rs` - 6 test cases
  - test_embed_route_registered()
  - test_embed_route_accepts_post()
  - test_embed_route_rejects_get()
  - test_server_starts_with_embeddings()
  - test_server_starts_without_embeddings()
  - test_embedding_manager_accessible()

**Success Criteria**:
- [ ] All 6 route tests pass
- [ ] Route is accessible at POST /v1/embed
- [ ] Server starts successfully with embeddings
- [ ] Server still starts if embeddings fail to load
- [ ] Embedding manager accessible from handlers

**Deliverables**:
- Updated `src/api/server.rs` (~50 lines added)
- Updated `src/main.rs` (~30 lines added)
- 6 passing TDD tests

**Estimated Time**: 2 hours

---

## Phase 5: Model Discovery Endpoint

### Sub-phase 5.1: GET /v1/models?type=embedding ⏳
**Goal**: Allow clients to discover available embedding models

**Tasks**:
- [ ] Create `ModelInfo` struct in `src/api/embed/response.rs`
  - [ ] `name: String`
  - [ ] `dimensions: usize`
  - [ ] `available: bool`
  - [ ] `is_default: bool`
- [ ] Create `ModelsResponse` struct
  - [ ] `models: Vec<ModelInfo>`
  - [ ] `chain_id: u64`
  - [ ] `chain_name: String`
- [ ] Update existing `list_models_handler()` in `src/api/http_server.rs`
  - [ ] Accept query parameter `?type=embedding`
  - [ ] If type=embedding, return embedding models
  - [ ] If type=inference or no param, return inference models
  - [ ] Get embedding models from `state.api_server.get_embedding_manager()`
  - [ ] Call `manager.list_models()`
  - [ ] Add chain context
  - [ ] Return JSON response
- [ ] Handle case where no embedding models loaded
  - [ ] Return empty models array (not an error)

**Test Files** (TDD - Write First):
- `tests/api/test_models_endpoint.rs` - 8 test cases
  - test_list_embedding_models()
  - test_list_inference_models()
  - test_default_model_marked()
  - test_model_dimensions_included()
  - test_model_availability_status()
  - test_no_models_returns_empty_array()
  - test_chain_context_included()
  - test_query_param_type_filtering()

**Success Criteria**:
- [ ] All 8 model listing tests pass
- [ ] Query parameter filtering works
- [ ] Default model is marked correctly
- [ ] Returns empty array if no models loaded
- [ ] Chain context included in response

**Deliverables**:
- Updated `src/api/http_server.rs` (~60 lines added)
- Updated `src/api/embed/response.rs` (~40 lines added)
- 8 passing TDD tests

**Estimated Time**: 2 hours

---

## Phase 6: Integration Testing

### Sub-phase 6.1: End-to-End Integration Tests ⏳
**Goal**: Comprehensive integration tests with real HTTP requests

**Tasks**:
- [ ] Create `tests/integration/test_embed_e2e.rs`
- [ ] Set up test server with real embedding models
- [ ] Test complete request/response cycle
- [ ] Test concurrent requests
- [ ] Test error scenarios
- [ ] Test performance benchmarks

**Test Files** (TDD - Write First):
- `tests/integration/test_embed_e2e.rs` - 14 test cases
  - test_e2e_single_embedding()
  - test_e2e_batch_embedding()
  - test_e2e_default_model()
  - test_e2e_custom_model()
  - test_e2e_model_discovery()
  - test_e2e_chain_context_base_sepolia()
  - test_e2e_chain_context_opbnb()
  - test_e2e_validation_errors()
  - test_e2e_model_not_found()
  - test_e2e_concurrent_requests()
  - test_e2e_large_batch_96_texts()
  - test_e2e_empty_text_rejected()
  - test_e2e_response_format()
  - test_e2e_performance_benchmark()

**Success Criteria**:
- [ ] All 14 E2E tests pass
- [ ] Concurrent requests handled correctly
- [ ] Performance meets targets (<100ms per embedding)
- [ ] All error scenarios covered
- [ ] Response format matches specification

**Deliverables**:
- `tests/integration/test_embed_e2e.rs` (~600 lines)
- 14 passing integration tests
- Performance benchmarks documented

**Estimated Time**: 4 hours

---

### Sub-phase 6.2: Compatibility Testing ⏳
**Goal**: Ensure embedding endpoint doesn't break existing functionality

**Tasks**:
- [ ] Run all existing API tests: `cargo test --test api_tests`
- [ ] Run all existing integration tests: `cargo test --test integration_tests`
- [ ] Run all existing WebSocket tests: `cargo test --test websocket_tests`
- [ ] Verify inference endpoint still works
- [ ] Verify health endpoint still works
- [ ] Verify metrics endpoint still works
- [ ] Test server startup with and without embedding models
- [ ] Test memory usage with both LLM and embedding models loaded

**Test Files** (TDD - Write First):
- `tests/integration/test_compatibility.rs` - 6 test cases
  - test_inference_endpoint_unaffected()
  - test_health_endpoint_works()
  - test_metrics_include_embeddings()
  - test_server_starts_without_embed_models()
  - test_memory_usage_acceptable()
  - test_no_port_conflicts()

**Success Criteria**:
- [ ] All 6 compatibility tests pass
- [ ] All existing tests still pass (no regressions)
- [ ] Memory usage increase is acceptable (<500 MB)
- [ ] Server starts with and without embedding models
- [ ] No port conflicts or resource leaks

**Deliverables**:
- `tests/integration/test_compatibility.rs` (~250 lines)
- 6 passing compatibility tests
- Regression test report (all existing tests pass)

**Estimated Time**: 2 hours

---

## Phase 7: Documentation

### Sub-phase 7.1: API Documentation ⏳
**Goal**: Complete API documentation for embedding endpoint

**Tasks**:
- [ ] Update `docs/API.md` with `/v1/embed` section
  - [ ] Endpoint description
  - [ ] Request format with examples
  - [ ] Response format with examples
  - [ ] Error codes specific to embeddings
  - [ ] cURL examples
  - [ ] TypeScript/SDK examples
- [ ] Document `/v1/models?type=embedding` endpoint
  - [ ] Query parameters
  - [ ] Response format
  - [ ] Example usage
- [ ] Add embedding troubleshooting section
- [ ] Document performance characteristics
- [ ] Add model download instructions

**Deliverables**:
- Updated `docs/API.md` (+~300 lines)
- cURL examples for all endpoints
- TypeScript client examples

**Estimated Time**: 2 hours

---

### Sub-phase 7.2: Deployment Documentation ⏳
**Goal**: Document how to deploy and configure embedding support

**Tasks**:
- [ ] Update `docs/DEPLOYMENT.md`
  - [ ] Add EMBEDDING_MODEL_PATH environment variable
  - [ ] Add model download instructions
  - [ ] Add Docker configuration for embeddings
  - [ ] Add Kubernetes ConfigMap example
  - [ ] Add memory requirements
- [ ] Update `docs/TROUBLESHOOTING.md`
  - [ ] Add Section 10: Embedding Issues
  - [ ] Model loading failures
  - [ ] ONNX Runtime errors
  - [ ] Memory issues
  - [ ] Performance problems
- [ ] Create `docs/sdk-reference/HOST_EMBEDDING_IMPLEMENTATION.md` (already exists, update status)
  - [ ] Mark Rust implementation as COMPLETE
  - [ ] Add deployment notes
  - [ ] Add performance benchmarks

**Deliverables**:
- Updated `docs/DEPLOYMENT.md` (+~200 lines)
- Updated `docs/TROUBLESHOOTING.md` (+~150 lines)
- Updated `docs/sdk-reference/HOST_EMBEDDING_IMPLEMENTATION.md` (status update)

**Estimated Time**: 2 hours

---

## Phase 8: Performance Optimization

### Sub-phase 8.1: Benchmarking and Profiling ⏳
**Goal**: Measure and optimize performance

**Tasks**:
- [ ] Create benchmark suite using criterion
  - [ ] Benchmark single embedding (target: <50ms)
  - [ ] Benchmark batch 10 (target: <200ms)
  - [ ] Benchmark batch 96 (target: <3s)
- [ ] Profile memory usage
  - [ ] Model loading memory
  - [ ] Request processing memory
  - [ ] Concurrent request memory
- [ ] Profile CPU usage
  - [ ] Tokenization overhead
  - [ ] ONNX inference overhead
  - [ ] Mean pooling overhead
- [ ] Identify bottlenecks
- [ ] Optimize hot paths

**Test Files**:
- `benches/embed_benchmark.rs` - Performance benchmarks
  - bench_single_embedding()
  - bench_batch_10_embeddings()
  - bench_batch_96_embeddings()
  - bench_tokenization()
  - bench_inference()
  - bench_concurrent_requests()

**Success Criteria**:
- [ ] Single embedding: <50ms (CPU), <20ms (GPU)
- [ ] Batch 10: <200ms (CPU), <80ms (GPU)
- [ ] Batch 96: <3s (CPU), <1s (GPU)
- [ ] Memory usage: <300 MB (model + overhead)
- [ ] Benchmarks documented

**Deliverables**:
- `benches/embed_benchmark.rs` (~200 lines)
- Performance report with benchmarks
- Optimization recommendations

**Estimated Time**: 3 hours

---

### Sub-phase 8.2: Optional GPU Support ⏳
**Goal**: Add optional GPU acceleration for high-throughput nodes (OPTIONAL)

**Tasks**:
- [ ] Add CUDA execution provider to ONNX Runtime
- [ ] Add feature flag: `features = ["cuda"]` in Cargo.toml
- [ ] Detect GPU availability at runtime
- [ ] Fall back to CPU if GPU unavailable
- [ ] Benchmark GPU vs CPU performance
- [ ] Document GPU requirements

**Note**: This is OPTIONAL and can be skipped for MVP. CPU performance is sufficient for most use cases.

**Success Criteria**:
- [ ] GPU acceleration works when available
- [ ] Automatic fallback to CPU works
- [ ] 10-50x speedup observed on GPU
- [ ] Feature flag allows CPU-only builds

**Deliverables**:
- GPU support code (~100 lines)
- GPU vs CPU benchmarks
- GPU deployment documentation

**Estimated Time**: 3 hours (optional)

---

## Phase 9: Production Readiness

### Sub-phase 9.1: Error Handling Audit ⏳
**Goal**: Ensure all error cases are handled gracefully

**Tasks**:
- [ ] Audit all error paths in embedding code
- [ ] Verify all errors logged with context
- [ ] Verify all errors return appropriate HTTP status codes
- [ ] Test error recovery (retry logic)
- [ ] Test error messages are clear and actionable
- [ ] Test no sensitive data in error messages

**Test Files**:
- `tests/api/test_embed_errors.rs` - Error handling tests
  - test_model_loading_failure_handled()
  - test_onnx_inference_failure_handled()
  - test_tokenization_failure_handled()
  - test_dimension_mismatch_handled()
  - test_memory_allocation_failure_handled()
  - test_concurrent_request_errors_isolated()
  - test_error_messages_clear()
  - test_no_sensitive_data_in_errors()

**Success Criteria**:
- [ ] All 8 error tests pass
- [ ] All error paths tested
- [ ] Error messages are user-friendly
- [ ] No panics in production code
- [ ] Errors logged with proper context

**Deliverables**:
- `tests/api/test_embed_errors.rs` (~300 lines)
- 8 passing error handling tests
- Error handling audit report

**Estimated Time**: 2 hours

---

### Sub-phase 9.2: Security Audit ⏳
**Goal**: Verify security best practices

**Tasks**:
- [ ] Audit input validation (text length, batch size)
- [ ] Verify no code injection vulnerabilities
- [ ] Verify no path traversal vulnerabilities (model loading)
- [ ] Verify rate limiting applied to embedding endpoint
- [ ] Verify embeddings not logged (privacy)
- [ ] Verify memory limits enforced
- [ ] Test malicious input handling
- [ ] Test resource exhaustion attacks

**Test Files**:
- `tests/security/test_embed_security.rs` - Security tests
  - test_input_validation_comprehensive()
  - test_no_code_injection()
  - test_no_path_traversal()
  - test_rate_limiting_applied()
  - test_embeddings_never_logged()
  - test_memory_limits_enforced()
  - test_malicious_input_rejected()
  - test_resource_exhaustion_prevented()

**Success Criteria**:
- [ ] All 8 security tests pass
- [ ] No security vulnerabilities found
- [ ] Input validation comprehensive
- [ ] Rate limiting works correctly
- [ ] Privacy preserved (no embedding logging)

**Deliverables**:
- `tests/security/test_embed_security.rs` (~350 lines)
- 8 passing security tests
- Security audit report

**Estimated Time**: 3 hours

---

### Sub-phase 9.3: Deployment Testing ⏳
**Goal**: Test deployment in production-like environment

**Tasks**:
- [ ] Test Docker deployment with embedding models
- [ ] Test Kubernetes deployment
- [ ] Test systemd service with embeddings
- [ ] Test model auto-download on first start
- [ ] Test graceful degradation (embeddings disabled)
- [ ] Test metrics collection
- [ ] Test log aggregation
- [ ] Load testing with realistic traffic

**Test Files**:
- `tests/deployment/test_docker_embed.sh` - Docker deployment test
- `tests/deployment/test_k8s_embed.sh` - Kubernetes deployment test

**Success Criteria**:
- [ ] Docker deployment works with embeddings
- [ ] Kubernetes deployment works
- [ ] Metrics collected correctly
- [ ] Logs useful for debugging
- [ ] Graceful degradation works
- [ ] Performance acceptable under load

**Deliverables**:
- Deployment test scripts (2 files)
- Deployment test report
- Load testing results

**Estimated Time**: 3 hours

---

## Implementation Timeline

**Phase 1**: 2 hours - Dependencies and Module Structure
**Phase 2**: 4 hours - Request/Response Types
**Phase 3**: 7 hours - ONNX Model Infrastructure
**Phase 4**: 6 hours - HTTP Endpoint Handler
**Phase 5**: 2 hours - Model Discovery Endpoint
**Phase 6**: 6 hours - Integration Testing
**Phase 7**: 4 hours - Documentation
**Phase 8**: 6 hours - Performance Optimization
**Phase 9**: 8 hours - Production Readiness

**Total Timeline**: ~45 hours (~1 week full-time, ~2 weeks part-time)

## Current Progress Summary

### 🚧 Phase Status
- **Phase 1**: ⏳ Not Started - Dependencies and Module Structure
  - Sub-phase 1.1: ⏳ Not Started - Add Dependencies
  - Sub-phase 1.2: ⏳ Not Started - Create Module Structure
- **Phase 2**: ⏳ Not Started - Request/Response Types
  - Sub-phase 2.1: ⏳ Not Started - Define Request Type
  - Sub-phase 2.2: ⏳ Not Started - Define Response Type
- **Phase 3**: ⏳ Not Started - ONNX Model Infrastructure
  - Sub-phase 3.1: ⏳ Not Started - ONNX Model Wrapper
  - Sub-phase 3.2: ⏳ Not Started - Multi-Model Manager
- **Phase 4**: ⏳ Not Started - HTTP Endpoint Handler
  - Sub-phase 4.1: ⏳ Not Started - Handler Implementation
  - Sub-phase 4.2: ⏳ Not Started - Route Registration
- **Phase 5**: ⏳ Not Started - Model Discovery Endpoint
  - Sub-phase 5.1: ⏳ Not Started - GET /v1/models?type=embedding
- **Phase 6**: ⏳ Not Started - Integration Testing
  - Sub-phase 6.1: ⏳ Not Started - End-to-End Integration Tests
  - Sub-phase 6.2: ⏳ Not Started - Compatibility Testing
- **Phase 7**: ⏳ Not Started - Documentation
  - Sub-phase 7.1: ⏳ Not Started - API Documentation
  - Sub-phase 7.2: ⏳ Not Started - Deployment Documentation
- **Phase 8**: ⏳ Not Started - Performance Optimization
  - Sub-phase 8.1: ⏳ Not Started - Benchmarking and Profiling
  - Sub-phase 8.2: ⏳ Not Started - Optional GPU Support
- **Phase 9**: ⏳ Not Started - Production Readiness
  - Sub-phase 9.1: ⏳ Not Started - Error Handling Audit
  - Sub-phase 9.2: ⏳ Not Started - Security Audit
  - Sub-phase 9.3: ⏳ Not Started - Deployment Testing

**Implementation Status**: 🚧 **NOT STARTED** - Embedding endpoint implementation ready to begin. Total estimated implementation: ~45 hours (~1 week full-time). Comprehensive plan with 9 phases, 15 sub-phases, 100+ TDD tests planned.

## Critical Path

1. **Phase 1.1**: Dependencies must be added without breaking existing build
2. **Phase 3.1**: ONNX model wrapper must work correctly for embeddings
3. **Phase 4.1**: HTTP handler is core functionality
4. **Phase 6**: Integration testing validates entire implementation
5. **Phase 9**: Production readiness ensures stability

## Risk Mitigation

1. **ONNX/CUDA Conflicts**: Use separate ONNX Runtime without CUDA features
2. **Memory Usage**: Load models lazily, add environment flag to disable
3. **Model Availability**: Continue startup even if models fail to load
4. **Performance**: CPU-only is acceptable for MVP, GPU is optional
5. **File Size**: Extract to separate module to avoid file size constraints
6. **Dependency Conflicts**: Test build incrementally after each dependency

## Success Metrics

- **Functional**: All tests passing (100+ tests)
- **Performance**: <100ms per embedding (CPU), <50ms (GPU optional)
- **Compatibility**: No regressions in existing functionality
- **Documentation**: Complete API and deployment docs
- **Security**: Input validation comprehensive, no vulnerabilities
- **Reliability**: Graceful error handling and recovery

## Dependencies

### External Crates (New)
```toml
ort = { version = "2.0", features = ["download-binaries"] }
tokenizers = "0.19"
ndarray = "0.15"
hf-hub = "0.3"  # Optional, for model auto-download
```

### SDK Requirements
- SDK will use HostAdapter to call /v1/embed endpoint
- SDK expects 384-dimensional embeddings
- SDK uses EmbeddingCache for performance
- SDK integrates with DocumentManager for RAG

## Notes

- Each sub-phase should be completed before moving to the next
- Write tests FIRST (TDD approach)
- Keep existing inference functionality working (no regressions)
- Document all configuration options
- Never log embeddings or user text (privacy)
- Test with real ONNX models, not mocks
- Validate 384 dimensions at runtime
- Follow existing API patterns (multi-chain, error handling)

## Reference Documentation

- **Host Implementation Guide**: `docs/sdk-reference/HOST_EMBEDDING_IMPLEMENTATION.md`
- **ONNX Runtime**: https://docs.rs/ort/
- **Tokenizers**: https://docs.rs/tokenizers/
- **ndarray**: https://docs.rs/ndarray/
- **all-MiniLM-L6-v2**: https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2
- **ONNX Format**: https://onnx.ai/

## Development Best Practices

### Do's ✅
- Use battle-tested libraries (ort, tokenizers)
- Validate all inputs (text length, batch size, dimensions)
- Follow existing API patterns (multi-chain, error handling)
- Write comprehensive tests (100+ tests planned)
- Log operations (without logging user data)
- Handle errors gracefully with clear messages
- Document all configuration options

### Don'ts ❌
- Never log embeddings or user text (privacy violation)
- Never persist embeddings on server (client handles storage)
- Never break existing inference functionality (no regressions)
- Never skip dimension validation (384 required)
- Never use custom ML code (use ONNX Runtime)
- Never exceed memory limits (model size + overhead < 500 MB)
- Never skip TDD tests (write tests first)
