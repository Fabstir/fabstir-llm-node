# IMPLEMENTATION - Checkpoint Publishing for Conversation Recovery

## Status: PHASE 9 IN PROGRESS - Encrypted Checkpoint Deltas

**Status**: Phase 9 - Encrypted Checkpoint Deltas (Privacy Enhancement)
**Version**: v8.12.0-encrypted-checkpoint-deltas-2026-01-13
**Start Date**: 2026-01-11
**Completion Date**: 2026-01-12
**Approach**: Strict TDD bounded autonomy - one sub-phase at a time
**Tests Passing**: 113 checkpoint + 13 storage + 11 CID-specific = 137 related tests passing

**E2E Verification (2026-01-12):**
- ✅ 4 messages recovered from 2 checkpoints
- ✅ 1563 tokens recovered
- ✅ SDK v1.8.6 released with full checkpoint support
- ✅ BlobIdentifier CIDs (59-65 chars) working correctly
- ✅ BLAKE3 hash verification passes
- ✅ CBOR-decoded deltas merged into recovered conversation

**Priority**: Critical for MVP - Enables SDK conversation recovery after session timeout

---

## SDK Compatibility Requirements (CRITICAL)

These requirements come from the SDK developer and are **essential for signature verification**:

### 1. JSON Key Ordering (Signature Breaking if Wrong)
```
# Node MUST produce JSON with:
# - Alphabetically sorted keys (recursive)
# - Compact format (no spaces)
# - Like Python: json.dumps(data, sort_keys=True, separators=(',', ':'))

# Example - messages array MUST serialize as:
'[{"content":"Hello","role":"user","timestamp":123}]'

# NOT (wrong key order):
'[{"role":"user","content":"Hello","timestamp":123}]'
```

**Rust Implementation:** Use `serde_json::to_string()` with a custom sorted serialization, or serialize to Value then sort keys recursively.

### 2. Proof Hash Verification
- SDK compares `checkpoint.proofHash === onChainProof.proofHash` (case-insensitive)
- SDK does NOT recompute hash from proof_data
- Node must use same hash for both checkpoint and on-chain submission

### 3. Session Resumption Required
- Node MUST fetch existing index from S5 on session resume
- Continue checkpoint numbering from last checkpoint
- Prevents duplicate checkpoint indices after node restart

### 4. CID Format
- Use raw CID without prefix: `bafybeig...`
- NOT: `s5://bafybeig...`

---

## Overview

Implementation plan for checkpoint publishing in fabstir-llm-node. At each proof submission (~1000 tokens), the node publishes a signed conversation delta to S5 storage BEFORE submitting the proof on-chain. This enables the SDK to recover conversation state up to the last proven checkpoint.

**Key Flow:**
```
At each proof submission (~1000 tokens):

1. Generate proof data (existing)
2. Compute proofHash = keccak256(proof_data) (existing)
3. Upload proof to S5 (existing)
4. Create delta JSON with messages since last checkpoint     ← NEW
5. Sign delta with EIP-191                                    ← NEW
6. Upload delta to S5                                         ← NEW
7. Update checkpoint index                                    ← NEW
8. Sign and upload index                                      ← NEW
9. Submit proof to chain (existing)
```

**Critical Requirement**: Steps 4-8 MUST complete BEFORE step 9. If S5 upload fails, do NOT submit proof.

**References:**
- Specification: `docs/sdk-reference/NODE_CHECKPOINT_SPEC.md`
- Existing Checkpoint Manager: `src/contracts/checkpoint_manager.rs`
- Existing Proof Signer: `src/crypto/proof_signer.rs`
- S5 Storage: `src/storage/s5_client.rs`, `src/storage/enhanced_s5_client.rs`
- Session Management: `src/api/websocket/session.rs`

---

## Dependencies

### Already Available (No Changes Needed)
```toml
[dependencies]
# Existing dependencies used:
serde = { version = "1.0", features = ["derive"] }  # JSON serialization
serde_json = "1.0"                                   # JSON parsing
sha2 = "0.10"                                        # SHA256 hashing
k256 = { version = "0.13", features = ["ecdsa"] }   # EIP-191 signing
tiny_keccak = { version = "2.0", features = ["keccak"] }  # Keccak256
hex = "0.4"                                          # Hex encoding
ethers = { version = "2.0", features = [...] }       # Address type
tokio = { version = "1", features = ["full"] }       # Async runtime
tracing = "0.1"                                      # Logging
anyhow = "1.0"                                       # Error handling
```

### Existing Infrastructure
- `S5Storage` trait - S5 upload/download operations
- `sign_proof_data()` - EIP-191 signing pattern
- `extract_node_private_key()` - Gets HOST_PRIVATE_KEY
- `CheckpointManager` - Token tracking, proof submission
- `WebSocketSession` - Conversation message history

---

## S5 Path Convention

### Checkpoint Index
```
home/checkpoints/{hostAddress}/{sessionId}/index.json
```

### Delta Storage
Deltas are content-addressed. CID is recorded in the index.

### Examples
```
# Index for session 123 from host 0xABC...
home/checkpoints/0xabc123def456789012345678901234567890abcd/123/index.json

# Delta CID (stored separately, referenced in index)
bafybeig123...  (content-addressed, raw CID without prefix)
```

**Important**: Host addresses in paths MUST be lowercase.

---

## Phase 1: Core Data Structures (2 hours)

### Sub-phase 1.1: Create Module Structure

**Goal**: Create the checkpoint module with stub files

**Status**: COMPLETE ✅

#### Tasks
- [x] Create `src/checkpoint/mod.rs` with submodule declarations
- [x] Create `src/checkpoint/delta.rs` stub
- [x] Create `src/checkpoint/index.rs` stub
- [x] Create `src/checkpoint/signer.rs` stub
- [x] Create `src/checkpoint/publisher.rs` stub
- [x] Create `src/checkpoint/cleanup.rs` stub
- [x] Add `pub mod checkpoint;` to `src/lib.rs`
- [x] Run `cargo check` to verify module structure compiles

**Implementation Files:**
- `src/checkpoint/mod.rs` (max 50 lines)
  ```rust
  //! Checkpoint Publishing for Conversation Recovery
  //!
  //! Publishes signed conversation checkpoints to S5 storage
  //! for SDK recovery after session timeout.
  //!
  //! ## Flow
  //! 1. Buffer conversation messages during inference
  //! 2. At each proof submission (~1000 tokens):
  //!    - Create delta with messages since last checkpoint
  //!    - Sign with EIP-191
  //!    - Upload to S5
  //!    - Update checkpoint index
  //! 3. THEN submit proof to chain
  //!
  //! ## Critical
  //! Checkpoint publishing MUST complete BEFORE proof submission.
  //! If S5 upload fails, proof submission is blocked.

  pub mod delta;
  pub mod index;
  pub mod signer;
  pub mod publisher;
  pub mod cleanup;

  pub use delta::{CheckpointDelta, CheckpointMessage};
  pub use index::{CheckpointIndex, CheckpointEntry, SessionState};
  pub use publisher::CheckpointPublisher;
  pub use signer::sign_checkpoint_data;
  ```

- `src/lib.rs` (modify - add 1 line)
  ```rust
  pub mod checkpoint;
  ```

---

### Sub-phase 1.2: Implement CheckpointDelta (TDD)

