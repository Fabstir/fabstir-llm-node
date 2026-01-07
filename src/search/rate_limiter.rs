// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Rate limiting for search requests

use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter as GovRateLimiter};
use std::num::NonZeroU32;
use std::sync::Arc;

use super::types::SearchError;

/// Rate limiter for search requests
pub struct SearchRateLimiter {
    limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    requests_per_minute: u32,
}

impl SearchRateLimiter {
    /// Create a new rate limiter
    ///
    /// # Arguments
    /// * `requests_per_minute` - Maximum requests allowed per minute
    pub fn new(requests_per_minute: u32) -> Self {
        let rpm = NonZeroU32::new(requests_per_minute).unwrap_or(NonZeroU32::new(60).unwrap());
        let quota = Quota::per_minute(rpm);
        let limiter = Arc::new(GovRateLimiter::direct(quota));

        Self {
            limiter,
            requests_per_minute,
        }
    }

    /// Check if a request is allowed
    ///
    /// Returns Ok(()) if allowed, or SearchError::RateLimited if not
    pub fn check(&self) -> Result<(), SearchError> {
        match self.limiter.check() {
            Ok(_) => Ok(()),
            Err(_) => Err(SearchError::RateLimited {
                retry_after_secs: 60,
            }),
        }
    }

    /// Wait until a request is allowed
    ///
    /// This is an async method that blocks until the rate limit allows a request
    pub async fn wait(&self) {
        self.limiter.until_ready().await;
    }

    /// Get the configured requests per minute
    pub fn requests_per_minute(&self) -> u32 {
        self.requests_per_minute
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_creation() {
        let limiter = SearchRateLimiter::new(60);
        assert_eq!(limiter.requests_per_minute(), 60);
    }

    #[test]
    fn test_rate_limiter_allows_requests() {
        let limiter = SearchRateLimiter::new(100);
        // First request should be allowed
        assert!(limiter.check().is_ok());
    }

    #[test]
    fn test_rate_limiter_zero_becomes_default() {
        // Zero should become a valid NonZeroU32 (60)
        let limiter = SearchRateLimiter::new(0);
        assert!(limiter.check().is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_wait() {
        let limiter = SearchRateLimiter::new(1000);
        // Should not block with high limit
        limiter.wait().await;
    }

    #[test]
    fn test_rate_limiter_burst() {
        // High rate limit to allow multiple requests
        let limiter = SearchRateLimiter::new(1000);

        // Multiple requests should be allowed
        for _ in 0..10 {
            assert!(limiter.check().is_ok());
        }
    }
}
