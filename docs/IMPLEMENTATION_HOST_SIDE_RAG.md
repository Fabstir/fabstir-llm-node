# Host-Side RAG Implementation Plan (Session-Scoped Vector Search)

## Overview

This implementation plan adds **host-side RAG (Retrieval-Augmented Generation)** to fabstir-llm-node, enabling users to upload document vectors at session start and perform semantic search during chat conversations. This approach leverages existing infrastructure (embedding generation, session management, cosine similarity) to deliver RAG functionality in **2-3 days** instead of the 7-week WASM alternative, following strict TDD (Test-Driven Development) with bounded autonomy.

## Core Requirements

- **Storage**: Session-scoped HashMap (vectors stored in host memory during session)
- **Lifecycle**: Upload at session start, search during chat, clear on disconnect
- **Privacy**: Session-scoped memory cleared on disconnect (same as conversation cache)
- **Performance**: <50ms vector search for 10K vectors (native Rust, no WASM overhead)
- **Dimensions**: 384-dimensional vectors (matching host embeddings from Phase 4.2)
- **Capacity**: Up to 100K vectors per session (configurable memory limit)
- **Multi-Session**: Isolated vector stores per session (concurrent users supported)
- **Protocol**: WebSocket messages for vector upload/search (extends existing protocol)

## Architecture Integration

### Current State (v8.2.0)

```
┌──────────────────────────────────────────────────────┐
│  fabstir-llm-node (Existing Infrastructure)         │
├──────────────────────────────────────────────────────┤
│                                                      │
│  ✅ POST /v1/embed (Phase 4.2 complete)             │
│     • all-MiniLM-L6-v2 ONNX model                   │
│     • 384-dimensional embeddings                    │
│     • 10.9ms per embedding, 90 req/s throughput     │
│                                                      │
│  ✅ Session Management (Phases 8.7-8.12 complete)   │
│     • WebSocket-based sessions                      │
│     • Conversation context in memory                │
│     • Cleanup on disconnect                         │
│     • Memory limits enforced                        │
│                                                      │
│  ✅ Vector Infrastructure (src/vector/)             │
│     • embeddings.rs (485 lines) - similarity funcs  │
│     • vector_db_client.rs (335 lines) - search      │
│     • Cosine/Euclidean/Manhattan distance           │
│                                                      │
└──────────────────────────────────────────────────────┘
```

### Target State (After This Implementation)

```
┌──────────────────────────────────────────────────────┐
│  fabstir-llm-node + Host-Side RAG                    │
├──────────────────────────────────────────────────────┤
│                                                      │
│  WebSocket Session:                                  │
│  ┌────────────────────────────────────────────────┐ │
│  │ SessionState                                   │ │
│  │ ┌────────────────────────────────────────────┐ │ │
│  │ │ conversation_context: Vec<Message> ✅      │ │ │
│  │ │ (existing - cleared on disconnect)         │ │ │
│  │ └────────────────────────────────────────────┘ │ │
│  │ ┌────────────────────────────────────────────┐ │ │
│  │ │ vector_store: SessionVectorStore ← NEW    │ │ │
│  │ │ • vectors: HashMap<String, Vector>        │ │ │
│  │ │ • add(), search(), clear()                │ │ │
│  │ │ • Cleared on disconnect                   │ │ │
│  │ └────────────────────────────────────────────┘ │ │
│  └────────────────────────────────────────────────┘ │
│                                                      │
│  New WebSocket Messages:                             │
│  • UploadVectors (client → host)                    │
│  • SearchVectors (client → host)                    │
│  • VectorSearchResult (host → client)               │
│                                                      │
└──────────────────────────────────────────────────────┘
```

### User Workflow

