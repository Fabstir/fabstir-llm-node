# End-to-End Encryption Implementation Plan (Phase 6.2)

## Overview

This implementation plan adds end-to-end encryption support to the Fabstir LLM Node following strict TDD (Test-Driven Development) with bounded autonomy. The SDK has already implemented client-side encryption (Phase 6.2), and now the node must implement decryption support.

## Core Requirements

- **Encryption Protocol**: ECDH (secp256k1) + XChaCha20-Poly1305 AEAD
- **Key Exchange**: Ephemeral-static ECDH for session initialization
- **Session Encryption**: Symmetric XChaCha20-Poly1305 for messages
- **Client Authentication**: ECDSA signature recovery for client address
- **Backward Compatible**: Support both encrypted and plaintext sessions
- **Security**: Store session keys in memory only, never persist

## Cryptographic Primitives

- **Key Exchange**: ECDH on secp256k1 (same curve as Ethereum)
- **Symmetric Cipher**: XChaCha20-Poly1305 (authenticated encryption)
- **Key Derivation**: HKDF-SHA256
- **Signatures**: ECDSA secp256k1 (Ethereum-compatible)
- **Libraries**: k256, chacha20poly1305, hkdf, sha2

## Phase 1: Cryptography Foundation

### Sub-phase 1.1: Dependencies and Module Structure ✅
**Goal**: Add cryptographic dependencies and create module structure

**Tasks**:
- [x] Add `k256` dependency with features ["ecdh", "ecdsa"]
- [x] Add `chacha20poly1305` dependency
- [x] Add `hkdf` dependency (sha2 already exists)
- [x] Create `src/crypto/mod.rs` module declaration
- [x] Create module structure (ecdh, encryption, session_keys, signature)
- [x] Add crypto module to main lib.rs

**Deliverables**:
- Updated `Cargo.toml` with crypto dependencies ✅
- `src/crypto/mod.rs` with module exports ✅
- Module files created (ecdh, encryption, session_keys, signature) ✅

**Success Criteria**:
- Cargo build succeeds ✅
- Crypto modules accessible ✅
- Dependencies resolve correctly ✅

### Sub-phase 1.2: ECDH Key Exchange Implementation ✅
**Goal**: Implement ECDH key derivation using k256

**Tasks**:
- [x] Create `src/crypto/ecdh.rs` module
- [x] Implement `derive_shared_key()` function
- [x] Parse compressed secp256k1 public key (33 bytes)
- [x] Perform ECDH multiplication using k256::ecdh::diffie_hellman
- [x] Extract shared secret from ECDH result
- [x] Apply HKDF-SHA256 to derive 32-byte encryption key
- [x] Add error handling for invalid keys
- [x] Support both compressed (33 bytes) and uncompressed (65 bytes) public keys

**Test Files**:
- `tests/crypto/test_ecdh.rs` - Comprehensive TDD tests (11 test cases) ✅
- `tests/crypto_simple.rs` - Basic integration tests ✅
  - test_ecdh_basic() ✅
  - test_ecdh_deterministic() ✅

**Success Criteria**:
- All tests pass ✅
- Keys derived correctly with HKDF-SHA256 ✅
- Invalid keys rejected with clear error messages ✅
- Supports both compressed and uncompressed public keys ✅

### Sub-phase 1.3: XChaCha20-Poly1305 Encryption ✅
**Goal**: Implement symmetric encryption/decryption

**Tasks**:
- [x] Create `src/crypto/encryption.rs` module
- [x] Implement `decrypt_with_aead()` function using chacha20poly1305
- [x] Implement `encrypt_with_aead()` function using chacha20poly1305
- [x] Support 24-byte nonces (XChaCha20)
- [x] Validate AAD (additional authenticated data)
- [x] Handle authentication tag verification (automatic with Poly1305)
- [x] Add error handling for decryption failures
- [x] Validate nonce and key sizes (24 bytes and 32 bytes respectively)

**Test Files**:
- `tests/crypto/test_encryption.rs` - Comprehensive TDD tests (14 test cases) ✅
  - test_encrypt_decrypt_roundtrip() ✅
  - test_decryption_with_aad() ✅
  - test_invalid_nonce_size() ✅
  - test_authentication_failure() ✅
  - test_tampered_ciphertext() ✅
  - test_wrong_key_decryption() ✅
  - +8 additional edge case tests ✅
