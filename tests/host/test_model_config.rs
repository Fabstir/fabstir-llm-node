use fabstir_llm_node::host::{
    HostingError, ModelConfig, ModelHostingManager, ModelMetadata, ModelParameters, ModelStatus,
};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_model_config() -> ModelConfig {
        ModelConfig {
            model_id: "llama-3.2-1b-instruct".to_string(),
            model_path: "/models/llama-3.2-1b-instruct.gguf".to_string(),
            parameters: ModelParameters {
                max_tokens: 2048,
                temperature_range: (0.0, 2.0),
                top_p_range: (0.0, 1.0),
                top_k_range: (1, 100),
                repeat_penalty_range: (0.0, 2.0),
                context_size: 4096,
                gpu_layers: Some(32),
            },
            metadata: ModelMetadata {
                description: "Fast 1B parameter instruction-tuned model".to_string(),
                tags: vec!["instruct".to_string(), "fast".to_string()],
                capabilities: vec!["chat".to_string(), "completion".to_string()],
                languages: vec!["en".to_string()],
                version: "3.2".to_string(),
            },
            status: ModelStatus::Enabled,
        }
    }

    #[tokio::test]
    async fn test_add_model_configuration() {
        let mut manager = ModelHostingManager::new();
        let config = create_test_model_config();

        let result = manager.add_model(config.clone()).await;
        assert!(result.is_ok());

        let hosted_models = manager.list_models().await;
        assert_eq!(hosted_models.len(), 1);
        assert_eq!(hosted_models[0].model_id, "llama-3.2-1b-instruct");
    }

    #[tokio::test]
    async fn test_remove_model() {
        let mut manager = ModelHostingManager::new();
        let config = create_test_model_config();

        manager.add_model(config).await.unwrap();

        let result = manager.remove_model("llama-3.2-1b-instruct").await;
        assert!(result.is_ok());

        let hosted_models = manager.list_models().await;
        assert_eq!(hosted_models.len(), 0);
    }

    #[tokio::test]
    async fn test_update_model_parameters() {
        let mut manager = ModelHostingManager::new();
        let config = create_test_model_config();

        manager.add_model(config).await.unwrap();

        let new_params = ModelParameters {
            max_tokens: 4096,
            temperature_range: (0.0, 1.5),
            top_p_range: (0.1, 0.9),
            top_k_range: (10, 50),
            repeat_penalty_range: (1.0, 1.5),
            context_size: 8192,
            gpu_layers: Some(48),
        };

        let result = manager
            .update_model_parameters("llama-3.2-1b-instruct", new_params)
            .await;
        assert!(result.is_ok());

        let model = manager.get_model("llama-3.2-1b-instruct").await.unwrap();
        assert_eq!(model.parameters.max_tokens, 4096);
        assert_eq!(model.parameters.context_size, 8192);
    }

    #[tokio::test]
    async fn test_enable_disable_model() {
        let mut manager = ModelHostingManager::new();
        let config = create_test_model_config();

        manager.add_model(config).await.unwrap();

        // Disable model
        let result = manager
            .set_model_status("llama-3.2-1b-instruct", ModelStatus::Disabled)
            .await;
        assert!(result.is_ok());

        let model = manager.get_model("llama-3.2-1b-instruct").await.unwrap();
        assert_eq!(model.status, ModelStatus::Disabled);

        // Enable model
        let result = manager
            .set_model_status("llama-3.2-1b-instruct", ModelStatus::Enabled)
            .await;
        assert!(result.is_ok());

        let model = manager.get_model("llama-3.2-1b-instruct").await.unwrap();
        assert_eq!(model.status, ModelStatus::Enabled);
    }

    #[tokio::test]
    async fn test_model_validation() {
        let mut manager = ModelHostingManager::new();

        // Invalid model path
        let mut config = create_test_model_config();
        config.model_path = "/invalid/path.gguf".to_string();

        let result = manager.add_model(config).await;
        assert!(matches!(result, Err(HostingError::ModelNotFound(_))));
    }

    #[tokio::test]
    async fn test_multiple_models() {
        let mut manager = ModelHostingManager::new();

        // Add multiple models
        let models = vec![
            ("llama-3.2-1b-instruct", "/models/llama-3.2-1b.gguf"),
            ("mistral-7b-instruct", "/models/mistral-7b.gguf"),
            ("llama-2-70b-chat", "/models/llama-2-70b.gguf"),
        ];

        for (id, path) in models {
            let mut config = create_test_model_config();
            config.model_id = id.to_string();
            config.model_path = path.to_string();
            manager.add_model(config).await.unwrap();
        }

        let hosted_models = manager.list_models().await;
        assert_eq!(hosted_models.len(), 3);

        // Filter by status
        let enabled_models = manager.list_models_by_status(ModelStatus::Enabled).await;
        assert_eq!(enabled_models.len(), 3);
    }

    #[tokio::test]
    async fn test_model_metadata_update() {
        let mut manager = ModelHostingManager::new();
        let config = create_test_model_config();

        manager.add_model(config).await.unwrap();

        let new_metadata = ModelMetadata {
            description: "Updated description".to_string(),
            tags: vec![
                "updated".to_string(),
                "fast".to_string(),
                "efficient".to_string(),
            ],
            capabilities: vec![
                "chat".to_string(),
                "completion".to_string(),
                "reasoning".to_string(),
            ],
            languages: vec!["en".to_string(), "es".to_string(), "fr".to_string()],
            version: "3.2.1".to_string(),
        };

        let result = manager
            .update_model_metadata("llama-3.2-1b-instruct", new_metadata.clone())
            .await;
        assert!(result.is_ok());

        let model = manager.get_model("llama-3.2-1b-instruct").await.unwrap();
        assert_eq!(model.metadata.tags.len(), 3);
        assert_eq!(model.metadata.languages.len(), 3);
    }

    #[tokio::test]
    async fn test_model_config_persistence() {
        let mut manager = ModelHostingManager::new();
        let config = create_test_model_config();

        manager.add_model(config).await.unwrap();

        // Save configuration
        let result = manager.save_config("/tmp/model_config.json").await;
        assert!(result.is_ok());

        // Load configuration
        let mut new_manager = ModelHostingManager::new();
        let result = new_manager.load_config("/tmp/model_config.json").await;
        assert!(result.is_ok());

        let models = new_manager.list_models().await;
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_id, "llama-3.2-1b-instruct");
    }
}
