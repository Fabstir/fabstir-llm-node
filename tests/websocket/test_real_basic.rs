use fabstir_llm_node::api::websocket::inference::{
    InferenceEngine, InferenceConfig, InferenceRequest,
};
use fabstir_llm_node::api::websocket::job_verification::{
    JobVerifier, JobVerificationConfig, JobStatus,
};
use std::path::PathBuf;
use std::time::Duration;
use std::collections::HashMap;

#[tokio::test]
async fn test_basic_inference_without_model() {
    // Test that we can create the engine structure even without a model
    let config = InferenceConfig {
        model_path: PathBuf::from("models/non-existent.gguf"),
        context_size: 2048,
        max_tokens: 256,
        temperature: 0.7,
        gpu_layers: 0,
        use_gpu: false,
    };
    
    let result = InferenceEngine::new(config).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("not found"));
    }
}

#[tokio::test]
async fn test_job_verification_disabled_mode() {
    // Test with verification disabled - should always work
    let mut marketplace_addresses = HashMap::new();
    marketplace_addresses.insert(84532, "0x0000000000000000000000000000000000000000".to_string());

    let config = JobVerificationConfig {
        enabled: false,
        blockchain_verification: false,
        cache_duration: Duration::from_secs(60),
        marketplace_addresses,
        supported_chains: vec![84532],
    };

    let verifier = JobVerifier::new(config).await.unwrap();

    // When disabled, all jobs should verify successfully
    let job = verifier.verify_job(12345, 84532).await.unwrap();
    assert_eq!(job.job_id, 12345);
    assert_eq!(job.status, JobStatus::Pending);
    
    // Can claim job when disabled
    assert!(verifier.can_claim_job(&job).await);
}

#[tokio::test]
async fn test_inference_request_creation() {
    // Test that we can create requests without needing a real engine
    let request = InferenceRequest {
        prompt: "Hello, world!".to_string(),
        max_tokens: 50,
        temperature: Some(0.7),
        stream: false,
    };
    
    assert_eq!(request.prompt, "Hello, world!");
    assert_eq!(request.max_tokens, 50);
    assert_eq!(request.temperature, Some(0.7));
    assert!(!request.stream);
}

#[tokio::test]
async fn test_job_status_transitions() {
    use fabstir_llm_node::api::websocket::job_verification::JobState;
    
    // Test status conversions
    assert_eq!(JobStatus::from(JobState::Open), JobStatus::Pending);
    assert_eq!(JobStatus::from(JobState::Assigned), JobStatus::Claimed);
    assert_eq!(JobStatus::from(JobState::Completed), JobStatus::Completed);
    assert_eq!(JobStatus::from(JobState::Cancelled), JobStatus::Failed);
    assert_eq!(JobStatus::from(JobState::Disputed), JobStatus::Failed);
}