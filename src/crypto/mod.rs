//! End-to-End Encryption Module (Phase 6.2)
//!
//! This module implements the cryptographic primitives needed for end-to-end
//! encryption between SDK clients and the node:
//!
//! - **ECDH**: Ephemeral-static key exchange using secp256k1
//! - **Encryption**: XChaCha20-Poly1305 AEAD for message encryption
//! - **Signature**: ECDSA signature recovery for client authentication
//! - **Session Keys**: In-memory storage of session encryption keys
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

pub mod ecdh;
pub mod encryption;
pub mod session_keys;
pub mod signature;

pub use ecdh::derive_shared_key;
pub use encryption::{decrypt_with_aead, encrypt_with_aead};
pub use session_keys::SessionKeyStore;
pub use signature::recover_client_address;
