// tests/models/test_finetuned.rs - Fine-tuned model support tests

use anyhow::Result;
use fabstir_llm_node::models::{
    AdapterConfig, BaseModel, FineTuneCapabilities, FineTuneMetadata, FineTuneRegistry,
    FineTuneStatus, FineTuneType, FineTuneValidationLevel as ValidationLevel,
    FineTuneValidationResult as ValidationResult, FineTuneValidator, FineTunedConfig,
    FineTunedManager, FineTunedModel, GenerationConfig, GenerationResponse, InferenceSession,
    MergeStrategy, ModelAdapter, ModelMerger,
};
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio;

async fn create_test_manager() -> Result<FineTunedManager> {
    let config = FineTunedConfig {
        enable_fine_tuned: true,
        adapter_directory: "/tmp/adapters".into(),
        max_adapters_loaded: 10,
        enable_lora: true,
        enable_qlora: true,
        merge_on_load: false,
        validation_required: true,
        supported_base_models: vec![
            "llama2".to_string(),
            "mistral".to_string(),
            "vicuna".to_string(),
        ],
    };

    FineTunedManager::new(config).await
}

#[tokio::test]
async fn test_finetuned_model_registration() {
    let manager = create_test_manager().await.unwrap();

    // Register a fine-tuned model
    let metadata = FineTuneMetadata {
        base_model: "llama2-7b".to_string(),
        fine_tune_type: FineTuneType::LoRA,
        adapter_path: "/tmp/adapters/medical-lora".into(),
        training_dataset: "medical_qa_v2".to_string(),
        training_steps: 10000,
        learning_rate: 0.0001,
        created_at: chrono::Utc::now(),
        description: "Medical Q&A fine-tune".to_string(),
        tags: vec![
            "medical".to_string(),
            "healthcare".to_string(),
            "qa".to_string(),
        ],
        adapter_size_bytes: 512_000_000, // 512MB
        version: "1.0.0".to_string(),
        parent_version: None,
    };

    let model_id = manager.register_finetuned(metadata).await.unwrap();
    assert!(!model_id.is_empty());

    // Verify registration
    let registered = manager.get_finetuned(&model_id).await.unwrap();
    assert_eq!(registered.metadata.base_model, "llama2-7b");
    assert_eq!(registered.metadata.fine_tune_type, FineTuneType::LoRA);
}

#[tokio::test]
async fn test_adapter_loading() {
    let manager = create_test_manager().await.unwrap();
    let temp_dir = TempDir::new().unwrap();

    // Create mock adapter files
    let adapter_path = temp_dir.path().join("test_adapter");
    std::fs::create_dir_all(&adapter_path).unwrap();
    std::fs::write(
        adapter_path.join("adapter_config.json"),
        r#"{"r": 8, "alpha": 16, "dropout": 0.1, "target_modules": ["q_proj", "v_proj"]}"#,
    )
    .unwrap();
    std::fs::write(adapter_path.join("adapter_model.bin"), vec![0u8; 1024]).unwrap();

    // Register model with adapter
    let mut metadata = FineTuneMetadata::default();
    metadata.adapter_path = adapter_path.clone();
    metadata.fine_tune_type = FineTuneType::LoRA;

    let model_id = manager.register_finetuned(metadata).await.unwrap();

    // Load adapter
    let adapter = manager.load_adapter(&model_id).await.unwrap();
    assert_eq!(adapter.config.r, 8);
    assert_eq!(adapter.config.alpha, 16);
    assert!(adapter.weights.len() > 0);
}

#[tokio::test]
async fn test_base_model_compatibility() {
    let manager = create_test_manager().await.unwrap();

    // Test compatible base model
    let compatible = manager
        .check_base_compatibility("llama2-7b", "llama2-7b-base")
        .await
        .unwrap();
    assert!(compatible);

    // Test incompatible base model
    let incompatible = manager
        .check_base_compatibility("llama2-7b", "gpt2-medium")
        .await
        .unwrap();
    assert!(!incompatible);
}

