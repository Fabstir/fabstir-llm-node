// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Model Validation Module (Phase 1, Sub-phase 1.1)
//!
//! This module enforces that production hosts can ONLY run models they're
//! registered for in the blockchain contracts.
//!
//! ## Security Issue Addressed
//!
//! Previously, hosts could run ANY local model file regardless of ModelRegistry
//! registration, bypassing contract governance and potentially defrauding clients.
//!
//! ## Validation Points
//!
//! 1. **Startup**: MODEL_PATH must match a model the host is registered for
//! 2. **Job Claim**: Hosts can only claim jobs for models they support
//! 3. **Inference**: Job model_id must match the loaded model
//!
//! ## Feature Flag
//!
//! Controlled by `REQUIRE_MODEL_VALIDATION` environment variable:
//! - `false` (default in v8.14.0): Validation disabled, warnings only
//! - `true`: Validation enforced, unauthorized operations refused
//!
//! ## References
//!
//! - Implementation Plan: `docs/IMPLEMENTATION-MODEL-VALIDATION.md`
//! - Contract Reference: `docs/compute-contracts-reference/API_REFERENCE.md`

use ethers::types::{Address, H256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::contracts::model_registry::ModelRegistryClient;
use crate::contracts::Web3Client;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during model validation
#[derive(Debug, Clone)]
pub enum ModelValidationError {
    /// Model filename not found in dynamic map (not registered on-chain)
    ModelNotRegistered(String),

    /// Host address is not authorized for this model_id
    HostNotAuthorized(Address, H256),

    /// Job's model_id doesn't match the loaded model's semantic_id
    ModelIdMismatch { expected: H256, actual: H256 },

    /// Model file SHA256 hash doesn't match on-chain hash
    ModelHashMismatch { expected: H256, path: String },

    /// Contract RPC query failed (fail-safe: refuse operation)
    ContractUnavailable(String),

    /// Model path is invalid (doesn't exist or no filename)
    InvalidModelPath(String),
}

impl std::fmt::Display for ModelValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelNotRegistered(path) => {
                write!(
                    f,
                    "Model not registered in ModelRegistry: {}. Only models approved on-chain can be used.",
                    path
                )
            }
            Self::HostNotAuthorized(host, model_id) => {
                write!(
                    f,
                    "Host 0x{} not authorized for model 0x{}. Register this model in NodeRegistry first.",
                    hex::encode(host.as_bytes()),
                    hex::encode(&model_id.0)
                )
            }
            Self::ModelIdMismatch { expected, actual } => {
                write!(
                    f,
                    "Model ID mismatch: job requests 0x{} but loaded model is 0x{}",
                    hex::encode(&expected.0),
                    hex::encode(&actual.0)
                )
            }
            Self::ModelHashMismatch { expected, path } => {
                write!(
                    f,
                    "Model file hash mismatch for {}: expected SHA256 0x{}. File may be corrupted or tampered.",
                    path,
                    hex::encode(&expected.0)
                )
            }
            Self::ContractUnavailable(msg) => {
                write!(
                    f,
                    "Contract unavailable (fail-safe: refusing operation): {}",
                    msg
                )
            }
            Self::InvalidModelPath(path) => {
                write!(
                    f,
                    "Invalid model path: {}. Path must exist and have a valid filename.",
                    path
                )
            }
        }
    }
}

impl std::error::Error for ModelValidationError {}

// ============================================================================
// Dynamic Model Info (from contract)
// ============================================================================

/// Model info fetched dynamically from ModelRegistry contract
///
/// This struct is populated by querying `getAllModels()` and `getModel()` at startup.
/// Any model registered on-chain is automatically supported without code changes.
#[derive(Debug, Clone)]
pub struct DynamicModelInfo {
    /// Contract model ID (keccak256 of repo/filename)
    pub model_id: H256,
    /// HuggingFace repository (e.g., "bartowski/openai_gpt-oss-20b-GGUF")
    pub repo: String,
    /// Model filename (e.g., "openai_gpt-oss-20b-MXFP4.gguf")
    pub filename: String,
    /// SHA256 hash of the model file (for integrity verification)
    pub sha256_hash: H256,
}

// ============================================================================
// ModelValidator
// ============================================================================

