# Fabstir LLM Node - Implementation Plan

## Overview

P2P node software for the Fabstir LLM marketplace, enabling GPU owners to provide compute directly to renters without central coordination.

## Development Setup

- **Language**: Rust
- **P2P**: libp2p
- **LLM**: llama.cpp bindings
- **Storage**: Enhanced S5.js with vector-db

## Phase 1: Foundation

### Sub-phase 1.1: Project Setup ✅ **COMPLETE**

- [x] Initialize Rust project structure
- [x] Configure libp2p dependencies
- [x] Set up development environment
- [x] Create module structure

**Test Files:**

- `tests/setup/test_project_structure.rs`
- `tests/setup/test_dependencies.rs`
- `tests/setup/test_modules.rs`
- `tests/setup/test_config.rs`

### Sub-phase 1.2: P2P Networking

- [x] Implement libp2p node creation
- [x] Implement DHT participation
- [x] Implement peer discovery
- [x] Implement message protocols

**Test Files:**

- `tests/p2p/test_node_creation.rs`
- `tests/p2p/test_dht.rs`
- `tests/p2p/test_discovery.rs`
- `tests/p2p/test_protocols.rs`

### Sub-phase 1.3: Client Communication ✅ **COMPLETE**

- [x] Implement request handling
- [x] Implement response streaming
- [x] Implement error handling
- [x] Implement connection management

**Test Files:**

- `tests/client/test_requests.rs`
- `tests/client/test_streaming.rs`
- `tests/client/test_errors.rs`
- `tests/client/test_connections.rs`

### Sub-phase 1.4: Contract Integration ✅ **COMPLETE**

- [x] Implement Base L2 connection
- [x] Implement job event monitoring
- [x] Implement payment verification
- [x] Implement proof submission

**Test Files:**

- `tests/contracts/test_web3.rs`
- `tests/contracts/test_job_monitor.rs`
- `tests/contracts/test_payments.rs`
- `tests/contracts/test_proofs.rs`

#### Phase 2: Core Features

- [x] Phase 2.1: LLM Integration ✅ **COMPLETE**
  - ✅ Real LLaMA inference working with GGUF models
  - ✅ Memory-safe implementation using llama-cpp-2 v0.1.38
  - ✅ GPU support ready for RTX 4090

### Sub-phase 2.2: Job Processing ✅ **COMPLETE**

- [x] Implement job queue
- [x] Implement job claiming
- [x] Implement job execution
- [x] Implement job tracking

**Test Files:**

- `tests/jobs/test_queue.rs`
- `tests/jobs/test_claiming.rs`
- `tests/jobs/test_execution.rs`
- `tests/jobs/test_tracking.rs`

### Sub-phase 2.3: Result Delivery ✅ **COMPLETE**

- [x] Implement result streaming
- [x] Implement result storage
- [x] Implement result verification
- [x] Implement result retrieval

**Test Files:**

- `tests/results/test_streaming.rs`
- `tests/results/test_storage.rs`
- `tests/results/test_verification.rs`
- `tests/results/test_retrieval.rs`

### Sub-phase 2.4: Payment Integration ✅ **COMPLETE**

- [x] Implement escrow management
- [x] Implement payment verification
- [x] Implement payment release
- [x] Implement refund handling

**Test Files:**

- `tests/payment/test_escrow.rs`
- `tests/payment/test_verification.rs`
- `tests/payment/test_release.rs`
- `tests/payment/test_refunds.rs`

### Sub-phase 2.5: Host Management ✅ **COMPLETE**

- [x] Implement model loading
- [x] Implement resource monitoring
- [x] Implement capacity management
- [x] Implement node health checks

**Test Files:**

- `tests/host/test_model_loading.rs`
- `tests/host/test_monitoring.rs`
- `tests/host/test_capacity.rs`
- `tests/host/test_health.rs`

