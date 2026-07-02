// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Per-session sliding-window rate limiter for LTX generation requests.
//!
//! Direct reuse: `transcoder::rate_limiter::TranscodingRateLimiter` is a fully
//! generic per-key (`session_id`) sliding window with no transcoder coupling, so
//! it is re-exported as `LtxRateLimiter` rather than copied. Only the LTX env
//! default differs (`LTX_RATE_LIMIT`, default 3 per 5-minute window).

pub use crate::transcoder::rate_limiter::TranscodingRateLimiter as LtxRateLimiter;

/// Default LTX requests per 5-minute window per session (`LTX_RATE_LIMIT`).
pub const DEFAULT_LTX_RATE_LIMIT: usize = 3;

/// Build an `LtxRateLimiter` from `LTX_RATE_LIMIT` (default 3), 5-minute window
/// (the `LtxRateLimiter::new` default window, matching the transcoder).
pub fn ltx_rate_limiter() -> LtxRateLimiter {
    let max = std::env::var("LTX_RATE_LIMIT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_LTX_RATE_LIMIT);
    LtxRateLimiter::new(max)
}