**Goal**: Define CheckpointDelta struct with serialization and signing

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_checkpoint_delta_serialization_camel_case`
- [x] Write test `test_checkpoint_message_serialization`
- [x] Write test `test_checkpoint_message_partial_field_optional`
- [x] Write test `test_delta_messages_json_sorted_keys` ← CRITICAL for SDK
- [x] Write test `test_sorted_keys_recursive_nested_objects`
- [x] Write test `test_delta_to_json_bytes`
- [x] Implement `CheckpointDelta` struct with serde attributes
- [x] Implement `CheckpointMessage` struct
- [x] Implement `sort_json_keys()` helper for SDK-compatible JSON
- [x] Implement `compute_messages_json()` with sorted keys
- [x] Implement `to_json_bytes()` for upload
- [x] Run tests: `cargo test checkpoint::delta` (12 tests passing)

**Test Files:**
- Inline tests in `src/checkpoint/delta.rs` (max 150 lines for tests)
  - Test camelCase serialization (sessionId, checkpointIndex, proofHash, etc.)
  - Test messages array serialization
  - Test partial field is omitted when None
  - Test deterministic JSON output for signing

**Implementation Files:**
- `src/checkpoint/delta.rs` (max 200 lines)
  ```rust
  use serde::{Deserialize, Serialize};

  /// A checkpoint delta containing messages since the last checkpoint
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct CheckpointDelta {
      /// Session ID (matches on-chain session)
      pub session_id: String,

      /// 0-based index of this checkpoint
      pub checkpoint_index: u32,

      /// bytes32 keccak256 hash of proof data (matches on-chain)
      pub proof_hash: String,

      /// Token count at start of this delta
      pub start_token: u64,

      /// Token count at end of this delta
      pub end_token: u64,

      /// Messages added since last checkpoint
      pub messages: Vec<CheckpointMessage>,

      /// EIP-191 signature of messages array
      pub host_signature: String,
  }

  /// A conversation message in a checkpoint delta
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct CheckpointMessage {
      /// "user" or "assistant"
      pub role: String,

      /// Message content
      pub content: String,

      /// Unix timestamp in milliseconds
      pub timestamp: u64,

      /// Optional metadata
      #[serde(skip_serializing_if = "Option::is_none")]
      pub metadata: Option<MessageMetadata>,
  }

  /// Optional metadata for checkpoint messages
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct MessageMetadata {
      /// True if message continues in next delta (streaming)
      #[serde(skip_serializing_if = "Option::is_none")]
      pub partial: Option<bool>,
  }

  impl CheckpointDelta {
      /// Create JSON string of messages array for signing
      /// CRITICAL: Uses alphabetically sorted keys for SDK compatibility
      pub fn compute_messages_json(&self) -> String {
          // Must sort keys alphabetically for SDK signature verification
          let value = serde_json::to_value(&self.messages).unwrap();
          let sorted = sort_json_keys(&value);
          serde_json::to_string(&sorted).unwrap()  // Compact, no spaces
      }

      /// Convert delta to JSON bytes for S5 upload
      pub fn to_json_bytes(&self) -> Vec<u8> {
          // Also sort keys for consistency
          let value = serde_json::to_value(self).unwrap();
          let sorted = sort_json_keys(&value);
          serde_json::to_vec_pretty(&sorted).unwrap()
      }
  }

  /// Recursively sort JSON object keys alphabetically
  /// Required for SDK signature verification compatibility
  fn sort_json_keys(value: &serde_json::Value) -> serde_json::Value {
      use serde_json::Value;
      match value {
          Value::Object(map) => {
              let mut sorted: serde_json::Map<String, Value> = serde_json::Map::new();
              let mut keys: Vec<_> = map.keys().collect();
              keys.sort(); // Alphabetical sort
              for key in keys {
                  sorted.insert(key.clone(), sort_json_keys(&map[key]));
              }
              Value::Object(sorted)
          }
          Value::Array(arr) => {
              Value::Array(arr.iter().map(sort_json_keys).collect())
          }
          _ => value.clone(),
      }
  }

  impl CheckpointMessage {
      pub fn new_user(content: String, timestamp: u64) -> Self {
          Self {
              role: "user".to_string(),
              content,
              timestamp,
              metadata: None,
          }
      }

      pub fn new_assistant(content: String, timestamp: u64, partial: bool) -> Self {
          Self {
              role: "assistant".to_string(),
              content,
              timestamp,
              metadata: if partial {
                  Some(MessageMetadata { partial: Some(true) })
              } else {
                  None
              },
          }
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_checkpoint_delta_serialization_camel_case() {
          // Verify JSON uses camelCase field names
      }

      #[test]
      fn test_checkpoint_message_serialization() {
          // Verify message serialization
      }

      #[test]
      fn test_checkpoint_message_partial_field_optional() {
          // Verify partial is omitted when None
      }

      #[test]
      fn test_delta_messages_json_sorted_keys() {
          // CRITICAL: Verify keys are alphabetically sorted for SDK
          let msg = CheckpointMessage::new_user("Hello".to_string(), 123);
          let delta = CheckpointDelta { /* ... */ };
          let json = delta.compute_messages_json();

          // Keys must be: content, role, timestamp (alphabetical)
          assert!(json.contains(r#""content":"Hello","role":"user","timestamp":123"#));
      }

      #[test]
      fn test_sorted_keys_recursive_nested_objects() {
          // Verify nested objects (metadata) also have sorted keys
          let msg = CheckpointMessage::new_assistant("Hi".to_string(), 124, true);
          let json = serde_json::to_string(&msg).unwrap();
          // metadata.partial should come after content but keys within metadata sorted
      }

      #[test]
      fn test_delta_to_json_bytes() {
          // Verify to_json_bytes produces valid JSON
      }
  }
  ```

---

### Sub-phase 1.3: Implement CheckpointIndex (TDD)

**Goal**: Define CheckpointIndex struct with S5 path generation

**Status**: COMPLETE ✅ (implemented in Sub-phase 1.1)

#### Tasks
- [x] Write test `test_checkpoint_index_serialization_camel_case`
- [x] Write test `test_checkpoint_entry_serialization`
- [x] Write test `test_s5_path_lowercase_address`
- [x] Write test `test_s5_path_format`
- [x] Write test `test_compute_checkpoints_json_deterministic`
- [x] Write test `test_session_state_serialization`
- [x] Implement `CheckpointIndex` struct
- [x] Implement `CheckpointEntry` struct
- [x] Implement `SessionState` enum
- [x] Implement `s5_path()` static method
- [x] Implement `compute_checkpoints_json()` for signing
- [x] Run tests: `cargo test checkpoint::index` (11 tests passing)

**Test Files:**
- Inline tests in `src/checkpoint/index.rs` (max 150 lines for tests)
  - Test camelCase serialization
  - Test S5 path uses lowercase host address
  - Test path format: `home/checkpoints/{host}/{session}/index.json`
  - Test deterministic JSON for signing

**Implementation Files:**
- `src/checkpoint/index.rs` (max 250 lines)
  ```rust
  use serde::{Deserialize, Serialize};

  /// Checkpoint index listing all checkpoints for a session
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct CheckpointIndex {
      /// Session ID
      pub session_id: String,

      /// Host's Ethereum address (lowercase)
      pub host_address: String,

      /// List of checkpoint entries
      pub checkpoints: Vec<CheckpointEntry>,

      /// EIP-191 signature of checkpoints array
      pub host_signature: String,
  }

  /// A single checkpoint entry in the index
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct CheckpointEntry {
      /// 0-based checkpoint index
      pub index: u32,

      /// bytes32 proof hash (matches on-chain)
      pub proof_hash: String,

      /// S5 CID where delta is stored
      pub delta_cid: String,

      /// [startToken, endToken] tuple
      pub token_range: [u64; 2],

      /// Unix timestamp in milliseconds
      pub timestamp: u64,
  }

  /// Session state for cleanup policy
  #[derive(Debug, Clone, Serialize, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub enum SessionState {
      Active,
      Completed,
      TimedOut,
      Cancelled,
  }

  impl CheckpointIndex {
      /// Generate S5 path for checkpoint index
      /// Format: home/checkpoints/{hostAddress}/{sessionId}/index.json
      pub fn s5_path(host_address: &str, session_id: &str) -> String {
          format!(
              "home/checkpoints/{}/{}/index.json",
              host_address.to_lowercase(),
              session_id
          )
      }

      /// Create JSON string of checkpoints array for signing
      pub fn compute_checkpoints_json(&self) -> String {
          serde_json::to_string(&self.checkpoints).unwrap()
      }

      /// Convert index to JSON bytes for S5 upload
      pub fn to_json_bytes(&self) -> Vec<u8> {
          serde_json::to_vec_pretty(self).unwrap()
      }

      /// Create empty index for new session
      pub fn new(session_id: String, host_address: String) -> Self {
          Self {
              session_id,
              host_address: host_address.to_lowercase(),
              checkpoints: Vec::new(),
              host_signature: String::new(),
          }
      }

      /// Add a checkpoint entry
      pub fn add_checkpoint(&mut self, entry: CheckpointEntry) {
          self.checkpoints.push(entry);
      }
  }

  impl CheckpointEntry {
      pub fn new(
          index: u32,
          proof_hash: String,
          delta_cid: String,
          start_token: u64,
          end_token: u64,
      ) -> Self {
          Self {
              index,
              proof_hash,
              delta_cid,
              token_range: [start_token, end_token],
              timestamp: std::time::SystemTime::now()
                  .duration_since(std::time::UNIX_EPOCH)
                  .unwrap()
                  .as_millis() as u64,
          }
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn test_checkpoint_index_serialization_camel_case() { }

      #[test]
      fn test_checkpoint_entry_serialization() { }

      #[test]
      fn test_s5_path_lowercase_address() {
          let path = CheckpointIndex::s5_path("0xABC123DEF", "session-1");
          assert!(path.contains("0xabc123def"));
      }

      #[test]
      fn test_s5_path_format() {
          let path = CheckpointIndex::s5_path("0xabc", "123");
          assert_eq!(path, "home/checkpoints/0xabc/123/index.json");
      }

      #[test]
      fn test_compute_checkpoints_json_deterministic() { }

      #[test]
      fn test_session_state_serialization() { }
  }
  ```

---

### Sub-phase 1.4: Implement Checkpoint Signer (TDD)

**Goal**: EIP-191 signing for checkpoint data

**Status**: COMPLETE ✅ (implemented in Sub-phase 1.1)

#### Tasks
- [x] Write test `test_sign_messages_returns_65_bytes` (as `test_sign_checkpoint_data_returns_correct_length`)
- [x] Write test `test_sign_messages_v_27_or_28` (as `test_sign_checkpoint_data_v_27_or_28`)
- [x] Write test `test_sign_messages_recoverable` (as `test_signature_is_recoverable`)
- [x] Write test `test_sign_checkpoints_returns_65_bytes` (same function works for both)
- [x] Write test `test_different_messages_different_signature` (as `test_different_data_different_signature`)
- [x] Write test `test_signature_hex_format` (covered by length test)
- [x] Implement `sign_checkpoint_data()` function
- [x] Implement `format_signature_hex()` helper (as `format_signature()`)
- [x] Run tests: `cargo test checkpoint::signer` (9 tests passing)

**Test Files:**
- Inline tests in `src/checkpoint/signer.rs` (max 150 lines for tests)
  - Test 65-byte signature output
  - Test v value is 27 or 28
  - Test signature is recoverable to host address
  - Test hex format with 0x prefix (132 chars total)

**Implementation Files:**
- `src/checkpoint/signer.rs` (max 200 lines)
  ```rust
  //! EIP-191 signing for checkpoint data
  //!
  //! Signs messages and checkpoints arrays for SDK verification.

  use anyhow::{anyhow, Result};
  use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, Signature, SigningKey};
  use tiny_keccak::{Hasher, Keccak};

  /// Sign data using EIP-191 personal_sign
  ///
  /// # Arguments
  /// * `private_key` - 32-byte host private key
  /// * `data` - JSON string to sign (messages or checkpoints array)
  ///
  /// # Returns
  /// 65-byte signature (r + s + v) as hex string with 0x prefix
  pub fn sign_checkpoint_data(private_key: &[u8; 32], data: &str) -> Result<String> {
      // 1. Create EIP-191 message hash
      let message_hash = eip191_hash(data.as_bytes());

      // 2. Sign with ECDSA
      let signing_key = SigningKey::from_bytes(private_key.into())
          .map_err(|e| anyhow!("Invalid private key: {}", e))?;

      let (signature, recovery_id) = signing_key
          .sign_prehash_recoverable(&message_hash)
          .map_err(|e| anyhow!("Signing failed: {}", e))?;

      // 3. Format as 65-byte signature with v = 27 or 28
      let sig_bytes = format_signature(signature, recovery_id);

      // 4. Return as hex with 0x prefix
      Ok(format!("0x{}", hex::encode(sig_bytes)))
  }

  /// Create EIP-191 message hash
  /// prefix = "\x19Ethereum Signed Message:\n" + len(message)
  fn eip191_hash(message: &[u8]) -> [u8; 32] {
      let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());

      let mut hasher = Keccak::v256();
      hasher.update(prefix.as_bytes());
      hasher.update(message);

      let mut hash = [0u8; 32];
      hasher.finalize(&mut hash);
      hash
  }

  /// Format signature as 65 bytes (r + s + v)
  fn format_signature(signature: Signature, recovery_id: RecoveryId) -> [u8; 65] {
      let mut sig_bytes = [0u8; 65];
      sig_bytes[..64].copy_from_slice(&signature.to_bytes());
      sig_bytes[64] = recovery_id.to_byte() + 27; // Ethereum v value
      sig_bytes
  }

  #[cfg(test)]
  mod tests {
      use super::*;
      use k256::ecdsa::SigningKey;
      use rand::rngs::OsRng;

      fn generate_test_key() -> [u8; 32] {
          let signing_key = SigningKey::random(&mut OsRng);
          signing_key.to_bytes().into()
      }

      #[test]
      fn test_sign_messages_returns_65_bytes() {
          let key = generate_test_key();
          let data = r#"[{"role":"user","content":"hello"}]"#;
          let sig = sign_checkpoint_data(&key, data).unwrap();

          // 0x + 130 hex chars = 132 total
          assert_eq!(sig.len(), 132);
          assert!(sig.starts_with("0x"));
      }

      #[test]
      fn test_sign_messages_v_27_or_28() {
          let key = generate_test_key();
          let sig = sign_checkpoint_data(&key, "test").unwrap();

          let sig_bytes = hex::decode(&sig[2..]).unwrap();
          let v = sig_bytes[64];
          assert!(v == 27 || v == 28);
      }

      #[test]
      fn test_different_messages_different_signature() {
          let key = generate_test_key();
          let sig1 = sign_checkpoint_data(&key, "message1").unwrap();
          let sig2 = sign_checkpoint_data(&key, "message2").unwrap();
          assert_ne!(sig1, sig2);
      }
  }
  ```

---

## Phase 2: CheckpointPublisher Core (3 hours)

### Sub-phase 2.1: Session State Management

**Goal**: Track per-session checkpoint state (message buffer, checkpoint index)

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_session_state_new` (as `test_session_checkpoint_state_new`)
- [x] Write test `test_buffer_message_accumulates`
- [x] Write test `test_buffer_message_increments_count`
- [x] Write test `test_get_buffered_messages_returns_copy`
- [x] Write test `test_clear_buffer_empties_messages`
- [x] Write test `test_increment_checkpoint_index`
- [x] Implement `SessionCheckpointState` struct
- [x] Implement message buffer methods (get_buffered_messages, clear_buffer, increment_checkpoint_index, buffer_size)
- [x] Run tests: `cargo test checkpoint::publisher` (12 tests passing)

**Test Files:**
- Inline tests in `src/checkpoint/publisher.rs` (max 100 lines for session tests)

**Implementation Files:**
- `src/checkpoint/publisher.rs` (max 500 lines total)
  ```rust
  use crate::checkpoint::{CheckpointDelta, CheckpointEntry, CheckpointIndex, CheckpointMessage};
  use crate::storage::S5Storage;
  use anyhow::{anyhow, Result};
  use ethers::types::Address;
  use std::collections::HashMap;
  use std::sync::Arc;
  use tokio::sync::RwLock;
  use tracing::{error, info, warn};

  const MAX_S5_RETRIES: u32 = 3;
  const S5_RETRY_DELAY_MS: u64 = 1000;

  /// Per-session checkpoint state
  #[derive(Debug, Clone)]
  pub struct SessionCheckpointState {
      /// Current checkpoint index (0-based)
      pub checkpoint_index: u32,

      /// Messages buffered since last checkpoint
      pub message_buffer: Vec<CheckpointMessage>,

      /// Token count at last checkpoint
      pub last_checkpoint_tokens: u64,

      /// Cached checkpoint index (loaded from S5 or created new)
      pub index: Option<CheckpointIndex>,
  }

  impl SessionCheckpointState {
      pub fn new() -> Self {
          Self {
              checkpoint_index: 0,
              message_buffer: Vec::new(),
              last_checkpoint_tokens: 0,
              index: None,
          }
      }

      pub fn buffer_message(&mut self, message: CheckpointMessage) {
          self.message_buffer.push(message);
      }

      pub fn get_buffered_messages(&self) -> Vec<CheckpointMessage> {
          self.message_buffer.clone()
      }

      pub fn clear_buffer(&mut self) {
          self.message_buffer.clear();
      }

      pub fn increment_checkpoint(&mut self, end_token: u64) {
          self.checkpoint_index += 1;
          self.last_checkpoint_tokens = end_token;
          self.clear_buffer();
      }
  }
  ```

---

### Sub-phase 2.2: S5 Upload with Retry

**Goal**: Implement S5 upload with exponential backoff retry

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_upload_succeeds_first_try`
- [x] Write test `test_upload_retries_on_transient_failure` (tests retry recovery)
- [x] Write test `test_upload_fails_with_invalid_path` (tests persistent failure)
- [x] Write test `test_upload_returns_cid_from_mock`
- [x] Write test `test_upload_different_paths_succeed`
- [x] Implement `upload_with_retry()` async function
- [x] Add exponential backoff logic (1s, 2s, 4s delays)
- [x] Run tests: `cargo test checkpoint::publisher` (17 tests passing)

**Test Files:**
- Inline tests in `src/checkpoint/publisher.rs` (max 100 lines for upload tests)
  - Use MockS5Backend for testing

**Implementation Files:**
- `src/checkpoint/publisher.rs` (continue from 2.1)
  ```rust
  impl CheckpointPublisher {
      /// Upload data to S5 with retry
      async fn upload_with_retry(&self, path: &str, data: Vec<u8>) -> Result<String> {
          let mut last_error = None;

          for attempt in 1..=MAX_S5_RETRIES {
              match self.s5_storage.put(path, data.clone()).await {
                  Ok(cid) => {
                      info!("S5 upload succeeded on attempt {}: {}", attempt, cid);
                      return Ok(cid);
                  }
                  Err(e) => {
                      warn!("S5 upload attempt {} failed: {}", attempt, e);
                      last_error = Some(e);

                      if attempt < MAX_S5_RETRIES {
                          let delay = S5_RETRY_DELAY_MS * attempt as u64;
                          tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                      }
                  }
              }
          }

          Err(anyhow!(
              "S5 upload failed after {} retries: {:?}",
              MAX_S5_RETRIES,
              last_error
          ))
      }
  }
  ```

---

### Sub-phase 2.3: Publish Checkpoint Core Logic

**Goal**: Implement the main `publish_checkpoint()` method

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_publish_checkpoint_creates_delta`
- [x] Write test `test_publish_checkpoint_signs_messages`
- [x] Write test `test_publish_checkpoint_uploads_delta`
- [x] Write test `test_publish_checkpoint_updates_index`
- [x] Write test `test_publish_checkpoint_returns_delta_cid`
- [x] Write test `test_publish_checkpoint_blocks_on_s5_failure`
- [x] Write test `test_publish_checkpoint_clears_buffer`
- [x] Write test `test_publish_checkpoint_increments_index`
- [x] Write test `test_publish_checkpoint_proof_hash_in_delta`
- [x] Implement `publish_checkpoint()` method with delta creation, signing, and upload
- [x] Implement `init_session()` method for session resumption
- [x] Run tests: `cargo test checkpoint::publisher` (26 tests passing)

**Test Files:**
- Inline tests in `src/checkpoint/publisher.rs` (max 200 lines for publish tests)
  - Use MockS5Backend for testing

**Implementation Files:**
- `src/checkpoint/publisher.rs` (continue)
  ```rust
  /// Main checkpoint publisher
  pub struct CheckpointPublisher {
      s5_storage: Box<dyn S5Storage>,
      host_address: Address,
      sessions: Arc<RwLock<HashMap<String, SessionCheckpointState>>>,
  }

  impl CheckpointPublisher {
      pub fn new(s5_storage: Box<dyn S5Storage>, host_address: Address) -> Self {
          Self {
              s5_storage,
              host_address,
              sessions: Arc::new(RwLock::new(HashMap::new())),
          }
      }

      /// Add a message to the buffer for a session
      pub async fn buffer_message(&self, session_id: &str, message: CheckpointMessage) {
          let mut sessions = self.sessions.write().await;
          let state = sessions
              .entry(session_id.to_string())
              .or_insert_with(SessionCheckpointState::new);
          state.buffer_message(message);
      }

      /// CRITICAL: Publish checkpoint to S5 BEFORE proof submission
      ///
      /// Returns Ok(delta_cid) on success.
      /// Returns Err if S5 upload fails - caller MUST NOT submit proof.
      pub async fn publish_checkpoint(
          &self,
          session_id: &str,
          proof_hash: [u8; 32],
          start_token: u64,
          end_token: u64,
          private_key: &[u8; 32],
      ) -> Result<String> {
          let proof_hash_hex = format!("0x{}", hex::encode(proof_hash));

          // 1. Get session state
          let mut sessions = self.sessions.write().await;
          let state = sessions
              .get_mut(session_id)
              .ok_or_else(|| anyhow!("No checkpoint state for session {}", session_id))?;

          let messages = state.get_buffered_messages();
          let checkpoint_index = state.checkpoint_index;

          info!(
              "Publishing checkpoint {} for session {} ({} messages, tokens {}-{})",
              checkpoint_index, session_id, messages.len(), start_token, end_token
          );

          // 2. Create and sign delta
          let messages_json = serde_json::to_string(&messages)?;
          let delta_signature = crate::checkpoint::sign_checkpoint_data(private_key, &messages_json)?;

          let delta = CheckpointDelta {
              session_id: session_id.to_string(),
              checkpoint_index,
              proof_hash: proof_hash_hex.clone(),
              start_token,
              end_token,
              messages,
              host_signature: delta_signature,
          };

          // 3. Upload delta to S5 (with retry)
          let delta_bytes = delta.to_json_bytes();
          let delta_cid = self
              .upload_with_retry(&format!("checkpoint_delta_{}", checkpoint_index), delta_bytes)
              .await
              .map_err(|e| {
                  error!("Delta upload failed for session {}: {}", session_id, e);
                  e
              })?;

          info!("Delta uploaded: {}", delta_cid);

          // 4. Update checkpoint index
          let index = state.index.get_or_insert_with(|| {
              CheckpointIndex::new(
                  session_id.to_string(),
                  format!("{:?}", self.host_address),
              )
          });

          let entry = CheckpointEntry::new(
              checkpoint_index,
              proof_hash_hex,
              delta_cid.clone(),
              start_token,
              end_token,
          );
          index.add_checkpoint(entry);

          // 5. Sign and upload index
          let checkpoints_json = index.compute_checkpoints_json();
          let index_signature = crate::checkpoint::sign_checkpoint_data(private_key, &checkpoints_json)?;
          index.host_signature = index_signature;

          let index_path = CheckpointIndex::s5_path(
              &format!("{:?}", self.host_address),
              session_id,
          );
          let index_bytes = index.to_json_bytes();

          self.upload_with_retry(&index_path, index_bytes)
              .await
              .map_err(|e| {
                  error!("Index upload failed for session {}: {}", session_id, e);
                  e
              })?;

          info!("Index uploaded to {}", index_path);

          // 6. Update state
          state.increment_checkpoint(end_token);

          Ok(delta_cid)
      }

      /// Clean up session state after completion
      pub async fn cleanup_session(&self, session_id: &str) {
          let mut sessions = self.sessions.write().await;
          sessions.remove(session_id);
      }
  }
  ```

---

### Sub-phase 2.4: Session Resumption from S5

**Goal**: Fetch existing checkpoint index from S5 on session resume

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_init_session_fetches_index_from_s5`
- [x] Write test `test_init_session_continues_checkpoint_numbering`
- [x] Write test `test_new_session_starts_at_zero`
- [x] Write test `test_init_session_handles_missing_index`
- [x] Write test `test_init_session_then_publish_continues_correctly` (integration test)
- [x] Implement `init_session()` method (done in Sub-phase 2.3)
- [x] Verify publish_checkpoint() works correctly with resumed state
- [x] Run tests: `cargo test checkpoint::publisher` (31 tests passing)

**Why This Matters:**
```
Without resumption (BAD):
[Session] → [checkpoint 0] → [NODE RESTART] → [checkpoint 0 again!]
                                               ↑ Overwrites first checkpoint

With resumption (GOOD):
[Session] → [checkpoint 0] → [NODE RESTART] → [fetch index] → [checkpoint 1]
                                               ↑ Continues correctly
```

**Implementation Files:**
- `src/checkpoint/publisher.rs` (add to CheckpointPublisher)
  ```rust
  impl CheckpointPublisher {
      /// Resume session from S5 or create new
      /// Called when session starts (before any checkpoints)
      pub async fn resume_or_create_session(&self, session_id: &str) -> Result<()> {
          let index_path = CheckpointIndex::s5_path(
              &format!("{:?}", self.host_address),
              session_id,
          );

          // Try to fetch existing index from S5
          match self.s5_storage.get(&index_path).await {
              Ok(bytes) => {
                  let existing_index: CheckpointIndex = serde_json::from_slice(&bytes)?;

                  // Resume from last checkpoint
                  let last_checkpoint = existing_index.checkpoints.last();
                  let (next_index, last_token) = match last_checkpoint {
                      Some(cp) => (cp.index + 1, cp.token_range[1]),
                      None => (0, 0),
                  };

                  info!(
                      "Resuming session {} from checkpoint {} (last token: {})",
                      session_id, next_index, last_token
                  );

                  let mut sessions = self.sessions.write().await;
                  let state = sessions
                      .entry(session_id.to_string())
                      .or_insert_with(SessionCheckpointState::new);

                  state.checkpoint_index = next_index;
                  state.last_checkpoint_tokens = last_token;
                  state.index = Some(existing_index);
              }
              Err(_) => {
                  // No existing index - fresh session
                  info!("Starting fresh session {}", session_id);
              }
          }

          Ok(())
      }
  }
  ```

---

## Phase 3: CheckpointManager Integration (2 hours)

### Sub-phase 3.1: Add CheckpointPublisher to CheckpointManager

**Goal**: Integrate CheckpointPublisher into existing CheckpointManager

**Status**: COMPLETE ✅

#### Tasks
- [x] Add `checkpoint_publisher: Arc<CheckpointPublisher>` field to CheckpointManager
- [x] Update `CheckpointManager::new()` to create publisher
- [x] Add `checkpoint_publisher()` accessor method
- [x] Add `track_conversation_message()` method
- [x] Add `init_checkpoint_session()` method that calls publisher's `init_session()`
- [x] Add `cleanup_checkpoint_session()` method for session cleanup
- [x] Run `cargo check` - PASSED
- [x] Run checkpoint tests - 92 tests passing

**Implementation Files:**
- `src/contracts/checkpoint_manager.rs` (modify - add ~50 lines)
  ```rust
  // Add to struct fields:
  checkpoint_publisher: Option<Arc<CheckpointPublisher>>,

  // Add to new():
  let checkpoint_publisher = s5_storage.as_ref().map(|s5| {
      Arc::new(CheckpointPublisher::new(
          s5.clone_box(),
          host_address,
      ))
  });

  // Add method:
  pub async fn track_conversation_message(
      &self,
      session_id: &str,
      role: &str,
      content: &str,
      partial: bool,
  ) {
      if let Some(publisher) = &self.checkpoint_publisher {
          let timestamp = std::time::SystemTime::now()
              .duration_since(std::time::UNIX_EPOCH)
              .unwrap()
              .as_millis() as u64;

          let message = if role == "user" {
              CheckpointMessage::new_user(content.to_string(), timestamp)
          } else {
              CheckpointMessage::new_assistant(content.to_string(), timestamp, partial)
          };

          publisher.buffer_message(session_id, message).await;
      }
  }
  ```

---

### Sub-phase 3.2: Integrate into submit_checkpoint_async

**Goal**: Call checkpoint publisher BEFORE chain submission

**Status**: COMPLETE ✅

#### Tasks
- [x] Modify `submit_checkpoint_async()` to accept session_id, checkpoint_publisher, previous_checkpoint_tokens
- [x] Add checkpoint publishing logic after proof upload, before chain submission
- [x] Add error handling to block proof on publish failure (returns Err, does NOT submit)
- [x] Update all 3 call sites to pass new parameters
- [x] Write integration tests:
  - `test_checkpoint_publisher_initialization`
  - `test_track_conversation_message_user`
  - `test_track_conversation_message_assistant_partial`
  - `test_cleanup_checkpoint_session`
  - `test_session_id_in_job_tracker`
- [x] Run tests: 97 checkpoint tests passing

**Critical Integration Point:**
- File: `src/contracts/checkpoint_manager.rs`
- Location: Line ~608, after S5 proof upload, before chain submission

**Implementation Files:**
- `src/contracts/checkpoint_manager.rs` (modify `submit_checkpoint_async`)
  ```rust
  // After line 608: let proof_cid = Self::upload_proof_to_s5_static(...).await?;

  // NEW: Publish checkpoint to S5 BEFORE chain submission
  if let Some(session_id) = session_id.as_ref() {
      if let Some(publisher) = checkpoint_publisher.as_ref() {
          let start_token = tokens_generated.saturating_sub(tokens_to_submit);

          match publisher
              .publish_checkpoint(
                  session_id,
                  proof_hash_bytes,
                  start_token,
                  tokens_generated,
                  &private_key,
              )
              .await
          {
              Ok(delta_cid) => {
                  info!(
                      "Checkpoint {} published to S5: {}",
                      session_id, delta_cid
                  );
              }
              Err(e) => {
                  error!(
                      "S5 checkpoint upload failed - NOT submitting proof: {}",
                      e
                  );
                  return Err(anyhow!(
                      "Checkpoint publishing failed - proof NOT submitted: {}",
                      e
                  ));
              }
          }
      }
  }

  // THEN proceed to chain submission (existing code at line ~628)
  let data = encode_checkpoint_call(...);
  ```

---

### Sub-phase 3.3: Add Session ID to Token Tracking

**Goal**: Pass session_id through token tracking flow

**Status**: COMPLETE ✅

#### Tasks
- [x] Verify `JobTokenTracker.session_id` is populated on tracker creation
- [x] Update `track_tokens()` to update session_id if initially None
- [x] Verify session_id flows from `track_tokens()` to `submit_checkpoint_async()`
- [x] Add tests:
  - `test_session_id_updated_when_initially_none`
  - `test_session_id_not_overwritten_if_already_set`
- [x] Run `cargo check` and tests - 99 tests passing

**Implementation Files:**
- `src/contracts/checkpoint_manager.rs` (modified)
  - Added logic to update session_id if provided later and not already set

---

## Phase 4: Session Message Integration (2 hours)

### Sub-phase 4.1: Track User Messages

**Goal**: Buffer user prompts when inference starts

**Status**: COMPLETE ✅

#### Tasks
- [x] Identify inference start points in `src/api/server.rs`
- [x] Identify inference start points in `src/api/websocket/handlers/`
- [x] Call `checkpoint_manager.init_session()` on session start (for resumption) - N/A for this sub-phase
- [x] Add `checkpoint_manager.track_conversation_message()` call for user prompt
- [x] Run `cargo check` - PASSED (99 tests passing)

**Implementation Files:**
- `src/api/server.rs` (modify HTTP inference path)
  ```rust
  // Before inference starts:
  if let Some(session_id) = &job_details.session_id {
      checkpoint_manager
          .track_conversation_message(session_id, "user", &request.prompt, false)
          .await;
  }
  ```

- `src/api/websocket/handlers/inference.rs` (modify WebSocket path)
  ```rust
  // On Inference message:
  checkpoint_manager
      .track_conversation_message(&session.id, "user", &inference.prompt, false)
      .await;
  ```

---

### Sub-phase 4.2: Track Assistant Responses

**Goal**: Buffer assistant response at completion (or partial at checkpoint)

**Status**: COMPLETE ✅

#### Tasks
- [x] Identify response completion points
- [x] Add response accumulation buffer (for streaming) - Already exists as `accumulated_text`
- [x] Add `track_conversation_message()` call for assistant response
- [x] Handle partial flag for streaming responses - Using `false` for complete responses
- [x] Run `cargo check` - PASSED (99 tests passing)

**Implementation Files:**
- `src/api/server.rs` (modify)
  ```rust
  // After response completes:
  if let Some(session_id) = &job_details.session_id {
      checkpoint_manager
          .track_conversation_message(
              session_id,
              "assistant",
              &accumulated_response,
              false, // not partial - complete response
          )
          .await;
  }
  ```

---

### Sub-phase 4.3: Handle Streaming Partial Responses

**Goal**: Mark in-progress responses as partial at checkpoint time

**Status**: COMPLETE ✅

#### Tasks
- [x] Add `streaming_response: Option<String>` to SessionCheckpointState
- [x] Add `update_streaming_response()`, `clear_streaming_response()`, `get_streaming_response()` methods
- [x] Add methods to CheckpointPublisher for managing streaming responses
- [x] On checkpoint trigger: include buffered response with `partial: true`
- [x] On response completion: clear streaming buffer (full message tracked separately)
- [x] Write test `test_streaming_response_marked_partial` - PASSED
- [x] Write test `test_partial_replaced_on_completion` - PASSED
- [x] Run tests: 105 checkpoint tests passing (6 new streaming tests)

**Implementation Files:**
- `src/checkpoint/publisher.rs` (modify)
  ```rust
  // Add to SessionCheckpointState:
  pub streaming_response: Option<String>,

  // Method to update streaming response:
  pub fn update_streaming_response(&mut self, chunk: &str) {
      match &mut self.streaming_response {
          Some(buffer) => buffer.push_str(chunk),
          None => self.streaming_response = Some(chunk.to_string()),
      }
  }

  // At checkpoint time, if streaming:
  if let Some(partial_response) = &state.streaming_response {
      messages.push(CheckpointMessage::new_assistant(
          partial_response.clone(),
          timestamp,
          true, // partial
      ));
  }
  ```

---

## Phase 5: Cleanup Policy (1 hour)

### Sub-phase 5.1: Implement Cleanup Methods

**Goal**: TTL-based cleanup for checkpoint data

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_cleanup_completed_session_7_days` - PASSED
- [x] Write test `test_cleanup_timed_out_session_30_days` - PASSED
- [x] Write test `test_cleanup_cancelled_session_immediate` - PASSED
- [x] Implement `cleanup_checkpoints()` async function with S5 integration
- [x] Add `CleanupResult` enum for operation results
- [x] Add `delete_all_checkpoints()` for immediate deletion
- [x] Add `mark_for_cleanup()` for TTL-based cleanup
- [x] Run tests: 110 checkpoint tests passing (5 new async cleanup tests)

**Cleanup Policy:**
| Event | TTL |
|-------|-----|
| Session completed normally | 7 days |
| Session timeout | 30 days |
| Session cancelled | Immediate |
| Dispute opened | Keep until resolved + 7 days |

**Implementation Files:**
- `src/checkpoint/cleanup.rs` (max 150 lines)
  ```rust
  use crate::checkpoint::index::{CheckpointIndex, SessionState};
  use crate::storage::S5Storage;
  use anyhow::Result;
  use tracing::{info, warn};

  const COMPLETED_TTL_DAYS: u64 = 7;
  const TIMEOUT_TTL_DAYS: u64 = 30;

  pub async fn cleanup_checkpoints(
      s5_storage: &dyn S5Storage,
      host_address: &str,
      session_id: &str,
      state: SessionState,
  ) -> Result<()> {
      let index_path = CheckpointIndex::s5_path(host_address, session_id);

      match state {
          SessionState::Cancelled => {
              // Immediate deletion
              delete_all_checkpoints(s5_storage, &index_path).await?;
          }
          SessionState::Completed => {
              // Mark for cleanup after 7 days
              mark_for_cleanup(s5_storage, &index_path, COMPLETED_TTL_DAYS).await?;
          }
          SessionState::TimedOut => {
              // Mark for cleanup after 30 days
              mark_for_cleanup(s5_storage, &index_path, TIMEOUT_TTL_DAYS).await?;
          }
          SessionState::Active => {
              // Do nothing
          }
      }

      Ok(())
  }

  async fn delete_all_checkpoints(
      s5_storage: &dyn S5Storage,
      index_path: &str,
  ) -> Result<()> {
      // 1. Fetch index
      // 2. Delete all deltas
      // 3. Delete index
      info!("Deleted checkpoints at {}", index_path);
      Ok(())
  }

  async fn mark_for_cleanup(
      s5_storage: &dyn S5Storage,
      index_path: &str,
      ttl_days: u64,
  ) -> Result<()> {
      // Update index with expires_at timestamp
      info!("Marked {} for cleanup in {} days", index_path, ttl_days);
      Ok(())
  }
  ```

---

## Phase 6: Testing & Finalization (2 hours)

### Sub-phase 6.1: Integration Tests

**Goal**: End-to-end checkpoint publishing tests

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_full_checkpoint_flow` - 1000 tokens triggers checkpoint
- [x] Write test `test_multiple_checkpoints` - 3000 tokens = 3 checkpoints
- [x] Write test `test_checkpoint_recovery_by_sdk` - verify data format
- [x] Write test `test_s5_failure_blocks_proof` - no orphaned proofs
- [x] Write test `test_session_resumption_from_s5` - session continuity
- [x] Write test `test_cleanup_deletes_all_checkpoint_data` - cleanup integration
- [x] Write test `test_checkpoint_signatures_verifiable` - EIP-191 format
- [x] Write test `test_json_keys_alphabetically_sorted` - SDK compatibility
- [x] Create `tests/checkpoint/test_checkpoint_publishing.rs`
- [x] Run tests: 8 integration tests passing

**Test Files:**
- `tests/checkpoint_tests.rs` (max 400 lines)
  ```rust
  //! Integration tests for checkpoint publishing

  use fabstir_llm_node::checkpoint::*;
  use fabstir_llm_node::storage::MockS5Backend;

  #[tokio::test]
  async fn test_full_checkpoint_flow() {
      // 1. Create publisher with mock S5
      // 2. Buffer messages
      // 3. Call publish_checkpoint
      // 4. Verify delta uploaded
      // 5. Verify index updated
  }

  #[tokio::test]
  async fn test_multiple_checkpoints() {
      // Verify checkpoint index accumulates entries
  }

  #[tokio::test]
  async fn test_s5_failure_blocks_proof() {
      // Configure mock to fail
      // Verify publish_checkpoint returns Err
  }
  ```

---

### Sub-phase 6.2: Update Version

**Goal**: Bump version and document feature

**Status**: COMPLETE ✅

#### Tasks
- [x] Update `VERSION` file to `8.11.0-checkpoint-publishing`
- [x] Update `src/version.rs`:
  - [x] VERSION constant to "v8.11.0-checkpoint-publishing-2026-01-11"
  - [x] VERSION_NUMBER to "8.11.0"
  - [x] VERSION_MINOR to 11, VERSION_PATCH to 0
  - [x] Add 8 checkpoint publishing features
  - [x] Add 7 BREAKING_CHANGES entries for v8.11.0
- [x] Run `cargo test version` - 3 version tests passing

**Implementation Files:**
- `VERSION`: `8.11.0-checkpoint-publishing`
- `src/version.rs`:
  ```rust
  pub const VERSION: &str = "v8.11.0-checkpoint-publishing-2026-01-XX";
  pub const VERSION_NUMBER: &str = "8.11.0";

  pub const FEATURES: &[&str] = &[
      // ... existing features ...
      "checkpoint-publishing",
      "conversation-recovery",
      "sdk-checkpoint-recovery",
  ];

  pub const BREAKING_CHANGES: &[&str] = &[
      // ... existing entries ...
      "FEAT: Checkpoint publishing for SDK conversation recovery (v8.11.0)",
  ];
  ```

---

### Sub-phase 6.3: Documentation

**Goal**: Update API and deployment docs

**Status**: COMPLETE ✅

#### Tasks
- [x] Add checkpoint publishing section to `docs/API.md`
- [x] Document S5 path convention
- [x] Document cleanup policy
- [x] Add troubleshooting section

**Added to `docs/API.md`:**
- New "Checkpoint Publishing (v8.11.0+)" section (~370 lines)
- Architecture overview with ASCII diagram
- S5 path convention documentation
- Checkpoint delta and index format specifications
- Signature requirements and SDK compatibility
- Cleanup policy (7 days completed, 30 days timeout, immediate cancelled)
- SDK conversation recovery example code
- Troubleshooting guide for common issues
- Log message patterns and environment variables

---

### Sub-phase 6.4: Create Release Tarball

**Goal**: Package release binary

**Status**: COMPLETE ✅

#### Tasks
- [x] Build release: `cargo build --release --features real-ezkl -j 4`
- [x] Verify version in binary: `v8.11.0-checkpoint-publishing-2026-01-11`
- [x] Copy binary to root
- [x] Create tarball with correct structure
- [x] Verify tarball contents
- [x] Clean up

**Tarball Created:**
- Filename: `fabstir-llm-node-v8.11.0-checkpoint-publishing.tar.gz`
- Size: 557MB (compressed from ~990MB binary)
- Contents:
  - `fabstir-llm-node` (at root, not in target/release/)
  - `scripts/download_florence_model.sh`
  - `scripts/download_ocr_models.sh`
  - `scripts/download_embedding_model.sh`
  - `scripts/setup_models.sh`

---

## Phase 7: HTTP Checkpoint Endpoint (SDK Access)

**Why This Phase Is Needed:**

S5's `home/` directory is a per-user private namespace. When SDK queries `home/checkpoints/...`, it accesses *its own* home directory, not the node's. The HTTP endpoint bridges this gap - SDK fetches index via HTTP, then retrieves deltas from S5 using globally-addressable CIDs.

**Endpoint:** `GET /v1/checkpoints/{sessionId}`

**Responses:**
- `200 OK` - Return CheckpointIndex JSON
- `404 Not Found` - No checkpoints exist for session
- `400 Bad Request` - Invalid session ID format
- `500 Internal Server Error` - S5 storage error

**CORS:** Already globally configured in http_server.rs (allow all origins/methods/headers).

---

### Sub-phase 7.1: Add CheckpointManager Accessor Methods

**Goal**: Expose host_address and s5_storage from CheckpointManager for HTTP handler

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_get_host_address_returns_lowercase`
- [x] Write test `test_get_s5_storage_returns_storage`
- [x] Implement `get_host_address()` method in CheckpointManager
- [x] Implement `get_s5_storage()` method in CheckpointManager
- [x] Run `cargo check` to verify compilation
- [x] Run tests: `cargo test checkpoint_manager::tests` (4 tests passed)

**Implementation Files:**
- `src/contracts/checkpoint_manager.rs` (add ~20 lines)

**Code to add:**
```rust
impl CheckpointManager {
    /// Get the host's Ethereum address (lowercase, 0x prefixed)
    pub fn get_host_address(&self) -> String {
        format!("{:#x}", self.host_address).to_lowercase()
    }

    /// Get reference to S5 storage for checkpoint retrieval
    pub fn get_s5_storage(&self) -> &dyn S5Storage {
        self.s5_storage.as_ref()
    }
}
```

---

### Sub-phase 7.2: Implement Checkpoint Handler (TDD)

**Goal**: Create HTTP handler with full test coverage

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_checkpoints_handler_returns_index_on_success`
- [x] Write test `test_checkpoints_handler_returns_404_when_not_found`
- [x] Write test `test_checkpoints_handler_returns_500_on_storage_error`
- [x] Implement `checkpoints_handler` function
- [x] Run tests: `cargo test checkpoint_handler_tests` (3 tests passed)

**Implementation Files:**
- `src/api/http_server.rs` (add ~50 lines for handler)
- `tests/api/test_checkpoints_endpoint.rs` (new file, ~100 lines)

**Handler Signature:**
```rust
async fn checkpoints_handler(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<axum::response::Json<CheckpointIndex>, ApiErrorResponse>
```

**Logic:**
1. Get checkpoint_manager from state
2. Get host_address from checkpoint_manager
3. Get s5_storage from checkpoint_manager
4. Build path: `CheckpointIndex::s5_path(&host_address, &session_id)`
5. Fetch from S5:
   - Success → deserialize and return 200
   - NotFound → return 404
   - Other error → return 500

---

### Sub-phase 7.3: Add Route and Integration

**Goal**: Register route and verify end-to-end

**Status**: COMPLETE ✅

#### Tasks
- [x] Add import: `use crate::checkpoint::index::CheckpointIndex;` (added in 7.2)
- [x] Add route: `.route("/v1/checkpoints/:session_id", get(checkpoints_handler))`
- [x] Run `cargo check` to verify compilation
- [x] Run `cargo build --release`
- [x] Manual test with curl (if node running with test data) - N/A, no test data

**Implementation Files:**
- `src/api/http_server.rs` (add 2 lines - import + route)

**Route Location:** After line ~96 in create_app()
```rust
.route("/v1/checkpoints/:session_id", get(checkpoints_handler))
```

---

### Sub-phase 7.4: Update Documentation and Version

**Goal**: Document endpoint and bump version

**Status**: COMPLETE ✅

#### Tasks
- [x] Add endpoint documentation to `docs/API.md`
- [x] Update VERSION file to `8.11.1-checkpoint-http-endpoint`
- [x] Update `src/version.rs` with new version constants
- [x] Add feature flag: `http-checkpoint-endpoint`, `checkpoint-index-api`
- [x] Run version tests: `cargo test --lib version` (4 tests passing)

**Documentation to add to API.md:**
```markdown
### Get Checkpoint Index

Retrieve checkpoint index for SDK conversation recovery.

#### Request

\`\`\`http
GET /v1/checkpoints/{sessionId}
\`\`\`

#### Response (200 OK)

\`\`\`json
{
  "sessionId": "123",
  "hostAddress": "0xabc...",
  "checkpoints": [...],
  "hostSignature": "0x..."
}
\`\`\`

#### Error Responses

- `404 Not Found` - No checkpoints exist
- `500 Internal Server Error` - S5 storage error
```

---

## Phase 8: S5 BlobIdentifier CID Format Fix

**Why This Phase Is Needed:**

The SDK developer reported that S5 portals reject our CIDs. The S5.js developer clarified:
- `pathToCID()` → 32-byte raw hash → **53 chars** → Portal **REJECTS** this
- `pathToBlobCID()` → BlobIdentifier with file size → **~59 chars** → **REQUIRED for portal downloads**

**BlobIdentifier Structure:**
```
[0x5b, 0x82]              # S5 blob prefix (2 bytes)
[0x1e]                    # BLAKE3 multihash code (1 byte)
[...32-byte-blake3-hash]  # Content hash (32 bytes)
[little-endian-size]      # File size (1-8 bytes, trimmed)
────────────────────────
Total: 36-43 bytes → 58-70 chars when base32 encoded with 'b' prefix
```

**Current vs Required Format:**
| Format | Contents | Bytes | Base32 Length | Portal |
|--------|----------|-------|---------------|--------|
| Raw Hash (current) | BLAKE3 hash only | 32 | 53 chars | REJECTED |
| **BlobIdentifier (required)** | Prefix + Hash + Size | 36+ | 58-70 chars | WORKS |

---

### Sub-phase 8.1: S5 Bridge - Construct BlobIdentifier

**Goal**: Update S5 bridge to return BlobIdentifier format CIDs with file size

**Status**: COMPLETE ✅

#### Tasks
- [x] Read S5.js BlobIdentifier implementation: `node_modules/@julesl23/s5js/dist/src/identifier/blob.js`
- [x] Import BlobIdentifier class in routes.js
- [x] Modify PUT handler to construct BlobIdentifier with:
  - Raw hash from `pathToCID()`
  - File size from `data.length`
  - Multihash prefix `0x1e` for BLAKE3
- [x] Return `blobId.toBase32()` instead of `formatCID(cidBytes, 'base32')`
- [ ] Test manually: `curl -X PUT http://localhost:5522/s5/fs/home/test.txt -d "hello"`
- [ ] Verify returned CID is 58-70 chars (not 53)

**Implementation File:** `/workspace/services/s5-bridge/src/routes.js`

**Current Code (lines 111-121):**
```javascript
const cidBytes = await advanced.pathToCID(path);
cid = formatCID(cidBytes, 'base32');
```

**New Code:**
```javascript
import { BlobIdentifier } from '@julesl23/s5js/dist/src/identifier/blob.js';

// In PUT handler after s5.fs.put():
const cidBytes = await advanced.pathToCID(path);  // 32-byte raw hash
const size = data.length;  // File size in bytes

// Construct BlobIdentifier (hash needs 0x1e multihash prefix)
const hashWithPrefix = new Uint8Array([0x1e, ...cidBytes]);
const blobId = new BlobIdentifier(hashWithPrefix, size);
cid = blobId.toBase32();  // Returns ~59 char CID
```

---

### Sub-phase 8.2: Rust CID Validation - Accept BlobIdentifier Format (TDD)

**Goal**: Update `is_valid_s5_cid()` to accept 58-70 char BlobIdentifier format

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_valid_blob_identifier_58_chars` (test_is_valid_s5_cid_blob_identifier_format)
- [x] Write test `test_valid_blob_identifier_70_chars` (included in above)
- [x] Write test `test_reject_old_53_char_raw_hash`
- [x] Write test `test_reject_ipfs_format_bafkrei` (test_is_valid_s5_cid)
- [x] Update `is_valid_s5_cid()` to accept 58-70 chars instead of exactly 53
- [x] Update `format_hash_as_cid()` - marked deprecated, kept for internal use
- [x] Run tests: `cargo test is_valid_s5_cid` - All 9 enhanced_s5_client tests pass

**Implementation File:** `/workspace/src/storage/enhanced_s5_client.rs`

**Current Code (lines 91-99):**
```rust
fn is_valid_s5_cid(s: &str) -> bool {
    if s.starts_with('b') && s.len() == 53 {
        return s[1..].chars().all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c));
    }
    false
}
```

**New Code:**
```rust
fn is_valid_s5_cid(s: &str) -> bool {
    // Must start with 'b' (base32 multibase prefix)
    if !s.starts_with('b') {
        return false;
    }
    // BlobIdentifier: 58-70 chars (varies by file size encoding)
    // Raw hash: 53 chars (DEPRECATED - portals reject this)
    let len = s.len();
    if len >= 58 && len <= 70 {
        return s[1..].chars().all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c));
    }
    false
}
```

---

### Sub-phase 8.3: MockS5Backend - Generate BlobIdentifier CIDs (TDD)

**Goal**: Update MockS5Backend to generate mock BlobIdentifier format CIDs for testing

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_mock_cid_is_blob_identifier_format` (test_mock_s5_generate_cid_returns_blob_identifier_format)
- [x] Write test `test_mock_cid_length_58_to_70` (included in above)
- [x] Write test `test_mock_cid_varies_with_data_size` (test_s5_cid_different_data)
- [x] Write test `test_mock_cid_deterministic_for_same_data` (test_s5_cid_deterministic)
- [x] Update `generate_cid(data: &[u8])` to:
  - Create blob prefix `[0x5b, 0x82]`
  - Add BLAKE3 multihash prefix `0x1e`
  - Add BLAKE3 hash of data (32 bytes)
  - Add little-endian file size (trimmed trailing zeros)
  - Base32 encode with 'b' prefix
