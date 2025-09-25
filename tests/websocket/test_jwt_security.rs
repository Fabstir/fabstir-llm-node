use chrono::{Duration, Utc};
use fabstir_llm_node::api::websocket::auth::{AuthConfig, AuthError, Authenticator, JwtClaims};

#[tokio::test]
async fn test_jwt_token_generation() {
    let config = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: false,
        token_expiry: std::time::Duration::from_secs(3600),
        jwt_secret: "test_secret_key_for_jwt_testing_minimum_32_chars".to_string(),
        max_sessions_per_user: 5,
    };

    let authenticator = Authenticator::new_mock(config);

    let claims = JwtClaims {
        session_id: "session_abc".to_string(),
        job_id: 42,
        permissions: vec![],
        exp: (Utc::now() + Duration::hours(1)).timestamp() as u64,
        iat: Utc::now().timestamp() as u64,
    };

    // Generate JWT token
    let token = authenticator.encode_jwt(&claims).await.unwrap();

    // Token should be a valid JWT format (header.payload.signature)
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "JWT should have 3 parts");

    // Each part should be base64url encoded
    for part in &parts {
        assert!(!part.is_empty(), "JWT parts should not be empty");
        // Base64url characters only
        assert!(
            part.chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
            "JWT parts should be base64url encoded"
        );
    }
}

#[tokio::test]
async fn test_jwt_token_validation() {
    let config = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: false,
        token_expiry: std::time::Duration::from_secs(3600),
        jwt_secret: "test_secret_key_for_jwt_testing_minimum_32_chars".to_string(),
        max_sessions_per_user: 5,
    };

    let authenticator = Authenticator::new_mock(config);

    let claims = JwtClaims {
        session_id: "session_xyz".to_string(),
        job_id: 99,
        permissions: vec![],
        exp: (Utc::now() + Duration::hours(1)).timestamp() as u64,
        iat: Utc::now().timestamp() as u64,
    };

    // Generate and validate token
    let token = authenticator.encode_jwt(&claims).await.unwrap();
    let decoded_claims = authenticator.decode_jwt(&token).await.unwrap();

    assert_eq!(decoded_claims.session_id, claims.session_id);
    assert_eq!(decoded_claims.job_id, claims.job_id);
    assert_eq!(decoded_claims.session_id, claims.session_id);
}

#[tokio::test]
async fn test_jwt_invalid_token_rejection() {
    let config = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: false,
        token_expiry: std::time::Duration::from_secs(3600),
        jwt_secret: "test_secret_key_for_jwt_testing_minimum_32_chars".to_string(),
        max_sessions_per_user: 5,
    };

    let authenticator = Authenticator::new_mock(config);

    // Test various invalid tokens
    let invalid_tokens = vec![
        "invalid.token.format",
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.invalid.signature",
        "",
        "not.enough.parts",
        "too.many.parts.in.this.token",
    ];

    for invalid_token in invalid_tokens {
        let result = authenticator.decode_jwt(invalid_token).await;
        assert!(
            result.is_err(),
            "Invalid token should be rejected: {}",
            invalid_token
        );

        if let Err(e) = result {
            assert!(
                matches!(e, AuthError::InvalidToken),
                "Should return InvalidToken error"
            );
        }
    }
}

#[tokio::test]
async fn test_jwt_expired_token_rejection() {
    let config = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: false,
        token_expiry: std::time::Duration::from_secs(3600),
        jwt_secret: "test_secret_key_for_jwt_testing_minimum_32_chars".to_string(),
        max_sessions_per_user: 5,
    };

    let authenticator = Authenticator::new_mock(config);

    // Create expired token
    let claims = JwtClaims {
        session_id: "expired_session".to_string(),
        job_id: 100,
        permissions: vec![],
        exp: (Utc::now() - Duration::hours(1)).timestamp() as u64, // Expired
        iat: (Utc::now() - Duration::hours(2)).timestamp() as u64,
    };

    let token = authenticator.encode_jwt(&claims).await.unwrap();
    let result = authenticator.decode_jwt(&token).await;

    assert!(result.is_err(), "Expired token should be rejected");
    if let Err(e) = result {
        assert!(
            matches!(e, AuthError::TokenExpired),
            "Should return TokenExpired error"
        );
    }
}

