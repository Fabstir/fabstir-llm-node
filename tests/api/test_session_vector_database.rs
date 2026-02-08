// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Tests for Session with S5 Vector Database Info (Sub-phase 1.2)

use fabstir_llm_node::api::websocket::message_types::VectorDatabaseInfo;
use fabstir_llm_node::api::websocket::session::{SessionConfig, WebSocketSession};

#[cfg(test)]
mod session_vector_database_tests {
    use super::*;

    /// Test 1: Session struct has vector_database field
    #[test]
    fn test_session_has_vector_database_field() {
        let session = WebSocketSession::new("test-session");

        // Session should have vector_database field
        // This will compile once we add the field
        assert!(session.vector_database.is_none());
    }

    /// Test 2: Session creation with vector_database info
    #[test]
    fn test_session_creation_with_vector_database() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string(),
            user_address: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        // Create session with vector_database
        let mut session = WebSocketSession::new("test-session");
        session.set_vector_database(Some(vdb_info.clone()));

        // Verify it was set
        assert!(session.vector_database.is_some());
        let stored_vdb = session.vector_database.as_ref().unwrap();
        assert_eq!(stored_vdb.manifest_path, vdb_info.manifest_path);
        assert_eq!(stored_vdb.user_address, vdb_info.user_address);
    }

    /// Test 3: Session creation without vector_database (backward compatibility)
    #[test]
    fn test_session_creation_without_vector_database() {
        let session = WebSocketSession::new("test-session");

        // Should be None by default
        assert!(session.vector_database.is_none());

        // Should have default loading status
        assert_eq!(
            session.vector_loading_status,
            VectorLoadingStatus::NotStarted
        );
    }

    /// Test 4: Vector loading status tracking
    #[test]
    fn test_vector_loading_status_transitions() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string(),
            user_address: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        let mut session = WebSocketSession::new("test-session");
        session.set_vector_database(Some(vdb_info));

        // Initial status
        assert_eq!(
            session.vector_loading_status,
            VectorLoadingStatus::NotStarted
        );

        // Transition to Loading
        session.set_vector_loading_status(VectorLoadingStatus::Loading);
        assert_eq!(session.vector_loading_status, VectorLoadingStatus::Loading);

        // Transition to Loaded
        session.set_vector_loading_status(VectorLoadingStatus::Loaded {
            vector_count: 1000,
            load_time_ms: 500,
        });
        match session.vector_loading_status {
            VectorLoadingStatus::Loaded {
                vector_count,
                load_time_ms,
            } => {
                assert_eq!(vector_count, 1000);
                assert_eq!(load_time_ms, 500);
            }
            _ => panic!("Expected Loaded status"),
        }
    }

    /// Test 5: Vector loading error status
    #[test]
    fn test_vector_loading_error_status() {
        let mut session = WebSocketSession::new("test-session");

        // Set error status
        session.set_vector_loading_status(VectorLoadingStatus::Error {
            error: "Failed to download manifest".to_string(),
        });

        match session.vector_loading_status {
            VectorLoadingStatus::Error { ref error } => {
                assert!(error.contains("manifest"));
            }
            _ => panic!("Expected Error status"),
        }
    }

    /// Test 6: Get vector_database info
    #[test]
    fn test_get_vector_database_info() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string(),
            user_address: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        let mut session = WebSocketSession::new("test-session");
        session.set_vector_database(Some(vdb_info.clone()));

        // Get the info
        let retrieved = session.get_vector_database_info();
        assert!(retrieved.is_some());
        let retrieved_vdb = retrieved.unwrap();
        assert_eq!(retrieved_vdb.manifest_path, vdb_info.manifest_path);
    }

    /// Test 7: Session with both uploaded vectors and S5 database
    #[test]
    fn test_session_with_both_vector_sources() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string(),
            user_address: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        let mut session = WebSocketSession::new("test-session");

        // Set S5 vector database
        session.set_vector_database(Some(vdb_info));

        // Session should also have vector_store for uploaded vectors
        // Both can coexist
        assert!(session.vector_database.is_some());
        assert!(session.vector_store.is_none()); // Not set yet
    }

    /// Test 8: Session serialization with vector_database
    #[test]
    #[ignore] // Enable once Serialize is implemented properly
    fn test_session_serialization_with_vector_database() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string(),
            user_address: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        let mut session = WebSocketSession::new("test-session");
        session.set_vector_database(Some(vdb_info));

        // Serialize session
        // let json = serde_json::to_string(&session).expect("Failed to serialize");
        // Should include vector_database field
    }

    /// Test 9: Clear vector_database info
    #[test]
    fn test_clear_vector_database() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string(),
            user_address: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        let mut session = WebSocketSession::new("test-session");
        session.set_vector_database(Some(vdb_info));
        assert!(session.vector_database.is_some());

        // Clear it
        session.set_vector_database(None);
        assert!(session.vector_database.is_none());
    }

    /// Test 10: Multiple sessions with different vector databases
    #[test]
    fn test_multiple_sessions_different_databases() {
        let vdb1 = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC.../docs1/manifest.json".to_string(),
            user_address: "0xABCDEF1234567890ABCDEF1234567890ABCDEF12".to_string(),
        };

        let vdb2 = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xDEF.../docs2/manifest.json".to_string(),
            user_address: "0xDEF1234567890ABCDEF1234567890ABCDEF123456".to_string(),
        };

        let mut session1 = WebSocketSession::new("session-1");
        let mut session2 = WebSocketSession::new("session-2");

        session1.set_vector_database(Some(vdb1.clone()));
        session2.set_vector_database(Some(vdb2.clone()));

        // Each session should have its own vector_database
        assert_eq!(
            session1.get_vector_database_info().unwrap().manifest_path,
            vdb1.manifest_path
        );
        assert_eq!(
            session2.get_vector_database_info().unwrap().manifest_path,
            vdb2.manifest_path
        );
    }
}

// Import the enum we'll create
use fabstir_llm_node::api::websocket::session::VectorLoadingStatus;