```
Session Start:
  1. Client uploads PDF
  2. Client chunks document (500 tokens per chunk)
  3. Client calls POST /v1/embed → receives 384D embeddings
  4. Client sends UploadVectors message → host stores in SessionVectorStore

During Chat:
  5. User asks: "What is machine learning?"
  6. Client calls POST /v1/embed on question → get query embedding
  7. Client sends SearchVectors message
  8. Host searches SessionVectorStore → returns top-k relevant chunks
  9. Client injects context into prompt
  10. Client sends augmented prompt to /v1/inference
  11. Host generates answer using retrieved context

Session End:
  12. User disconnects → host calls SessionVectorStore.clear()
  13. All vectors removed from memory (same as conversation cleanup)
```

### Key Design Decisions

1. **Session-Scoped, Not Persistent**
   - Vectors stored only during active WebSocket session
   - Cleared on disconnect (same lifecycle as conversation context)
   - No database, no files, no persistence
   - Privacy equivalent to current conversation cache

2. **Leverage Existing Infrastructure**
   - Reuse `Embedding::cosine_similarity()` from `src/vector/embeddings.rs`
   - Reuse session management from `src/api/websocket/session.rs`
   - Reuse WebSocket protocol from `src/api/websocket/messages.rs`
   - Only add ~400 new lines of code

3. **Client-Controlled Upload**
   - Client decides what vectors to upload
   - Client controls when to upload (session start)
   - Client manages document chunking
   - Host only provides search compute

4. **Native Rust Performance**
   - No WASM overhead
   - No IndexedDB delays
   - <50ms search for 10K vectors
   - Scales better than browser-based search

## Implementation Status

| Phase | Status | Tests | Estimated | Notes |
|-------|--------|-------|-----------|-------|
| Phase 1: Session Vector Storage | ✅ Complete + Hardened | 47/47 | 6h | All sub-phases + critical fixes! |
| Phase 2: WebSocket Protocol | ✅ Complete | 29/29 | 6h | All sub-phases complete! ✅ |
| Phase 3: Integration & Testing | ⏳ Not Started | 0/13 | 8h | E2E RAG workflow |
| **TOTAL** | **86% Complete** | **76/88 tests** | **~20 hours** | **~2-3 days** |

### Phase 1 Critical Fixes Applied ✅
- **NaN/Infinity Validation**: Added validation to prevent invalid float values (4 tests)
- **Metadata Size Limit**: 10KB max per vector to prevent memory exhaustion (2 tests)
- **Edge Case Coverage**: k=0, negative thresholds, capacity management (4 tests)
- **Total**: 47 tests passing (37 original + 10 hardening tests)

---

## Phase 1: Session Vector Storage

### Sub-phase 1.1: Create SessionVectorStore Struct ✅
**Goal**: Implement in-memory vector storage with search capabilities

**Tasks**:
- [x] Create `src/rag/mod.rs` (new module)
- [x] Create `src/rag/session_vector_store.rs`
- [x] Define `SessionVectorStore` struct with HashMap<String, VectorEntry>
- [x] Define `VectorEntry` struct: `{ vector: Vec<f32>, metadata: Value, created_at: Instant }`
- [x] Implement `new(session_id: String, max_vectors: usize) -> Self`
- [x] Implement `add(&mut self, id: String, vector: Vec<f32>, metadata: Value) -> Result<()>`
- [x] Validate vector dimensions = 384
- [x] Implement `get(&self, id: &str) -> Option<&VectorEntry>`
- [x] Implement `delete(&mut self, id: &str) -> bool`
- [x] Implement `count(&self) -> usize`
- [x] Implement `clear(&mut self)`
- [x] Add memory limit enforcement (max_vectors)
- [x] Update `src/lib.rs` to export `rag` module

**Test Files** (TDD - Written First):
- `tests/rag/test_session_vector_store.rs` - 15 tests
  - test_new_creates_empty_store()
  - test_add_single_vector()
  - test_add_validates_dimensions() (reject non-384)
  - test_add_with_metadata()
  - test_add_duplicate_id_replaces()
  - test_get_existing_vector()
  - test_get_nonexistent_returns_none()
  - test_delete_existing_vector()
  - test_delete_nonexistent_returns_false()
  - test_count_accurate()
  - test_clear_removes_all()
  - test_max_vectors_enforced() (reject when full)
  - test_multiple_sessions_isolated()
  - test_concurrent_add_safe()
  - test_memory_usage_reasonable()

