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

### Sub-phase 1.1: Add PRICE_PRECISION Constant and Update Pricing Constants ✅ COMPLETE

**Goal**: Update pricing_constants.rs with PRICE_PRECISION and new MIN/MAX values

#### Tasks

- [x] Write tests for PRICE_PRECISION constant existence
- [x] Write tests for new MIN_PRICE_PER_TOKEN_NATIVE validation (227,273)
- [x] Write tests for new MAX_PRICE_PER_TOKEN_NATIVE validation (22,727,272,727,273,000)
- [x] Write tests for new MIN_PRICE_PER_TOKEN_STABLE validation (1)
- [x] Write tests for new MAX_PRICE_PER_TOKEN_STABLE validation (100,000,000)
- [x] Write tests for default price calculations with PRICE_PRECISION
- [x] Add PRICE_PRECISION constant (pub const PRICE_PRECISION: u64 = 1000)
- [x] Update native::MIN_PRICE_PER_TOKEN to 227,273
- [x] Update native::MAX_PRICE_PER_TOKEN to 22,727,272,727,273,000
- [x] Update stable::MIN_PRICE_PER_TOKEN to 1
- [x] Update stable::MAX_PRICE_PER_TOKEN to 100,000,000
- [x] Update default_price() functions with geometric mean of new ranges
- [x] Update validate_price() functions with new ranges
- [x] Add helper functions: `to_precision_format()` and `from_precision_format()`

**Files:**

- `src/contracts/pricing_constants.rs` - Update constants and validation
- Test assertions in `#[cfg(test)]` module

**Test Results:** 20 tests passed (December 9, 2025)

### Sub-phase 1.2: Update Contract Addresses ✅ COMPLETE

**Goal**: Update all hardcoded contract addresses to new deployment

#### Tasks

- [x] Write tests for new JobMarketplace address validation
- [x] Write tests for new NodeRegistry address validation
- [x] Update `src/blockchain/chain_config.rs` line 47-48 (hardcoded Base Sepolia addresses)
- [x] Update `.env.local.test` lines 19, 22 with new addresses
- [x] Search and replace any other hardcoded old addresses
- [x] Verify ContractAddresses struct can hold new addresses
- [x] Run existing contract connection tests to verify connectivity

**Files:**

- `src/blockchain/chain_config.rs` - Update Base Sepolia contract addresses
- `.env.local.test` - Update CONTRACT_JOB_MARKETPLACE and CONTRACT_NODE_REGISTRY

**Test Results:** 7 tests passed (December 9, 2025)

---

## Phase 2: Payment Distribution & Settlement (TDD) ✅ COMPLETE

### Sub-phase 2.1: Update Payment Calculation Functions ✅ COMPLETE

**Goal**: Fix payment_distribution.rs to use PRICE_PRECISION division

#### Tasks

- [x] Write test for `calculate_payment_split()` with PRICE_PRECISION division
- [x] Write test for `calculate_refund()` with PRICE_PRECISION division
- [x] Write test for edge case: very small token amounts
- [x] Write test for edge case: very large deposits
- [x] Write test for precision loss handling (rounding)
- [x] Update line 131: `total_payment = (price_per_token * tokens_used) / PRICE_PRECISION`
- [x] Update line 220: `amount_spent = (price_per_token * tokens_used) / PRICE_PRECISION`
- [x] Import PRICE_PRECISION from pricing_constants module
- [x] Add overflow protection for large multiplications (U256 handles this)
- [x] Document formula change in function comments

**Files:**

- `src/settlement/payment_distribution.rs` - Fixed payment calculations
- `tests/settlement/test_payment_distribution.rs` - Added 6 new PRICE_PRECISION tests

**Test Results:** 6 new tests passed (December 9, 2025)

### Sub-phase 2.2: Update Settlement Test Suite ✅ COMPLETE

**Goal**: Update all settlement tests to use new pricing format

#### Tasks