### Sub-phase 2.6: Quality Assurance ✅ **COMPLETE**

- [x] Implement uptime tracking
- [x] Implement response time metrics
- [x] Implement accuracy verification
- [x] Implement user ratings

**Test Files:**

- `tests/qa/test_uptime.rs`
- `tests/qa/test_response_time.rs`
- `tests/qa/test_accuracy.rs`
- `tests/qa/test_ratings.rs`

### Sub-phase 2.7: Storage Integration (Mock) ✅ **COMPLETE**

- [x] Implement S5 client wrapper (mock)
- [x] Implement model storage on S5 (mock)
- [x] Implement result caching with paths (mock)
- [x] Implement CBOR compatibility

**Test Files:**

- `tests/storage/test_s5_client.rs`
- `tests/storage/test_model_storage.rs`
- `tests/storage/test_result_cache.rs`
- `tests/storage/test_cbor_compat.rs`

### Sub-phase 2.8: Vector DB Integration ✅ **COMPLETE**

- [x] Implement Vector DB client (mock)
- [x] Implement prompt embedding generation
- [x] Implement semantic cache lookup
- [x] Implement embedding storage

**Test Files:**

- `tests/vector/test_client.rs`
- `tests/vector/test_embeddings.rs`
- `tests/vector/test_semantic_cache.rs`
- `tests/vector/test_storage.rs`

## Phase 3: Advanced Features (Month 3)

### Sub-phase 3.1: EZKL Proof Generation ✅ **COMPLETE**

- [x] Implement EZKL integration
- [x] Implement proof creation
- [x] Implement batch proofs
- [x] Implement verification

**Test Files:**

- `tests/ezkl/test_integration.rs`
- `tests/ezkl/test_proof_creation.rs`
- `tests/ezkl/test_batch_proofs.rs`
- `tests/ezkl/test_verification.rs`

### Sub-phase 3.2: Model Management

- [x] Implement model downloading
- [x] Implement model validation
- [x] Implement model caching
- [x] Implement model updates

# TODO: Fix test API mismatches in Phase 3.2 tests

# - Update method signatures in test files

# - Adjust type imports to match src/models/mod.rs

# - Ensure all 52 tests compile and pass

**Test Files:**

- `tests/models/test_downloading.rs`
- `tests/models/test_validation.rs`
- `tests/models/test_caching.rs`
- `tests/models/test_updates.rs`

### Sub-phase 3.3: Performance Optimization ✅ **COMPLETE**

- [x] Implement GPU management
- [x] Implement batching
- [x] Implement caching
- [x] Implement load balancing

**Test Files:**

- `tests/performance/test_gpu_management.rs`
- `tests/performance/test_batching.rs`
- `tests/performance/test_caching.rs`
- `tests/performance/test_load_balancing.rs`

### Sub-phase 3.4: Monitoring & Metrics ✅ **COMPLETE**

- [x] Implement performance metrics
- [x] Implement health checks
- [x] Implement alerting
- [x] Implement dashboards

**Test Files:**

- `tests/monitoring/test_metrics.rs`
- `tests/monitoring/test_health_checks.rs`
- `tests/monitoring/test_alerting.rs`
- `tests/monitoring/test_dashboards.rs`

### Sub-phase 3.5: Advanced Model Features ✅ **COMPLETE**

- [x] Implement fine-tuned model support
- [x] Implement private model hosting
- [x] Implement GDPR compliance routing
- [x] Implement model specialization

**Test Files:**

- `tests/models/test_finetuned.rs`
- `tests/models/test_private.rs`
- `tests/models/test_gdpr.rs`
- `tests/models/test_specialization.rs`

## Phase 4: Production Services Integration (Progressive Approach)

**Strategy**: Progressive integration minimizes risk and speeds development by using internal mocks first, then gradually connecting real services.

### Sub-phase 4.1: Mock-to-Mock Development (Week 1)

