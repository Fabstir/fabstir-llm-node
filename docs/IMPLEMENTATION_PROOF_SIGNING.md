# IMPLEMENTATION - Security Audit: Proof Signing

## Status: Phase 4 Pending 🔧

**Phases 1-3**: ✅ COMPLETE - Proof signing implemented and deployed (v8.9.1)
**Phase 4**: 🔧 PENDING - Enhanced proof witness with real content hashes

**Current Version**: v8.9.1-proof-signing-eip191
**Next Version**: v8.10.0-real-content-hashes (after Phase 4)
**Start Date**: 2026-01-06
**Approach**: Strict TDD bounded autonomy - one sub-phase at a time
**Tests Passing**: 18/18 (10 proof_signer + 5 checkpoint_manager + 3 version)
**Release**: `fabstir-llm-node-v8.9.1-proof-signing-eip191.tar.gz` (556 MB)

### v8.9.1 Hotfix (2026-01-07)
- **Fixed**: Added EIP-191 personal_sign prefix (`\x19Ethereum Signed Message:\n32`)
- **Cause**: Contract uses `ecrecover` with EIP-191 prefix, node was signing raw hash
- **Result**: Signatures now match contract's verification logic

### Phase 4 Goal (Pending)
- **Problem**: Current proofs use placeholder hashes, not actual prompt/response content
- **Solution**: Bind real `SHA256(prompt)` and `SHA256(response)` into the STARK proof
- **Benefit**: Cryptographically proves exact content that was processed

---

## Overview

Implementation plan for the security audit requirement: cryptographic proof signing for `submitProofOfWork`. This prevents token count manipulation, unauthorized submissions, and replay attacks.

**Breaking Change:**
```
OLD (4 params): submitProofOfWork(jobId, tokensClaimed, proofHash, proofCID)
NEW (5 params): submitProofOfWork(jobId, tokensClaimed, proofHash, signature, proofCID)
```

**Signature Formula:**
```
1. proofHash = keccak256(proofData)
2. dataHash = keccak256(abi.encodePacked(proofHash, hostAddress, tokensClaimed))
3. signature = personal_sign(dataHash)  // 65 bytes: r(32) + s(32) + v(1)
```

**Key Constraints:**
- **65-byte signature** - Must be exactly r(32) + s(32) + v(1) format
- **Host wallet must sign** - Signature must match session.host address
- **EIP-191 personal_sign** - Standard Ethereum message signing
- **Replay protection** - Each proofHash can only be used once

**References:**
- Migration Guide: `docs/compute-contracts-reference/SECURITY-AUDIT-NODE-MIGRATION.md`
- Updated ABI: `docs/compute-contracts-reference/client-abis/JobMarketplaceWithModelsUpgradeable-CLIENT-ABI.json`
- Existing Signature Recovery: `src/crypto/signature.rs`
- Private Key Extraction: `src/crypto/private_key.rs`
- Checkpoint Manager: `src/contracts/checkpoint_manager.rs`

---

## Dependencies

### Already Available (No Changes Needed)
```toml
k256 = { version = "0.13", features = ["ecdsa"] }  # ECDSA signing
tiny_keccak = { version = "2.0", features = ["keccak"] }  # Keccak256 hashing
ethers = { version = "2.0", features = [...] }  # Address type, ABI encoding
hex = "0.4"  # Hex encoding
```

### Existing Infrastructure
- `extract_node_private_key()` - Gets HOST_PRIVATE_KEY from environment
- `recover_client_address()` - ECDSA signature recovery (for testing)
- `Web3Client` with `LocalWallet` - Transaction signing

---

## Phase 1: Proof Signing Function (2 hours)

### Sub-phase 1.1: Create Module Structure

**Goal**: Create the proof_signer module with stub functions

**Status**: ✅ COMPLETE (2026-01-07)

#### Tasks
- [x] Create `src/crypto/proof_signer.rs` with module documentation
- [x] Add `pub mod proof_signer;` to `src/crypto/mod.rs`
- [x] Add `pub use proof_signer::sign_proof_data;` to exports
- [x] Define `sign_proof_data` function signature (fully implemented)
- [x] Define `ProofSigningError` enum for error handling
- [x] Run `cargo check` to verify module structure compiles

