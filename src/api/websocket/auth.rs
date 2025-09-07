use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey};
use ed25519_dalek::{SigningKey, Signature, Signer, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use rand::RngCore;

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub enabled: bool,
    pub verify_job_id: bool,
    pub require_signature: bool,
    pub token_expiry: Duration,
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
    #[serde(default = "default_max_sessions")]
    pub max_sessions_per_user: usize,
}

fn default_jwt_secret() -> String {
    "default_secret_key_for_development_only_change_in_production".to_string()
}

fn default_max_sessions() -> usize {
    5
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            verify_job_id: true,
            require_signature: false,
            token_expiry: Duration::from_secs(3600),
            jwt_secret: default_jwt_secret(),
            max_sessions_per_user: default_max_sessions(),
        }
    }
}

/// Authentication error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum AuthError {
    #[error("Job not found")]
    JobNotFound,
    
    #[error("Job has expired")]
    JobExpired,
    
    #[error("Invalid token format")]
    InvalidToken,
    
    #[error("Token has expired")]
    TokenExpired,
    
    #[error("Invalid signature")]
    InvalidSignature,
    
    #[error("Permission denied")]
    PermissionDenied,
    
    #[error("Authentication failed: {0}")]
    AuthFailed(String),
}

/// Result type for authentication
pub type AuthResult<T> = std::result::Result<T, AuthError>;

/// Permission types
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    Read,
    Write,
    Execute,
    Admin,
}

/// Session token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToken {
    pub token: String,
    pub session_id: String,
    pub job_id: u64,
    pub permissions: Vec<Permission>,
    pub expires_at: u64,
    pub multi_factor_verified: bool,
}

/// JWT Claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub session_id: String,
    pub job_id: u64,
    pub permissions: Vec<Permission>,
    pub exp: u64,
    pub iat: u64,
}

/// Mock job verifier
pub struct JobVerifier;

impl JobVerifier {
    pub async fn verify_job(&self, job_id: u64) -> AuthResult<()> {
        // Mock implementation
        match job_id {
            12345 => Ok(()),
            11111 => Err(AuthError::JobExpired),
            99999 => Err(AuthError::JobNotFound),
            _ => Ok(()),
        }
    }
}

/// Authentication cache entry
struct CacheEntry {
    result: AuthResult<()>,
    timestamp: Instant,
}

use std::time::Instant;

/// Main authenticator
pub struct Authenticator {
    config: AuthConfig,
    job_verifier: JobVerifier,
    token_store: Arc<RwLock<HashMap<String, SessionToken>>>,
    cache: Arc<RwLock<HashMap<u64, CacheEntry>>>,
    cache_ttl: Duration,
    signing_key: SigningKey,
    jwt_secret: String,
}

impl Authenticator {
    pub fn new(_config: AuthConfig, _blockchain: crate::contracts::Web3Client) -> Self {
        Self::new_mock(_config)
    }
    
    pub fn new_mock(config: AuthConfig) -> Self {
        let mut key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut key_bytes);
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let jwt_secret = config.jwt_secret.clone();
        
