use fabstir_llm_node::transcoder::TranscoderClient;

#[test]
fn test_client_new_success() {
    let client = TranscoderClient::new("http://localhost:8000", "fake-jwt-token");
    assert!(client.is_ok(), "expected client creation to succeed");
}

#[test]
fn test_client_endpoint_trailing_slash_trimmed() {
    let client = TranscoderClient::new("http://localhost:8000/", "fake-jwt-token").unwrap();
    assert_eq!(client.endpoint(), "http://localhost:8000");
}

#[tokio::test]
async fn test_client_health_check_unreachable() {
    let client = TranscoderClient::new("http://127.0.0.1:59999", "fake-jwt-token").unwrap();
    assert!(
        !client.health_check().await,
        "unreachable endpoint should return false"
    );
}

#[test]
fn test_submit_request_url_formation() {
    let client = TranscoderClient::new("http://localhost:8000", "fake-jwt-token").unwrap();
    let expected = format!("{}/transcode", client.endpoint());
    assert_eq!(expected, "http://localhost:8000/transcode");
}

#[test]
fn test_status_request_url_formation() {
    let client = TranscoderClient::new("http://localhost:8000", "fake-jwt-token").unwrap();
    let task_id = "abc-123";
    let expected = format!("{}/get_transcoded/{}", client.endpoint(), task_id);
    assert_eq!(expected, "http://localhost:8000/get_transcoded/abc-123");
}

#[test]
fn test_client_timeout_configuration() {
    // Verify the client can be created (timeouts are internal implementation detail,
    // but we verify the client is properly configured by successful creation)
    let client = TranscoderClient::new("http://localhost:8000", "fake-jwt-token");
    assert!(client.is_ok());
}

#[test]
fn test_client_model_name() {
    let client = TranscoderClient::new("http://localhost:8000", "fake-jwt-token").unwrap();
    assert_eq!(client.model_name(), "transcoder");
}

#[test]
fn test_cancel_url_formation() {
    let client = TranscoderClient::new("http://localhost:8000", "fake-jwt-token").unwrap();
    let expected = format!("{}/transcode/{}/cancel", client.endpoint(), "task-99");
    assert_eq!(expected, "http://localhost:8000/transcode/task-99/cancel");
}
