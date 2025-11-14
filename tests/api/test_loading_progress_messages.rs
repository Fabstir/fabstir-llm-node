// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for vector loading progress message types (Sub-phase 7.1)
//!
//! Tests serialization, validation, and backward compatibility of
//! LoadingProgressMessage enum and related types.

use fabstir_llm_node::api::websocket::message_types::{
    LoadingProgressMessage, MessageType, WebSocketMessage,
};
use serde_json::json;

// ============================================================================
// Test: ManifestDownloaded Event Serialization
// ============================================================================

#[test]
fn test_manifest_downloaded_serialization() {
    let progress = LoadingProgressMessage::ManifestDownloaded;

    // Serialize to JSON
    let json = serde_json::to_value(&progress).expect("Failed to serialize");

    // Verify structure
    assert_eq!(json["event"], "manifest_downloaded");
    assert_eq!(json["message"], "Manifest downloaded, loading chunks...");
}

#[test]
fn test_manifest_downloaded_deserialization() {
    let json = json!({
        "event": "manifest_downloaded",
        "message": "Manifest downloaded, loading chunks..."
    });

    let progress: LoadingProgressMessage =
        serde_json::from_value(json).expect("Failed to deserialize");

    match progress {
        LoadingProgressMessage::ManifestDownloaded => (),
        _ => panic!("Expected ManifestDownloaded variant"),
    }
}

// ============================================================================
// Test: ChunkDownloaded Event with Progress Tracking
// ============================================================================

#[test]
fn test_chunk_downloaded_serialization() {
    let progress = LoadingProgressMessage::ChunkDownloaded {
        chunk_id: 5,
        total: 10,
    };

    let json = serde_json::to_value(&progress).expect("Failed to serialize");

    assert_eq!(json["event"], "chunk_downloaded");
    assert_eq!(json["chunk_id"], 5);
    assert_eq!(json["total"], 10);
    assert_eq!(json["percent"], 60); // (5+1)/10 * 100 = 60%
    assert_eq!(json["message"], "Downloading chunks... 60% (6/10)");
}

#[test]
fn test_chunk_downloaded_deserialization() {
    let json = json!({
        "event": "chunk_downloaded",
        "chunk_id": 3,
        "total": 8,
        "percent": 50,
        "message": "Downloading chunks... 50% (4/8)"
    });

    let progress: LoadingProgressMessage =
        serde_json::from_value(json).expect("Failed to deserialize");

    match progress {
        LoadingProgressMessage::ChunkDownloaded { chunk_id, total } => {
            assert_eq!(chunk_id, 3);
            assert_eq!(total, 8);
        }
        _ => panic!("Expected ChunkDownloaded variant"),
    }
}

#[test]
fn test_chunk_downloaded_progress_percentage() {
    // Test various progress percentages
    let test_cases = vec![
        (0, 10, 10),   // First chunk: (0+1)/10 = 10%
        (4, 10, 50),   // Middle: (4+1)/10 = 50%
        (9, 10, 100),  // Last chunk: (9+1)/10 = 100%
        (0, 1, 100),   // Single chunk: (0+1)/1 = 100%
    ];

    for (chunk_id, total, expected_percent) in test_cases {
        let progress = LoadingProgressMessage::ChunkDownloaded { chunk_id, total };
        let json = serde_json::to_value(&progress).expect("Failed to serialize");
        assert_eq!(json["percent"], expected_percent,
            "Failed for chunk_id={}, total={}", chunk_id, total);
    }
}

// ============================================================================
// Test: IndexBuilding Event
// ============================================================================

#[test]
fn test_index_building_serialization() {
    let progress = LoadingProgressMessage::IndexBuilding;

    let json = serde_json::to_value(&progress).expect("Failed to serialize");

    assert_eq!(json["event"], "index_building");
    assert_eq!(json["message"], "Building search index...");
}

