// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::api::websocket::auth::{
    AuthConfig, AuthError, AuthResult, Authenticator, JobVerifier, Permission, SessionToken,
};
use std::collections::HashSet;
use std::time::Duration;

#[tokio::test]
async fn test_job_id_verification() {
    // Mock authentication setup
    let config = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: false,
        token_expiry: Duration::from_secs(3600),
        jwt_secret: "test_secret_minimum_32_characters_long".to_string(),
        max_sessions_per_user: 5,
    };

    let auth = Authenticator::new_mock(config);

    // Valid job ID from blockchain
    let result = auth.verify_job_id(12345).await;
    assert!(result.is_ok());

    // Invalid job ID
    let result = auth.verify_job_id(99999).await;
    assert!(matches!(result, Err(AuthError::JobNotFound)));

    // Expired job
    let result = auth.verify_job_id(11111).await;
    assert!(matches!(result, Err(AuthError::JobExpired)));
}

#[tokio::test]
async fn test_session_authentication_tokens() {
    let config = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: false,
        token_expiry: Duration::from_secs(3600),
        jwt_secret: "test_secret_minimum_32_characters_long".to_string(),
        max_sessions_per_user: 5,
    };

    let auth = Authenticator::new_mock(config);

    // Create session token
    let token = auth
        .create_session_token(
            "session-1",
            12345,
            vec![Permission::Read, Permission::Write],
        )
        .await
        .unwrap();

    assert!(!token.token.is_empty());
    assert_eq!(token.session_id, "session-1");
    assert_eq!(token.job_id, 12345);

    // Verify token
    let verified = auth.verify_token(&token.token).await.unwrap();
    assert_eq!(verified.session_id, "session-1");
    assert_eq!(verified.job_id, 12345);
    assert!(verified.permissions.contains(&Permission::Read));
    assert!(verified.permissions.contains(&Permission::Write));
}

#[tokio::test]
async fn test_authentication_failures() {
    let config = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: true,
        token_expiry: Duration::from_secs(3600),
        jwt_secret: "test_secret_minimum_32_characters_long".to_string(),
        max_sessions_per_user: 5,
    };

    let auth = Authenticator::new_mock(config);

    // Invalid token format
    let result = auth.verify_token("invalid-token").await;
    assert!(matches!(result, Err(AuthError::InvalidToken)));

    // Expired token
    let expired_token = auth.create_expired_token("session-1", 12345).await;
    let result = auth.verify_token(&expired_token).await;
    assert!(matches!(result, Err(AuthError::TokenExpired)));

    // Tampered token
    let token = auth
        .create_session_token("session-1", 12345, vec![])
        .await
        .unwrap();
    let tampered = format!("{}tampered", token.token);
    let result = auth.verify_token(&tampered).await;
    assert!(matches!(result, Err(AuthError::InvalidSignature)));
}

#[tokio::test]
async fn test_permission_checks() {
    let auth = Authenticator::new_mock(AuthConfig::default());

    let token = auth
        .create_session_token("session-1", 12345, vec![Permission::Read])
        .await
        .unwrap();

    // Check allowed permission
    assert!(auth
        .check_permission(&token.token, Permission::Read)
        .await
        .unwrap());

    // Check denied permission
    assert!(!auth
        .check_permission(&token.token, Permission::Write)
        .await
        .unwrap());
    assert!(!auth
        .check_permission(&token.token, Permission::Admin)
        .await
        .unwrap());
}

#[tokio::test]
async fn test_authentication_caching() {
    let config = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: false,
        token_expiry: Duration::from_secs(3600),
        jwt_secret: "test_secret_minimum_32_characters_long".to_string(),
        max_sessions_per_user: 5,
    };

    let auth = Authenticator::with_cache(config, Duration::from_secs(60));

    // First verification hits blockchain
    let start = std::time::Instant::now();
    auth.verify_job_id(12345).await.unwrap();
    let first_duration = start.elapsed();

    // Second verification uses cache (much faster)
    let start = std::time::Instant::now();
    auth.verify_job_id(12345).await.unwrap();
    let cached_duration = start.elapsed();

    assert!(cached_duration < first_duration / 2);

    // Cache stats
    let stats = auth.cache_stats().await;
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.entries, 1);
}

