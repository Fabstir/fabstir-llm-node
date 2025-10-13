# IMPLEMENTATION-EZKL.md - Fabstir LLM Node - Real EZKL Proof System

## Overview
Implementation plan for replacing mock EZKL proofs with real commitment-based zero-knowledge proofs using the EZKL library. This provides cryptographic verification of inference results for 20B+ parameter LLM models without requiring full computation proving.

**Timeline**: 7 days total
**Location**: `fabstir-llm-node/` (Rust project)
**Approach**: TDD with bounded autonomy, one sub-phase at a time
**Proof Type**: Commitment-based (proves hash relationships, not full inference)

---

## Phase 1: EZKL Setup and Circuit Design (2 Days)

### Sub-phase 1.1: EZKL Dependencies and Environment Setup

**Goal**: Add EZKL library and verify basic functionality

#### Tasks
- [x] Research latest stable EZKL crate version (v22.x or newer) - v22.3.0
- [x] Write test for EZKL library availability check
- [x] Write test for basic EZKL circuit compilation
- [x] Add EZKL to Cargo.toml with feature flag
- [x] Add required crypto dependencies (halo2, ark-std)
- [x] Create feature flag `real-ezkl` (default off)
- [x] Create EZKL config module with environment variables
- [x] Implement EZKL availability check function
- [x] Add conditional compilation for mock vs real
- [x] Verify builds work with and without `real-ezkl` feature
- [x] Document EZKL installation requirements - requires nightly Rust

**Test Files:**
- `tests/ezkl/test_ezkl_availability.rs` (max 150 lines) - Library availability tests
- `tests/ezkl/test_basic_circuit.rs` (max 250 lines) - Basic circuit compilation tests

**Implementation Files:**
- `src/crypto/ezkl/mod.rs` (max 100 lines) - Module definition and feature flags
- `src/crypto/ezkl/config.rs` (max 200 lines) - Configuration from environment
- `src/crypto/ezkl/availability.rs` (max 150 lines) - Version and feature checks

**Environment Variables:**
```bash
ENABLE_REAL_EZKL=true               # Enable real EZKL (default: false, uses mock)
EZKL_PROVING_KEY_PATH=./keys/pk.key
EZKL_VERIFYING_KEY_PATH=./keys/vk.key
EZKL_CIRCUIT_PATH=./circuits/commitment.circuit
EZKL_MAX_PROOF_SIZE=10000           # Bytes
```

### Sub-phase 1.2: Commitment Circuit Design

**Goal**: Design and implement simple commitment circuit for hash relationships

#### Tasks
- [ ] Write test for circuit input structure (4 hash fields)
- [ ] Write test for circuit constraint correctness
- [ ] Write test for witness generation from hashes
- [ ] Design commitment circuit specification
  - Input: job_id (bytes32)
  - Input: model_hash (bytes32)
  - Input: input_hash (bytes32)
  - Input: output_hash (bytes32)
  - Constraint: Bind all hashes together
  - Constraint: Verify hash format (32 bytes each)
- [ ] Implement circuit struct with EZKL annotations
- [ ] Implement witness data builder
- [ ] Create circuit compilation function
- [ ] Generate proving key (one-time setup)
- [ ] Generate verification key (one-time setup)
- [ ] Store keys in `/keys` directory
- [ ] Test circuit with sample data
- [ ] Verify constraints are satisfiable
- [ ] Document circuit design and security properties

**Test Files:**
- `tests/ezkl/test_commitment_circuit.rs` (max 350 lines) - Circuit design tests
- `tests/ezkl/test_witness_generation.rs` (max 250 lines) - Witness builder tests
- `tests/ezkl/test_circuit_constraints.rs` (max 300 lines) - Constraint satisfaction tests

**Implementation Files:**
- `src/crypto/ezkl/circuit.rs` (max 400 lines) - Commitment circuit definition
- `src/crypto/ezkl/witness.rs` (max 300 lines) - Witness data builder
- `src/crypto/ezkl/setup.rs` (max 250 lines) - Key generation and circuit compilation
- `scripts/generate_ezkl_keys.sh` (max 100 lines) - Setup script for key generation

