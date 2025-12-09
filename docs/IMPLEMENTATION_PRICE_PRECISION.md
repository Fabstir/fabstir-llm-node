# IMPLEMENTATION_PRICE_PRECISION.md - Contract Migration: PRICE_PRECISION=1000

## Overview

Implementation plan to migrate fabstir-llm-node to the new contract deployment with PRICE_PRECISION=1000 support. This breaking change requires updates to contract addresses, pricing constants, and all payment calculation logic.

**Location**: `fabstir-llm-node/` (Rust project)
**Approach**: Strict TDD, bounded autonomy, one sub-phase at a time
**Breaking Change Reference**: `docs/compute-contracts-reference/BREAKING_CHANGES.md`

---

## Contract Changes Summary

### New Contract Addresses (December 9, 2025)

| Contract                 | OLD Address                                  | NEW Address                                  |
| ------------------------ | -------------------------------------------- | -------------------------------------------- |
| JobMarketplaceWithModels | `0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E` | `0x0c942eADAF86855F69Ee4fa7f765bc6466f254A1` |
| NodeRegistryWithModels   | `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6` | `0x48aa4A8047A45862Da8412FAB71ef66C17c7766d` |

### PRICE_PRECISION Changes

All prices are now stored with a **1000x multiplier** to support sub-$1/million token pricing:

| Constant                     | OLD Value          | NEW Value              |
| ---------------------------- | ------------------ | ---------------------- |
| `MIN_PRICE_PER_TOKEN_STABLE` | 10                 | 1                      |
| `MAX_PRICE_PER_TOKEN_STABLE` | 100,000            | 100,000,000            |
| `MIN_PRICE_PER_TOKEN_NATIVE` | 2,272,727,273      | 227,273                |
| `MAX_PRICE_PER_TOKEN_NATIVE` | 22,727,272,727,273 | 22,727,272,727,273,000 |

### Payment Calculation Formula Changes

**OLD:**

```rust
max_tokens = deposit / price_per_token
host_payment = tokens_used * price_per_token
```

**NEW:**

```rust
const PRICE_PRECISION: u64 = 1000;
max_tokens = (deposit * PRICE_PRECISION) / price_per_token
host_payment = (tokens_used * price_per_token) / PRICE_PRECISION
```

---

## Phase 1: Pricing Constants & Core Infrastructure (TDD)

### Sub-phase 1.1: Add PRICE_PRECISION Constant and Update Pricing Constants

**Goal**: Update pricing_constants.rs with PRICE_PRECISION and new MIN/MAX values

#### Tasks

- [ ] Write tests for PRICE_PRECISION constant existence
- [ ] Write tests for new MIN_PRICE_PER_TOKEN_NATIVE validation (227,273)
- [ ] Write tests for new MAX_PRICE_PER_TOKEN_NATIVE validation (22,727,272,727,273,000)
- [ ] Write tests for new MIN_PRICE_PER_TOKEN_STABLE validation (1)
- [ ] Write tests for new MAX_PRICE_PER_TOKEN_STABLE validation (100,000,000)
- [ ] Write tests for default price calculations with PRICE_PRECISION
- [ ] Add PRICE_PRECISION constant (pub const PRICE_PRECISION: u64 = 1000)
- [ ] Update native::MIN_PRICE_PER_TOKEN to 227,273
- [ ] Update native::MAX_PRICE_PER_TOKEN to 22,727,272,727,273,000
- [ ] Update stable::MIN_PRICE_PER_TOKEN to 1
- [ ] Update stable::MAX_PRICE_PER_TOKEN to 100,000,000
- [ ] Update default_price() functions with geometric mean of new ranges
- [ ] Update validate_price() functions with new ranges
- [ ] Add helper functions: `to_precision_format()` and `from_precision_format()`

**Files:**

- `src/contracts/pricing_constants.rs` - Update constants and validation
- Test assertions in `#[cfg(test)]` module

### Sub-phase 1.2: Update Contract Addresses

**Goal**: Update all hardcoded contract addresses to new deployment

#### Tasks

- [ ] Write tests for new JobMarketplace address validation
- [ ] Write tests for new NodeRegistry address validation
- [ ] Update `src/blockchain/chain_config.rs` line 47-48 (hardcoded Base Sepolia addresses)
- [ ] Update `.env.local.test` lines 19, 22 with new addresses
- [ ] Search and replace any other hardcoded old addresses
- [ ] Verify ContractAddresses struct can hold new addresses
- [ ] Run existing contract connection tests to verify connectivity

**Files:**

- `src/blockchain/chain_config.rs` - Update Base Sepolia contract addresses
- `src/config/chains.rs` - Verify env var loading works with new addresses
- `.env.local.test` - Update CONTRACT_JOB_MARKETPLACE and CONTRACT_NODE_REGISTRY