**Success Criteria**:
- [x] All vectors stored with 384D validation
- [x] CRUD operations work correctly
- [x] Memory limits enforced
- [x] 19 passing tests (15 original + 4 NaN/Infinity validation)

**Deliverables**:
- [x] `src/rag/mod.rs`
- [x] `src/rag/session_vector_store.rs` (~370 lines with validation)
- [x] 19 passing tests
- [x] Module exported from `src/lib.rs`

**Estimated Time**: 2 hours

**Notes**:
- Use `std::collections::HashMap` for simplicity (no async needed)
- Store metadata as `serde_json::Value` for flexibility
- max_vectors default: 100,000 (configurable via env var)

---

### Sub-phase 1.2: Implement Vector Search ✅
**Goal**: Add semantic search using existing cosine similarity

**Tasks**:
- [x] Add `use crate::vector::embeddings::Embedding;` import
- [x] Implement `search(&self, query: Vec<f32>, k: usize, threshold: Option<f32>) -> Result<Vec<SearchResult>>`
- [x] Define `SearchResult` struct: `{ id: String, score: f32, metadata: Value }`
- [x] Validate query vector dimensions = 384
- [x] Compute cosine similarity for all vectors (reuse `Embedding::cosine_similarity`)
- [x] Filter by threshold if provided (e.g., score >= 0.7)
- [x] Sort by score descending
- [x] Return top-k results
- [x] Handle empty store gracefully
- [x] Add `search_with_filter(&self, query: Vec<f32>, k: usize, metadata_filter: Value) -> Result<Vec<SearchResult>>`
- [x] Implement metadata filtering (basic $eq, $in support)

**Test Files** (TDD - Written First):
- `tests/rag/test_vector_search.rs` - 12 tests
  - test_search_empty_store_returns_empty()
  - test_search_single_vector()
  - test_search_returns_top_k()
  - test_search_sorted_by_score_descending()
  - test_search_validates_query_dimensions()
  - test_search_with_threshold()
  - test_search_threshold_filters_results()
  - test_search_exact_match_highest_score()
  - test_search_orthogonal_vectors_low_score()
  - test_search_k_larger_than_store()
  - test_search_with_metadata_filter_eq()
  - test_search_with_metadata_filter_in()

**Success Criteria**:
- [x] Correct cosine similarity calculation
- [x] Results sorted by relevance
- [x] Threshold filtering works
- [x] 16 passing tests (12 original + 4 edge case tests)

**Deliverables**:
- [x] `search()` method (~80 lines)
- [x] `SearchResult` struct
- [x] Metadata filtering logic
- [x] 16 passing tests

**Estimated Time**: 2 hours

**Notes**:
- Reuse `Embedding::cosine_similarity()` from `src/vector/embeddings.rs:97-117`
- For 10K vectors: simple linear scan is fast enough (<50ms)
- Can optimize with HNSW/IVF later if needed

---

### Sub-phase 1.3: Integrate with Session Management ✅
**Goal**: Add SessionVectorStore to existing WebSocket sessions

**Tasks**:
- [x] Update `src/api/websocket/session.rs` to import `SessionVectorStore`
- [x] Add `vector_store: Option<Arc<Mutex<SessionVectorStore>>>` field to `WebSocketSession`
- [x] Initialize `vector_store: None` in `WebSocketSession::with_chain()`
- [x] Add `fn enable_rag(&mut self, max_vectors: usize)` method
- [x] Add `fn get_vector_store(&self) -> Option<Arc<Mutex<SessionVectorStore>>>`
- [x] Update `clear()` to call `vector_store.clear()` on disconnect
- [ ] Add environment variable `RAG_MAX_VECTORS_PER_SESSION` (default: 100000) - Deferred to Phase 2
- [ ] Add environment variable `RAG_ENABLED` (default: true) - Deferred to Phase 2
- [ ] Update session initialization to create vector store if RAG_ENABLED - Deferred to Phase 2

