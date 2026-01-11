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

use crate::checkpoint::SessionState;
use std::time::Duration;

/// TTL for completed sessions (7 days)
pub const TTL_COMPLETED: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// TTL for timed out sessions (30 days)
pub const TTL_TIMED_OUT: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// TTL for cancelled sessions (immediate deletion)
pub const TTL_CANCELLED: Duration = Duration::ZERO;

/// Grace period after dispute resolution (7 days)
pub const TTL_DISPUTE_GRACE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

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
}
