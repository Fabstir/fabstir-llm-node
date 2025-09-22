# IMPLEMENTATION.md - Fabstir LLM Node (Rust) - Proof & Security Systems

## Overview
Implementation plan for core security systems in the Rust node: commitment proofs, staking/slashing execution, and reputation calculation.

**Timeline**: 7 days total
**Location**: `fabstir-llm-node/` (Rust project)
**Approach**: TDD, one sub-phase at a time

---

## Phase 1: Commitment-Based Proof System (2 Days)

### Sub-phase 1.1: Proof Generation Module

**Goal**: Implement proof generation in Rust for performance

#### Tasks
- [ ] Write tests for commitment hash generation
- [ ] Write tests for proof structure serialization
- [ ] Write tests for signature generation with ethers-rs
- [ ] Implement keccak256 hashing using tiny-keccak
- [ ] Create ProofGenerator struct with model fingerprinting
- [ ] Implement EIP-712 typed data for proof structure
- [ ] Add proof caching with sled database
- [ ] Create proof queue for batch submission
- [ ] Implement proof expiry tracking
- [ ] Add Prometheus metrics for proof generation

**Test Files:**
- `src/proof/generator.rs` - Tests in `#[cfg(test)]` module (max 300 lines)
- `src/proof/signature.rs` - Tests in module (max 200 lines)
- `tests/proof_integration.rs` (max 250 lines) - Integration tests

**Implementation Files:**
- `src/proof/mod.rs` (max 100 lines) - Module definition
- `src/proof/generator.rs` (max 400 lines) - Proof generation logic
- `src/proof/signature.rs` (max 300 lines) - Signature handling
- `src/proof/types.rs` (max 150 lines) - Proof structures
- `src/proof/storage.rs` (max 250 lines) - Proof caching

### Sub-phase 1.2: Proof Submission Service

**Goal**: Implement efficient proof submission to blockchain

#### Tasks
- [ ] Write tests for Web3 connection management
- [ ] Write tests for gas price optimization
- [ ] Write tests for retry logic
- [ ] Implement Web3 client with ethers-rs
- [ ] Create proof submission queue with tokio channels
- [ ] Implement gas price oracle integration
- [ ] Add transaction nonce management
- [ ] Create batch submission for gas savings
- [ ] Implement exponential backoff for retries
- [ ] Add submission status tracking

**Test Files:**
- `src/blockchain/submitter.rs` - Tests in module (max 300 lines)
- `tests/blockchain_integration.rs` (max 300 lines)

**Implementation Files:**
- `src/blockchain/mod.rs` (max 100 lines) - Module setup
- `src/blockchain/client.rs` (max 350 lines) - Web3 client
- `src/blockchain/submitter.rs` (max 400 lines) - Proof submission
- `src/blockchain/gas_oracle.rs` (max 200 lines) - Gas optimization
- `src/blockchain/contracts/proof_system.rs` (max 150 lines) - Contract bindings

---

## Phase 2: Staking and Slashing Engine (2 Days)

### Sub-phase 2.1: Staking Manager

**Goal**: Implement staking operations and monitoring

#### Tasks
- [ ] Write tests for stake amount tracking
- [ ] Write tests for stake/unstake operations
- [ ] Write tests for stake event monitoring
- [ ] Implement stake amount cache with RwLock
- [ ] Create stake operation executor
- [ ] Add stake event listener with WebSocket
- [ ] Implement minimum stake validation
- [ ] Create unstake timelock tracker
- [ ] Add stake history database (SQLite)
- [ ] Implement stake status reporting

**Test Files:**
- `src/staking/manager.rs` - Tests in module (max 350 lines)
- `src/staking/events.rs` - Tests in module (max 200 lines)

**Implementation Files:**
- `src/staking/mod.rs` (max 100 lines) - Module definition
- `src/staking/manager.rs` (max 400 lines) - Staking logic
- `src/staking/events.rs` (max 300 lines) - Event monitoring
- `src/staking/storage.rs` (max 250 lines) - Stake database
- `src/staking/contracts/staking.rs` (max 150 lines) - Contract bindings

### Sub-phase 2.2: Slashing Detector

**Goal**: Monitor and respond to slashing events

#### Tasks
- [ ] Write tests for slashing event detection
- [ ] Write tests for slash amount calculation
- [ ] Write tests for dispute submission
- [ ] Implement slashing event monitor
- [ ] Create slash reason categorization
- [ ] Add automatic dispute filing logic
- [ ] Implement evidence collection system
- [ ] Create slash impact calculator
- [ ] Add slash notification system
- [ ] Implement recovery strategy after slash

**Test Files:**
- `src/slashing/detector.rs` - Tests in module (max 300 lines)
- `src/slashing/dispute.rs` - Tests in module (max 250 lines)

**Implementation Files:**
- `src/slashing/mod.rs` (max 100 lines) - Module definition
- `src/slashing/detector.rs` (max 350 lines) - Detection logic
- `src/slashing/dispute.rs` (max 300 lines) - Dispute handling
- `src/slashing/evidence.rs` (max 250 lines) - Evidence collection
- `src/slashing/recovery.rs` (max 200 lines) - Recovery strategy

