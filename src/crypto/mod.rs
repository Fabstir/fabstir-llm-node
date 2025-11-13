// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! End-to-End Encryption Module (Phase 6.2)
//!
//! This module implements the cryptographic primitives needed for end-to-end
//! encryption between SDK clients and the node:
//!
//! - **ECDH**: Ephemeral-static key exchange using secp256k1
//! - **Encryption**: XChaCha20-Poly1305 AEAD for message encryption
//! - **Signature**: ECDSA signature recovery for client authentication
//! - **Session Keys**: In-memory storage of session encryption keys
//! - **EZKL**: Zero-knowledge proof generation for result commitments (Phase 1.1)
//!
//! ## Security Considerations
//!
//! - Session keys are stored in memory only, never persisted
//! - Nonces must be unique per encryption operation
//! - Signatures are verified before processing messages
//! - AAD (Additional Authenticated Data) prevents replay attacks
//!
//! ## Protocol Flow
//!
//! 1. Client generates ephemeral keypair and performs ECDH with node's public key
//! 2. Client derives encryption key using HKDF-SHA256
//! 3. Client encrypts session init payload (contains random session key)
//! 4. Client signs encrypted payload with wallet private key
//! 5. Node receives, performs ECDH, decrypts, verifies signature
//! 6. Node stores session key for subsequent message encryption
//! 7. All messages encrypted with session key using XChaCha20-Poly1305

pub mod aes_gcm;
pub mod ecdh;
pub mod encryption;
pub mod error;
pub mod ezkl;
pub mod private_key;
pub mod session_init;
pub mod session_keys;
pub mod signature;

pub use aes_gcm::{decrypt_aes_gcm, decrypt_chunk, decrypt_manifest, extract_nonce};
pub use ecdh::derive_shared_key;
pub use encryption::{decrypt_with_aead, encrypt_with_aead};
pub use error::CryptoError;
pub use private_key::extract_node_private_key;
pub use session_init::{decrypt_session_init, EncryptedSessionPayload, SessionInitData};
pub use session_keys::SessionKeyStore;
pub use signature::recover_client_address;
