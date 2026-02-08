// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Tests for S5 Vector Database Loading during Session Initialization (Sub-phase 3.2)

use fabstir_llm_node::api::websocket::message_types::VectorDatabaseInfo;
use fabstir_llm_node::api::websocket::session::{VectorLoadingStatus, WebSocketSession};
use fabstir_llm_node::rag::session_vector_store::SessionVectorStore;
use fabstir_llm_node::storage::manifest::Vector;
use std::sync::{Arc, Mutex};
use tokio::time::{timeout, Duration};

#[cfg(test)]
mod session_init_s5_tests {
    use super::*;

    /// Helper: Create VectorDatabaseInfo for testing
    fn create_test_vector_db_info(user_address: &str, db_name: &str) -> VectorDatabaseInfo {
        VectorDatabaseInfo {
            manifest_path: format!(
                "home/vector-databases/{}/{}/manifest.json",
                user_address, db_name
            ),
            user_address: user_address.to_string(),
        }
    }

    /// Helper: Create test vectors
    fn create_test_vectors(count: usize) -> Vec<Vector> {
        (0..count)
            .map(|i| Vector {
                id: format!("vec-{}", i),
                vector: vec![0.1; 384],
                metadata: serde_json::json!({
                    "source": "test.pdf",
                    "page": i,
                }),
            })
            .collect()
    }

    /// Test 1: Session initialization without vector_database (backward compatibility)
    #[tokio::test]
    async fn test_session_init_without_vector_database() {
        let session = WebSocketSession::new("test-session-1");

        // Verify default state
        assert_eq!(session.vector_database, None);
        assert_eq!(
            session.vector_loading_status,
            VectorLoadingStatus::NotStarted
        );
        assert!(session.vector_store.is_none());
    }

    /// Test 2: Session initialization with vector_database field
    #[tokio::test]
    async fn test_session_init_with_vector_database() {
        let mut session = WebSocketSession::new("test-session-2");
        let vdb_info = create_test_vector_db_info("0xABC123", "my-docs");

        // Set vector database info
        session.vector_database = Some(vdb_info.clone());

        // Verify it's stored
        assert!(session.vector_database.is_some());
        let stored_vdb = session.vector_database.as_ref().unwrap();
        assert_eq!(
            stored_vdb.manifest_path,
            "home/vector-databases/0xABC123/my-docs/manifest.json"
        );
        assert_eq!(stored_vdb.user_address, "0xABC123");

        // Initially not started
        assert_eq!(
            session.vector_loading_status,
            VectorLoadingStatus::NotStarted
        );
    }

    /// Test 3: Vector loading status transitions
    #[tokio::test]
    async fn test_vector_loading_status_transitions() {
        let mut session = WebSocketSession::new("test-session-3");

        // NotStarted -> Loading
        session.vector_loading_status = VectorLoadingStatus::Loading;
        assert_eq!(session.vector_loading_status, VectorLoadingStatus::Loading);

        // Loading -> Loaded
        session.vector_loading_status = VectorLoadingStatus::Loaded {
            vector_count: 100,
            load_time_ms: 500,
        };
        match session.vector_loading_status {
            VectorLoadingStatus::Loaded {
                vector_count,
                load_time_ms,
            } => {
                assert_eq!(vector_count, 100);
                assert_eq!(load_time_ms, 500);
            }
            _ => panic!("Expected Loaded status"),
        }

        // Can also transition to Error
        session.vector_loading_status = VectorLoadingStatus::Error {
            error: "Test error".to_string(),
        };
        match session.vector_loading_status {
            VectorLoadingStatus::Error { error } => {
                assert_eq!(error, "Test error");
            }
            _ => panic!("Expected Error status"),
        }
    }

    /// Test 4: Loading vectors and populating vector_store
    #[tokio::test]
    async fn test_load_vectors_into_store() {
        let mut session = WebSocketSession::new("test-session-4");

        // Create vector store
        let vector_store = Arc::new(Mutex::new(SessionVectorStore::new(
            "test-session-4".to_string(),
            100_000,
        )));

        session.vector_store = Some(vector_store.clone());

        // Simulate loading vectors
        let test_vectors = create_test_vectors(10);

        // Add vectors to store
        {
            let mut store = vector_store.lock().unwrap();
            for vector in test_vectors {
                store
                    .add(
                        vector.id.clone(),
                        vector.vector.clone(),
                        vector.metadata.clone(),
                    )
                    .expect("Failed to add vector");
            }
        }

        // Verify vectors were added
        let store = vector_store.lock().unwrap();
        assert_eq!(store.count(), 10);
    }

    /// Test 5: VectorDatabaseInfo validation
    #[tokio::test]
    async fn test_vector_database_info_validation() {
        let vdb_info = VectorDatabaseInfo {
            manifest_path: "home/vector-databases/0xABC/docs/manifest.json".to_string(),
            user_address: "0xABC".to_string(),
        };

        // Validate manifest path format
        assert!(vdb_info.manifest_path.contains("manifest.json"));
        assert!(vdb_info.manifest_path.starts_with("home/vector-databases/"));

        // Validate user address is present
        assert!(!vdb_info.user_address.is_empty());
    }

    /// Test 6: Concurrent session initialization
    #[tokio::test]
    async fn test_concurrent_session_init() {
        let mut handles = vec![];

        for i in 0..10 {
            let handle = tokio::spawn(async move {
                let mut session = WebSocketSession::new(format!("concurrent-session-{}", i));
                let vdb_info = create_test_vector_db_info("0xUSER", &format!("db-{}", i));
                session.vector_database = Some(vdb_info);
                session
            });
            handles.push(handle);
        }

        // Wait for all sessions to be created
        for handle in handles {
            let session = handle.await.unwrap();
            assert!(session.vector_database.is_some());
        }
    }

