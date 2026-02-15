// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Integration Tests for Checkpoint Publishing
//!
//! Tests the full checkpoint publishing flow for SDK conversation recovery.

use anyhow::Result;
use fabstir_llm_node::checkpoint::{
    cleanup_checkpoints, CheckpointDelta, CheckpointEntry, CheckpointIndex, CheckpointMessage,
    CheckpointPublisher, CleanupResult, SessionState,
};
use fabstir_llm_node::storage::s5_client::MockS5Backend;
use fabstir_llm_node::storage::S5Storage;
use k256::ecdsa::SigningKey;
use rand::rngs::OsRng;

/// Generate a test private key
fn generate_test_private_key() -> [u8; 32] {
    let signing_key = SigningKey::random(&mut OsRng);
    signing_key.to_bytes().into()
}

// ==================== Full Flow Tests ====================

#[tokio::test]
async fn test_full_checkpoint_flow() -> Result<()> {
    // 1. Create publisher with mock S5
    let mock = MockS5Backend::new();
    let publisher = CheckpointPublisher::new("0xhostflow".to_string());
    let private_key = generate_test_private_key();

    // 2. Buffer user message
    publisher
        .buffer_message(
            "session-flow",
            CheckpointMessage::new_user("What is the capital of France?".to_string(), 100),
        )
        .await;

    // 3. Buffer assistant response
    publisher
        .buffer_message(
            "session-flow",
            CheckpointMessage::new_assistant(
                "The capital of France is Paris.".to_string(),
                200,
                false,
            ),
        )
        .await;

    // 4. Publish checkpoint (simulating 1000 tokens generated)
    let proof_hash = [0x12u8; 32];
    let delta_cid = publisher
        .publish_checkpoint("session-flow", proof_hash, 0, 1000, &private_key, &mock)
        .await?;

    // 5. Verify delta was uploaded
    let delta_path = "home/checkpoints/0xhostflow/session-flow/delta_0.json";
    let delta_bytes = mock.get(delta_path).await?;
    let delta: CheckpointDelta = serde_json::from_slice(&delta_bytes)?;

    assert_eq!(delta.session_id, "session-flow");
    assert_eq!(delta.checkpoint_index, 0);
    assert_eq!(delta.messages.len(), 2);
    assert_eq!(delta.messages[0].role, "user");
    assert_eq!(delta.messages[1].role, "assistant");
    assert!(!delta.host_signature.is_empty());

    // 6. Verify index was updated
    let index_path = "home/checkpoints/0xhostflow/session-flow/index.json";
    let index_bytes = mock.get(index_path).await?;
    let index: CheckpointIndex = serde_json::from_slice(&index_bytes)?;

    assert_eq!(index.session_id, "session-flow");
    assert_eq!(index.checkpoints.len(), 1);
    assert_eq!(index.checkpoints[0].index, 0);
    assert_eq!(index.checkpoints[0].token_range, [0, 1000]);
    assert_eq!(index.checkpoints[0].delta_cid, delta_cid);
    assert!(!index.host_signature.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_multiple_checkpoints() -> Result<()> {
    // Verify that 3000 tokens = 3 checkpoints accumulate correctly
    let mock = MockS5Backend::new();
    let publisher = CheckpointPublisher::new("0xhostmulti".to_string());
    let private_key = generate_test_private_key();

    // Checkpoint 1: 0-1000 tokens
    publisher
        .buffer_message(
            "session-multi",
            CheckpointMessage::new_user("Question 1".to_string(), 100),
        )
        .await;
    publisher
        .buffer_message(
            "session-multi",
            CheckpointMessage::new_assistant("Answer 1".to_string(), 200, false),
        )
        .await;

    let proof_hash1 = [0x11u8; 32];
    publisher
        .publish_checkpoint("session-multi", proof_hash1, 0, 1000, &private_key, &mock)
        .await?;

    // Checkpoint 2: 1000-2000 tokens
    publisher
        .buffer_message(
            "session-multi",
            CheckpointMessage::new_user("Question 2".to_string(), 1100),
        )
        .await;
    publisher
        .buffer_message(
            "session-multi",
            CheckpointMessage::new_assistant("Answer 2".to_string(), 1200, false),
        )
        .await;

    let proof_hash2 = [0x22u8; 32];
    publisher
        .publish_checkpoint(
            "session-multi",
            proof_hash2,
            1000,
            2000,
            &private_key,
            &mock,
        )
        .await?;

    // Checkpoint 3: 2000-3000 tokens
    publisher
        .buffer_message(
            "session-multi",
            CheckpointMessage::new_user("Question 3".to_string(), 2100),
        )
        .await;
    publisher
        .buffer_message(
            "session-multi",
            CheckpointMessage::new_assistant("Answer 3".to_string(), 2200, false),
        )
        .await;

    let proof_hash3 = [0x33u8; 32];
    publisher
        .publish_checkpoint(
            "session-multi",
            proof_hash3,
            2000,
            3000,
            &private_key,
            &mock,
        )
        .await?;

    // Verify index has all 3 checkpoints
    let index_path = "home/checkpoints/0xhostmulti/session-multi/index.json";
    let index_bytes = mock.get(index_path).await?;
    let index: CheckpointIndex = serde_json::from_slice(&index_bytes)?;

    assert_eq!(index.checkpoints.len(), 3, "Should have 3 checkpoints");
    assert_eq!(index.checkpoints[0].index, 0);
    assert_eq!(index.checkpoints[0].token_range, [0, 1000]);
    assert_eq!(index.checkpoints[1].index, 1);
    assert_eq!(index.checkpoints[1].token_range, [1000, 2000]);
    assert_eq!(index.checkpoints[2].index, 2);
    assert_eq!(index.checkpoints[2].token_range, [2000, 3000]);

    // Verify each delta exists
    for i in 0..3 {
        let delta_path = format!(
            "home/checkpoints/0xhostmulti/session-multi/delta_{}.json",
            i
        );
        let delta_bytes = mock.get(&delta_path).await?;
        let delta: CheckpointDelta = serde_json::from_slice(&delta_bytes)?;
        assert_eq!(delta.checkpoint_index, i as u32);
        assert_eq!(delta.messages.len(), 2);
    }

    Ok(())
}

#[tokio::test]
async fn test_checkpoint_recovery_by_sdk() -> Result<()> {
    // Verify data format is correct for SDK to recover conversation
    let mock = MockS5Backend::new();
    let publisher = CheckpointPublisher::new("0xhostsdk".to_string());
    let private_key = generate_test_private_key();

    // Create a conversation
    let messages = vec![
        ("user", "Hello, how are you?"),
        ("assistant", "I'm doing well, thank you for asking!"),
        ("user", "What's the weather like?"),
        ("assistant", "I don't have access to weather data."),
    ];

    for (i, (role, content)) in messages.iter().enumerate() {
        let timestamp = 1704844800000 + (i as u64 * 1000);
        if *role == "user" {
            publisher
                .buffer_message(
                    "session-sdk",
                    CheckpointMessage::new_user(content.to_string(), timestamp),
                )
                .await;
        } else {
            publisher
                .buffer_message(
                    "session-sdk",
                    CheckpointMessage::new_assistant(content.to_string(), timestamp, false),
                )
                .await;
        }
    }

    let proof_hash = [0xABu8; 32];
    publisher
        .publish_checkpoint("session-sdk", proof_hash, 0, 1000, &private_key, &mock)
        .await?;

    // SDK recovery: Fetch index
    let index_path = CheckpointIndex::s5_path("0xhostsdk", "session-sdk");
    let index_bytes = mock.get(&index_path).await?;
    let index: CheckpointIndex = serde_json::from_slice(&index_bytes)?;

    // SDK recovery: Verify index has expected structure
    assert_eq!(index.session_id, "session-sdk");
    assert_eq!(index.host_address, "0xhostsdk");
    assert!(!index.host_signature.is_empty());
    assert!(index.host_signature.starts_with("0x"));
    assert_eq!(index.host_signature.len(), 132); // 0x + 130 hex chars = 65 bytes

    // SDK recovery: Fetch delta using CID from index
    let _delta_cid = &index.checkpoints[0].delta_cid;
    let delta_path = format!(
        "home/checkpoints/0xhostsdk/session-sdk/delta_{}.json",
        index.checkpoints[0].index
    );
    let delta_bytes = mock.get(&delta_path).await?;
    let delta: CheckpointDelta = serde_json::from_slice(&delta_bytes)?;

    // SDK recovery: Verify delta has conversation messages
    assert_eq!(delta.messages.len(), 4);
    assert_eq!(delta.messages[0].role, "user");
    assert_eq!(delta.messages[0].content, "Hello, how are you?");
    assert_eq!(delta.messages[1].role, "assistant");
    assert_eq!(
        delta.messages[1].content,
        "I'm doing well, thank you for asking!"
    );
    assert_eq!(delta.messages[2].role, "user");
    assert_eq!(delta.messages[2].content, "What's the weather like?");
    assert_eq!(delta.messages[3].role, "assistant");
    assert_eq!(
        delta.messages[3].content,
        "I don't have access to weather data."
    );

    // SDK recovery: Verify signature format (EIP-191)
    assert!(!delta.host_signature.is_empty());
    assert!(delta.host_signature.starts_with("0x"));
    assert_eq!(delta.host_signature.len(), 132);

    Ok(())
}

#[tokio::test]
async fn test_s5_failure_blocks_proof() -> Result<()> {
    // Configure mock to fail - no orphaned proofs should exist
    let mock = MockS5Backend::new();
    mock.set_quota_limit(0).await; // All uploads will fail

    let publisher = CheckpointPublisher::new("0xhostfail".to_string());
    let private_key = generate_test_private_key();

    publisher
        .buffer_message(
            "session-fail",
            CheckpointMessage::new_user("Test message".to_string(), 100),
        )
        .await;

    let proof_hash = [0xFFu8; 32];
    let result = publisher
        .publish_checkpoint("session-fail", proof_hash, 0, 1000, &private_key, &mock)
        .await;

    // Verify publish failed
    assert!(result.is_err(), "Should fail when S5 upload fails");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("NOT submitting proof"),
        "Error should indicate proof blocked: {}",
        err
    );

    // Verify no data was stored (no orphaned state)
    let index_path = "home/checkpoints/0xhostfail/session-fail/index.json";
    assert!(
        mock.get(index_path).await.is_err(),
        "No index should exist after failure"
    );

    Ok(())
}

// ==================== Session Resumption Tests ====================

#[tokio::test]
async fn test_session_resumption_from_s5() -> Result<()> {
    let mock = MockS5Backend::new();
    let private_key = generate_test_private_key();

    // Pre-populate S5 with existing session data
    let mut existing_index =
        CheckpointIndex::new("session-resume".to_string(), "0xhostresume".to_string());
    existing_index.add_checkpoint(CheckpointEntry::with_timestamp(
        0,
        "0xoldproof".to_string(),
        "bafyoldcid".to_string(),
        0,
        1000,
        1704844800000,
    ));
    existing_index.host_signature = "0xsig".to_string();

    let index_path = "home/checkpoints/0xhostresume/session-resume/index.json";
    let index_bytes = serde_json::to_vec(&existing_index)?;
    mock.put(index_path, index_bytes).await?;

    // Create new publisher and initialize from S5
    let publisher = CheckpointPublisher::new("0xhostresume".to_string());
    publisher.init_session("session-resume", &mock).await?;

    // Continue the session
    publisher
        .buffer_message(
            "session-resume",
            CheckpointMessage::new_user("Continuing conversation".to_string(), 1100),
        )
        .await;

    let proof_hash = [0x99u8; 32];
    publisher
        .publish_checkpoint(
            "session-resume",
            proof_hash,
            1000,
            2000,
            &private_key,
            &mock,
        )
        .await?;

    // Verify new checkpoint was added at index 1
    let index_bytes = mock.get(index_path).await?;
    let index: CheckpointIndex = serde_json::from_slice(&index_bytes)?;

    assert_eq!(index.checkpoints.len(), 2);
    assert_eq!(index.checkpoints[0].index, 0); // Original
    assert_eq!(index.checkpoints[1].index, 1); // New
    assert_eq!(index.checkpoints[1].token_range, [1000, 2000]);

    Ok(())
}

// ==================== Cleanup Integration Tests ====================

#[tokio::test]
async fn test_cleanup_deletes_all_checkpoint_data() -> Result<()> {
    let mock = MockS5Backend::new();
    let publisher = CheckpointPublisher::new("0xhostclean".to_string());
    let private_key = generate_test_private_key();

    // Create checkpoints
    publisher
        .buffer_message(
            "session-clean",
            CheckpointMessage::new_user("Message 1".to_string(), 100),
        )
        .await;
    publisher
        .publish_checkpoint("session-clean", [0x11u8; 32], 0, 1000, &private_key, &mock)
        .await?;

    publisher
        .buffer_message(
            "session-clean",
            CheckpointMessage::new_user("Message 2".to_string(), 1100),
        )
        .await;
    publisher
        .publish_checkpoint(
            "session-clean",
            [0x22u8; 32],
            1000,
            2000,
            &private_key,
            &mock,
        )
        .await?;

    // Verify data exists
    let index_path = "home/checkpoints/0xhostclean/session-clean/index.json";
    assert!(mock.get(index_path).await.is_ok());

    // Run cleanup for cancelled session
    let result = cleanup_checkpoints(
        &mock,
        "0xhostclean",
        "session-clean",
        SessionState::Cancelled,
    )
    .await?;

    match result {
        CleanupResult::Deleted { deltas_removed } => {
            assert_eq!(deltas_removed, 2);
        }
        _ => panic!("Expected Deleted result"),
    }

    // Verify data was deleted
    assert!(mock.get(index_path).await.is_err());

    Ok(())
}

// ==================== Signature Verification Tests ====================

#[tokio::test]
async fn test_checkpoint_signatures_verifiable() -> Result<()> {
    let mock = MockS5Backend::new();
    let publisher = CheckpointPublisher::new("0xhostsig".to_string());
    let private_key = generate_test_private_key();

    publisher
        .buffer_message(
            "session-sig",
            CheckpointMessage::new_user("Test".to_string(), 100),
        )
        .await;

    publisher
        .publish_checkpoint("session-sig", [0xABu8; 32], 0, 1000, &private_key, &mock)
        .await?;

    // Fetch and verify delta signature
    let delta_path = "home/checkpoints/0xhostsig/session-sig/delta_0.json";
    let delta_bytes = mock.get(delta_path).await?;
    let delta: CheckpointDelta = serde_json::from_slice(&delta_bytes)?;

    // Verify signature format
    assert!(delta.host_signature.starts_with("0x"));
    assert_eq!(delta.host_signature.len(), 132);

    // Verify v value is 27 or 28 (EIP-191)
    let sig_bytes = hex::decode(&delta.host_signature[2..])?;
    let v = sig_bytes[64];
    assert!(v == 27 || v == 28, "v must be 27 or 28, got {}", v);

    Ok(())
}

#[tokio::test]
async fn test_json_keys_alphabetically_sorted() -> Result<()> {
    // SDK requires alphabetically sorted keys for signature verification
    let mock = MockS5Backend::new();
    let publisher = CheckpointPublisher::new("0xhostsort".to_string());
    let private_key = generate_test_private_key();

    publisher
        .buffer_message(
            "session-sort",
            CheckpointMessage::new_user("Test".to_string(), 100),
        )
        .await;

    publisher
        .publish_checkpoint("session-sort", [0xCDu8; 32], 0, 1000, &private_key, &mock)
        .await?;

    // Fetch delta JSON
    let delta_path = "home/checkpoints/0xhostsort/session-sort/delta_0.json";
    let delta_bytes = mock.get(delta_path).await?;
    let delta_json = String::from_utf8(delta_bytes)?;

    // Verify keys appear in alphabetical order in the JSON
    // For CheckpointMessage: content, metadata, role, timestamp
    let content_pos = delta_json.find("\"content\"").unwrap();
    let role_pos = delta_json.find("\"role\"").unwrap();
    let timestamp_pos = delta_json.find("\"timestamp\"").unwrap();

    assert!(
        content_pos < role_pos,
        "content should come before role alphabetically"
    );
    assert!(
        role_pos < timestamp_pos,
        "role should come before timestamp alphabetically"
    );

    Ok(())
}