**Circuit Structure:**
```rust
// Commitment circuit (simplified)
pub struct CommitmentCircuit {
    pub job_id: [u8; 32],
    pub model_hash: [u8; 32],
    pub input_hash: [u8; 32],
    pub output_hash: [u8; 32],
}

// What we prove:
// - I know these 4 hash values
// - They are correctly formatted (32 bytes each)
// - They are cryptographically bound together in this proof
// This prevents:
// - Swapping hashes between jobs
// - Claiming someone else's output
// - Tampering with result after generation
```

---

## Phase 2: Real Proof Generation Implementation (2 Days)

### Sub-phase 2.1: Replace Mock ProofGenerator

**Goal**: Replace mock proof generation in `src/results/proofs.rs` with real EZKL

#### Tasks
- [ ] Write test for real EZKL proof structure validation
- [ ] Write test for proof generation with valid inputs
- [ ] Write test for proof generation error handling
- [ ] Write test for proof determinism (same input → same proof structure)
- [ ] Update ProofGenerator to conditionally use real EZKL
- [ ] Implement real EZKL proof generation for ProofType::EZKL
- [ ] Create witness from InferenceResult hashes
- [ ] Call EZKL prove function with witness and keys
- [ ] Handle EZKL errors and map to CryptoError
- [ ] Update proof size validation (real proofs are larger)
- [ ] Update timestamp and metadata in proof structure
- [ ] Test proof generation with various input sizes
- [ ] Verify proof data is non-empty and valid
- [ ] Update all existing tests to handle real proof sizes
- [ ] Add timeout tests for proof generation (max 5 seconds)

**Test Files:**
- `tests/ezkl/test_real_proof_generation.rs` (max 400 lines) - Real EZKL proof tests
- `tests/results/test_proofs_real_ezkl.rs` (max 350 lines) - Integration with results
- `tests/ezkl/test_proof_errors.rs` (max 250 lines) - Error handling tests

**Implementation Files:**
- `src/results/proofs.rs` (EDIT, lines 60-91) - Replace mock with real EZKL
- `src/crypto/ezkl/prover.rs` (max 400 lines) - Real EZKL proving logic
- `src/crypto/ezkl/error.rs` (max 200 lines) - EZKL-specific error types

**Key Changes to `src/results/proofs.rs`:**
```rust
// OLD (lines 72-84):
ProofType::EZKL => {
    // Simulate EZKL proof generation
    let mut proof = vec![0xEF; 200]; // Mock EZKL proof header
    proof.extend_from_slice(model_hash.as_bytes());
    // ... mock implementation
}

// NEW:
ProofType::EZKL => {
    #[cfg(feature = "real-ezkl")]
    {
        // Real EZKL proof generation
        use crate::crypto::ezkl::{create_witness, generate_proof};

        let witness = create_witness(
            result.job_id.as_bytes(),
            &model_hash,
            &input_hash,
            &output_hash
        )?;

        generate_proof(&witness, &self.config.proving_key_path)?
    }
    #[cfg(not(feature = "real-ezkl"))]
    {
        // Mock fallback for development
        let mut proof = vec![0xEF; 200];
        // ... existing mock
    }
}
```

### Sub-phase 2.2: Key Management and Performance

**Goal**: Implement efficient key loading and proof caching

#### Tasks
- [ ] Write test for proving key loading from file
- [ ] Write test for proving key caching in memory
- [ ] Write test for key validation on load
- [ ] Write test for proof caching with LRU eviction
- [ ] Write test for cache hit rate metrics
- [ ] Implement key loader with file reading
- [ ] Add key validation (check format and size)
- [ ] Create in-memory key cache with Arc<RwLock>
- [ ] Implement lazy key loading on first use
- [ ] Add proof result caching (same inputs → cached proof)
- [ ] Implement LRU eviction for proof cache
- [ ] Add Prometheus metrics for cache hits/misses
- [ ] Add metrics for proof generation duration
- [ ] Test concurrent proof generation with shared keys
- [ ] Optimize witness serialization
- [ ] Profile proof generation bottlenecks

**Test Files:**
- `tests/ezkl/test_key_management.rs` (max 350 lines) - Key loading and caching
- `tests/ezkl/test_proof_caching.rs` (max 300 lines) - Proof cache tests
- `tests/ezkl/test_performance.rs` (max 250 lines) - Performance benchmarks

**Implementation Files:**
- `src/crypto/ezkl/key_manager.rs` (max 400 lines) - Key loading and caching
- `src/crypto/ezkl/cache.rs` (max 350 lines) - Proof caching with LRU
- `src/crypto/ezkl/metrics.rs` (max 200 lines) - Prometheus metrics

