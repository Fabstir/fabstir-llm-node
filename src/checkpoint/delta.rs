// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Checkpoint Delta data structures
//!
//! A delta contains messages added since the last checkpoint.
//! Used for SDK conversation recovery.

use serde::{Deserialize, Serialize};

/// A checkpoint delta containing messages since the last checkpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointDelta {
    /// Session ID (matches on-chain session)
    pub session_id: String,

    /// 0-based index of this checkpoint
    pub checkpoint_index: u32,

    /// bytes32 hash of proof data (matches on-chain)
    pub proof_hash: String,

    /// Token count at start of this delta
    pub start_token: u64,

    /// Token count at end of this delta
    pub end_token: u64,

    /// Messages added since last checkpoint
    pub messages: Vec<CheckpointMessage>,

    /// EIP-191 signature of messages array
    pub host_signature: String,
}

/// A conversation message in a checkpoint delta
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointMessage {
    /// "user" or "assistant"
    pub role: String,

    /// Message content
    pub content: String,

    /// Unix timestamp in milliseconds
    pub timestamp: u64,

    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MessageMetadata>,
}

/// Optional metadata for checkpoint messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// True if message continues in next delta (streaming)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial: Option<bool>,
}

impl CheckpointDelta {
    /// Create JSON string of messages array for signing
    /// CRITICAL: Uses alphabetically sorted keys for SDK compatibility
    pub fn compute_messages_json(&self) -> String {
        // Must sort keys alphabetically for SDK signature verification
        let value = serde_json::to_value(&self.messages).unwrap();
        let sorted = sort_json_keys(&value);
        serde_json::to_string(&sorted).unwrap() // Compact, no spaces
    }

    /// Convert delta to JSON bytes for S5 upload
    pub fn to_json_bytes(&self) -> Vec<u8> {
        // Also sort keys for consistency
        let value = serde_json::to_value(self).unwrap();
        let sorted = sort_json_keys(&value);
        serde_json::to_vec_pretty(&sorted).unwrap()
    }
}

impl CheckpointMessage {
    /// Create a user message
    pub fn new_user(content: String, timestamp: u64) -> Self {
        Self {
            role: "user".to_string(),
            content,
            timestamp,
            metadata: None,
        }
    }

    /// Create an assistant message
    pub fn new_assistant(content: String, timestamp: u64, partial: bool) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            timestamp,
            metadata: if partial {
                Some(MessageMetadata {
                    partial: Some(true),
                })
            } else {
                None
            },
        }
    }
}

/// Recursively sort JSON object keys alphabetically
/// Required for SDK signature verification compatibility
pub fn sort_json_keys(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut sorted: serde_json::Map<String, Value> = serde_json::Map::new();
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort(); // Alphabetical sort
            for key in keys {
                sorted.insert(key.clone(), sort_json_keys(&map[key]));
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(sort_json_keys).collect()),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_message_new_user() {
        let msg = CheckpointMessage::new_user("Hello".to_string(), 123456789);
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");
        assert_eq!(msg.timestamp, 123456789);
        assert!(msg.metadata.is_none());
    }

    #[test]
    fn test_checkpoint_message_new_assistant() {
        let msg = CheckpointMessage::new_assistant("Hi there".to_string(), 123456789, false);
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "Hi there");
        assert!(msg.metadata.is_none());
    }

    #[test]
    fn test_checkpoint_message_new_assistant_partial() {
        let msg = CheckpointMessage::new_assistant("Partial response...".to_string(), 123456789, true);
        assert_eq!(msg.role, "assistant");
        assert!(msg.metadata.is_some());
        assert_eq!(msg.metadata.unwrap().partial, Some(true));
    }

    #[test]
    fn test_sort_json_keys_simple_object() {
        let json = serde_json::json!({
            "zebra": 1,
            "apple": 2,
            "mango": 3
        });
        let sorted = sort_json_keys(&json);
        let json_str = serde_json::to_string(&sorted).unwrap();
        // Keys should be in alphabetical order: apple, mango, zebra
        assert!(json_str.find("apple").unwrap() < json_str.find("mango").unwrap());
        assert!(json_str.find("mango").unwrap() < json_str.find("zebra").unwrap());
    }

    #[test]
    fn test_sort_json_keys_nested_object() {
        let json = serde_json::json!({
            "outer": {
                "zebra": 1,
                "apple": 2
            }
        });
        let sorted = sort_json_keys(&json);
        let json_str = serde_json::to_string(&sorted).unwrap();
        // Nested keys should also be sorted
        assert!(json_str.find("apple").unwrap() < json_str.find("zebra").unwrap());
    }

    #[test]
    fn test_checkpoint_message_json_sorted_keys() {
        // CRITICAL: Verify keys are alphabetically sorted for SDK
        let msg = CheckpointMessage::new_user("Hello".to_string(), 123);
        let value = serde_json::to_value(&msg).unwrap();
        let sorted = sort_json_keys(&value);
        let json = serde_json::to_string(&sorted).unwrap();

        // Keys must be: content, role, timestamp (alphabetical)
        let content_pos = json.find("\"content\"").unwrap();
        let role_pos = json.find("\"role\"").unwrap();
        let timestamp_pos = json.find("\"timestamp\"").unwrap();

        assert!(content_pos < role_pos, "content should come before role");
        assert!(role_pos < timestamp_pos, "role should come before timestamp");
    }

    #[test]
    fn test_checkpoint_delta_serialization_camel_case() {
        let delta = CheckpointDelta {
            session_id: "test-session".to_string(),
            checkpoint_index: 0,
            proof_hash: "0x1234".to_string(),
            start_token: 0,
            end_token: 1000,
            messages: vec![],
            host_signature: "0xsig".to_string(),
        };

        let json = serde_json::to_string(&delta).unwrap();
        assert!(json.contains("sessionId"));
        assert!(json.contains("checkpointIndex"));
        assert!(json.contains("proofHash"));
        assert!(json.contains("startToken"));
        assert!(json.contains("endToken"));
        assert!(json.contains("hostSignature"));
    }

    #[test]
    fn test_checkpoint_message_partial_field_optional() {
        // When partial is false/None, metadata should be omitted
        let msg = CheckpointMessage::new_assistant("Hi".to_string(), 123, false);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("metadata"));
        assert!(!json.contains("partial"));

        // When partial is true, metadata should be present
        let msg_partial = CheckpointMessage::new_assistant("Hi".to_string(), 123, true);
        let json_partial = serde_json::to_string(&msg_partial).unwrap();
        assert!(json_partial.contains("metadata"));
        assert!(json_partial.contains("partial"));
    }
}