- `tests/crypto_simple.rs` - Integration tests ✅
  - test_encryption_basic() ✅
  - test_encryption_wrong_key() ✅
- Unit tests in `src/crypto/encryption.rs` (2 passing) ✅

**Success Criteria**:
- Encryption/decryption roundtrip works ✅
- AAD properly authenticated ✅
- Tampered messages rejected ✅
- 16-byte authentication tag appended to ciphertext ✅

## Phase 2: Signature Verification

### Sub-phase 2.1: ECDSA Signature Recovery
**Goal**: Recover Ethereum address from ECDSA signature

**Tasks**:
- [ ] Create `src/crypto/signature.rs` module
- [ ] Implement `recover_public_key()` from signature
- [ ] Parse 65-byte compact signature (r + s + v)
- [ ] Handle recovery ID (v parameter)
- [ ] Convert public key to Ethereum address
- [ ] Apply Keccak-256 hash for address derivation
- [ ] Add error handling for invalid signatures

**Test Files** (TDD - Write First):
- `tests/crypto/test_signature.rs`
  - test_recover_public_key()
  - test_ethereum_address_derivation()
  - test_matches_sdk_signature()
  - test_invalid_signature_format()
  - test_invalid_recovery_id()
  - test_signature_roundtrip()

**Success Criteria**:
- Signature recovery works
- Address matches SDK expectations
- Invalid signatures rejected

### Sub-phase 2.2: Session Init Decryption
**Goal**: Decrypt and verify session initialization payload

**Tasks**:
- [ ] Implement `decrypt_session_init()` function
- [ ] Parse encrypted payload struct
- [ ] Perform ECDH with client's ephemeral public key
- [ ] Decrypt session data with derived key
- [ ] Recover client address from signature
- [ ] Verify signature over ciphertext
- [ ] Parse decrypted session data (job_id, model_name, session_key)
- [ ] Return session data + client address

**Test Files** (TDD - Write First):
- `tests/crypto/test_session_init.rs`
  - test_decrypt_session_init_valid()
  - test_session_init_with_sdk_data()
  - test_signature_verification()
  - test_invalid_signature()
  - test_corrupted_ciphertext()
  - test_wrong_node_key()
  - test_extract_session_key()

**Success Criteria**:
- Session init decrypts successfully
- Client address recovered correctly
- Invalid payloads rejected

## Phase 3: Session Key Management

### Sub-phase 3.1: In-Memory Session Key Store
**Goal**: Store session keys securely in memory

**Tasks**:
- [ ] Create `src/crypto/session_keys.rs` module
- [ ] Implement `SessionKeyStore` struct with HashMap
- [ ] Implement `store_key(session_id, key)` method
- [ ] Implement `get_key(session_id)` method
- [ ] Implement `clear_key(session_id)` method
- [ ] Implement `clear_expired_keys()` with TTL
- [ ] Add thread-safe Arc<RwLock<>> wrapper
- [ ] Log key operations (without logging actual keys)

**Test Files** (TDD - Write First):
- `tests/crypto/test_session_keys.rs`
  - test_store_and_retrieve_key()
  - test_get_nonexistent_key()
  - test_clear_key()
  - test_concurrent_access()
  - test_key_expiration()
  - test_multiple_sessions()

**Success Criteria**:
- Keys stored and retrieved correctly
- Thread-safe concurrent access
- Keys cleared on session end

### Sub-phase 3.2: Session Lifecycle Integration
**Goal**: Integrate session keys with session lifecycle

**Tasks**:
- [ ] Add `session_key_store` to ApiServer state
- [ ] Store session key on successful init
- [ ] Retrieve session key for message decryption
- [ ] Clear session key on WebSocket disconnect
- [ ] Clear session key on session timeout
- [ ] Add session key metrics (count, memory usage)

**Test Files** (TDD - Write First):
- `tests/crypto/test_session_lifecycle.rs`
  - test_session_key_stored_on_init()
  - test_session_key_used_for_decryption()
  - test_session_key_cleared_on_disconnect()
  - test_session_key_cleared_on_timeout()
  - test_session_without_encryption()

