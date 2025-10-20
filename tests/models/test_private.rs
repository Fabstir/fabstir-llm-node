// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// tests/models/test_private.rs - Private model hosting tests

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use fabstir_llm_node::models::{
    AccessControl, AccessLevel, AccessToken, AuditLog, EncryptionConfig, ExportPolicy, LicenseType,
    ModelLicense, ModelOwner, ModelVisibility, PrivateModel, PrivateModelConfig,
    PrivateModelManager, PrivateModelRegistry, RateLimits, SharingSettings, StorageIsolation,
    UsagePolicy,
};
use futures;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tempfile::TempDir;
use tokio;

async fn create_test_manager() -> Result<PrivateModelManager> {
    let config = PrivateModelConfig {
        enable_private_models: true,
        encryption_enabled: true,
        storage_backend: "isolated".to_string(),
        max_private_models_per_user: 10,
        enable_usage_tracking: true,
        enable_audit_logging: true,
        token_expiry_hours: 24,
        enable_model_sharing: true,
    };

    PrivateModelManager::new(config).await
}

#[tokio::test]
async fn test_private_model_creation() {
    let manager = create_test_manager().await.unwrap();

    // Create a private model
    let owner = ModelOwner {
        id: "user123".to_string(),
        organization: Some("acme-corp".to_string()),
        email: "owner@acme.com".to_string(),
    };

    let model = PrivateModel {
        id: String::new(),
        name: "proprietary-llm-v1".to_string(),
        owner: owner.clone(),
        visibility: ModelVisibility::Private,
        created_at: Utc::now(),
        model_path: PathBuf::from("/models/private/proprietary-llm-v1"),
        encrypted: true,
        size_bytes: 5_000_000_000, // 5GB
    };

    let model_id = manager.create_private_model(model, &owner).await.unwrap();
    assert!(!model_id.is_empty());

    // Verify only owner can access
    let retrieved = manager.get_private_model(&model_id, &owner).await.unwrap();
    assert_eq!(retrieved.name, "proprietary-llm-v1");

    // Verify others cannot access
    let other_owner = ModelOwner {
        id: "user456".to_string(),
        organization: None,
        email: "other@example.com".to_string(),
    };
    let result = manager.get_private_model(&model_id, &other_owner).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_model_encryption() {
    let manager = create_test_manager().await.unwrap();
    let temp_dir = TempDir::new().unwrap();

    // Create test model file
    let model_path = temp_dir.path().join("model.bin");
    std::fs::write(&model_path, b"model weights data").unwrap();

    let owner = ModelOwner::new("user123");

    // Encrypt model during upload
    let encryption_config = EncryptionConfig {
        algorithm: "AES-256-GCM".to_string(),
        key_derivation: "PBKDF2".to_string(),
        iterations: 100_000,
    };

    let encrypted_path = manager
        .encrypt_model(&model_path, &owner, &encryption_config)
        .await
        .unwrap();

    // Verify file is encrypted
    let encrypted_data = std::fs::read(&encrypted_path).unwrap();
    let original_data = std::fs::read(&model_path).unwrap();
    assert_ne!(encrypted_data, original_data);

    // Decrypt for authorized user
    let decrypted_path = manager
        .decrypt_model(&encrypted_path, &owner)
        .await
        .unwrap();

    let decrypted_data = std::fs::read(&decrypted_path).unwrap();
    assert_eq!(decrypted_data, original_data);
}

#[tokio::test]
async fn test_access_token_generation() {
    let manager = create_test_manager().await.unwrap();
    let owner = ModelOwner::new("user123");

    // Create private model
    let model_id = manager
        .create_private_model(PrivateModel::new("test-model", owner.clone()), &owner)
        .await
        .unwrap();

    // Generate access token
    let token = manager
        .generate_access_token(
            &model_id,
            &owner,
            AccessLevel::ReadOnly,
            Duration::hours(24),
        )
        .await
        .unwrap();

    assert!(!token.value.is_empty());
    assert_eq!(token.access_level, AccessLevel::ReadOnly);
    assert!(token.expires_at > Utc::now());

    // Validate token
    let validated = manager.validate_token(&token.value).await.unwrap();
    assert_eq!(validated.model_id, model_id);
    assert_eq!(validated.access_level, AccessLevel::ReadOnly);
}

#[tokio::test]
async fn test_model_sharing() {
    let manager = create_test_manager().await.unwrap();

    let owner = ModelOwner::new("user123");
    let collaborator = ModelOwner::new("user456");

    // Create private model
    let model_id = manager
        .create_private_model(PrivateModel::new("shared-model", owner.clone()), &owner)
        .await
        .unwrap();

    // Share with collaborator
    let sharing = SharingSettings {
        shared_with: vec![collaborator.id.clone()],
        access_level: AccessLevel::ReadOnly,
        expires_at: Some(Utc::now() + Duration::days(7)),
        can_reshare: false,
    };

    manager
        .share_model(&model_id, &owner, sharing)
        .await
        .unwrap();

    // Verify collaborator can access
    let result = manager.get_private_model(&model_id, &collaborator).await;
    assert!(result.is_ok());

    // Verify collaborator cannot modify
    let update_result = manager
        .update_model(&model_id, &collaborator, HashMap::new())
        .await;
    assert!(update_result.is_err());
}

#[tokio::test]
async fn test_usage_tracking() {
    let manager = create_test_manager().await.unwrap();
    let owner = ModelOwner::new("user123");

    // Create model with usage policy
    let model_id = manager
        .create_private_model(PrivateModel::new("metered-model", owner.clone()), &owner)
        .await
        .unwrap();

    let policy = UsagePolicy {
        max_requests_per_day: Some(1000),
        max_tokens_per_request: Some(4096),
        allowed_purposes: vec!["research".to_string(), "development".to_string()],
        geographic_restrictions: Some(vec!["US".to_string(), "EU".to_string()]),
    };

    manager
        .set_usage_policy(&model_id, &owner, policy)
        .await
        .unwrap();

    // Track usage
    for i in 0..5 {
        manager
            .track_usage(
                &model_id,
                &owner,
                "inference",
                HashMap::from([
                    ("tokens".to_string(), 100),
                    ("duration_ms".to_string(), 250),
                ]),
            )
            .await
            .unwrap();
    }

    // Get usage stats
    let stats = manager
        .get_usage_stats(
            &model_id,
            &owner,
            Utc::now() - Duration::hours(1),
            Utc::now(),
        )
        .await
        .unwrap();

    assert_eq!(stats.total_requests, 5);
    assert_eq!(stats.total_tokens, 500);
}

#[tokio::test]
async fn test_model_licensing() {
    let manager = create_test_manager().await.unwrap();
    let owner = ModelOwner::new("user123");

    // Create model with license
    let model_id = manager
        .create_private_model(PrivateModel::new("licensed-model", owner.clone()), &owner)
        .await
        .unwrap();

    let license = ModelLicense {
        license_type: LicenseType::Commercial,
        terms: "Proprietary license. Usage requires payment.".to_string(),
        restrictions: vec![
            "No redistribution".to_string(),
            "No derivative works".to_string(),
        ],
        attribution_required: true,
        fee_structure: Some("$0.01 per 1000 tokens".to_string()),
    };

    manager
        .set_license(&model_id, &owner, license.clone())
        .await
        .unwrap();

    // Accept license before use
    let user = ModelOwner::new("user456");
    let acceptance = manager.accept_license(&model_id, &user).await.unwrap();
    assert!(acceptance.accepted);
    assert_eq!(acceptance.license_version, "1.0");

    // Verify license is enforced
    let can_use = manager
        .check_license_compliance(&model_id, &user)
        .await
        .unwrap();
    assert!(can_use);
}

#[tokio::test]
async fn test_isolated_inference() {
    let manager = create_test_manager().await.unwrap();
    let owner = ModelOwner::new("company-a");

    // Create private model
    let model_id = manager
        .create_private_model(PrivateModel::new("isolated-model", owner.clone()), &owner)
        .await
        .unwrap();

    // Create isolated inference session
    let isolation = StorageIsolation {
        separate_process: true,
        memory_limit_gb: 8,
        no_network_access: true,
        temp_storage_only: true,
        cleanup_after_use: true,
    };

    let session = manager
        .create_isolated_session(&model_id, &owner, isolation)
        .await
        .unwrap();

    // Run inference in isolation
    let result = session
        .generate("Sensitive prompt", Default::default())
        .await
        .unwrap();

    assert!(!result.text.is_empty());
    assert!(result.metadata.contains_key("isolation_id"));

    // Verify cleanup
    session.cleanup().await.unwrap();
    assert!(!session.is_active().await);
}

#[tokio::test]
async fn test_audit_logging() {
    let manager = create_test_manager().await.unwrap();
    let owner = ModelOwner::new("user123");

    // Create model
    let model_id = manager
        .create_private_model(PrivateModel::new("audited-model", owner.clone()), &owner)
        .await
        .unwrap();

    // Perform various actions
    manager
        .update_model(
            &model_id,
            &owner,
            HashMap::from([("description".to_string(), "Updated description".to_string())]),
        )
        .await
        .unwrap();

    let token = manager
        .generate_access_token(
            &model_id,
            &owner,
            AccessLevel::FullAccess,
            Duration::hours(1),
        )
        .await
        .unwrap();

    // Query audit log
    let logs = manager
        .get_audit_logs(
            &model_id,
            &owner,
            Utc::now() - Duration::hours(1),
            Utc::now(),
        )
        .await
        .unwrap();

    assert!(logs.len() >= 3); // create, update, token generation
    assert!(logs.iter().any(|log| log.action == "model_created"));
    assert!(logs.iter().any(|log| log.action == "model_updated"));
    assert!(logs.iter().any(|log| log.action == "token_generated"));
}

#[tokio::test]
async fn test_private_model_registry() {
    let manager = create_test_manager().await.unwrap();

    // Create models for different organizations
    let orgs = vec!["acme-corp", "techno-inc", "ai-labs"];
    for org in &orgs {
        let owner = ModelOwner {
            id: format!("{}-admin", org),
            organization: Some(org.to_string()),
            email: format!("admin@{}.com", org),
        };

        for i in 0..3 {
            manager
                .create_private_model(
                    PrivateModel::new(&format!("{}-model-{}", org, i), owner.clone()),
                    &owner,
                )
                .await
                .unwrap();
        }
    }

    // List models by organization
    let acme_models = manager.list_organization_models("acme-corp").await.unwrap();
    assert_eq!(acme_models.len(), 3);

    // Search private models (only returns authorized)
    let owner = ModelOwner::new("acme-corp-admin");
    let search_results = manager
        .search_private_models("model", &owner, 10)
        .await
        .unwrap();

    assert_eq!(search_results.len(), 3); // Only sees own org's models
}

#[tokio::test]
async fn test_rate_limiting() {
    let manager = create_test_manager().await.unwrap();
    let owner = ModelOwner::new("user123");

    // Create model with rate limits
    let model_id = manager
        .create_private_model(PrivateModel::new("rate-limited", owner.clone()), &owner)
        .await
        .unwrap();

    let limits = RateLimits {
        requests_per_minute: 10,
        requests_per_hour: 100,
        tokens_per_minute: 10_000,
        concurrent_requests: 2,
    };

    manager
        .set_rate_limits(&model_id, &owner, limits)
        .await
        .unwrap();

    // Test rate limiting
    let mut handles = vec![];
    for i in 0..15 {
        let mgr = manager.clone();
        let mid = model_id.clone();
        let usr = owner.clone();

        let handle = tokio::spawn(async move { mgr.check_rate_limit(&mid, &usr).await });
        handles.push(handle);
    }

    let results: Vec<_> = futures::future::join_all(handles).await;
    let allowed = results
        .iter()
        .filter(|r| r.as_ref().unwrap().is_ok())
        .count();
    let denied = results
        .iter()
        .filter(|r| r.as_ref().unwrap().is_err())
        .count();

    assert!(allowed <= 10); // Should respect per-minute limit
    assert!(denied > 0); // Some should be rate limited
}

#[tokio::test]
async fn test_model_deletion() {
    let manager = create_test_manager().await.unwrap();
    let owner = ModelOwner::new("user123");

    // Create model
    let model_id = manager
        .create_private_model(PrivateModel::new("temp-model", owner.clone()), &owner)
        .await
        .unwrap();

    // Share with another user
    let other_user = ModelOwner::new("user456");
    manager
        .share_model(
            &model_id,
            &owner,
            SharingSettings {
                shared_with: vec![other_user.id.clone()],
                access_level: AccessLevel::ReadOnly,
                expires_at: None,
                can_reshare: false,
            },
        )
        .await
        .unwrap();

    // Delete model
    manager
        .delete_private_model(&model_id, &owner)
        .await
        .unwrap();

    // Verify owner cannot access
    let result = manager.get_private_model(&model_id, &owner).await;
    assert!(result.is_err());

    // Verify shared user cannot access
    let result = manager.get_private_model(&model_id, &other_user).await;
    assert!(result.is_err());

    // Verify model is marked as deleted in audit log
    let logs = manager
        .get_audit_logs(
            &model_id,
            &owner,
            Utc::now() - Duration::minutes(5),
            Utc::now(),
        )
        .await
        .unwrap();

    assert!(logs.iter().any(|log| log.action == "model_deleted"));
}

#[tokio::test]
async fn test_model_export_restrictions() {
    let manager = create_test_manager().await.unwrap();
    let owner = ModelOwner::new("user123");

    // Create model with export restrictions
    let model_id = manager
        .create_private_model(PrivateModel::new("no-export-model", owner.clone()), &owner)
        .await
        .unwrap();

    manager
        .set_export_policy(
            &model_id,
            &owner,
            ExportPolicy {
                allow_download: false,
                allow_api_access_only: true,
                watermark_outputs: true,
                require_attribution: true,
            },
        )
        .await
        .unwrap();

    // Try to export (should fail)
    let result = manager.export_model(&model_id, &owner).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Export not allowed"));

    // API access should work
    let session = manager.create_api_session(&model_id, &owner).await.unwrap();
    let response = session.generate("test", Default::default()).await.unwrap();

    // Verify watermark in output
    assert!(response.metadata.contains_key("watermark"));
    assert!(response.metadata.contains_key("attribution_required"));
}

#[tokio::test]
async fn test_multi_tenant_isolation() {
    let manager = create_test_manager().await.unwrap();

    // Create models for different tenants
    let tenant_a = ModelOwner::new("tenant-a");
    let tenant_b = ModelOwner::new("tenant-b");

    let model_a = manager
        .create_private_model(
            PrivateModel::new("tenant-a-model", tenant_a.clone()),
            &tenant_a,
        )
        .await
        .unwrap();

    let model_b = manager
        .create_private_model(
            PrivateModel::new("tenant-b-model", tenant_b.clone()),
            &tenant_b,
        )
        .await
        .unwrap();

    // Verify complete isolation
    assert!(manager
        .get_private_model(&model_a, &tenant_b)
        .await
        .is_err());
    assert!(manager
        .get_private_model(&model_b, &tenant_a)
        .await
        .is_err());

    // Verify separate storage paths
    let info_a = manager.get_storage_info(&model_a, &tenant_a).await.unwrap();
    let info_b = manager.get_storage_info(&model_b, &tenant_b).await.unwrap();

    assert!(!info_a.path.starts_with(&info_b.path));
    assert!(!info_b.path.starts_with(&info_a.path));
}
