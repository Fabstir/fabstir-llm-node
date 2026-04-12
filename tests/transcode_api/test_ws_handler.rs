use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::api::websocket::handlers::transcode::handle_encrypted_transcode;
use serde_json::json;

fn test_session_key() -> [u8; 32] {
    [0xAA; 32]
}

/// Helper: decrypt the inner payload from an encrypted_response envelope.
fn decrypt_inner(envelope: &serde_json::Value, session_key: &[u8; 32]) -> serde_json::Value {
    let payload = &envelope["payload"];
    let ciphertext = hex::decode(payload["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce_bytes = hex::decode(payload["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(payload["aadHex"].as_str().unwrap()).unwrap();
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&nonce_bytes);
    let plaintext =
        fabstir_llm_node::crypto::decrypt_with_aead(&ciphertext, &nonce, &aad, session_key)
            .unwrap();
    serde_json::from_slice(&plaintext).unwrap()
}

#[tokio::test]
async fn test_transcode_request_validation_missing_cid() {
    let server = ApiServer::new_for_test();
    let key = test_session_key();
    let request = json!({
        "action": "transcode",
        "mediaFormats": [{"id": 1, "ext": "mp4"}]
    });
    let (ack, task) =
        handle_encrypted_transcode(&server, &request, &key, "sess-1", None, None).await;
    assert!(task.is_none(), "no task on validation error");
    let inner = decrypt_inner(&ack, &key);
    assert_eq!(inner["type"], "transcode_error");
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
}

#[tokio::test]
async fn test_transcode_request_validation_empty_formats() {
    let server = ApiServer::new_for_test();
    let key = test_session_key();
    let request = json!({
        "action": "transcode",
        "sourceCid": "uEiBkTest",
        "mediaFormats": []
    });
    let (ack, task) =
        handle_encrypted_transcode(&server, &request, &key, "sess-1", None, None).await;
    assert!(task.is_none(), "no task on validation error");
    let inner = decrypt_inner(&ack, &key);
    assert_eq!(inner["type"], "transcode_error");
    assert!(inner["error"]["message"]
        .as_str()
        .unwrap()
        .contains("empty"));
}

#[tokio::test]
async fn test_transcode_accepted_response_format() {
    // Without a transcoder configured, we expect SIDECAR_UNAVAILABLE
    let server = ApiServer::new_for_test();
    let key = test_session_key();
    let request = json!({
        "action": "transcode",
        "sourceCid": "uEiBkTest",
        "mediaFormats": [{"id": 1, "ext": "mp4", "vcodec": "h264_nvenc"}],
        "isGpu": true,
        "isEncrypted": false
    });
    let (ack, task) =
        handle_encrypted_transcode(&server, &request, &key, "sess-1", None, None).await;
    assert!(task.is_none(), "no task when sidecar unavailable");
    let inner = decrypt_inner(&ack, &key);
    assert_eq!(inner["type"], "transcode_error");
    assert_eq!(inner["error"]["code"], "SIDECAR_UNAVAILABLE");
}

#[test]
fn test_transcode_progress_message_format() {
    let msg = json!({
        "type": "transcode_progress",
        "taskId": "task-123",
        "progress": 45
    });
    assert_eq!(msg["type"], "transcode_progress");
    assert_eq!(msg["taskId"], "task-123");
    assert_eq!(msg["progress"], 45);
}

#[test]
fn test_transcode_complete_message_format() {
    let msg = json!({
        "type": "transcode_complete",
        "taskId": "task-123",
        "outputs": [{"format_index": 0, "cid": "uEiBk...", "dest": "s5"}],
        "billing": {"units": 120.0, "tokens": 120000},
        "duration": 120.5
    });
    assert_eq!(msg["type"], "transcode_complete");
    assert!(msg["outputs"].is_array());
    assert!(msg["billing"].is_object());
    assert_eq!(msg["duration"], 120.5);
}

#[test]
fn test_transcode_error_message_format() {
    let msg = json!({
        "type": "transcode_error",
        "error": {
            "code": "SIDECAR_UNAVAILABLE",
            "message": "Transcoder sidecar not configured"
        }
    });
    assert_eq!(msg["type"], "transcode_error");
    assert_eq!(msg["error"]["code"], "SIDECAR_UNAVAILABLE");
    assert!(msg["error"]["message"].is_string());
}

#[tokio::test]
async fn test_transcode_request_with_preview_percent() {
    let server = ApiServer::new_for_test();
    let key = test_session_key();
    let request = json!({
        "action": "transcode",
        "sourceCid": "uEiBkTestHls",
        "mediaFormats": [{"id": 1, "ext": "mp4", "vcodec": "av1_nvenc", "hls": true, "hls_time": 6}],
        "isGpu": true,
        "isEncrypted": true,
        "previewPercent": 15
    });
    let (ack, task) =
        handle_encrypted_transcode(&server, &request, &key, "sess-hls", None, None).await;
    assert!(task.is_none(), "no task when sidecar unavailable");
    let inner = decrypt_inner(&ack, &key);
    assert_eq!(inner["type"], "transcode_error");
    assert_eq!(inner["error"]["code"], "SIDECAR_UNAVAILABLE");
}

#[test]
fn test_transcode_complete_hls_message_format() {
    let msg = json!({
        "type": "transcode_complete",
        "taskId": "task-hls-456",
        "outputs": [{
            "id": 1,
            "hls": true,
            "initSegmentCid": "zInitSeg1080p...",
            "segments": [
                {"index": 0, "cid": "zSeg0Plain...", "duration": 6.006, "encrypted": false},
                {"index": 1, "cid": "zSeg1Plain...", "duration": 6.006, "encrypted": false},
                {"index": 15, "cid": "uSeg15Enc...", "duration": 6.006, "encrypted": true}
            ],
            "previewSegments": 15,
            "totalSegments": 100,
            "totalDuration": 598.764
        }],
        "billing": {"units": 120.0, "tokens": 120000},
        "duration": 598.764
    });
    assert_eq!(msg["type"], "transcode_complete");
    let output = &msg["outputs"][0];
    assert_eq!(output["hls"], true);
    assert!(output["initSegmentCid"].is_string());
    assert!(output["segments"].is_array());
    assert_eq!(output["segments"].as_array().unwrap().len(), 3);
    assert_eq!(output["previewSegments"], 15);
    assert_eq!(output["totalSegments"], 100);
    assert_eq!(output["totalDuration"], 598.764);
}
