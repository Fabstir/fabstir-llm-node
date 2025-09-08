use serde::{Deserialize, Serialize};
use std::env;

/// Proof generation modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProofMode {
    Simple,
    EZKL,
    Risc0,
}

impl ProofMode {
    /// Create from string
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "ezkl" => ProofMode::EZKL,
            "risc0" => ProofMode::Risc0,
            "simple" | _ => ProofMode::Simple,
        }
    }
    
    /// Convert to string
    pub fn to_string(&self) -> String {
        match self {
            ProofMode::Simple => "simple".to_string(),
            ProofMode::EZKL => "ezkl".to_string(),
            ProofMode::Risc0 => "risc0".to_string(),
        }
    }
}

/// Configuration for proof generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofConfig {
    /// Whether proof generation is enabled
    pub enabled: bool,
    
    /// Type of proof to generate
    pub proof_type: String,
    
    /// Path to the model file
    pub model_path: String,
    
    /// Maximum number of proofs to cache
    pub cache_size: usize,
    
    /// Batch size for concurrent proof generation
    pub batch_size: usize,
}

impl ProofConfig {
    /// Create configuration from environment variables
    pub fn from_env() -> Self {
        let enabled = env::var("ENABLE_PROOF_GENERATION")
            .unwrap_or_else(|_| "false".to_string())
            .parse::<bool>()
            .unwrap_or(false);
        
        let proof_type = env::var("PROOF_TYPE")
            .unwrap_or_else(|_| "Simple".to_string());
        
        let model_path = env::var("PROOF_MODEL_PATH")
            .unwrap_or_else(|_| "./models/model.gguf".to_string());
        
        let cache_size = env::var("PROOF_CACHE_SIZE")
            .unwrap_or_else(|_| "100".to_string())
            .parse::<usize>()
            .unwrap_or(100);
        
        let batch_size = env::var("PROOF_BATCH_SIZE")
            .unwrap_or_else(|_| "10".to_string())
            .parse::<usize>()
            .unwrap_or(10);
        
        Self {
            enabled,
            proof_type,
            model_path,
            cache_size,
            batch_size,
        }
    }
    
    /// Get the proof mode
    pub fn get_mode(&self) -> ProofMode {
        ProofMode::from_str(&self.proof_type)
    }
    
    /// Validate and fix configuration
    pub fn validate(mut self) -> Self {
        // Ensure minimum cache size
        if self.cache_size == 0 {
            self.cache_size = 1;
        }
        
        // Ensure minimum batch size
        if self.batch_size == 0 {
            self.batch_size = 1;
        }
        
        // Validate proof type
        let mode = self.get_mode();
        self.proof_type = mode.to_string();
        
        self
    }
    
    /// Check if proof generation is enabled for a specific session
    pub fn is_enabled_for_session(&self, _session_id: &str) -> bool {
        // Could implement per-session logic here
        self.enabled
    }
}

impl Default for ProofConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            proof_type: "Simple".to_string(),
            model_path: "./models/model.gguf".to_string(),
            cache_size: 100,
            batch_size: 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_proof_mode_from_str() {
        assert_eq!(ProofMode::from_str("Simple"), ProofMode::Simple);
        assert_eq!(ProofMode::from_str("EZKL"), ProofMode::EZKL);
        assert_eq!(ProofMode::from_str("ezkl"), ProofMode::EZKL);
        assert_eq!(ProofMode::from_str("Risc0"), ProofMode::Risc0);
        assert_eq!(ProofMode::from_str("invalid"), ProofMode::Simple);
    }
    
    #[test]
    fn test_proof_config_default() {
        let config = ProofConfig::default();
        assert_eq!(config.enabled, false);
        assert_eq!(config.proof_type, "Simple");
        assert_eq!(config.cache_size, 100);
    }
    
    #[test]
    fn test_proof_config_validation() {
        let config = ProofConfig {
            enabled: true,
            proof_type: "InvalidType".to_string(),
            model_path: "./test.gguf".to_string(),
            cache_size: 0,
            batch_size: 0,
        };
        
        let validated = config.validate();
        assert_eq!(validated.proof_type, "simple");
        assert!(validated.cache_size >= 1);
        assert!(validated.batch_size >= 1);
    }
}