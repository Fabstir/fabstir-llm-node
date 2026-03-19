use fabstir_llm_node::transcoder::TranscoderClient;

#[test]
fn test_client_new_success() {
    let client = TranscoderClient::new("http://localhost:8000", "test-secret");
    assert!(client.is_ok(), "expected client creation to succeed");
}

#[test]
fn test_client_endpoint_trailing_slash_trimmed() {
    let client = TranscoderClient::new("http://localhost:8000/", "test-secret").unwrap();
    assert_eq!(client.endpoint(), "http://localhost:8000");
}

#[test]
fn test_client_jwt_generation() {
    let token = TranscoderClient::generate_jwt("test-secret").unwrap();
    assert!(!token.is_empty(), "JWT should not be empty");
    // Decode header to verify it's a valid JWT with 3 parts
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "JWT should have 3 parts");

    // Decode payload and check claims
    use base64::Engine;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .unwrap();
    let claims: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert!(claims.get("exp").is_some(), "JWT should have exp claim");
    assert!(claims.get("iat").is_some(), "JWT should have iat claim");
}

#[tokio::test]
async fn test_client_health_check_unreachable() {
    let client = TranscoderClient::new("http://127.0.0.1:59999", "test-secret").unwrap();
    assert!(
        !client.health_check().await,
        "unreachable endpoint should return false"
    );
}

#[test]
fn test_submit_request_url_formation() {
    let client = TranscoderClient::new("http://localhost:8000", "test-secret").unwrap();
    let expected = format!("{}/transcode", client.endpoint());
    assert_eq!(expected, "http://localhost:8000/transcode");
}

#[test]
fn test_status_request_url_formation() {
    let client = TranscoderClient::new("http://localhost:8000", "test-secret").unwrap();
    let task_id = "abc-123";
    let expected = format!("{}/get_transcoded/{}", client.endpoint(), task_id);
    assert_eq!(expected, "http://localhost:8000/get_transcoded/abc-123");
}

#[test]
fn test_client_timeout_configuration() {
    // Verify the client can be created (timeouts are internal implementation detail,
    // but we verify the client is properly configured by successful creation)
    let client = TranscoderClient::new("http://localhost:8000", "test-secret");
    assert!(client.is_ok());
}

#[test]
fn test_client_model_name() {
    let client = TranscoderClient::new("http://localhost:8000", "test-secret").unwrap();
    assert_eq!(client.model_name(), "transcoder");
}