**Test Files** (TDD - Written First):
- `tests/rag/test_session_integration.rs` - 10 tests
  - test_session_creates_vector_store()
  - test_session_rag_disabled_no_store()
  - test_session_vector_store_isolated()
  - test_session_cleanup_clears_vectors()
  - test_concurrent_sessions_independent_stores()
  - test_max_vectors_configurable()
  - test_session_disconnect_frees_memory()
  - test_session_vector_store_thread_safe()
  - test_multiple_sessions_no_memory_leak()
  - test_rag_enable_disable_toggle()

**Success Criteria**:
- [x] Vector store lifecycle matches session lifecycle
- [x] Cleanup on disconnect works
- [x] Concurrent sessions isolated
- [x] 12 passing tests (10 original + 2 metadata size limit tests)

**Deliverables**:
- [x] Updated `src/api/websocket/session.rs` (+40 lines)
- [⚠️] Environment variable configuration (Deferred to Phase 2 - manual enable_rag() works)
- [x] 12 passing tests
- [x] Session cleanup verified

**Estimated Time**: 2 hours

**Notes**:
- Use `Arc<Mutex<SessionVectorStore>>` for thread-safe access
- Cleanup already called on disconnect (Phase 8.12 complete)
- Just add vector_store.clear() to existing cleanup logic

---

## Phase 2: WebSocket Protocol Extensions

### Sub-phase 2.1: Define Vector Upload Messages ✅
**Goal**: Add message types for uploading vectors to session

**Tasks**:
- [x] Update `src/api/websocket/message_types.rs`
- [x] Define `UploadVectorsRequest` struct:
  ```rust
  pub struct UploadVectorsRequest {
      pub vectors: Vec<VectorUpload>,
      pub replace: bool,  // true = clear existing, false = append
  }
  pub struct VectorUpload {
      pub id: String,
      pub vector: Vec<f32>,
      pub metadata: Value,
  }
  ```
- [x] Define `UploadVectorsResponse` struct:
  ```rust
  pub struct UploadVectorsResponse {
      pub uploaded: usize,
      pub rejected: usize,
      pub errors: Vec<String>,
  }
  ```
- [⚠️] Add `UploadVectors` variant to `ClientMessage` enum (Deferred to Sub-phase 2.3)
- [⚠️] Add `UploadVectorsResult` variant to `ServerMessage` enum (Deferred to Sub-phase 2.3)
- [x] Implement serde serialization with camelCase
- [x] Add validation: max batch size 1000 vectors per message
- [x] Add request ID for tracking

**Test Files** (TDD - Written First):
- `tests/api/test_upload_vectors_messages.rs` - 8 tests
  - test_upload_vectors_request_serialization()
  - test_upload_vectors_response_deserialization()
  - test_upload_validates_batch_size()
  - test_upload_validates_dimensions()
  - test_upload_replace_flag()
  - test_upload_with_metadata()
  - test_upload_error_messages_clear()
  - test_upload_request_id_preserved()

**Success Criteria**:
- [x] Message types serialize/deserialize correctly
- [x] Batch size validation works
- [x] Error messages clear
- [x] 8 passing tests

**Deliverables**:
- [x] Updated `src/api/websocket/message_types.rs` (+91 lines)
- [x] Request/response types
- [x] Validation logic
- [x] 8 passing tests

**Estimated Time**: 2 hours

---

### Sub-phase 2.2: Define Vector Search Messages ✅
**Goal**: Add message types for searching vectors