**Performance Targets:**
- Proof generation: < 100ms on modern CPU
- Key loading: < 50ms (cached in memory after first load)
- Witness generation: < 5ms
- Memory usage: < 500MB for keys + cache
- Cache hit rate: > 80% for repeated inferences
- Concurrent proving: Support 10+ parallel proof generations

---

## Phase 3: Verification and Integration (2 Days)

### Sub-phase 3.1: Real Proof Verification

**Goal**: Implement real EZKL proof verification

#### Tasks
- [ ] Write test for valid proof verification
- [ ] Write test for tampered proof detection
- [ ] Write test for wrong hash detection
- [ ] Write test for verification key loading
- [ ] Write test for verification key caching
- [ ] Implement EZKL verify function wrapper
- [ ] Load verification key from file/environment
- [ ] Cache verification key in memory
- [ ] Update verify_proof() in ProofGenerator (line 125)
- [ ] Call real EZKL verification API
- [ ] Handle verification errors gracefully
- [ ] Add verification metrics (success/failure counts)
- [ ] Test verification with correct proofs
- [ ] Test verification rejects invalid proofs
- [ ] Test verification rejects tampered hashes
- [ ] Benchmark verification performance (target: < 10ms)

**Test Files:**
- `tests/ezkl/test_verification.rs` (EDIT, expand) - Real verification tests
- `tests/ezkl/test_tamper_detection.rs` (max 300 lines) - Tamper detection
- `tests/ezkl/test_verification_performance.rs` (max 200 lines) - Performance tests

**Implementation Files:**
- `src/crypto/ezkl/verifier.rs` (max 350 lines) - Real EZKL verification
- `src/results/proofs.rs` (EDIT, lines 125-158) - Update verify_proof()

**Updated `verify_proof()` Logic:**
```rust
pub async fn verify_proof(&self, proof: &InferenceProof, result: &InferenceResult) -> Result<bool> {
    // Recompute hashes (lines 131-133)
    let model_hash = self.compute_data_hash(self.config.model_path.as_bytes());
    let input_hash = self.compute_data_hash(result.prompt.as_bytes());
    let output_hash = self.compute_data_hash(result.response.as_bytes());

    // Check hash equality (lines 136-141)
    if proof.model_hash != model_hash || proof.input_hash != input_hash || proof.output_hash != output_hash {
        return Ok(false);
    }

    // Verify based on proof type (lines 144-157)
    match proof.proof_type {
        ProofType::EZKL => {
            #[cfg(feature = "real-ezkl")]
            {
                use crate::crypto::ezkl::verify_proof as ezkl_verify;

                // Call real EZKL verification
                ezkl_verify(
                    &proof.proof_data,
                    &self.config.verifying_key_path,
                    &[&proof.model_hash, &proof.input_hash, &proof.output_hash]
                ).await
            }
            #[cfg(not(feature = "real-ezkl"))]
            {
                // Mock fallback
                Ok(proof.proof_data.len() >= 200 && proof.proof_data[0] == 0xEF)
            }
        }
        // ... other types
    }
}
```

### Sub-phase 3.2: Payment System Integration

**Goal**: Integrate proofs with checkpoint submission and payment flow

#### Tasks
- [ ] Write test for checkpoint with proof submission
- [ ] Write test for payment release with valid proof
- [ ] Write test for payment rejection with invalid proof
- [ ] Write test for proof validation in settlement
- [ ] Update checkpoint submission to include proof data
- [ ] Add proof validation before payment release
- [ ] Integrate with submitProofOfWork contract function
- [ ] Add proof data to on-chain submission
- [ ] Create proof verification before settlement
- [ ] Add proof storage in database for auditing
- [ ] Test end-to-end: inference → proof → payment
- [ ] Test proof rejection prevents payment
- [ ] Add metrics for proof validation success/failure
- [ ] Document proof requirements for payment

**Test Files:**
- `tests/integration/test_proof_payment_flow.rs` (max 400 lines) - End-to-end flow
- `tests/checkpoint/test_checkpoint_with_proof.rs` (max 350 lines) - Checkpoint integration
- `tests/settlement/test_settlement_validation.rs` (max 300 lines) - Settlement with proofs

