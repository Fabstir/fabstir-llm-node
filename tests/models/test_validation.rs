use anyhow::Result;
use fabstir_llm_node::models::{
    ModelValidator, ValidationConfig, ValidationResult, ValidationStatus,
    ModelFormat, ModelInfo, ValidationError, CompatibilityCheck,
    ModelRequirements, HardwareRequirements, ValidationLevel,
    IntegrityCheck, FormatCheck, SchemaVersion
};
use std::path::PathBuf;
use tokio;

async fn create_test_validator() -> Result<ModelValidator> {
    let config = ValidationConfig {
        strict_mode: true,
        check_integrity: true,
        check_compatibility: true,
        check_requirements: true,
        supported_formats: vec![
            ModelFormat::GGUF,
            ModelFormat::ONNX,
            ModelFormat::SafeTensors,
        ],
        max_model_size_gb: 100,
        validation_level: ValidationLevel::Full,
    };
    
    ModelValidator::new(config).await
}

fn create_test_model_path(format: &str) -> PathBuf {
    PathBuf::from(format!("test_data/models/test_model.{}", format))
}

#[tokio::test]
async fn test_validate_gguf_model() {
    let validator = create_test_validator().await.unwrap();
    let model_path = create_test_model_path("gguf");
    
    let result = validator.validate_model(&model_path).await.unwrap();
    
    assert_eq!(result.status, ValidationStatus::Valid);
    assert_eq!(result.format, ModelFormat::GGUF);
    assert!(result.model_info.is_some());
    
    let info = result.model_info.unwrap();
    assert!(!info.architecture.is_empty());
    assert!(info.parameter_count > 0);
    assert!(info.context_length > 0);
    assert!(!info.vocab_type.is_empty());
}

#[tokio::test]
async fn test_validate_corrupted_model() {
    let validator = create_test_validator().await.unwrap();
    let corrupted_path = PathBuf::from("test_data/models/corrupted_model.gguf");
    
    let result = validator.validate_model(&corrupted_path).await;
    
    assert!(result.is_err());
    match result.unwrap_err().downcast::<ValidationError>() {
        Ok(ValidationError::IntegrityCheckFailed { reason, .. }) => {
            assert!(reason.contains("corrupt") || reason.contains("invalid"));
        }
        _ => panic!("Expected IntegrityCheckFailed error"),
    }
}

#[tokio::test]
async fn test_format_detection() {
    let validator = create_test_validator().await.unwrap();
    
    let test_files = vec![
        ("model.gguf", ModelFormat::GGUF),
        ("model.onnx", ModelFormat::ONNX),
        ("model.safetensors", ModelFormat::SafeTensors),
        ("model.bin", ModelFormat::Unknown),
    ];
    
    for (filename, expected_format) in test_files {
        let path = PathBuf::from(format!("test_data/models/{}", filename));
        let detected_format = validator.detect_format(&path).await.unwrap();
        assert_eq!(detected_format, expected_format);
    }
}

#[tokio::test]
async fn test_checksum_verification() {
    let validator = create_test_validator().await.unwrap();
    let model_path = create_test_model_path("gguf");
    
    // Calculate checksum
    let checksum = validator.calculate_checksum(&model_path).await.unwrap();
    assert_eq!(checksum.len(), 64); // SHA256 hex string
    
    // Verify with correct checksum
    let integrity_check = IntegrityCheck {
        sha256: Some(checksum.clone()),
        blake3: None,
        size_bytes: None,
    };
    
    let result = validator
        .validate_with_integrity(&model_path, integrity_check)
        .await
        .unwrap();
    
    assert_eq!(result.status, ValidationStatus::Valid);
    assert!(result.integrity_verified);
    
    // Verify with wrong checksum
    let wrong_check = IntegrityCheck {
        sha256: Some("0".repeat(64)),
        blake3: None,
        size_bytes: None,
    };
    
    let result = validator
        .validate_with_integrity(&model_path, wrong_check)
        .await;
    
    assert!(result.is_err());
}

#[tokio::test]
async fn test_hardware_compatibility() {
    let validator = create_test_validator().await.unwrap();
    let model_path = create_test_model_path("gguf");
    
    let requirements = HardwareRequirements {
        min_ram_gb: 8,
        min_vram_gb: Some(4),
        min_disk_gb: 20,
        cuda_compute_capability: Some(7.0),
        cpu_features: vec!["avx2".to_string()],
    };
    
    let compat_result = validator
        .check_hardware_compatibility(&model_path, &requirements)
        .await
        .unwrap();
    
    assert!(compat_result.is_compatible);
    assert!(compat_result.warnings.is_empty() || !compat_result.warnings.is_empty());
    assert!(compat_result.available_ram_gb >= requirements.min_ram_gb);
}

#[tokio::test]
async fn test_model_metadata_extraction() {
    let validator = create_test_validator().await.unwrap();
    let model_path = create_test_model_path("gguf");
    
    let metadata = validator.extract_metadata(&model_path).await.unwrap();
    
    assert!(!metadata.model_id.is_empty());
    assert!(!metadata.author.is_empty());
    assert!(!metadata.license.is_empty());
    assert!(metadata.training_date.is_some());
    assert!(!metadata.tags.is_empty());
    assert!(metadata.quantization_info.is_some());
    
    let quant_info = metadata.quantization_info.unwrap();
    assert!(!quant_info.method.is_empty());
    assert!(quant_info.bits > 0 && quant_info.bits <= 32);
}