#[tokio::test]
async fn test_multiple_adapters() {
    let manager = create_test_manager().await.unwrap();

    // Register multiple fine-tuned models
    let adapters = vec![
        ("medical", FineTuneType::LoRA),
        ("legal", FineTuneType::QLoRA),
        ("finance", FineTuneType::LoRA),
    ];

    let mut model_ids = Vec::new();
    for (domain, ft_type) in adapters {
        let mut metadata = FineTuneMetadata::default();
        metadata.base_model = "llama2-7b".to_string();
        metadata.fine_tune_type = ft_type;
        metadata.tags = vec![domain.to_string()];

        let id = manager.register_finetuned(metadata).await.unwrap();
        model_ids.push(id);
    }

    // List all fine-tuned models
    let all_models = manager.list_finetuned().await.unwrap();
    assert!(all_models.len() >= 3);

    // Filter by tag
    let medical_models = manager.find_by_tag("medical").await.unwrap();
    assert_eq!(medical_models.len(), 1);
}

#[tokio::test]
async fn test_adapter_merging() {
    let manager = create_test_manager().await.unwrap();

    // Register a LoRA model
    let mut metadata = FineTuneMetadata::default();
    metadata.base_model = "llama2-7b".to_string();
    metadata.fine_tune_type = FineTuneType::LoRA;
    let model_id = manager.register_finetuned(metadata).await.unwrap();

    // Create merger
    let merger = ModelMerger::new(MergeStrategy::Linear { weight: 0.5 });

    // Merge adapter with base model
    let merged_path = merger
        .merge_with_base(
            &manager.get_base_model_path("llama2-7b").await.unwrap(),
            &model_id,
            &manager,
        )
        .await
        .unwrap();

    assert!(merged_path.exists());
    assert!(merged_path.join("config.json").exists());
}

#[tokio::test]
async fn test_finetuned_inference() {
    let manager = create_test_manager().await.unwrap();

    // Register and load a fine-tuned model
    let mut metadata = FineTuneMetadata::default();
    metadata.base_model = "llama2-7b".to_string();
    metadata.fine_tune_type = FineTuneType::LoRA;
    metadata.tags = vec!["medical".to_string()];

    let model_id = manager.register_finetuned(metadata).await.unwrap();

    // Create inference session with fine-tuned model
    let session = manager.create_inference_session(&model_id).await.unwrap();

    // Test inference
    let prompt = "What are the symptoms of diabetes?";
    let response = session.generate(prompt, Default::default()).await.unwrap();

    assert!(!response.text.is_empty());
    assert!(response.metadata.contains_key("fine_tuned_model"));
    assert_eq!(response.metadata["fine_tuned_model"], model_id);
}

#[tokio::test]
async fn test_adapter_hot_swapping() {
    let manager = create_test_manager().await.unwrap();

    // Load base model
    let base_session = manager.load_base_model("llama2-7b").await.unwrap();

    // Register two different adapters
    let medical_id = manager
        .register_finetuned(FineTuneMetadata {
            base_model: "llama2-7b".to_string(),
            tags: vec!["medical".to_string()],
            ..Default::default()
        })
        .await
        .unwrap();

    let legal_id = manager
        .register_finetuned(FineTuneMetadata {
            base_model: "llama2-7b".to_string(),
            tags: vec!["legal".to_string()],
            ..Default::default()
        })
        .await
        .unwrap();

    // Hot swap between adapters
    base_session.apply_adapter(&medical_id).await.unwrap();
    let medical_response = base_session
        .generate("Diagnose symptoms", Default::default())
        .await
        .unwrap();

    base_session.apply_adapter(&legal_id).await.unwrap();
    let legal_response = base_session
        .generate("Legal precedent for", Default::default())
        .await
        .unwrap();

    // Verify different responses
    assert_ne!(medical_response.text, legal_response.text);
}

#[tokio::test]
async fn test_finetuned_model_validation() {
    let manager = create_test_manager().await.unwrap();
    let validator = FineTuneValidator::new();

    // Create a fine-tuned model
    let metadata = FineTuneMetadata {
        base_model: "llama2-7b".to_string(),
        fine_tune_type: FineTuneType::LoRA,
        training_steps: 10000,
        learning_rate: 0.0001,
        ..Default::default()
    };

    let model_id = manager.register_finetuned(metadata).await.unwrap();

    // Validate the fine-tuned model
    let validation_result = validator
        .validate_finetuned(&model_id, &manager, ValidationLevel::Full)
        .await
        .unwrap();

    assert!(validation_result.is_valid);
    assert!(validation_result.perplexity.is_some());
    assert!(validation_result.adapter_integrity);
}

