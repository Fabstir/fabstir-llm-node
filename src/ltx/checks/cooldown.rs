// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Cool-down against free rendering (D13). Refusals are counted per authenticated client address (on the card path
//! the payer is the shared vault, so the payer is the wrong key). From the third `CONTENT_REFUSED` in 24 hours the
//! address's jobs are rejected before accept for 10 minutes after each further refusal. In memory: a restart resets.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const WINDOW: Duration = Duration::from_secs(24 * 3600);
pub const PAUSE: Duration = Duration::from_secs(600);
/// The refusal from which the pause applies (two are free).
pub const THRESHOLD: usize = 3;

#[derive(Default)]
struct Entry {
    refusals: Vec<Instant>,
    paused_until: Option<Instant>,
}

#[derive(Default)]
pub struct Cooldown {
    by_address: Mutex<HashMap<String, Entry>>,
}

/// What recording a refusal did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recorded {
    /// Refusals in the last 24 hours, this one included.
    pub count: usize,
    /// This was exactly the third: alert the named person once (D13).
    pub alert: bool,
}

impl Cooldown {
    /// Whether `address` is paused at `now`. In-memory only (D2: runs before accept).
    pub fn paused_at(&self, address: &str, now: Instant) -> bool {
        let Ok(g) = self.by_address.lock() else {
            return true; // a poisoned counter fails closed
        };
        g.get(&address.to_ascii_lowercase())
            .and_then(|e| e.paused_until)
            .is_some_and(|until| now < until)
    }

    pub fn paused(&self, address: &str) -> bool {
        self.paused_at(address, Instant::now())
    }

    /// Count one `CONTENT_REFUSED` for `address` at `now`.
    pub fn record_at(&self, address: &str, now: Instant) -> Recorded {
        let Ok(mut g) = self.by_address.lock() else {
            return Recorded {
                count: THRESHOLD,
                alert: false,
            };
        };
        // forget addresses with nothing inside the window, so the map cannot grow without bound
        g.retain(|_, e| {
            e.refusals
                .last()
                .is_some_and(|t| now.saturating_duration_since(*t) < WINDOW)
        });
        let e = g.entry(address.to_ascii_lowercase()).or_default();
        e.refusals
            .retain(|t| now.saturating_duration_since(*t) < WINDOW);
        e.refusals.push(now);
        let count = e.refusals.len();
        if count >= THRESHOLD {
            e.paused_until = Some(now + PAUSE);
        }
        Recorded {
            count,
            alert: count == THRESHOLD,
        }
    }

    pub fn record(&self, address: &str) -> Recorded {
        self.record_at(address, Instant::now())
    }
}
