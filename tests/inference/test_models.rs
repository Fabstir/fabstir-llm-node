use fabstir_llm_node::inference::{
    ModelManager, ModelRegistry, ModelInfo, ModelSource,
    ModelRequirements, ModelStatus, DownloadProgress
};
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_model_registry_initialization() {
    let models_dir = PathBuf::from("./models");
    let registry = ModelRegistry::new(models_dir.clone())
        .await
        .expect("Failed to create model registry");
    
    // Should scan existing models
    let available_models = registry.list_available_models();
    
    // Registry should be initialized
    assert!(registry.is_initialized());
    assert_eq!(registry.models_directory(), &models_dir);
}

#[tokio::test]
async fn test_model_discovery() {
    let registry = ModelRegistry::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create registry");
    
    // Scan for models
    registry.scan_models_directory()
        .await
        .expect("Failed to scan models");
    
    let models = registry.list_available_models();
    
    // Should find test models
    for model in &models {
        assert!(model.path.exists());
        assert!(model.path.extension().map(|e| e == "gguf").unwrap_or(false));
        assert!(model.size_bytes > 0);
        assert!(!model.model_type.is_empty());
    }
}

#[tokio::test]
async fn test_model_metadata_extraction() {
    let registry = ModelRegistry::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create registry");
    
    // Add a test model
    let model_path = PathBuf::from("./models/llama-2-7b-q4_0.gguf");
    
    let metadata = registry.extract_model_metadata(&model_path)
        .await
        .expect("Failed to extract metadata");
    
    // Should extract GGUF metadata
    assert!(!metadata.architecture.is_empty());
    assert!(metadata.parameter_count > 0);
    assert!(metadata.quantization.contains("q4") || metadata.quantization.contains("Q4"));
    assert!(metadata.context_length > 0);
    assert!(!metadata.tensor_info.is_empty());
}

#[tokio::test]
async fn test_model_download() {
    let manager = ModelManager::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create manager");
    
    // Download a small test model
    let source = ModelSource::HuggingFace {
        repo_id: "TheBloke/TinyLlama-1.1B-GGUF".to_string(),
        filename: "tinyllama-1.1b.Q4_0.gguf".to_string(),
        revision: None,
    };
    
    let mut progress_receiver = manager.download_model(source, None).await;
    
    // Track download progress
    let mut last_progress = 0.0;
    
    while let Some(progress) = progress_receiver.recv().await {
        match progress {
            DownloadProgress::Progress { percent, bytes_downloaded, total_bytes } => {
                assert!(percent >= last_progress);
                assert!(percent <= 100.0);
                last_progress = percent;
            }
            DownloadProgress::Completed { path, hash } => {
                assert!(path.exists());
                assert!(!hash.is_empty());
                break;
            }
            DownloadProgress::Failed { error } => {
                panic!("Download failed: {}", error);
            }
        }
    }
    
    assert_eq!(last_progress, 100.0);
}

#[tokio::test]
async fn test_model_verification() {
    let manager = ModelManager::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create manager");
    
    let model_path = PathBuf::from("./models/llama-2-7b-q4_0.gguf");
    
    // Verify model integrity
    let is_valid = manager.verify_model(&model_path, None)
        .await
        .expect("Failed to verify model");
    
    assert!(is_valid);
    
    // Test with checksum
    let checksum = manager.calculate_checksum(&model_path)
        .await
        .expect("Failed to calculate checksum");
    
    let is_valid = manager.verify_model(&model_path, Some(&checksum))
        .await
        .expect("Failed to verify model");
    
    assert!(is_valid);
}

#[tokio::test]
async fn test_model_requirements_check() {
    let manager = ModelManager::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create manager");
    
    // Check system requirements for different models
    let test_cases = vec![
        ("llama-7b-q4_0.gguf", 8 * 1024 * 1024 * 1024), // 8GB
        ("llama-13b-q4_0.gguf", 16 * 1024 * 1024 * 1024), // 16GB
        ("llama-70b-q4_0.gguf", 64 * 1024 * 1024 * 1024), // 64GB
    ];
    
    for (model_name, min_memory) in test_cases {
        let requirements = ModelRequirements {
            min_memory_bytes: min_memory,
            recommended_memory_bytes: min_memory * 2,
            requires_gpu: false,
            min_gpu_memory_bytes: 0,
            supported_backends: vec!["cpu".to_string(), "cuda".to_string()],
        };
        
        let can_run = manager.check_system_requirements(&requirements).await;
        
        // Should depend on actual system
        if can_run {
            let system_info = manager.get_system_info().await;
            assert!(system_info.total_memory >= min_memory);
        }
    }
}

#[tokio::test]
async fn test_model_preloading() {
    let manager = ModelManager::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create manager");
    
    let model_path = PathBuf::from("./models/llama-2-7b-q4_0.gguf");
    
    // Preload model into memory
    let preload_handle = manager.preload_model(&model_path)
        .await
        .expect("Failed to start preloading");
    
    // Wait for preload
    let preloaded_path = preload_handle.await
        .expect("Failed to preload model");
    
    assert_eq!(preloaded_path, model_path);
    
    // Should be cached in memory
    let is_cached = manager.is_model_cached(&model_path);
    assert!(is_cached);
}