- [ ] Run tests: `cargo test mock_s5_generate_cid`

**Implementation File:** `/workspace/src/storage/s5_client.rs`

**Current Code (lines 146-159):**
```rust
fn generate_cid(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    let hash_bytes = hash.as_bytes();
    let base32_encoded = BASE32_NOPAD.encode(hash_bytes).to_lowercase();
    format!("b{}", base32_encoded)
}
```

**New Code:**
```rust
/// Generate S5 BlobIdentifier format CID
/// Structure: prefix(2) + multihash(1) + hash(32) + size(1-8) = 36-43 bytes
/// Base32 encoded: 58-70 chars with 'b' multibase prefix
fn generate_cid(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    let hash_bytes = hash.as_bytes();
    let size = data.len() as u64;

    // Build BlobIdentifier bytes
    let mut blob_bytes = Vec::with_capacity(44);
    blob_bytes.extend_from_slice(&[0x5b, 0x82]);  // S5 blob prefix
    blob_bytes.push(0x1e);                         // BLAKE3 multihash code
    blob_bytes.extend_from_slice(hash_bytes);      // 32-byte hash

    // Little-endian size encoding (trim trailing zeros)
    let mut size_bytes = size.to_le_bytes().to_vec();
    while size_bytes.len() > 1 && size_bytes.last() == Some(&0) {
        size_bytes.pop();
    }
    blob_bytes.extend_from_slice(&size_bytes);

    // Base32 encode with 'b' multibase prefix
    let base32_encoded = BASE32_NOPAD.encode(&blob_bytes).to_lowercase();
    format!("b{}", base32_encoded)
}
```