- [x] Update `test_host_earnings_base_sepolia()` price values and formula
- [x] Update `test_host_earnings_opbnb()` price values and formula
- [x] Update `test_user_refund_calculation()` price values and formula
- [x] Update expected payment amounts in assertions
- [x] Add new tests specifically for PRICE_PRECISION edge cases
- [x] Verify all settlement tests pass with new values

**Files:**

- `tests/settlement/test_payment_distribution.rs` - Updated all test pricing

**Test Results:** 38 settlement tests passed (December 9, 2025)

---

## Phase 3: Job Claim & Verification (TDD) ✅ COMPLETE

### Sub-phase 3.1: Fix Job Claim Payment Validation ✅ COMPLETE

**Goal**: Rewrite job_claim.rs payment validation for PRICE_PRECISION format

#### Tasks

- [x] Write test for correct payment_per_token extraction with PRICE_PRECISION
- [x] Write test for minimum payment threshold validation
- [x] Write test for sub-dollar pricing validation
- [x] Rewrite lines 304-308 payment validation logic
- [x] OLD: `payment_per_token = payment_amount / max_tokens`
- [x] NEW: `price_per_token = (deposit * PRICE_PRECISION) / max_tokens`
- [x] Add PRICE_PRECISION import
- [x] Add inline tests for price calculation formulas

**Files:**

- `src/job_claim.rs` - Fixed payment validation (lines 305-312)

**Test Results:** 3 tests passed (December 9, 2025)

### Sub-phase 3.2: Fix Job Verification Payment Amount ✅ COMPLETE

**Goal**: Fix job_verification.rs payment_amount calculation

#### Tasks

- [x] Write test for payment_amount with PRICE_PRECISION
- [x] Write test for sub-dollar pricing payment amount
- [x] Write test for native token payment amount
- [x] Fix line 419: Add PRICE_PRECISION division
- [x] OLD: `payment_amount = max_price_per_token * max_tokens`
- [x] NEW: `payment_amount = (max_price_per_token * max_tokens) / PRICE_PRECISION`
- [x] Add PRICE_PRECISION import

**Files:**

- `src/api/websocket/job_verification.rs` - Fixed payment_amount calculation (line 422)

**Test Results:** 3 tests passed (December 9, 2025)

---

## Phase 4: Host Registration & Node Pricing (TDD) ✅ COMPLETE

### Sub-phase 4.1: Update Registration Pricing Validation ✅ COMPLETE

**Goal**: Update host/registration.rs to validate against new constants

#### Tasks

- [x] Write test for native price validation with new MIN (227,273)
- [x] Write test for native price validation with new MAX (22,727,272,727,273,000)
- [x] Write test for stable price validation with new MIN (1)
- [x] Write test for stable price validation with new MAX (100,000,000)
- [x] Write test for default price assignment with new defaults
- [x] Verify lines 162-176 already use updated pricing_constants (no changes needed)
- [x] Verify registerNode contract call uses correct ABI format

**Files:**

- `src/host/registration.rs` - Already uses pricing_constants module (lines 14, 162-176)
- Added 3 new tests for PRICE_PRECISION validation

**Test Results:** 5 tests passed (December 9, 2025)

### Sub-phase 4.2: Update Multi-Chain Registrar ✅ COMPLETE

**Goal**: Ensure multi_chain_registrar.rs validates pricing correctly

#### Tasks

- [x] Verify multi_chain_registrar.rs uses pricing_constants module
- [x] Verify minPricePerTokenNative encoding uses default_price()
- [x] Verify minPricePerTokenStable encoding uses default_price()
- [x] Lines 297-298 already use native::default_price() and stable::default_price()

**Files:**

- `src/blockchain/multi_chain_registrar.rs` - Already uses pricing_constants module (line 15)

**Note:** No code changes needed - both files already import and use the pricing_constants module which was updated in Phase 1.

---

## Phase 5: Session Initialization & Token Tracking (TDD) ✅ COMPLETE

### Sub-phase 5.1: Session Price Handling ✅ COMPLETE

**Goal**: Ensure session initialization handles PRICE_PRECISION correctly

