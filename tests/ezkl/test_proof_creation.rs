use anyhow::Result;
use fabstir_llm_node::ezkl::{
    ProofGenerator, ProofRequest, ProofResult, ProofMetadata,
    ProofFormat, CompressionLevel, ProofError, ProofStatus,
    InferenceData, ModelInput, ModelOutput
};
use fabstir_llm_node::inference::InferenceResult;
use std::time::Duration;
use std::collections::HashMap;
use tokio;

async fn create_test_generator() -> Result<ProofGenerator> {
    ProofGenerator::new_mock().await // Use mock for testing
}

fn create_test_inference_data() -> InferenceData {
    InferenceData {
        model_id: "llama-7b".to_string(),
        model_hash: "abc123def456".to_string(),
        input: ModelInput {
            prompt: "What is artificial intelligence?".to_string(),
            tokens: vec![1, 234, 567, 890], // Mock token IDs
            embeddings: vec![0.1, 0.2, 0.3, 0.4], // Mock embeddings
        },
        output: ModelOutput {
            response: "Artificial intelligence is...".to_string(),
            tokens: vec![2, 345, 678, 901],
            logits: vec![0.5, 0.6, 0.7, 0.8],
            attention_weights: Some(vec![vec![0.1; 4]; 4]),
            is_streaming: false,
            partial_tokens: vec![],
        },
        timestamp: chrono::Utc::now().timestamp() as u64,
        node_id: "node_12345".to_string(),
    }
}

#[tokio::test]
async fn test_basic_proof_creation() {
    let generator = create_test_generator().await.unwrap();
    let inference_data = create_test_inference_data();
    
    let request = ProofRequest {
        inference_data: inference_data.clone(),
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        include_metadata: true,
        custom_params: std::collections::HashMap::new(),
    };
    
    let proof_result = generator.create_proof(request).await.unwrap();
    
    assert!(!proof_result.proof_data.is_empty());
    assert_eq!(proof_result.status, ProofStatus::Completed);
    assert_eq!(proof_result.model_id, inference_data.model_id);
    assert!(proof_result.proof_hash.len() == 64); // SHA256 hash
    assert!(proof_result.generation_time_ms > 0);
    assert!(proof_result.proof_size_bytes > 0);
}

#[tokio::test]
async fn test_proof_with_compression() {
    let generator = create_test_generator().await.unwrap();
    let inference_data = create_test_inference_data();
    
    // Test different compression levels
    let compression_levels = vec![
        CompressionLevel::None,
        CompressionLevel::Fast,
        CompressionLevel::Balanced,
        CompressionLevel::Maximum,
    ];
    
    let mut sizes = Vec::new();
    
    for compression in compression_levels {
        let request = ProofRequest {
            inference_data: inference_data.clone(),
            proof_format: ProofFormat::Standard,
            compression: compression.clone(),
            include_metadata: true,
            custom_params: HashMap::new(),
        };
        
        let proof_result = generator.create_proof(request).await.unwrap();
        sizes.push((compression.clone(), proof_result.proof_size_bytes));
    }
    
    // Verify compression reduces size
    assert!(sizes[1].1 < sizes[0].1); // Fast < None
    assert!(sizes[2].1 <= sizes[1].1); // Balanced <= Fast
    assert!(sizes[3].1 <= sizes[2].1); // Maximum <= Balanced
}

