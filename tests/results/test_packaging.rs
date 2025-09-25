use chrono::Utc;
use fabstir_llm_node::results::{InferenceResult, PackagedResult, ResultMetadata, ResultPackager};

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_result() -> InferenceResult {
        InferenceResult {
            job_id: "job_12345".to_string(),
            model_id: "llama2-7b".to_string(),
            prompt: "What is the capital of France?".to_string(),
            response: "The capital of France is Paris.".to_string(),
            tokens_generated: 8,
            inference_time_ms: 1250,
            timestamp: Utc::now(),
            node_id: "node_abc123".to_string(),
            metadata: ResultMetadata {
                temperature: 0.7,
                max_tokens: 100,
                top_p: 0.9,
                frequency_penalty: 0.0,
                presence_penalty: 0.0,
            },
        }
    }

    #[tokio::test]
    async fn test_create_inference_result() {
        let result = create_test_result();

        assert_eq!(result.job_id, "job_12345");
        assert_eq!(result.model_id, "llama2-7b");
        assert_eq!(result.tokens_generated, 8);
        assert_eq!(result.inference_time_ms, 1250);
    }

    #[tokio::test]
    async fn test_package_result_with_signature() {
        let packager = ResultPackager::new("node_abc123".to_string());
        let result = create_test_result();

        let packaged = packager.package_result(result.clone()).unwrap();

        assert_eq!(packaged.result, result);
        assert!(!packaged.signature.is_empty());
        assert_eq!(packaged.encoding, "cbor");
        assert_eq!(packaged.version, "1.0");
    }

    #[tokio::test]
    async fn test_verify_packaged_result() {
        let packager = ResultPackager::new("node_abc123".to_string());
        let result = create_test_result();

        let packaged = packager.package_result(result).unwrap();
        let is_valid = packager.verify_package(&packaged).unwrap();

        assert!(is_valid);
    }

    #[tokio::test]
    async fn test_verify_tampered_package_fails() {
        let packager = ResultPackager::new("node_abc123".to_string());
        let result = create_test_result();

        let mut packaged = packager.package_result(result).unwrap();
        // Tamper with the result
        packaged.result.response = "Tampered response".to_string();

        let is_valid = packager.verify_package(&packaged).unwrap();
        assert!(!is_valid);
    }

    #[tokio::test]
    async fn test_cbor_encoding() {
        let packager = ResultPackager::new("node_abc123".to_string());
        let result = create_test_result();

        let encoded = packager.encode_cbor(&result).unwrap();
        assert!(!encoded.is_empty());

        // Should be able to decode back
        let decoded = packager.decode_cbor(&encoded).unwrap();
        assert_eq!(decoded, result);
    }

    #[tokio::test]
    async fn test_cbor_deterministic_encoding() {
        let packager = ResultPackager::new("node_abc123".to_string());
        let result = create_test_result();

        let encoded1 = packager.encode_cbor(&result).unwrap();
        let encoded2 = packager.encode_cbor(&result).unwrap();

        // Same input should produce same CBOR output (deterministic)
        assert_eq!(encoded1, encoded2);
    }

    #[tokio::test]
    async fn test_package_large_result() {
        let packager = ResultPackager::new("node_abc123".to_string());
        let mut result = create_test_result();

        // Create a large response (e.g., 10MB)
        result.response = "x".repeat(10 * 1024 * 1024);
        result.tokens_generated = 2_000_000;

        let packaged = packager.package_result(result.clone()).unwrap();
        assert_eq!(packaged.result.response.len(), 10 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_package_with_special_characters() {
        let packager = ResultPackager::new("node_abc123".to_string());
        let mut result = create_test_result();

        result.response = "Response with émojis 🚀 and special chars: \n\t\"quotes\"".to_string();

        let packaged = packager.package_result(result.clone()).unwrap();
        let verified = packager.verify_package(&packaged).unwrap();

        assert!(verified);
        assert_eq!(packaged.result.response, result.response);
    }
}
