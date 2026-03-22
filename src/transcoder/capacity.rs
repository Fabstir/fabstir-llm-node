// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Lock-free transcode capacity tracking with RAII slot guard.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Try to acquire a transcode slot. Returns `true` if acquired.
/// Uses a CAS loop to prevent TOCTOU races.
pub fn try_acquire(counter: &AtomicUsize, max: usize) -> bool {
    loop {
        let current = counter.load(Ordering::Acquire);
        if current >= max {
            return false;
        }
        if counter
            .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return true;
        }
    }
}

/// Release a transcode slot, guarding against underflow.
pub fn release(counter: &AtomicUsize) {
    let _ = counter.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
        Some(current.saturating_sub(1))
    });
}

/// RAII guard that releases a transcode slot on drop.
pub struct TranscodeSlotGuard {
    counter: Arc<AtomicUsize>,
}

impl TranscodeSlotGuard {
    pub fn new(counter: Arc<AtomicUsize>) -> Self {
        Self { counter }
    }
}

impl Drop for TranscodeSlotGuard {
    fn drop(&mut self) {
        release(&self.counter);
    }
}