---

### Sub-phase 8.4: Update Tests - Fix Hardcoded 53-char Assertions

**Goal**: Update all tests that check for 53-char CIDs to expect 58-70 chars

**Status**: COMPLETE ✅

#### Tasks
- [x] Search for all `cid.len() == 53` assertions
- [x] Update `/workspace/src/storage/s5_client.rs` tests - renamed to `is_valid_blob_identifier_cid`
- [x] Update `/workspace/src/storage/enhanced_s5_client.rs` tests - added new tests, updated passthrough
- [x] Update `/workspace/src/checkpoint/publisher.rs` tests (lines 894-918) - changed to 58-70 range
- [x] Replace `assert_eq!(cid.len(), 53)` with `assert!(cid.len() >= 58 && cid.len() <= 70)`
- [x] Update test CID literals - created proper 58-char test strings
- [x] Run all checkpoint tests: `cargo test --lib checkpoint` - 113 passed
- [x] Run all storage tests: `cargo test --lib s5_client` - 13 passed

**Files to Update:**
1. `/workspace/src/storage/s5_client.rs` - Test assertions
2. `/workspace/src/storage/enhanced_s5_client.rs` - Test assertions
3. `/workspace/src/checkpoint/publisher.rs` - Test assertions
4. `/workspace/src/checkpoint/index.rs` - Test CID literals

