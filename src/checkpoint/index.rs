// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Checkpoint Index data structures
//!
//! The index lists all checkpoints for a session, stored at:
//! `home/checkpoints/{hostAddress}/{sessionId}/index.json`

use crate::checkpoint::delta::sort_json_keys;
use serde::{Deserialize, Serialize};

/// Checkpoint index listing all checkpoints for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointIndex {
    /// Session ID
    pub session_id: String,

    /// Host's Ethereum address (lowercase)
    pub host_address: String,

    /// List of checkpoint entries
    pub checkpoints: Vec<CheckpointEntry>,

    /// EIP-191 signature of checkpoints array
    pub host_signature: String,
}

/// A single checkpoint entry in the index
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointEntry {
    /// 0-based checkpoint index
    pub index: u32,

    /// bytes32 proof hash (matches on-chain)
    pub proof_hash: String,

    /// S5 CID where delta is stored (raw CID, no s5:// prefix)
    pub delta_cid: String,

    /// [startToken, endToken] tuple
    pub token_range: [u64; 2],

    /// Unix timestamp in milliseconds
    pub timestamp: u64,
}

/// Session state for cleanup policy
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Active,
    Completed,
    TimedOut,
    Cancelled,
}

impl CheckpointIndex {
    /// Generate S5 path for checkpoint index
    /// Format: home/checkpoints/{hostAddress}/{sessionId}/index.json
    /// Note: Host address is kept lowercase with 0x prefix for consistency
    pub fn s5_path(host_address: &str, session_id: &str) -> String {
        format!(
            "home/checkpoints/{}/{}/index.json",
            host_address.to_lowercase(),
            session_id
        )
    }

    /// Create JSON string of checkpoints array for signing
    /// CRITICAL: Uses alphabetically sorted keys for SDK compatibility
    pub fn compute_checkpoints_json(&self) -> String {
        let value = serde_json::to_value(&self.checkpoints).unwrap();
        let sorted = sort_json_keys(&value);
        serde_json::to_string(&sorted).unwrap() // Compact, no spaces
    }

    /// Convert index to JSON bytes for S5 upload
    pub fn to_json_bytes(&self) -> Vec<u8> {
        let value = serde_json::to_value(self).unwrap();
        let sorted = sort_json_keys(&value);
        serde_json::to_vec_pretty(&sorted).unwrap()
    }

    /// Create empty index for new session
    pub fn new(session_id: String, host_address: String) -> Self {
        Self {
            session_id,
            host_address: host_address.to_lowercase(),
            checkpoints: Vec::new(),
            host_signature: String::new(),
        }
    }

    /// Add a checkpoint entry
    pub fn add_checkpoint(&mut self, entry: CheckpointEntry) {
        self.checkpoints.push(entry);
    }

    /// Get the last checkpoint entry
    pub fn last_checkpoint(&self) -> Option<&CheckpointEntry> {
        self.checkpoints.last()
    }

    /// Get next checkpoint index
    pub fn next_checkpoint_index(&self) -> u32 {
        self.checkpoints.len() as u32
    }
}

impl CheckpointEntry {
    /// Create a new checkpoint entry
    pub fn new(
        index: u32,
        proof_hash: String,
        delta_cid: String,
        start_token: u64,
        end_token: u64,
    ) -> Self {
        Self {
            index,
            proof_hash,
            delta_cid,
            token_range: [start_token, end_token],
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
        }
    }

