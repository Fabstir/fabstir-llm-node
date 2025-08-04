// tests/models/test_specialization.rs - Model specialization tests

use anyhow::Result;
use fabstir_llm_node::models::{
    SpecializationManager, SpecializationConfig, ModelSpecialization,
    DomainType, TaskType, LanguageSupport, IndustryVertical,
    SpecializedModel, SpecializationMetrics, TokenizerConfig,
    InferencePipeline, EnsembleStrategy, BenchmarkResult,
    QueryAnalyzer, CostOptimizer, SpecializationMarketplace,
    PerformanceProfile, AccuracyRequirement, SpecializedRouter,
};
use std::collections::{HashMap, HashSet};
use tokio;

async fn create_test_manager() -> Result<SpecializationManager> {
    let config = SpecializationConfig {
        enable_specialization: true,
        auto_routing: true,
        benchmark_on_registration: true,
        cost_optimization: true,
        marketplace_enabled: true,
        supported_domains: vec![
            DomainType::Medical,
            DomainType::Legal,
            DomainType::Financial,
            DomainType::Scientific,
        ],
        performance_monitoring: true,
    };
    
    SpecializationManager::new(config).await
}

#[tokio::test]
async fn test_domain_specific_model_registration() {
    let manager = create_test_manager().await.unwrap();
    
    // Register a medical domain model
    let medical_model = ModelSpecialization {
        model_id: "llama-medical-v1".to_string(),
        base_model: "llama2-70b".to_string(),
        domain: DomainType::Medical,
        tasks: vec![
            TaskType::Diagnosis,
            TaskType::TreatmentPlanning,
            TaskType::MedicalQA,
        ],
        accuracy_score: 0.95,
        speed_multiplier: 0.8, // Slightly slower but more accurate
        specialized_tokens: 50_000, // Medical terminology
        training_hours: 1000,
    };
    
    let registration = manager.register_specialization(medical_model).await.unwrap();
    assert!(!registration.id.is_empty());
    assert_eq!(registration.status, "active");
    
    // Verify specialization is searchable
    let medical_models = manager.find_by_domain(DomainType::Medical).await.unwrap();
    assert!(medical_models.len() >= 1);
    assert!(medical_models.iter().any(|m| m.model_id == "llama-medical-v1"));
}

#[tokio::test]
async fn test_task_specific_optimization() {
    let manager = create_test_manager().await.unwrap();
    
    // Register models optimized for specific tasks
    let code_model = ModelSpecialization {
        model_id: "codellama-optimized".to_string(),
        base_model: "codellama-34b".to_string(),
        domain: DomainType::Technical,
        tasks: vec![
            TaskType::CodeGeneration,
            TaskType::CodeReview,
            TaskType::BugDetection,
        ],
        accuracy_score: 0.92,
        speed_multiplier: 1.5, // Optimized for speed
        specialized_tokens: 30_000, // Programming languages
        training_hours: 500,
    };
    
    manager.register_specialization(code_model).await.unwrap();
    
    // Test task-specific routing
    let best_model = manager.find_best_for_task(
        TaskType::CodeGeneration,
        AccuracyRequirement::High,
    ).await.unwrap();
    
    assert_eq!(best_model.model_id, "codellama-optimized");
    assert!(best_model.tasks.contains(&TaskType::CodeGeneration));
}