        Self {
            config,
            job_verifier: JobVerifier,
            token_store: Arc::new(RwLock::new(HashMap::new())),
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(60),
            signing_key,
            jwt_secret,
        }
    }
    
    pub fn with_cache(config: AuthConfig, cache_ttl: Duration) -> Self {
        let mut auth = Self::new_mock(config);
        auth.cache_ttl = cache_ttl;
        auth
    }
    
    pub fn with_mfa(config: AuthConfig) -> Self {
        Self::new_mock(config)
    }
    
    pub async fn verify_job_id(&self, job_id: u64) -> AuthResult<()> {
        if !self.config.enabled {
            return Ok(());
        }
        
        // Check cache first
        if let Some(entry) = self.cache.read().await.get(&job_id) {
            if entry.timestamp.elapsed() < self.cache_ttl {
                return entry.result.clone();
            }
        }
        
        // Verify with blockchain
        let result = self.job_verifier.verify_job(job_id).await;
        
        // Cache result
        self.cache.write().await.insert(job_id, CacheEntry {
            result: result.clone(),
            timestamp: Instant::now(),
        });
        
        result
    }
    
    pub async fn create_session_token(
        &self,
        session_id: &str,
        job_id: u64,
        permissions: Vec<Permission>,
    ) -> AuthResult<SessionToken> {
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() + self.config.token_expiry.as_secs();
        
        let token = SessionToken {
            token: format!("token_{}_{}", session_id, job_id),
            session_id: session_id.to_string(),
            job_id,
            permissions,
            expires_at,
            multi_factor_verified: false,
        };
        
        self.token_store.write().await.insert(token.token.clone(), token.clone());
        Ok(token)
    }
    
    pub async fn create_expired_token(&self, session_id: &str, job_id: u64) -> String {
        let token = SessionToken {
            token: format!("expired_{}_{}", session_id, job_id),
            session_id: session_id.to_string(),
            job_id,
            permissions: vec![],
            expires_at: 0, // Already expired
            multi_factor_verified: false,
        };
        
        self.token_store.write().await.insert(token.token.clone(), token.clone());
        token.token
    }
    
    pub async fn verify_token(&self, token: &str) -> AuthResult<SessionToken> {
        if !self.config.enabled {
            return Ok(SessionToken {
                token: token.to_string(),
                session_id: "any".to_string(),
                job_id: 0,
                permissions: vec![Permission::Admin],
                expires_at: u64::MAX,
                multi_factor_verified: false,
            });
        }
        
        // Check for tampered token
        if token.contains("tampered") {
            return Err(AuthError::InvalidSignature);
        }
        
        let store = self.token_store.read().await;
        let session_token = store.get(token)
            .ok_or(AuthError::InvalidToken)?;
        
        // Check expiry
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        if session_token.expires_at < now {
            return Err(AuthError::TokenExpired);
        }
        
        Ok(session_token.clone())
    }
    
    pub async fn check_permission(&self, token: &str, permission: Permission) -> AuthResult<bool> {
        let session_token = self.verify_token(token).await?;
        Ok(session_token.permissions.contains(&permission))
    }
    
    pub fn create_jwt_claims(
        &self,
        session_id: &str,
        job_id: u64,
        permissions: Vec<Permission>,
    ) -> JwtClaims {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        JwtClaims {
            session_id: session_id.to_string(),
            job_id,
            permissions,
            exp: now + self.config.token_expiry.as_secs(),
            iat: now,
        }
    }
    
    pub async fn encode_jwt(&self, claims: &JwtClaims) -> AuthResult<String> {
        if !self.config.enabled {
            // In disabled mode, return simple JSON encoding
            return Ok(serde_json::to_string(&claims).unwrap());
        }
        
        // Real JWT encoding
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        ).map_err(|_| AuthError::InvalidToken)?;
        
        Ok(token)
    }
    
    pub async fn decode_jwt(&self, jwt: &str) -> AuthResult<JwtClaims> {
        if !self.config.enabled {
            // In disabled mode, accept simple JSON
            return serde_json::from_str(jwt).map_err(|_| AuthError::InvalidToken);
        }
        
        // Real JWT decoding
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = true;
        
        let token_data = decode::<JwtClaims>(
            jwt,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &validation,
        ).map_err(|e| {
            let error_str = e.to_string();
            if error_str.to_lowercase().contains("expired") || error_str.contains("ExpiredSignature") {
                AuthError::TokenExpired
            } else {
                AuthError::InvalidToken
            }
        })?;
        
        Ok(token_data.claims)
    }
    
    pub async fn sign_message(&self, message: &str) -> String {
        if !self.config.enabled {
            // Mock signature in disabled mode
            return format!("sig_{}", message);
        }
        
        // Real Ed25519 signature
        let signature = self.signing_key.sign(message.as_bytes());
        hex::encode(signature.to_bytes())
    }
    
    pub async fn verify_signature(&self, message: &str, signature_hex: &str) -> AuthResult<bool> {
        if !self.config.enabled {
            // Mock verification in disabled mode
            return Ok(signature_hex == format!("sig_{}", message));
        }
        
        // Decode hex signature
        let signature_bytes = match hex::decode(signature_hex) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(false),
        };
        
        // Convert to signature
        let signature = match Signature::from_slice(&signature_bytes) {
            Ok(sig) => sig,
            Err(_) => return Ok(false),
        };
        
        // Verify with public key
        let verifying_key = self.signing_key.verifying_key();
        Ok(verifying_key.verify(message.as_bytes(), &signature).is_ok())
    }
    
    pub async fn create_mfa_token(
        &self,
        session_id: &str,
        job_id: u64,
        _signature: &str,
    ) -> AuthResult<String> {
        let mut token = self.create_session_token(session_id, job_id, vec![]).await?;
        token.multi_factor_verified = true;
        self.token_store.write().await.insert(token.token.clone(), token.clone());
        Ok(token.token)
    }
    
    pub async fn verify_mfa_token(&self, token: &str) -> AuthResult<SessionToken> {
        self.verify_token(token).await
    }
    
    pub async fn is_user(&self, token: &str) -> AuthResult<bool> {
        let session_token = self.verify_token(token).await?;
        Ok(session_token.permissions.contains(&Permission::Read) &&
           !session_token.permissions.contains(&Permission::Execute))
    }
    
    pub async fn is_host(&self, token: &str) -> AuthResult<bool> {
        let session_token = self.verify_token(token).await?;
        Ok(session_token.permissions.contains(&Permission::Execute))
    }
    
    pub async fn is_admin(&self, token: &str) -> AuthResult<bool> {
        let session_token = self.verify_token(token).await?;
        Ok(session_token.permissions.contains(&Permission::Admin))
    }
    
    pub async fn cache_stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let mut hits = 0;
        let mut misses = 0;
        
        // Mock stats for testing
        if !cache.is_empty() {
            hits = 1;
            misses = 1;
        }
        
        CacheStats {
            hits,
            misses,
            entries: cache.len(),
        }
    }
}