**Notes:**
- Implemented full signing logic instead of stub (ahead of schedule)
- Added `verify_proof_signature()` helper function
- Added `encode_proof_data()` and `hash_data()` helper functions
- Basic unit tests added for encoding and hashing

**Implementation Files:**
- `src/crypto/proof_signer.rs` (NEW)
  ```rust
  //! Proof Signing for Security Audit Compliance
  //!
  //! Signs proof data for submitProofOfWork contract calls.
  //! Required after January 2026 security audit.
  //!
  //! Signature formula:
  //!   dataHash = keccak256(abi.encodePacked(proofHash, hostAddress, tokensClaimed))
  //!   signature = personal_sign(dataHash)

  use anyhow::Result;
  use ethers::types::Address;

  /// Sign proof data for contract submission
  ///
  /// # Arguments
  /// * `private_key` - 32-byte host private key
  /// * `proof_hash` - 32-byte keccak256 hash of proof data
  /// * `host_address` - 20-byte Ethereum address of host
  /// * `tokens_claimed` - Number of tokens being claimed
  ///
  /// # Returns
  /// 65-byte signature (r + s + v)
  pub fn sign_proof_data(
      private_key: &[u8; 32],
      proof_hash: [u8; 32],
      host_address: Address,
      tokens_claimed: u64,
  ) -> Result<[u8; 65]> {
      todo!("Implement in sub-phase 1.2")
  }
  ```

- `src/crypto/mod.rs` (MODIFY)
  ```rust
  pub mod proof_signer;
  pub use proof_signer::sign_proof_data;
  ```

---

### Sub-phase 1.2: Implement Signing Logic (TDD)

**Goal**: Implement the core signing function with tests first

**Status**: ✅ COMPLETE (2026-01-07)

#### Tasks
- [x] Write test `test_sign_proof_data_returns_65_bytes`
- [x] Write test `test_sign_proof_data_recoverable_address`
- [x] Write test `test_sign_proof_data_different_tokens_different_signature`
- [x] Write test `test_sign_proof_data_different_proof_hash_different_signature`
- [x] Write test `test_sign_proof_data_v_value_is_27_or_28`
- [x] Implement `encode_proof_data()` helper - packs proofHash + address + tokens
- [x] Implement `hash_proof_data()` helper - keccak256 of packed data (named `hash_data`)
- [x] Implement `sign_proof_data()` - full signing using k256
- [x] Run tests: `cargo test proof_signer` - **10/10 tests passing**

**Additional Tests Added:**
- `test_sign_proof_data_wrong_address_fails_verification` - Security test
- `test_sign_proof_data_wrong_tokens_fails_verification` - Security test
- `test_encode_proof_data_tokens_big_endian` - Encoding verification

**Test File:** `src/crypto/proof_signer.rs` (inline tests)
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::SigningKey;
    use rand::rngs::OsRng;

    fn generate_test_keypair() -> ([u8; 32], Address) {
        let signing_key = SigningKey::random(&mut OsRng);
        let private_key: [u8; 32] = signing_key.to_bytes().into();
        // Derive address from public key...
        (private_key, address)
    }

    #[test]
    fn test_sign_proof_data_returns_65_bytes() {
        let (private_key, host_address) = generate_test_keypair();
        let proof_hash = [0u8; 32];
        let tokens = 1000u64;

        let signature = sign_proof_data(&private_key, proof_hash, host_address, tokens).unwrap();
        assert_eq!(signature.len(), 65);
    }

    #[test]
    fn test_sign_proof_data_recoverable_address() {
        // Sign data and verify recovered address matches host_address
    }

    #[test]
    fn test_sign_proof_data_different_tokens_different_signature() {
        // Same proof_hash but different tokens should produce different signatures
    }

    #[test]
    fn test_sign_proof_data_v_value_is_27_or_28() {
        // Verify v byte is Ethereum-compatible (27 or 28)
    }
}
```

**Implementation Details:**
```rust
use k256::ecdsa::{SigningKey, Signature, signature::Signer};
use tiny_keccak::{Hasher, Keccak};