/// Validates model authorization against blockchain contracts
///
/// This validator ensures:
/// 1. The loaded model is registered in ModelRegistry
/// 2. The host is authorized to run the model (registered in NodeRegistry)
/// 3. The model file hash matches the on-chain SHA256
/// 4. Job model_id matches the loaded model
///
/// ## Usage
///
/// ```ignore
/// let validator = ModelValidator::new(
///     model_registry_client,
///     node_registry_address,
///     web3_client,
/// );
///
/// // Build dynamic model map at startup
/// validator.build_model_map().await?;
///
/// // Validate model before loading
/// let model_id = validator
///     .validate_model_at_startup(&model_path, host_address)
///     .await?;
/// ```
pub struct ModelValidator {
    /// ModelRegistry contract client
    model_registry: Arc<ModelRegistryClient>,

    /// NodeRegistry contract address
    node_registry_address: Address,

    /// Web3 client for contract queries
    web3_client: Arc<Web3Client>,

    /// Cache: host_address → list of authorized model IDs
    authorized_models_cache: Arc<RwLock<HashMap<Address, Vec<H256>>>>,

    /// Dynamic map: filename → model info (built at startup from contract)
    model_map: Arc<RwLock<HashMap<String, DynamicModelInfo>>>,

    /// Whether model validation is enabled (REQUIRE_MODEL_VALIDATION env var)
    feature_enabled: bool,
}

impl ModelValidator {
    /// Create a new ModelValidator
    ///
    /// Reads `REQUIRE_MODEL_VALIDATION` environment variable:
    /// - `true` or `1`: Validation enforced
    /// - `false` or unset (default for v8.14.0): Validation disabled, warnings only
    ///
    /// After construction, call `build_model_map()` to populate the dynamic model map.
    pub fn new(
        model_registry: Arc<ModelRegistryClient>,
        node_registry_address: Address,
        web3_client: Arc<Web3Client>,
    ) -> Self {
        let feature_enabled = std::env::var("REQUIRE_MODEL_VALIDATION")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false); // Default false for v8.14.0

        if feature_enabled {
            info!("🔒 Model validation ENABLED (REQUIRE_MODEL_VALIDATION=true)");
        } else {
            warn!("⚠️  Model validation DISABLED (set REQUIRE_MODEL_VALIDATION=true to enable)");
        }