#[tokio::test]
async fn test_jwt_token_validation() {
    let auth = Authenticator::new_mock(AuthConfig::default());

    // Create JWT token
    let claims = auth.create_jwt_claims(
        "session-1",
        12345,
        vec![Permission::Read, Permission::Write],
    );

    let jwt = auth.encode_jwt(&claims).await.unwrap();

    // Validate JWT
    let decoded = auth.decode_jwt(&jwt).await.unwrap();
    assert_eq!(decoded.session_id, "session-1");
    assert_eq!(decoded.job_id, 12345);
    assert_eq!(decoded.permissions.len(), 2);
}

#[tokio::test]
async fn test_signature_verification() {
    let config = AuthConfig {
        enabled: true,
        verify_job_id: false,
        require_signature: true,
        token_expiry: Duration::from_secs(3600),
        jwt_secret: "test_secret_minimum_32_characters_long".to_string(),
        max_sessions_per_user: 5,
    };

    let auth = Authenticator::new_mock(config);

    // Create signed request
    let message = "session_init:session-1:12345";
    let signature = auth.sign_message(message).await;

    // Verify signature
    assert!(auth.verify_signature(message, &signature).await.unwrap());

    // Invalid signature
    assert!(!auth.verify_signature(message, "invalid-sig").await.unwrap());

    // Different message
    assert!(!auth
        .verify_signature("different", &signature)
        .await
        .unwrap());
}

#[tokio::test]
async fn test_multi_factor_authentication() {
    let auth = Authenticator::with_mfa(AuthConfig::default());

    // First factor: job ID
    auth.verify_job_id(12345).await.unwrap();

    // Second factor: signature
    let signature = auth.sign_message("session-1:12345").await;
    auth.verify_signature("session-1:12345", &signature)
        .await
        .unwrap();

    // Create MFA token
    let token = auth
        .create_mfa_token("session-1", 12345, &signature)
        .await
        .unwrap();

    // Verify MFA token
    let verified = auth.verify_mfa_token(&token).await.unwrap();
    assert!(verified.multi_factor_verified);
}

#[tokio::test]
async fn test_role_based_access_control() {
    let auth = Authenticator::new_mock(AuthConfig::default());

    // Create tokens with different roles
    let user_token = auth
        .create_session_token("user-session", 12345, vec![Permission::Read])
        .await
        .unwrap();

    let host_token = auth
        .create_session_token(
            "host-session",
            12345,
            vec![Permission::Read, Permission::Write, Permission::Execute],
        )
        .await
        .unwrap();

    let admin_token = auth
        .create_session_token("admin-session", 12345, vec![Permission::Admin])
        .await
        .unwrap();

    // Check role-based permissions
    assert!(auth.is_user(&user_token.token).await.unwrap());
    assert!(!auth.is_host(&user_token.token).await.unwrap());
    assert!(!auth.is_admin(&user_token.token).await.unwrap());

    assert!(auth.is_host(&host_token.token).await.unwrap());
    assert!(!auth.is_admin(&host_token.token).await.unwrap());

    assert!(auth.is_admin(&admin_token.token).await.unwrap());
}

#[tokio::test]
async fn test_auth_disabled_mode() {
    let config = AuthConfig {
        enabled: false,
        verify_job_id: true,
        require_signature: true,
        token_expiry: Duration::from_secs(3600),
        jwt_secret: "test_secret_minimum_32_characters_long".to_string(),
        max_sessions_per_user: 5,
    };

    let auth = Authenticator::new_mock(config);

    // All auth checks should pass when disabled
    assert!(auth.verify_job_id(99999).await.is_ok());
    assert!(auth.verify_token("any-token").await.is_ok());
    assert!(auth.verify_signature("msg", "sig").await.unwrap());
}
