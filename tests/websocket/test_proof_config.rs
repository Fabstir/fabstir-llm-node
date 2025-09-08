use anyhow::Result;
use std::sync::Arc;
use std::env;

use fabstir_llm_node::api::websocket::{
    proof_config::{ProofConfig, ProofMode},
    proof_manager::ProofManager,
    handlers::{response::ResponseHandler, session_init::SessionInitHandler},
};

#[tokio::test]
async fn test_proof_config_from_env() -> Result<()> {
    // Set environment variables
    env::set_var("ENABLE_PROOF_GENERATION", "true");
    env::set_var("PROOF_TYPE", "EZKL");
    env::set_var("PROOF_MODEL_PATH", "/models/test.gguf");
    
    let config = ProofConfig::from_env();
    
    assert_eq!(config.enabled, true);
    assert_eq!(config.proof_type, "EZKL");
    assert_eq!(config.model_path, "/models/test.gguf");
    
    // Clean up
    env::remove_var("ENABLE_PROOF_GENERATION");
    env::remove_var("PROOF_TYPE");
    env::remove_var("PROOF_MODEL_PATH");
    
    Ok(())
}

#[tokio::test]
async fn test_proof_config_defaults() -> Result<()> {
    // Ensure env vars are not set
    env::remove_var("ENABLE_PROOF_GENERATION");
    env::remove_var("PROOF_TYPE");
    
    let config = ProofConfig::from_env();
    
    assert_eq!(config.enabled, false);
    assert_eq!(config.proof_type, "Simple");
    assert_eq!(config.model_path, "./models/model.gguf");
    
    Ok(())
}

#[tokio::test]
async fn test_proof_mode_selection() -> Result<()> {
    let config = ProofConfig {
        enabled: true,
        proof_type: "EZKL".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    };
    
    assert_eq!(config.get_mode(), ProofMode::EZKL);
    
    let config_simple = ProofConfig {
        enabled: true,
        proof_type: "Simple".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    };
    
    assert_eq!(config_simple.get_mode(), ProofMode::Simple);
    
    Ok(())
}

#[tokio::test]
async fn test_proof_manager_with_config() -> Result<()> {
    let config = ProofConfig {
        enabled: true,
        proof_type: "EZKL".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 50,
        batch_size: 5,
    };
    
    let manager = ProofManager::with_config(config);
    
    // Generate proof and verify it uses configured settings
    let proof = manager.generate_proof("model", "prompt", "output").await?;
    assert_eq!(proof.proof_type, "ezkl");
    
    Ok(())
}

#[tokio::test]
async fn test_proof_disabled_returns_none() -> Result<()> {
    let config = ProofConfig {
        enabled: false,
        proof_type: "EZKL".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    };
    
    let manager = ProofManager::with_config(config);
    
    // When disabled, should return None or minimal proof
    let proof = manager.generate_proof_optional("model", "prompt", "output").await?;
    assert!(proof.is_none());
    
    Ok(())
}

#[tokio::test]
async fn test_response_handler_respects_proof_config() -> Result<()> {
    let session_handler = Arc::new(SessionInitHandler::new());
    
    // Test with proofs disabled
    env::set_var("ENABLE_PROOF_GENERATION", "false");
    let config = ProofConfig::from_env();
    let proof_manager = Arc::new(ProofManager::with_config(config));
    let response_handler = ResponseHandler::new(session_handler.clone(), Some(proof_manager));
    
    session_handler.handle_session_init("test-disabled", 123, vec![]).await?;
    let response = response_handler.generate_response("test-disabled", "Test", 0).await?;
    
    // Should not have proof when disabled
    assert!(response.proof.is_none() || response.proof.as_ref().unwrap().proof_type == "disabled");
    
    env::remove_var("ENABLE_PROOF_GENERATION");
    
    Ok(())
}

#[tokio::test]
async fn test_proof_cache_size_configuration() -> Result<()> {
    let config = ProofConfig {
        enabled: true,
        proof_type: "Simple".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 2, // Very small cache
        batch_size: 1,
    };
    
    let manager = ProofManager::with_config(config);
    
    // Generate 3 different proofs
    let proof1 = manager.generate_proof("model", "prompt1", "output1").await?;
    let proof2 = manager.generate_proof("model", "prompt2", "output2").await?;
    let proof3 = manager.generate_proof("model", "prompt3", "output3").await?;
    
    // First proof should be evicted from cache
    let proof1_again = manager.generate_proof("model", "prompt1", "output1").await?;
    
    // Should be different timestamp (regenerated, not cached)
    assert_ne!(proof1.timestamp, proof1_again.timestamp);
    
    Ok(())
}

#[tokio::test]
async fn test_proof_batch_configuration() -> Result<()> {
    let config = ProofConfig {
        enabled: true,
        proof_type: "Simple".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 3,
    };
    
    let manager = Arc::new(ProofManager::with_config(config));
    
    // Generate multiple proofs concurrently
    let mut handles = vec![];
    for i in 0..6 {
        let m = manager.clone();
        let handle = tokio::spawn(async move {
            m.generate_proof("model", &format!("prompt{}", i), "output").await
        });
        handles.push(handle);
    }
    
    // All should complete successfully
    for handle in handles {
        let proof = handle.await??;
        assert!(!proof.hash.is_empty());
    }
    
    Ok(())
}

#[tokio::test]
async fn test_proof_type_switching() -> Result<()> {
    // Start with Simple
    let config1 = ProofConfig {
        enabled: true,
        proof_type: "Simple".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    };
    
    let manager1 = ProofManager::with_config(config1);
    let proof1 = manager1.generate_proof("model", "prompt", "output").await?;
    assert_eq!(proof1.proof_type, "simple");
    
    // Switch to EZKL
    let config2 = ProofConfig {
        enabled: true,
        proof_type: "EZKL".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    };
    
    let manager2 = ProofManager::with_config(config2);
    let proof2 = manager2.generate_proof("model", "prompt", "output").await?;
    assert_eq!(proof2.proof_type, "ezkl");
    
    // Switch to Risc0
    let config3 = ProofConfig {
        enabled: true,
        proof_type: "Risc0".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    };
    
    let manager3 = ProofManager::with_config(config3);
    let proof3 = manager3.generate_proof("model", "prompt", "output").await?;
    assert_eq!(proof3.proof_type, "risc0");
    
    Ok(())
}

#[tokio::test]
async fn test_proof_config_validation() -> Result<()> {
    // Test invalid proof type defaults to Simple
    let config = ProofConfig {
        enabled: true,
        proof_type: "InvalidType".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    };
    
    assert_eq!(config.get_mode(), ProofMode::Simple);
    
    // Test cache size validation
    let config_zero_cache = ProofConfig {
        enabled: true,
        proof_type: "EZKL".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 0,
        batch_size: 10,
    };
    
    // Should use minimum cache size
    let validated = config_zero_cache.validate();
    assert!(validated.cache_size >= 1);
    
    Ok(())
}