---

## Phase 3: Reputation System (3 Days)

### Sub-phase 3.1: Reputation Calculator

**Goal**: Implement reputation scoring algorithm

#### Tasks
- [ ] Write tests for reputation calculation formula
- [ ] Write tests for reputation decay
- [ ] Write tests for reputation updates
- [ ] Implement base reputation algorithm (0-1000 scale)
- [ ] Create job success/failure impact calculation
- [ ] Add time-based decay function
- [ ] Implement stake-based reputation boost
- [ ] Create reputation history tracking
- [ ] Add reputation checkpoint system
- [ ] Implement percentile ranking

**Test Files:**
- `src/reputation/calculator.rs` - Tests in module (max 400 lines)
- `src/reputation/decay.rs` - Tests in module (max 200 lines)

**Implementation Files:**
- `src/reputation/mod.rs` (max 100 lines) - Module definition
- `src/reputation/calculator.rs` (max 400 lines) - Core algorithm
- `src/reputation/decay.rs` (max 200 lines) - Decay logic
- `src/reputation/history.rs` (max 300 lines) - History tracking
- `src/reputation/storage.rs` (max 250 lines) - Database layer

### Sub-phase 3.2: Reputation Service

**Goal**: Provide reputation data to other components

#### Tasks
- [ ] Write tests for reputation API endpoints
- [ ] Write tests for reputation caching
- [ ] Write tests for reputation aggregation
- [ ] Implement gRPC service for reputation queries
- [ ] Create reputation cache with TTL
- [ ] Add reputation leaderboard calculation
- [ ] Implement reputation threshold checks
- [ ] Create reputation change notifications
- [ ] Add reputation export for analytics
- [ ] Implement reputation recovery paths

**Test Files:**
- `src/reputation/service.rs` - Tests in module (max 350 lines)
- `tests/reputation_integration.rs` (max 300 lines)

**Implementation Files:**
- `src/reputation/service.rs` (max 400 lines) - gRPC service
- `src/reputation/cache.rs` (max 250 lines) - Caching layer
- `src/reputation/aggregator.rs` (max 300 lines) - Aggregation logic
- `src/reputation/proto/reputation.proto` (max 100 lines) - Protocol definition
- `src/reputation/notifications.rs` (max 200 lines) - Change notifications

### Sub-phase 3.3: Performance Monitoring

**Goal**: Track and optimize system performance

#### Tasks
- [ ] Write tests for metrics collection
- [ ] Write tests for performance thresholds
- [ ] Write tests for alert conditions
- [ ] Implement Prometheus metrics exporter
- [ ] Create performance dashboards config
- [ ] Add proof generation time tracking
- [ ] Implement submission success rate monitoring
- [ ] Create reputation calculation benchmarks
- [ ] Add system health checks
- [ ] Implement performance alerts

**Test Files:**
- `src/monitoring/metrics.rs` - Tests in module (max 250 lines)
- `src/monitoring/health.rs` - Tests in module (max 200 lines)

**Implementation Files:**
- `src/monitoring/mod.rs` (max 100 lines) - Module definition
- `src/monitoring/metrics.rs` (max 300 lines) - Metrics collection
- `src/monitoring/health.rs` (max 200 lines) - Health checks
- `src/monitoring/alerts.rs` (max 250 lines) - Alert system
- `config/prometheus.yml` (max 100 lines) - Prometheus config

---

## Cargo.toml Dependencies

```toml
[dependencies]
# Core
tokio = { version = "1.35", features = ["full"] }
async-trait = "0.1"
anyhow = "1.0"
thiserror = "1.0"

# Cryptography
ethers = "2.0"
tiny-keccak = { version = "2.0", features = ["keccak"] }
secp256k1 = "0.28"

# Storage
sled = "0.34"
sqlx = { version = "0.7", features = ["runtime-tokio", "sqlite"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
bincode = "1.3"

# Web3
web3 = "0.19"

# gRPC
tonic = "0.10"
prost = "0.12"
tonic-build = "0.10"

# Monitoring
prometheus = "0.13"
tracing = "0.1"
tracing-subscriber = "0.3"

# Testing
mockito = "1.2"
proptest = "1.4"
```

---

## Smart Contracts Location

If keeping contracts in the Rust project:

**Contract Files:**
- `contracts/src/ProofSystem.sol` (max 250 lines)
- `contracts/src/StakingManager.sol` (max 350 lines)
- `contracts/src/SlashingManager.sol` (max 400 lines)
- `contracts/src/ReputationRegistry.sol` (max 350 lines)
- `contracts/test/` - Foundry tests
- `contracts/script/Deploy.s.sol` - Deployment script

**Build Integration:**
- `build.rs` - Generate Rust bindings from ABIs
- `scripts/deploy.sh` - Deploy contracts and update addresses