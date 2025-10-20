// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Crypto Error Types (Phase 7, Sub-phase 7.1)
//!
//! Comprehensive error types for all cryptographic operations with context preservation.
//!
//! ## Error Variants
//!
//! - **DecryptionFailed**: AEAD decryption failed (wrong key, corrupted ciphertext, auth tag mismatch)
//! - **InvalidSignature**: ECDSA signature verification or recovery failed
//! - **InvalidKey**: Invalid cryptographic key (wrong size, invalid point, malformed)
//! - **InvalidNonce**: Nonce size validation failed (XChaCha20 requires 24 bytes)
//! - **KeyDerivationFailed**: ECDH or HKDF key derivation failed
//! - **InvalidPayload**: Encrypted payload validation failed (missing fields, wrong format)
//! - **SessionKeyNotFound**: Session key not found in SessionKeyStore
//! - **Other**: Generic error for library errors or unexpected failures
//!
//! ## Context Preservation
//!
//! All error variants include contextual information:
//! - **operation**: Which crypto operation failed (e.g., "session_init", "encrypted_message")
//! - **reason**: Specific failure reason (e.g., "authentication tag mismatch")
//! - **session_id**: Which session was affected (when applicable)
//! - **key_type**: Which type of key failed (e.g., "ephemeral_public_key", "node_private_key")
//!
//! ## Usage Example
//!
//! ```rust
//! use fabstir_llm_node::crypto::CryptoError;
//!
//! fn decrypt_message(ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
//!     // Perform decryption...
//!     Err(CryptoError::DecryptionFailed {
//!         operation: "encrypted_message".to_string(),
//!         reason: "authentication tag verification failed".to_string(),
//!     })
//! }
//! ```

use std::fmt;

/// Comprehensive error type for all cryptographic operations
#[derive(Debug, Clone)]
pub enum CryptoError {
    /// AEAD decryption failed
    ///
    /// This error occurs when:
    /// - Authentication tag verification fails (ciphertext tampered or wrong key)
    /// - Ciphertext is corrupted
    /// - AAD doesn't match
    DecryptionFailed {
        /// Which operation was being performed
        operation: String,
        /// Specific failure reason
        reason: String,
    },

    /// ECDSA signature verification or recovery failed
    ///
    /// This error occurs when:
    /// - Signature is malformed (wrong size, invalid format)
    /// - Recovery ID is invalid
    /// - Signature doesn't match the message hash
    InvalidSignature {
        /// Which operation was being performed
        operation: String,
        /// Specific failure reason
        reason: String,
    },

    /// Invalid cryptographic key
    ///
    /// This error occurs when:
    /// - Key has wrong length
    /// - Key represents invalid curve point
    /// - Key format is unrecognized
    InvalidKey {
        /// Type of key that failed (e.g., "ephemeral_public_key", "node_private_key")
        key_type: String,
        /// Specific failure reason
        reason: String,
    },

    /// Invalid nonce size
    ///
    /// XChaCha20-Poly1305 requires exactly 24-byte nonces.
    InvalidNonce {
        /// Expected nonce size (always 24 for XChaCha20)
        expected_size: usize,
        /// Actual nonce size provided
        actual_size: usize,
    },

    /// Key derivation failed (ECDH or HKDF)
    ///
    /// This error occurs when:
    /// - ECDH shared secret computation fails
    /// - HKDF expansion fails
    /// - Ephemeral public key is invalid
    KeyDerivationFailed {
        /// Which key derivation operation failed
        operation: String,
        /// Specific failure reason
        reason: String,
    },

    /// Encrypted payload validation failed
    ///
    /// This error occurs when:
    /// - Required fields are missing
    /// - Hex decoding fails
    /// - Field has wrong format or size
    InvalidPayload {
        /// Which field failed validation
        field: String,
        /// Specific failure reason
        reason: String,
    },

    /// Session key not found in SessionKeyStore
    ///
    /// This error occurs when:
    /// - Session hasn't been initialized
    /// - Session key has expired
    /// - Session was explicitly cleared
    SessionKeyNotFound {
        /// Session ID that was not found
        session_id: String,
    },