#### Tasks

- [x] Review `src/crypto/session_init.rs` price_per_token field (line 39)
- [x] Add documentation comments explaining PRICE_PRECISION
- [x] Verify price_per_token is passed through without modification (correct behavior)

**Files:**

- `src/crypto/session_init.rs` - Added PRICE_PRECISION documentation to price_per_token field

**Note:** SessionInitData stores price_per_token as received from client. The client is expected to send prices in PRICE_PRECISION format (×1000). No calculations are performed in session_init.rs - prices flow through to payment_distribution.rs where PRICE_PRECISION division is applied.

### Sub-phase 5.2: Token Tracker & Checkpoint Integration ✅ COMPLETE

**Goal**: Ensure checkpoint submission uses correct pricing format

#### Tasks

- [x] Review `src/api/token_tracker.rs` checkpoint logic - only tracks token counts
- [x] Review `src/contracts/checkpoint_manager.rs` submitProofOfWork call
- [x] Verify contract uses stored pricePerToken with PRICE_PRECISION on-chain

**Files:**

- `src/api/token_tracker.rs` - Token counting only, no price calculations
- `src/contracts/checkpoint_manager.rs` - Submits tokensGenerated to contract

**Note:** Token tracking and checkpoint submission don't perform price calculations. They submit token counts to the contract, which handles payment distribution using the job's stored pricePerToken (in PRICE_PRECISION format) and the PRICE_PRECISION constant on-chain.

---

## Phase 6: Documentation & Verification (Final) ✅ COMPLETE

### Sub-phase 6.1: Update Project Documentation ✅ COMPLETE

**Goal**: Update all documentation to reflect new contract addresses and pricing

#### Tasks

- [x] Created `docs/IMPLEMENTATION_PRICE_PRECISION.md` with full migration plan
- [x] Updated `.env.local.test` with new contract addresses
- [x] Added PRICE_PRECISION documentation to `src/crypto/session_init.rs`
- [x] Updated pricing_constants.rs with comprehensive documentation

**Files:**

- `docs/IMPLEMENTATION_PRICE_PRECISION.md` - This implementation plan
- `src/contracts/pricing_constants.rs` - Updated with detailed documentation
- `src/crypto/session_init.rs` - Added PRICE_PRECISION comment

### Sub-phase 6.2: Integration Testing & Verification ✅ COMPLETE

**Goal**: Verify complete system works with new contracts

#### Test Results Summary (December 9, 2025):

| Test Suite | Tests | Status |
|------------|-------|--------|
| pricing_constants | 20 | ✅ PASS |
| chain_config | 7 | ✅ PASS |
| job_claim | 3 | ✅ PASS |
| job_verification | 3 | ✅ PASS |
| registration | 5 | ✅ PASS |
| settlement_tests | 38 | ✅ PASS |
| **Total PRICE_PRECISION Related** | **76** | **✅ ALL PASS** |

#### Tasks

- [x] Run pricing_constants tests: 20 passed
- [x] Run chain_config tests: 7 passed
- [x] Run job_claim tests: 3 passed
- [x] Run job_verification tests: 3 passed
- [x] Run registration tests: 5 passed
- [x] Run settlement_tests: 38 passed
- [x] Verify no regressions in PRICE_PRECISION-related functionality

**Note:** Some unrelated tests (version, crypto) have pre-existing failures not related to PRICE_PRECISION changes.

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

## Verification Checklist ✅ COMPLETE

After implementation, verify:

- [x] `cargo test --lib pricing_constants` passes (20 tests)
- [x] `cargo test --lib chain_config` passes (7 tests)
- [x] `cargo test --lib job_claim` passes (3 tests)
- [x] `cargo test --lib job_verification` passes (3 tests)
- [x] `cargo test --lib registration` passes (5 tests)
- [x] `cargo test --test settlement_tests` passes (38 tests)
- [x] Payment calculations use PRICE_PRECISION division
- [x] Settlement distributes correct amounts to host (90%) and treasury (10%)

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