/// Encode proof data for signing (Solidity abi.encodePacked equivalent)
fn encode_proof_data(proof_hash: [u8; 32], host_address: Address, tokens_claimed: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(32 + 20 + 32);
    data.extend_from_slice(&proof_hash);           // 32 bytes
    data.extend_from_slice(host_address.as_bytes()); // 20 bytes
    // tokens as uint256 (32 bytes, big-endian, zero-padded)
    let mut tokens_bytes = [0u8; 32];
    tokens_bytes[24..].copy_from_slice(&tokens_claimed.to_be_bytes());
    data.extend_from_slice(&tokens_bytes);         // 32 bytes
    data
}

/// Hash encoded data with keccak256
fn hash_proof_data(encoded: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut hash = [0u8; 32];
    hasher.update(encoded);
    hasher.finalize(&mut hash);
    hash
}

pub fn sign_proof_data(
    private_key: &[u8; 32],
    proof_hash: [u8; 32],
    host_address: Address,
    tokens_claimed: u64,
) -> Result<[u8; 65]> {
    // 1. Encode the data
    let encoded = encode_proof_data(proof_hash, host_address, tokens_claimed);

    // 2. Hash it
    let data_hash = hash_proof_data(&encoded);

    // 3. Sign using EIP-191 personal_sign
    let signing_key = SigningKey::from_bytes(private_key.into())
        .map_err(|e| anyhow!("Invalid private key: {}", e))?;

    let (signature, recovery_id) = signing_key
        .sign_prehash_recoverable(&data_hash)
        .map_err(|e| anyhow!("Signing failed: {}", e))?;

    // 4. Format as 65-byte signature (r + s + v)
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = recovery_id.to_byte() + 27; // Ethereum v value

    Ok(sig_bytes)
}
```

---

### Sub-phase 1.3: Add Verification Helper (TDD)

**Goal**: Add function to verify signature locally (for debugging)

**Status**: ✅ COMPLETE (2026-01-07)

#### Tasks
- [x] Write test `test_verify_proof_signature_valid` → `test_sign_proof_data_recoverable_address`
- [x] Write test `test_verify_proof_signature_wrong_address` → `test_sign_proof_data_wrong_address_fails_verification`
- [x] Write test `test_verify_proof_signature_wrong_tokens` → `test_sign_proof_data_wrong_tokens_fails_verification`
- [x] Implement `verify_proof_signature()` function (done in Sub-phase 1.1)
- [x] Run tests: `cargo test proof_signer` - **10/10 tests passing**

**Note:** Verification helper was implemented ahead of schedule in Sub-phase 1.1, tests added in Sub-phase 1.2.

**Implementation:**
```rust
/// Verify a proof signature locally (for debugging)
///
/// Returns true if signature was created by host_address for this data
pub fn verify_proof_signature(
    signature: &[u8; 65],
    proof_hash: [u8; 32],
    host_address: Address,
    tokens_claimed: u64,
) -> Result<bool> {
    // 1. Encode and hash the data
    let encoded = encode_proof_data(proof_hash, host_address, tokens_claimed);
    let data_hash = hash_proof_data(&encoded);

    // 2. Recover address from signature
    let recovered = crate::crypto::signature::recover_client_address(signature, &data_hash)?;

    // 3. Compare addresses (case-insensitive)
    Ok(recovered.to_lowercase() == format!("{:?}", host_address).to_lowercase())
}
```

---

## Phase 2: Checkpoint Manager Integration (2 hours)

### Sub-phase 2.1: Update ABI Encoding Function

**Goal**: Modify `encode_checkpoint_call` to accept signature parameter

**Status**: ✅ COMPLETE (2026-01-07)

#### Tasks
- [x] Update `encode_checkpoint_call` function signature to add `signature: [u8; 65]`
- [x] Add signature parameter to ABI definition (type: `bytes`, position: 4th)
- [x] Update token encoding to include signature bytes
- [x] Update function documentation
- [x] Run `cargo check` to find all callers that need updating

**File:** `src/contracts/checkpoint_manager.rs` (lines 1261-1313)

**Before:**
```rust
fn encode_checkpoint_call(
    job_id: u64,
    tokens_generated: u64,
    proof_hash: [u8; 32],
    proof_cid: String,
) -> Vec<u8>
```

**After:**
```rust
fn encode_checkpoint_call(
    job_id: u64,
    tokens_generated: u64,
    proof_hash: [u8; 32],
    signature: [u8; 65],  // NEW: Host's proof signature
    proof_cid: String,
) -> Vec<u8>
```

**ABI Changes:**
```rust
let function = Function {
    name: "submitProofOfWork".to_string(),
    inputs: vec![
        Param { name: "jobId", kind: ParamType::Uint(256), .. },
        Param { name: "tokensClaimed", kind: ParamType::Uint(256), .. },
        Param { name: "proofHash", kind: ParamType::FixedBytes(32), .. },
        Param { name: "signature", kind: ParamType::Bytes, .. },  // NEW
        Param { name: "proofCID", kind: ParamType::String, .. },
    ],
    ..
};

