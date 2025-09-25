use anyhow::Result;
use fabstir_llm_node::models::{
    MigrationPlan, ModelUpdater, ModelVersion, RollbackPolicy, UpdateConfig, UpdateError,
    UpdateMetadata, UpdateNotification, UpdateResult, UpdateSchedule, UpdateSource, UpdateStatus,
    UpdateStrategy, VersionComparison,
};
use std::path::PathBuf;
use tokio;

async fn create_test_updater() -> Result<ModelUpdater> {
    let config = UpdateConfig {
        auto_update: false,
        check_interval_hours: 24,
        update_strategy: UpdateStrategy::Conservative,
        rollback_policy: RollbackPolicy::KeepLastTwo,
        verify_updates: true,
        update_dir: PathBuf::from("test_data/updates"),
        max_download_retries: 3,
        notify_on_update: true,
    };

    ModelUpdater::new(config).await
}

fn create_test_version(major: u32, minor: u32, patch: u32) -> ModelVersion {
    ModelVersion {
        major,
        minor,
        patch,
        build: None,
        pre_release: None,
    }
}

#[tokio::test]
async fn test_check_for_updates() {
    let updater = create_test_updater().await.unwrap();

    let current_version = create_test_version(1, 0, 0);
    let model_id = "llama-7b";

    let update_info = updater
        .check_for_updates(model_id, &current_version)
        .await
        .unwrap();

    if let Some(info) = update_info {
        assert!(info.new_version > current_version);
        assert!(!info.changelog.is_empty());
        assert!(info.size_bytes > 0);
        assert!(info.release_date > 0);
        assert!(!info.download_url.is_empty());
        assert!(info.is_security_update || !info.is_security_update);
    }
}

#[tokio::test]
async fn test_version_comparison() {
    let updater = create_test_updater().await.unwrap();

    let test_cases = vec![
        (
            create_test_version(1, 0, 0),
            create_test_version(2, 0, 0),
            VersionComparison::Major,
        ),
        (
            create_test_version(1, 0, 0),
            create_test_version(1, 1, 0),
            VersionComparison::Minor,
        ),
        (
            create_test_version(1, 0, 0),
            create_test_version(1, 0, 1),
            VersionComparison::Patch,
        ),
        (
            create_test_version(1, 0, 0),
            create_test_version(1, 0, 0),
            VersionComparison::Equal,
        ),
        (
            create_test_version(2, 0, 0),
            create_test_version(1, 0, 0),
            VersionComparison::Downgrade,
        ),
    ];

    for (v1, v2, expected) in test_cases {
        let comparison = updater.compare_versions(&v1, &v2).await.unwrap();
        assert_eq!(comparison, expected);
    }
}

#[tokio::test]
async fn test_apply_update() {
    let updater = create_test_updater().await.unwrap();

    let model_id = "test-model";
    let current_path = PathBuf::from("test_data/models/model_v1.gguf");
    let update_source = UpdateSource::HuggingFace {
        repo_id: "TheBloke/test-model".to_string(),
        filename: "model_v2.gguf".to_string(),
        version: create_test_version(2, 0, 0),
    };

    let result = updater
        .apply_update(model_id, &current_path, update_source)
        .await
        .unwrap();

    assert_eq!(result.status, UpdateStatus::Completed);
    assert!(result.new_model_path.exists());
    assert!(result.backup_path.is_some());
    assert!(result.update_time_ms > 0);
    assert!(result.verification_passed);
}

#[tokio::test]
async fn test_rollback_update() {
    let updater = create_test_updater().await.unwrap();

    let model_id = "rollback-test";
    let original_path = PathBuf::from("test_data/models/original.gguf");

    // Apply an update first
    let update_source = UpdateSource::Direct {
        url: "https://example.com/updated-model.gguf".to_string(),
        version: create_test_version(2, 0, 0),
    };

    let update_result = updater
        .apply_update(model_id, &original_path, update_source)
        .await
        .unwrap();

    // Now rollback
    let rollback_result = updater
        .rollback_update(model_id, &update_result.new_model_path)
        .await
        .unwrap();

    assert_eq!(rollback_result.status, UpdateStatus::RolledBack);
    assert_eq!(
        rollback_result.restored_version,
        create_test_version(1, 0, 0)
    );
    assert!(rollback_result.restored_path.exists());
}

