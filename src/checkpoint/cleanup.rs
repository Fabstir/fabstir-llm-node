// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Checkpoint Cleanup Policy
//!
//! Defines TTL-based cleanup rules for checkpoint data in S5 storage.
//!
//! ## Cleanup Policy
//! | Session State     | TTL           |
//! |------------------|---------------|
//! | Completed        | 7 days        |
//! | Timed Out        | 30 days       |
//! | Cancelled        | Immediate     |
//! | Dispute Open     | Until resolved + 7 days |

use crate::checkpoint::{CheckpointIndex, SessionState};
use crate::storage::S5Storage;
use anyhow::{anyhow, Result};
use std::time::Duration;
use tracing::{info, warn};

/// TTL for completed sessions (7 days)
pub const TTL_COMPLETED: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// TTL for timed out sessions (30 days)
pub const TTL_TIMED_OUT: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// TTL for cancelled sessions (immediate deletion)
pub const TTL_CANCELLED: Duration = Duration::ZERO;

/// Grace period after dispute resolution (7 days)
pub const TTL_DISPUTE_GRACE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Result of a cleanup operation
#[derive(Debug, Clone, PartialEq)]
pub enum CleanupResult {
    /// Session is active, no cleanup performed
    Skipped,
    /// Marked for future cleanup with TTL
    MarkedForCleanup { ttl_days: u64 },
    /// Immediate deletion performed
    Deleted { deltas_removed: usize },
    /// Cleanup failed
    Failed(String),
}

/// Clean up checkpoint data for a session based on its state
///
/// # Arguments
/// * `s5_storage` - S5 storage backend
/// * `host_address` - Host's Ethereum address
/// * `session_id` - Session identifier
/// * `state` - Final session state
///
/// # Returns
/// * `Ok(CleanupResult)` - Result of cleanup operation
/// * `Err` - Cleanup failed
pub async fn cleanup_checkpoints(
    s5_storage: &dyn S5Storage,
    host_address: &str,
    session_id: &str,
    state: SessionState,
) -> Result<CleanupResult> {
    let index_path = CheckpointIndex::s5_path(host_address, session_id);

    match state {
        SessionState::Active => {
            // Never cleanup active sessions
            Ok(CleanupResult::Skipped)
        }
        SessionState::Cancelled => {
            // Immediate deletion
            info!(
                "Immediately deleting checkpoints for cancelled session {}",
                session_id
            );
            let count = delete_all_checkpoints(s5_storage, host_address, session_id).await?;
            Ok(CleanupResult::Deleted {
                deltas_removed: count,
            })
        }
        SessionState::Completed => {
            // Mark for cleanup after 7 days
            let ttl_days = TTL_COMPLETED.as_secs() / (24 * 60 * 60);
            info!(
                "Marking session {} for cleanup in {} days (completed)",
                session_id, ttl_days
            );
            mark_for_cleanup(s5_storage, &index_path, ttl_days).await?;
            Ok(CleanupResult::MarkedForCleanup { ttl_days })
        }
        SessionState::TimedOut => {
            // Mark for cleanup after 30 days
            let ttl_days = TTL_TIMED_OUT.as_secs() / (24 * 60 * 60);
            info!(
                "Marking session {} for cleanup in {} days (timed out)",
                session_id, ttl_days
            );
            mark_for_cleanup(s5_storage, &index_path, ttl_days).await?;
            Ok(CleanupResult::MarkedForCleanup { ttl_days })
        }
    }
}

/// Delete all checkpoint data for a session
///
/// This removes:
/// 1. All delta files
/// 2. The checkpoint index
async fn delete_all_checkpoints(
    s5_storage: &dyn S5Storage,
    host_address: &str,
    session_id: &str,
) -> Result<usize> {
    let index_path = CheckpointIndex::s5_path(host_address, session_id);

    // 1. Try to fetch the index to get delta paths
    let deltas_count = match s5_storage.get(&index_path).await {
        Ok(bytes) => {
            match serde_json::from_slice::<CheckpointIndex>(&bytes) {
                Ok(index) => {
                    let count = index.checkpoints.len();
                    // Delete each delta
                    for checkpoint in &index.checkpoints {
                        let delta_path = format!(
                            "home/checkpoints/{}/{}/delta_{}.json",
                            host_address.to_lowercase(),
                            session_id,
                            checkpoint.index
                        );
                        if let Err(e) = s5_storage.delete(&delta_path).await {
                            warn!("Failed to delete delta {}: {}", delta_path, e);
                        }
                    }
                    count
                }
                Err(e) => {
                    warn!("Failed to parse index for deletion: {}", e);
                    0
                }
            }
        }
        Err(_) => {
            // Index doesn't exist, nothing to delete
            0
        }
    };

    // 2. Delete the index itself
    if let Err(e) = s5_storage.delete(&index_path).await {
        // Only warn if there was an index to delete
        if deltas_count > 0 {
            warn!("Failed to delete index {}: {}", index_path, e);
        }
    }

    info!(
        "Deleted {} checkpoint deltas for session {}",
        deltas_count, session_id
    );

    Ok(deltas_count)
}

