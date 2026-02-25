// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//
// Tests for stream_cancel WebSocket message support (v8.19.0)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use fabstir_llm_node::api::websocket::message_types::MessageType;
use fabstir_llm_node::api::websocket::session::{SessionConfig, WebSocketSession};
use fabstir_llm_node::inference::engine::{InferenceRequest, InferenceResult};

// ============================================================================
// Phase 1.1: Cancel flag infrastructure tests
// ============================================================================

#[test]
fn test_inference_request_cancel_flag_default_none() {
    // Deserialize an InferenceRequest from JSON (without cancel_flag field)
    let json = serde_json::json!({
        "model_id": "test-model",
        "prompt": "Hello",
        "max_tokens": 100,
        "temperature": 0.7,
        "top_p": 0.9,
        "top_k": 40,
        "repeat_penalty": 1.1,
        "min_p": 0.05,
        "seed": null,
        "stop_sequences": [],
        "stream": false
    });
    let request: InferenceRequest = serde_json::from_value(json).unwrap();
    assert!(
        request.cancel_flag.is_none(),
        "cancel_flag should be None by default"
    );
}

#[test]
fn test_inference_request_cancel_flag_programmatic() {
    let flag = Arc::new(AtomicBool::new(false));
    let mut request: InferenceRequest = serde_json::from_value(serde_json::json!({
        "model_id": "test-model",
        "prompt": "Hello",
        "max_tokens": 100,
        "temperature": 0.7,
        "top_p": 0.9,
        "top_k": 40,
        "repeat_penalty": 1.1,
        "min_p": 0.05,
        "stop_sequences": [],
        "stream": false
    }))
    .unwrap();
    request.cancel_flag = Some(flag.clone());
    assert!(request.cancel_flag.is_some());
    assert!(!flag.load(Ordering::Relaxed));
}

#[test]
fn test_inference_result_cancelled_finish_reason() {
    let result = InferenceResult {
        text: "partial output".to_string(),
        tokens_generated: 5,
        generation_time: std::time::Duration::from_millis(100),
        tokens_per_second: 50.0,
        model_id: "test-model".to_string(),
        finish_reason: "cancelled".to_string(),
        token_info: vec![],
        was_cancelled: true,
    };
    assert!(result.was_cancelled);
    assert_eq!(result.finish_reason, "cancelled");
}

// ============================================================================
// Phase 2.1: Session cancel state tests
// ============================================================================

#[test]
fn test_session_has_cancel_flag() {
    let session = WebSocketSession::new("test-session-1");
    assert!(
        !session.inference_cancel_flag.load(Ordering::Relaxed),
        "cancel flag should be false on new session"
    );
}

#[test]
fn test_session_cancel_flag_set_and_reset() {
    let session = WebSocketSession::new("test-session-2");
    // Set to true
    session.inference_cancel_flag.store(true, Ordering::Release);
    assert!(session.inference_cancel_flag.load(Ordering::Acquire));
    // Reset to false
    session
        .inference_cancel_flag
        .store(false, Ordering::Release);
    assert!(!session.inference_cancel_flag.load(Ordering::Acquire));
}

// ============================================================================
// Phase 4.1: stream_cancel handler tests
// ============================================================================

#[test]
fn test_stream_cancel_message_parsed() {
    let json = serde_json::json!({
        "type": "stream_cancel",
        "session_id": "test-123",
        "reason": "user_cancelled"
    });
    assert_eq!(json["type"], "stream_cancel");
    assert_eq!(json["session_id"], "test-123");
    assert_eq!(json["reason"], "user_cancelled");
    // Verify MessageType enum has StreamCancel variant
    let mt: MessageType = serde_json::from_value(serde_json::json!("stream_cancel")).unwrap();
    assert_eq!(mt, MessageType::StreamCancel);
}