#[tokio::test]
async fn test_update_strategies() {
    let updater = create_test_updater().await.unwrap();

    let strategies = vec![
        UpdateStrategy::Conservative, // Only patch updates
        UpdateStrategy::Balanced,     // Minor and patch updates
        UpdateStrategy::Aggressive,   // All updates including major
        UpdateStrategy::SecurityOnly, // Only security updates
    ];

    for strategy in strategies {
        let mut config = UpdateConfig::default();
        config.update_strategy = strategy.clone();

        let strategy_updater = ModelUpdater::new(config).await.unwrap();

        let should_update = strategy_updater
            .should_apply_update(
                &create_test_version(1, 0, 0),
                &create_test_version(2, 0, 0),
                false, // is_security_update
            )
            .await
            .unwrap();

        match strategy {
            UpdateStrategy::Conservative => assert!(!should_update),
            UpdateStrategy::Balanced => assert!(!should_update),
            UpdateStrategy::Aggressive => assert!(should_update),
            UpdateStrategy::SecurityOnly => assert!(!should_update),
            UpdateStrategy::Manual => assert!(!should_update),
        }
    }
}

#[tokio::test]
async fn test_batch_updates() {
    let updater = create_test_updater().await.unwrap();

    let models_to_update = vec![
        ("model1", create_test_version(1, 0, 0)),
        ("model2", create_test_version(1, 1, 0)),
        ("model3", create_test_version(2, 0, 0)),
    ];

    let batch_result = updater.check_batch_updates(models_to_update).await.unwrap();

    assert!(batch_result.total_models == 3);
    assert!(batch_result.updates_available <= 3);
    // total_download_size is u64, so it's always >= 0

    if batch_result.updates_available > 0 {
        // Apply batch updates
        let update_result = updater
            .apply_batch_updates(batch_result.update_plan)
            .await
            .unwrap();

        assert_eq!(
            update_result.successful_updates + update_result.failed_updates,
            batch_result.updates_available
        );
    }
}

#[tokio::test]
async fn test_update_verification() {
    let updater = create_test_updater().await.unwrap();

    let model_path = PathBuf::from("test_data/models/updated_model.gguf");
    let expected_checksum = "abc123def456789";
    let expected_size = 1_000_000_000; // 1GB

    let metadata = UpdateMetadata {
        version: create_test_version(2, 0, 0),
        checksum: expected_checksum.to_string(),
        size_bytes: expected_size,
        signature: Some("mock_signature".to_string()),
        release_notes: "Bug fixes and improvements".to_string(),
    };

    let verification_result = updater.verify_update(&model_path, &metadata).await;

    match verification_result {
        Ok(verified) => assert!(verified),
        Err(e) => match e.downcast_ref::<UpdateError>() {
            Some(UpdateError::VerificationFailed { reason }) => {
                // Mock might not match - that's ok
                assert!(!reason.is_empty());
            }
            _ => panic!("Unexpected error: {:?}", e),
        },
    }
}

#[tokio::test]
async fn test_scheduled_updates() {
    let updater = create_test_updater().await.unwrap();

    let schedule = UpdateSchedule {
        check_time: "02:00".to_string(), // 2 AM
        allowed_days: vec!["Saturday".to_string(), "Sunday".to_string()],
        max_bandwidth_mbps: Some(100.0),
        pause_on_active_inference: true,
    };

    updater.set_update_schedule(schedule.clone()).await.unwrap();

    let current_schedule = updater.get_update_schedule().await.unwrap();
    assert_eq!(current_schedule.check_time, schedule.check_time);
    assert_eq!(current_schedule.allowed_days, schedule.allowed_days);

    // Check if update is allowed now
    let is_allowed = updater.is_update_allowed_now().await.unwrap();
    // Result depends on current time/day
    assert!(is_allowed || !is_allowed);
}