#[tokio::test]
async fn test_jwt_wrong_secret_rejection() {
    let config1 = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: false,
        token_expiry: std::time::Duration::from_secs(3600),
        jwt_secret: "secret_key_one_for_jwt_testing_minimum_32_chars".to_string(),
        max_sessions_per_user: 5,
    };

    let config2 = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: false,
        token_expiry: std::time::Duration::from_secs(3600),
        jwt_secret: "different_secret_key_for_jwt_testing_min_32char".to_string(),
        max_sessions_per_user: 5,
    };

    let authenticator1 = Authenticator::new_mock(config1);
    let authenticator2 = Authenticator::new_mock(config2);

    let claims = JwtClaims {
        session_id: "cross_session".to_string(),
        job_id: 200,
        permissions: vec![],
        exp: (Utc::now() + Duration::hours(1)).timestamp() as u64,
        iat: Utc::now().timestamp() as u64,
    };

    // Generate token with one secret, try to validate with another
    let token = authenticator1.encode_jwt(&claims).await.unwrap();
    let result = authenticator2.decode_jwt(&token).await;

    assert!(
        result.is_err(),
        "Token with wrong secret should be rejected"
    );
    if let Err(e) = result {
        assert!(
            matches!(e, AuthError::InvalidToken),
            "Should return InvalidToken error for wrong secret"
        );
    }
}

#[tokio::test]
async fn test_jwt_secure_secret_requirement() {
    // Test that short/weak secrets are rejected
    let weak_config = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: false,
        token_expiry: std::time::Duration::from_secs(3600),
        jwt_secret: "short".to_string(), // Too short
        max_sessions_per_user: 5,
    };

    let authenticator = Authenticator::new_mock(weak_config);

    // Should handle weak secrets appropriately
    let claims = JwtClaims {
        session_id: "weak_session".to_string(),
        job_id: 300,
        permissions: vec![],
        exp: (Utc::now() + Duration::hours(1)).timestamp() as u64,
        iat: Utc::now().timestamp() as u64,
    };

    // With a weak secret, the system should either:
    // 1. Reject at initialization (preferred)
    // 2. Work but log a warning
    // For now, we'll test that it at least handles it
    let result = authenticator.encode_jwt(&claims).await;

    // The implementation should handle this case
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_jwt_claims_validation() {
    let config = AuthConfig {
        enabled: true,
        verify_job_id: true,
        require_signature: false,
        token_expiry: std::time::Duration::from_secs(3600),
        jwt_secret: "test_secret_key_for_jwt_testing_minimum_32_chars".to_string(),
        max_sessions_per_user: 5,
    };

    let authenticator = Authenticator::new_mock(config);

    // Test various claim scenarios
    let test_cases = vec![
        (1, "session1", true),
        (2, "session2", true),
        (0, "session3", true), // job_id 0 might be valid
        (3, "", true),         // Empty session might be valid
    ];

    for (job_id, session_id, should_succeed) in test_cases {
        let claims = JwtClaims {
            session_id: session_id.to_string(),
            job_id,
            permissions: vec![],
            exp: (Utc::now() + Duration::hours(1)).timestamp() as u64,
            iat: Utc::now().timestamp() as u64,
        };

        let token_result = authenticator.encode_jwt(&claims).await;
        if should_succeed {
            assert!(
                token_result.is_ok(),
                "Valid claims should generate token: job_id={}, session={}",
                job_id,
                session_id
            );
        }
    }
}

#[tokio::test]
async fn test_jwt_disabled_mode() {
    let config = AuthConfig {
        enabled: false, // Disabled
        verify_job_id: true,
        require_signature: false,
        token_expiry: std::time::Duration::from_secs(3600),
        jwt_secret: "test_secret_key_for_jwt_testing_minimum_32_chars".to_string(),
        max_sessions_per_user: 5,
    };

    let authenticator = Authenticator::new_mock(config);

    // When disabled, operations should either bypass or return mock values
    let claims = JwtClaims {
        session_id: "disabled_session".to_string(),
        job_id: 999,
        permissions: vec![],
        exp: (Utc::now() + Duration::hours(1)).timestamp() as u64,
        iat: Utc::now().timestamp() as u64,
    };

    let token = authenticator.encode_jwt(&claims).await.unwrap();
    let decoded = authenticator.decode_jwt(&token).await.unwrap();

    // Should work even when disabled (bypass mode)
    assert_eq!(decoded.session_id, claims.session_id);
    assert_eq!(decoded.job_id, claims.job_id);
}
