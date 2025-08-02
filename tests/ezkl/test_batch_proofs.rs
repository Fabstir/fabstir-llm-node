use anyhow::Result;
use fabstir_llm_node::ezkl::{
    BatchProofGenerator, BatchProofRequest, BatchProofResult,
    BatchStrategy, AggregationMethod, ParallelismConfig,
    BatchProofStatus, BatchProofError, ProofRequest,
    InferenceData, ProofFormat, CompressionLevel
};
use std::time::Duration;
use tokio;

async fn create_test_batch_generator() -> Result<BatchProofGenerator> {
    let config = ParallelismConfig {
        max_parallel_proofs: 4,
        worker_threads: 4,
        memory_limit_mb: 2048,
        use_gpu: false,
    };
    
    BatchProofGenerator::new_mock(config).await
}

fn create_test_inference_batch(size: usize) -> Vec<InferenceData> {
    (0..size)
        .map(|i| InferenceData {
            model_id: "llama-7b".to_string(),
            model_hash: "abc123def456".to_string(),
            input: fabstir_llm_node::ezkl::ModelInput {
                prompt: format!("Test prompt {}", i),
                tokens: vec![1, 234 + i as i32, 567, 890],
                embeddings: vec![0.1 + i as f32 * 0.01; 512],
            },
            output: fabstir_llm_node::ezkl::ModelOutput {
                response: format!("Test response {}", i),
                tokens: vec![2, 345 + i as i32, 678, 901],
                logits: vec![0.5 + i as f32 * 0.01; 1024],
                attention_weights: None,
                is_streaming: false,
                partial_tokens: vec![],
            },
            timestamp: chrono::Utc::now().timestamp() as u64 + i as u64,
            node_id: "node_12345".to_string(),
        })
        .collect()
}

#[tokio::test]
async fn test_basic_batch_proof() {
    let generator = create_test_batch_generator().await.unwrap();
    let batch = create_test_inference_batch(5);
    
    let request = BatchProofRequest {
        inferences: batch.clone(),
        strategy: BatchStrategy::Sequential,
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::Fast,
        priority: 5,
        enable_deduplication: false,
    };
    
    let result = generator.create_batch_proof(request).await.unwrap();
    
    assert_eq!(result.total_count, 5);
    assert_eq!(result.successful_count, 5);
    assert_eq!(result.failed_count, 0);
    assert_eq!(result.proofs.len(), 5);
    assert_eq!(result.status, BatchProofStatus::Completed);
    assert!(result.total_time_ms > 0);
    
    // Verify individual proofs
    for (i, proof) in result.proofs.iter().enumerate() {
        assert!(proof.is_success());
        assert_eq!(proof.inference_index(), i);
        assert!(!proof.proof_data().is_empty());
    }
}

#[tokio::test]
async fn test_parallel_batch_processing() {
    let generator = create_test_batch_generator().await.unwrap();
    let batch = create_test_inference_batch(10);
    
    // Test sequential vs parallel
    let sequential_request = BatchProofRequest {
        inferences: batch.clone(),
        strategy: BatchStrategy::Sequential,
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        priority: 5,
        enable_deduplication: false,
    };
    
    let parallel_request = BatchProofRequest {
        inferences: batch.clone(),
        strategy: BatchStrategy::Parallel { max_concurrent: 4 },
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        priority: 5,
        enable_deduplication: false,
    };
    
    let start = std::time::Instant::now();
    let sequential_result = generator.create_batch_proof(sequential_request).await.unwrap();
    let sequential_time = start.elapsed();
    
    let start = std::time::Instant::now();
    let parallel_result = generator.create_batch_proof(parallel_request).await.unwrap();
    let parallel_time = start.elapsed();
    
    // Parallel should be faster
    assert!(parallel_time < sequential_time);
    assert!(parallel_result.parallelism_speedup > 1.0);
    
    // Results should be the same
    assert_eq!(sequential_result.successful_count, parallel_result.successful_count);
    assert_eq!(sequential_result.proofs.len(), parallel_result.proofs.len());
}

