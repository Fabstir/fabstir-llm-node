use anyhow::Result;
use fabstir_llm_node::performance::{
    BatchProcessor, BatchConfig, BatchRequest, BatchResult,
    BatchStatus, BatchError, BatchingStrategy, QueueConfig,
    BatchMetrics, PaddingStrategy, BatchPriority
};
use std::sync::Arc;
use std::time::Duration;
use tokio;
use futures::StreamExt;

async fn create_test_batch_processor() -> Result<BatchProcessor> {
    let config = BatchConfig {
        max_batch_size: 32,
        max_sequence_length: 2048,
        max_wait_time_ms: 100,
        batching_strategy: BatchingStrategy::Dynamic,
        padding_strategy: PaddingStrategy::RightPadding,
        enable_continuous_batching: true,
        queue_size: 1000,
        priority_queues: 3,
    };
    
    BatchProcessor::new(config).await
}

#[tokio::test]
async fn test_basic_batching() {
    let processor = create_test_batch_processor().await.unwrap();
    
    // Submit multiple requests
    let req1 = BatchRequest {
        id: "req1".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "Hello world".to_string(),
        max_tokens: 100,
        priority: BatchPriority::Normal,
    };
    
    let req2 = BatchRequest {
        id: "req2".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "How are you?".to_string(),
        max_tokens: 100,
        priority: BatchPriority::Normal,
    };
    
    processor.submit_request(req1).await.unwrap();
    processor.submit_request(req2).await.unwrap();
    
    // Process batch
    let batch = processor.get_next_batch().await.unwrap();
    
    assert_eq!(batch.requests.len(), 2);
    assert_eq!(batch.model_id, "llama-7b");
    assert!(batch.batch_id.len() > 0);
}

#[tokio::test]
async fn test_batch_size_limits() {
    let processor = create_test_batch_processor().await.unwrap();
    
    // Submit more requests than max batch size
    for i in 0..50 {
        let req = BatchRequest {
            id: format!("req{}", i),
            model_id: "llama-7b".to_string(),
            prompt: format!("Test prompt {}", i),
            max_tokens: 50,
            priority: BatchPriority::Normal,
        };
        processor.submit_request(req).await.unwrap();
    }
    
    let batch = processor.get_next_batch().await.unwrap();
    
    assert_eq!(batch.requests.len(), 32); // Max batch size
    assert_eq!(batch.total_tokens, 32 * 50); // Tokens per request * batch size
}

#[tokio::test]
async fn test_dynamic_batching() {
    let processor = create_test_batch_processor().await.unwrap();
    
    // Submit requests with different arrival times
    processor.submit_request(BatchRequest {
        id: "early".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "First request".to_string(),
        max_tokens: 100,
        priority: BatchPriority::Normal,
    }).await.unwrap();
    
    // Wait less than max_wait_time
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    processor.submit_request(BatchRequest {
        id: "late".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "Second request".to_string(),
        max_tokens: 100,
        priority: BatchPriority::Normal,
    }).await.unwrap();
    
    // Should batch together due to dynamic batching
    let batch = processor.get_next_batch().await.unwrap();
    assert_eq!(batch.requests.len(), 2);
}