**Success Criteria**:
- Session keys integrated into lifecycle
- Keys cleared automatically
- No memory leaks

## Phase 4: WebSocket Message Types

### Sub-phase 4.1: Encrypted Message Type Definitions
**Goal**: Add encrypted message types to WebSocket protocol

**Tasks**:
- [ ] Update `src/api/websocket/message_types.rs`
- [ ] Add `EncryptedSessionInit` to `MessageType` enum
- [ ] Add `EncryptedMessage` to `MessageType` enum
- [ ] Add `EncryptedChunk` to `MessageType` enum
- [ ] Add `EncryptedResponse` to `MessageType` enum
- [ ] Create `EncryptedPayload` struct
- [ ] Create `SessionInitData` struct
- [ ] Implement serde serialization/deserialization

**Test Files** (TDD - Write First):
- `tests/websocket/test_encrypted_messages.rs`
  - test_encrypted_session_init_parsing()
  - test_encrypted_message_parsing()
  - test_encrypted_payload_structure()
  - test_message_type_serialization()
  - test_backward_compatible_parsing()

**Success Criteria**:
- Message types parse correctly
- Serde works for all types
- Backward compatible with plaintext

### Sub-phase 4.2: Message Parsing and Validation
**Goal**: Parse and validate encrypted messages

**Tasks**:
- [ ] Implement `EncryptedPayload::from_json()`
- [ ] Validate hex-encoded fields (ephPubHex, ciphertextHex, etc.)
- [ ] Validate nonce size (24 bytes for XChaCha20)
- [ ] Validate signature size (65 bytes)
- [ ] Validate ephemeral public key size (33 bytes compressed)
- [ ] Add error types for validation failures

**Test Files** (TDD - Write First):
- `tests/websocket/test_message_parsing.rs`
  - test_parse_valid_encrypted_payload()
  - test_invalid_hex_encoding()
  - test_invalid_nonce_size()
  - test_invalid_signature_size()
  - test_invalid_pubkey_size()
  - test_missing_fields()

**Success Criteria**:
- Valid messages parse successfully
- Invalid messages rejected with clear errors
- All sizes validated

## Phase 5: WebSocket Handler Integration

### Sub-phase 5.1: Encrypted Session Init Handler
**Goal**: Handle encrypted session initialization

**Tasks**:
- [ ] Add `handle_encrypted_session_init()` function to `src/api/server.rs`
- [ ] Parse encrypted_session_init message from JSON
- [ ] Call `decrypt_session_init()` with node's private key
- [ ] Extract session data (job_id, model_name, session_key, price)
- [ ] Recover and log client address
- [ ] Store session key in SessionKeyStore
- [ ] Track session metadata (job_id, chain_id, client_address)
- [ ] Send `session_init_ack` response

**Test Files** (TDD - Write First):
- `tests/websocket/test_encrypted_session_init.rs`
  - test_encrypted_init_handler()
  - test_init_stores_session_key()
  - test_init_recovers_client_address()
  - test_init_sends_acknowledgment()
  - test_init_invalid_signature()
  - test_init_decryption_failure()

**Success Criteria**:
- Encrypted init handled correctly
- Session key stored
- Client authenticated

### Sub-phase 5.2: Encrypted Message Handler
**Goal**: Handle encrypted prompt messages

**Tasks**:
- [ ] Add `handle_encrypted_message()` function
- [ ] Parse encrypted_message from JSON
- [ ] Retrieve session key from SessionKeyStore
- [ ] Decrypt message with session key
- [ ] Validate AAD for replay protection
- [ ] Extract plaintext prompt
- [ ] Process inference (existing logic)
- [ ] Return encrypted response

**Test Files** (TDD - Write First):
- `tests/websocket/test_encrypted_messages_handler.rs`
  - test_encrypted_message_handler()
  - test_message_decryption()
  - test_missing_session_key()
  - test_invalid_nonce()
  - test_aad_validation()
  - test_inference_with_encrypted_prompt()

**Success Criteria**:
- Encrypted messages decrypt successfully
- Missing session key handled
- AAD validated

### Sub-phase 5.3: Encrypted Response Streaming
**Goal**: Encrypt and stream response chunks

