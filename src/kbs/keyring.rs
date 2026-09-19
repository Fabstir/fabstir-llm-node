// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! The keyring (design §3): `keyring.json`, mode 0600, owned by the running user.
//!
//! Rules, all checked at load (violation = refuse to start, exit 78):
//! - `model_id` 32 bytes, `provider` 20-byte `0x` address, `dek` 32 bytes, all
//!   lowercase hex, no duplicates, at least one entry;
//! - `test: true` ⇔ `model_id` starts with the bytes `t5t:` (`7435743a`), a
//!   labelling convention enforced here and nowhere else; the security boundary is
//!   the flag × mode coupling: any test evidence mode ⇒ every entry `test: true`;
//!   both real ⇒ every entry `test: false` ([`Keyring::check_class`]);
//! - `min_policy_version ≥ 1`: a policy below it is refused (revocation = bump the
//!   floor + replace the file + restart; the immediate lever is a past `expiry`);
//! - the DEK is held in a `Zeroizing` wrapper, never logged, never captured.

use crate::tee::policy_source::ProviderRegistry;
use serde::{Deserialize, Serialize};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use zeroize::Zeroizing;

/// The reserved test-id prefix (`t5t:`), as bytes of the 32-byte model id.
pub const TEST_ID_PREFIX: &[u8; 4] = b"t5t:";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("keyring: {0}")]
pub struct KeyringError(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyringClass {
    Test,
    Real,
}

impl KeyringClass {
    /// The `/info` string.
    pub fn wire(self) -> &'static str {
        match self {
            KeyringClass::Test => "test",
            KeyringClass::Real => "real",
        }
    }
}

/// One validated entry. `dek` zeroises on drop.
pub struct KeyringEntry {
    pub model_id: [u8; 32],
    /// `0x` + 40 lowercase hex, as `ProviderRegistry` expects it.
    pub provider: String,
    pub dek: Zeroizing<[u8; 32]>,
    pub test: bool,
    pub min_policy_version: u32,
    pub note: String,
}

impl std::fmt::Debug for KeyringEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyringEntry")
            .field("model_id", &hex::encode(self.model_id))
            .field("provider", &self.provider)
            .field("dek", &"<redacted>")
            .field("test", &self.test)
            .field("min_policy_version", &self.min_policy_version)
            .field("note", &self.note)
            .finish()
    }
}

/// The on-disk shape (`schema: 1`). Unknown fields refuse. `Debug` redacts the DEKs
/// (`Zeroizing`'s own `Debug` would print them).
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyringFile {
    pub schema: u32,
    pub keys: Vec<KeyringFileEntry>,
}

impl std::fmt::Debug for KeyringFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyringFile")
            .field("schema", &self.schema)
            .field("keys", &self.keys)
            .finish()
    }
}

impl std::fmt::Debug for KeyringFileEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyringFileEntry")
            .field("model_id", &self.model_id)
            .field("provider", &self.provider)
            .field("dek", &"<redacted>")
            .field("test", &self.test)
            .field("min_policy_version", &self.min_policy_version)
            .field("note", &self.note)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyringFileEntry {
    pub model_id: String,
    pub provider: String,
    /// Hex; zeroised when the parsed file is dropped.
    pub dek: Zeroizing<String>,
    pub test: bool,
    #[serde(default = "one")]
    pub min_policy_version: u32,
    #[serde(default)]
    pub note: String,
}

fn one() -> u32 {
    1
}

/// The validated keyring.
#[derive(Debug)]
pub struct Keyring {
    entries: Vec<KeyringEntry>,
}