**Implementation Files:**
- `src/checkpoint/submission.rs` (EDIT) - Add proof to checkpoint
- `src/settlement/validator.rs` (max 300 lines) - Proof validation logic
- `src/settlement/auto_settle.rs` (EDIT) - Integrate proof validation
- `src/blockchain/contracts/proof_submission.rs` (max 250 lines) - On-chain proof submission

**Integration Points:**

1. **Checkpoint Submission** (`src/checkpoint/submission.rs`):
```rust
// Add proof to checkpoint data
pub async fn submit_checkpoint_with_proof(
    &self,
    job_id: u64,
    tokens_processed: u64,
    proof: InferenceProof,
) -> Result<()> {
    // Generate commitment hash from proof
    let commitment = self.compute_proof_commitment(&proof)?;

    // Submit to contract with proof hash
    self.contract.submit_proof_of_work(
        job_id,
        tokens_processed,
        commitment,  // On-chain proof commitment
        proof.timestamp.timestamp() as u64,
    ).await?;

    // Store full proof off-chain for later verification
    self.store_proof_data(job_id, &proof).await?;

    Ok(())
}
```

2. **Settlement Validation** (`src/settlement/validator.rs`):
```rust
pub async fn validate_before_settlement(
    &self,
    job_id: u64,
) -> Result<bool> {
    // Retrieve stored proof
    let proof = self.retrieve_proof(job_id).await?;

    // Retrieve original inference result
    let result = self.retrieve_result(job_id).await?;

    // Verify proof against result
    let is_valid = self.proof_generator.verify_proof(&proof, &result).await?;

    if !is_valid {
        warn!("❌ Proof verification failed for job {}", job_id);
        return Ok(false);
    }

    info!("✅ Proof verified for job {}", job_id);
    Ok(true)
}
```

---

## Phase 4: Testing and Production Readiness (1 Day)

### Sub-phase 4.1: Comprehensive Testing

**Goal**: End-to-end testing with real proofs

#### Tasks
- [ ] Write test for complete inference → proof → payment flow
- [ ] Write test for concurrent proof generation (10+ parallel)
- [ ] Write test for proof generation under load
- [ ] Write test for error recovery (key missing, corruption)
- [ ] Write test for cache behavior under memory pressure
- [ ] Run all existing tests with `real-ezkl` feature enabled
- [ ] Update test expectations for real proof sizes (2-10KB vs 200 bytes)
- [ ] Update test timeouts for real proof generation (5s vs instant)
- [ ] Create integration test with mock contracts
- [ ] Test proof validation in settlement flow
- [ ] Test dispute scenario with invalid proof
- [ ] Benchmark proof generation under load
- [ ] Profile memory usage with real proofs
- [ ] Test key rotation scenario
- [ ] Test graceful degradation (fallback to mock if EZKL fails)

**Test Files:**
- `tests/integration/test_ezkl_end_to_end.rs` (max 500 lines) - Full E2E tests
- `tests/performance/test_ezkl_load.rs` (max 350 lines) - Load testing
- `tests/integration/test_proof_dispute.rs` (max 300 lines) - Dispute scenarios
- `tests/ezkl/test_error_recovery.rs` (max 250 lines) - Error handling

**Performance Benchmarks:**
```bash
# Run with real EZKL feature
cargo test --features real-ezkl --test test_ezkl_end_to_end

# Load test
cargo test --features real-ezkl --test test_ezkl_load -- --nocapture

# Benchmark proof generation
cargo bench --features real-ezkl --bench ezkl_benchmarks
```

**Expected Results:**
- All 43 existing proof tests pass with real EZKL
- Proof generation: 10-100ms (target: < 50ms avg)
- Verification: < 10ms
- Concurrent proving: 10+ parallel with no degradation
- Memory: < 1GB for keys + cache
- Cache hit rate: > 80% for typical workload

### Sub-phase 4.2: Production Readiness and Documentation

**Goal**: Prepare for production deployment

#### Tasks
- [ ] Add Prometheus metrics dashboard config
- [ ] Create alert rules for proof generation failures
- [ ] Add logging for proof generation events
- [ ] Document proof generation flow with diagrams
- [ ] Create deployment guide for EZKL setup
- [ ] Document key generation procedure
- [ ] Create troubleshooting guide for common issues
- [ ] Add environment variable documentation
- [ ] Create migration guide from mock to real EZKL
- [ ] Update API documentation with proof fields
- [ ] Create circuit specification document
- [ ] Document security assumptions and guarantees
- [ ] Add example proof verification for clients
- [ ] Create monitoring runbook
- [ ] Test deployment on staging environment