let tokens = vec![
    Token::Uint(U256::from(job_id)),
    Token::Uint(U256::from(tokens_generated)),
    Token::FixedBytes(proof_hash.to_vec()),
    Token::Bytes(signature.to_vec()),  // NEW
    Token::String(proof_cid),
];
```

---

### Sub-phase 2.2: Update Async Checkpoint Submission

**Goal**: Generate signature before submitting checkpoints

**Status**: ✅ COMPLETE (2026-01-07)

#### Tasks
- [x] Import `sign_proof_data` in checkpoint_manager.rs (via crate::crypto::)
- [x] Get host address from wallet in `submit_checkpoint_async`
- [x] Generate signature before calling `encode_checkpoint_call`
- [x] Update call to pass signature
- [x] Add error handling for signing failures
- [x] Run `cargo check`

**Note:** Updated both `submit_checkpoint` (sync) and `submit_checkpoint_async` callers.

**File:** `src/contracts/checkpoint_manager.rs` (modify `submit_checkpoint_async`)

**Changes needed in `submit_checkpoint_async` (~line 414-522):**
```rust
// After generating proof_hash, before calling encode_checkpoint_call:

// Get host address from wallet
let host_address = {
    let wallet = client.wallet.read().await;
    wallet.as_ref()
        .ok_or_else(|| anyhow!("No wallet configured for signing"))?
        .address()
};

// Get private key for signing
let private_key = crate::crypto::extract_node_private_key()?;

// Sign the proof data
let signature = crate::crypto::sign_proof_data(
    &private_key,
    proof_hash,
    host_address,
    tokens_to_submit,
)?;

info!("✅ Proof signed by host {}", host_address);