    /// Test 7: Loading status serialization
    #[tokio::test]
    async fn test_loading_status_serialization() {
        // NotStarted
        let status = VectorLoadingStatus::NotStarted;
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: VectorLoadingStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);

        // Loading
        let status = VectorLoadingStatus::Loading;
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: VectorLoadingStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);

        // Loaded
        let status = VectorLoadingStatus::Loaded {
            vector_count: 1000,
            load_time_ms: 2500,
        };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: VectorLoadingStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);

        // Error
        let status = VectorLoadingStatus::Error {
            error: "Network failure".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: VectorLoadingStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);
    }

    /// Test 8: Session with vector_store capacity limits
    #[tokio::test]
    async fn test_vector_store_capacity() {
        let vector_store = Arc::new(Mutex::new(SessionVectorStore::new(
            "capacity-test".to_string(),
            10, // Small capacity for testing
        )));

        // Add vectors up to capacity
        {
            let mut store = vector_store.lock().unwrap();
            for i in 0..10 {
                let result = store.add(
                    format!("vec-{}", i),
                    vec![0.1; 384],
                    serde_json::json!({"index": i}),
                );
                assert!(result.is_ok());
            }
        }

        // Try to add beyond capacity
        {
            let mut store = vector_store.lock().unwrap();
            let result = store.add(
                "vec-overflow".to_string(),
                vec![0.1; 384],
                serde_json::json!({}),
            );
            assert!(result.is_err());
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("Maximum vector capacity"));
        }
    }

    /// Test 9: VectorDatabaseInfo with different paths
    #[tokio::test]
    async fn test_vector_database_info_paths() {
        let test_cases = vec![
            (
                "0xABC",
                "docs",
                "home/vector-databases/0xABC/docs/manifest.json",
            ),
            (
                "0x123456",
                "my-project",
                "home/vector-databases/0x123456/my-project/manifest.json",
            ),
            (
                "0xUSER",
                "test_db_1",
                "home/vector-databases/0xUSER/test_db_1/manifest.json",
            ),
        ];

        for (user, db_name, expected_path) in test_cases {
            let vdb_info = create_test_vector_db_info(user, db_name);
            assert_eq!(vdb_info.manifest_path, expected_path);
            assert_eq!(vdb_info.user_address, user);
        }
    }

    /// Test 10: Session cleanup with vector_store
    #[tokio::test]
    async fn test_session_cleanup() {
        let mut session = WebSocketSession::new("cleanup-test");

        // Set up vector store
        let vector_store = Arc::new(Mutex::new(SessionVectorStore::new(
            "cleanup-test".to_string(),
            100_000,
        )));

        // Add some vectors
        {
            let mut store = vector_store.lock().unwrap();
            for i in 0..5 {
                store
                    .add(
                        format!("vec-{}", i),
                        vec![0.1; 384],
                        serde_json::json!({"index": i}),
                    )
                    .unwrap();
            }
        }

        session.vector_store = Some(vector_store.clone());

        // Verify vectors exist
        {
            let store = vector_store.lock().unwrap();
            assert_eq!(store.count(), 5);
        }

        // Simulate cleanup by dropping the session
        drop(session);

        // Vector store should still be accessible via the Arc
        // but in real implementation, dropping the session would clean up
        let store = vector_store.lock().unwrap();
        assert_eq!(store.count(), 5);
    }

    /// Test 11: Error status with different error messages
    #[tokio::test]
    async fn test_error_status_messages() {
        let error_cases = vec![
            "Manifest not found",
            "Decryption failed: wrong key",
            "Owner mismatch: expected 0xABC, got 0xDEF",
            "Network timeout after 5 minutes",
            "Invalid vector dimensions: expected 384, got 256",
        ];

        for error_msg in error_cases {
            let status = VectorLoadingStatus::Error {
                error: error_msg.to_string(),
            };

            match status {
                VectorLoadingStatus::Error { error } => {
                    assert_eq!(error, error_msg);
                }
                _ => panic!("Expected Error status"),
            }
        }
    }

    /// Test 12: Session with both vector_database and vector_store
    #[tokio::test]
    async fn test_session_with_full_vector_setup() {
        let mut session = WebSocketSession::new("full-setup-test");

        // Set vector database info
        let vdb_info = create_test_vector_db_info("0xFULL", "complete-db");
        session.vector_database = Some(vdb_info);

        // Set loading status to Loading
        session.vector_loading_status = VectorLoadingStatus::Loading;

        // Create vector store
        let vector_store = Arc::new(Mutex::new(SessionVectorStore::new(
            "full-setup-test".to_string(),
            100_000,
        )));

        // Simulate successful loading
        {
            let mut store = vector_store.lock().unwrap();
            let test_vectors = create_test_vectors(50);
            for vector in test_vectors {
                store
                    .add(vector.id, vector.vector, vector.metadata)
                    .unwrap();
            }
        }

        session.vector_store = Some(vector_store.clone());

        // Update status to Loaded
        session.vector_loading_status = VectorLoadingStatus::Loaded {
            vector_count: 50,
            load_time_ms: 1500,
        };

        // Verify everything is set up correctly
        assert!(session.vector_database.is_some());
        assert!(session.vector_store.is_some());

        match session.vector_loading_status {
            VectorLoadingStatus::Loaded {
                vector_count,
                load_time_ms,
            } => {
                assert_eq!(vector_count, 50);
                assert_eq!(load_time_ms, 1500);
            }
            _ => panic!("Expected Loaded status"),
        }

        let store = vector_store.lock().unwrap();
        assert_eq!(store.count(), 50);
    }
}
