// src/models/private.rs - Private model hosting support

use anyhow::{Result, anyhow};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex};
use chrono::{DateTime, Utc, Duration};
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use sha2::{Sha256, Digest};
use base64;
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateModelConfig {
    pub enable_private_models: bool,
    pub encryption_enabled: bool,
    pub storage_backend: String,
    pub max_private_models_per_user: usize,
    pub enable_usage_tracking: bool,
    pub enable_audit_logging: bool,
    pub token_expiry_hours: u32,
    pub enable_model_sharing: bool,
}

impl Default for PrivateModelConfig {
    fn default() -> Self {
        PrivateModelConfig {
            enable_private_models: true,
            encryption_enabled: true,
            storage_backend: "isolated".to_string(),
            max_private_models_per_user: 10,
            enable_usage_tracking: true,
            enable_audit_logging: true,
            token_expiry_hours: 24,
            enable_model_sharing: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateModel {
    pub id: String,
    pub name: String,
    pub owner: ModelOwner,
    pub visibility: ModelVisibility,
    pub created_at: DateTime<Utc>,
    pub model_path: PathBuf,
    pub encrypted: bool,
    pub size_bytes: u64,
}

impl PrivateModel {
    pub fn new(name: &str, owner: ModelOwner) -> Self {
        PrivateModel {
            id: String::new(),
            name: name.to_string(),
            owner,
            visibility: ModelVisibility::Private,
            created_at: Utc::now(),
            model_path: PathBuf::new(),
            encrypted: false,
            size_bytes: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelOwner {
    pub id: String,
    pub organization: Option<String>,
    pub email: String,
}

impl ModelOwner {
    pub fn new(id: &str) -> Self {
        ModelOwner {
            id: id.to_string(),
            organization: None,
            email: format!("{}@example.com", id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelVisibility {
    Private,
    Organization,
    Public,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessLevel {
    ReadOnly,
    ReadWrite,
    FullAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessToken {
    pub value: String,
    pub model_id: String,
    pub owner_id: String,
    pub access_level: AccessLevel,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelLicense {
    pub license_type: LicenseType,
    pub terms: String,
    pub restrictions: Vec<String>,
    pub attribution_required: bool,
    pub fee_structure: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LicenseType {
    OpenSource,
    Commercial,
    Research,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsagePolicy {
    pub max_requests_per_day: Option<u32>,
    pub max_tokens_per_request: Option<u32>,
    pub allowed_purposes: Vec<String>,
    pub geographic_restrictions: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct PrivateModelRegistry {
    models: HashMap<String, PrivateModel>,
    by_owner: HashMap<String, HashSet<String>>,
    by_organization: HashMap<String, HashSet<String>>,
}

impl PrivateModelRegistry {
    pub fn new() -> Self {
        PrivateModelRegistry {
            models: HashMap::new(),
            by_owner: HashMap::new(),
            by_organization: HashMap::new(),
        }
    }

    pub fn register(&mut self, model: PrivateModel) {
        let model_id = model.id.clone();
        let owner_id = model.owner.id.clone();
        
        // Index by owner
        self.by_owner.entry(owner_id).or_insert_with(HashSet::new).insert(model_id.clone());
        
        // Index by organization
        if let Some(org) = &model.owner.organization {
            self.by_organization.entry(org.clone()).or_insert_with(HashSet::new).insert(model_id.clone());
        }
        
        self.models.insert(model_id, model);
    }

    pub fn get(&self, id: &str) -> Option<&PrivateModel> {
        self.models.get(id)
    }

    pub fn list_by_owner(&self, owner_id: &str) -> Vec<&PrivateModel> {
        self.by_owner.get(owner_id)
            .map(|ids| ids.iter().filter_map(|id| self.models.get(id)).collect())
            .unwrap_or_default()
    }

    pub fn list_by_organization(&self, org: &str) -> Vec<&PrivateModel> {
        self.by_organization.get(org)
            .map(|ids| ids.iter().filter_map(|id| self.models.get(id)).collect())
            .unwrap_or_default()
    }

    pub fn remove(&mut self, id: &str) -> Option<PrivateModel> {
        if let Some(model) = self.models.remove(id) {
            // Remove from indexes
            if let Some(owner_models) = self.by_owner.get_mut(&model.owner.id) {
                owner_models.remove(id);
            }
            if let Some(org) = &model.owner.organization {
                if let Some(org_models) = self.by_organization.get_mut(org) {
                    org_models.remove(id);
                }
            }
            Some(model)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharingSettings {
    pub shared_with: Vec<String>,
    pub access_level: AccessLevel,
    pub expires_at: Option<DateTime<Utc>>,
    pub can_reshare: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub model_id: String,
    pub user_id: String,
    pub action: String,
    pub details: HashMap<String, String>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    pub algorithm: String,
    pub key_derivation: String,
    pub iterations: u32,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        EncryptionConfig {
            algorithm: "AES-256-GCM".to_string(),
            key_derivation: "PBKDF2".to_string(),
            iterations: 100_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageIsolation {
    pub separate_process: bool,
    pub memory_limit_gb: u32,
    pub no_network_access: bool,
    pub temp_storage_only: bool,
    pub cleanup_after_use: bool,
}

#[derive(Debug, Clone)]
pub struct AccessControl {
    permissions: HashMap<String, HashMap<String, AccessLevel>>,
    sharing: HashMap<String, SharingSettings>,
    tokens: HashMap<String, AccessToken>,
}

impl AccessControl {
    pub fn new() -> Self {
        AccessControl {
            permissions: HashMap::new(),
            sharing: HashMap::new(),
            tokens: HashMap::new(),
        }
    }

    pub fn check_access(&self, model_id: &str, user_id: &str) -> Option<AccessLevel> {
        self.permissions.get(model_id)?.get(user_id).copied()
    }

    pub fn grant_access(&mut self, model_id: &str, user_id: &str, level: AccessLevel) {
        self.permissions.entry(model_id.to_string())
            .or_insert_with(HashMap::new)
            .insert(user_id.to_string(), level);
    }

    pub fn revoke_access(&mut self, model_id: &str, user_id: &str) {
        if let Some(model_perms) = self.permissions.get_mut(model_id) {
            model_perms.remove(user_id);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimits {
    pub requests_per_minute: u32,
    pub requests_per_hour: u32,
    pub tokens_per_minute: u32,
    pub concurrent_requests: u32,
}

#[derive(Debug, Clone)]
pub struct UsageStats {
    pub total_requests: u32,
    pub total_tokens: u32,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct LicenseAcceptance {
    pub accepted: bool,
    pub license_version: String,
    pub accepted_at: DateTime<Utc>,
    pub user_id: String,
}

#[derive(Debug, Clone)]
pub struct IsolatedSession {
    pub id: String,
    pub model_id: String,
    pub owner_id: String,
    pub isolation: StorageIsolation,
    pub created_at: DateTime<Utc>,
    pub active: Arc<RwLock<bool>>,
}

impl IsolatedSession {
    pub async fn generate(&self, prompt: &str, config: GenerationConfig) -> Result<GenerationResponse> {
        // Mock generation in isolated environment
        Ok(GenerationResponse {
            text: format!("Isolated response for: {}", prompt),
            metadata: HashMap::from([
                ("isolation_id".to_string(), self.id.clone()),
                ("model_id".to_string(), self.model_id.clone()),
            ]),
        })
    }

    pub async fn cleanup(&self) -> Result<()> {
        let mut active = self.active.write().await;
        *active = false;
        Ok(())
    }

    pub async fn is_active(&self) -> bool {
        *self.active.read().await
    }
}

#[derive(Debug, Clone, Default)]
pub struct GenerationConfig {
    pub max_tokens: Option<usize>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct GenerationResponse {
    pub text: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct StorageInfo {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub encrypted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportPolicy {
    pub allow_download: bool,
    pub allow_api_access_only: bool,
    pub watermark_outputs: bool,
    pub require_attribution: bool,
}

#[derive(Debug, Clone)]
pub struct ApiSession {
    pub id: String,
    pub model_id: String,
    pub owner_id: String,
}

impl ApiSession {
    pub async fn generate(&self, prompt: &str, config: GenerationConfig) -> Result<GenerationResponse> {
        Ok(GenerationResponse {
            text: format!("API response for: {}", prompt),
            metadata: HashMap::from([
                ("watermark".to_string(), "true".to_string()),
                ("attribution_required".to_string(), "true".to_string()),
            ]),
        })
    }
}

#[derive(Clone)]
pub struct PrivateModelManager {
    config: PrivateModelConfig,
    state: Arc<RwLock<ManagerState>>,
}

struct ManagerState {
    registry: PrivateModelRegistry,
    access_control: AccessControl,
    licenses: HashMap<String, ModelLicense>,
    usage_policies: HashMap<String, UsagePolicy>,
    usage_tracking: HashMap<String, HashMap<String, Vec<UsageRecord>>>,
    audit_logs: Vec<AuditLog>,
    encryption_keys: HashMap<String, Vec<u8>>,
    rate_limiters: HashMap<String, RateLimiter>,
    license_acceptances: HashMap<String, Vec<LicenseAcceptance>>,
    export_policies: HashMap<String, ExportPolicy>,
}

#[derive(Debug, Clone)]
struct UsageRecord {
    timestamp: DateTime<Utc>,
    action: String,
    metrics: HashMap<String, u32>,
}

#[derive(Debug, Clone)]
struct RateLimiter {
    limits: RateLimits,
    requests: Arc<Mutex<Vec<DateTime<Utc>>>>,
    tokens: Arc<Mutex<Vec<(DateTime<Utc>, u32)>>>,
}

impl RateLimiter {
    fn new(limits: RateLimits) -> Self {
        RateLimiter {
            limits,
            requests: Arc::new(Mutex::new(Vec::new())),
            tokens: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn check_rate_limit(&self) -> Result<()> {
        let now = Utc::now();
        let mut requests = self.requests.lock().await;
        
        // Clean old requests
        requests.retain(|r| *r > now - Duration::minutes(1));
        
        if requests.len() >= self.limits.requests_per_minute as usize {
            return Err(anyhow!("Rate limit exceeded"));
        }
        
        requests.push(now);
        Ok(())
    }
}

impl PrivateModelManager {
    pub async fn new(config: PrivateModelConfig) -> Result<Self> {
        let state = Arc::new(RwLock::new(ManagerState {
            registry: PrivateModelRegistry::new(),
            access_control: AccessControl::new(),
            licenses: HashMap::new(),
            usage_policies: HashMap::new(),
            usage_tracking: HashMap::new(),
            audit_logs: Vec::new(),
            encryption_keys: HashMap::new(),
            rate_limiters: HashMap::new(),
            license_acceptances: HashMap::new(),
            export_policies: HashMap::new(),
        }));

        Ok(PrivateModelManager { config, state })
    }

    pub async fn create_private_model(&self, mut model: PrivateModel, owner: &ModelOwner) -> Result<String> {
        let mut state = self.state.write().await;
        
        // Check user limits
        let owner_models = state.registry.list_by_owner(&owner.id);
        if owner_models.len() >= self.config.max_private_models_per_user {
            return Err(anyhow!("Maximum private models limit reached"));
        }

        // Generate ID
        model.id = Uuid::new_v4().to_string();
        let model_id = model.id.clone();

        // Set default path
        if model.model_path.as_os_str().is_empty() {
            model.model_path = PathBuf::from(format!("/models/private/{}", model.id));
        }

        // Grant owner full access
        state.access_control.grant_access(&model_id, &owner.id, AccessLevel::FullAccess);

        // Register model
        state.registry.register(model.clone());

        // Audit log
        self.add_audit_log(&mut state, &model_id, &owner.id, "model_created", HashMap::new()).await;

        Ok(model_id)
    }

    pub async fn get_private_model(&self, model_id: &str, requester: &ModelOwner) -> Result<PrivateModel> {
        let state = self.state.read().await;
        
        // Check if model exists
        let model = state.registry.get(model_id)
            .ok_or_else(|| anyhow!("Model not found"))?;

        // Check access
        if !self.check_access(&state, model_id, &requester.id, AccessLevel::ReadOnly) {
            return Err(anyhow!("Access denied"));
        }

        Ok(model.clone())
    }

    pub async fn encrypt_model(&self, path: &Path, owner: &ModelOwner, config: &EncryptionConfig) -> Result<PathBuf> {
        let data = std::fs::read(path)?;
        
        // Generate encryption key
        let mut salt = [0u8; 32];
        OsRng.fill_bytes(&mut salt);
        
        let mut key = [0u8; 32];
        pbkdf2_hmac::<Sha256>(owner.id.as_bytes(), &salt, config.iterations, &mut key);

        // Encrypt data
        let cipher = Aes256Gcm::new_from_slice(&key)?;
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        let encrypted = cipher.encrypt(nonce, data.as_ref())
            .map_err(|e| anyhow!("Encryption failed: {}", e))?;

        // Save encrypted file
        let encrypted_path = path.with_extension("enc");
        let mut encrypted_data = Vec::new();
        encrypted_data.extend_from_slice(&salt);
        encrypted_data.extend_from_slice(&nonce_bytes);
        encrypted_data.extend_from_slice(&encrypted);
        
        std::fs::write(&encrypted_path, encrypted_data)?;

        // Store key for owner
        let mut state = self.state.write().await;
        state.encryption_keys.insert(owner.id.clone(), key.to_vec());

        Ok(encrypted_path)
    }

    pub async fn decrypt_model(&self, path: &Path, owner: &ModelOwner) -> Result<PathBuf> {
        let encrypted_data = std::fs::read(path)?;
        
        // Extract salt, nonce, and ciphertext
        if encrypted_data.len() < 44 {
            return Err(anyhow!("Invalid encrypted file"));
        }
        
        let salt = &encrypted_data[0..32];
        let nonce_bytes = &encrypted_data[32..44];
        let ciphertext = &encrypted_data[44..];

        // Derive key
        let mut key = [0u8; 32];
        pbkdf2_hmac::<Sha256>(owner.id.as_bytes(), salt, 100_000, &mut key);

        // Decrypt
        let cipher = Aes256Gcm::new_from_slice(&key)?;
        let nonce = Nonce::from_slice(nonce_bytes);
        
        let decrypted = cipher.decrypt(nonce, ciphertext)
            .map_err(|e| anyhow!("Decryption failed: {}", e))?;

        // Save decrypted file
        let decrypted_path = path.with_extension("dec");
        std::fs::write(&decrypted_path, decrypted)?;

        Ok(decrypted_path)
    }

    pub async fn generate_access_token(
        &self,
        model_id: &str,
        owner: &ModelOwner,
        access_level: AccessLevel,
        duration: Duration,
    ) -> Result<AccessToken> {
        let mut state = self.state.write().await;

        // Verify owner has permission to generate tokens
        if !self.check_access(&state, model_id, &owner.id, AccessLevel::FullAccess) {
            return Err(anyhow!("Insufficient permissions to generate token"));
        }

        use base64::Engine;
        let token = AccessToken {
            value: base64::engine::general_purpose::STANDARD.encode(Uuid::new_v4().as_bytes()),
            model_id: model_id.to_string(),
            owner_id: owner.id.clone(),
            access_level,
            expires_at: Utc::now() + duration,
            created_at: Utc::now(),
        };

        state.access_control.tokens.insert(token.value.clone(), token.clone());

        // Audit log
        self.add_audit_log(&mut state, model_id, &owner.id, "token_generated", HashMap::new()).await;

        Ok(token)
    }

    pub async fn validate_token(&self, token_value: &str) -> Result<AccessToken> {
        let state = self.state.read().await;
        
        let token = state.access_control.tokens.get(token_value)
            .ok_or_else(|| anyhow!("Invalid token"))?;

        if token.expires_at < Utc::now() {
            return Err(anyhow!("Token expired"));
        }

        Ok(token.clone())
    }

    pub async fn share_model(&self, model_id: &str, owner: &ModelOwner, settings: SharingSettings) -> Result<()> {
        let mut state = self.state.write().await;

        // Verify owner has permission to share
        if !self.check_access(&state, model_id, &owner.id, AccessLevel::FullAccess) {
            return Err(anyhow!("Insufficient permissions to share model"));
        }

        // Grant access to shared users
        for user_id in &settings.shared_with {
            state.access_control.grant_access(model_id, user_id, settings.access_level);
        }

        state.access_control.sharing.insert(model_id.to_string(), settings);

        Ok(())
    }

    pub async fn update_model(&self, model_id: &str, owner: &ModelOwner, updates: HashMap<String, String>) -> Result<()> {
        let mut state = self.state.write().await;

        // Check permissions
        if !self.check_access(&state, model_id, &owner.id, AccessLevel::ReadWrite) {
            return Err(anyhow!("Insufficient permissions to update model"));
        }

        // Audit log
        self.add_audit_log(&mut state, model_id, &owner.id, "model_updated", updates).await;

        Ok(())
    }

    pub async fn set_usage_policy(&self, model_id: &str, owner: &ModelOwner, policy: UsagePolicy) -> Result<()> {
        let mut state = self.state.write().await;

        // Check permissions
        if !self.check_access(&state, model_id, &owner.id, AccessLevel::FullAccess) {
            return Err(anyhow!("Insufficient permissions to set usage policy"));
        }

        state.usage_policies.insert(model_id.to_string(), policy);

        Ok(())
    }

    pub async fn track_usage(
        &self,
        model_id: &str,
        user: &ModelOwner,
        action: &str,
        metrics: HashMap<String, u32>,
    ) -> Result<()> {
        let mut state = self.state.write().await;

        let record = UsageRecord {
            timestamp: Utc::now(),
            action: action.to_string(),
            metrics,
        };

        state.usage_tracking
            .entry(model_id.to_string())
            .or_insert_with(HashMap::new)
            .entry(user.id.clone())
            .or_insert_with(Vec::new)
            .push(record);

        Ok(())
    }

    pub async fn get_usage_stats(
        &self,
        model_id: &str,
        owner: &ModelOwner,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<UsageStats> {
        let state = self.state.read().await;

        let records = state.usage_tracking
            .get(model_id)
            .and_then(|m| m.get(&owner.id))
            .ok_or_else(|| anyhow!("No usage data found"))?;

        let mut total_requests = 0;
        let mut total_tokens = 0;

        for record in records {
            if record.timestamp >= start_time && record.timestamp <= end_time {
                total_requests += 1;
                if let Some(tokens) = record.metrics.get("tokens") {
                    total_tokens += tokens;
                }
            }
        }

        Ok(UsageStats {
            total_requests,
            total_tokens,
            start_time,
            end_time,
        })
    }

    pub async fn set_license(&self, model_id: &str, owner: &ModelOwner, license: ModelLicense) -> Result<()> {
        let mut state = self.state.write().await;

        // Check permissions
        if !self.check_access(&state, model_id, &owner.id, AccessLevel::FullAccess) {
            return Err(anyhow!("Insufficient permissions to set license"));
        }

        state.licenses.insert(model_id.to_string(), license);

        Ok(())
    }

    pub async fn accept_license(&self, model_id: &str, user: &ModelOwner) -> Result<LicenseAcceptance> {
        let mut state = self.state.write().await;

        let acceptance = LicenseAcceptance {
            accepted: true,
            license_version: "1.0".to_string(),
            accepted_at: Utc::now(),
            user_id: user.id.clone(),
        };

        state.license_acceptances
            .entry(model_id.to_string())
            .or_insert_with(Vec::new)
            .push(acceptance.clone());

        Ok(acceptance)
    }

    pub async fn check_license_compliance(&self, model_id: &str, user: &ModelOwner) -> Result<bool> {
        let state = self.state.read().await;

        if let Some(acceptances) = state.license_acceptances.get(model_id) {
            Ok(acceptances.iter().any(|a| a.user_id == user.id && a.accepted))
        } else {
            Ok(false)
        }
    }

    pub async fn create_isolated_session(
        &self,
        model_id: &str,
        owner: &ModelOwner,
        isolation: StorageIsolation,
    ) -> Result<IsolatedSession> {
        let state = self.state.read().await;

        // Check access
        if !self.check_access(&state, model_id, &owner.id, AccessLevel::ReadOnly) {
            return Err(anyhow!("Access denied"));
        }

        let session = IsolatedSession {
            id: Uuid::new_v4().to_string(),
            model_id: model_id.to_string(),
            owner_id: owner.id.clone(),
            isolation,
            created_at: Utc::now(),
            active: Arc::new(RwLock::new(true)),
        };

        Ok(session)
    }

    pub async fn get_audit_logs(
        &self,
        model_id: &str,
        owner: &ModelOwner,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Vec<AuditLog>> {
        let state = self.state.read().await;

        // For audit logs, we allow access even if model is deleted
        // Check if the user was the owner by looking at audit logs
        let owner_logs: Vec<_> = state.audit_logs.iter()
            .filter(|log| log.model_id == model_id && log.user_id == owner.id)
            .collect();
        
        // If user has any logs for this model, they had access
        if owner_logs.is_empty() {
            // Still check current access for non-deleted models
            if state.registry.get(model_id).is_some() && 
               !self.check_access(&state, model_id, &owner.id, AccessLevel::FullAccess) {
                return Err(anyhow!("Insufficient permissions to view audit logs"));
            }
        }

        let logs: Vec<AuditLog> = state.audit_logs.iter()
            .filter(|log| {
                log.model_id == model_id &&
                log.timestamp >= start_time &&
                log.timestamp <= end_time
            })
            .cloned()
            .collect();

        Ok(logs)
    }

    pub async fn list_organization_models(&self, organization: &str) -> Result<Vec<PrivateModel>> {
        let state = self.state.read().await;
        
        Ok(state.registry.list_by_organization(organization)
            .into_iter()
            .cloned()
            .collect())
    }

    pub async fn search_private_models(
        &self,
        query: &str,
        requester: &ModelOwner,
        limit: usize,
    ) -> Result<Vec<PrivateModel>> {
        let state = self.state.read().await;
        
        let models: Vec<PrivateModel> = state.registry.list_by_owner(&requester.id)
            .into_iter()
            .filter(|m| m.name.contains(query))
            .take(limit)
            .cloned()
            .collect();

        Ok(models)
    }

    pub async fn set_rate_limits(&self, model_id: &str, owner: &ModelOwner, limits: RateLimits) -> Result<()> {
        let mut state = self.state.write().await;

        // Check permissions
        if !self.check_access(&state, model_id, &owner.id, AccessLevel::FullAccess) {
            return Err(anyhow!("Insufficient permissions to set rate limits"));
        }

        let limiter = RateLimiter::new(limits);
        state.rate_limiters.insert(model_id.to_string(), limiter);

        Ok(())
    }

    pub async fn check_rate_limit(&self, model_id: &str, user: &ModelOwner) -> Result<()> {
        let state = self.state.read().await;

        if let Some(limiter) = state.rate_limiters.get(model_id) {
            limiter.check_rate_limit().await
        } else {
            Ok(())
        }
    }

    pub async fn delete_private_model(&self, model_id: &str, owner: &ModelOwner) -> Result<()> {
        let mut state = self.state.write().await;

        // Check permissions
        if !self.check_access(&state, model_id, &owner.id, AccessLevel::FullAccess) {
            return Err(anyhow!("Insufficient permissions to delete model"));
        }

        // Remove model
        state.registry.remove(model_id);

        // Remove all access controls
        state.access_control.permissions.remove(model_id);
        state.access_control.sharing.remove(model_id);

        // Audit log
        self.add_audit_log(&mut state, model_id, &owner.id, "model_deleted", HashMap::new()).await;

        Ok(())
    }

    pub async fn set_export_policy(&self, model_id: &str, owner: &ModelOwner, policy: ExportPolicy) -> Result<()> {
        let mut state = self.state.write().await;

        // Check permissions
        if !self.check_access(&state, model_id, &owner.id, AccessLevel::FullAccess) {
            return Err(anyhow!("Insufficient permissions to set export policy"));
        }

        state.export_policies.insert(model_id.to_string(), policy);

        Ok(())
    }

    pub async fn export_model(&self, model_id: &str, owner: &ModelOwner) -> Result<PathBuf> {
        let state = self.state.read().await;

        // Check export policy
        if let Some(policy) = state.export_policies.get(model_id) {
            if !policy.allow_download {
                return Err(anyhow!("Export not allowed by policy"));
            }
        }

        Err(anyhow!("Export not implemented"))
    }

    pub async fn create_api_session(&self, model_id: &str, owner: &ModelOwner) -> Result<ApiSession> {
        let state = self.state.read().await;

        // Check access
        if !self.check_access(&state, model_id, &owner.id, AccessLevel::ReadOnly) {
            return Err(anyhow!("Access denied"));
        }

        Ok(ApiSession {
            id: Uuid::new_v4().to_string(),
            model_id: model_id.to_string(),
            owner_id: owner.id.clone(),
        })
    }

    pub async fn get_storage_info(&self, model_id: &str, owner: &ModelOwner) -> Result<StorageInfo> {
        let state = self.state.read().await;

        // Check permissions
        if !self.check_access(&state, model_id, &owner.id, AccessLevel::ReadOnly) {
            return Err(anyhow!("Access denied"));
        }

        let model = state.registry.get(model_id)
            .ok_or_else(|| anyhow!("Model not found"))?;

        // Ensure tenant isolation in path
        let isolated_path = PathBuf::from(format!("/models/tenants/{}/{}", owner.id, model_id));

        Ok(StorageInfo {
            path: isolated_path,
            size_bytes: model.size_bytes,
            encrypted: model.encrypted,
        })
    }

    // Helper methods
    fn check_access(&self, state: &ManagerState, model_id: &str, user_id: &str, required_level: AccessLevel) -> bool {
        // Check if user is owner
        if let Some(model) = state.registry.get(model_id) {
            if model.owner.id == user_id {
                return true;
            }
        }

        // Check granted access
        if let Some(level) = state.access_control.check_access(model_id, user_id) {
            match (required_level, level) {
                (AccessLevel::ReadOnly, _) => true,
                (AccessLevel::ReadWrite, AccessLevel::ReadWrite) | 
                (AccessLevel::ReadWrite, AccessLevel::FullAccess) => true,
                (AccessLevel::FullAccess, AccessLevel::FullAccess) => true,
                _ => false,
            }
        } else {
            false
        }
    }

    async fn add_audit_log(
        &self,
        state: &mut ManagerState,
        model_id: &str,
        user_id: &str,
        action: &str,
        details: HashMap<String, String>,
    ) {
        if self.config.enable_audit_logging {
            let log = AuditLog {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now(),
                model_id: model_id.to_string(),
                user_id: user_id.to_string(),
                action: action.to_string(),
                details,
                ip_address: None,
            };

            state.audit_logs.push(log);
        }
    }
}