// Now encode with signature
let call_data = encode_checkpoint_call(
    job_id,
    tokens_to_submit,
    proof_hash,
    signature,  // NEW
    proof_cid.clone(),
);
```

---

### Sub-phase 2.3: Update All Checkpoint Callers

**Goal**: Ensure all paths that call `encode_checkpoint_call` pass signature

**Status**: ✅ COMPLETE (2026-01-07)

#### Tasks
- [x] Search for all usages of `encode_checkpoint_call`
- [x] Update `submit_checkpoint` (sync version) - line 338
- [x] Update `submit_checkpoint_async` - line 492
- [x] No other callers found (no force_checkpoint_sync, no test helpers)

**Callers Updated:**
1. `submit_checkpoint()` at line 338: Added signature generation using `self.host_address`
2. `submit_checkpoint_async()` at line 492: Added signature generation using `host_address` parameter

**Verification:** `cargo check` passes, 10/10 proof_signer tests passing.

---

## Phase 3: Testing & Finalization (1.5 hours)

### Sub-phase 3.1: Integration Tests

**Goal**: Test end-to-end proof signing and submission

**Status**: ✅ COMPLETE (2026-01-07)

#### Tasks
- [x] Write test `test_checkpoint_with_signature_encodes_correctly`
- [x] Write test `test_signature_in_transaction_data`
- [x] Write test `test_different_signatures_different_encoding`
- [x] Write test `test_signature_length_in_encoding`
- [x] Write test `test_encoding_is_deterministic`
- [x] Run test suite: `cargo test` - **15/15 tests passing**

**Test locations:**
- `src/contracts/checkpoint_manager.rs` (5 new inline tests)
- `src/crypto/proof_signer.rs` (10 unit tests)

---

### Sub-phase 3.2: Update Version and Documentation

**Goal**: Bump version and update docs

**Status**: ✅ COMPLETE (2026-01-07)

#### Tasks
- [x] Update `VERSION` file to `8.9.0-proof-signing`
- [x] Update `src/version.rs`:
  - [x] VERSION constant → "v8.9.0-proof-signing-2026-01-07"
  - [x] VERSION_NUMBER to "8.9.0"
  - [x] VERSION_MINOR to 9
  - [x] VERSION_PATCH to 0
  - [x] Add features: "proof-signing", "security-audit-compliance", "ecdsa-proof-signatures", "65-byte-signatures"
  - [x] Add BREAKING_CHANGES entries (4 new entries)
  - [x] Update test assertions
- [x] Run all tests: `cargo test` - **18/18 passing**

**BREAKING_CHANGES entry:**
```rust
"BREAKING: submitProofOfWork now requires 5th parameter: 65-byte proof signature (v8.9.0)",
"FEAT: Proof signing for security audit compliance - prevents token manipulation",
"FEAT: Host wallet cryptographically signs proof data before submission",
```

**Features to add:**
```rust
"proof-signing",
"security-audit-compliance",
"eip191-personal-sign",
```

---

### Sub-phase 3.3: Create Release Tarball

**Goal**: Package release binary

**Status**: ✅ COMPLETE (2026-01-07)

#### Tasks
- [x] Build release: `cargo build --release --features real-ezkl -j 4`
- [x] Verify version in binary: `strings target/release/fabstir-llm-node | grep "v8.9"` → Found `v8.9.0-proof-signing-2026-01-07`
- [x] Copy binary to root: `cp target/release/fabstir-llm-node ./fabstir-llm-node`
- [x] Create tarball with correct structure (binary at root, not in target/release/)
- [x] Verify tarball contents - 5 files included
- [x] Clean up temporary binary

**Tarball details:**
- **File**: `fabstir-llm-node-v8.9.1-proof-signing-eip191.tar.gz`
- **Size**: 556 MB (compressed from ~1 GB binary)
- **Contents**:
  - `fabstir-llm-node` (binary at root)
  - `scripts/download_florence_model.sh`
  - `scripts/download_ocr_models.sh`
  - `scripts/download_embedding_model.sh`
  - `scripts/setup_models.sh`

**Tarball command used:**
```bash
tar -czvf fabstir-llm-node-v8.9.1-proof-signing-eip191.tar.gz \
  fabstir-llm-node \
  scripts/download_florence_model.sh \
  scripts/download_ocr_models.sh \
  scripts/download_embedding_model.sh \
  scripts/setup_models.sh