**Tasks**:
- [x] Update `src/api/websocket/message_types.rs`
- [x] Define `SearchVectorsRequest` struct:
  ```rust
  pub struct SearchVectorsRequest {
      pub query_vector: Vec<f32>,
      pub k: usize,              // top-k results
      pub threshold: Option<f32>, // minimum similarity score
      pub metadata_filter: Option<Value>,
  }
  ```
- [x] Define `SearchVectorsResponse` struct:
  ```rust
  pub struct SearchVectorsResponse {
      pub results: Vec<VectorSearchResult>,
      pub total_vectors: usize,
      pub search_time_ms: f64,
  }
  pub struct VectorSearchResult {
      pub id: String,
      pub score: f32,
      pub metadata: Value,
  }
  ```
- [⚠️] Add `SearchVectors` variant to `ClientMessage` enum (Deferred to Sub-phase 2.3)
- [⚠️] Add `SearchVectorsResult` variant to `ServerMessage` enum (Deferred to Sub-phase 2.3)
- [x] Implement serde serialization with camelCase
- [x] Add validation: k <= 100 (MAX_SEARCH_K constant)
- [x] Add request ID for async response matching

**Test Files** (TDD - Written First):
- `tests/api/test_search_vectors_messages.rs` - 9 tests
  - test_search_request_serialization()
  - test_search_response_deserialization()
  - test_search_validates_k_limit()
  - test_search_validates_query_dimensions()
  - test_search_with_threshold()
  - test_search_with_metadata_filter()
  - test_search_timing_included()
  - test_search_empty_results()
  - test_search_result_structure()

**Success Criteria**:
- [x] Message types work correctly
- [x] Validation enforced
- [x] Search timing tracked
- [x] 9 passing tests

**Deliverables**:
- [x] Updated `src/api/websocket/message_types.rs` (+86 lines)
- [x] Request/response types (SearchVectorsRequest, SearchVectorsResponse, VectorSearchResult)
- [x] Validation logic (MAX_SEARCH_K = 100)
- [x] 9 passing tests

**Estimated Time**: 2 hours

---

### Sub-phase 2.3: Implement Message Handlers ✅
**Goal**: Handle vector upload/search in WebSocket handler

**Tasks**:
- [x] Create `src/api/websocket/handlers/rag.rs`
- [x] Implement `handle_upload_vectors(session: &Arc<Mutex<WebSocketSession>>, request: UploadVectorsRequest) -> Result<UploadVectorsResponse>`
- [x] Get vector_store from session (return error if RAG not enabled)
- [x] Validate all vectors before adding (384 dimensions via SessionVectorStore)
- [x] If replace=true, call vector_store.clear() first
- [x] Add vectors to store, collect errors
- [x] Return response with counts
- [x] Implement `handle_search_vectors(session: &Arc<Mutex<WebSocketSession>>, request: SearchVectorsRequest) -> Result<SearchVectorsResponse>`
- [x] Get vector_store from session
- [x] Start timer for performance tracking (Instant::now())
- [x] Call vector_store.search() with parameters (handles threshold and metadata_filter)
- [x] Stop timer, include in response (elapsed_ms)
- [x] Return results with timing
- [x] Add rag module to `src/api/websocket/handlers/mod.rs`
- [x] Add error handling for all edge cases (RAG not enabled, invalid dimensions, NaN values)

**Test Files** (TDD - Written First):
- `tests/api/test_rag_handlers.rs` - 12 tests
  - test_upload_handler_success()
  - test_upload_handler_validates_dimensions()
  - test_upload_handler_replace_clears()
  - test_upload_handler_rag_disabled_error()
  - test_upload_handler_batch_processing()
  - test_upload_handler_partial_success()
  - test_search_handler_success()
  - test_search_handler_empty_store()
  - test_search_handler_with_threshold()
  - test_search_handler_with_filter()
  - test_search_handler_rag_disabled_error()
  - test_search_handler_timing_accurate()

**Success Criteria**:
- [x] Upload handler processes batches
- [x] Search handler returns results
- [x] Error handling works
- [x] 12 passing tests