#[tokio::test]
async fn test_continuous_batching() {
    let processor = create_test_batch_processor().await.unwrap();
    
    // Start continuous batching
    let mut batch_stream = processor.start_continuous_batching().await;
    
    // Submit requests while processing
    tokio::spawn(async move {
        for i in 0..10 {
            processor.submit_request(BatchRequest {
                id: format!("continuous_{}", i),
                model_id: "llama-7b".to_string(),
                prompt: format!("Continuous prompt {}", i),
                max_tokens: 50,
                priority: BatchPriority::Normal,
            }).await.unwrap();
            
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    
    // Collect batches
    let mut batch_count = 0;
    while let Some(batch) = batch_stream.next().await {
        batch_count += 1;
        assert!(batch.requests.len() > 0);
        
        if batch_count >= 3 {
            break;
        }
    }
    
    assert!(batch_count >= 3);
}

#[tokio::test]
async fn test_priority_batching() {
    let processor = create_test_batch_processor().await.unwrap();
    
    // Submit mix of priorities
    processor.submit_request(BatchRequest {
        id: "low".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "Low priority".to_string(),
        max_tokens: 100,
        priority: BatchPriority::Low,
    }).await.unwrap();
    
    processor.submit_request(BatchRequest {
        id: "high".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "High priority".to_string(),
        max_tokens: 100,
        priority: BatchPriority::High,
    }).await.unwrap();
    
    processor.submit_request(BatchRequest {
        id: "normal".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "Normal priority".to_string(),
        max_tokens: 100,
        priority: BatchPriority::Normal,
    }).await.unwrap();
    
    // High priority should be in first batch
    let batch = processor.get_next_batch().await.unwrap();
    assert!(batch.requests.iter().any(|r| r.id == "high"));
}

#[tokio::test]
async fn test_padding_strategies() {
    let mut config = BatchConfig::default();
    config.padding_strategy = PaddingStrategy::LeftPadding;
    
    let processor = BatchProcessor::new(config).await.unwrap();
    
    // Submit requests with different lengths
    let short_req = BatchRequest {
        id: "short".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "Hi".to_string(),
        max_tokens: 50,
        priority: BatchPriority::Normal,
    };
    
    let long_req = BatchRequest {
        id: "long".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "This is a much longer prompt that will require padding".to_string(),
        max_tokens: 50,
        priority: BatchPriority::Normal,
    };
    
    processor.submit_request(short_req).await.unwrap();
    processor.submit_request(long_req).await.unwrap();
    
    let batch = processor.get_next_batch().await.unwrap();
    
    assert_eq!(batch.padding_info.strategy, PaddingStrategy::LeftPadding);
    assert!(batch.padding_info.max_length > 0);
    assert_eq!(batch.padding_info.padded_sequences.len(), 2);
}

#[tokio::test]
async fn test_batch_timeout() {
    let processor = create_test_batch_processor().await.unwrap();
    
    // Submit single request
    processor.submit_request(BatchRequest {
        id: "timeout_test".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "Waiting for timeout".to_string(),
        max_tokens: 100,
        priority: BatchPriority::Normal,
    }).await.unwrap();
    
    let start = tokio::time::Instant::now();
    let batch = processor.get_next_batch().await.unwrap();
    let elapsed = start.elapsed();
    
    // Should return after max_wait_time even with single request
    assert_eq!(batch.requests.len(), 1);
    assert!(elapsed >= Duration::from_millis(100));
    assert!(elapsed < Duration::from_millis(200));
}

#[tokio::test]
async fn test_model_specific_batching() {
    let processor = create_test_batch_processor().await.unwrap();
    
    // Submit requests for different models
    processor.submit_request(BatchRequest {
        id: "llama_req".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "Llama prompt".to_string(),
        max_tokens: 100,
        priority: BatchPriority::Normal,
    }).await.unwrap();
    
    processor.submit_request(BatchRequest {
        id: "mistral_req".to_string(),
        model_id: "mistral-7b".to_string(),
        prompt: "Mistral prompt".to_string(),
        max_tokens: 100,
        priority: BatchPriority::Normal,
    }).await.unwrap();
    
    // Should create separate batches per model
    let batch1 = processor.get_next_batch().await.unwrap();
    let batch2 = processor.get_next_batch().await.unwrap();
    
    assert_ne!(batch1.model_id, batch2.model_id);
    assert_eq!(batch1.requests.len(), 1);
    assert_eq!(batch2.requests.len(), 1);
}

#[tokio::test]
async fn test_batch_metrics() {
    let processor = create_test_batch_processor().await.unwrap();
    
    // Process some batches
    for i in 0..20 {
        processor.submit_request(BatchRequest {
            id: format!("metric_test_{}", i),
            model_id: "llama-7b".to_string(),
            prompt: format!("Test prompt {}", i),
            max_tokens: 50,
            priority: BatchPriority::Normal,
        }).await.unwrap();
    }
    
    processor.get_next_batch().await.unwrap();
    
    let metrics = processor.get_metrics().await;
    
    assert!(metrics.total_requests_processed > 0);
    assert!(metrics.total_batches_created > 0);
    assert!(metrics.average_batch_size > 0.0);
    assert!(metrics.average_wait_time_ms > 0.0);
    assert!(metrics.queue_depth >= 0);
    assert!(metrics.throughput_requests_per_sec > 0.0);
}

#[tokio::test]
async fn test_batch_cancellation() {
    let processor = create_test_batch_processor().await.unwrap();
    
    // Submit request
    let req_id = "cancel_me";
    processor.submit_request(BatchRequest {
        id: req_id.to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "This will be cancelled".to_string(),
        max_tokens: 100,
        priority: BatchPriority::Normal,
    }).await.unwrap();
    
    // Cancel before batching
    let cancelled = processor.cancel_request(req_id).await.unwrap();
    assert!(cancelled);
    
    // Submit another request to trigger batching
    processor.submit_request(BatchRequest {
        id: "keep_me".to_string(),
        model_id: "llama-7b".to_string(),
        prompt: "This stays".to_string(),
        max_tokens: 100,
        priority: BatchPriority::Normal,
    }).await.unwrap();
    
    let batch = processor.get_next_batch().await.unwrap();
    
    // Cancelled request should not be in batch
    assert_eq!(batch.requests.len(), 1);
    assert_eq!(batch.requests[0].id, "keep_me");
}

#[tokio::test]
async fn test_queue_overflow() {
    let mut config = BatchConfig::default();
    config.queue_size = 10; // Small queue
    
    let processor = BatchProcessor::new(config).await.unwrap();
    
    // Try to overflow queue
    let mut overflow_count = 0;
    for i in 0..20 {
        let result = processor.submit_request(BatchRequest {
            id: format!("overflow_{}", i),
            model_id: "llama-7b".to_string(),
            prompt: "Overflow test".to_string(),
            max_tokens: 50,
            priority: BatchPriority::Normal,
        }).await;
        
        if result.is_err() {
            overflow_count += 1;
        }
    }
    
    assert!(overflow_count > 0);
}

#[tokio::test]
async fn test_adaptive_batching() {
    let mut config = BatchConfig::default();
    config.batching_strategy = BatchingStrategy::Adaptive;
    
    let processor = BatchProcessor::new(config).await.unwrap();
    
    // Simulate varying load
    for i in 0..5 {
        processor.submit_request(BatchRequest {
            id: format!("adaptive_{}", i),
            model_id: "llama-7b".to_string(),
            prompt: "Adaptive test".to_string(),
            max_tokens: 100,
            priority: BatchPriority::Normal,
        }).await.unwrap();
    }
    
    let batch1 = processor.get_next_batch().await.unwrap();
    
    // Submit high load
    for i in 5..50 {
        processor.submit_request(BatchRequest {
            id: format!("adaptive_{}", i),
            model_id: "llama-7b".to_string(),
            prompt: "High load test".to_string(),
            max_tokens: 100,
            priority: BatchPriority::Normal,
        }).await.unwrap();
    }
    
    let batch2 = processor.get_next_batch().await.unwrap();
    
    // Adaptive strategy should adjust batch size based on load
    assert!(batch2.requests.len() > batch1.requests.len());
}