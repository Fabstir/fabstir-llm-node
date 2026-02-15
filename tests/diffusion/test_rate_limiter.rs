// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for image generation rate limiter (Sub-phase 2.4)

use fabstir_llm_node::diffusion::rate_limiter::ImageGenerationRateLimiter;
use std::time::Duration;

#[test]
fn test_allows_requests_within_limit() {
    let limiter = ImageGenerationRateLimiter::new(5);
    let session = "session-1";

    for i in 0..5 {
        assert!(
            limiter.check_rate_limit(session),
            "Request {} should be allowed",
            i + 1
        );
        limiter.record_request(session);
    }
}

#[test]
fn test_rejects_request_when_limit_exceeded() {
    let limiter = ImageGenerationRateLimiter::new(3);
    let session = "session-2";

    for _ in 0..3 {
        assert!(limiter.check_rate_limit(session));
        limiter.record_request(session);
    }

    // 4th request should be rejected
    assert!(
        !limiter.check_rate_limit(session),
        "Should reject when limit exceeded"
    );
}

#[test]
fn test_different_sessions_independent_limits() {
    let limiter = ImageGenerationRateLimiter::new(2);

    // Fill up session-a
    limiter.record_request("session-a");
    limiter.record_request("session-a");
    assert!(!limiter.check_rate_limit("session-a"));

    // session-b should still be allowed
    assert!(limiter.check_rate_limit("session-b"));
    limiter.record_request("session-b");
    assert!(limiter.check_rate_limit("session-b"));
}

#[test]
fn test_window_slides_old_requests_expire() {
    // Use a custom window of 1 second for test speed
    let limiter = ImageGenerationRateLimiter::with_window(2, Duration::from_millis(100));

    limiter.record_request("session-x");
    limiter.record_request("session-x");
    assert!(!limiter.check_rate_limit("session-x"));

    // Wait for the window to expire
    std::thread::sleep(Duration::from_millis(150));

    // Old requests should have expired
    assert!(
        limiter.check_rate_limit("session-x"),
        "Should allow after window expires"
    );
}