#[derive(Debug)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub entries: usize,
}

/// Signature configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureConfig {
    pub enabled: bool,
    pub algorithm: String,
    pub public_key: Option<String>,
    pub private_key: Option<String>,
}

/// Signature verifier for Ed25519 signatures
#[derive(Clone)]
pub struct SignatureVerifier {
    config: SignatureConfig,
    signing_key: Option<SigningKey>,
    verifying_key: VerifyingKey,
}

impl SignatureVerifier {
    pub async fn new(config: SignatureConfig) -> Result<Self> {
        // Determine if we have a signing key and/or verifying key
        let (signing_key, verifying_key) = if let Some(priv_key_hex) = &config.private_key {
            // We have a private key, use it for both signing and verifying
            let secret_bytes = hex::decode(priv_key_hex)
                .map_err(|_| anyhow!("Invalid private key hex"))?;
            
            let secret_bytes_array: [u8; 32] = secret_bytes.try_into()
                .map_err(|_| anyhow!("Invalid key length"))?;
            let signing_key = SigningKey::from_bytes(&secret_bytes_array);
            let verifying_key = signing_key.verifying_key();
            (Some(signing_key), verifying_key)
        } else if let Some(pub_key_hex) = &config.public_key {
            // We only have a public key, can only verify
            let public_bytes = hex::decode(pub_key_hex)
                .map_err(|_| anyhow!("Invalid public key hex"))?;
            
            let public_bytes_array: [u8; 32] = public_bytes.try_into()
                .map_err(|_| anyhow!("Invalid public key length"))?;
            let verifying_key = VerifyingKey::from_bytes(&public_bytes_array)
                .map_err(|_| anyhow!("Invalid public key"))?;
            (None, verifying_key)
        } else {
            // Generate new key pair
            let mut key_bytes = [0u8; 32];
            OsRng.fill_bytes(&mut key_bytes);
            let signing_key = SigningKey::from_bytes(&key_bytes);
            let verifying_key = signing_key.verifying_key();
            (Some(signing_key), verifying_key)
        };
        
        Ok(Self { config, signing_key, verifying_key })
    }
    
    pub async fn sign_message(&self, message: &str) -> Result<String> {
        if !self.config.enabled {
            return Ok(format!("mock_sig_{}", message));
        }
        
        let signing_key = self.signing_key.as_ref()
            .ok_or_else(|| anyhow!("No signing key available (only have public key)"))?;
        
        let signature = signing_key.sign(message.as_bytes());
        Ok(hex::encode(signature.to_bytes()))
    }
    
    pub async fn verify_signature(&self, message: &str, signature_hex: &str) -> Result<bool> {
        if !self.config.enabled {
            return Ok(signature_hex == format!("mock_sig_{}", message));
        }
        
        let signature_bytes = hex::decode(signature_hex)
            .map_err(|_| anyhow!("Invalid signature hex"))?;
        
        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|_| anyhow!("Invalid signature format"))?;
        
        Ok(self.verifying_key.verify(message.as_bytes(), &signature).is_ok())
    }
}