#[tokio::test]
async fn test_language_specialization() {
    let manager = create_test_manager().await.unwrap();
    
    // Register multilingual specialized models
    let languages = vec![
        ("japanese", vec!["ja"], 0.94),
        ("european", vec!["en", "de", "fr", "es", "it"], 0.91),
        ("chinese", vec!["zh-CN", "zh-TW"], 0.93),
    ];
    
    for (name, langs, accuracy) in languages {
        let model = ModelSpecialization {
            model_id: format!("llama-{}", name),
            base_model: "llama2-13b".to_string(),
            domain: DomainType::General,
            tasks: vec![TaskType::Translation, TaskType::Conversation],
            accuracy_score: accuracy,
            speed_multiplier: 1.0,
            specialized_tokens: 20_000,
            training_hours: 300,
        };
        
        let id = manager.register_specialization(model).await.unwrap().id;
        
        // Set language support
        manager.set_language_support(
            &id,
            LanguageSupport {
                primary_languages: langs.iter().map(|l| l.to_string()).collect(),
                fluency_scores: langs.iter().map(|l| (l.to_string(), accuracy)).collect(),
                specialized_tokenizer: true,
            }
        ).await.unwrap();
    }
    
    // Find best model for Japanese
    let japanese_model = manager.find_best_for_language("ja").await.unwrap();
    assert!(japanese_model.model_id.contains("japanese"));
}

#[tokio::test]
async fn test_industry_vertical_models() {
    let manager = create_test_manager().await.unwrap();
    
    // Register industry-specific models
    let verticals = vec![
        (IndustryVertical::Healthcare, "medical-llama", vec!["HIPAA", "FDA"]),
        (IndustryVertical::Finance, "finllama", vec!["SOX", "PCI-DSS"]),
        (IndustryVertical::Legal, "lawllama", vec!["Bar", "GDPR"]),
    ];
    
    for (vertical, model_name, compliance) in verticals {
        let model = SpecializedModel {
            name: model_name.to_string(),
            vertical,
            compliance_certifications: compliance.iter().map(|c| c.to_string()).collect(),
            specialized_knowledge: vec![
                "regulations".to_string(),
                "terminology".to_string(),
                "best_practices".to_string(),
            ],
            accuracy_benchmarks: HashMap::from([
                ("general", 0.85),
                ("domain_specific", 0.95),
            ]),
        };
        
        manager.register_vertical_model(model).await.unwrap();
    }
    
    // Query models by compliance requirement
    let hipaa_models = manager.find_by_compliance("HIPAA").await.unwrap();
    assert_eq!(hipaa_models.len(), 1);
    assert_eq!(hipaa_models[0].name, "medical-llama");
}

#[tokio::test]
async fn test_performance_vs_accuracy_tradeoffs() {
    let manager = create_test_manager().await.unwrap();
    
    // Register models with different performance profiles
    let profiles = vec![
        ("fast-llama", 2.0, 0.80, "speed_optimized"),
        ("balanced-llama", 1.0, 0.88, "balanced"),
        ("accurate-llama", 0.5, 0.95, "accuracy_optimized"),
    ];
    
    for (name, speed, accuracy, profile_type) in profiles {
        let model = ModelSpecialization {
            model_id: name.to_string(),
            base_model: "llama2-7b".to_string(),
            domain: DomainType::General,
            tasks: vec![TaskType::Conversation],
            accuracy_score: accuracy,
            speed_multiplier: speed,
            specialized_tokens: 0,
            training_hours: 100,
        };
        
        let id = manager.register_specialization(model).await.unwrap().id;
        
        manager.set_performance_profile(
            &id,
            PerformanceProfile {
                profile_type: profile_type.to_string(),
                tokens_per_second: (100.0 * speed) as u32,
                first_token_latency_ms: (50.0 / speed) as u32,
                memory_usage_mb: 4000,
                batch_size_limit: 8,
            }
        ).await.unwrap();
    }
    
    // Find model based on requirements
    let fast_model = manager.find_optimal_model(
        AccuracyRequirement::Minimum(0.75),
        Some(PerformanceProfile::speed_priority()),
    ).await.unwrap();
    
    assert_eq!(fast_model.model_id, "fast-llama");
    
    let accurate_model = manager.find_optimal_model(
        AccuracyRequirement::Maximum,
        None,
    ).await.unwrap();
    
    assert_eq!(accurate_model.model_id, "accurate-llama");
}

