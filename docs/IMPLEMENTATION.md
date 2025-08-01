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

## Phase 2: Core Features

### Sub-phase 2.1: LLM Integration ✅ **COMPLETE**

- [x] Implement llama.cpp bindings
- [x] Implement model loading
- [x] Implement inference pipeline
- [x] Implement result formatting

**Test Files:**

- `tests/inference/test_llama.rs`
- `tests/inference/test_loading.rs`
- `tests/inference/test_pipeline.rs`
- `tests/inference/test_formatting.rs`

### Sub-phase 2.2: Job Processing ✅ **COMPLETE**

- [x] Implement job queue
- [x] Implement job claiming
- [x] Implement progress tracking
- [x] Implement result delivery

**Test Files:**

- `tests/jobs/test_queue.rs`
- `tests/jobs/test_claiming.rs`
- `tests/jobs/test_progress.rs`
- `tests/jobs/test_delivery.rs`

### Sub-phase 2.3: Result Delivery **IN PROGRESS**

- [ ] Result packaging with CBOR encoding
- [ ] P2P delivery to clients via libp2p
- [ ] S5 storage integration at `/results/{job_id}/`
- [ ] Proof generation (Simple/EZKL)

**Test Files:**

- `tests/results/test_packaging.rs`
- `tests/results/test_p2p_delivery.rs`
- `tests/results/test_s5_storage.rs`
- `tests/results/test_proofs.rs`

### Sub-phase 2.4: Payment Integration

- [ ] Implement payment tracking
- [ ] Implement revenue calculation
- [ ] Implement withdrawal mechanism
- [ ] Implement fee distribution

**Test Files:**

- `tests/payments/test_tracking.rs`
- `tests/payments/test_revenue.rs`
- `tests/payments/test_withdrawal.rs`
- `tests/payments/test_fees.rs`

### Sub-phase 2.5: Host Management (**NEW**)

- [ ] Implement model hosting configuration
- [ ] Implement pricing per token/minute
- [ ] Implement availability management
- [ ] Implement resource monitoring

**Test Files:**

- `tests/host/test_model_config.rs`
- `tests/host/test_pricing.rs`
- `tests/host/test_availability.rs`
- `tests/host/test_resources.rs`

### Sub-phase 2.6: Quality Assurance (**NEW**)

- [ ] Implement uptime tracking
- [ ] Implement response time metrics
- [ ] Implement accuracy verification
- [ ] Implement user ratings

**Test Files:**

- `tests/qa/test_uptime.rs`
- `tests/qa/test_response_time.rs`
- `tests/qa/test_accuracy.rs`
- `tests/qa/test_ratings.rs`

### Sub-phase 2.7: Storage Integration (**NEW**)

- [ ] Implement S5 client wrapper
- [ ] Implement model storage on S5
- [ ] Implement result caching with paths
- [ ] Implement CBOR compatibility

**Test Files:**

- `tests/storage/test_s5_client.rs`
- `tests/storage/test_model_storage.rs`
- `tests/storage/test_result_cache.rs`
- `tests/storage/test_cbor_compat.rs`

### Sub-phase 2.8: Vector DB Integration (**NEW**)

- [ ] Implement Vector DB client
- [ ] Implement prompt embedding generation
- [ ] Implement semantic cache lookup
- [ ] Implement embedding storage

**Test Files:**

- `tests/vector/test_client.rs`
- `tests/vector/test_embeddings.rs`
- `tests/vector/test_semantic_cache.rs`
- `tests/vector/test_storage.rs`

## Phase 3: Advanced Features (Month 3)

### Sub-phase 3.1: EZKL Proof Generation

- [ ] Implement EZKL integration
- [ ] Implement proof creation
- [ ] Implement batch proofs
- [ ] Implement verification

**Test Files:**

- `tests/ezkl/test_integration.rs`
- `tests/ezkl/test_proof_creation.rs`
- `tests/ezkl/test_batch_proofs.rs`
- `tests/ezkl/test_verification.rs`

### Sub-phase 3.2: Model Management

- [ ] Implement model downloading
- [ ] Implement model validation
- [ ] Implement model caching
- [ ] Implement model updates

**Test Files:**

- `tests/models/test_downloading.rs`
- `tests/models/test_validation.rs`
- `tests/models/test_caching.rs`
- `tests/models/test_updates.rs`

### Sub-phase 3.3: Performance Optimization

- [ ] Implement GPU management
- [ ] Implement batching
- [ ] Implement caching
- [ ] Implement load balancing

**Test Files:**

- `tests/performance/test_gpu_management.rs`
- `tests/performance/test_batching.rs`
- `tests/performance/test_caching.rs`
- `tests/performance/test_load_balancing.rs`

### Sub-phase 3.4: Monitoring & Metrics

- [ ] Implement performance metrics
- [ ] Implement health checks
- [ ] Implement alerting
- [ ] Implement dashboards

**Test Files:**

- `tests/monitoring/test_metrics.rs`
- `tests/monitoring/test_health_checks.rs`
- `tests/monitoring/test_alerting.rs`
- `tests/monitoring/test_dashboards.rs`

### Sub-phase 3.5: Advanced Model Features (**NEW**)

- [ ] Implement fine-tuned model support
- [ ] Implement private model hosting
- [ ] Implement GDPR compliance routing
- [ ] Implement model specialization

**Test Files:**

- `tests/models/test_finetuned.rs`
- `tests/models/test_private.rs`
- `tests/models/test_gdpr.rs`
- `tests/models/test_specialization.rs`

## Phase 4: Storage & Caching Integration (NEW)

### Sub-phase 4.1: S5 Storage Layer

- [ ] Create S5 storage module matching Enhanced S5.js API
- [ ] Implement path structure (/models, /results, /proofs)
- [ ] Add CBOR encoding matching s5-rs format
- [ ] Implement chunked upload for large models
- [ ] Add compression support (zstd)

### Sub-phase 4.2: Vector DB Semantic Cache

- [ ] Integrate Vector DB REST client
- [ ] Implement embedding generation for prompts
- [ ] Add semantic similarity threshold (0.95)
- [ ] Create cache-first inference pipeline
- [ ] Add metrics for cache hit rates

### Sub-phase 4.3: Integration Testing

- [ ] Test S5 storage with real Enhanced S5.js
- [ ] Test Vector DB with real deployment
- [ ] Verify CBOR compatibility across systems
- [ ] Performance benchmarks with caching