---

## Phase 2: Payment Distribution & Settlement (TDD)

### Sub-phase 2.1: Update Payment Calculation Functions

**Goal**: Fix payment_distribution.rs to use PRICE_PRECISION division

#### Tasks

- [ ] Write test for `calculate_payment_split()` with PRICE_PRECISION division
- [ ] Write test for `calculate_refund()` with PRICE_PRECISION division
- [ ] Write test for edge case: very small token amounts
- [ ] Write test for edge case: very large deposits
- [ ] Write test for precision loss handling (rounding)
- [ ] Update line 131: `total_payment = (price_per_token * tokens_used) / PRICE_PRECISION`
- [ ] Update line 220: `amount_spent = (price_per_token * tokens_used) / PRICE_PRECISION`
- [ ] Import PRICE_PRECISION from pricing_constants module
- [ ] Add overflow protection for large multiplications
- [ ] Document formula change in function comments

**Files:**

- `src/settlement/payment_distribution.rs` - Fix payment calculations
- `tests/settlement/test_payment_distribution.rs` - Update test values

### Sub-phase 2.2: Update Settlement Test Suite

**Goal**: Update all settlement tests to use new pricing format

#### Tasks

- [ ] Update `test_host_earnings_base_sepolia()` line 43 price values
- [ ] Update `test_host_earnings_opbnb()` line 77 price values
- [ ] Update `test_user_refund_calculation()` line 151 price values
- [ ] Update expected payment amounts in assertions
- [ ] Add new tests specifically for PRICE_PRECISION edge cases
- [ ] Verify all settlement tests pass with new values

**Files:**

- `tests/settlement/test_payment_distribution.rs` - Update all test pricing

---

## Phase 3: Job Claim & Verification (TDD)

### Sub-phase 3.1: Fix Job Claim Payment Validation

**Goal**: Rewrite job_claim.rs payment validation for PRICE_PRECISION format

#### Tasks

- [ ] Write test for correct payment_per_token extraction with PRICE_PRECISION
- [ ] Write test for minimum payment threshold validation
- [ ] Write test for rejection of below-minimum jobs
- [ ] Rewrite lines 304-308 payment validation logic
- [ ] OLD: `payment_per_token = payment_amount / max_tokens`
- [ ] NEW: `price_per_token = (deposit * PRICE_PRECISION) / max_tokens`
- [ ] Update `min_payment_per_token` config to use new scale
- [ ] Add PRICE_PRECISION import
- [ ] Update ClaimConfig defaults for new pricing scale

**Files:**

- `src/job_claim.rs` - Fix payment validation (lines 304-308)

### Sub-phase 3.2: Fix Job Verification Payment Amount

**Goal**: Fix job_verification.rs payment_amount calculation

#### Tasks

- [ ] Write test for `convert_job_to_details()` with PRICE_PRECISION
- [ ] Write test for JobDetails.payment_amount accuracy
- [ ] Fix line 419: Add PRICE_PRECISION division
- [ ] OLD: `payment_amount = max_price_per_token * max_tokens`
- [ ] NEW: `payment_amount = (max_price_per_token * max_tokens) / PRICE_PRECISION`
- [ ] Update mock job values in tests (lines 400-412)
- [ ] Add integration test verifying end-to-end payment calculation

**Files:**

- `src/api/websocket/job_verification.rs` - Fix payment_amount calculation (line 419)

---

## Phase 4: Host Registration & Node Pricing (TDD)

### Sub-phase 4.1: Update Registration Pricing Validation

**Goal**: Update host/registration.rs to validate against new constants

#### Tasks

- [ ] Write test for native price validation with new MIN (227,273)
- [ ] Write test for native price validation with new MAX (22,727,272,727,273,000)
- [ ] Write test for stable price validation with new MIN (1)
- [ ] Write test for stable price validation with new MAX (100,000,000)
- [ ] Write test for default price assignment with new defaults
- [ ] Update lines 162-176 to use updated pricing_constants
- [ ] Verify registerNode contract call uses correct ABI format
- [ ] Test registration against new contract address

**Files:**

- `src/host/registration.rs` - Update pricing validation (lines 162-192)

### Sub-phase 4.2: Update Multi-Chain Registrar

**Goal**: Ensure multi_chain_registrar.rs validates pricing correctly

#### Tasks

- [ ] Write test for cross-chain registration with PRICE_PRECISION values
- [ ] Update lines 250-286 to validate against new constants
- [ ] Verify minPricePerTokenNative encoding for new ranges
- [ ] Verify minPricePerTokenStable encoding for new ranges
- [ ] Test registration on Base Sepolia with new contract

**Files:**