#[tokio::test]
async fn test_dynamic_query_routing() {
    let manager = create_test_manager().await.unwrap();
    
    // Register various specialized models
    let models = vec![
        ("medical-expert", DomainType::Medical),
        ("legal-expert", DomainType::Legal),
        ("code-expert", DomainType::Technical),
        ("general-purpose", DomainType::General),
    ];
    
    for (name, domain) in models {
        manager.register_specialization(ModelSpecialization {
            model_id: name.to_string(),
            domain,
            ..Default::default()
        }).await.unwrap();
    }
    
    // Create query analyzer
    let analyzer = QueryAnalyzer::new();
    
    // Test medical query routing
    let medical_query = "What are the symptoms of diabetes mellitus type 2?";
    let analysis = analyzer.analyze_query(medical_query).await.unwrap();
    assert_eq!(analysis.detected_domain, DomainType::Medical);
    
    let routed_model = manager.route_query(&analysis).await.unwrap();
    assert_eq!(routed_model.model_id, "medical-expert");
    
    // Test code query routing
    let code_query = "Write a Python function to sort a binary tree";
    let analysis = analyzer.analyze_query(code_query).await.unwrap();
    assert_eq!(analysis.detected_domain, DomainType::Technical);
    
    let routed_model = manager.route_query(&analysis).await.unwrap();
    assert_eq!(routed_model.model_id, "code-expert");
}

#[tokio::test]
async fn test_specialized_tokenizers() {
    let manager = create_test_manager().await.unwrap();
    
    // Register model with specialized tokenizer
    let scientific_model = ModelSpecialization {
        model_id: "sci-llama".to_string(),
        base_model: "llama2-13b".to_string(),
        domain: DomainType::Scientific,
        tasks: vec![TaskType::Research, TaskType::Analysis],
        accuracy_score: 0.93,
        speed_multiplier: 0.9,
        specialized_tokens: 75_000, // Scientific notation, formulas
        training_hours: 800,
    };
    
    let id = manager.register_specialization(scientific_model).await.unwrap().id;
    
    // Configure specialized tokenizer
    let tokenizer_config = TokenizerConfig {
        vocab_size: 100_000,
        special_tokens: vec![
            "[FORMULA]", "[EQUATION]", "[CHEMICAL]", "[UNIT]"
        ],
        custom_rules: vec![
            "preserve_subscripts".to_string(),
            "handle_superscripts".to_string(),
            "parse_chemical_formulas".to_string(),
        ],
        merge_base_vocab: true,
    };
    
    manager.configure_tokenizer(&id, tokenizer_config).await.unwrap();
    
    // Test tokenization
    let test_text = "H₂O has a molar mass of 18.015 g/mol";
    let tokens = manager.tokenize_with_specialized(&id, test_text).await.unwrap();
    
    // Should preserve scientific notation
    assert!(tokens.preserved_entities.contains(&"H₂O".to_string()));
    assert!(tokens.preserved_entities.contains(&"g/mol".to_string()));
}

#[tokio::test]
async fn test_custom_inference_pipelines() {
    let manager = create_test_manager().await.unwrap();
    
    // Create a medical diagnosis pipeline
    let pipeline = InferencePipeline {
        name: "medical_diagnosis_pipeline".to_string(),
        stages: vec![
            "symptom_extraction".to_string(),
            "medical_reasoning".to_string(),
            "differential_diagnosis".to_string(),
            "treatment_suggestion".to_string(),
        ],
        preprocessing: vec![
            "normalize_medical_terms".to_string(),
            "extract_vitals".to_string(),
        ],
        postprocessing: vec![
            "add_disclaimers".to_string(),
            "format_medical_report".to_string(),
        ],
        model_requirements: HashMap::from([
            ("min_accuracy", "0.95"),
            ("required_certifications", "medical"),
        ]),
    };
    
    let pipeline_id = manager.create_pipeline(pipeline).await.unwrap();
    
    // Run inference through pipeline
    let input = "Patient presents with fever, cough, and shortness of breath";
    let result = manager.run_pipeline_inference(
        &pipeline_id,
        input,
    ).await.unwrap();
    
    assert!(result.stages_completed.len() == 4);
    assert!(result.output.contains("differential diagnosis"));
    assert!(result.output.contains("medical professional"));
}