#[tokio::test]
async fn test_model_conversion() {
    let manager = ModelManager::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create manager");
    
    // Test conversion from different formats
    let source_path = PathBuf::from("./models/test_model.bin");
    let target_format = "gguf";
    
    // This would require actual conversion tools
    let result = manager.convert_model(&source_path, target_format).await;
    
    match result {
        Ok(converted_path) => {
            assert!(converted_path.extension().map(|e| e == "gguf").unwrap_or(false));
        }
        Err(e) => {
            // Conversion tools might not be available
            assert!(e.to_string().contains("conversion") || e.to_string().contains("not supported"));
        }
    }
}

#[tokio::test]
async fn test_model_quantization() {
    let manager = ModelManager::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create manager");
    
    let source_model = PathBuf::from("./models/llama-2-7b-f16.gguf");
    
    // Test different quantization levels
    let quant_levels = vec!["q4_0", "q4_1", "q5_0", "q5_1", "q8_0"];
    
    for level in quant_levels {
        let result = manager.quantize_model(&source_model, level).await;
        
        match result {
            Ok(quantized_path) => {
                assert!(quantized_path.to_string_lossy().contains(level));
                
                // Quantized model should be smaller
                let source_size = tokio::fs::metadata(&source_model).await.unwrap().len();
                let quant_size = tokio::fs::metadata(&quantized_path).await.unwrap().len();
                assert!(quant_size < source_size);
            }
            Err(e) => {
                // Quantization might not be available
                assert!(e.to_string().contains("quantization") || e.to_string().contains("not found"));
            }
        }
    }
}

#[tokio::test]
async fn test_model_auto_download() {
    let manager = ModelManager::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create manager");
    
    // Configure auto-download
    manager.set_auto_download(true);
    manager.set_preferred_sources(vec![
        ModelSource::HuggingFace {
            repo_id: "TheBloke".to_string(),
            filename: String::new(),
            revision: None,
        }
    ]);
    
    // Request a model that might not exist locally
    let model_request = "mistral-7b-instruct-q4_0";
    
    let model_path = manager.ensure_model_available(model_request)
        .await
        .expect("Failed to ensure model");
    
    // Should either find or download the model
    assert!(model_path.exists());
}

#[tokio::test]
async fn test_model_lifecycle_events() {
    let manager = ModelManager::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create manager");
    
    // Subscribe to model events
    let mut event_receiver = manager.subscribe_events().await;
    
    // Trigger some events
    let model_path = PathBuf::from("./models/test-model.gguf");
    
    // Simulate model operations
    manager.register_model(&model_path).await.unwrap();
    manager.mark_model_loaded(&model_path).await;
    manager.mark_model_unloaded(&model_path).await;
    
    // Collect events
    let mut events = Vec::new();
    while let Ok(Some(event)) = timeout(Duration::from_millis(100), event_receiver.recv()).await {
        events.push(event);
    }
    
    // Should have lifecycle events
    assert!(events.iter().any(|e| matches!(e, ModelEvent::Registered { .. })));
    assert!(events.iter().any(|e| matches!(e, ModelEvent::Loaded { .. })));
    assert!(events.iter().any(|e| matches!(e, ModelEvent::Unloaded { .. })));
}

#[tokio::test]
async fn test_model_storage_management() {
    let manager = ModelManager::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create manager");
    
    // Set storage limits
    manager.set_max_storage_bytes(50 * 1024 * 1024 * 1024); // 50GB
    manager.set_cleanup_threshold(0.9); // Clean up at 90% full
    
    // Check current usage
    let usage = manager.get_storage_usage()
        .await
        .expect("Failed to get storage usage");
    
    assert!(usage.total_bytes > 0);
    assert!(usage.used_bytes <= usage.total_bytes);
    assert!(usage.model_bytes <= usage.used_bytes);
    
    // List models by size
    let models_by_size = manager.list_models_by_size().await;
    
    // Should be sorted largest first
    for i in 1..models_by_size.len() {
        assert!(models_by_size[i-1].1 >= models_by_size[i].1);
    }
}

#[tokio::test]
async fn test_model_cleanup() {
    let manager = ModelManager::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create manager");
    
    // Configure cleanup policy
    manager.set_cleanup_policy(CleanupPolicy {
        max_age: Duration::from_days(30),
        max_unused: Duration::from_days(7),
        keep_popular: 5,
        keep_recent: 3,
    });
    
    // Run cleanup
    let cleaned = manager.cleanup_old_models()
        .await
        .expect("Failed to cleanup models");
    
    // Should report what was cleaned
    assert!(cleaned.bytes_freed >= 0);
    assert!(cleaned.models_removed >= 0);
    
    // Popular and recent models should remain
    let remaining = manager.list_available_models().await;
    assert!(remaining.len() >= 3); // At least keep_recent models
}

#[tokio::test]
async fn test_model_aliasing() {
    let manager = ModelManager::new(PathBuf::from("./models"))
        .await
        .expect("Failed to create manager");
    
    let model_path = PathBuf::from("./models/llama-2-7b-chat.Q4_0.gguf");
    
    // Create aliases
    manager.add_model_alias("llama2-chat", &model_path)
        .await
        .expect("Failed to add alias");
    
    manager.add_model_alias("chat-model", &model_path)
        .await
        .expect("Failed to add alias");
    
    // Resolve aliases
    let resolved = manager.resolve_model_alias("llama2-chat")
        .await
        .expect("Failed to resolve alias");
    
    assert_eq!(resolved, model_path);
    
    // List all aliases
    let aliases = manager.list_model_aliases().await;
    assert!(aliases.contains_key("llama2-chat"));
    assert!(aliases.contains_key("chat-model"));
}

// Helper types for tests
use fabstir_llm_node::inference::{ModelEvent, CleanupPolicy};