**Tasks**:
- [ ] Add `encrypt_and_send_chunk()` function
- [ ] Retrieve session key for encryption
- [ ] Generate random 24-byte nonce per chunk
- [ ] Prepare AAD with message index + timestamp
- [ ] Encrypt chunk with XChaCha20-Poly1305
- [ ] Send encrypted_chunk message
- [ ] Handle streaming completion
- [ ] Send final encrypted_response

**Test Files** (TDD - Write First):
- `tests/websocket/test_encrypted_streaming.rs`
  - test_encrypt_response_chunk()
  - test_streaming_encrypted_chunks()
  - test_unique_nonces_per_chunk()
  - test_aad_includes_index()
  - test_final_encrypted_response()
  - test_streaming_without_session_key()

**Success Criteria**:
- Response chunks encrypted correctly
- Nonces unique per chunk
- Streaming works end-to-end

### Sub-phase 5.4: Backward Compatibility
**Goal**: Support both encrypted and plaintext sessions

**Tasks**:
- [ ] Keep existing `session_init` handler
- [ ] Keep existing `prompt` handler
- [ ] Add plaintext detection in message router
- [ ] Log deprecation warnings for plaintext
- [ ] Add encryption status to session metadata
- [ ] Support mixed sessions per connection (?)
- [ ] Document plaintext deprecation timeline

**Test Files** (TDD - Write First):
- `tests/websocket/test_backward_compat.rs`
  - test_plaintext_session_still_works()
  - test_plaintext_deprecation_warning()
  - test_encrypted_and_plaintext_separate()
  - test_upgrade_plaintext_to_encrypted()
  - test_reject_mixed_mode()

**Success Criteria**:
- Plaintext sessions still work
- Warnings logged for plaintext
- Encryption preferred by default

## Phase 6: Node Private Key Access

### Sub-phase 6.1: Private Key Extraction
**Goal**: Extract node's private key from environment

**Tasks**:
- [ ] Read `HOST_PRIVATE_KEY` from environment
- [ ] Parse using ethers `LocalWallet::from_str()`
- [ ] Extract raw 32-byte private key
- [ ] Validate key format (0x-prefixed hex)
- [ ] Add error handling for missing/invalid key
- [ ] Log key availability (NOT the key itself)

**Test Files** (TDD - Write First):
- `tests/crypto/test_private_key.rs`
  - test_extract_private_key_from_env()
  - test_invalid_key_format()
  - test_missing_host_private_key()
  - test_key_validation()

**Success Criteria**:
- Private key extracted correctly
- Invalid keys rejected
- Never logged to console

### Sub-phase 6.2: Key Propagation to Server
**Goal**: Pass private key to ApiServer for encryption

**Tasks**:
- [ ] Add `node_private_key` field to ApiServer
- [ ] Pass private key during ApiServer initialization
- [ ] Make key available to WebSocket handler
- [ ] Store key in secure format (zero on drop)
- [ ] Handle optional key (for non-encrypted mode)

**Test Files** (TDD - Write First):
- `tests/api/test_server_crypto.rs`
  - test_server_with_private_key()
  - test_server_without_private_key()
  - test_key_available_to_handler()
  - test_key_not_logged()

**Success Criteria**:
- Key propagated to server
- Encryption available when key present
- Fallback to plaintext-only when absent

## Phase 7: Error Handling

### Sub-phase 7.1: Crypto Error Types
**Goal**: Define comprehensive error types for crypto operations

**Tasks**:
- [ ] Create `CryptoError` enum in `src/crypto/error.rs`
- [ ] Add variants: DecryptionFailed, InvalidSignature, InvalidKey, etc.
- [ ] Implement Display and Error traits
- [ ] Add error context (session_id, operation)
- [ ] Create From implementations for crypto library errors
- [ ] Add error logging with context

**Test Files** (TDD - Write First):
- `tests/crypto/test_errors.rs`
  - test_crypto_error_types()
  - test_error_display()
  - test_error_context()
  - test_error_conversion()

**Success Criteria**:
- Errors well-defined
- Context preserved
- Clear error messages

### Sub-phase 7.2: WebSocket Error Responses
**Goal**: Send appropriate error messages to clients