```

---

## Summary

| Phase | Sub-phase | Description | Est. Time | Status |
|-------|-----------|-------------|-----------|--------|
| 1 | 1.1 | Create module structure | 15 min | ✅ |
| 1 | 1.2 | Implement signing logic (TDD) | 45 min | ✅ |
| 1 | 1.3 | Add verification helper | 30 min | ✅ |
| 2 | 2.1 | Update ABI encoding function | 30 min | ✅ |
| 2 | 2.2 | Update async checkpoint submission | 45 min | ✅ |
| 2 | 2.3 | Update all checkpoint callers | 30 min | ✅ |
| 3 | 3.1 | Integration tests | 30 min | ✅ |
| 3 | 3.2 | Update version and documentation | 20 min | ✅ |
| 3 | 3.3 | Create release tarball | 15 min | ✅ |
| **Phases 1-3** | | **Proof Signing (v8.9.1)** | **~5 hours** | ✅ |
| 4 | 4.1 | Extend TokenTracker State | 45 min | 🔧 |
| 4 | 4.2 | Compute Prompt Hash at Inference Start | 30 min | 🔧 |
| 4 | 4.3 | Accumulate and Hash Response | 45 min | 🔧 |
| 4 | 4.4 | Update Proof Generation | 30 min | 🔧 |
| 4 | 4.5 | Integration Tests | 45 min | 🔧 |
| 4 | 4.6 | Update Version and Create Release | 20 min | 🔧 |
| **Phase 4** | | **Real Content Hashes (v8.10.0)** | **~3.5 hours** | 🔧 |
| **Grand Total** | | | **~8.5 hours** | |

---

## Error Reference

| Contract Error | Cause | Solution |
|----------------|-------|----------|
| `"Invalid signature length"` | Signature not 65 bytes | Check signing output format |
| `"Invalid proof signature"` | Wrong signer | Use correct HOST_PRIVATE_KEY |
| `"Invalid proof signature"` | Wrong data encoded | Check encodePacked order |
| `"Proof already verified"` | Replay attack | Each proof must be unique |

---

## Testing Checklist

- [x] Unit tests for `sign_proof_data` (5 tests) ✅
- [x] Unit tests for `verify_proof_signature` (3 tests) ✅
- [x] Integration tests for checkpoint encoding (5 tests) ✅
- [x] Verify signature is exactly 65 bytes ✅
- [x] Verify recovered address matches host wallet ✅
- [x] Verify v value is 27 or 28 (Ethereum compatible) ✅
- [x] Full test suite passes: `cargo test` → 18/18 passing ✅

---

## Phase 4: Enhanced Proof Witness (Real Content Hashes)

**Status**: PENDING 🔧
**Priority**: High - Current proofs use placeholder hashes, not actual content

### Problem Statement

The current proof witness uses **placeholder hashes** instead of actual content:

```rust
// CURRENT (WEAK): Deterministic placeholders
let input_hash = SHA256("job_127:input");           // NOT the actual prompt!
let output_hash = SHA256("job_127:output:tokens_1000"); // NOT the actual response!
```

This means the STARK proof only proves:
- ✅ "A job with ID X was processed"
- ✅ "N tokens were claimed"
- ❌ **NOT** "This specific prompt produced this specific response"

### Solution

Replace placeholder hashes with real content hashes:

```rust
// ENHANCED (STRONG): Actual content hashes
let input_hash = SHA256(actual_prompt_text);        // Real prompt binding
let output_hash = SHA256(actual_response_text);     // Real response binding
```

### Data Flow Analysis

```
┌─────────────────────────────────────────────────────────────────────────┐
│  CURRENT FLOW (WEAK)                                                    │
├─────────────────────────────────────────────────────────────────────────┤
│  API Server → Inference → Token Tracker → Checkpoint                    │
│     │                          │              │                         │
│     │ prompt                   │ count only   │ placeholder hashes      │
│     ▼                          ▼              ▼                         │
│  (discarded)              track_tokens()  generate_proof()              │
└─────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────┐
│  ENHANCED FLOW (STRONG)                                                 │
├─────────────────────────────────────────────────────────────────────────┤
│  API Server → Inference → Token Tracker → Checkpoint                    │
│     │             │            │              │                         │
│     │ prompt      │ response   │ content +    │ real hashes             │
│     ▼             ▼            │ hashes       ▼                         │
│  SHA256(p) → SHA256(r) ──────→ store ───→ generate_proof()              │
└─────────────────────────────────────────────────────────────────────────┘
```

---

### Sub-phase 4.1: Extend TokenTracker State

**Goal**: Add storage for content hashes in per-job tracking state

**Status**: PENDING

**File**: `src/contracts/checkpoint_manager.rs`

#### Tasks
- [ ] Add `ContentHashes` struct to store prompt and response hashes
- [ ] Add `content_hashes: HashMap<u64, ContentHashes>` to `CheckpointManager`
- [ ] Add `response_buffer: HashMap<u64, String>` for accumulating response text
- [ ] Implement `set_prompt_hash(job_id: u64, hash: [u8; 32])`
- [ ] Implement `append_response(job_id: u64, text: &str)`
- [ ] Implement `finalize_response_hash(job_id: u64) -> [u8; 32]`
- [ ] Implement `get_content_hashes(job_id: u64) -> Option<([u8; 32], [u8; 32])>`
- [ ] Write unit tests for each method

**New struct:**
```rust
/// Content hashes for cryptographic proof binding
#[derive(Debug, Clone, Default)]
pub struct ContentHashes {
    /// SHA256 of the original prompt
    pub prompt_hash: Option<[u8; 32]>,
    /// SHA256 of the generated response (computed at checkpoint time)
    pub response_hash: Option<[u8; 32]>,
    /// Accumulated response text (cleared after hash computation)
    response_buffer: String,
}
```

**Tests (TDD):**
```rust
#[test]
fn test_set_prompt_hash_stores_hash() { ... }