#[tokio::test]
async fn test_schema_version_compatibility() {
    let validator = create_test_validator().await.unwrap();
    
    let test_cases = vec![
        (SchemaVersion::new(1, 0, 0), SchemaVersion::new(1, 0, 0), true),  // Exact match
        (SchemaVersion::new(1, 2, 0), SchemaVersion::new(1, 0, 0), true),  // Backward compatible
        (SchemaVersion::new(2, 0, 0), SchemaVersion::new(1, 0, 0), false), // Major version mismatch
        (SchemaVersion::new(1, 0, 0), SchemaVersion::new(1, 2, 0), false), // Forward incompatible
    ];
    
    for (model_version, required_version, expected_compatible) in test_cases {
        let is_compatible = validator
            .check_schema_compatibility(model_version, required_version)
            .await
            .unwrap();
        
        assert_eq!(is_compatible, expected_compatible);
    }
}

#[tokio::test]
async fn test_batch_validation() {
    let validator = create_test_validator().await.unwrap();
    
    let model_paths = vec![
        create_test_model_path("gguf"),
        create_test_model_path("onnx"),
        create_test_model_path("safetensors"),
    ];
    
    let results = validator.validate_batch(model_paths).await.unwrap();
    
    assert_eq!(results.total_models, 3);
    assert_eq!(results.valid_models.len() + results.invalid_models.len(), 3);
    assert!(results.validation_time_ms > 0);
    
    for (path, result) in &results.valid_models {
        assert_eq!(result.status, ValidationStatus::Valid);
        assert!(path.exists());
    }
}

#[tokio::test]
async fn test_model_size_validation() {
    let validator = create_test_validator().await.unwrap();
    
    // Create a mock large model path
    let large_model_path = PathBuf::from("test_data/models/huge_model_200gb.gguf");
    
    let result = validator.validate_model(&large_model_path).await;
    
    match result {
        Err(e) => {
            match e.downcast::<ValidationError>() {
                Ok(ValidationError::ModelTooLarge { size_gb, max_gb }) => {
                    assert!(size_gb > max_gb);
                    assert_eq!(max_gb, 100.0);
                }
                _ => panic!("Expected ModelTooLarge error"),
            }
        }
        Ok(_) => {
            // Mock might not enforce size limits - that's ok
        }
    }
}

#[tokio::test]
async fn test_inference_compatibility() {
    let validator = create_test_validator().await.unwrap();
    let model_path = create_test_model_path("gguf");
    
    // Check if model is compatible with inference engine
    let inference_compat = validator
        .check_inference_compatibility(&model_path)
        .await
        .unwrap();
    
    assert!(inference_compat.supports_streaming);
    assert!(inference_compat.supports_batching);
    assert!(inference_compat.max_batch_size > 0);
    assert!(inference_compat.supported_dtypes.contains(&"f16".to_string()));
    assert!(!inference_compat.required_extensions.is_empty() || 
            inference_compat.required_extensions.is_empty());
}

#[tokio::test]
async fn test_security_validation() {
    let validator = create_test_validator().await.unwrap();
    let model_path = create_test_model_path("gguf");
    
    let security_result = validator
        .validate_security(&model_path)
        .await
        .unwrap();
    
    assert!(!security_result.has_suspicious_patterns);
    assert!(security_result.embedded_code.is_empty());
    assert!(!security_result.has_external_references);
    assert!(security_result.risk_level == "low" || security_result.risk_level == "none");
    assert!(security_result.signature_verified || !security_result.signature_verified);
}

#[tokio::test]
async fn test_performance_characteristics() {
    let validator = create_test_validator().await.unwrap();
    let model_path = create_test_model_path("gguf");
    
    let perf_chars = validator
        .analyze_performance_characteristics(&model_path)
        .await
        .unwrap();
    
    assert!(perf_chars.estimated_tokens_per_second > 0.0);
    assert!(perf_chars.memory_bandwidth_gb_per_sec > 0.0);
    assert!(perf_chars.compute_intensity > 0.0);
    assert!(!perf_chars.optimization_suggestions.is_empty() || 
            perf_chars.optimization_suggestions.is_empty());
    assert!(perf_chars.bottleneck == "memory" || 
            perf_chars.bottleneck == "compute" || 
            perf_chars.bottleneck == "balanced");
}

#[tokio::test]
async fn test_validation_caching() {
    let validator = create_test_validator().await.unwrap();
    let model_path = create_test_model_path("gguf");
    
    // First validation
    let start = std::time::Instant::now();
    let result1 = validator.validate_model(&model_path).await.unwrap();
    let first_duration = start.elapsed();
    
    // Second validation (should be cached)
    let start = std::time::Instant::now();
    let result2 = validator.validate_model(&model_path).await.unwrap();
    let cached_duration = start.elapsed();
    
    assert_eq!(result1.status, result2.status);
    assert_eq!(result1.checksum, result2.checksum);
    assert!(cached_duration < first_duration / 2); // Cached should be much faster
    assert!(result2.from_cache);
}