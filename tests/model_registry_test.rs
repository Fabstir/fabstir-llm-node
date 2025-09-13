use ethers::prelude::*;
use fabstir_llm_node::contracts::model_registry::{ModelRegistryClient, ApprovedModels};
use std::sync::Arc;

#[tokio::test]
async fn test_model_id_calculation() {
    // Test that model IDs are calculated correctly
    let approved = ApprovedModels::default();

    // Test TinyVicuna
    let vicuna_id = ApprovedModels::calculate_model_id(
        "CohereForAI/TinyVicuna-1B-32k-GGUF",
        "tiny-vicuna-1b.q4_k_m.gguf"
    );
    assert_eq!(vicuna_id, approved.tiny_vicuna.id);
    println!("TinyVicuna Model ID: {:?}", vicuna_id);

    // Test TinyLlama
    let llama_id = ApprovedModels::calculate_model_id(
        "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF",
        "tinyllama-1b.Q4_K_M.gguf"
    );
    assert_eq!(llama_id, approved.tiny_llama.id);
    println!("TinyLlama Model ID: {:?}", llama_id);

    // Test that different inputs produce different IDs
    assert_ne!(vicuna_id, llama_id);
}

#[tokio::test]
async fn test_model_validation() {
    // Create a mock provider
    let provider = Arc::new(Provider::<Http>::try_from("http://localhost:8545").unwrap());

    // Use test addresses
    let model_registry_address = "0xfE54c2aa68A7Afe8E0DD571933B556C8b6adC357".parse::<Address>().unwrap();
    let node_registry_address = "0xaa14Ed58c3EF9355501bc360E5F09Fb9EC8c1100".parse::<Address>().unwrap();

    // Create model registry client
    let registry = ModelRegistryClient::new(
        provider,
        model_registry_address,
        Some(node_registry_address),
    ).await.unwrap();

    // Test getting approved models
    let approved = registry.get_approved_models();
    assert_eq!(approved.get_all_ids().len(), 2);

    // Test model lookup by filename
    let spec = approved.get_spec_by_file("tiny-vicuna-1b.q4_k_m.gguf");
    assert!(spec.is_some());
    assert_eq!(spec.unwrap().repo, "CohereForAI/TinyVicuna-1B-32k-GGUF");

    // Test validation for registration
    let model_paths = vec![
        "models/tiny-vicuna-1b.q4_k_m.gguf".to_string(),
        "models/tinyllama-1b.Q4_K_M.gguf".to_string(),
    ];

    // This will validate that the models are in the approved list
    // (actual hash verification would fail since files don't exist in test)
    let result = registry.validate_models_for_registration(&model_paths).await;

    // Since files don't exist, this will skip hash verification
    // but still validate they're in the approved list
    assert!(result.is_ok());
    let validated_ids = result.unwrap();
    assert_eq!(validated_ids.len(), 2);
}

#[tokio::test]
async fn test_sha256_verification() {
    use std::path::Path;
    use tokio::fs;

    // Create a test file
    let test_dir = "/tmp/model_test";
    fs::create_dir_all(test_dir).await.ok();
    let test_file = Path::new(test_dir).join("test_model.gguf");

    // Write test content
    fs::write(&test_file, b"test model content").await.unwrap();

    // Create registry client
    let provider = Arc::new(Provider::<Http>::try_from("http://localhost:8545").unwrap());
    let model_registry_address = "0xfE54c2aa68A7Afe8E0DD571933B556C8b6adC357".parse::<Address>().unwrap();

    let registry = ModelRegistryClient::new(
        provider,
        model_registry_address,
        None,
    ).await.unwrap();

    // Calculate expected hash
    let expected_hash = "95d1ec7e3dadd526c4e84b94f31a2b59dbb0da5b1c83cfc3ed965c7bd0c7bbfd";

    // Verify hash
    let result = registry.verify_model_hash(&test_file, expected_hash).await;
    assert!(result.is_ok());
    assert!(result.unwrap());

    // Test with wrong hash
    let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    let result = registry.verify_model_hash(&test_file, wrong_hash).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());

    // Cleanup
    fs::remove_file(&test_file).await.ok();
}

#[test]
fn test_approved_models_initialization() {
    let approved = ApprovedModels::default();

    // Check that both models are initialized
    assert!(!approved.tiny_vicuna.repo.is_empty());
    assert!(!approved.tiny_llama.repo.is_empty());

    // Check SHA256 hashes are set
    assert_eq!(approved.tiny_vicuna.sha256.len(), 64); // SHA256 is 64 hex chars
    assert_eq!(approved.tiny_llama.sha256.len(), 64);

    // Check model IDs are calculated
    assert_ne!(approved.tiny_vicuna.id, H256::zero());
    assert_ne!(approved.tiny_llama.id, H256::zero());

    // Verify the exact SHA256 hashes
    assert_eq!(approved.tiny_vicuna.sha256, "329d002bc20d4e7baae25df802c9678b5a4340b3ce91f23e6a0644975e95935f");
    assert_eq!(approved.tiny_llama.sha256, "45b71fe98efe5f530b825dce6f5049d738e9c16869f10be4370ab81a9912d4a6");
}