impl Keyring {
    /// Parse and validate the JSON (every §3 rule except file permissions and the
    /// mode coupling, which need the file and the config).
    pub fn parse(json: &[u8]) -> Result<Self, KeyringError> {
        let file: KeyringFile =
            serde_json::from_slice(json).map_err(|e| KeyringError(format!("parse: {e}")))?;
        if file.schema != 1 {
            return Err(KeyringError(format!(
                "schema {} unsupported (want 1)",
                file.schema
            )));
        }
        if file.keys.is_empty() {
            return Err(KeyringError("no entries".into()));
        }
        let mut entries = Vec::with_capacity(file.keys.len());
        for (i, e) in file.keys.iter().enumerate() {
            let at = |what: &str| KeyringError(format!("entry {i}: {what}"));
            let model_id = decode_lower_hex(&e.model_id, 32)
                .ok_or_else(|| at("model_id must be 64 lowercase hex"))?;
            let model_id: [u8; 32] = model_id.try_into().expect("32 bytes");
            let prov_hex = e
                .provider
                .strip_prefix("0x")
                .ok_or_else(|| at("provider must start with 0x"))?;
            if decode_lower_hex(prov_hex, 20).is_none() {
                return Err(at("provider must be 0x + 40 lowercase hex"));
            }
            let dek_vec = Zeroizing::new(
                decode_lower_hex(&e.dek, 32).ok_or_else(|| at("dek must be 64 lowercase hex"))?,
            );
            let mut dek = Zeroizing::new([0u8; 32]);
            dek.copy_from_slice(&dek_vec);
            let is_test_id = model_id.starts_with(TEST_ID_PREFIX);
            if e.test != is_test_id {
                return Err(at(if is_test_id {
                    "model_id carries the t5t: prefix but test is false"
                } else {
                    "test is true but model_id does not carry the t5t: prefix"
                }));
            }
            if e.min_policy_version < 1 {
                return Err(at("min_policy_version must be >= 1"));
            }
            if entries
                .iter()
                .any(|x: &KeyringEntry| x.model_id == model_id)
            {
                return Err(at("duplicate model_id"));
            }
            entries.push(KeyringEntry {
                model_id,
                provider: e.provider.clone(),
                dek,
                test: e.test,
                min_policy_version: e.min_policy_version,
                note: e.note.clone(),
            });
        }
        Ok(Self { entries })
    }

    /// Load from disk: the file must be mode 0600 and owned by the running user,
    /// then [`Keyring::parse`], then [`Keyring::check_class`] against
    /// `require_test` (design D3).
    pub fn load(path: &Path, require_test: bool) -> Result<Self, KeyringError> {
        let meta = std::fs::metadata(path)
            .map_err(|e| KeyringError(format!("{}: {e}", path.display())))?;
        if !meta.is_file() {
            return Err(KeyringError(format!(
                "{} is not a regular file",
                path.display()
            )));
        }
        let mode = meta.mode() & 0o777;
        if mode != 0o600 && mode != 0o400 {
            return Err(KeyringError(format!(
                "{} must be mode 0600 or 0400 (is {mode:o})",
                path.display()
            )));
        }
        // SAFETY: geteuid has no preconditions.
        let euid = unsafe { libc::geteuid() };
        if meta.uid() != euid {
            return Err(KeyringError(format!(
                "{} must be owned by the running user (uid {euid}, is {})",
                path.display(),
                meta.uid()
            )));
        }
        let bytes = Zeroizing::new(
            std::fs::read(path).map_err(|e| KeyringError(format!("{}: {e}", path.display())))?,
        );
        let ring = Self::parse(&bytes)?;
        ring.check_class(require_test)?;
        Ok(ring)
    }

    /// The mode coupling: a test mode needs every entry `test: true`; both real
    /// modes need every entry `test: false`.
    pub fn check_class(&self, require_test: bool) -> Result<(), KeyringError> {
        let class = self.class();
        match (require_test, class) {
            (true, Some(KeyringClass::Test)) | (false, Some(KeyringClass::Real)) => Ok(()),
            (true, _) => Err(KeyringError(
                "a test evidence mode (KBS_GPU_EVIDENCE=canned or KBS_CPU_EVIDENCE=simulator) requires every entry test: true".into(),
            )),
            (false, _) => Err(KeyringError(
                "real evidence modes require every entry test: false".into(),
            )),
        }
    }

    /// `Some(Test)` when every entry is test, `Some(Real)` when none is, `None`
    /// when mixed (which [`Keyring::check_class`] refuses in either mode).
    pub fn class(&self) -> Option<KeyringClass> {
        let tests = self.entries.iter().filter(|e| e.test).count();
        if tests == self.entries.len() {
            Some(KeyringClass::Test)
        } else if tests == 0 {
            Some(KeyringClass::Real)
        } else {
            None
        }
    }

    pub fn get(&self, model_id: &[u8; 32]) -> Option<&KeyringEntry> {
        self.entries.iter().find(|e| &e.model_id == model_id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `model_id → provider` for the node's `fetch_validated_policy`.
    pub fn provider_registry(&self) -> ProviderRegistry {
        self.entries.iter().fold(ProviderRegistry::new(), |r, e| {
            r.with_provider(e.model_id, e.provider.clone())
        })
    }
}

/// Strict lowercase hex of exactly `len` bytes (no `0x`, no upper case).
pub fn decode_lower_hex(s: &str, len: usize) -> Option<Vec<u8>> {
    if s.len() != len * 2 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    hex::decode(s).ok()
}
