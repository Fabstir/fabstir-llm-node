// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! The nonce store (design §5, gate A-8).
//!
//! In memory only. A nonce is issued for `(model_id, pk_att)` from a `source` (the
//! last `X-Forwarded-For` element; nginx overwrites it with `$remote_addr`), and
//! burned on `request_key` arrival BEFORE any verification: [`NonceStore::take`]
//! removes and returns it, then the caller binds it with [`Issued::bind`].
//!
//! Every method takes `now: Instant`; nothing in here reads a clock, so expiry is
//! monotonic (an NTP step neither extends nor shortens an outstanding nonce) and
//! the tests inject time. Caps refuse, never evict: an eviction policy would let a
//! fast flood remove the legitimate nonce.

use rand::{rngs::OsRng, RngCore};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapScope {
    Global,
    Source,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NonceError {
    /// The global or per-source outstanding cap is reached (`freshness`).
    TooManyOutstanding(CapScope),
    /// Never issued, or already burned (`freshness`; the log says which is unknowable).
    Unknown,
    /// Issued but past `issued_at + ttl` (`freshness`).
    Expired,
    /// Bound to another `pk_att` (`freshness`).
    BoundToAnotherKey,
    /// Bound to another model (`freshness`).
    BoundToAnotherModel,
}

impl std::fmt::Display for NonceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NonceError::TooManyOutstanding(CapScope::Global) => write!(f, "too many outstanding"),
            NonceError::TooManyOutstanding(CapScope::Source) => {
                write!(f, "too many outstanding for this source")
            }
            NonceError::Unknown => write!(f, "nonce unknown or already used"),
            NonceError::Expired => write!(f, "nonce expired"),
            NonceError::BoundToAnotherKey => write!(f, "nonce bound to another key"),
            NonceError::BoundToAnotherModel => write!(f, "nonce bound to another model"),
        }
    }
}

/// What a nonce was issued for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issued {
    pub model_id: [u8; 32],
    pub pk_att: [u8; 33],
    pub issued_at: Instant,
    pub source: String,
}

impl Issued {
    /// The binding check (design §5): the request's model and the evidence's key
    /// must be the ones the nonce was issued for. The nonce is already burned.
    pub fn bind(&self, model_id: &[u8; 32], pk_att: &[u8; 33]) -> Result<(), NonceError> {
        if &self.model_id != model_id {
            return Err(NonceError::BoundToAnotherModel);
        }
        if &self.pk_att != pk_att {
            return Err(NonceError::BoundToAnotherKey);
        }
        Ok(())
    }
}

struct Inner {
    map: HashMap<[u8; 32], Issued>,
    per_source: HashMap<String, usize>,
    /// Set once the store crossed 50 % occupancy (one CRITICAL line, not a flood).
    warned_half: bool,
    /// Issues since the last full sweep.
    since_sweep: u32,
    /// When the last sweep ran (a source parked at its cap must not force a full
    /// O(cap) walk on every challenge).
    last_sweep: Option<Instant>,
}

/// A full sweep walks the map under the lock: run it when a cap would otherwise
/// refuse, and every this many issues regardless.
const SWEEP_EVERY: u32 = 256;

pub struct NonceStore {
    ttl: Duration,
    cap: usize,
    per_source_cap: usize,
    inner: Mutex<Inner>,
}

impl NonceStore {
    pub fn new(ttl: Duration, cap: usize, per_source_cap: usize) -> Self {
        Self {
            ttl,
            cap,
            per_source_cap,
            inner: Mutex::new(Inner {
                map: HashMap::new(),
                per_source: HashMap::new(),
                warned_half: false,
                since_sweep: 0,
                last_sweep: None,
            }),
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Issue 32 random bytes for `(model_id, pk_att)` from `source`. Sweeps expired
    /// entries first; refuses at either cap (never evicts).
    pub fn issue(
        &self,
        model_id: [u8; 32],
        pk_att: [u8; 33],
        source: &str,
        now: Instant,
    ) -> Result<[u8; 32], NonceError> {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        g.since_sweep += 1;
        let global_full = g.map.len() >= self.cap;
        let source_full = g.per_source.get(source).copied().unwrap_or(0) >= self.per_source_cap;
        // A refusal sweeps first so expired entries never refuse a live node, but a
        // source parked at its cap must not force a full walk on every challenge:
        // at most one sweep per second for the global cap, per 100 ms for a
        // per-source cap (a crash-looping node retries far slower than that).
        let since = g.last_sweep.map(|t| now.saturating_duration_since(t));
        let recently_global = since.map(|d| d < Duration::from_secs(1)).unwrap_or(false);
        let recently_source = since
            .map(|d| d < Duration::from_millis(100))
            .unwrap_or(false);
        let would_refuse = (global_full && !recently_global) || (source_full && !recently_source);
        if would_refuse || g.since_sweep >= SWEEP_EVERY {
            Self::sweep(&mut g, self.ttl, self.cap, now);
            g.since_sweep = 0;
            g.last_sweep = Some(now);
        }
        if g.map.len() >= self.cap {
            return Err(NonceError::TooManyOutstanding(CapScope::Global));
        }
        let per = g.per_source.get(source).copied().unwrap_or(0);
        if per >= self.per_source_cap {
            return Err(NonceError::TooManyOutstanding(CapScope::Source));
        }
        let mut nonce = [0u8; 32];
        loop {
            OsRng.fill_bytes(&mut nonce);
            if !g.map.contains_key(&nonce) {
                break;
            }
        }
        g.map.insert(
            nonce,
            Issued {
                model_id,
                pk_att,
                issued_at: now,
                source: source.to_string(),
            },
        );
        *g.per_source.entry(source.to_string()).or_insert(0) += 1;
        if !g.warned_half && g.map.len() * 2 >= self.cap {
            g.warned_half = true;
            tracing::error!(
                outstanding = g.map.len(),
                cap = self.cap,
                "CRITICAL: nonce store at 50% occupancy"
            );
        }
        Ok(nonce)
    }

    /// Burn: remove and return. `Unknown` for never-issued or already-burned,
    /// `Expired` for a known entry past its TTL (also removed).
    pub fn take(&self, nonce: &[u8; 32], now: Instant) -> Result<Issued, NonceError> {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let issued = g.map.remove(nonce).ok_or(NonceError::Unknown)?;
        Self::decrement(&mut g, &issued.source);
        if now.saturating_duration_since(issued.issued_at) > self.ttl {
            return Err(NonceError::Expired);
        }
        Ok(issued)
    }

    /// Outstanding (unexpired at the last sweep) entries.
    pub fn outstanding(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .map
            .len()
    }

    fn sweep(g: &mut Inner, ttl: Duration, cap: usize, now: Instant) {
        let expired: Vec<[u8; 32]> = g
            .map
            .iter()
            .filter(|(_, v)| now.saturating_duration_since(v.issued_at) > ttl)
            .map(|(k, _)| *k)
            .collect();
        for k in expired {
            if let Some(v) = g.map.remove(&k) {
                Self::decrement(g, &v.source);
            }
        }
        if g.warned_half && g.map.len() * 4 < cap {
            // Occupancy fell below a quarter: allow the CRITICAL line again later.
            g.warned_half = false;
        }
    }

    fn decrement(g: &mut Inner, source: &str) {
        if let Some(n) = g.per_source.get_mut(source) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                g.per_source.remove(source);
            }
        }
    }
}