**Tasks**:
- [ ] Send error on decryption failure
- [ ] Send error on invalid signature
- [ ] Send error on missing session key
- [ ] Send error on session corruption
- [ ] Include error codes for client handling
- [ ] Close WebSocket on critical errors
- [ ] Log errors with session context

**Test Files** (TDD - Write First):
- `tests/websocket/test_crypto_errors.rs`
  - test_decryption_failure_response()
  - test_invalid_signature_response()
  - test_missing_session_key_response()
  - test_error_closes_connection()
  - test_error_logged_with_context()

**Success Criteria**:
- Errors sent to client
- Connections closed appropriately
- Errors logged for debugging

## Phase 8: Testing and Validation

### Sub-phase 8.1: Unit Tests
**Goal**: Comprehensive unit tests for all crypto functions

**Tasks**:
- [ ] Test ECDH key derivation
- [ ] Test XChaCha20-Poly1305 encryption/decryption
- [ ] Test signature recovery
- [ ] Test session init decryption
- [ ] Test session key storage
- [ ] Test all error paths
- [ ] Achieve >90% code coverage

**Test Files** (TDD - Write First):
- All test files from previous phases
- `tests/crypto/test_coverage.rs`
  - test_all_error_paths_covered()

**Success Criteria**:
- All unit tests pass
- Code coverage >90%
- Edge cases covered

### Sub-phase 8.2: Integration Tests
**Goal**: Test encryption end-to-end with SDK

**Tasks**:
- [ ] Create test harness with SDK Phase 6.2
- [ ] Test encrypted session initialization
- [ ] Test encrypted prompt/response flow
- [ ] Test streaming with encryption
- [ ] Test session key lifecycle
- [ ] Test concurrent encrypted sessions
- [ ] Test encryption + settlement flow

**Test Files** (TDD - Write First):
- `tests/integration/test_e2e_encryption.rs`
  - test_encrypted_session_flow()
  - test_encrypted_streaming()
  - test_concurrent_encrypted_sessions()
  - test_encryption_with_settlement()

**Success Criteria**:
- E2E tests pass with SDK
- Streaming works correctly
- Settlement unaffected

### Sub-phase 8.3: Security Testing
**Goal**: Validate security properties

**Tasks**:
- [ ] Test replay attack prevention (AAD validation)
- [ ] Test signature forgery attempts
- [ ] Test man-in-the-middle scenarios
- [ ] Test session key isolation
- [ ] Test nonce reuse detection
- [ ] Test timing attack resistance
- [ ] Audit crypto implementation

**Test Files** (TDD - Write First):
- `tests/security/test_crypto_security.rs`
  - test_replay_attack_prevented()
  - test_signature_forgery_rejected()
  - test_mitm_detected()
  - test_session_isolation()
  - test_nonce_uniqueness()

**Success Criteria**:
- Security tests pass
- No vulnerabilities found
- Crypto properly implemented

## Phase 9: Documentation

### Sub-phase 9.1: Implementation Documentation
**Goal**: Document the encryption implementation

**Tasks**:
- [ ] Update `docs/API.md` with encryption protocol
- [ ] Create `docs/ENCRYPTION_SECURITY.md`
- [ ] Document key management practices
- [ ] Add encryption troubleshooting guide
- [ ] Create deployment guide with encryption
- [ ] Document backward compatibility

**Deliverables**:
- `docs/ENCRYPTION_SECURITY.md`
- Updated `docs/API.md`
- Updated `docs/TROUBLESHOOTING.md`
- Updated `docs/DEPLOYMENT.md`

**Success Criteria**:
- Documentation complete
- Security practices documented
- Troubleshooting guide helpful

### Sub-phase 9.2: Migration Guide
**Goal**: Help node operators enable encryption

**Tasks**:
- [ ] Create migration checklist
- [ ] Document HOST_PRIVATE_KEY configuration
- [ ] Explain client SDK update requirements
- [ ] Document testing procedures
- [ ] Create rollback plan

**Deliverables**:
- `docs/ENCRYPTION_MIGRATION.md`

**Success Criteria**:
- Migration guide clear
- Steps tested
- Rollback documented

## Implementation Timeline