---

### Sub-phase 8.5: Build, Test, and Verify

**Goal**: Full test suite and release build

**Status**: COMPLETE ✅

#### Tasks
- [x] Run full storage tests: `cargo test --lib storage -- --nocapture` - 13 passed
- [x] Run full checkpoint tests: `cargo test --lib checkpoint -- --nocapture` - 113 passed
- [x] Run version tests: `cargo test --lib version` - 11 passed
- [x] Update VERSION file to `8.11.9-blobidentifier-cid`
- [x] Update `src/version.rs`:
  - VERSION to `v8.11.9-blobidentifier-cid-2026-01-12`
  - VERSION_NUMBER to `8.11.9`
  - VERSION_PATCH to `9`
  - Added BREAKING_CHANGES entries for v8.11.9
- [ ] Build release: `cargo build --release -j 4` (optional - user can do this)
- [ ] Verify CID length in binary logs (after deployment)
- [ ] Create tarball: `fabstir-llm-node-v8.11.9-blobidentifier-cid.tar.gz` (optional)

**Verification:**
```bash
# 1. Run S5 client tests
cargo test --lib s5_client -- --nocapture

# 2. Run checkpoint tests
cargo test --lib checkpoint -- --nocapture

# 3. Build release
cargo build --release -j 4

# 4. Manual test: Check CID length in production logs
# Should see ~59 char CIDs like: blo4qaaaaaae...
```

