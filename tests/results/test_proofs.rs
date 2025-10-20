// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use chrono::Utc;
use fabstir_llm_node::results::{
    InferenceProof, InferenceResult, PackagedResult, ProofGenerationConfig, ProofGenerator,
    ProofType, ResultMetadata, VerifiableResult,
};
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> ProofGenerationConfig {
        ProofGenerationConfig {
            proof_type: ProofType::Simple,
            model_path: "/models/llama2-7b".to_string(),
            settings_path: None,
            max_proof_size: 1024 * 1024, // 1MB
        }
    }

    fn create_test_result() -> InferenceResult {
        InferenceResult {
            job_id: "job_12345".to_string(),
            model_id: "llama2-7b".to_string(),
            prompt: "What is 2+2?".to_string(),
            response: "2+2 equals 4.".to_string(),
            tokens_generated: 5,
            inference_time_ms: 250,
            timestamp: Utc::now(),
            node_id: "node_abc123".to_string(),
            metadata: ResultMetadata::default(),
        }
    }

    #[tokio::test]
    async fn test_generate_simple_proof() {
        let config = create_test_config();
        let generator = ProofGenerator::new(config, "node_123".to_string());
        let result = create_test_result();

        let proof = generator.generate_proof(&result).await.unwrap();

        assert_eq!(proof.job_id, result.job_id);
        assert_eq!(proof.proof_type, ProofType::Simple);
        assert!(!proof.proof_data.is_empty());
        assert!(!proof.model_hash.is_empty());
        assert!(!proof.input_hash.is_empty());
        assert!(!proof.output_hash.is_empty());
        assert_eq!(proof.prover_id, "node_123");
    }

    #[tokio::test]
    async fn test_create_verifiable_result() {
        let config = create_test_config();
        let generator = ProofGenerator::new(config, "node_123".to_string());

        let result = create_test_result();
        let packaged = PackagedResult {
            result: result.clone(),
            signature: vec![1, 2, 3],
            encoding: "cbor".to_string(),
            version: "1.0".to_string(),
            job_request: None,
        };

        let verifiable = generator
            .create_verifiable_result(packaged.clone())
            .await
            .unwrap();

        assert_eq!(
            verifiable.packaged_result.result.job_id,
            packaged.result.job_id
        );
        assert_eq!(verifiable.proof.job_id, result.job_id);
        assert!(!verifiable.verification_key.is_empty());
    }

    #[tokio::test]
    async fn test_verify_valid_proof() {
        let config = create_test_config();
        let generator = ProofGenerator::new(config, "node_123".to_string());
        let result = create_test_result();

        let proof = generator.generate_proof(&result).await.unwrap();
        let is_valid = generator.verify_proof(&proof, &result).await.unwrap();

        assert!(is_valid);
    }

    #[tokio::test]
    async fn test_verify_invalid_proof_fails() {
        let config = create_test_config();
        let generator = ProofGenerator::new(config, "node_123".to_string());
        let result = create_test_result();

        let proof = generator.generate_proof(&result).await.unwrap();

        // Modify result after proof generation
        let mut modified_result = result.clone();
        modified_result.response = "2+2 equals 5.".to_string();

        let is_valid = generator
            .verify_proof(&proof, &modified_result)
            .await
            .unwrap();

        assert!(!is_valid);
    }

    #[tokio::test]
    async fn test_deterministic_hashing() {
        let config = create_test_config();
        let generator = ProofGenerator::new(config, "node_123".to_string());

        let data = b"test data for hashing";
        let hash1 = generator.compute_data_hash(data);
        let hash2 = generator.compute_data_hash(data);

        // Same data should produce same hash
        assert_eq!(hash1, hash2);

        // Different data should produce different hash
        let different_data = b"different data";
        let hash3 = generator.compute_data_hash(different_data);
        assert_ne!(hash1, hash3);
    }

    #[tokio::test]
    async fn test_model_hash_computation() {
        let config = create_test_config();
        let generator = ProofGenerator::new(config, "node_123".to_string());

        let model_path = PathBuf::from("/models/llama2-7b");
        let model_hash = generator.compute_model_hash(&model_path).unwrap();

        // Should be a valid hash
        assert!(!model_hash.is_empty());
        assert!(model_hash.len() >= 32); // At least 128 bits
    }

    #[tokio::test]
    async fn test_ezkl_proof_generation() {
        let mut config = create_test_config();
        config.proof_type = ProofType::EZKL;
        config.settings_path = Some("/settings/ezkl_settings.json".to_string());

        let generator = ProofGenerator::new(config, "node_123".to_string());
        let result = create_test_result();

        let proof = generator.generate_proof(&result).await.unwrap();

        assert_eq!(proof.proof_type, ProofType::EZKL);
        // EZKL proofs are typically larger
        assert!(proof.proof_data.len() > 100);
    }

    #[tokio::test]
    async fn test_proof_size_limit() {
        let config = ProofGenerationConfig {
            proof_type: ProofType::Simple,
            model_path: "/models/llama2-7b".to_string(),
            settings_path: None,
            max_proof_size: 1024, // 1KB limit
        };

        let generator = ProofGenerator::new(config, "node_123".to_string());
        let result = create_test_result();

        let proof = generator.generate_proof(&result).await.unwrap();

        // Proof should respect size limit
        assert!(proof.proof_data.len() <= 1024);
    }

    #[tokio::test]
    async fn test_concurrent_proof_generation() {
        let config = create_test_config();
        let generator = ProofGenerator::new(config, "node_123".to_string());

        // Generate proofs for multiple results concurrently
        let mut handles = vec![];
        for i in 0..5 {
            let generator = generator.clone();
            let mut result = create_test_result();
            result.job_id = format!("job_{}", i);

            let handle = tokio::spawn(async move { generator.generate_proof(&result).await });
            handles.push(handle);
        }

        // All should succeed
        for (i, handle) in handles.into_iter().enumerate() {
            let proof = handle.await.unwrap().unwrap();
            assert_eq!(proof.job_id, format!("job_{}", i));
        }
    }

    #[tokio::test]
    async fn test_proof_timestamp_ordering() {
        let config = create_test_config();
        let generator = ProofGenerator::new(config, "node_123".to_string());
        let result = create_test_result();

        let proof1 = generator.generate_proof(&result).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let proof2 = generator.generate_proof(&result).await.unwrap();

        // Later proof should have later timestamp
        assert!(proof2.timestamp > proof1.timestamp);
    }
}