**Documentation Files:**
- `docs/EZKL_DEPLOYMENT_GUIDE.md` (max 500 lines) - Complete deployment guide
- `docs/EZKL_CIRCUIT_SPEC.md` (max 300 lines) - Circuit design and security
- `docs/EZKL_TROUBLESHOOTING.md` (max 400 lines) - Common issues and solutions
- `docs/EZKL_API.md` (max 250 lines) - API changes and proof formats

**Monitoring and Alerts:**
```yaml
# prometheus_alerts.yml
groups:
  - name: ezkl_proofs
    interval: 30s
    rules:
      - alert: EZKLProofGenerationFailure
        expr: rate(ezkl_proof_generation_errors[5m]) > 0.1
        annotations:
          summary: "High EZKL proof generation failure rate"

      - alert: EZKLProofGenerationSlow
        expr: histogram_quantile(0.95, ezkl_proof_generation_duration_seconds) > 0.5
        annotations:
          summary: "EZKL proof generation is slow (p95 > 500ms)"

      - alert: EZKLCacheMissRateHigh
        expr: ezkl_cache_miss_rate > 0.5
        annotations:
          summary: "EZKL proof cache miss rate too high"
```

**Deployment Checklist:**
- [ ] Generate proving and verification keys
- [ ] Store keys securely (encrypted at rest)
- [ ] Set environment variables for key paths
- [ ] Enable `real-ezkl` feature in production builds
- [ ] Test proof generation on production hardware
- [ ] Verify proof verification works for clients
- [ ] Set up monitoring and alerts
- [ ] Create rollback plan to mock EZKL if needed
- [ ] Document key backup and recovery procedure
- [ ] Test key rotation procedure

---

## Cargo.toml Dependency Changes

### Add EZKL and Supporting Libraries
```toml
[dependencies]
# Existing dependencies...

# Real EZKL Integration (Phase 1)
ezkl = { version = "22.3", optional = true }
halo2_proofs = { version = "0.3", optional = true }
ark-std = { version = "0.4", optional = true }
ark-ff = { version = "0.4", optional = true }
ark-serialize = { version = "0.4", optional = true }

# Already present (used by EZKL)
sha2 = "0.10"
blake3 = "1.5"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[features]
default = ["inference"]
inference = []
real-ezkl = ["ezkl", "halo2_proofs", "ark-std", "ark-ff", "ark-serialize"]

[dev-dependencies]
# For benchmarking EZKL performance
criterion = "0.5"

[[bench]]
name = "ezkl_benchmarks"
harness = false
required-features = ["real-ezkl"]
```

---

## File Structure After Implementation