- `src/blockchain/multi_chain_registrar.rs` - Update pricing validation

---

## Phase 5: Session Initialization & Token Tracking (TDD)

### Sub-phase 5.1: Session Price Handling

**Goal**: Ensure session initialization handles PRICE_PRECISION correctly

#### Tasks

- [ ] Write test for SessionInitData.price_per_token usage
- [ ] Write test for price display/logging accuracy
- [ ] Review `src/crypto/session_init.rs` price_per_token field (line 39)
- [ ] Review `src/api/server.rs` price_per_token usage (lines 1155-1172)
- [ ] Add documentation comments explaining PRICE_PRECISION
- [ ] Ensure price is displayed correctly to users (divide by 1000 for USD)

**Files:**

- `src/crypto/session_init.rs` - Add price precision documentation
- `src/api/server.rs` - Verify price handling

### Sub-phase 5.2: Token Tracker & Checkpoint Integration

**Goal**: Ensure checkpoint submission uses correct pricing format

#### Tasks

- [ ] Write test for checkpoint payment calculation with PRICE_PRECISION
- [ ] Review `src/api/token_tracker.rs` checkpoint logic (lines 70-108)
- [ ] Review `src/contracts/checkpoint_manager.rs` submitProofOfWork call
- [ ] Verify contract ABI signature matches new contract
- [ ] Add comments documenting PRICE_PRECISION in checkpoint flow
- [ ] Test checkpoint submission against new contract

**Files:**

- `src/api/token_tracker.rs` - Document price precision in checkpoints
- `src/contracts/checkpoint_manager.rs` - Verify contract call compatibility

---

## Phase 6: Documentation & Verification (Final)

### Sub-phase 6.1: Update Project Documentation

**Goal**: Update all documentation to reflect new contract addresses and pricing

#### Tasks

- [ ] Update `docs/NODE_DUAL_PRICING_UPDATE_v7.0.29.md` with PRICE_PRECISION
- [ ] Add migration notes for any hosts using old contracts
- [ ] Document formula changes in API documentation

### Sub-phase 6.2: Integration Testing & Verification

**Goal**: Verify complete system works with new contracts

#### Tasks

- [ ] Run full test suite: `cargo test --lib`
- [ ] Run integration tests: `cargo test --test integration_tests`
- [ ] Run contract tests: `cargo test --test contracts_tests`
- [ ] Test host registration against new NodeRegistry contract
- [ ] Test session creation against new JobMarketplace contract
- [ ] Test proof submission against new contract
- [ ] Test payment settlement end-to-end
- [ ] Verify no regressions in existing functionality
- [ ] Build release binary: `cargo build --release --features real-ezkl -j 4`

**Files:**

- All test files
- Build verification

---

## Critical Files Summary

| File                                            | Changes Required                                  |
| ----------------------------------------------- | ------------------------------------------------- |
| `src/contracts/pricing_constants.rs`            | Add PRICE_PRECISION, update all MIN/MAX constants |
| `src/settlement/payment_distribution.rs`        | Fix payment calculations with /1000 division      |
| `src/job_claim.rs`                              | Rewrite payment validation logic (lines 304-308)  |
| `src/api/websocket/job_verification.rs`         | Fix payment_amount calculation (line 419)         |
| `src/host/registration.rs`                      | Update pricing validation ranges                  |
| `src/blockchain/chain_config.rs`                | Update hardcoded contract addresses               |
| `src/blockchain/multi_chain_registrar.rs`       | Update pricing validation                         |
| `.env.local.test`                               | Update CONTRACT\_\* addresses                     |
| `tests/settlement/test_payment_distribution.rs` | Update test values                                |

---

## Verification Checklist

After implementation, verify:

- [ ] `cargo test --lib` passes
- [ ] `cargo test --test contracts_tests` passes
- [ ] `cargo test --test settlement_tests` passes
- [ ] Host can register with new NodeRegistry contract
- [ ] Session creation works with new JobMarketplace contract
- [ ] Payment calculations produce correct USD values
- [ ] Checkpoint submission succeeds
- [ ] Settlement distributes correct amounts to host (90%) and treasury (10%)

---

## Rollback Plan

If issues arise:

1. Revert contract addresses to old values in `.env.local.test`
2. Revert `pricing_constants.rs` to old MIN/MAX values
3. Remove PRICE_PRECISION division from payment calculations
4. Old contracts remain functional during transition period

---

## References

- `docs/compute-contracts-reference/BREAKING_CHANGES.md` - Full migration guide
- `docs/compute-contracts-reference/API_REFERENCE.md` - New contract API
- `docs/compute-contracts-reference/CONTRACT_ADDRESSES.md` - Address reference
- `docs/compute-contracts-reference/client-abis/` - Updated ABIs