#[tokio::test]
async fn test_aggregated_batch_proof() {
    let generator = create_test_batch_generator().await.unwrap();
    let batch = create_test_inference_batch(8);
    
    let request = BatchProofRequest {
        inferences: batch,
        strategy: BatchStrategy::Parallel { max_concurrent: 4 },
        aggregation: AggregationMethod::Recursive { depth: 3 },
        proof_format: ProofFormat::Aggregated,
        compression: CompressionLevel::Balanced,
        priority: 5,
        enable_deduplication: false,
    };
    
    let result = generator.create_batch_proof(request).await.unwrap();
    
    assert_eq!(result.aggregation_method, Some(AggregationMethod::Recursive { depth: 3 }));
    assert!(result.aggregated_proof.is_some());
    
    let agg_proof = result.aggregated_proof.unwrap();
    assert!(!agg_proof.data.is_empty());
    assert_eq!(agg_proof.num_aggregated, 8);
    assert!(agg_proof.aggregation_tree_root.len() == 64); // SHA256
    assert!(agg_proof.size_reduction_factor > 1.0);
}

#[tokio::test]
async fn test_batch_with_failures() {
    let generator = create_test_batch_generator().await.unwrap();
    let mut batch = create_test_inference_batch(10);
    
    // Corrupt some entries to cause failures
    batch[2].model_hash = "".to_string(); // Invalid
    batch[5].output.tokens = vec![]; // Invalid
    batch[7].input.embeddings = vec![]; // Invalid
    
    let request = BatchProofRequest {
        inferences: batch,
        strategy: BatchStrategy::Parallel { max_concurrent: 4 },
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        priority: 5,
        enable_deduplication: false,
    };
    
    let result = generator.create_batch_proof(request).await.unwrap();
    
    assert_eq!(result.total_count, 10);
    assert_eq!(result.successful_count, 7);
    assert_eq!(result.failed_count, 3);
    assert_eq!(result.status, BatchProofStatus::PartialSuccess);
    
    // Check specific failures
    assert!(result.proofs[2].is_failure());
    assert!(result.proofs[5].is_failure());
    assert!(result.proofs[7].is_failure());
    
    // Verify error messages
    assert!(result.errors.len() >= 3);
    for error in &result.errors {
        assert!(error.inference_index == 2 || error.inference_index == 5 || error.inference_index == 7);
        assert!(!error.error_message.is_empty());
    }
}

#[tokio::test]
async fn test_streaming_batch_proof() {
    let generator = create_test_batch_generator().await.unwrap();
    let batch = create_test_inference_batch(20);
    
    let request = BatchProofRequest {
        inferences: batch,
        strategy: BatchStrategy::Streaming { chunk_size: 5 },
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::Fast,
        priority: 5,
        enable_deduplication: false,
    };
    
    // Use streaming API
    let mut stream = generator.create_batch_proof_stream(request).await.unwrap();
    let mut received_count = 0;
    let mut chunks = Vec::new();
    
    while let Some(chunk_result) = stream.next_chunk().await.unwrap() {
        assert!(chunk_result.chunk_size <= 5);
        received_count += chunk_result.proofs.len();
        chunks.push(chunk_result);
    }
    
    assert_eq!(received_count, 20);
    assert_eq!(chunks.len(), 4); // 20 / 5 = 4 chunks
    
    // Verify chunks are processed in order
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.chunk_index, i);
        assert_eq!(chunk.total_chunks, 4);
    }
}

#[tokio::test]
async fn test_adaptive_batching() {
    let generator = create_test_batch_generator().await.unwrap();
    let batch = create_test_inference_batch(15);
    
    let request = BatchProofRequest {
        inferences: batch,
        strategy: BatchStrategy::Adaptive {
            target_latency_ms: 1000,
            min_batch_size: 2,
            max_batch_size: 8,
        },
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        priority: 5,
        enable_deduplication: false,
    };
    
    let result = generator.create_batch_proof(request).await.unwrap();
    
    assert_eq!(result.total_count, 15);
    assert!(result.adaptive_metrics.is_some());
    
    let metrics = result.adaptive_metrics.unwrap();
    assert!(metrics.avg_batch_size >= 2.0 && metrics.avg_batch_size <= 8.0);
    assert!(metrics.latency_compliance_rate >= 0.0 && metrics.latency_compliance_rate <= 1.0);
    assert_eq!(metrics.total_batches, (15.0 / metrics.avg_batch_size).ceil() as usize);
}

#[tokio::test]
async fn test_batch_proof_cancellation() {
    let generator = create_test_batch_generator().await.unwrap();
    let batch = create_test_inference_batch(100); // Large batch
    
    let request = BatchProofRequest {
        inferences: batch,
        strategy: BatchStrategy::Sequential,
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        priority: 5,
        enable_deduplication: false,
    };
    
    // Start batch proof
    let batch_id = generator.start_batch_proof(request).await.unwrap();
    
    // Let it run briefly
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Cancel it
    generator.cancel_batch_proof(&batch_id).await.unwrap();
    
    // Check status
    let status = generator.get_batch_status(&batch_id).await.unwrap();
    assert_eq!(status.status, BatchProofStatus::Cancelled);
    assert!(status.processed_count < 100); // Should not have finished all
}