#[test]
fn test_index_building_deserialization() {
    let json = json!({
        "event": "index_building",
        "message": "Building search index..."
    });

    let progress: LoadingProgressMessage =
        serde_json::from_value(json).expect("Failed to deserialize");

    match progress {
        LoadingProgressMessage::IndexBuilding => (),
        _ => panic!("Expected IndexBuilding variant"),
    }
}

// ============================================================================
// Test: LoadingComplete Event with Metrics
// ============================================================================

#[test]
fn test_loading_complete_serialization() {
    let progress = LoadingProgressMessage::LoadingComplete {
        vector_count: 1500,
        duration_ms: 3250,
    };

    let json = serde_json::to_value(&progress).expect("Failed to serialize");

    assert_eq!(json["event"], "loading_complete");
    assert_eq!(json["vector_count"], 1500);
    assert_eq!(json["duration_ms"], 3250);
    assert_eq!(json["message"], "Vector database ready (1500 vectors, loaded in 3.25s)");
}

#[test]
fn test_loading_complete_deserialization() {
    let json = json!({
        "event": "loading_complete",
        "vector_count": 2000,
        "duration_ms": 5000,
        "message": "Vector database ready (2000 vectors, loaded in 5.00s)"
    });

    let progress: LoadingProgressMessage =
        serde_json::from_value(json).expect("Failed to deserialize");

    match progress {
        LoadingProgressMessage::LoadingComplete { vector_count, duration_ms } => {
            assert_eq!(vector_count, 2000);
            assert_eq!(duration_ms, 5000);
        }
        _ => panic!("Expected LoadingComplete variant"),
    }
}

#[test]
fn test_loading_complete_duration_formatting() {
    let test_cases = vec![
        (500, "0.50s"),    // 500ms
        (1000, "1.00s"),   // 1s
        (1500, "1.50s"),   // 1.5s
        (10000, "10.00s"), // 10s
    ];

    for (duration_ms, expected_duration_str) in test_cases {
        let progress = LoadingProgressMessage::LoadingComplete {
            vector_count: 1000,
            duration_ms,
        };

        let json = serde_json::to_value(&progress).expect("Failed to serialize");
        let message = json["message"].as_str().unwrap();

        assert!(message.contains(expected_duration_str),
            "Expected message to contain '{}', got '{}'", expected_duration_str, message);
    }
}

// ============================================================================
// Test: LoadingError Event
// ============================================================================

#[test]
fn test_loading_error_serialization() {
    let progress = LoadingProgressMessage::LoadingError {
        error: "Failed to download chunk 3: Network timeout".to_string(),
    };

    let json = serde_json::to_value(&progress).expect("Failed to serialize");

    assert_eq!(json["event"], "loading_error");
    assert_eq!(json["error"], "Failed to download chunk 3: Network timeout");
    assert_eq!(json["message"], "Loading failed: Failed to download chunk 3: Network timeout");
}

#[test]
fn test_loading_error_deserialization() {
    let json = json!({
        "event": "loading_error",
        "error": "Decryption failed: Invalid key",
        "message": "Loading failed: Decryption failed: Invalid key"
    });

    let progress: LoadingProgressMessage =
        serde_json::from_value(json).expect("Failed to deserialize");

    match progress {
        LoadingProgressMessage::LoadingError { error } => {
            assert_eq!(error, "Decryption failed: Invalid key");
        }
        _ => panic!("Expected LoadingError variant"),
    }
}

// ============================================================================
// Test: WebSocket Message Integration
// ============================================================================

#[test]
fn test_progress_message_websocket_integration() {
    let progress = LoadingProgressMessage::ChunkDownloaded {
        chunk_id: 2,
        total: 5,
    };

    let ws_message = WebSocketMessage {
        msg_type: MessageType::VectorLoadingProgress,
        session_id: Some("test-session-123".to_string()),
        payload: serde_json::to_value(&progress).expect("Failed to serialize progress"),
    };

    // Serialize complete WebSocket message
    let json = serde_json::to_value(&ws_message).expect("Failed to serialize ws_message");

    // Verify structure
    assert_eq!(json["type"], "vector_loading_progress");
    assert_eq!(json["session_id"], "test-session-123");
    assert_eq!(json["payload"]["event"], "chunk_downloaded");
    assert_eq!(json["payload"]["chunk_id"], 2);
    assert_eq!(json["payload"]["total"], 5);
}