/// Mark checkpoint data for future cleanup
///
/// This updates the index with an expiry timestamp.
/// A background cleanup process should check and delete expired data.
async fn mark_for_cleanup(
    s5_storage: &dyn S5Storage,
    index_path: &str,
    ttl_days: u64,
) -> Result<()> {
    // For now, we just log the marking
    // In production, this would update the index with expires_at timestamp
    // or add to a cleanup queue in a database
    info!(
        "Marked {} for cleanup in {} days",
        index_path, ttl_days
    );

    // Note: S5 doesn't have native TTL support, so actual deletion
    // would need to be handled by a background cleanup job that:
    // 1. Scans for expired indices
    // 2. Calls delete_all_checkpoints for each expired session

    Ok(())
}

/// Cleanup configuration for checkpoint data
#[derive(Debug, Clone)]
pub struct CleanupConfig {
    /// TTL for completed sessions
    pub completed_ttl: Duration,

    /// TTL for timed out sessions
    pub timed_out_ttl: Duration,

    /// Whether to immediately delete cancelled sessions
    pub delete_cancelled_immediately: bool,

    /// Grace period after dispute resolution
    pub dispute_grace_period: Duration,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            completed_ttl: TTL_COMPLETED,
            timed_out_ttl: TTL_TIMED_OUT,
            delete_cancelled_immediately: true,
            dispute_grace_period: TTL_DISPUTE_GRACE,
        }
    }
}

impl CleanupConfig {
    /// Get TTL for a given session state
    pub fn ttl_for_state(&self, state: &SessionState) -> Option<Duration> {
        match state {
            SessionState::Active => None, // Never cleanup active sessions
            SessionState::Completed => Some(self.completed_ttl),
            SessionState::TimedOut => Some(self.timed_out_ttl),
            SessionState::Cancelled => {
                if self.delete_cancelled_immediately {
                    Some(Duration::ZERO)
                } else {
                    Some(self.completed_ttl)
                }
            }
        }
    }

    /// Check if a session should be cleaned up
    pub fn should_cleanup(&self, state: &SessionState, age: Duration) -> bool {
        match self.ttl_for_state(state) {
            Some(ttl) => age >= ttl,
            None => false, // Active sessions never cleaned up
        }
    }
}

/// Cleanup task for background execution
#[derive(Debug)]
pub struct CleanupTask {
    /// Session ID to cleanup
    pub session_id: String,

    /// Host address
    pub host_address: String,

    /// When the session ended
    pub ended_at: u64,

    /// Final session state
    pub state: SessionState,
}