**Deliverables**:
- [x] `src/api/websocket/handlers/rag.rs` (~220 lines with tests)
- [x] Updated `handlers/mod.rs` to export rag module
- [x] 12 passing tests

**Estimated Time**: 2 hours

---

## Phase 3: Integration & Testing

### Sub-phase 3.1: End-to-End RAG Workflow Test ⏳
**Goal**: Test complete RAG flow from upload to search to inference

**Tasks**:
- [ ] Create test document fixture (sample PDF text)
- [ ] Create test: upload vectors → search → verify results
- [ ] Test: upload 100 chunks from document
- [ ] Test: search for "machine learning" query
- [ ] Test: verify top-5 results are relevant
- [ ] Test: inject context into prompt
- [ ] Test: send augmented prompt to inference
- [ ] Test: verify response uses retrieved context
- [ ] Test: session cleanup removes vectors
- [ ] Add performance benchmarks (search latency)
- [ ] Test with realistic dataset (10K vectors)

**Test Files** (TDD - Written First):
- `tests/integration/test_rag_e2e.rs` - 8 tests
  - test_full_rag_workflow()
  - test_upload_search_inference_pipeline()
  - test_multiple_searches_same_session()
  - test_replace_vectors_mid_session()
  - test_search_with_filters()
  - test_session_cleanup_removes_vectors()
  - test_rag_10k_vectors_performance()
  - test_concurrent_sessions_rag()

**Success Criteria**:
- [ ] Complete RAG workflow works
- [ ] Search results relevant
- [ ] Context injection successful
- [ ] 8 passing tests

**Deliverables**:
- [ ] End-to-end integration tests
- [ ] Test fixtures
- [ ] Performance benchmarks
- [ ] 8 passing tests

**Estimated Time**: 3 hours

---

### Sub-phase 3.2: SDK Integration Example ⏳
**Goal**: Provide example code for SDK developers

**Tasks**:
- [ ] Create `examples/rag_integration.rs`
- [ ] Show how to connect WebSocket
- [ ] Show how to upload vectors
- [ ] Show how to search vectors
- [ ] Show how to inject context into prompts
- [ ] Add documentation comments
- [ ] Create `docs/RAG_SDK_INTEGRATION.md`
- [ ] Document UploadVectors message format
- [ ] Document SearchVectors message format
- [ ] Document expected workflow
- [ ] Add TypeScript examples for SDK
- [ ] Add error handling examples

**Test Files**:
- Manual validation of examples

**Success Criteria**:
- [ ] Example code compiles and runs
- [ ] Documentation complete
- [ ] SDK developers can integrate

**Deliverables**:
- [ ] `examples/rag_integration.rs`
- [ ] `docs/RAG_SDK_INTEGRATION.md`
- [ ] TypeScript usage examples

**Estimated Time**: 2 hours

---

### Sub-phase 3.3: Performance & Memory Testing ⏳
**Goal**: Validate performance and memory usage

**Tasks**:
- [ ] Benchmark vector upload (1K, 10K, 100K vectors)
- [ ] Benchmark search latency (1K, 10K, 100K vectors)
- [ ] Measure memory usage per session
- [ ] Test memory cleanup on disconnect
- [ ] Test max_vectors limit enforcement
- [ ] Profile for memory leaks
- [ ] Test concurrent sessions (10, 50, 100)
- [ ] Validate session isolation
- [ ] Document performance characteristics

**Test Files** (TDD - Written First):
- `tests/performance/test_rag_performance.rs` - 5 tests
  - test_upload_1k_vectors_under_100ms()
  - test_search_10k_vectors_under_50ms()
  - test_search_100k_vectors_under_500ms()
  - test_memory_usage_per_session()
  - test_concurrent_sessions_no_slowdown()

**Success Criteria**:
- [ ] Upload: <100ms for 1K vectors
- [ ] Search: <50ms for 10K vectors
- [ ] Memory: <500MB per session (100K vectors)
- [ ] 5 passing performance tests