#[test]
fn test_append_response_accumulates_text() { ... }

#[test]
fn test_finalize_response_hash_computes_sha256() { ... }

#[test]
fn test_get_content_hashes_returns_both() { ... }

#[test]
fn test_content_hashes_cleared_after_checkpoint() { ... }
```

---

### Sub-phase 4.2: Compute Prompt Hash at Inference Start

**Goal**: Hash the prompt when inference begins and store it

**Status**: PENDING

**Files**:
- `src/api/server.rs` (HTTP streaming path)
- `src/api/websocket/handlers/inference.rs` (WebSocket path)

#### Tasks
- [ ] In HTTP inference handler: compute `SHA256(prompt)` before calling engine
- [ ] Call `checkpoint_manager.set_prompt_hash(job_id, hash)`
- [ ] In WebSocket inference handler: same pattern
- [ ] Write integration test for prompt hash flow

**HTTP path (src/api/server.rs):**
```rust
// Before inference starts
let prompt_hash = Sha256::digest(request.prompt.as_bytes());
let mut prompt_hash_bytes = [0u8; 32];
prompt_hash_bytes.copy_from_slice(&prompt_hash);

// Store in checkpoint manager
checkpoint_manager.set_prompt_hash(job_id, prompt_hash_bytes);
```

**WebSocket path:**
```rust
// Same pattern in inference handler
checkpoint_manager.set_prompt_hash(session.job_id, prompt_hash_bytes);
```

---

### Sub-phase 4.3: Accumulate and Hash Response

**Goal**: Build up response text and compute hash at checkpoint time

**Status**: PENDING

**Files**:
- `src/api/server.rs` (token streaming)
- `src/contracts/checkpoint_manager.rs` (hash computation)

#### Tasks
- [ ] In streaming loop: call `checkpoint_manager.append_response(job_id, token_text)`
- [ ] Before checkpoint submission: call `finalize_response_hash(job_id)`
- [ ] Handle incremental checkpoints (each checkpoint = cumulative response)
- [ ] Write integration test for response accumulation

**Streaming accumulation:**
```rust
// In token streaming loop
for token in tokens {
    // Send to client
    send_token(token);

    // Accumulate for hash (new)
    checkpoint_manager.append_response(job_id, &token.text);

    // Track count (existing)
    checkpoint_manager.track_tokens(job_id, 1);
}
```

**At checkpoint time:**
```rust
// In generate_proof_async, before building witness
let response_hash = self.finalize_response_hash(job_id);
```

---

### Sub-phase 4.4: Update Proof Generation to Use Real Hashes

**Goal**: Replace placeholder hashes with actual content hashes

**Status**: PENDING

**File**: `src/contracts/checkpoint_manager.rs`

#### Tasks
- [ ] Modify `generate_proof_async` to retrieve content hashes
- [ ] Use real hashes when available, fall back to placeholder for backward compat
- [ ] Add logging to show which hash type is being used
- [ ] Update inline documentation

**Before (placeholder):**
```rust
let input_data = format!("job_{}:input", job_id);
let input_hash = Sha256::digest(input_data.as_bytes());
```

**After (real content):**
```rust
let (prompt_hash, response_hash) = match self.get_content_hashes(job_id) {
    Some((p, r)) => {
        info!("✅ Using real content hashes for job {}", job_id);
        (p, r)
    }
    None => {
        warn!("⚠️ Falling back to placeholder hashes for job {}", job_id);
        // Legacy placeholder computation
        let input_data = format!("job_{}:input", job_id);
        let input_hash = Sha256::digest(input_data.as_bytes());
        // ... etc
        (input_hash_bytes, output_hash_bytes)
    }
};
```

---

### Sub-phase 4.5: Integration Tests

**Goal**: Verify end-to-end content hash binding

**Status**: PENDING

**File**: `tests/proof_content_tests.rs` (NEW) or inline in checkpoint_manager.rs

#### Tasks
- [ ] Test: Different prompts produce different `input_hash` in proof
- [ ] Test: Different responses produce different `output_hash` in proof
- [ ] Test: Same prompt+response produces same proof hash (determinism)
- [ ] Test: Checkpoint at 1000 tokens includes response up to that point
- [ ] Test: Force checkpoint includes full response
- [ ] Test: Fallback to placeholder when content hashes not set

**Test cases:**
```rust
#[test]
fn test_different_prompts_different_proof_hash() {
    // Prompt A: "What is 2+2?"
    // Prompt B: "What is 3+3?"
    // Assert: proof_hash_A != proof_hash_B
}

