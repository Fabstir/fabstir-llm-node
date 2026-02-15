// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Safety types and configuration for image generation content safety pipeline

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Safety enforcement level controlling which categories are blocked
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyLevel {
    Strict,
    Moderate,
    Permissive,
}

impl Default for SafetyLevel {
    fn default() -> Self {
        Self::Strict
    }
}

/// Categories of unsafe content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyCategory {
    Violence,
    Sexual,
    Hate,
    SelfHarm,
    Illegal,
    Deceptive,
    Other,
}

/// Result of a safety classification check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyResult {
    pub is_safe: bool,
    pub category: Option<SafetyCategory>,
    pub reason: Option<String>,
    #[serde(default)]
    pub confidence: f32,
}

/// Configuration for safety classification
#[derive(Debug, Clone)]
pub struct SafetyConfig {
    pub level: SafetyLevel,
    pub blocked_categories: Vec<SafetyCategory>,
    pub custom_blocked_terms: Vec<String>,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            level: SafetyLevel::default(),
            blocked_categories: vec![
                SafetyCategory::Sexual,
                SafetyCategory::Violence,
                SafetyCategory::Illegal,
                SafetyCategory::SelfHarm,
            ],
            custom_blocked_terms: Vec::new(),
        }
    }
}

/// Attestation record for safety checks, used for proof of content moderation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyAttestation {
    pub prompt_hash: [u8; 32],
    pub prompt_safe: bool,
    pub output_hash: Option<[u8; 32]>,
    pub output_safe: Option<bool>,
    pub safety_level: SafetyLevel,
    pub timestamp: u64,
}

impl SafetyAttestation {
    /// Compute a SHA-256 hash of the attestation fields for integrity verification
    pub fn compute_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.prompt_hash);
        hasher.update([self.prompt_safe as u8]);
        if let Some(ref h) = self.output_hash {
            hasher.update(h);
        }
        if let Some(safe) = self.output_safe {
            hasher.update([safe as u8]);
        }
        hasher.update(self.timestamp.to_le_bytes());
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Serialize the attestation to JSON bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}