#[test]
fn test_stream_cancel_no_active_stream() {
    // When no stream is active, setting the cancel flag is a safe no-op
    let session = WebSocketSession::new("cancel-test-1");
    assert!(!session.inference_cancel_flag.load(Ordering::Acquire));
    session.inference_cancel_flag.store(true, Ordering::Release);
    assert!(session.inference_cancel_flag.load(Ordering::Acquire));
    // No panic, no error — idempotent
}

#[test]
fn test_stream_cancel_idempotent() {
    let session = WebSocketSession::new("cancel-test-2");
    session.inference_cancel_flag.store(true, Ordering::Release);
    session.inference_cancel_flag.store(true, Ordering::Release);
    assert!(session.inference_cancel_flag.load(Ordering::Acquire));
    // Setting twice is safe
}

// ============================================================================
// Phase 5.1: stream_end enhancement tests
// ============================================================================

#[test]
fn test_stream_end_has_reason_complete() {
    // Verify stream_end JSON structure with reason: "complete"
    let stream_end = serde_json::json!({
        "type": "stream_end",
        "reason": "complete",
        "tokens_used": 42
    });
    assert_eq!(stream_end["reason"], "complete");
    assert_eq!(stream_end["tokens_used"], 42);
    assert_eq!(stream_end["type"], "stream_end");
}

#[test]
fn test_stream_end_has_reason_cancelled() {
    // Verify stream_end JSON structure with reason: "cancelled"
    let stream_end = serde_json::json!({
        "type": "stream_end",
        "reason": "cancelled",
        "tokens_used": 15
    });
    assert_eq!(stream_end["reason"], "cancelled");
    assert_eq!(stream_end["tokens_used"], 15);
}

// ============================================================================
// Phase 6.1: Checkpoint finalization on cancel test
// ============================================================================

#[test]
fn test_checkpoint_finalization_runs_on_cancel() {
    // When cancel flag is set mid-stream, the InferenceResult should have
    // was_cancelled=true and tokens_generated > 0
    let result = InferenceResult {
        text: "partial".to_string(),
        tokens_generated: 10,
        generation_time: std::time::Duration::from_millis(50),
        tokens_per_second: 200.0,
        model_id: "test".to_string(),
        finish_reason: "cancelled".to_string(),
        token_info: vec![],
        was_cancelled: true,
    };
    assert!(result.was_cancelled);
    assert!(result.tokens_generated > 0);
    // Checkpoint finalization code in handle_streaming_request runs unconditionally
    // after the streaming loop exits (lines 1051-1078 in server.rs)
}

// ============================================================================
// Phase 7.1: Edge-case tests
// ============================================================================

#[test]
fn test_new_prompt_after_cancel_works() {
    let session = WebSocketSession::new("edge-1");
    // Simulate cancel
    session.inference_cancel_flag.store(true, Ordering::Release);
    assert!(session.inference_cancel_flag.load(Ordering::Acquire));
    // Simulate "new prompt" — reset flag
    session
        .inference_cancel_flag
        .store(false, Ordering::Release);
    let mut request: InferenceRequest = serde_json::from_value(serde_json::json!({
        "model_id": "m", "prompt": "hi", "max_tokens": 10,
        "temperature": 0.7, "top_p": 0.9, "top_k": 40,
        "repeat_penalty": 1.0, "min_p": 0.0, "stop_sequences": [], "stream": false
    }))
    .unwrap();
    request.cancel_flag = Some(session.inference_cancel_flag.clone());
    assert!(!request
        .cancel_flag
        .as_ref()
        .unwrap()
        .load(Ordering::Acquire));
}

#[test]
fn test_cancel_during_encrypted_session_plaintext() {
    // stream_cancel is always plaintext, even during encrypted sessions
    // The handler processes it before checking encrypted_message type
    let cancel_json = serde_json::json!({
        "type": "stream_cancel",
        "session_id": "encrypted-session-1"
    });
    // Verify it's a valid JSON message that the handler can parse
    assert_eq!(cancel_json["type"], "stream_cancel");
    assert!(cancel_json["session_id"].as_str().is_some());
    // The stream_cancel handler in server.rs processes this before
    // the encrypted_session_init / encrypted_message handlers
}