```
fabstir-llm-node/
├── src/
│   ├── crypto/
│   │   ├── ezkl/
│   │   │   ├── mod.rs                    # Module definition, feature flags
│   │   │   ├── config.rs                 # Environment configuration
│   │   │   ├── availability.rs           # Library availability checks
│   │   │   ├── circuit.rs                # Commitment circuit definition
│   │   │   ├── witness.rs                # Witness data builder
│   │   │   ├── setup.rs                  # Key generation
│   │   │   ├── prover.rs                 # Real proof generation
│   │   │   ├── verifier.rs               # Real proof verification
│   │   │   ├── key_manager.rs            # Key loading and caching
│   │   │   ├── cache.rs                  # Proof caching with LRU
│   │   │   ├── metrics.rs                # Prometheus metrics
│   │   │   └── error.rs                  # EZKL-specific errors
│   ├── results/
│   │   └── proofs.rs                     # UPDATED: Real EZKL integration
│   ├── checkpoint/
│   │   └── submission.rs                 # UPDATED: Include proofs
│   ├── settlement/
│   │   ├── validator.rs                  # NEW: Proof validation
│   │   └── auto_settle.rs                # UPDATED: Proof integration
│   └── blockchain/
│       └── contracts/
│           └── proof_submission.rs       # NEW: On-chain proof submission
├── tests/
│   ├── ezkl/
│   │   ├── test_ezkl_availability.rs     # Library checks
│   │   ├── test_basic_circuit.rs         # Basic circuit tests
│   │   ├── test_commitment_circuit.rs    # Circuit design tests
│   │   ├── test_witness_generation.rs    # Witness builder tests
│   │   ├── test_circuit_constraints.rs   # Constraint tests
│   │   ├── test_real_proof_generation.rs # Real proof generation
│   │   ├── test_proof_errors.rs          # Error handling
│   │   ├── test_key_management.rs        # Key loading/caching
│   │   ├── test_proof_caching.rs         # Proof cache tests
│   │   ├── test_performance.rs           # Performance benchmarks
│   │   ├── test_verification.rs          # UPDATED: Real verification
│   │   ├── test_tamper_detection.rs      # Tamper detection
│   │   ├── test_verification_performance.rs # Verification benchmarks
│   │   └── test_error_recovery.rs        # Error handling
│   ├── integration/
│   │   ├── test_proof_payment_flow.rs    # End-to-end payment flow
│   │   ├── test_ezkl_end_to_end.rs       # Full E2E tests
│   │   └── test_proof_dispute.rs         # Dispute scenarios
│   ├── performance/
│   │   └── test_ezkl_load.rs             # Load testing
│   ├── checkpoint/
│   │   └── test_checkpoint_with_proof.rs # Checkpoint integration
│   ├── settlement/
│   │   └── test_settlement_validation.rs # Settlement with proofs
│   └── results/
│       └── test_proofs_real_ezkl.rs      # Results integration
├── benches/
│   └── ezkl_benchmarks.rs                # Criterion benchmarks
├── keys/
│   ├── .gitignore                        # Don't commit keys!
│   ├── proving_key.bin                   # Generated proving key
│   └── verifying_key.bin                 # Generated verification key
├── circuits/
│   └── commitment.circuit                # Compiled circuit
├── scripts/
│   └── generate_ezkl_keys.sh             # Key generation script
└── docs/
    ├── IMPLEMENTATION-EZKL.md            # THIS FILE
    ├── EZKL_DEPLOYMENT_GUIDE.md          # Deployment guide
    ├── EZKL_CIRCUIT_SPEC.md              # Circuit specification
    ├── EZKL_TROUBLESHOOTING.md           # Troubleshooting guide
    └── EZKL_API.md                       # API documentation
```

---

## Migration Path from Mock to Real EZKL

### Development Phase (Weeks 1-2)
1. **Day 1-2**: Phase 1 - Setup and circuit design
2. **Day 3-4**: Phase 2 - Real proof generation
3. **Day 5-6**: Phase 3 - Verification and integration
4. **Day 7**: Phase 4 - Testing and documentation

### Testing Phase (Week 3)
1. Run all tests with `real-ezkl` feature in CI/CD
2. Performance testing on target hardware
3. Load testing with concurrent proofs
4. Security audit of circuit design

### Staging Deployment (Week 4)
1. Deploy to staging environment
2. Generate production keys
3. Test end-to-end with staging contracts
4. Monitor performance and errors

### Production Rollout (Week 5+)
1. Deploy with `real-ezkl` disabled initially
2. Enable `real-ezkl` for 10% of proofs (canary)
3. Monitor metrics and errors closely
4. Gradually increase to 100% if stable
5. Keep mock as fallback for emergency

---

## Security Considerations

### What Real EZKL Proofs Guarantee
✅ **Cryptographically proven**:
- The node knows the preimages of all 4 hashes (job_id, model, input, output)
- These hashes are bound together in this specific proof
- The proof cannot be forged or replayed for different jobs

✅ **Attack prevention**:
- Cannot swap output from another inference
- Cannot claim another host's work
- Cannot modify result after generation
- Cannot reuse proof for different job

### What Real EZKL Proofs Do NOT Guarantee
❌ **Not proven**:
- That the LLM inference was actually performed
- That output was correctly computed from input
- That the specified model was actually used
- That the computation followed the model's algorithm

### Additional Security Layers Needed
These are covered in `docs/IMPLEMENTATION_PROOF.md`:
1. **Economic Security**: Staking and slashing (Phase 2)
2. **Reputation System**: Track host performance (Phase 3)
3. **Spot Verification**: Random client checks (future)
4. **Dispute Resolution**: Arbitration for contested results (future)

---

## Performance Targets

### Proof Generation (Phase 2)
- **Target**: < 50ms per proof (p50)
- **Acceptable**: < 100ms per proof (p95)
- **Maximum**: < 500ms per proof (p99)