**Goal**: Implement full functionality using only internal mocks for fastest development.

#### 4.1.1: Enhanced S5.js with Internal Mock

- [x] **Setup Enhanced S5.js with mock backend**

  - [x] Run Enhanced S5.js Docker container with mock storage
  - [x] Configure test environment with ENHANCED_S5_URL
  - [x] Verify health check endpoint connectivity
  - [x] Test basic put/get operations with mock

- [x] **Implement Enhanced S5.js HTTP client**

  - [x] Create EnhancedS5Backend implementing S5Storage trait
  - [x] Implement HTTP operations (PUT/GET/DELETE)
  - [x] Add directory listing support with proper path handling
  - [x] Integrate with existing S5Client when ENHANCED_S5_URL is set

- [x] **Integration testing**

  - [x] Write 9 comprehensive integration tests
  - [x] Test path structures and directory operations
  - [x] Test concurrent operations and thread safety
  - [x] Test error handling (404s, non-existent files)
  - [x] Test large file uploads (5MB)
  - [x] Verify backward compatibility with existing mock

**Test Files:**

- `tests/storage/mock/test_enhanced_s5_api.rs` - 9 tests, all passing ✅

**Implementation Files:**

- `src/storage/enhanced_s5_client.rs` - HTTP client for Enhanced S5.js
- `src/storage/mod.rs` - Updated to support Enhanced S5 backend
- `src/storage/s5_client.rs` - Modified to use Enhanced S5 when configured

**Note**: HAMT sharding and advanced CBOR features deferred as Enhanced S5.js mock uses SimpleKVStorage

#### 4.1.2: Fabstir Vector DB with Internal Mock ✅

- [x] **Setup Vector DB with mock backend**

  - [x] Run Vector DB Docker container with S5_MODE=mock
  - [x] Configure Vector DB client to use REST API
  - [x] Test health check endpoint
  - [ ] ~~Verify API authentication~~ (No auth required in mock mode)

- [x] **Implement vector operations**

  - [x] Replace mock HashMap with REST API calls
  - [x] Implement vector insertion with metadata
  - [x] Implement similarity search
  - [x] Handle batch operations

- [ ] **Test index behavior** (Deferred to Phase 4.2)

  - [ ] Verify HNSW index for recent vectors
  - [ ] Verify IVF index for historical vectors
  - [ ] Test automatic migration between indices
  - [ ] Monitor memory usage

- [ ] **MCP server integration** (Optional - not blocking)
  - [ ] Test MCP server connectivity (port 7531)
  - [ ] Implement vector_search tool
  - [ ] Implement insert_vector tool
  - [ ] Test from LLM client

**Test Files Created:**

- `tests/vector/mock/test_vector_db_api.rs` ✅ (9 tests passing)
- ~~`tests/vector/mock/test_index_behavior.rs`~~ (Not needed for mock)
- ~~`tests/vector/mock/test_mcp_server.rs`~~ (Deferred)
- ~~`tests/vector/mock/test_batch_ops.rs`~~ (Included in main test file)

**Additional Work Completed:**

- Implemented Vector DB REST API handlers (replaced TODOs)
- Fixed API path routing with `/api/v1` prefix
- Added container-to-container networking
- Handled API differences (field mappings, UUID generation)
- All core functionality working with mock S5 storage

#### 4.1.3: Integration with Both Mocks ✅

- [ ] **Complete workflow testing**

  - [x] Store model in Enhanced S5.js
  - [x] Generate embeddings for model
  - [x] Store embeddings in Vector DB
  - [x] Test semantic search for similar models

- [ ] **Cache flow implementation**
  - [x] Hash prompts for cache lookup
  - [x] Search Vector DB for similar prompts
  - [x] Retrieve cached results from S5
  - [x] Measure cache hit rates

**Test Files:**

- `tests/integration/mock/test_e2e_workflow.rs`
- `tests/integration/mock/test_cache_flow.rs`