#[tokio::test]
async fn test_multi_model_ensembles() {
    let manager = create_test_manager().await.unwrap();
    
    // Register multiple models for ensemble
    let model_ids = vec![];
    for i in 0..3 {
        let model = ModelSpecialization {
            model_id: format!("ensemble-member-{}", i),
            base_model: "llama2-7b".to_string(),
            domain: DomainType::General,
            tasks: vec![TaskType::Conversation],
            accuracy_score: 0.85 + (i as f64 * 0.02),
            speed_multiplier: 1.0,
            specialized_tokens: 0,
            training_hours: 200,
        };
        
        let reg = manager.register_specialization(model).await.unwrap();
        model_ids.push(reg.id);
    }
    
    // Create ensemble
    let ensemble = manager.create_ensemble(
        "consensus-ensemble",
        model_ids,
        EnsembleStrategy::WeightedVoting {
            weights: vec![0.3, 0.35, 0.35],
            threshold: 0.7,
        },
    ).await.unwrap();
    
    // Test ensemble inference
    let query = "What is the capital of France?";
    let ensemble_result = manager.run_ensemble_inference(
        &ensemble.id,
        query,
    ).await.unwrap();
    
    assert_eq!(ensemble_result.consensus_answer, "Paris");
    assert!(ensemble_result.confidence > 0.9);
    assert_eq!(ensemble_result.individual_responses.len(), 3);
}

#[tokio::test]
async fn test_specialization_benchmarking() {
    let manager = create_test_manager().await.unwrap();
    
    // Register model for benchmarking
    let model = ModelSpecialization {
        model_id: "benchmark-model".to_string(),
        base_model: "llama2-13b".to_string(),
        domain: DomainType::General,
        tasks: vec![TaskType::Conversation, TaskType::Reasoning],
        accuracy_score: 0.0, // Will be determined by benchmark
        speed_multiplier: 1.0,
        specialized_tokens: 0,
        training_hours: 0,
    };
    
    let id = manager.register_specialization(model).await.unwrap().id;
    
    // Run comprehensive benchmark
    let benchmark = manager.run_benchmark(
        &id,
        vec![
            "MMLU",
            "HellaSwag", 
            "TruthfulQA",
            "Custom-Domain",
        ],
    ).await.unwrap();
    
    assert!(benchmark.overall_score > 0.0);
    assert_eq!(benchmark.test_results.len(), 4);
    assert!(benchmark.tokens_per_second > 0);
    assert!(benchmark.memory_usage_mb > 0);
    
    // Update model with benchmark results
    manager.update_from_benchmark(&id, &benchmark).await.unwrap();
    
    // Verify accuracy was updated
    let updated = manager.get_specialization(&id).await.unwrap();
    assert_eq!(updated.accuracy_score, benchmark.overall_score);
}

#[tokio::test]
async fn test_automatic_specialization_detection() {
    let manager = create_test_manager().await.unwrap();
    
    // Register models with auto-detection
    let model = ModelSpecialization {
        model_id: "auto-detect-model".to_string(),
        base_model: "llama2-7b".to_string(),
        domain: DomainType::Unknown, // Will be detected
        tasks: vec![], // Will be detected
        accuracy_score: 0.0,
        speed_multiplier: 1.0,
        specialized_tokens: 0,
        training_hours: 0,
    };
    
    let id = manager.register_specialization(model).await.unwrap().id;
    
    // Run auto-detection
    let detection_result = manager.detect_specialization(
        &id,
        1000, // Number of test queries
    ).await.unwrap();
    
    assert_ne!(detection_result.detected_domain, DomainType::Unknown);
    assert!(!detection_result.detected_tasks.is_empty());
    assert!(detection_result.confidence > 0.8);
    
    // Apply detected specialization
    manager.apply_detected_specialization(
        &id,
        detection_result,
    ).await.unwrap();
}

