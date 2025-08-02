use anyhow::Result;
use fabstir_llm_node::ezkl::{
    EZKLConfig, EZKLIntegration, ProofSystem, ModelCircuit,
    CircuitConfig, ProofBackend, ProvingKey, VerifyingKey,
    EZKLError, IntegrationStatus
};
use fabstir_llm_node::inference::InferenceEngine;
use std::path::PathBuf;
use tokio;

async fn create_test_integration() -> Result<EZKLIntegration> {
    let config = EZKLConfig {
        proof_backend: ProofBackend::Halo2,
        srs_path: PathBuf::from("test_data/srs"),
        circuit_path: PathBuf::from("test_data/circuits"),
        vk_path: PathBuf::from("test_data/vk"),
        pk_path: PathBuf::from("test_data/pk"),
        model_path: PathBuf::from("test_data/models/tiny-llama.onnx"),
        witness_path: PathBuf::from("test_data/witness"),
        max_circuit_size: 22, // 2^22 constraints
        optimization_level: 2,
        mock_mode: true, // Use mock for tests
    };
    
    EZKLIntegration::new(config).await
}

#[tokio::test]
async fn test_ezkl_initialization() {
    let integration = create_test_integration().await.unwrap();
    
    assert_eq!(integration.status(), IntegrationStatus::Ready);
    assert!(integration.is_initialized());
    
    let info = integration.get_info();
    assert_eq!(info.backend, ProofBackend::Halo2);
    assert!(info.max_model_size > 0);
    assert!(info.supported_ops.contains(&"MatMul".to_string()));
    assert!(info.supported_ops.contains(&"Add".to_string()));
    assert!(info.supported_ops.contains(&"ReLU".to_string()));
}

#[tokio::test]
async fn test_model_circuit_compilation() {
    let integration = create_test_integration().await.unwrap();
    
    // Compile a simple model to circuit
    let model_path = PathBuf::from("test_data/models/tiny-llama.onnx");
    let circuit_config = CircuitConfig {
        input_scale: 7,
        param_scale: 7,
        output_scale: 7,
        bits: 16,
        logrows: 20,
    };
    
    let circuit = integration.compile_model_circuit(
        &model_path,
        circuit_config
    ).await.unwrap();
    
    assert!(circuit.num_constraints() > 0);
    assert!(circuit.num_constraints() < 1_000_000); // Reasonable size
    assert_eq!(circuit.input_visibility().len(), 1); // Single input
    assert_eq!(circuit.output_visibility().len(), 1); // Single output
    assert!(circuit.is_valid());
}

#[tokio::test]
async fn test_setup_proving_keys() {
    let integration = create_test_integration().await.unwrap();
    
    let model_path = PathBuf::from("test_data/models/tiny-llama.onnx");
    let circuit = integration.compile_model_circuit(
        &model_path,
        CircuitConfig::default()
    ).await.unwrap();
    
    // Generate proving and verifying keys
    let (pk, vk) = integration.setup_keys(&circuit).await.unwrap();
    
    assert!(pk.size_bytes() > 1000); // Non-trivial key
    assert!(vk.size_bytes() > 100);
    assert!(vk.size_bytes() < pk.size_bytes()); // VK should be smaller
    
    // Keys should be serializable
    let pk_bytes = pk.to_bytes();
    let vk_bytes = vk.to_bytes();
    assert!(pk_bytes.len() > 0);
    assert!(vk_bytes.len() > 0);
}

#[tokio::test]
async fn test_inference_integration() {
    let integration = create_test_integration().await.unwrap();
    
    // Create inference engine with mock config
    let config = fabstir_llm_node::inference::EngineConfig {
        models_directory: std::path::PathBuf::from("./models"),
        max_loaded_models: 3,
        max_context_length: 4096,
        gpu_layers: 0, // No GPU for testing
        thread_count: 4,
        batch_size: 1,
        use_mmap: true,
        use_mlock: false,
        max_concurrent_inferences: 10,
        model_eviction_policy: "lru".to_string(),
    };
    let mut inference_engine = InferenceEngine::new(config).await.unwrap();
    
    // Register EZKL integration
    integration.register_with_engine(&mut inference_engine).await.unwrap();
    
    // Run normal inference (mock proof generation)
    let input = "What is machine learning?";
    let request = fabstir_llm_node::inference::InferenceRequest {
        model_id: "mock-model".to_string(),
        prompt: input.to_string(),
        temperature: 0.7,
        max_tokens: 100,
        stream: false,
        stop_sequences: vec![],
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.1,
        seed: None,
    };
    
    let result = inference_engine.run_inference(request).await.unwrap();
    
    // Mock proof verification
    assert!(!result.text.is_empty());
    // In a real implementation, the proof would be generated during inference
    // For this mock, we just verify the output was generated
}