        Self {
            model_registry,
            node_registry_address,
            web3_client,
            authorized_models_cache: Arc::new(RwLock::new(HashMap::new())),
            model_map: Arc::new(RwLock::new(HashMap::new())),
            feature_enabled,
        }
    }

    /// Check if model validation is enabled
    ///
    /// Returns `true` if `REQUIRE_MODEL_VALIDATION=true` or `1` was set,
    /// `false` otherwise (default for v8.14.0).
    pub fn is_enabled(&self) -> bool {
        self.feature_enabled
    }

    /// Get the node registry address
    pub fn node_registry_address(&self) -> Address {
        self.node_registry_address
    }

    /// Get reference to the model registry client
    pub fn model_registry(&self) -> &Arc<ModelRegistryClient> {
        &self.model_registry
    }

    /// Get reference to the web3 client
    pub fn web3_client(&self) -> &Arc<Web3Client> {
        &self.web3_client
    }

    // ========================================================================
    // Sub-phase 1.2: Dynamic Model Map from Contract
    // ========================================================================

    /// Build dynamic filename→model_id map from ModelRegistry contract
    ///
    /// Called at startup - any model registered on-chain is automatically supported.
    /// This eliminates the need for hardcoded model lists.
    ///
    /// # Process
    /// 1. Query `getAllModels()` to get all approved model IDs
    /// 2. For each model_id, query `getModel()` for details
    /// 3. Build HashMap keyed by filename
    ///
    /// # Errors
    /// Returns `ContractUnavailable` if contract queries fail
    pub async fn build_model_map(&self) -> Result<(), ModelValidationError> {
        info!("📋 Building dynamic model map from ModelRegistry...");

        // Step 1: Get all approved model IDs from contract
        let model_ids = self
            .model_registry
            .get_all_approved_models()
            .await
            .map_err(|e| {
                ModelValidationError::ContractUnavailable(format!(
                    "Failed to get approved models: {}",
                    e
                ))
            })?;

        info!("Found {} approved models on-chain", model_ids.len());

        // Step 2: Fetch details for each model
        let mut map = HashMap::new();
        for model_id in model_ids {
            match self.model_registry.get_model_details(model_id).await {
                Ok(info) => {
                    let dynamic_info = DynamicModelInfo {
                        model_id,
                        repo: info.huggingface_repo.clone(),
                        filename: info.file_name.clone(),
                        sha256_hash: info.sha256_hash,
                    };
                    debug!(
                        "  ✓ {} → 0x{}",
                        info.file_name,
                        hex::encode(&model_id.0[..8])
                    );
                    map.insert(info.file_name, dynamic_info);
                }
                Err(e) => {
                    warn!(
                        "Failed to get details for model 0x{}: {}",
                        hex::encode(&model_id.0),
                        e
                    );
                }
            }
        }

        // Step 3: Store in validator
        let mut model_map = self.model_map.write().await;
        *model_map = map;

        info!("✅ Model map built with {} models", model_map.len());
        Ok(())
    }

    /// Lookup model info by filename (from dynamic map)
    ///
    /// Returns `None` if the filename is not found in the map.
    /// The map is built at startup from ModelRegistry contract.
    pub async fn get_model_by_filename(&self, filename: &str) -> Option<DynamicModelInfo> {
        let map = self.model_map.read().await;
        map.get(filename).cloned()
    }

    /// Get current model map size (for testing/debugging)
    pub async fn model_map_size(&self) -> usize {
        let map = self.model_map.read().await;
        map.len()
    }

    // ========================================================================
    // Sub-phase 1.3: Contract Query with Caching
    // ========================================================================

    /// Check if a host is authorized to run a specific model
    ///
    /// Queries NodeRegistry.nodeSupportsModel() with in-memory caching.
    ///
    /// # Caching Strategy
    /// - Cache hit: Return immediately (< 1ms)
    /// - Cache miss: Query contract, cache on success (< 200ms)
    /// - Cache stores only positive authorizations (host IS authorized)
    ///
    /// # Errors
    /// Returns `ContractUnavailable` if the contract query fails.
    pub async fn check_host_authorization(
        &self,
        host_address: Address,
        model_id: H256,
    ) -> Result<bool, ModelValidationError> {
        // Check cache first
        {
            let cache = self.authorized_models_cache.read().await;
            if let Some(models) = cache.get(&host_address) {
                if models.contains(&model_id) {
                    debug!(
                        "✅ Cache hit: host {} authorized for model 0x{}",
                        host_address,
                        hex::encode(&model_id.0[..8])
                    );
                    return Ok(true);
                }
            }
        }

        // Query NodeRegistry contract
        debug!(
            "📡 Querying NodeRegistry.nodeSupportsModel({}, 0x{})...",
            host_address,
            hex::encode(&model_id.0[..8])
        );

        let supports = self
            .model_registry
            .node_supports_model(host_address, model_id)
            .await
            .map_err(|e| {
                ModelValidationError::ContractUnavailable(format!(
                    "Failed to query nodeSupportsModel: {}",
                    e
                ))
            })?;

        // Update cache if authorized
        if supports {
            let mut cache = self.authorized_models_cache.write().await;
            cache
                .entry(host_address)
                .or_insert_with(Vec::new)
                .push(model_id);
            info!(
                "✅ Host {} authorized for model 0x{} (cached)",
                host_address,
                hex::encode(&model_id.0[..8])
            );
        } else {
            debug!(
                "❌ Host {} NOT authorized for model 0x{}",
                host_address,
                hex::encode(&model_id.0[..8])
            );
        }

        Ok(supports)
    }

    /// Clear the authorization cache (for testing or cache refresh)
    pub async fn clear_authorization_cache(&self) {
        let mut cache = self.authorized_models_cache.write().await;
        cache.clear();
        debug!("Authorization cache cleared");
    }

    // ========================================================================
    // Sub-phase 1.4: Model ID Extraction Using Dynamic Map
    // ========================================================================

    /// Extract model ID from MODEL_PATH filename using dynamic map
    ///
    /// The dynamic map is built at startup from ModelRegistry contract,
    /// so any model registered on-chain is automatically supported.
    ///
    /// # Arguments
    /// * `model_path` - Path to the model file (e.g., "/models/tiny-vicuna-1b.q4_k_m.gguf")
    ///
    /// # Returns
    /// * `Ok(H256)` - The model ID from the contract
    /// * `Err(InvalidModelPath)` - If path has no filename
    /// * `Err(ModelNotRegistered)` - If filename not in dynamic map
    ///
    /// # Example
    /// ```ignore
    /// let model_id = validator.extract_model_id_from_path(Path::new("/models/tiny-vicuna.gguf")).await?;
    /// ```
    pub async fn extract_model_id_from_path(
        &self,
        model_path: &Path,
    ) -> Result<H256, ModelValidationError> {
        // Extract filename from path
        let filename = model_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                ModelValidationError::InvalidModelPath(format!(
                    "Cannot extract filename from path: {}",
                    model_path.display()
                ))
            })?;

        // Validate it looks like a model file (has .gguf extension)
        if !filename.ends_with(".gguf") {
            return Err(ModelValidationError::InvalidModelPath(format!(
                "Model file must have .gguf extension: {}",
                filename
            )));
        }

        // Lookup in dynamic map (built from contract at startup)
        match self.get_model_by_filename(filename).await {
            Some(model_info) => {
                debug!(
                    "Found model {} → 0x{}",
                    filename,
                    hex::encode(&model_info.model_id.0[..8])
                );
                Ok(model_info.model_id)
            }
            None => {
                // Model not found in dynamic map = not registered on-chain
                Err(ModelValidationError::ModelNotRegistered(format!(
                    "Model file '{}' is not registered in ModelRegistry. \
                     Only models approved on-chain can be used.",
                    filename
                )))
            }
        }
    }

    // ========================================================================
    // Sub-phase 2.1: Full Startup Validation with SHA256 Verification
    // ========================================================================

    /// Validate model authorization at startup
    ///
    /// This is the main entry point for model validation. It performs
    /// a 4-step verification:
    ///
    /// 1. **Extract model ID** from filename using dynamic map
    /// 2. **Check global approval** via `isModelApproved(modelId)`
    /// 3. **Verify file hash** against on-chain SHA256 from `getModel()`
    /// 4. **Check host authorization** via `nodeSupportsModel(host, modelId)`
    ///
    /// # Security
    ///
    /// - Fail-safe: If contract unavailable, refuse operation
    /// - SHA256 verification prevents tampered model files
    /// - Dynamic map ensures only on-chain registered models work
    ///
    /// # Arguments
    /// * `model_path` - Path to the model file
    /// * `host_address` - Host wallet address
    ///
    /// # Returns
    /// * `Ok(H256)` - The verified model ID on success
    /// * `Ok(H256::zero())` - If validation is disabled (feature flag)
    /// * `Err(ModelValidationError)` - On validation failure
    ///
    /// # Feature Flag
    /// If `REQUIRE_MODEL_VALIDATION=false`, this returns `H256::zero()` immediately.
    pub async fn validate_model_at_startup(
        &self,
        model_path: &Path,
        host_address: Address,
    ) -> Result<H256, ModelValidationError> {
        // Check feature flag first
        if !self.feature_enabled {
            warn!("⚠️  Model validation DISABLED (REQUIRE_MODEL_VALIDATION=false)");
            return Ok(H256::zero());
        }

        info!("🔒 Validating model authorization at startup...");

        // Verify file exists
        if !model_path.exists() {
            return Err(ModelValidationError::InvalidModelPath(format!(
                "Model file not found: {}",
                model_path.display()
            )));
        }

        // Step 1: Extract model ID from filename (uses dynamic map)
        let model_id = self.extract_model_id_from_path(model_path).await?;
        info!("📋 Model ID from path: 0x{}", hex::encode(&model_id.0[..8]));

        // Step 2: Check if model is globally approved
        let is_approved = self
            .model_registry
            .is_model_approved(model_id)
            .await
            .map_err(|e| {
                ModelValidationError::ContractUnavailable(format!(
                    "Failed to check model approval: {}",
                    e
                ))
            })?;

        if !is_approved {
            error!(
                "❌ Model 0x{} is NOT approved in ModelRegistry",
                hex::encode(&model_id.0)
            );
            return Err(ModelValidationError::ModelNotRegistered(
                model_path.display().to_string(),
            ));
        }

        info!("✅ Model is globally approved");

        // Step 3: CRITICAL - Verify file hash matches on-chain SHA256
        // Query model details from contract (authoritative source for SHA256)
        let model_info = self
            .model_registry
            .get_model_details(model_id)
            .await
            .map_err(|e| {
                ModelValidationError::ContractUnavailable(format!(
                    "Failed to get model details: {}",
                    e
                ))
            })?;

        info!("🔍 Verifying file hash against on-chain SHA256...");
        let expected_hash = format!("{:x}", model_info.sha256_hash);

        let hash_valid = self
            .model_registry
            .verify_model_hash(model_path, &expected_hash)
            .await
            .map_err(|e| {
                ModelValidationError::ContractUnavailable(format!(
                    "Hash verification failed: {}",
                    e
                ))
            })?;

        if !hash_valid {
            error!(
                "❌ Model file hash MISMATCH! Expected: 0x{}",
                hex::encode(&model_info.sha256_hash.0)
            );
            return Err(ModelValidationError::ModelHashMismatch {
                expected: model_info.sha256_hash,
                path: model_path.display().to_string(),
            });
        }

        info!("✅ Model file hash verified against contract");

        // Step 4: Verify host is authorized for this model
        let is_authorized = self
            .check_host_authorization(host_address, model_id)
            .await?;

        if !is_authorized {
            error!(
                "❌ Host 0x{} is NOT authorized for model 0x{}",
                hex::encode(host_address.as_bytes()),
                hex::encode(&model_id.0)
            );
            return Err(ModelValidationError::HostNotAuthorized(
                host_address,
                model_id,
            ));
        }

        info!(
            "✅ Model validation passed: host 0x{} authorized for 0x{} (hash verified)",
            hex::encode(host_address.as_bytes()),
            hex::encode(&model_id.0[..8])
        );

        Ok(model_id)
    }
}

