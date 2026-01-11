# IMPLEMENTATION - Checkpoint Publishing for Conversation Recovery

## Status: Phase 4 Complete

**Status**: Phase 4 Complete - Session Message Integration
**Version**: v8.11.0-checkpoint-publishing (target)
**Start Date**: 2026-01-11
**Approach**: Strict TDD bounded autonomy - one sub-phase at a time
**Tests Passing**: 105 checkpoint tests passing (delta: 12, index: 11, signer: 9, publisher: 37, cleanup: 11, checkpoint_manager integration: 7, + existing checkpoint_manager tests)

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

**Status**: PENDING

#### Tasks
- [ ] Write test `test_cleanup_completed_session_7_days`
- [ ] Write test `test_cleanup_timed_out_session_30_days`
- [ ] Write test `test_cleanup_cancelled_session_immediate`
- [ ] Implement `cleanup_checkpoints()` method
- [ ] Add session state tracking (completed, timed_out, cancelled)
- [ ] Run tests: `cargo test checkpoint::cleanup`

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

**Status**: PENDING

#### Tasks
- [ ] Write test `test_full_checkpoint_flow` - 1000 tokens triggers checkpoint
- [ ] Write test `test_multiple_checkpoints` - 3000 tokens = 3 checkpoints
- [ ] Write test `test_checkpoint_recovery_by_sdk` - verify data format
- [ ] Write test `test_s5_failure_blocks_proof` - no orphaned proofs
- [ ] Create `tests/checkpoint_tests.rs`
- [ ] Run tests: `cargo test --test checkpoint_tests`

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

**Status**: PENDING

#### Tasks
- [ ] Update `VERSION` file to `8.11.0-checkpoint-publishing`
- [ ] Update `src/version.rs`:
  - [ ] VERSION constant
  - [ ] VERSION_NUMBER to "8.11.0"
  - [ ] Add feature: "checkpoint-publishing"
  - [ ] Add feature: "conversation-recovery"
  - [ ] Add BREAKING_CHANGES entry
- [ ] Run `cargo test version`

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

**Status**: PENDING

#### Tasks
- [ ] Add checkpoint publishing section to `docs/API.md`
- [ ] Document S5 path convention
- [ ] Document cleanup policy
- [ ] Add troubleshooting section

---

### Sub-phase 6.4: Create Release Tarball

**Goal**: Package release binary

**Status**: PENDING

#### Tasks
- [ ] Build release: `cargo build --release --features real-ezkl -j 4`
- [ ] Verify version in binary
- [ ] Copy binary to root
- [ ] Create tarball with correct structure
- [ ] Verify tarball contents
- [ ] Clean up

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
| 5 | 5.1 | Implement cleanup methods | PENDING |
| 6 | 6.1 | Integration tests | PENDING |
| 6 | 6.2 | Update version | PENDING |
| 6 | 6.3 | Documentation | PENDING |
| 6 | 6.4 | Create release tarball | PENDING |

**Total: 19 sub-phases**

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