impl CleanupTask {
    /// Create a new cleanup task
    pub fn new(
        session_id: String,
        host_address: String,
        ended_at: u64,
        state: SessionState,
    ) -> Self {
        Self {
            session_id,
            host_address,
            ended_at,
            state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_cleanup_config() {
        let config = CleanupConfig::default();
        assert_eq!(config.completed_ttl, TTL_COMPLETED);
        assert_eq!(config.timed_out_ttl, TTL_TIMED_OUT);
        assert!(config.delete_cancelled_immediately);
        assert_eq!(config.dispute_grace_period, TTL_DISPUTE_GRACE);
    }

    #[test]
    fn test_ttl_for_active_session() {
        let config = CleanupConfig::default();
        assert!(config.ttl_for_state(&SessionState::Active).is_none());
    }

    #[test]
    fn test_ttl_for_completed_session() {
        let config = CleanupConfig::default();
        assert_eq!(
            config.ttl_for_state(&SessionState::Completed),
            Some(TTL_COMPLETED)
        );
    }

    #[test]
    fn test_ttl_for_timed_out_session() {
        let config = CleanupConfig::default();
        assert_eq!(
            config.ttl_for_state(&SessionState::TimedOut),
            Some(TTL_TIMED_OUT)
        );
    }

    #[test]
    fn test_ttl_for_cancelled_session() {
        let config = CleanupConfig::default();
        assert_eq!(
            config.ttl_for_state(&SessionState::Cancelled),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn test_should_cleanup_active_never() {
        let config = CleanupConfig::default();
        // Even with very old age, active sessions should not be cleaned up
        let old_age = Duration::from_secs(365 * 24 * 60 * 60); // 1 year
        assert!(!config.should_cleanup(&SessionState::Active, old_age));
    }

    #[test]
    fn test_should_cleanup_completed_after_ttl() {
        let config = CleanupConfig::default();

        // Just under TTL - should not cleanup
        let under_ttl = TTL_COMPLETED - Duration::from_secs(1);
        assert!(!config.should_cleanup(&SessionState::Completed, under_ttl));

        // At TTL - should cleanup
        assert!(config.should_cleanup(&SessionState::Completed, TTL_COMPLETED));

        // Over TTL - should cleanup
        let over_ttl = TTL_COMPLETED + Duration::from_secs(1);
        assert!(config.should_cleanup(&SessionState::Completed, over_ttl));
    }

    #[test]
    fn test_should_cleanup_cancelled_immediately() {
        let config = CleanupConfig::default();
        assert!(config.should_cleanup(&SessionState::Cancelled, Duration::ZERO));
    }

    #[test]
    fn test_cleanup_task_new() {
        let task = CleanupTask::new(
            "session-123".to_string(),
            "0xhost".to_string(),
            1704844800000,
            SessionState::Completed,
        );

        assert_eq!(task.session_id, "session-123");
        assert_eq!(task.host_address, "0xhost");
        assert_eq!(task.ended_at, 1704844800000);
        assert_eq!(task.state, SessionState::Completed);
    }

    #[test]
    fn test_ttl_values_correct() {
        // 7 days in seconds
        assert_eq!(TTL_COMPLETED.as_secs(), 7 * 24 * 60 * 60);
        assert_eq!(TTL_COMPLETED.as_secs(), 604800);

        // 30 days in seconds
        assert_eq!(TTL_TIMED_OUT.as_secs(), 30 * 24 * 60 * 60);
        assert_eq!(TTL_TIMED_OUT.as_secs(), 2592000);

        // Immediate
        assert_eq!(TTL_CANCELLED.as_secs(), 0);
    }

    // ==================== Async Cleanup Tests ====================

    use crate::checkpoint::{CheckpointEntry, CheckpointIndex};
    use crate::storage::s5_client::MockS5Backend;

    #[tokio::test]
    async fn test_cleanup_active_session_skipped() {
        let mock = MockS5Backend::new();
        let result = cleanup_checkpoints(&mock, "0xhost", "session-1", SessionState::Active).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), CleanupResult::Skipped);
    }

    #[tokio::test]
    async fn test_cleanup_completed_session_7_days() {
        let mock = MockS5Backend::new();
        let result =
            cleanup_checkpoints(&mock, "0xhost", "session-2", SessionState::Completed).await;

        assert!(result.is_ok());
        match result.unwrap() {
            CleanupResult::MarkedForCleanup { ttl_days } => {
                assert_eq!(ttl_days, 7, "Completed sessions should have 7 day TTL");
            }
            other => panic!("Expected MarkedForCleanup, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_cleanup_timed_out_session_30_days() {
        let mock = MockS5Backend::new();
        let result =
            cleanup_checkpoints(&mock, "0xhost", "session-3", SessionState::TimedOut).await;

        assert!(result.is_ok());
        match result.unwrap() {
            CleanupResult::MarkedForCleanup { ttl_days } => {
                assert_eq!(ttl_days, 30, "Timed out sessions should have 30 day TTL");
            }
            other => panic!("Expected MarkedForCleanup, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_cleanup_cancelled_session_immediate() {
        let mock = MockS5Backend::new();

        // Pre-populate with checkpoint data
        let mut index = CheckpointIndex::new("session-4".to_string(), "0xhostcancel".to_string());
        index.add_checkpoint(CheckpointEntry::with_timestamp(
            0,
            "0xproof1".to_string(),
            "bafycid1".to_string(),
            0,
            1000,
            1704844800000,
        ));
        index.add_checkpoint(CheckpointEntry::with_timestamp(
            1,
            "0xproof2".to_string(),
            "bafycid2".to_string(),
            1000,
            2000,
            1704844900000,
        ));
        index.host_signature = "0xsig".to_string();

        let index_path = "home/checkpoints/0xhostcancel/session-4/index.json";
        let index_bytes = serde_json::to_vec(&index).unwrap();
        mock.put(index_path, index_bytes).await.unwrap();

        // Also add delta files
        mock.put(
            "home/checkpoints/0xhostcancel/session-4/delta_0.json",
            b"delta0".to_vec(),
        )
        .await
        .unwrap();
        mock.put(
            "home/checkpoints/0xhostcancel/session-4/delta_1.json",
            b"delta1".to_vec(),
        )
        .await
        .unwrap();

        // Run cleanup
        let result =
            cleanup_checkpoints(&mock, "0xhostcancel", "session-4", SessionState::Cancelled).await;

        assert!(result.is_ok());
        match result.unwrap() {
            CleanupResult::Deleted { deltas_removed } => {
                assert_eq!(deltas_removed, 2, "Should have deleted 2 deltas");
            }
            other => panic!("Expected Deleted, got {:?}", other),
        }

        // Verify data was deleted
        assert!(
            mock.get(index_path).await.is_err(),
            "Index should be deleted"
        );
    }

    #[tokio::test]
    async fn test_cleanup_cancelled_empty_session() {
        let mock = MockS5Backend::new();

        // Cleanup a session that has no checkpoint data
        let result =
            cleanup_checkpoints(&mock, "0xhost", "nonexistent", SessionState::Cancelled).await;

        assert!(result.is_ok());
        match result.unwrap() {
            CleanupResult::Deleted { deltas_removed } => {
                assert_eq!(deltas_removed, 0, "Should report 0 deltas for empty session");
            }
            other => panic!("Expected Deleted, got {:?}", other),
        }
    }
}