// ============================================================================
// Helper Functions (for use by job_claim.rs and other modules)
// ============================================================================

/// Parse a model_id string (from job requests) to H256
///
/// Handles both "0x..." prefixed and raw hex strings.
///
/// # Arguments
/// * `model_id` - Model ID as hex string (with or without 0x prefix)
///
/// # Returns
/// * `Ok(H256)` - Parsed model ID
/// * `Err(String)` - Error message if invalid
///
/// # Example
/// ```ignore
/// let id = parse_model_id_string("0x0b75a206...")?;
/// let id = parse_model_id_string("0b75a206...")?; // Also works
/// ```
pub fn parse_model_id_string(model_id: &str) -> Result<H256, String> {
    let hex_str = model_id.strip_prefix("0x").unwrap_or(model_id);

    let bytes = hex::decode(hex_str).map_err(|e| format!("Invalid model_id hex: {}", e))?;

    if bytes.len() != 32 {
        return Err(format!(
            "model_id must be 32 bytes, got {} bytes",
            bytes.len()
        ));
    }

    Ok(H256::from_slice(&bytes))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_model_not_registered() {
        let err = ModelValidationError::ModelNotRegistered("test.gguf".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("not registered"));
        assert!(msg.contains("test.gguf"));
    }

    #[test]
    fn test_error_display_host_not_authorized() {
        let host = Address::zero();
        let model_id = H256::zero();
        let err = ModelValidationError::HostNotAuthorized(host, model_id);
        let msg = format!("{}", err);
        assert!(msg.to_lowercase().contains("not authorized"));
    }

    #[test]
    fn test_error_display_model_hash_mismatch() {
        let err = ModelValidationError::ModelHashMismatch {
            expected: H256::zero(),
            path: "/test/path.gguf".to_string(),
        };
        let msg = format!("{}", err);
        assert!(msg.to_lowercase().contains("hash"));
        assert!(msg.contains("/test/path.gguf"));
    }

    #[test]
    fn test_error_is_error_trait() {
        let err = ModelValidationError::ContractUnavailable("test".to_string());
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn test_dynamic_model_info_fields() {
        let info = DynamicModelInfo {
            model_id: H256::zero(),
            repo: "test/repo".to_string(),
            filename: "model.gguf".to_string(),
            sha256_hash: H256::zero(),
        };
        assert_eq!(info.repo, "test/repo");
        assert_eq!(info.filename, "model.gguf");
    }
}