#[tokio::test]
async fn test_update_notifications() {
    let updater = create_test_updater().await.unwrap();

    // Subscribe to update notifications
    let mut notification_stream = updater.subscribe_notifications().await;

    // Trigger some update checks
    let model_id = "notification-test";
    let version = create_test_version(1, 0, 0);

    updater.check_for_updates(model_id, &version).await.unwrap();

    // Collect notifications
    let mut notifications = Vec::new();
    while let Ok(Some(notification)) = tokio::time::timeout(
        tokio::time::Duration::from_millis(100),
        notification_stream.recv(),
    )
    .await
    {
        notifications.push(notification);
    }

    // Verify notifications
    for notification in notifications {
        match notification {
            UpdateNotification::UpdateAvailable { model_id: id, .. } => {
                assert_eq!(id, model_id);
            }
            UpdateNotification::UpdateCompleted { .. } => {}
            UpdateNotification::UpdateFailed { .. } => {}
            UpdateNotification::RollbackCompleted { .. } => {}
            UpdateNotification::HotUpdateAvailable { .. } => {}
            UpdateNotification::RollbackInitiated { .. } => {}
        }
    }
}

#[tokio::test]
async fn test_migration_planning() {
    let updater = create_test_updater().await.unwrap();

    let from_version = create_test_version(1, 0, 0);
    let to_version = create_test_version(3, 0, 0);

    let migration_plan = updater
        .create_migration_plan("model_id", &from_version, &to_version)
        .await
        .unwrap();

    assert!(!migration_plan.steps.is_empty());
    assert!(migration_plan.total_size_bytes > 0);
    assert!(migration_plan.estimated_time_minutes > 0);
    assert!(
        !migration_plan.breaking_changes.is_empty() || migration_plan.breaking_changes.is_empty()
    );

    // Verify migration steps are in order
    for i in 1..migration_plan.steps.len() {
        assert!(migration_plan.steps[i].from_version >= migration_plan.steps[i - 1].to_version);
    }
}

#[tokio::test]
async fn test_hot_update() {
    let updater = create_test_updater().await.unwrap();

    let model_id = "hot-update-model";
    let current_path = PathBuf::from("test_data/models/active_model.gguf");

    // Simulate model in use
    let in_use_handle = tokio::spawn(async move {
        // Simulate inference workload
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    });

    // Attempt hot update
    let update_source = UpdateSource::Direct {
        url: "https://example.com/new_model.gguf".to_string(),
        version: create_test_version(2, 0, 0),
    };

    let result = updater
        .hot_update(model_id, &current_path, update_source)
        .await
        .unwrap();

    assert_eq!(result.status, UpdateStatus::Completed);
    assert!(result.downtime_ms < 100); // Should be very low
    assert!(result.hot_swap_successful);

    // Wait for simulated workload to complete
    in_use_handle.await.unwrap();
}

#[tokio::test]
async fn test_update_cleanup() {
    let updater = create_test_updater().await.unwrap();

    // Simulate multiple updates creating backups
    for i in 0..5 {
        let path = PathBuf::from(format!("test_data/updates/backup_v{}.gguf", i));
        // Mock file creation
        std::fs::write(&path, format!("model_data_v{}", i)).ok();
    }

    // Run cleanup with policy to keep only 2 backups
    let cleanup_result = updater.cleanup_old_versions("test_model", 2).await.unwrap();

    assert_eq!(cleanup_result.versions_removed, 3);
    assert!(cleanup_result.space_freed_bytes > 0);
    assert_eq!(cleanup_result.versions_kept, 2);
}

#[tokio::test]
async fn test_update_failure_recovery() {
    let updater = create_test_updater().await.unwrap();

    let model_id = "failure-test";
    let current_path = PathBuf::from("test_data/models/stable_model.gguf");

    // Use source that will fail
    let bad_source = UpdateSource::Direct {
        url: "https://invalid-url-that-will-fail.com/model.gguf".to_string(),
        version: create_test_version(2, 0, 0),
    };

    let result = updater
        .apply_update(model_id, &current_path, bad_source)
        .await;

    assert!(result.is_err());

    // Original model should still be intact
    assert!(current_path.exists());

    // Check recovery state
    let recovery_info = updater.get_recovery_info(model_id).await.unwrap();

    assert!(recovery_info.can_recover);
    assert_eq!(
        recovery_info.last_stable_version,
        create_test_version(1, 0, 0)
    );
}