#[tokio::test]
async fn test_cost_optimization() {
    let manager = create_test_manager().await.unwrap();
    
    // Register models with different cost profiles
    let models = vec![
        ("cheap-model", 0.001, 0.80, 100), // $0.001/1k tokens, 80% accuracy
        ("balanced-model", 0.005, 0.88, 80),
        ("premium-model", 0.02, 0.95, 50),
    ];
    
    for (name, cost_per_1k, accuracy, throughput) in models {
        let model = ModelSpecialization {
            model_id: name.to_string(),
            base_model: "llama2".to_string(),
            domain: DomainType::General,
            tasks: vec![TaskType::Conversation],
            accuracy_score: accuracy,
            speed_multiplier: throughput as f64 / 100.0,
            specialized_tokens: 0,
            training_hours: 0,
        };
        
        let id = manager.register_specialization(model).await.unwrap().id;
        
        manager.set_cost_profile(
            &id,
            CostProfile {
                cost_per_1k_tokens: cost_per_1k,
                cost_per_hour: cost_per_1k * throughput as f64 * 60.0,
                minimum_batch_size: 1,
                volume_discounts: vec![
                    (1000, 0.9),  // 10% discount at 1M tokens
                    (10000, 0.8), // 20% discount at 10M tokens
                ],
            },
        ).await.unwrap();
    }
    
    // Find most cost-effective model for requirements
    let optimizer = CostOptimizer::new();
    let requirements = CostRequirements {
        max_cost_per_1k_tokens: 0.01,
        min_accuracy: 0.85,
        expected_volume: 5000, // 5M tokens
        latency_requirement: None,
    };
    
    let optimal = optimizer.find_optimal_model(
        &manager,
        requirements,
    ).await.unwrap();
    
    assert_eq!(optimal.model_id, "balanced-model");
    assert!(optimal.effective_cost_per_1k <= 0.005 * 0.9); // With volume discount
}

#[tokio::test]
async fn test_specialization_marketplace() {
    let manager = create_test_manager().await.unwrap();
    
    // Create marketplace instance
    let marketplace = SpecializationMarketplace::new();
    
    // List specialized model for sale/rent
    let listing = MarketplaceListing {
        model_id: "premium-medical-model".to_string(),
        seller_id: "medical-ai-corp".to_string(),
        specialization: ModelSpecialization {
            model_id: "premium-medical-model".to_string(),
            base_model: "llama2-70b".to_string(),
            domain: DomainType::Medical,
            tasks: vec![
                TaskType::Diagnosis,
                TaskType::Research,
                TaskType::ClinicalNotes,
            ],
            accuracy_score: 0.97,
            speed_multiplier: 0.6,
            specialized_tokens: 100_000,
            training_hours: 5000,
        },
        pricing: PricingModel {
            license_type: "usage_based".to_string(),
            base_price: 0.05, // per 1k tokens
            volume_tiers: vec![
                (1000, 0.04),
                (10000, 0.03),
            ],
            exclusive_license_price: Some(50000.0),
        },
        ratings: MarketplaceRatings {
            average_score: 4.8,
            total_reviews: 127,
            verified_benchmarks: true,
        },
    };
    
    let listing_id = marketplace.create_listing(listing).await.unwrap();
    
    // Search marketplace
    let search_results = marketplace.search(
        SearchCriteria {
            domain: Some(DomainType::Medical),
            min_accuracy: Some(0.95),
            max_price_per_1k: Some(0.10),
            required_tasks: vec![TaskType::Diagnosis],
        }
    ).await.unwrap();
    
    assert!(!search_results.is_empty());
    assert!(search_results[0].model_id == "premium-medical-model");
    
    // Purchase/rent model
    let transaction = marketplace.initiate_transaction(
        &listing_id,
        "buyer-123",
        TransactionType::UsageBased {
            estimated_tokens: 1_000_000,
            duration_days: 30,
        },
    ).await.unwrap();
    
    assert!(!transaction.id.is_empty());
    assert_eq!(transaction.status, "pending_payment");
}