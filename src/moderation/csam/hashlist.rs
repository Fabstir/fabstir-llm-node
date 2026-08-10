// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `HashListSource` adapter + availability-tagged snapshot (fail-closed core).
//!
//! Trait/type *signatures* pinned here (Sub-phase 0.2.4). `NcmecHashStore` +
//! `MockHashListSource` impls land in Sub-phase 3.1.

use std::collections::HashSet;
use std::sync::RwLock;

use crate::moderation::types::{ModerationError, Pdq256, Result};

/// Availability of the block-hash list (NCMEC-sourced or operator-loaded —
/// WP-N2). `Unavailable` (including first boot before any successful refresh)
/// ⇒ the matcher HOLDs; a failed refresh never installs an empty `Loaded` list
/// (§3.4, fail-closed core).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListState {
    /// A successful refresh — or an explicit, logged operator list load
    /// (WP-N2 `MODERATION_LIST_FILE`) — installed this list.
    Loaded,
    /// Last-good list retained past its refresh window; `age_secs` since the last
    /// success. Usable only within a bounded TTL when stale-reuse is explicitly
    /// enabled (D7, off by default).
    Stale { age_secs: u64 },
    /// No usable list (first boot, or a failed refresh under the default policy).
    Unavailable,
}

/// A point-in-time view of the block-hash list, tagged with its availability
/// state. `version` is a monotonic counter on the NCMEC refresh path but a
/// NON-ORDINAL content fingerprint for operator-loaded lists (WP-N2 rule 6) —
/// never compare versions with `>`.
#[derive(Clone)]
pub struct HashListSnapshot {
    pub state: ListState,
    pub sha256: HashSet<[u8; 32]>,
    pub pdq: Vec<Pdq256>,
    pub version: u64,
}

impl HashListSnapshot {
    /// The fail-closed "no usable list" snapshot (empty sets, `Unavailable`).
    pub fn unavailable() -> Self {
        Self {
            state: ListState::Unavailable,
            sha256: HashSet::new(),
            pdq: Vec::new(),
            version: 0,
        }
    }

    /// Gate matching on availability (§3.4): `Unavailable` ⇒ `Err(ListUnavailable)`
    /// (the matcher propagates this to a HOLD — it is NOT a clean/empty list). A
    /// `Loaded` list, or a `Stale` one (only ever installed within TTL when D7
    /// stale-reuse is explicitly enabled), is usable.
    pub fn require_available(&self) -> Result<()> {
        match self.state {
            ListState::Unavailable => Err(ModerationError::ListUnavailable),
            ListState::Loaded | ListState::Stale { .. } => Ok(()),
        }
    }
}

/// Source of NCMEC / known-bad hashes. A failed refresh must NEVER yield an
/// empty "clean" snapshot — it yields `ListState::Unavailable` instead (§3.3/§3.4).
pub trait HashListSource {
    /// Fetch the current snapshot (never an empty `Loaded` list on failure).
    fn refresh(&self) -> Result<HashListSnapshot>;
    /// Monotonic version of the installed list.
    fn version(&self) -> u64;
}

/// Last-good loaded list held by [`NcmecHashStore`].
struct LoadedList {
    sha256: HashSet<[u8; 32]>,
    pdq: Vec<Pdq256>,
    version: u64,
}

/// Encrypted-at-rest NCMEC hash store (D5). First boot — before any successful
/// refresh — is `Unavailable`; a refresh that cannot fetch yields `Unavailable`
/// (D7 default: never reuse, never install an empty `Loaded` list). The real
/// NCMEC Hash-Sharing client is wired at go-live behind [`HashListSource`]; until
/// `endpoint` is set, `refresh` stays `Unavailable` (fail-closed).
pub struct NcmecHashStore {
    current: RwLock<Option<LoadedList>>,
    /// At-rest key material (HKDF-derived per use; see `csam::atrest`).
    key_material: Vec<u8>,
    /// Real NCMEC endpoint; `None` until go-live ⇒ refresh stays `Unavailable`.
    endpoint: Option<String>,
}

impl NcmecHashStore {
    pub fn new(key_material: Vec<u8>) -> Self {
        Self {
            current: RwLock::new(None),
            key_material,
            endpoint: None,
        }
    }

    /// The current snapshot. No last-good list (first boot / after a failure under
    /// the D7 default) ⇒ `Unavailable`.
    pub fn current_snapshot(&self) -> HashListSnapshot {
        let guard = self.current.read().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(l) => HashListSnapshot {
                state: ListState::Loaded,
                sha256: l.sha256.clone(),
                pdq: l.pdq.clone(),
                version: l.version,
            },
            None => HashListSnapshot::unavailable(),
        }
    }

    /// Seal the current list to an encrypted at-rest blob (skeleton persistence).
    /// Returns `None` when there is no list to persist.
    pub fn seal_current(&self) -> Result<Option<Vec<u8>>> {
        let guard = self.current.read().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            None => Ok(None),
            Some(l) => {
                let plain: Vec<u8> = l.sha256.iter().flatten().copied().collect();
                super::atrest::seal(&self.key_material, &plain).map(Some)
            }
        }
    }
}

impl HashListSource for NcmecHashStore {
    fn refresh(&self) -> Result<HashListSnapshot> {
        // No real NCMEC endpoint wired yet (go-live). D7 default: a refresh that
        // cannot fetch yields Unavailable — NEVER an empty Loaded list, and never
        // silently reuses a stale list.
        match self.endpoint {
            None => Ok(HashListSnapshot::unavailable()),
            Some(_) => Ok(self.current_snapshot()), // real fetch path wired at go-live
        }
    }

    fn version(&self) -> u64 {
        self.current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|l| l.version)
            .unwrap_or(0)
    }
}