---

## Phase 9: Encrypted Checkpoint Deltas (Privacy Enhancement)

**Why This Phase Is Needed:**

Sessions use E2E encryption (SDK Phase 6.2), but checkpoint deltas were previously saved as **plaintext** to S5. This leaks conversation content to anyone who knows the CID.

**Spec Location**: `/workspace/docs/sdk-reference/NODE_CHECKPOINT_SPEC.md` (lines 944-1657)

```
Previous Flow (Privacy Leak):
────────────────────────────
1. User sends encrypted prompt → Host decrypts → LLM processes
2. Host generates response → Encrypts → Sends to user
3. Host saves checkpoint delta with PLAINTEXT messages to S5 ⚠️
4. Anyone with CID can read conversation content

Fixed Flow (Private):
─────────────────────
1. User provides recoveryPublicKey in session init (SDK v1.8.7+)
2. Host generates ephemeral keypair for forward secrecy
3. Host does ECDH + HKDF to derive encryption key
4. Host encrypts delta with XChaCha20-Poly1305
5. Host signs ciphertext with EIP-191
6. Host uploads encrypted delta to S5
7. Only user with matching private key can decrypt during recovery
```

**Encrypted Delta Format:**
```json
{
  "encrypted": true,
  "version": 1,
  "userRecoveryPubKey": "0x02abc123def456789...",
  "ephemeralPublicKey": "0x03xyz789abc456def...",
  "nonce": "f47ac10b58cc4372a5670e02b2c3d4e5f67890abcdef1234",
  "ciphertext": "a1b2c3d4e5f6...",
  "hostSignature": "0x1234...abcd"
}
```

**Backward Compatibility:**
| Scenario | Behavior |
|----------|----------|
| `recoveryPublicKey` present | Encrypt deltas with user's key |
| `recoveryPublicKey` absent | Plaintext deltas (legacy SDKs) |
| Encryption fails | **DO NOT** fall back to plaintext - block proof submission |

**Existing Reusable Infrastructure:**
| Component | File | Function | Reusability |
|-----------|------|----------|-------------|
| ECDH | `src/crypto/ecdh.rs` | `derive_shared_key()` | Pattern - need custom info param |
| XChaCha20-Poly1305 | `src/crypto/encryption.rs` | `encrypt_with_aead()` | Direct |
| EIP-191 Signing | `src/checkpoint/signer.rs` | `sign_checkpoint_data()` | Direct |
| Session Keys | `src/crypto/session_keys.rs` | `SessionKeyStore` | Pattern |

---

### Sub-phase 9.1: Extract recoveryPublicKey from Session Init

**Goal**: Add recoveryPublicKey field to session init payload parsing and storage

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_session_init_message_with_recovery_public_key`
- [x] Write test `test_session_init_message_without_recovery_public_key_is_none`
- [x] Write test `test_session_init_message_recovery_key_not_serialized_when_none`
- [x] Add `recovery_public_key: Option<String>` to `SessionInitMessage` struct
- [x] Add `#[serde(skip_serializing_if = "Option::is_none")]` attribute
- [x] Update `from_legacy()` to set `recovery_public_key: None`
- [x] Run tests: `cargo test --lib "test_session_init_message" -- --nocapture` (3 passed)

**Implementation File:** `/workspace/src/api/websocket/messages.rs`

**Current `SessionInitMessage` struct (line ~100):**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionInitMessage {
    pub job_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    pub user_address: String,
    pub host_address: String,
    pub model_id: String,
    pub timestamp: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_database: Option<VectorDatabaseInfo>,
}
```

**Add field:**
```rust
    /// User's recovery public key for checkpoint encryption (SDK v1.8.7+)
    /// Compressed secp256k1 public key (33 bytes = 66 hex chars + 0x prefix)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_public_key: Option<String>,
```

---

### Sub-phase 9.2: Add recoveryPublicKey to SessionContext

**Goal**: Store recoveryPublicKey in session context for use during checkpoint publishing

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_session_context_stores_recovery_public_key`
- [x] Write test `test_session_context_recovery_key_is_optional`
- [x] Write test `test_session_context_new_with_recovery_key_none`
- [x] Add `recovery_public_key: Option<String>` to `SessionContext` struct
- [x] Add `new_with_recovery_key()` method to accept optional recovery_public_key
- [x] Update `new()` to delegate to `new_with_recovery_key()` with None
- [x] Run tests: `cargo test --lib "test_session_context" -- --nocapture` (4 passed)

**Implementation File:** `/workspace/src/api/websocket/session_context.rs`

**Current `SessionContext` struct (line ~15):**
```rust
#[derive(Debug, Clone)]
pub struct SessionContext {
    pub session_id: String,
    pub job_id: u64,
    pub chain_id: u64,
    pub chain_info: ChainInfo,
    pub is_active: bool,
    pub created_at: u64,
}
```

**Add field:**
```rust
    /// User's recovery public key for encrypted checkpoint deltas
    pub recovery_public_key: Option<String>,
```

---

### Sub-phase 9.3: Update Session Init Handler

**Goal**: Extract recoveryPublicKey from incoming payload and pass through to response

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_session_init_with_recovery_public_key`
- [x] Write test `test_session_init_without_recovery_key_is_backwards_compatible`
- [x] Add `recovery_public_key: Option<String>` to `SessionInitResponse` struct
- [x] Add `handle_session_init_with_recovery_key()` method
- [x] Update `handle_session_init_with_chain()` to delegate with None
- [x] Run tests: `cargo test --lib "test_session_init" -- --nocapture` (6 passed)

**Implementation File:** `/workspace/src/api/websocket/handlers/session_init.rs`

**Location:** Line ~45-80 in `handle_session_init_with_chain()`

---

### Sub-phase 9.4: Create EncryptedCheckpointDelta Struct

**Goal**: Define the encrypted delta format matching the spec

**Status**: COMPLETE ✅

#### Tasks
- [x] Write test `test_encrypted_checkpoint_delta_serialization_camel_case`
- [x] Write test `test_encrypted_checkpoint_delta_all_fields_present`
- [x] Write test `test_encrypted_checkpoint_delta_to_json_bytes`
- [x] Write test `test_encrypted_checkpoint_delta_deserialization`
- [x] Write test `test_encrypted_checkpoint_delta_validation_pass`
- [x] Write test `test_encrypted_checkpoint_delta_validation_bad_encrypted_flag`
- [x] Write test `test_encrypted_checkpoint_delta_validation_bad_nonce_length`
- [x] Write test `test_encrypted_checkpoint_delta_validation_empty_ciphertext`
- [x] Create `src/checkpoint/encryption.rs` file
- [x] Add `pub mod encryption;` to `src/checkpoint/mod.rs`
- [x] Implement `EncryptedCheckpointDelta` struct with serde `rename_all = "camelCase"`
- [x] Implement `new()`, `to_json_bytes()`, and `validate()` methods
- [x] Run tests: `cargo test --lib checkpoint::encryption -- --nocapture` (8 passed)

**Implementation File:** `/workspace/src/checkpoint/encryption.rs` (NEW)

**Struct definition:**
```rust
use serde::{Deserialize, Serialize};