### Sub-phase 4.2: Service-to-Service Integration (Week 2)

**Goal**: Connect Vector DB to Enhanced S5.js (both still using internal mocks).

#### 4.2.1: Vector DB → Enhanced S5.js Connection

- [ ] **Configure Vector DB to use Enhanced S5.js**

  - [ ] Update Vector DB S5_MODE=real
  - [ ] Set S5_PORTAL_URL to Enhanced S5.js endpoint
  - [ ] Verify connectivity between containers
  - [ ] Test vector persistence in S5

- [ ] **Verify integrated storage**

  - [ ] Store vectors via Vector DB API
  - [ ] Verify vectors appear in Enhanced S5.js
  - [ ] Test HAMT sharding for vector storage
  - [ ] Monitor storage paths and structure

- [ ] **Test persistence and recovery**
  - [ ] Restart Vector DB container
  - [ ] Verify vectors persist via S5
  - [ ] Test backup and restore
  - [ ] Measure recovery time

**Test Files:**

- `tests/integration/connected/test_vectordb_s5_storage.rs`
- `tests/integration/connected/test_persistence.rs`
- `tests/integration/connected/test_recovery.rs`

#### 4.2.2: Performance Testing with Connected Mocks

- [ ] **Benchmark operations**

  - [ ] Measure vector insertion throughput
  - [ ] Test search latency at scale
  - [ ] Monitor S5 API call patterns
  - [ ] Identify bottlenecks

- [ ] **Scale testing**
  - [ ] Insert 10K+ vectors (HAMT trigger)
  - [ ] Test with 100K+ vectors
  - [ ] Monitor memory and CPU usage
  - [ ] Test concurrent operations

**Test Files:**

- `tests/performance/connected/test_throughput.rs`
- `tests/performance/connected/test_scale.rs`

### Sub-phase 4.3: Real Backend Integration (Week 3)

**Goal**: Switch services to real backends one at a time.

#### 4.3.1: Enhanced S5.js → Real S5 Portal

- [ ] **Configure Enhanced S5.js for real S5**

  - [ ] Update S5_MODE=real
  - [ ] Configure S5_PORTAL_URL=https://s5.vup.cx
  - [ ] Set up S5_SEED_PHRASE authentication
  - [ ] Test portal connectivity

- [ ] **Verify real storage operations**

  - [ ] Test file upload to real S5
  - [ ] Verify CID generation
  - [ ] Test file retrieval
  - [ ] Monitor bandwidth usage

- [ ] **Test reliability**
  - [ ] Handle network timeouts
  - [ ] Implement retry strategies
  - [ ] Test error recovery
  - [ ] Monitor success rates

**Test Files:**

- `tests/storage/real/test_s5_portal.rs`
- `tests/storage/real/test_reliability.rs`

#### 4.3.2: Complete Real Integration

- [ ] **Vector DB with real S5 backend**

  - [ ] Verify Vector DB → Enhanced S5.js → S5 Portal chain
  - [ ] Test vector persistence on real S5
  - [ ] Monitor storage costs
  - [ ] Test at production scale

- [ ] **Migration from mock data**
  - [ ] Export mock data
  - [ ] Import to real S5
  - [ ] Verify data integrity
  - [ ] Update references

**Test Files:**

- `tests/integration/real/test_full_chain.rs`
- `tests/integration/real/test_migration.rs`

### Sub-phase 4.4: Production Readiness (Week 4)

#### 4.4.1: Full System Testing

- [ ] **End-to-end production tests**

  - [ ] Complete inference workflow with real backends
  - [ ] Load testing at expected scale
  - [ ] Chaos testing (kill containers, network issues)
  - [ ] Security audit

- [ ] **Performance validation**
  - [ ] Verify < 100ms S5 latency (p95)
  - [ ] Verify < 50ms vector search (p99)
  - [ ] Test 30%+ cache hit rate
  - [ ] Monitor resource usage