#[tokio::test]
async fn test_adapter_caching() {
    let manager = create_test_manager().await.unwrap();

    // Register model
    let model_id = manager
        .register_finetuned(Default::default())
        .await
        .unwrap();

    // First load - should load from disk
    let start = std::time::Instant::now();
    let adapter1 = manager.load_adapter(&model_id).await.unwrap();
    let first_load_time = start.elapsed();

    // Second load - should use cache
    let start = std::time::Instant::now();
    let adapter2 = manager.load_adapter(&model_id).await.unwrap();
    let cached_load_time = start.elapsed();

    // Cached load should be much faster
    assert!(cached_load_time < first_load_time / 10);

    // Verify same adapter
    assert_eq!(adapter1.id, adapter2.id);
}

#[tokio::test]
async fn test_finetuned_model_export() {
    let manager = create_test_manager().await.unwrap();
    let temp_dir = TempDir::new().unwrap();

    // Create and register a fine-tuned model
    let model_id = manager
        .register_finetuned(FineTuneMetadata {
            base_model: "llama2-7b".to_string(),
            fine_tune_type: FineTuneType::LoRA,
            description: "Test model for export".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    // Export the model
    let export_path = temp_dir.path().join("exported_model");
    manager
        .export_finetuned(&model_id, &export_path)
        .await
        .unwrap();

    // Verify export contents
    assert!(export_path.exists());
    assert!(export_path.join("metadata.json").exists());
    assert!(export_path.join("adapter").exists());
    assert!(export_path.join("README.md").exists());
}

#[tokio::test]
async fn test_finetuned_capabilities() {
    let manager = create_test_manager().await.unwrap();

    // Register model with specific capabilities
    let mut metadata = FineTuneMetadata::default();
    metadata.base_model = "llama2-7b".to_string();
    metadata.tags = vec![
        "medical".to_string(),
        "diagnosis".to_string(),
        "treatment".to_string(),
    ];

    let model_id = manager.register_finetuned(metadata).await.unwrap();

    // Define capabilities
    let capabilities = FineTuneCapabilities {
        domains: vec!["healthcare".to_string()],
        tasks: vec![
            "qa".to_string(),
            "diagnosis".to_string(),
            "treatment_planning".to_string(),
        ],
        languages: vec!["en".to_string(), "es".to_string()],
        max_context_length: 4096,
        supports_streaming: true,
        supports_function_calling: false,
    };

    manager
        .set_capabilities(&model_id, capabilities.clone())
        .await
        .unwrap();

    // Query capabilities
    let retrieved = manager.get_capabilities(&model_id).await.unwrap();
    assert_eq!(retrieved.domains, capabilities.domains);
    assert_eq!(retrieved.max_context_length, 4096);
}

#[tokio::test]
async fn test_adapter_size_limits() {
    let manager = create_test_manager().await.unwrap();

    // Try to register an adapter that's too large
    let mut metadata = FineTuneMetadata::default();
    metadata.base_model = "llama2-7b".to_string();
    metadata.fine_tune_type = FineTuneType::LoRA;
    metadata.adapter_size_bytes = 10 * 1024 * 1024 * 1024; // 10GB

    let result = manager.register_finetuned(metadata).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
}

#[tokio::test]
async fn test_finetuned_model_versioning() {
    let manager = create_test_manager().await.unwrap();

    // Register initial version
    let v1_metadata = FineTuneMetadata {
        base_model: "llama2-7b".to_string(),
        version: "1.0.0".to_string(),
        ..Default::default()
    };
    let v1_id = manager.register_finetuned(v1_metadata).await.unwrap();

    // Register updated version
    let v2_metadata = FineTuneMetadata {
        base_model: "llama2-7b".to_string(),
        version: "2.0.0".to_string(),
        parent_version: Some(v1_id.clone()),
        ..Default::default()
    };
    let v2_id = manager.register_finetuned(v2_metadata).await.unwrap();

    // List versions
    let versions = manager.get_model_versions(&v1_id).await.unwrap();
    assert_eq!(versions.len(), 2);
    assert!(versions.iter().any(|v| v.metadata.version == "1.0.0"));
    assert!(versions.iter().any(|v| v.metadata.version == "2.0.0"));
}