/// Encrypted checkpoint delta for SDK recovery
/// Only the user with the matching private key can decrypt
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedCheckpointDelta {
    /// Always true for encrypted deltas
    pub encrypted: bool,

    /// Encryption version (currently 1)
    pub version: u8,

    /// User's recovery public key (echoed back for verification)
    pub user_recovery_pub_key: String,

    /// Host's ephemeral public key for ECDH (compressed, 33 bytes)
    pub ephemeral_public_key: String,

    /// 24-byte random nonce for XChaCha20 (hex, 48 chars)
    pub nonce: String,

    /// Encrypted CheckpointDelta JSON (hex)
    pub ciphertext: String,

    /// EIP-191 signature over keccak256(ciphertext)
    pub host_signature: String,
}
```

---

### Sub-phase 9.5: Implement ECDH with Custom HKDF Info Parameter

**Goal**: Create checkpoint-specific ECDH key derivation with domain separation

**Status**: COMPLETE ✅ (2026-01-13)

#### Tasks
- [x] Write test `test_derive_checkpoint_key_returns_32_bytes`
- [x] Write test `test_derive_checkpoint_key_is_deterministic`
- [x] Write test `test_derive_checkpoint_key_different_inputs_different_outputs`
- [x] Write test `test_derive_checkpoint_key_rejects_invalid_private_key_size`
- [x] Write test `test_derive_checkpoint_key_rejects_invalid_public_key_size`
- [x] Write test `test_derive_checkpoint_key_rejects_invalid_public_key_point`
- [x] Write test `test_derive_checkpoint_key_uses_checkpoint_info_param`
- [x] Implement `derive_checkpoint_encryption_key()` function
- [x] Use HKDF info parameter: `b"checkpoint-delta-encryption-v1"`
- [x] Run tests: `cargo test --lib checkpoint::encryption -- --nocapture` (7 tests passing)

**Implementation File:** `/workspace/src/checkpoint/encryption.rs`

**Function signature:**
```rust
use anyhow::Result;
use hkdf::Hkdf;
use k256::{ecdh::diffie_hellman, PublicKey, SecretKey};
use sha2::Sha256;

/// HKDF info parameter for checkpoint encryption domain separation
const CHECKPOINT_HKDF_INFO: &[u8] = b"checkpoint-delta-encryption-v1";

/// Derive encryption key for checkpoint delta using ECDH + HKDF
///
/// # Arguments
/// * `ephemeral_private` - Host's ephemeral private key (32 bytes)
/// * `user_recovery_pubkey` - User's recovery public key (33 bytes compressed)
///
/// # Returns
/// 32-byte encryption key for XChaCha20-Poly1305
pub fn derive_checkpoint_encryption_key(
    ephemeral_private: &[u8; 32],
    user_recovery_pubkey: &[u8],
) -> Result<[u8; 32]> {
    // 1. Parse keys
    let secret_key = SecretKey::from_slice(ephemeral_private)?;
    let public_key = PublicKey::from_sec1_bytes(user_recovery_pubkey)?;

    // 2. ECDH: shared_point = user_pubkey * ephemeral_private
    let shared_secret = diffie_hellman(
        secret_key.to_nonzero_scalar(),
        public_key.as_affine(),
    );

    // 3. HKDF with checkpoint-specific info parameter
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret.raw_secret_bytes());
    let mut encryption_key = [0u8; 32];
    hkdf.expand(CHECKPOINT_HKDF_INFO, &mut encryption_key)?;

    Ok(encryption_key)
}
```

---

### Sub-phase 9.6: Implement encrypt_checkpoint_delta Function

**Goal**: Full encryption flow - ephemeral key, ECDH, XChaCha20, signature

**Status**: COMPLETE ✅ (2026-01-13)

#### Tasks
- [x] Write test `test_encrypt_checkpoint_delta_returns_encrypted_delta`
- [x] Write test `test_encrypt_checkpoint_delta_has_correct_field_formats`
- [x] Write test `test_encrypt_checkpoint_delta_validates_structure`
- [x] Write test `test_encrypt_checkpoint_delta_different_calls_different_ciphertext`
- [x] Write test `test_encrypt_checkpoint_delta_rejects_invalid_pubkey`
- [x] Write test `test_encrypt_checkpoint_delta_rejects_invalid_pubkey_point`
- [x] Write test `test_encrypt_checkpoint_delta_ciphertext_decryptable`
- [x] Implement `encrypt_checkpoint_delta()` function
- [x] Run tests: `cargo test --lib checkpoint::encryption -- --nocapture` (22 tests passing)

**Implementation File:** `/workspace/src/checkpoint/encryption.rs`

**Function signature:**
```rust
use crate::checkpoint::CheckpointDelta;
use crate::crypto::encryption::encrypt_with_aead;
use k256::ecdsa::SigningKey;
use rand::rngs::OsRng;

/// Encrypt a checkpoint delta for the user
///
/// # Arguments
/// * `delta` - Plaintext checkpoint delta (already signed)
/// * `user_recovery_pubkey` - User's recovery public key (0x-prefixed hex)
/// * `host_private_key` - Host's private key for signing ciphertext
///
/// # Returns
/// EncryptedCheckpointDelta ready for S5 upload
///
/// # Security
/// - Generates fresh ephemeral keypair (forward secrecy)
/// - Uses random 24-byte nonce (unique per encryption)
/// - Signs ciphertext with host key (authenticity)
pub fn encrypt_checkpoint_delta(
    delta: &CheckpointDelta,
    user_recovery_pubkey: &str,
    host_private_key: &[u8; 32],
) -> Result<EncryptedCheckpointDelta> {
    // 1. Generate ephemeral keypair
    let ephemeral_secret = SigningKey::random(&mut OsRng);
    let ephemeral_public = ephemeral_secret.verifying_key();

    // 2. Parse user's recovery public key
    let user_pubkey_bytes = hex::decode(user_recovery_pubkey.trim_start_matches("0x"))?;

    // 3. Derive encryption key via ECDH + HKDF
    let encryption_key = derive_checkpoint_encryption_key(
        &ephemeral_secret.to_bytes().into(),
        &user_pubkey_bytes,
    )?;

    // 4. Serialize delta to JSON (sorted keys for determinism)
    let plaintext = delta.to_json_bytes();

    // 5. Generate random 24-byte nonce
    let nonce: [u8; 24] = rand::random();

    // 6. Encrypt with XChaCha20-Poly1305
    let ciphertext = encrypt_with_aead(&plaintext, &nonce, &[], &encryption_key)?;

    // 7. Sign keccak256(ciphertext) with host key
    let ciphertext_hash = keccak256(&ciphertext);
    let signature = sign_checkpoint_data(host_private_key, &hex::encode(ciphertext_hash))?;

    // 8. Build encrypted delta
    Ok(EncryptedCheckpointDelta {
        encrypted: true,
        version: 1,
        user_recovery_pub_key: user_recovery_pubkey.to_string(),
        ephemeral_public_key: format!("0x{}", hex::encode(ephemeral_public.to_sec1_bytes())),
        nonce: hex::encode(nonce),
        ciphertext: hex::encode(ciphertext),
        host_signature: signature,
    })
}
```

---

### Sub-phase 9.7: Add encrypted Marker to CheckpointEntry

**Goal**: Add `encrypted` field to checkpoint index entries

**Status**: COMPLETE ✅ (2026-01-13)

#### Tasks
- [x] Write test `test_checkpoint_entry_encrypted_marker_when_true`
- [x] Write test `test_checkpoint_entry_no_encrypted_marker_when_plaintext`
- [x] Write test `test_checkpoint_entry_with_timestamp_encrypted`
- [x] Write test `test_checkpoint_entry_encrypted_deserialization`
- [x] Write test `test_checkpoint_entry_plaintext_deserialization_backward_compat`
- [x] Write test `test_checkpoint_entry_encrypted_serialization_camel_case`
- [x] Add `encrypted: Option<bool>` to `CheckpointEntry` struct
- [x] Add `#[serde(skip_serializing_if = "Option::is_none")]` attribute
- [x] Add `CheckpointEntry::new_encrypted()` constructor
- [x] Add `CheckpointEntry::with_timestamp_encrypted()` constructor
- [x] Add `CheckpointEntry::is_encrypted()` method
- [x] Run tests: `cargo test --lib checkpoint::index -- --nocapture` (17 tests passing)

**Implementation File:** `/workspace/src/checkpoint/index.rs`

**Current `CheckpointEntry` struct:**
```rust
pub struct CheckpointEntry {
    pub index: u32,
    pub proof_hash: String,
    pub delta_cid: String,
    pub token_range: [u64; 2],
    pub timestamp: u64,
}
```

**Add field:**
```rust
    /// True if delta is encrypted (SDK v1.8.7+)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
```

---

### Sub-phase 9.8: Add recoveryPublicKey to SessionCheckpointState

**Goal**: Store recovery public key in checkpoint publisher's session state

**Status**: COMPLETE ✅ (2026-01-13)

#### Tasks
- [x] Write test `test_session_state_recovery_key_default_none`
- [x] Write test `test_session_state_set_recovery_key`
- [x] Write test `test_session_state_get_recovery_key`
- [x] Write test `test_session_state_has_recovery_key`
- [x] Write test `test_session_state_from_index_preserves_recovery_key`
- [x] Write test `test_publisher_set_recovery_key`
- [x] Write test `test_publisher_get_recovery_key`
- [x] Write test `test_publisher_has_recovery_key`
- [x] Add `recovery_public_key: Option<String>` to `SessionCheckpointState` struct
- [x] Add `set_recovery_public_key()` method to `SessionCheckpointState`
- [x] Add `get_recovery_public_key()` method to `SessionCheckpointState`
- [x] Add `has_recovery_key()` method to `SessionCheckpointState`
- [x] Add `set_recovery_public_key()` method to `CheckpointPublisher`
- [x] Add `get_recovery_public_key()` method to `CheckpointPublisher`
- [x] Add `has_recovery_key()` method to `CheckpointPublisher`
- [x] Run tests: `cargo test --lib checkpoint::publisher -- --nocapture` (45 tests passing)

**Implementation File:** `/workspace/src/checkpoint/publisher.rs`

**Current `SessionCheckpointState` struct:**
```rust
pub struct SessionCheckpointState {
    pub checkpoint_index: u32,
    pub message_buffer: Vec<CheckpointMessage>,
    pub last_checkpoint_tokens: u64,
    pub index: Option<CheckpointIndex>,
    pub streaming_response: Option<String>,
}
```

**Add field:**
```rust
    /// User's recovery public key for encrypted deltas (SDK v1.8.7+)
    pub recovery_public_key: Option<String>,
```

**Add to CheckpointPublisher:**
```rust
/// Set the recovery public key for a session
pub async fn set_recovery_public_key(&self, session_id: &str, pubkey: String) {
    let mut sessions = self.sessions.write().await;
    let state = sessions
        .entry(session_id.to_string())
        .or_insert_with(SessionCheckpointState::new);
    state.recovery_public_key = Some(pubkey);
}
```

---

### Sub-phase 9.9: Integrate Encryption into publish_checkpoint

**Goal**: Conditionally encrypt deltas when recovery public key is present

**Status**: PENDING

#### Tasks
- [ ] Write test `test_publish_checkpoint_encrypts_when_recovery_key_present`
- [ ] Write test `test_publish_checkpoint_plaintext_when_no_recovery_key`
- [ ] Write test `test_publish_checkpoint_sets_encrypted_marker_in_index`
- [ ] Write test `test_publish_checkpoint_blocks_on_encryption_failure`
- [ ] Modify `publish_checkpoint()` to check for recovery_public_key
- [ ] Call `encrypt_checkpoint_delta()` when key is present
- [ ] Upload encrypted delta instead of plaintext
- [ ] Set `encrypted: true` in CheckpointEntry
- [ ] Run tests: `cargo test --lib checkpoint::publisher -- --nocapture`

