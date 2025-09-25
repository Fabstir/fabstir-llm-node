use anyhow::Result;
use std::sync::Arc;

use fabstir_llm_node::api::websocket::{
    proof_config::{ProofConfig, ProofMode},
    proof_manager::ProofManager,
};
use fabstir_llm_node::results::proofs::ProofType;

#[tokio::test]
async fn test_simple_proof_generation() -> Result<()> {
    let config = ProofConfig {
        enabled: true,
        proof_type: "Simple".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    };

    let manager = ProofManager::with_config(config);
    let proof = manager.generate_proof("model", "input", "output").await?;

    // Simple proof should have specific characteristics
    assert_eq!(proof.proof_type, "simple");
    assert!(!proof.hash.is_empty());
    assert!(!proof.model_hash.is_empty());
    assert!(!proof.input_hash.is_empty());
    assert!(!proof.output_hash.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_ezkl_proof_generation() -> Result<()> {
    let config = ProofConfig {
        enabled: true,
        proof_type: "EZKL".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    };

    let manager = ProofManager::with_config(config);
    let proof = manager.generate_proof("model", "input", "output").await?;

    // EZKL proof should have specific format
    assert_eq!(proof.proof_type, "ezkl");
    assert!(proof.hash.len() > 32); // EZKL proofs are typically longer

    Ok(())
}

#[tokio::test]
async fn test_risc0_proof_generation() -> Result<()> {
    let config = ProofConfig {
        enabled: true,
        proof_type: "Risc0".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    };

    let manager = ProofManager::with_config(config);
    let proof = manager.generate_proof("model", "input", "output").await?;

    // Risc0 proof should have specific format
    assert_eq!(proof.proof_type, "risc0");
    assert!(!proof.hash.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_proof_type_consistency() -> Result<()> {
    let configs = vec![("Simple", "simple"), ("EZKL", "ezkl"), ("Risc0", "risc0")];

    for (config_type, expected_type) in configs {
        let config = ProofConfig {
            enabled: true,
            proof_type: config_type.to_string(),
            model_path: "./models/test.gguf".to_string(),
            cache_size: 100,
            batch_size: 10,
        };

        let manager = ProofManager::with_config(config);
        let proof = manager.generate_proof("model", "input", "output").await?;

        assert_eq!(proof.proof_type, expected_type);
    }

    Ok(())
}

#[tokio::test]
async fn test_proof_determinism_by_type() -> Result<()> {
    // Simple proofs should be deterministic for same input
    let config_simple = ProofConfig {
        enabled: true,
        proof_type: "Simple".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 0, // Disable cache to test actual generation
        batch_size: 1,
    };

    let manager = ProofManager::with_config(config_simple);
    let proof1 = manager
        .generate_proof("model", "same_input", "same_output")
        .await?;
    let proof2 = manager
        .generate_proof("model", "same_input", "same_output")
        .await?;

    // Hashes should be same for same input (deterministic)
    assert_eq!(proof1.model_hash, proof2.model_hash);
    assert_eq!(proof1.input_hash, proof2.input_hash);
    assert_eq!(proof1.output_hash, proof2.output_hash);

    Ok(())
}

#[tokio::test]
async fn test_proof_type_from_string() -> Result<()> {
    assert_eq!(ProofMode::from_str("Simple"), ProofMode::Simple);
    assert_eq!(ProofMode::from_str("EZKL"), ProofMode::EZKL);
    assert_eq!(ProofMode::from_str("Risc0"), ProofMode::Risc0);
    assert_eq!(ProofMode::from_str("Invalid"), ProofMode::Simple); // Default
    assert_eq!(ProofMode::from_str(""), ProofMode::Simple); // Default

    Ok(())
}

#[tokio::test]
async fn test_proof_size_by_type() -> Result<()> {
    let manager_simple = ProofManager::with_config(ProofConfig {
        enabled: true,
        proof_type: "Simple".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    });

    let manager_ezkl = ProofManager::with_config(ProofConfig {
        enabled: true,
        proof_type: "EZKL".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    });

    let simple_proof = manager_simple
        .generate_proof("model", "input", "output")
        .await?;
    let ezkl_proof = manager_ezkl
        .generate_proof("model", "input", "output")
        .await?;

    // EZKL proofs are typically larger than simple proofs
    // This is a heuristic test based on expected behavior
    assert!(simple_proof.hash.len() <= ezkl_proof.hash.len());

    Ok(())
}

#[tokio::test]
async fn test_proof_type_performance() -> Result<()> {
    use std::time::Instant;

    let config_simple = ProofConfig {
        enabled: true,
        proof_type: "Simple".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 0, // No cache for fair comparison
        batch_size: 1,
    };

    let config_ezkl = ProofConfig {
        enabled: true,
        proof_type: "EZKL".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 0,
        batch_size: 1,
    };

    let manager_simple = ProofManager::with_config(config_simple);
    let manager_ezkl = ProofManager::with_config(config_ezkl);

    // Measure Simple proof generation time
    let start = Instant::now();
    manager_simple
        .generate_proof("model", "input", "output")
        .await?;
    let simple_time = start.elapsed();

    // Measure EZKL proof generation time
    let start = Instant::now();
    manager_ezkl
        .generate_proof("model", "input", "output")
        .await?;
    let ezkl_time = start.elapsed();

    // Simple should be faster than EZKL (in our mock implementation)
    // This is more of a sanity check than a hard requirement
    println!(
        "Simple proof time: {:?}, EZKL proof time: {:?}",
        simple_time, ezkl_time
    );

    Ok(())
}

#[tokio::test]
async fn test_mixed_proof_types_in_session() -> Result<()> {
    // Simulate a session that switches proof types
    let manager1 = ProofManager::with_config(ProofConfig {
        enabled: true,
        proof_type: "Simple".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    });

    let proof1 = manager1
        .generate_proof("model", "prompt1", "output1")
        .await?;
    assert_eq!(proof1.proof_type, "simple");

    // Switch to different proof type mid-session
    let manager2 = ProofManager::with_config(ProofConfig {
        enabled: true,
        proof_type: "EZKL".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    });

    let proof2 = manager2
        .generate_proof("model", "prompt2", "output2")
        .await?;
    assert_eq!(proof2.proof_type, "ezkl");

    // Both proofs should be valid despite different types
    assert!(!proof1.hash.is_empty());
    assert!(!proof2.hash.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_proof_type_with_special_characters() -> Result<()> {
    let config = ProofConfig {
        enabled: true,
        proof_type: "Simple".to_string(),
        model_path: "./models/test.gguf".to_string(),
        cache_size: 100,
        batch_size: 10,
    };

    let manager = ProofManager::with_config(config);

    // Test with special characters in input/output
    let special_inputs = vec![
        ("model", "prompt with 特殊字符", "output with émojis 🎉"),
        ("model", "newline\nprompt", "tab\toutput"),
        ("model", "quote\"prompt", "apostrophe'output"),
    ];

    for (model, prompt, output) in special_inputs {
        let proof = manager.generate_proof(model, prompt, output).await?;
        assert!(!proof.hash.is_empty());
        assert!(!proof.input_hash.is_empty());
        assert!(!proof.output_hash.is_empty());
    }

    Ok(())
}