#[tokio::test]
async fn test_batch_resource_limits() {
    let mut config = ParallelismConfig {
        max_parallel_proofs: 2,
        worker_threads: 2,
        memory_limit_mb: 100, // Very low limit
        use_gpu: false,
    };
    
    let generator = BatchProofGenerator::new_mock(config).await.unwrap();
    let batch = create_test_inference_batch(50);
    
    let request = BatchProofRequest {
        inferences: batch,
        strategy: BatchStrategy::Parallel { max_concurrent: 10 }, // Request more than allowed
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        priority: 5,
        enable_deduplication: false,
    };
    
    let result = generator.create_batch_proof(request).await.unwrap();
    
    // Should respect resource limits
    assert!(result.resource_metrics.is_some());
    let metrics = result.resource_metrics.unwrap();
    assert!(metrics.peak_memory_mb <= 100);
    assert!(metrics.max_concurrent_proofs <= 2);
}

#[tokio::test]
async fn test_batch_proof_recovery() {
    let generator = create_test_batch_generator().await.unwrap();
    let batch = create_test_inference_batch(10);
    
    let request = BatchProofRequest {
        inferences: batch,
        strategy: BatchStrategy::Sequential,
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        priority: 5,
        enable_deduplication: false,
    };
    
    // Simulate a batch that was interrupted
    let batch_id = generator.start_batch_proof(request).await.unwrap();
    
    // Simulate interruption after processing 5
    generator.simulate_interruption(&batch_id, 5).await.unwrap();
    
    // Attempt recovery
    let recovered_result = generator.recover_batch_proof(&batch_id).await.unwrap();
    
    assert_eq!(recovered_result.total_count, 10);
    assert_eq!(recovered_result.successful_count, 10);
    assert_eq!(recovered_result.recovered_from_index, 5);
    assert!(recovered_result.is_recovered);
}

#[tokio::test]
async fn test_batch_priority_ordering() {
    let generator = create_test_batch_generator().await.unwrap();
    
    // Create batches with different priorities
    let high_priority = create_test_inference_batch(5);
    let medium_priority = create_test_inference_batch(5);
    let low_priority = create_test_inference_batch(5);
    
    let high_request = BatchProofRequest {
        inferences: high_priority,
        strategy: BatchStrategy::Sequential,
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        priority: 5,
        enable_deduplication: false,
    }.with_priority(10);
    
    let medium_request = BatchProofRequest {
        inferences: medium_priority,
        strategy: BatchStrategy::Sequential,
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        priority: 5,
        enable_deduplication: false,
    }.with_priority(5);
    
    let low_request = BatchProofRequest {
        inferences: low_priority,
        strategy: BatchStrategy::Sequential,
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        priority: 5,
        enable_deduplication: false,
    }.with_priority(1);
    
    // Submit in reverse priority order
    let low_id = generator.start_batch_proof(low_request).await.unwrap();
    let medium_id = generator.start_batch_proof(medium_request).await.unwrap();
    let high_id = generator.start_batch_proof(high_request).await.unwrap();
    
    // Wait for all to complete
    let high_result = generator.wait_for_batch(&high_id).await.unwrap();
    let medium_result = generator.wait_for_batch(&medium_id).await.unwrap();
    let low_result = generator.wait_for_batch(&low_id).await.unwrap();
    
    // High priority should finish first
    assert!(high_result.completion_timestamp < medium_result.completion_timestamp);
    assert!(medium_result.completion_timestamp < low_result.completion_timestamp);
}

#[tokio::test]
async fn test_batch_deduplication() {
    let generator = create_test_batch_generator().await.unwrap();
    let mut batch = create_test_inference_batch(10);
    
    // Add duplicates
    batch.push(batch[0].clone());
    batch.push(batch[1].clone());
    batch.push(batch[0].clone());
    
    let request = BatchProofRequest {
        inferences: batch,
        strategy: BatchStrategy::Sequential,
        aggregation: AggregationMethod::None,
        proof_format: ProofFormat::Standard,
        compression: CompressionLevel::None,
        priority: 5,
        enable_deduplication: false,
    }.with_deduplication(true);
    
    let result = generator.create_batch_proof(request).await.unwrap();
    
    assert_eq!(result.total_count, 13); // Original count
    assert_eq!(result.unique_count, 10); // After deduplication
    assert_eq!(result.duplicate_count, 3);
    assert_eq!(result.proofs.len(), 10); // Only unique proofs generated
}