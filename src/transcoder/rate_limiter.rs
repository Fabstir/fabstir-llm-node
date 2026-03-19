// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Per-session rate limiter for transcoding requests (sliding window).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Per-session sliding-window rate limiter for transcoding.
pub struct TranscodingRateLimiter {
    sessions: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
    max_per_window: usize,
    window: Duration,
}

impl TranscodingRateLimiter {
    /// Create with a default 5-minute window (transcoding is expensive).
    pub fn new(max_per_window: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            max_per_window,
            window: Duration::from_secs(300),
        }
    }

    /// Create with a custom window duration (for testing).
    pub fn with_window(max_per_window: usize, window: Duration) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            max_per_window,
            window,
        }
    }

    /// Check whether a session is within its rate limit (does NOT record).
    pub fn check_rate_limit(&self, session_id: &str) -> bool {
        let now = Instant::now();
        let sessions = self.sessions.read().unwrap();
        match sessions.get(session_id) {
            None => true,
            Some(timestamps) => {
                let recent = timestamps
                    .iter()
                    .filter(|&&t| now.duration_since(t) < self.window)
                    .count();
                recent < self.max_per_window
            }
        }
    }

    /// Record a request for the given session.
    pub fn record_request(&self, session_id: &str) {
        let mut sessions = self.sessions.write().unwrap();
        let timestamps = sessions.entry(session_id.to_string()).or_default();
        let now = Instant::now();
        timestamps.retain(|&t| now.duration_since(t) < self.window);
        timestamps.push(now);
    }
}