#[test]
fn test_different_responses_different_proof_hash() {
    // Same prompt, different responses
    // Assert: proof_hash_A != proof_hash_B
}

#[test]
fn test_proof_determinism_same_content() {
    // Same prompt + same response
    // Assert: proof_hash_A == proof_hash_B
}

#[test]
fn test_incremental_checkpoint_response_hash() {
    // Generate 1500 tokens
    // First checkpoint at 1000 tokens: response_hash = SHA256(first 1000 tokens)
    // Second checkpoint at 500 tokens: response_hash = SHA256(all 1500 tokens)
}
```

---

### Sub-phase 4.6: Update Version and Create Release

**Goal**: Bump version and package release

**Status**: PENDING

#### Tasks
- [ ] Update `VERSION` to `8.10.0-real-content-hashes`
- [ ] Update `src/version.rs`:
  - VERSION, VERSION_NUMBER, VERSION_MINOR
  - Add feature: `"real-content-hashes"`
  - Add BREAKING_CHANGES entry
- [ ] Run full test suite
- [ ] Build release: `cargo build --release --features real-ezkl -j 4`
- [ ] Create tarball

**New feature flag:**
```rust
// v8.10.0 - Real content hashes
"real-content-hashes",
"prompt-hash-binding",
"response-hash-binding",
```

---

### Files to Modify

| File | Change |
|------|--------|
| `src/contracts/checkpoint_manager.rs` | Add ContentHashes, storage, methods |
| `src/api/server.rs` | Compute and store prompt hash, accumulate response |
| `src/api/websocket/handlers/inference.rs` | Same for WebSocket path |
| `src/version.rs` | Version bump to 8.10.0 |
| `VERSION` | Version bump |
| `tests/proof_content_tests.rs` | NEW - integration tests |

---

### Security Improvement

| Before (v8.9.1) | After (v8.10.0) |
|-----------------|-----------------|
| Proof binds job_id + token_count | Proof binds job_id + token_count |
| Placeholder `input_hash` | **Real SHA256(prompt)** |
| Placeholder `output_hash` | **Real SHA256(response)** |
| Proof could be reused for different content | Proof is content-specific |

This enhancement ensures the STARK proof cryptographically binds:
- ✅ The exact prompt that was sent
- ✅ The exact response that was generated
- ✅ The model that was used
- ✅ The job ID and token count

---

### Estimated Time

| Sub-phase | Description | Est. Time |
|-----------|-------------|-----------|
| 4.1 | Extend TokenTracker State | 45 min |
| 4.2 | Compute Prompt Hash at Inference Start | 30 min |
| 4.3 | Accumulate and Hash Response | 45 min |
| 4.4 | Update Proof Generation | 30 min |
| 4.5 | Integration Tests | 45 min |
| 4.6 | Update Version and Create Release | 20 min |
| **Total** | | **~3.5 hours** |
