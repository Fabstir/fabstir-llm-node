use fabstir_llm_node::transcoder::capacity::CachedSidecarStatus;
use fabstir_llm_node::transcoder::client::TranscoderClient;
use fabstir_llm_node::transcoder::types::SidecarStatus;
use std::time::Duration;

#[tokio::test]
async fn test_get_sidecar_status_unreachable() {
    let client = TranscoderClient::new("http://127.0.0.1:1", "test-token").unwrap();
    let result = client.get_sidecar_status().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_cached_status_none_when_unreachable() {
    let cache = CachedSidecarStatus::new(Duration::from_secs(2));
    let client = TranscoderClient::new("http://127.0.0.1:1", "test-token").unwrap();
    let result = cache.get_or_fetch(&client).await;
    assert!(result.is_none());
}

#[test]
fn test_sidecar_status_has_capacity_true() {
    let s = SidecarStatus {
        active_jobs: 1,
        queued_jobs: 0,
        max_concurrent: 3,
    };
    assert!(s.has_capacity());
}

#[test]
fn test_sidecar_status_has_capacity_false_at_max() {
    let s = SidecarStatus {
        active_jobs: 3,
        queued_jobs: 0,
        max_concurrent: 3,
    };
    assert!(!s.has_capacity());
}

#[test]
fn test_sidecar_status_has_capacity_false_over() {
    let s = SidecarStatus {
        active_jobs: 4,
        queued_jobs: 1,
        max_concurrent: 3,
    };
    assert!(!s.has_capacity());
}

#[test]
fn test_sidecar_status_available() {
    let s = SidecarStatus {
        active_jobs: 1,
        queued_jobs: 0,
        max_concurrent: 3,
    };
    assert_eq!(s.available(), 2);
}
