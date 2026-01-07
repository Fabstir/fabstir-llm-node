# IMPLEMENTATION - Security Audit: Proof Signing

## Status: IN PROGRESS 🔧

**Status**: Phase 1 Complete ✅ → Ready for Phase 2
**Version**: v8.9.0-proof-signing
**Start Date**: 2026-01-06
**Approach**: Strict TDD bounded autonomy - one sub-phase at a time
**Tests Passing**: 10/10 (all proof_signer tests)

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

**Status**: PENDING

#### Tasks
- [ ] Update `encode_checkpoint_call` function signature to add `signature: [u8; 65]`
- [ ] Add signature parameter to ABI definition (type: `bytes`, position: 4th)
- [ ] Update token encoding to include signature bytes
- [ ] Update function documentation
- [ ] Run `cargo check` to find all callers that need updating

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

**Status**: PENDING

#### Tasks
- [ ] Import `sign_proof_data` in checkpoint_manager.rs
- [ ] Get host address from wallet in `submit_checkpoint_async`
- [ ] Generate signature before calling `encode_checkpoint_call`
- [ ] Update call to pass signature
- [ ] Add error handling for signing failures
- [ ] Run `cargo check`

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

**Status**: PENDING

#### Tasks
- [ ] Search for all usages of `encode_checkpoint_call`
- [ ] Update `force_checkpoint_sync` if it exists
- [ ] Update `submit_proof_of_work` if called directly anywhere
- [ ] Update any test helpers that mock checkpoint calls
- [ ] Run `cargo test` to verify no compilation errors

**Locations to check:**
- `submit_checkpoint_async()` (main path)
- `force_checkpoint()` (if exists)
- Any test files that construct checkpoint calls

---

## Phase 3: Testing & Finalization (1.5 hours)

### Sub-phase 3.1: Integration Tests

**Goal**: Test end-to-end proof signing and submission

**Status**: PENDING

#### Tasks
- [ ] Write test `test_checkpoint_with_signature_encodes_correctly`
- [ ] Write test `test_signature_in_transaction_data`
- [ ] Verify signature is at correct position in encoded call data
- [ ] Run full test suite: `cargo test`

**Test locations:**
- `src/contracts/checkpoint_manager.rs` (inline tests)
- `tests/checkpoint_tests.rs` (if exists)

---

### Sub-phase 3.2: Update Version and Documentation

**Goal**: Bump version and update docs

**Status**: PENDING

#### Tasks
- [ ] Update `VERSION` file to `8.9.0-proof-signing`
- [ ] Update `src/version.rs`:
  - [ ] VERSION constant
  - [ ] VERSION_NUMBER to "8.9.0"
  - [ ] VERSION_MINOR to 9
  - [ ] VERSION_PATCH to 0
  - [ ] Add feature: "proof-signing"
  - [ ] Add feature: "security-audit-compliance"
  - [ ] Add BREAKING_CHANGES entry
  - [ ] Update test assertions
- [ ] Build and verify: `cargo build --release`
- [ ] Run all tests: `cargo test`

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

**Status**: PENDING

#### Tasks
- [ ] Build release: `cargo build --release -j 4`
- [ ] Verify version in binary: `strings target/release/fabstir-llm-node | grep "v8.9"`
- [ ] Copy binary to root: `cp target/release/fabstir-llm-node ./fabstir-llm-node`
- [ ] Create tarball with correct structure
- [ ] Verify tarball contents
- [ ] Clean up temporary binary

**Tarball command:**
```bash
tar -czvf fabstir-llm-node-v8.9.0-proof-signing.tar.gz \
  fabstir-llm-node \
  scripts/download_florence_model.sh \
  scripts/download_ocr_models.sh \
  scripts/download_embedding_model.sh \
  scripts/setup_models.sh
```

---

## Summary

| Phase | Sub-phase | Description | Est. Time |
|-------|-----------|-------------|-----------|
| 1 | 1.1 | Create module structure | 15 min |
| 1 | 1.2 | Implement signing logic (TDD) | 45 min |
| 1 | 1.3 | Add verification helper | 30 min |
| 2 | 2.1 | Update ABI encoding function | 30 min |
| 2 | 2.2 | Update async checkpoint submission | 45 min |
| 2 | 2.3 | Update all checkpoint callers | 30 min |
| 3 | 3.1 | Integration tests | 30 min |
| 3 | 3.2 | Update version and documentation | 20 min |
| 3 | 3.3 | Create release tarball | 15 min |
| **Total** | | | **~5 hours** |

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

- [ ] Unit tests for `sign_proof_data` (5 tests)
- [ ] Unit tests for `verify_proof_signature` (3 tests)
- [ ] Integration test for checkpoint encoding
- [ ] Verify signature is exactly 65 bytes
- [ ] Verify recovered address matches host wallet
- [ ] Test with real HOST_PRIVATE_KEY from .env
- [ ] Full test suite passes: `cargo test`