**Phase 1**: 2-3 days - Cryptography Foundation
**Phase 2**: 2 days - Signature Verification
**Phase 3**: 1-2 days - Session Key Management
**Phase 4**: 1 day - WebSocket Message Types
**Phase 5**: 2-3 days - WebSocket Handler Integration
**Phase 6**: 1 day - Node Private Key Access
**Phase 7**: 1-2 days - Error Handling
**Phase 8**: 2-3 days - Testing and Validation
**Phase 9**: 1-2 days - Documentation

**Total Timeline**: 2-3 weeks

## Current Progress Summary

### 🚧 Phase Status
- **Phase 1**: ✅ Complete - Cryptography Foundation
  - Sub-phase 1.1: ✅ Complete - Dependencies and Module Structure
  - Sub-phase 1.2: ✅ Complete - ECDH Key Exchange Implementation
  - Sub-phase 1.3: ✅ Complete - XChaCha20-Poly1305 Encryption
- **Phase 2**: Not Started - Signature Verification
- **Phase 3**: Not Started - Session Key Management
- **Phase 4**: Not Started - WebSocket Message Types
- **Phase 5**: Not Started - WebSocket Handler Integration
- **Phase 6**: Not Started - Node Private Key Access
- **Phase 7**: Not Started - Error Handling
- **Phase 8**: Not Started - Testing and Validation
- **Phase 9**: Not Started - Documentation

**Implementation Status**: 🟢 **IN PROGRESS** - Phase 1 complete, ready for Phase 2

## Critical Path

1. **Phase 1.2-1.3**: ECDH + encryption must work correctly
2. **Phase 2.1-2.2**: Signature recovery critical for authentication
3. **Phase 3.1**: Session key storage must be secure
4. **Phase 5.1-5.3**: WebSocket integration is core functionality
5. **Phase 8**: Testing validates entire implementation

## Risk Mitigation

1. **Crypto Implementation Bugs**: Use battle-tested libraries (k256, chacha20poly1305)
2. **Key Management**: Never persist keys, zero on drop
3. **Replay Attacks**: Validate AAD with timestamp and index
4. **Signature Forgery**: Use standard ECDSA recovery
5. **Performance**: Encryption overhead <1ms per message
6. **Backward Compatibility**: Keep plaintext support during migration

## Success Metrics

- **Functional**: All tests passing (100%)
- **Security**: No crypto vulnerabilities found
- **Performance**: Encryption overhead <1ms per message
- **Compatibility**: SDK Phase 6.2 clients work seamlessly
- **Coverage**: >90% code coverage in crypto modules
- **Documentation**: Complete implementation and security docs

## Dependencies

### External Crates
```toml
k256 = { version = "0.13", features = ["ecdh", "ecdsa"] }
chacha20poly1305 = "0.10"
hkdf = "0.12"
sha2 = "0.10"  # already exists
```

### SDK Requirements
- SDK must be at Phase 6.2 or later
- Clients must send `encrypted_session_init` messages
- Session keys must be 32 bytes (256-bit)
- Signatures must be Ethereum-compatible ECDSA

## Notes

- Each sub-phase should be completed before moving to the next
- Write tests FIRST (TDD approach)
- Keep backward compatibility with plaintext sessions
- Document all security considerations
- Never log session keys or plaintext messages
- Use secure random for nonces
- Verify all signature operations
- Test with real SDK Phase 6.2 client

## Reference Documentation

- **SDK Encryption Guide**: `docs/sdk-reference/NODE_ENCRYPTION_GUIDE.md`
- **Cryptographic Libraries**:
  - k256: https://docs.rs/k256/
  - chacha20poly1305: https://docs.rs/chacha20poly1305/
  - hkdf: https://docs.rs/hkdf/
- **Standards**:
  - XChaCha20-Poly1305: https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha
  - HKDF: RFC 5869
  - ECDSA: SEC 1 v2

## Security Considerations

### Do's ✅
- Use battle-tested crypto libraries
- Validate all inputs (sizes, formats)
- Store session keys in memory only
- Clear keys on session end
- Use unique nonces per encryption
- Verify signatures before processing
- Log errors (without sensitive data)

### Don'ts ❌
- Never reuse nonces with same key
- Never persist session keys to disk
- Never log session keys or plaintext
- Never skip signature verification
- Never use weak random for nonces
- Never trust client-provided keys
- Never implement custom crypto primitives