#[tokio::test]
async fn test_proof_formats() {
    let generator = create_test_generator().await.unwrap();
    let inference_data = create_test_inference_data();
    
    let formats = vec![
        ProofFormat::Standard,
        ProofFormat::Compact,
        ProofFormat::Aggregated,
        ProofFormat::Recursive,
    ];
    
    for format in formats {
        let request = ProofRequest {
            inference_data: inference_data.clone(),
            proof_format: format.clone(),
            compression: CompressionLevel::None,
            include_metadata: true,
            custom_params: HashMap::new(),
        };
        
        let proof_result = generator.create_proof(request).await.unwrap();
        
        assert_eq!(proof_result.format, format.clone());
        assert!(!proof_result.proof_data.is_empty());
        
        // Verify format-specific properties
        match format {
            ProofFormat::Compact => {
                assert!(proof_result.metadata.as_ref().unwrap().optimizations.contains(&"size".to_string()));
            }
            ProofFormat::Aggregated => {
                assert!(proof_result.metadata.as_ref().unwrap().supports_batching);
            }
            ProofFormat::Recursive => {
                assert!(proof_result.metadata.as_ref().unwrap().recursion_depth > 0);
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn test_proof_metadata() {
    let generator = create_test_generator().await.unwrap();
    let inference_data = create_test_inference_data();
    
    let request = ProofRequest {
        inference_data,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        include_metadata: true,
        custom_params: HashMap::new(),
    };
    
    let proof_result = generator.create_proof(request).await.unwrap();
    let metadata = proof_result.metadata.unwrap();
    
    assert!(!metadata.circuit_hash.is_empty());
    assert!(metadata.num_constraints > 0);
    assert!(metadata.num_public_inputs > 0);
    assert!(!metadata.prover_id.is_empty());
    assert!(metadata.proof_system_version.starts_with("v"));
    assert!(metadata.timestamp > 0);
}

#[tokio::test]
async fn test_incremental_proof_generation() {
    let generator = create_test_generator().await.unwrap();
    let inference_data = create_test_inference_data();
    
    // Start incremental proof
    let proof_id = generator.start_incremental_proof(&inference_data).await.unwrap();
    
    // Add intermediate steps
    for i in 0..3 {
        let step_data = format!("Step {} computation", i);
        generator.add_proof_step(&proof_id, step_data.as_bytes()).await.unwrap();
        
        let status = generator.get_proof_status(&proof_id).await.unwrap();
        assert_eq!(status, ProofStatus::InProgress);
    }
    
    // Finalize proof
    let proof_result = generator.finalize_incremental_proof(&proof_id).await.unwrap();
    
    assert_eq!(proof_result.status, ProofStatus::Completed);
    assert!(proof_result.metadata.as_ref().unwrap().is_incremental);
    assert_eq!(proof_result.metadata.as_ref().unwrap().num_steps, 3);
}

#[tokio::test]
async fn test_proof_cancellation() {
    let generator = create_test_generator().await.unwrap();
    let inference_data = create_test_inference_data();
    
    // Start a long-running proof
    let proof_id = generator.start_incremental_proof(&inference_data).await.unwrap();
    
    // Cancel it
    generator.cancel_proof(&proof_id).await.unwrap();
    
    // Verify cancellation
    let status = generator.get_proof_status(&proof_id).await.unwrap();
    assert_eq!(status, ProofStatus::Cancelled);
    
    // Attempting to finalize should fail
    let result = generator.finalize_incremental_proof(&proof_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_proof_performance_metrics() {
    let generator = create_test_generator().await.unwrap();
    let inference_data = create_test_inference_data();
    
    let request = ProofRequest {
        inference_data,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::Fast,
        include_metadata: true,
        custom_params: HashMap::new(),
    };
    
    let start = std::time::Instant::now();
    let proof_result = generator.create_proof(request).await.unwrap();
    let duration = start.elapsed();
    
    // Verify performance metrics
    assert!(proof_result.generation_time_ms > 0);
    assert!(proof_result.generation_time_ms <= duration.as_millis() as u64);
    
    let metrics = proof_result.performance_metrics.unwrap();
    assert!(metrics.witness_generation_ms > 0);
    assert!(metrics.proof_generation_ms > 0);
    assert!(metrics.total_time_ms > 0);
    assert_eq!(
        metrics.total_time_ms,
        metrics.witness_generation_ms + metrics.proof_generation_ms + metrics.overhead_ms
    );
}

#[tokio::test]
async fn test_proof_with_custom_parameters() {
    let generator = create_test_generator().await.unwrap();
    let inference_data = create_test_inference_data();
    
    let mut request = ProofRequest {
        inference_data,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        include_metadata: true,
        custom_params: HashMap::new(),
    };
    
    // Add custom parameters
    request.set_custom_param("security_level", "128");
    request.set_custom_param("optimization", "latency");
    request.set_custom_param("parallelism", "4");
    
    let proof_result = generator.create_proof(request).await.unwrap();
    
    let metadata = proof_result.metadata.unwrap();
    assert_eq!(metadata.custom_params.get("security_level"), Some(&"128".to_string()));
    assert_eq!(metadata.custom_params.get("optimization"), Some(&"latency".to_string()));
    assert_eq!(metadata.custom_params.get("parallelism"), Some(&"4".to_string()));
}

#[tokio::test]
async fn test_proof_for_streaming_output() {
    let generator = create_test_generator().await.unwrap();
    
    // Create inference data with streaming tokens
    let mut inference_data = create_test_inference_data();
    inference_data.output.is_streaming = true;
    inference_data.output.partial_tokens = vec![
        vec![2, 345],
        vec![2, 345, 678],
        vec![2, 345, 678, 901],
    ];
    
    let request = ProofRequest {
        inference_data,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        include_metadata: true,
        custom_params: HashMap::new(),
    };
    
    let proof_result = generator.create_proof(request).await.unwrap();
    
    assert!(proof_result.metadata.as_ref().unwrap().handles_streaming);
    assert_eq!(proof_result.metadata.as_ref().unwrap().stream_chunks_count, 3);
}

#[tokio::test]
async fn test_error_handling() {
    let generator = create_test_generator().await.unwrap();
    
    // Test with invalid model hash
    let mut inference_data = create_test_inference_data();
    inference_data.model_hash = "".to_string(); // Invalid
    
    let request = ProofRequest {
        inference_data,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        include_metadata: true,
        custom_params: HashMap::new(),
    };
    
    let result = generator.create_proof(request).await;
    assert!(result.is_err());
    
    match result.unwrap_err().downcast::<ProofError>() {
        Ok(ProofError::InvalidInput(msg)) => {
            assert!(msg.contains("model_hash") || msg.contains("invalid"));
        }
        _ => panic!("Expected InvalidInput error"),
    }
}

#[tokio::test]
async fn test_proof_determinism() {
    let generator = create_test_generator().await.unwrap();
    let inference_data = create_test_inference_data();
    
    // Create same proof twice
    let request1 = ProofRequest {
        inference_data: inference_data.clone(),
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        include_metadata: false,
        custom_params: HashMap::new(),
    };
    
    let request2 = request1.clone();
    
    let proof1 = generator.create_proof(request1).await.unwrap();
    let proof2 = generator.create_proof(request2).await.unwrap();
    
    // Proofs should be identical for same input
    assert_eq!(proof1.proof_hash, proof2.proof_hash);
    assert_eq!(proof1.proof_data, proof2.proof_data);
}