#[test]
fn test_all_progress_events_in_websocket_messages() {
    let session_id = "session-456";

    let events = vec![
        LoadingProgressMessage::ManifestDownloaded,
        LoadingProgressMessage::ChunkDownloaded { chunk_id: 0, total: 3 },
        LoadingProgressMessage::IndexBuilding,
        LoadingProgressMessage::LoadingComplete { vector_count: 500, duration_ms: 1200 },
        LoadingProgressMessage::LoadingError { error: "Test error".to_string() },
    ];

    for progress in events {
        let ws_message = WebSocketMessage {
            msg_type: MessageType::VectorLoadingProgress,
            session_id: Some(session_id.to_string()),
            payload: serde_json::to_value(&progress).expect("Failed to serialize"),
        };

        // Ensure it serializes without errors
        let json_str = serde_json::to_string(&ws_message)
            .expect("Failed to serialize WebSocket message");

        // Ensure session_id is included
        assert!(json_str.contains(session_id));
        assert!(json_str.contains("vector_loading_progress"));
    }
}

// ============================================================================
// Test: Backward Compatibility
// ============================================================================

#[test]
fn test_backward_compatibility_unknown_event() {
    // Client receives unknown event type (from future version)
    let json = json!({
        "event": "future_event_type",
        "some_field": 123
    });

    // Should fail to deserialize gracefully (not panic)
    let result: Result<LoadingProgressMessage, _> = serde_json::from_value(json);

    assert!(result.is_err(), "Should fail to deserialize unknown event");
}

#[test]
fn test_backward_compatibility_missing_optional_fields() {
    // Old client may send minimal ChunkDownloaded without percent/message
    let json = json!({
        "event": "chunk_downloaded",
        "chunk_id": 1,
        "total": 4
    });

    let progress: LoadingProgressMessage =
        serde_json::from_value(json).expect("Should deserialize with minimal fields");

    match progress {
        LoadingProgressMessage::ChunkDownloaded { chunk_id, total } => {
            assert_eq!(chunk_id, 1);
            assert_eq!(total, 4);
        }
        _ => panic!("Expected ChunkDownloaded variant"),
    }
}

#[test]
fn test_session_id_routing() {
    // Ensure messages include session_id for correct client routing
    let progress = LoadingProgressMessage::ManifestDownloaded;

    let ws_message = WebSocketMessage {
        msg_type: MessageType::VectorLoadingProgress,
        session_id: Some("session-routing-test".to_string()),
        payload: serde_json::to_value(&progress).unwrap(),
    };

    assert_eq!(ws_message.session_id, Some("session-routing-test".to_string()));

    // Serialize and verify session_id is present
    let json = serde_json::to_value(&ws_message).unwrap();
    assert_eq!(json["session_id"], "session-routing-test");
}

// ============================================================================
// Test: Error Message User-Friendliness
// ============================================================================

#[test]
fn test_loading_error_user_friendly_messages() {
    let test_cases = vec![
        (
            "Failed to download manifest.json: Network timeout after 30s",
            "Failed to download manifest.json: Network timeout after 30s"
        ),
        (
            "Decryption failed: Invalid session key length",
            "Decryption failed: Invalid session key length"
        ),
        (
            "Loading timed out after 5 minutes",
            "Loading timed out after 5 minutes"
        ),
    ];

    for (error_msg, expected_in_message) in test_cases {
        let progress = LoadingProgressMessage::LoadingError {
            error: error_msg.to_string(),
        };

        let json = serde_json::to_value(&progress).unwrap();
        let message = json["message"].as_str().unwrap();

        assert!(message.contains(expected_in_message),
            "Expected '{}' in message, got '{}'", expected_in_message, message);
    }
}