### Proof Verification (Phase 3)
- **Target**: < 5ms per proof (p50)
- **Acceptable**: < 10ms per proof (p95)

### Memory Usage (Phase 2)
- **Proving key**: ~100-300 MB in memory (cached)
- **Verification key**: ~10-50 MB in memory (cached)
- **Proof cache**: 100-500 MB (configurable LRU)
- **Total**: < 1 GB for proof system

### Throughput (Phase 4)
- **Sequential**: 20-100 proofs/second
- **Parallel**: 100-1000 proofs/second (10+ cores)
- **Cache hit**: 10,000+ proofs/second (cached results)

### Proof Size (Phase 2)
- **SNARK proof**: 2-10 KB per proof
- **vs Mock**: 200 bytes (mock) → 2-10 KB (real)
- **Network**: Acceptable for WebSocket transmission

---

## Success Criteria

### Phase 1 Complete
- [ ] EZKL library compiles and links successfully
- [ ] Commitment circuit compiles without errors
- [ ] Proving and verification keys generated
- [ ] All Phase 1 tests passing (8 tests)

### Phase 2 Complete
- [ ] Real EZKL proofs generated successfully
- [ ] Proof generation < 100ms (p95)
- [ ] Key management and caching functional
- [ ] All Phase 2 tests passing (15 tests)
- [ ] All existing proof tests still passing with real EZKL

### Phase 3 Complete
- [ ] Real EZKL verification working
- [ ] Verification < 10ms per proof
- [ ] Payment flow integrated with proofs
- [ ] All Phase 3 tests passing (12 tests)
- [ ] End-to-end inference → payment flow working

### Phase 4 Complete
- [ ] All 43+ tests passing with real EZKL
- [ ] Performance benchmarks meet targets
- [ ] Load testing successful (10+ concurrent)
- [ ] Documentation complete
- [ ] Monitoring and alerts configured
- [ ] Staging deployment successful

### Production Ready
- [ ] Security audit of circuit design passed
- [ ] 1 week of stable staging operation
- [ ] Rollback plan tested
- [ ] On-call runbook complete
- [ ] Client SDK updated for proof verification

---

## Troubleshooting Guide (Quick Reference)

### Common Issues

**"EZKL proof generation failed"**
- Check proving key exists at EZKL_PROVING_KEY_PATH
- Verify key file permissions (readable)
- Check memory availability (need ~500MB)
- Review logs for specific EZKL error

**"Proof verification failed"**
- Verify verification key matches proving key
- Check hash values are correctly computed
- Ensure proof wasn't corrupted in transit
- Verify EZKL library version matches

**"Proof generation too slow (> 500ms)"**
- Check CPU usage (proof generation is CPU-bound)
- Verify keys are cached in memory (not reloading)
- Check witness serialization isn't bottleneck
- Consider hardware upgrade if consistently slow

**"Out of memory during proof generation"**
- Reduce proof cache size (PROOF_CACHE_SIZE)
- Check for memory leaks in key management
- Ensure old proofs are being evicted from cache
- Increase server memory allocation

Full troubleshooting guide: `docs/EZKL_TROUBLESHOOTING.md`

---

## Next Steps After EZKL Implementation

### Immediate (Post-MVP)
1. **Smart Contract Integration**: On-chain proof verification
2. **Batch Proving**: Generate proofs for multiple inferences in parallel
3. **Proof Compression**: Reduce proof size for network efficiency

### Medium Term (3-6 months)
1. **Recursive Proofs**: Prove batches of proofs for aggregation
2. **Hardware Acceleration**: GPU support for faster proving
3. **Client Verification**: SDK for client-side proof verification

### Long Term (6-12 months)
1. **Full Inference Proving**: Explore zkVM for full LLM proving as tech matures
2. **Privacy Features**: Zero-knowledge inference (hide input/output)
3. **Cross-chain Verification**: Verify proofs on multiple chains

---

## References

- **EZKL Documentation**: https://docs.ezkl.xyz/
- **EZKL GitHub**: https://github.com/zkonduit/ezkl
- **Halo2 Book**: https://zcash.github.io/halo2/
- **IMPLEMENTATION_PROOF.md**: Staking and slashing implementation
- **CHECKPOINT_IMPLEMENTATION_GUIDE.md**: Checkpoint submission details
- **NODE_ENCRYPTION_GUIDE.md**: Encryption implementation reference
