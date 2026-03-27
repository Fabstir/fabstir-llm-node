use fabstir_llm_node::transcoder::rate_limiter::TranscodingRateLimiter;
use std::time::Duration;

#[test]
fn test_rate_limiter_allows_under_limit() {
    let limiter = TranscodingRateLimiter::new(3);
    for _ in 0..3 {
        assert!(limiter.check_rate_limit("session-1"));
        limiter.record_request("session-1");
    }
}

#[test]
fn test_rate_limiter_blocks_over_limit() {
    let limiter = TranscodingRateLimiter::new(3);
    for _ in 0..3 {
        assert!(limiter.check_rate_limit("session-1"));
        limiter.record_request("session-1");
    }
    assert!(
        !limiter.check_rate_limit("session-1"),
        "4th request should be blocked"
    );
}

#[test]
fn test_rate_limiter_window_expiry() {
    let limiter = TranscodingRateLimiter::with_window(2, Duration::from_millis(50));
    limiter.record_request("session-1");
    limiter.record_request("session-1");
    assert!(!limiter.check_rate_limit("session-1"), "at limit");
    std::thread::sleep(Duration::from_millis(60));
    assert!(
        limiter.check_rate_limit("session-1"),
        "should be allowed after window expires"
    );
}

#[test]
fn test_rate_limiter_multiple_sessions() {
    let limiter = TranscodingRateLimiter::new(1);
    limiter.record_request("session-a");
    assert!(!limiter.check_rate_limit("session-a"), "session-a at limit");
    assert!(
        limiter.check_rate_limit("session-b"),
        "session-b should be independent"
    );
}