    /// Generic error for library errors or unexpected failures
    Other(String),
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CryptoError::DecryptionFailed { operation, reason } => {
                write!(f, "Decryption failed during {}: {}", operation, reason)
            }
            CryptoError::InvalidSignature { operation, reason } => {
                write!(f, "Invalid signature during {}: {}", operation, reason)
            }
            CryptoError::InvalidKey { key_type, reason } => {
                write!(f, "Invalid key ({}): {}", key_type, reason)
            }
            CryptoError::InvalidNonce { expected_size, actual_size } => {
                write!(
                    f,
                    "Invalid nonce size: expected {} bytes, got {} bytes",
                    expected_size, actual_size
                )
            }
            CryptoError::KeyDerivationFailed { operation, reason } => {
                write!(f, "Key derivation failed during {}: {}", operation, reason)
            }
            CryptoError::InvalidPayload { field, reason } => {
                write!(f, "Invalid payload field '{}': {}", field, reason)
            }
            CryptoError::SessionKeyNotFound { session_id } => {
                write!(f, "Session key not found for session_id: {}", session_id)
            }
            CryptoError::Other(msg) => {
                write!(f, "Crypto error: {}", msg)
            }
        }
    }
}

impl std::error::Error for CryptoError {}

// Conversion from anyhow::Error
impl From<anyhow::Error> for CryptoError {
    fn from(err: anyhow::Error) -> Self {
        CryptoError::Other(err.to_string())
    }
}

// Conversion from hex decode errors
impl From<hex::FromHexError> for CryptoError {
    fn from(err: hex::FromHexError) -> Self {
        CryptoError::InvalidPayload {
            field: "hex_field".to_string(),
            reason: format!("hex decode error: {}", err),
        }
    }
}

// Conversion from k256 errors (elliptic curve operations)
impl From<k256::elliptic_curve::Error> for CryptoError {
    fn from(err: k256::elliptic_curve::Error) -> Self {
        CryptoError::InvalidKey {
            key_type: "unknown".to_string(),
            reason: format!("k256 error: {}", err),
        }
    }
}

// Conversion from chacha20poly1305 errors
impl From<chacha20poly1305::aead::Error> for CryptoError {
    fn from(err: chacha20poly1305::aead::Error) -> Self {
        CryptoError::DecryptionFailed {
            operation: "AEAD".to_string(),
            reason: format!("chacha20poly1305 error: {}", err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_messages() {
        let err = CryptoError::DecryptionFailed {
            operation: "test".to_string(),
            reason: "test reason".to_string(),
        };
        assert_eq!(
            format!("{}", err),
            "Decryption failed during test: test reason"
        );

        let err = CryptoError::InvalidNonce {
            expected_size: 24,
            actual_size: 16,
        };
        assert_eq!(
            format!("{}", err),
            "Invalid nonce size: expected 24 bytes, got 16 bytes"
        );

        let err = CryptoError::SessionKeyNotFound {
            session_id: "test-123".to_string(),
        };
        assert_eq!(
            format!("{}", err),
            "Session key not found for session_id: test-123"
        );
    }

    #[test]
    fn test_error_implements_error_trait() {
        let err: Box<dyn std::error::Error> = Box::new(CryptoError::Other("test".to_string()));
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn test_from_anyhow_conversion() {
        let anyhow_err = anyhow::anyhow!("test anyhow error");
        let crypto_err: CryptoError = anyhow_err.into();

        match crypto_err {
            CryptoError::Other(msg) => assert!(msg.contains("test anyhow error")),
            _ => panic!("Expected CryptoError::Other"),
        }
    }

    #[test]
    fn test_from_hex_error_conversion() {
        let hex_err = hex::decode("not_valid_hex").unwrap_err();
        let crypto_err: CryptoError = hex_err.into();

        match crypto_err {
            CryptoError::InvalidPayload { field, reason } => {
                assert_eq!(field, "hex_field");
                assert!(reason.contains("decode"));
            }
            _ => panic!("Expected CryptoError::InvalidPayload"),
        }
    }
}
