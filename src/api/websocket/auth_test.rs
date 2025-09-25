#[cfg(test)]
mod tests {
    use super::super::auth::*;
    use chrono::{Duration, Utc};

    #[tokio::test]
    async fn test_jwt_basic() {
        let config = AuthConfig {
            enabled: true,
            verify_job_id: true,
            require_signature: false,
            token_expiry: std::time::Duration::from_secs(3600),
            jwt_secret: "test_secret_key_minimum_32_characters_long".to_string(),
            max_sessions_per_user: 5,
        };

        let auth = Authenticator::new_mock(config);

        let claims = JwtClaims {
            session_id: "test".to_string(),
            job_id: 1,
            permissions: vec![],
            exp: (Utc::now() + Duration::hours(1)).timestamp() as u64,
            iat: Utc::now().timestamp() as u64,
        };

        let token = auth.encode_jwt(&claims).await.unwrap();
        assert!(token.contains('.')); // JWT format check

        let decoded = auth.decode_jwt(&token).await.unwrap();
        assert_eq!(decoded.session_id, claims.session_id);
        assert_eq!(decoded.job_id, claims.job_id);
    }

    #[tokio::test]
    async fn test_signature_basic() {
        let config = AuthConfig::default();
        let auth = Authenticator::new_mock(config);

        let message = "test message";
        let sig = auth.sign_message(message).await;
        assert!(!sig.is_empty());

        let valid = auth.verify_signature(message, &sig).await.unwrap();
        assert!(valid);
    }
}
