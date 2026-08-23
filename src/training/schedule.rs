// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The pinned slice schedule (interface B.1; FROZEN — the interface's Status
//! line is the version authority).
//!
//! Billing never measures GPU work: `trainingTokens = declaredTokens × epochs`
//! and the slice deltas are a pure function of the job, precomputable by the
//! client. Floor rule: the LAST slice absorbs the remainder, so every delta is
//! `>= min(totalTokens, sliceTokens) >= minTotalTokens (10,000)` — above both
//! chain floors (`proofInterval` 1000, `MIN_PROVEN_TOKENS` 100) even when
//! earlier slices forfeit and a later slice becomes on-chain proof 0.
//!
//! Misuse resistance: T3's accept path validates the A.4 bounds BEFORE calling
//! in here, but these functions do not rely on that ordering — out-of-range
//! inputs return `Err`/`None`, never panic and never allocate unboundedly
//! (client-reachable panics have stranded escrow before; see the moderation
//! Slice A record).

/// Hard sanity ceiling for schedule arithmetic — far above the frozen
/// `maxTotalTokens` bound (15,000,000) and far below any allocation hazard.
pub const MAX_SCHEDULABLE_TOKENS: u64 = 100_000_000;

/// `trainingTokens(job) = declaredTokens × epochs` — the ONLY billing inputs.
pub fn training_tokens(declared_tokens: u64, epochs: u32) -> Option<u64> {
    declared_tokens.checked_mul(u64::from(epochs))
}

/// The pinned deltas: `slices = max(1, floor(total / sliceTokens))`; every
/// non-final delta is exactly `sliceTokens`; the final delta absorbs the
/// remainder (`< 2 × sliceTokens`).
pub fn slice_deltas(total_tokens: u64, slice_tokens: u64) -> Result<Vec<u64>, String> {
    if total_tokens == 0 || slice_tokens == 0 {
        return Err(format!(
            "schedule inputs must be positive (total {total_tokens}, sliceTokens {slice_tokens})"
        ));
    }
    if total_tokens > MAX_SCHEDULABLE_TOKENS {
        return Err(format!(
            "totalTokens {total_tokens} exceeds MAX_SCHEDULABLE_TOKENS {MAX_SCHEDULABLE_TOKENS}"
        ));
    }
    let slices = std::cmp::max(1, total_tokens / slice_tokens);
    let mut deltas = Vec::with_capacity(slices as usize);
    for _ in 0..slices - 1 {
        deltas.push(slice_tokens);
    }
    deltas.push(total_tokens - (slices - 1) * slice_tokens);
    Ok(deltas)
}