#[tokio::test]
async fn test_model_compatibility_check() {
    let integration = create_test_integration().await.unwrap();
    
    // Test compatible model
    let compatible_model = PathBuf::from("test_data/models/tiny-llama.onnx");
    let is_compatible = integration.check_model_compatibility(&compatible_model).await.unwrap();
    assert!(is_compatible.is_compatible);
    assert!(is_compatible.unsupported_ops.is_empty());
    
    // Test incompatible model (with unsupported ops)
    let incompatible_model = PathBuf::from("test_data/models/complex-model.onnx");
    let is_compatible = integration.check_model_compatibility(&incompatible_model).await.unwrap();
    assert!(!is_compatible.is_compatible || !is_compatible.unsupported_ops.is_empty());
}

#[tokio::test]
async fn test_witness_generation() {
    let integration = create_test_integration().await.unwrap();
    
    let model_path = PathBuf::from("test_data/models/tiny-llama.onnx");
    let circuit = integration.compile_model_circuit(
        &model_path,
        CircuitConfig::default()
    ).await.unwrap();
    
    // Generate witness for specific input
    let input_data = vec![0.1_f32; 512]; // Mock embedding
    let witness = integration.generate_witness(
        &circuit,
        &input_data
    ).await.unwrap();
    
    assert!(witness.size() > 0);
    assert!(witness.is_valid_for_circuit(&circuit));
}

#[tokio::test]
async fn test_proof_artifacts_caching() {
    let integration = create_test_integration().await.unwrap();
    
    let model_id = "llama-7b";
    
    // First time should generate and cache
    let start = std::time::Instant::now();
    let artifacts = integration.get_or_create_artifacts(model_id).await.unwrap();
    let first_duration = start.elapsed();
    
    assert!(artifacts.proving_key.is_some());
    assert!(artifacts.verifying_key.is_some());
    assert!(artifacts.circuit.is_some());
    
    // Second time should be cached and faster
    let start = std::time::Instant::now();
    let cached_artifacts = integration.get_or_create_artifacts(model_id).await.unwrap();
    let cached_duration = start.elapsed();
    
    assert!(cached_duration < first_duration / 2); // At least 2x faster
    assert_eq!(artifacts.hash, cached_artifacts.hash);
}

#[tokio::test]
async fn test_integration_with_s5_storage() {
    let mut integration = create_test_integration().await.unwrap();
    
    // Configure S5 storage for proof artifacts
    integration.configure_storage_backend(
        fabstir_llm_node::vector::StorageBackend::Mock
    ).await.unwrap();
    
    // Store proof artifacts
    let model_id = "test-model";
    let artifacts = integration.get_or_create_artifacts(model_id).await.unwrap();
    
    let storage_path = integration.store_artifacts(&artifacts).await.unwrap();
    assert!(storage_path.starts_with("s5://"));
    
    // Retrieve artifacts
    let retrieved = integration.retrieve_artifacts(&storage_path).await.unwrap();
    assert_eq!(artifacts.hash, retrieved.hash);
}

#[tokio::test]
async fn test_resource_monitoring() {
    let integration = create_test_integration().await.unwrap();
    
    let metrics = integration.get_resource_metrics();
    
    assert!(metrics.memory_usage_mb > 0);
    assert!(metrics.circuit_compilation_time_ms >= 0);
    assert!(metrics.setup_time_ms >= 0);
    assert!(metrics.cached_circuits_count >= 0);
    assert!(metrics.total_proofs_generated >= 0);
}

#[tokio::test]
async fn test_error_handling() {
    let mut config = EZKLConfig::default();
    config.mock_mode = false;
    config.srs_path = PathBuf::from("/invalid/path");
    
    // Should handle missing SRS gracefully
    let result = EZKLIntegration::new(config).await;
    assert!(result.is_err());
    
    // Check that it's an error without calling unwrap_err (which requires Debug)
    if let Err(e) = result {
        // Error handling verified
        let _ = e; // Use the error to avoid unused variable warning
    } else {
        panic!("Expected error for invalid SRS path");
    }
}