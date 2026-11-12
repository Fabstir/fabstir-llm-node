// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The claim-signing key (plan D5; C2PA conformance Level 1: encrypted at rest and in memory except while
//! signing).
//!
//! At rest: an ENCRYPTED PKCS#8 PEM (PBES2, scrypt + AES-256-CBC); a plaintext key file is refused. The
//! passphrase comes from a systemd credential (`LoadCredentialEncrypted=`), i.e. it is itself encrypted at
//! rest by systemd-creds. In memory: right after decryption the scalar is re-sealed with ChaCha20-Poly1305
//! under a random per-process key; it is unsealed into a zeroizing buffer for each signature only.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use p256::ecdsa::{signature::Signer as _, Signature, SigningKey};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use rand::RngCore;
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("key file is not an ENCRYPTED PRIVATE KEY (plaintext keys are refused)")]
    NotEncrypted,
    #[error("key file unreadable or wrong passphrase")]
    Decrypt,
    #[error("passphrase credential missing or empty")]
    NoPassphrase,
    #[error("sealed key unusable")]
    Sealed,
}

/// A P-256 key held sealed in memory.
pub struct KeyStore {
    wrap: Zeroizing<[u8; 32]>,
    nonce: [u8; 12],
    sealed: Vec<u8>,
    public_sec1: Vec<u8>,
}

impl std::fmt::Debug for KeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyStore")
            .field("public_sec1", &hex::encode(&self.public_sec1))
            .finish_non_exhaustive()
    }
}

impl KeyStore {
    /// Load from encrypted PKCS#8 PEM text + passphrase.
    pub fn from_encrypted_pem(pem_text: &str, passphrase: &[u8]) -> Result<KeyStore, KeyError> {
        if passphrase.is_empty() {
            return Err(KeyError::NoPassphrase);
        }
        let trimmed = pem_text.trim_start();
        if !trimmed.starts_with("-----BEGIN ENCRYPTED PRIVATE KEY-----")
            || pem_text.contains("-----BEGIN PRIVATE KEY-----")
            || pem_text.contains("-----BEGIN EC PRIVATE KEY-----")
        {
            return Err(KeyError::NotEncrypted);
        }
        let sk = SigningKey::from_pkcs8_encrypted_pem(pem_text, passphrase)
            .map_err(|_| KeyError::Decrypt)?;
        Ok(Self::seal(sk))
    }

    /// Load from a key file and a passphrase file (the systemd credential path).
    pub fn load(
        key_path: &std::path::Path,
        passphrase_path: &std::path::Path,
    ) -> Result<KeyStore, KeyError> {
        let pem_text =
            Zeroizing::new(std::fs::read_to_string(key_path).map_err(|_| KeyError::Decrypt)?);
        let mut pass =
            Zeroizing::new(std::fs::read(passphrase_path).map_err(|_| KeyError::NoPassphrase)?);
        while pass
            .last()
            .map(|b| *b == b'\n' || *b == b'\r')
            .unwrap_or(false)
        {
            pass.pop();
        }
        Self::from_encrypted_pem(&pem_text, &pass)
    }

    fn seal(sk: SigningKey) -> KeyStore {
        let mut wrap = Zeroizing::new([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(&mut wrap[..]);
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let mut fb = sk.to_bytes();
        let scalar = Zeroizing::new(fb.to_vec());
        fb.as_mut_slice().zeroize();
        let sealed = ChaCha20Poly1305::new(Key::from_slice(&wrap[..]))
            .encrypt(Nonce::from_slice(&nonce), scalar.as_slice())
            .expect("aead seal");
        let public_sec1 = sk
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();
        drop(sk); // p256 SigningKey zeroizes on drop
        KeyStore {
            wrap,
            nonce,
            sealed,
            public_sec1,
        }
    }

    /// Sign `data` (ES256, P1363 r||s). The key exists in plaintext only inside this call.
    pub fn sign(&self, data: &[u8]) -> Result<Vec<u8>, KeyError> {
        let mut scalar = Zeroizing::new(
            ChaCha20Poly1305::new(Key::from_slice(&self.wrap[..]))
                .decrypt(Nonce::from_slice(&self.nonce), self.sealed.as_slice())
                .map_err(|_| KeyError::Sealed)?,
        );
        let sk = SigningKey::from_slice(&scalar).map_err(|_| KeyError::Sealed)?;
        scalar.zeroize();
        let sig: Signature = sk.sign(data);
        Ok(sig.to_bytes().to_vec())
    }

    /// Uncompressed SEC1 public key (to check it matches the certificate).
    pub fn public_sec1(&self) -> &[u8] {
        &self.public_sec1
    }
}

/// Encrypt a fresh or given key as PKCS#8 PEM under `passphrase` (gen-staging-chain, key ceremony).
pub fn encrypt_pkcs8_pem(sk: &SigningKey, passphrase: &[u8]) -> anyhow::Result<Zeroizing<String>> {
    let doc = sk.to_pkcs8_der()?;
    let pki = pkcs8::PrivateKeyInfo::try_from(doc.as_bytes())?;
    let enc = pki.encrypt(rand::rngs::OsRng, passphrase)?;
    Ok(Zeroizing::new(
        enc.to_pem("ENCRYPTED PRIVATE KEY", pkcs8::LineEnding::LF)?
            .to_string(),
    ))
}