**Deliverables**:
- [ ] Performance benchmarks
- [ ] Memory usage report
- [ ] 5 passing tests

**Estimated Time**: 3 hours

---

## Development Guidelines

### Do's ✅

**Architecture**:
- Reuse existing infrastructure (embeddings, sessions, WebSocket)
- Keep vectors session-scoped (no persistence)
- Clear vectors on disconnect (same as conversation cache)
- Enforce memory limits per session
- Validate 384-dimensional vectors

**Testing**:
- Write tests BEFORE implementation (strict TDD)
- Test all error paths
- Test concurrent sessions
- Benchmark performance
- Test memory cleanup

**Code Quality**:
- Use existing `Embedding::cosine_similarity()`
- Follow existing WebSocket message patterns
- Add comprehensive error messages
- Document all public APIs
- Use `Arc<Mutex<>>` for thread safety

### Don'ts ❌

**Architecture**:
- Never persist vectors to disk
- Never share vectors across sessions
- Never skip dimension validation (must be 384)
- Never keep vectors after disconnect
- Never exceed session memory limits

**Testing**:
- Never skip TDD (tests first!)
- Never commit failing tests
- Never skip performance testing
- Never ignore memory leaks

**Code Quality**:
- Never panic (use Result)
- Never log user vectors (privacy!)
- Never block WebSocket thread
- Never create new similarity functions (reuse existing)

---

## Timeline Estimate

| Phase | Time |
|-------|------|
| Phase 1: Session Vector Storage | 6h |
| Phase 2: WebSocket Protocol | 6h |
| Phase 3: Integration & Testing | 8h |
| **TOTAL** | **~20 hours (~2-3 days)** |

---

## Success Metrics

### Test Coverage
- **Target**: 40 total tests
- **Current**: 0/40 (0%)
- **Minimum**: 90% pass rate

### Performance
- **Upload**: <100ms for 1K vectors
- **Search**: <50ms for 10K vectors
- **Memory**: <500MB per session (100K vectors)
- **Cleanup**: Complete memory free on disconnect

### Integration
- Full RAG workflow working
- SDK integration documented
- Example code provided

---

## Comparison to Client-Side WASM Alternative

| Factor | Host-Side RAG (This Plan) | Client-Side WASM |
|--------|---------------------------|------------------|
| **Implementation Time** | 2-3 days | 7 weeks |
| **Code to Write** | ~400 lines | Entire WASM infrastructure |
| **Complexity** | Low (reuse existing) | High (wasm-bindgen, IndexedDB, S5) |
| **Performance** | <50ms search (native) | >100ms (WASM overhead) |
| **Memory** | Server RAM (abundant) | Browser RAM (limited) |
| **Bundle Size** | 0 KB | +500 KB |
| **Privacy** | Session-scoped (cleared) | Same (browser cleared) |
| **Offline Mode** | ❌ | ✅ (partial) |
| **Mobile Support** | ✅ (no browser limits) | ⚠️ (RAM constraints) |
| **Maintenance** | Simple Rust | WASM + JS + IndexedDB + S5 |

**Recommendation**: Start with host-side RAG to validate demand, then add WASM later if offline mode is truly needed.

---

## Next Steps

1. ✅ Document created
2. ⏭️ Start Phase 1, Sub-phase 1.1: Create SessionVectorStore
3. ⏭️ Follow strict TDD (tests first!)
4. ⏭️ Mark progress with [x]
5. ⏭️ Deploy and gather user feedback
6. ⏭️ Decide on WASM investment based on actual usage

---

**Document Version**: 1.0.0
**Created**: November 4, 2025
**Status**: ⏳ Ready to Start Phase 1
**Scope**: Session-scoped vector search leveraging existing infrastructure
**Estimated Effort**: ~20 hours (2-3 days)
**Alternative Avoided**: 7-week WASM implementation