    /// Create with explicit timestamp (for testing)
    pub fn with_timestamp(
        index: u32,
        proof_hash: String,
        delta_cid: String,
        start_token: u64,
        end_token: u64,
        timestamp: u64,
    ) -> Self {
        Self {
            index,
            proof_hash,
            delta_cid,
            token_range: [start_token, end_token],
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_index_new() {
        let index = CheckpointIndex::new(
            "session-123".to_string(),
            "0xABC123DEF456".to_string(),
        );
        assert_eq!(index.session_id, "session-123");
        assert_eq!(index.host_address, "0xabc123def456"); // lowercase
        assert!(index.checkpoints.is_empty());
        assert!(index.host_signature.is_empty());
    }

    #[test]
    fn test_s5_path_lowercase_address() {
        let path = CheckpointIndex::s5_path("0xABC123DEF", "session-1");
        assert!(path.contains("0xabc123def"));
        assert!(!path.contains("ABC"));
    }

    #[test]
    fn test_s5_path_format() {
        let path = CheckpointIndex::s5_path("0xabc123", "123");
        assert_eq!(path, "home/checkpoints/0xabc123/123/index.json");
    }

    #[test]
    fn test_s5_path_preserves_0x_prefix() {
        // Host addresses keep 0x prefix for consistency with delta paths
        let path = CheckpointIndex::s5_path("0xDEADBEEF", "session");
        assert!(path.contains("0x"), "Path should preserve 0x prefix");
        assert!(path.contains("0xdeadbeef"), "Path should be lowercase with 0x");
    }

    #[test]
    fn test_checkpoint_entry_new() {
        let entry = CheckpointEntry::new(
            0,
            "0x1234".to_string(),
            "bafybeig123".to_string(),
            0,
            1000,
        );
        assert_eq!(entry.index, 0);
        assert_eq!(entry.proof_hash, "0x1234");
        assert_eq!(entry.delta_cid, "bafybeig123");
        assert_eq!(entry.token_range, [0, 1000]);
        assert!(entry.timestamp > 0);
    }

    #[test]
    fn test_checkpoint_index_add_checkpoint() {
        let mut index = CheckpointIndex::new("session".to_string(), "0xhost".to_string());

        let entry = CheckpointEntry::with_timestamp(
            0,
            "0x1234".to_string(),
            "bafybeig123".to_string(),
            0,
            1000,
            1704844800000,
        );
        index.add_checkpoint(entry);

        assert_eq!(index.checkpoints.len(), 1);
        assert_eq!(index.next_checkpoint_index(), 1);
    }

    #[test]
    fn test_checkpoint_index_last_checkpoint() {
        let mut index = CheckpointIndex::new("session".to_string(), "0xhost".to_string());
        assert!(index.last_checkpoint().is_none());

        index.add_checkpoint(CheckpointEntry::with_timestamp(
            0, "0x1".to_string(), "cid1".to_string(), 0, 1000, 100,
        ));
        index.add_checkpoint(CheckpointEntry::with_timestamp(
            1, "0x2".to_string(), "cid2".to_string(), 1000, 2000, 200,
        ));

        let last = index.last_checkpoint().unwrap();
        assert_eq!(last.index, 1);
        assert_eq!(last.proof_hash, "0x2");
    }

    #[test]
    fn test_checkpoint_index_serialization_camel_case() {
        let index = CheckpointIndex::new("session".to_string(), "0xhost".to_string());
        let json = serde_json::to_string(&index).unwrap();

        assert!(json.contains("sessionId"));
        assert!(json.contains("hostAddress"));
        assert!(json.contains("hostSignature"));
    }

    #[test]
    fn test_checkpoint_entry_serialization_camel_case() {
        let entry = CheckpointEntry::with_timestamp(
            0,
            "0x1234".to_string(),
            "bafybeig123".to_string(),
            0,
            1000,
            1704844800000,
        );
        let json = serde_json::to_string(&entry).unwrap();

        assert!(json.contains("proofHash"));
        assert!(json.contains("deltaCid"));
        assert!(json.contains("tokenRange"));
    }

    #[test]
    fn test_session_state_serialization() {
        assert_eq!(
            serde_json::to_string(&SessionState::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&SessionState::Completed).unwrap(),
            "\"completed\""
        );
        assert_eq!(
            serde_json::to_string(&SessionState::TimedOut).unwrap(),
            "\"timed_out\""
        );
        assert_eq!(
            serde_json::to_string(&SessionState::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }

    #[test]
    fn test_compute_checkpoints_json_sorted_keys() {
        let mut index = CheckpointIndex::new("session".to_string(), "0xhost".to_string());
        index.add_checkpoint(CheckpointEntry::with_timestamp(
            0,
            "0x1234".to_string(),
            "bafybeig123".to_string(),
            0,
            1000,
            1704844800000,
        ));

        let json = index.compute_checkpoints_json();

        // Keys should be alphabetically sorted: deltaCid, index, proofHash, timestamp, tokenRange
        let delta_cid_pos = json.find("deltaCid").unwrap();
        let index_pos = json.find("index").unwrap();
        let proof_hash_pos = json.find("proofHash").unwrap();

        assert!(delta_cid_pos < index_pos);
        assert!(index_pos < proof_hash_pos);
    }
}
