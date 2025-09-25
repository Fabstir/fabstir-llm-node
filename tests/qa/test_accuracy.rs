use fabstir_llm_node::qa::{
    AccuracyAlert, AccuracyError, AccuracyMetrics, AccuracyVerifier, ConsistencyCheck,
    QualityScore, SamplingStrategy, ValidationRule, VerificationConfig, VerificationMethod,
    VerificationResult,
};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> VerificationConfig {
        VerificationConfig {
            sampling_rate: 0.1, // 10% of responses
            verification_methods: vec![
                VerificationMethod::GroundTruth,
                VerificationMethod::ConsistencyCheck,
                VerificationMethod::FormatValidation,
                VerificationMethod::SemanticSimilarity,
            ],
            accuracy_threshold: 0.95,
            consistency_threshold: 0.90,
            batch_size: 100,
            async_verification: true,
            store_results: true,
        }
    }

    fn create_test_response() -> (String, String) {
        let prompt = "What is the capital of France?".to_string();
        let response = "The capital of France is Paris.".to_string();
        (prompt, response)
    }

    #[tokio::test]
    async fn test_basic_verification() {
        let config = create_test_config();
        let verifier = AccuracyVerifier::new(config);

        let (prompt, response) = create_test_response();

        let result = verifier
            .verify_response("job-123", &prompt, &response, Some("Paris"))
            .await;

        assert!(result.is_ok());
        let verification = result.unwrap();
        assert!(verification.is_accurate);
        assert!(verification.accuracy_score > 0.9);
    }

    #[tokio::test]
    async fn test_sampling_decision() {
        let mut config = create_test_config();
        config.sampling_rate = 0.5; // 50% sampling

        let verifier = AccuracyVerifier::new(config);

        let mut sampled_count = 0;
        let total_requests = 1000;

        for i in 0..total_requests {
            let should_sample = verifier.should_sample_request(&format!("job-{}", i)).await;
            if should_sample {
                sampled_count += 1;
            }
        }

        // Should be approximately 50% (with some variance)
        let sample_rate = sampled_count as f64 / total_requests as f64;
        assert!(sample_rate > 0.45 && sample_rate < 0.55);
    }

    #[tokio::test]
    async fn test_ground_truth_verification() {
        let config = create_test_config();
        let verifier = AccuracyVerifier::new(config);

        // Test with correct answer
        let result = verifier
            .verify_against_ground_truth("The capital of France is Paris.", "Paris")
            .await;

        assert!(result.is_accurate);
        assert!(result.confidence > 0.9);

        // Test with incorrect answer
        let result = verifier
            .verify_against_ground_truth("The capital of France is London.", "Paris")
            .await;

        assert!(!result.is_accurate);
        assert!(result.confidence < 0.5);
    }

    #[tokio::test]
    async fn test_consistency_check() {
        let config = create_test_config();
        let verifier = AccuracyVerifier::new(config);

        let prompt = "What is 2 + 2?";
        let responses = vec![
            "2 + 2 equals 4",
            "The sum of 2 and 2 is 4",
            "2 plus 2 is 4",
            "Two plus two equals four",
            "The answer is 4",
        ];

        let consistency = verifier.check_consistency(prompt, &responses).await;

        assert!(consistency.is_ok());
        let check = consistency.unwrap();
        assert!(check.is_consistent);
        assert!(check.consistency_score > 0.9);
        assert_eq!(check.canonical_answer, Some("4".to_string()));
    }

    #[tokio::test]
    async fn test_format_validation() {
        let config = create_test_config();
        let verifier = AccuracyVerifier::new(config);

        // Define validation rules
        let json_rule = ValidationRule {
            name: "json_format".to_string(),
            pattern: r"^\{.*\}$".to_string(),
            required_fields: vec!["result".to_string()],
            format_type: "json".to_string(),
        };

        verifier
            .add_validation_rule("json_response", json_rule)
            .await;

        // Test valid JSON
        let valid_response = r#"{"result": "success", "value": 42}"#;
        let result = verifier
            .validate_format("json_response", valid_response)
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_valid);

        // Test invalid JSON
        let invalid_response = "This is not JSON";
        let result = verifier
            .validate_format("json_response", invalid_response)
            .await;
        assert!(result.is_ok());
        assert!(!result.unwrap().is_valid);
    }

    #[tokio::test]
    async fn test_semantic_similarity() {
        let config = create_test_config();
        let verifier = AccuracyVerifier::new(config);

        let reference = "The weather is sunny and warm today.";

        let similar_responses = vec![
            ("It's a beautiful sunny day with warm temperatures.", 0.8),
            ("Today's weather is warm and sunny.", 0.9),
            ("The sun is shining and it's quite warm.", 0.85),
        ];

        for (response, min_similarity) in similar_responses {
            let similarity = verifier
                .calculate_semantic_similarity(reference, response)
                .await;

            assert!(similarity.is_ok());
            assert!(similarity.unwrap() > min_similarity);
        }

        // Test dissimilar response
        let dissimilar = "The stock market crashed today.";
        let similarity = verifier
            .calculate_semantic_similarity(reference, dissimilar)
            .await;

        assert!(similarity.is_ok());
        assert!(similarity.unwrap() < 0.3);
    }

    #[tokio::test]
    async fn test_accuracy_metrics_aggregation() {
        let config = create_test_config();
        let verifier = AccuracyVerifier::new(config);

        // Simulate verification results
        let results = vec![
            (true, 0.95),  // Accurate
            (true, 0.92),  // Accurate
            (false, 0.45), // Inaccurate
            (true, 0.88),  // Accurate
            (true, 0.91),  // Accurate
            (false, 0.32), // Inaccurate
            (true, 0.94),  // Accurate
            (true, 0.89),  // Accurate
            (true, 0.93),  // Accurate
            (true, 0.90),  // Accurate
        ];

        for (i, (is_accurate, score)) in results.iter().enumerate() {
            verifier
                .record_verification_result(&format!("job-{}", i), *is_accurate, *score)
                .await
                .unwrap();
        }

        let metrics = verifier.get_accuracy_metrics("1hour").await;

        assert_eq!(metrics.total_verifications, 10);
        assert_eq!(metrics.accurate_count, 8);
        assert_eq!(metrics.accuracy_rate, 0.8);
        assert!(metrics.average_confidence > 0.7);
    }

    #[tokio::test]
    async fn test_accuracy_alerts() {
        let mut config = create_test_config();
        config.accuracy_threshold = 0.9;

        let verifier = AccuracyVerifier::new(config);

        // Subscribe to alerts
        let mut alert_receiver = verifier.subscribe_to_alerts().await;

        // Record many inaccurate results
        for i in 0..20 {
            let is_accurate = i < 5; // Only first 5 are accurate (25%)
            verifier
                .record_verification_result(
                    &format!("job-{}", i),
                    is_accurate,
                    if is_accurate { 0.9 } else { 0.3 },
                )
                .await
                .unwrap();
        }

        // Should receive accuracy alert
        let alert =
            tokio::time::timeout(std::time::Duration::from_secs(1), alert_receiver.recv()).await;

        assert!(alert.is_ok());
        let alert_data = alert.unwrap().unwrap();
        assert!(alert_data.current_accuracy < 0.9);
        assert_eq!(alert_data.threshold, 0.9);
    }

    #[tokio::test]
    async fn test_model_specific_accuracy() {
        let config = create_test_config();
        let verifier = AccuracyVerifier::new(config);

        // Record results for different models
        let model_results = vec![
            ("llama-3.2-1b", vec![true, true, false, true, true]), // 80%
            ("mistral-7b", vec![true, true, true, true, false]),   // 80%
            ("llama-70b", vec![true, true, true, true, true]),     // 100%
        ];

        for (model, results) in model_results {
            for (i, is_accurate) in results.iter().enumerate() {
                verifier
                    .record_model_verification(
                        model,
                        &format!("job-{}-{}", model, i),
                        *is_accurate,
                        0.9,
                    )
                    .await
                    .unwrap();
            }
        }

        // Get model-specific metrics
        let llama_small = verifier.get_model_accuracy("llama-3.2-1b").await;
        let mistral = verifier.get_model_accuracy("mistral-7b").await;
        let llama_large = verifier.get_model_accuracy("llama-70b").await;

        assert_eq!(llama_small.accuracy_rate, 0.8);
        assert_eq!(mistral.accuracy_rate, 0.8);
        assert_eq!(llama_large.accuracy_rate, 1.0);
    }

    #[tokio::test]
    async fn test_verification_queue() {
        let config = create_test_config();
        let verifier = AccuracyVerifier::new(config);

        // Queue multiple verifications
        let mut handles = vec![];

        for i in 0..10 {
            let verifier_clone = verifier.clone();
            let handle = tokio::spawn(async move {
                verifier_clone
                    .queue_verification(&format!("job-{}", i), "prompt", "response", None)
                    .await
            });
            handles.push(handle);
        }

        // Wait for all to complete
        for handle in handles {
            assert!(handle.await.is_ok());
        }

        let queue_stats = verifier.get_queue_statistics().await;
        assert_eq!(queue_stats.total_queued, 10);
        assert_eq!(queue_stats.pending, 0);
    }

    #[tokio::test]
    async fn test_verification_history() {
        let config = create_test_config();
        let verifier = AccuracyVerifier::new(config);

        // Perform verifications
        for i in 0..5 {
            let (prompt, response) = create_test_response();
            verifier
                .verify_response(&format!("job-{}", i), &prompt, &response, Some("Paris"))
                .await
                .unwrap();
        }

        // Get verification history
        let history = verifier.get_verification_history(10).await;

        assert_eq!(history.len(), 5);
        assert!(history[0].timestamp > history[4].timestamp); // Most recent first

        // Export history
        let export = verifier.export_verification_data("csv").await;
        assert!(export.is_ok());
        assert!(export.unwrap().contains("job_id,accuracy,score"));
    }
}