**Implementation File:** `/workspace/src/checkpoint/publisher.rs`

**Location:** In `publish_checkpoint()` method, after delta creation and before upload

**Code to add:**
```rust
// Check if encryption is enabled for this session
let delta_bytes = if let Some(recovery_pubkey) = &state.recovery_public_key {
    // Encrypt the delta
    let encrypted_delta = crate::checkpoint::encryption::encrypt_checkpoint_delta(
        &delta,
        recovery_pubkey,
        private_key,
    ).map_err(|e| {
        error!("Checkpoint encryption failed - NOT uploading: {}", e);
        anyhow!("Checkpoint encryption failed: {}", e)
    })?;

    info!("Encrypting checkpoint {} for session {}", checkpoint_index, session_id);
    serde_json::to_vec_pretty(&encrypted_delta)?
} else {
    // Legacy plaintext mode
    delta.to_json_bytes()
};

// ... upload delta_bytes ...

// Set encrypted marker in index entry
let encrypted_marker = state.recovery_public_key.is_some();
let entry = CheckpointEntry::new_with_encryption(
    checkpoint_index,
    proof_hash_hex,
    delta_cid.clone(),
    start_token,
    end_token,
    encrypted_marker,
);
```

---

### Sub-phase 9.10: Wire Up Session Init to Checkpoint Publisher

**Goal**: Connect session init handler to set recovery public key in publisher

**Status**: PENDING

#### Tasks
- [ ] Write integration test `test_session_init_sets_recovery_key_in_publisher`
- [ ] Write integration test `test_full_encrypted_checkpoint_flow`
- [ ] Update session init handler to call `checkpoint_publisher.set_recovery_public_key()`
- [ ] Ensure recovery key is set before any checkpoints are published
- [ ] Run tests: `cargo test --lib -- --nocapture`

**Implementation Files:**
- `/workspace/src/api/websocket/handlers/session_init.rs`
- `/workspace/src/api/server.rs` (if HTTP session init exists)

**Code to add in session init handler:**
```rust
// Set recovery public key in checkpoint publisher (if provided)
if let Some(recovery_pubkey) = &session_init.recovery_public_key {
    if let Some(checkpoint_publisher) = &checkpoint_manager.checkpoint_publisher {
        checkpoint_publisher
            .set_recovery_public_key(&session_id, recovery_pubkey.clone())
            .await;
        info!("Recovery public key set for session {} (encrypted checkpoints enabled)", session_id);
    }
}
```

---

### Sub-phase 9.11: Update Version and Documentation

**Goal**: Bump version and update tracking document

**Status**: PENDING

#### Tasks
- [ ] Update `VERSION` file to `8.12.0-encrypted-checkpoint-deltas`
- [ ] Update `src/version.rs`:
  - [ ] VERSION to `v8.12.0-encrypted-checkpoint-deltas-2026-01-XX`
  - [ ] VERSION_NUMBER to `8.12.0`
  - [ ] VERSION_MINOR to 12, VERSION_PATCH to 0
  - [ ] Add feature: `encrypted-checkpoint-deltas`
  - [ ] Add BREAKING_CHANGES entry for v8.12.0
- [ ] Update test assertions in version.rs
- [ ] Run tests: `cargo test --lib version -- --nocapture`
- [ ] Mark all Phase 9 tasks as complete in this document

**Implementation Files:**
- `/workspace/VERSION`
- `/workspace/src/version.rs`

---

### Sub-phase 9.12: Build, Test, and Verify

**Goal**: Full test suite and release build

**Status**: PENDING

#### Tasks
- [ ] Run full checkpoint tests: `cargo test --lib checkpoint -- --nocapture`
- [ ] Run full crypto tests: `cargo test --lib crypto -- --nocapture`
- [ ] Run WebSocket tests: `cargo test --lib websocket -- --nocapture`
- [ ] Run full test suite: `cargo test --lib -- --nocapture`
- [ ] Build release: `cargo build --release -j 4`
- [ ] Verify encrypted checkpoint flow in logs
- [ ] Create tarball (optional): `fabstir-llm-node-v8.12.0-encrypted-checkpoint-deltas.tar.gz`

**Verification:**
```bash
# 1. Run encryption module tests
cargo test --lib checkpoint::encryption -- --nocapture

# 2. Run full checkpoint tests
cargo test --lib checkpoint -- --nocapture

# 3. Run all tests
cargo test --lib -- --nocapture

# 4. Build release
cargo build --release -j 4

# 5. Check for encrypted checkpoint logs
# Should see: "Encrypting checkpoint X for session Y"
# Should see: "encrypted: true" in checkpoint index
```

**Expected Log Patterns:**
- `🔐 Recovery public key set for session X (encrypted checkpoints enabled)`
- `🔐 Encrypting checkpoint 0 for session X`
- `✅ Encrypted checkpoint published: CID`

---

### Phase 9 Security Properties

| Property | How Achieved |
|----------|--------------|
| **Confidentiality** | XChaCha20-Poly1305 with ECDH-derived key |
| **Forward Secrecy** | Ephemeral keypair per checkpoint |
| **Authenticity** | Poly1305 MAC + host signature over ciphertext |
| **Integrity** | AEAD (Authenticated Encryption with Associated Data) |
| **User-Only Access** | Only user has private key for recoveryPublicKey |
| **No Plaintext Fallback** | Encryption failure blocks proof submission |

---

### Phase 9 Error Handling

If encryption fails:
1. Log the error with details
2. **DO NOT** fall back to plaintext (security violation)
3. **DO NOT** submit proof to chain (checkpoint not recoverable)
4. Return error to caller to block proof submission

```rust
// WRONG - Security violation!
match encrypt_checkpoint_delta(...) {
    Ok(encrypted) => upload(encrypted),
    Err(_) => upload(plaintext_delta),  // ❌ NEVER DO THIS
}

// CORRECT - Block on failure
let encrypted_delta = encrypt_checkpoint_delta(...)?;  // ✅ Propagate error
upload(encrypted_delta)
```

---

### Phase 9 File Size Constraints

| File | Max Lines |
|------|-----------|
| `src/checkpoint/encryption.rs` | 300 |
| `src/api/websocket/messages.rs` | +20 lines |
| `src/api/websocket/session_context.rs` | +10 lines |
| `src/api/websocket/handlers/session_init.rs` | +30 lines |
| `src/checkpoint/index.rs` | +15 lines |
| `src/checkpoint/publisher.rs` | +50 lines |

---

## Summary

| Phase | Sub-phase | Description | Status |
|-------|-----------|-------------|--------|
| 1 | 1.1 | Create module structure | COMPLETE ✅ |
| 1 | 1.2 | Implement CheckpointDelta (TDD) | COMPLETE ✅ |
| 1 | 1.3 | Implement CheckpointIndex (TDD) | COMPLETE ✅ |
| 1 | 1.4 | Implement Checkpoint Signer (TDD) | COMPLETE ✅ |
| 2 | 2.1 | Session state management | COMPLETE ✅ |
| 2 | 2.2 | S5 upload with retry | COMPLETE ✅ |
| 2 | 2.3 | Publish checkpoint core logic | COMPLETE ✅ |
| 2 | 2.4 | Session resumption from S5 | COMPLETE ✅ |
| 3 | 3.1 | Add CheckpointPublisher to CheckpointManager | COMPLETE ✅ |
| 3 | 3.2 | Integrate into submit_checkpoint_async | COMPLETE ✅ |
| 3 | 3.3 | Add session ID to token tracking | COMPLETE ✅ |
| 4 | 4.1 | Track user messages | COMPLETE ✅ |
| 4 | 4.2 | Track assistant responses | COMPLETE ✅ |
| 4 | 4.3 | Handle streaming partial responses | COMPLETE ✅ |
| 5 | 5.1 | Implement cleanup methods | COMPLETE ✅ |
| 6 | 6.1 | Integration tests | COMPLETE ✅ |
| 6 | 6.2 | Update version | COMPLETE ✅ |
| 6 | 6.3 | Documentation | COMPLETE ✅ |
| 6 | 6.4 | Create release tarball | COMPLETE ✅ |
| 7 | 7.1 | Add CheckpointManager accessor methods | COMPLETE ✅ |
| 7 | 7.2 | Implement checkpoint handler (TDD) | COMPLETE ✅ |
| 7 | 7.3 | Add route and integration | COMPLETE ✅ |
| 7 | 7.4 | Update documentation and version | COMPLETE ✅ |
| 8 | 8.1 | S5 Bridge - Construct BlobIdentifier | COMPLETE ✅ |
| 8 | 8.2 | Rust CID Validation - Accept BlobIdentifier | COMPLETE ✅ |
| 8 | 8.3 | MockS5Backend - Generate BlobIdentifier CIDs | COMPLETE ✅ |
| 8 | 8.4 | Update Tests - Fix 53-char Assertions | COMPLETE ✅ |
| 8 | 8.5 | Build, Test, and Verify | COMPLETE ✅ |
| 9 | 9.1 | Extract recoveryPublicKey from Session Init | COMPLETE ✅ |
| 9 | 9.2 | Add recoveryPublicKey to SessionContext | COMPLETE ✅ |
| 9 | 9.3 | Update Session Init Handler | COMPLETE ✅ |
| 9 | 9.4 | Create EncryptedCheckpointDelta Struct | COMPLETE ✅ |
| 9 | 9.5 | Implement ECDH with Custom HKDF Info | PENDING |
| 9 | 9.6 | Implement encrypt_checkpoint_delta Function | PENDING |
| 9 | 9.7 | Add encrypted Marker to CheckpointEntry | PENDING |
| 9 | 9.8 | Add recoveryPublicKey to SessionCheckpointState | PENDING |
| 9 | 9.9 | Integrate Encryption into publish_checkpoint | PENDING |
| 9 | 9.10 | Wire Up Session Init to Checkpoint Publisher | PENDING |
| 9 | 9.11 | Update Version and Documentation | PENDING |
| 9 | 9.12 | Build, Test, and Verify | PENDING |

**Total: 40 sub-phases (32 complete, 8 pending) - PHASE 9 IN PROGRESS**

**E2E Verification by SDK Developer (2026-01-12):**
```
Flow verified:
1. SDK calls node HTTP API: GET /v1/checkpoints/{sessionId}
2. Node returns checkpoint index with delta CIDs
3. SDK fetches deltas from S5 using downloadByCID() with P2P discovery
4. BLAKE3 hash verification passes
5. CBOR-decoded deltas merged into recovered conversation
```

---

## Environment Variables

```bash
# Existing (no changes)
HOST_PRIVATE_KEY=0x...          # Required for signing
ENHANCED_S5_URL=http://localhost:5522  # S5 bridge URL
CONTRACT_JOB_MARKETPLACE=0x...  # JobMarketplace contract
```

---

## File Size Constraints

| File | Max Lines |
|------|-----------|
| `src/checkpoint/mod.rs` | 50 |
| `src/checkpoint/delta.rs` | 200 |
| `src/checkpoint/index.rs` | 250 |
| `src/checkpoint/signer.rs` | 200 |
| `src/checkpoint/publisher.rs` | 500 |
| `src/checkpoint/cleanup.rs` | 150 |
| `tests/checkpoint_tests.rs` | 400 |

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| S5 upload fails | 3 retries with exponential backoff; block proof if still fails |
| Message buffer too large | Limit messages per checkpoint; compress if needed |
| Session state memory | HashMap with cleanup on session end |
| Signature verification | Follow existing proof_signer.rs pattern (tested) |
| Orphaned deltas on index failure | Log orphan CID for manual cleanup |

---

## Critical Success Criteria

1. **Checkpoint published BEFORE proof** - Verify in logs
2. **S5 failure blocks proof** - No orphaned proofs on-chain
3. **Signatures valid** - SDK can verify with host address
4. **Index path correct** - `home/checkpoints/{host}/{session}/index.json`
5. **Delta format matches spec** - camelCase, all required fields