**Test Files:**

- `tests/production/test_e2e_real.rs`
- `tests/production/test_load.rs`
- `tests/production/test_chaos.rs`
- `tests/production/test_performance.rs`

#### 4.4.2: Deployment Configuration

- [ ] **Production environment setup**

  - [ ] Create production docker-compose.yml
  - [ ] Configure environment variables
  - [ ] Set up monitoring (Prometheus/Grafana)
  - [ ] Create backup procedures

- [ ] **Documentation**
  - [ ] Update deployment guide
  - [ ] Document configuration options
  - [ ] Create troubleshooting guide
  - [ ] Write operational runbooks

**Files:**

- `deployment/docker-compose.prod.yml`
- `deployment/.env.production`
- `docs/deployment.md`
- `docs/operations.md`

## Docker Commands for Phase 4

```bash
# Week 1: Both services with mocks
docker-compose -f docker-compose.mocks.yml up -d

# Week 2: Vector DB uses Enhanced S5.js
docker-compose -f docker-compose.connected.yml up -d

# Week 3: Enhanced S5.js uses real S5 portal
docker-compose -f docker-compose.real.yml up -d

# Week 4: Production configuration
docker-compose -f docker-compose.prod.yml up -d
```

## Phase 5: SDK Development

### fabstir-llm-sdk

- [ ] Phase 5.1: Core SDK
- [ ] Phase 5.2: WebAssembly Module
- [ ] Phase 5.3: Demo Application
- [ ] Phase 5.4: Documentation

## Testing Strategy

Each phase follows strict TDD principles:

1. **Unit Tests**: Core functionality in isolation
2. **Integration Tests**: Component interactions
3. **Mock Tests**: Fast development with mock services
4. **Production Tests**: Real service validation
5. **Performance Tests**: Scalability verification

## Environment Variables for Phase 4 Progression

### Week 1: Both Mocks

```bash
# Enhanced S5.js
ENHANCED_S5_MODE=mock
ENHANCED_S5_PORT=5050

# Vector DB
VECTOR_DB_MODE=mock
VECTOR_DB_PORT=7530
```

### Week 2: Connected Mocks

```bash
# Enhanced S5.js (still mock)
ENHANCED_S5_MODE=mock
ENHANCED_S5_PORT=5050

# Vector DB (using Enhanced S5.js)
VECTOR_DB_MODE=real
VECTOR_DB_S5_URL=http://enhanced-s5:5050
```

### Week 3-4: Real Backends

```bash
# Enhanced S5.js (real S5 portal)
ENHANCED_S5_MODE=real
ENHANCED_S5_PORTAL_URL=https://s5.vup.cx
ENHANCED_S5_SEED_PHRASE=${S5_SEED_PHRASE}

# Vector DB (using real Enhanced S5.js)
VECTOR_DB_MODE=real
VECTOR_DB_S5_URL=http://enhanced-s5:5050
```

## Key Decisions Summary

1. **Development Approach**: Progressive integration from mocks to real services
2. **Storage Architecture**: Enhanced S5.js provides decentralized storage with path-based API
3. **Caching Strategy**: Vector DB provides semantic caching to reduce redundant inference
4. **CBOR Standard**: All components use deterministic CBOR for cross-platform compatibility
5. **HAMT Sharding**: Automatic activation at 1000+ items for O(log n) performance

## Migration Path

1. **Week 1**: Both services with internal mocks
2. **Week 2**: Vector DB uses Enhanced S5.js (both mocked)
3. **Week 3**: Enhanced S5.js uses real S5 portal
4. **Week 4**: Production testing and deployment

## Success Metrics

- Mock → Real migration completed without data loss
- Semantic cache hit rate > 30%
- S5 storage latency < 100ms (p95)
- Vector search latency < 50ms (p99)
- HAMT performance verified at 1M+ items
