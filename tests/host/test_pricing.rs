use fabstir_llm_node::host::{
    PricingManager, PricingModel, PricingTier, Currency, 
    DynamicPricingConfig, PricingError, PriceUpdate
};
use std::collections::HashMap;
use chrono::{DateTime, Utc, Duration};

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_pricing() -> PricingModel {
        PricingModel {
            model_id: "llama-3.2-1b-instruct".to_string(),
            base_price_per_token: 0.000001, // $0.001 per 1K tokens
            base_price_per_minute: 0.01,
            currency: Currency::USDC,
            tiers: vec![
                PricingTier {
                    min_tokens: 0,
                    max_tokens: 100_000,
                    multiplier: 1.0,
                },
                PricingTier {
                    min_tokens: 100_001,
                    max_tokens: 1_000_000,
                    multiplier: 0.9, // 10% discount
                },
                PricingTier {
                    min_tokens: 1_000_001,
                    max_tokens: u64::MAX,
                    multiplier: 0.8, // 20% discount
                },
            ],
            dynamic_pricing: Some(DynamicPricingConfig {
                enabled: true,
                min_multiplier: 0.5,
                max_multiplier: 2.0,
                demand_threshold: 0.8, // 80% capacity
            }),
        }
    }

    #[tokio::test]
    async fn test_set_model_pricing() {
        let mut manager = PricingManager::new();
        let pricing = create_test_pricing();
        
        let result = manager.set_pricing(pricing).await;
        assert!(result.is_ok());
        
        let price = manager.get_pricing("llama-3.2-1b-instruct").await;
        assert!(price.is_some());
        assert_eq!(price.unwrap().base_price_per_token, 0.000001);
    }

    #[tokio::test]
    async fn test_calculate_token_price() {
        let mut manager = PricingManager::new();
        let pricing = create_test_pricing();
        
        manager.set_pricing(pricing).await.unwrap();
        
        // Test different token counts
        let test_cases = vec![
            (1_000, 0.001),      // 1K tokens = $0.001
            (50_000, 0.05),      // 50K tokens = $0.05
            (150_000, 0.135),    // 150K tokens with tier discount
            (2_000_000, 1.6),    // 2M tokens with max tier discount
        ];
        
        for (tokens, expected_price) in test_cases {
            let price = manager.calculate_token_price("llama-3.2-1b-instruct", tokens).await;
            assert!(price.is_ok());
            assert!((price.unwrap() - expected_price).abs() < 0.0001);
        }
    }

    #[tokio::test]
    async fn test_calculate_time_based_price() {
        let mut manager = PricingManager::new();
        let pricing = create_test_pricing();
        
        manager.set_pricing(pricing).await.unwrap();
        
        let duration_minutes = 5.5;
        let price = manager.calculate_time_price("llama-3.2-1b-instruct", duration_minutes).await;
        
        assert!(price.is_ok());
        assert_eq!(price.unwrap(), 0.055); // $0.01 * 5.5
    }

    #[tokio::test]
    async fn test_dynamic_pricing() {
        let mut manager = PricingManager::new();
        let pricing = create_test_pricing();
        
        manager.set_pricing(pricing).await.unwrap();
        
        // Set high demand
        manager.update_demand_level(0.9).await; // 90% capacity
        
        let price = manager.calculate_token_price_with_demand("llama-3.2-1b-instruct", 1_000).await;
        assert!(price.is_ok());
        assert!(price.unwrap() > 0.001); // Should be higher than base price
        
        // Set low demand
        manager.update_demand_level(0.2).await; // 20% capacity
        
        let price = manager.calculate_token_price_with_demand("llama-3.2-1b-instruct", 1_000).await;
        assert!(price.is_ok());
        assert!(price.unwrap() < 0.001); // Should be lower than base price
    }

    #[tokio::test]
    async fn test_multi_currency_support() {
        let mut manager = PricingManager::new();
        
        // USDC pricing
        let mut usdc_pricing = create_test_pricing();
        usdc_pricing.currency = Currency::USDC;
        manager.set_pricing(usdc_pricing).await.unwrap();
        
        // FAB token pricing
        let mut fab_pricing = create_test_pricing();
        fab_pricing.model_id = "mistral-7b".to_string();
        fab_pricing.currency = Currency::FAB;
        fab_pricing.base_price_per_token = 0.1; // FAB tokens
        manager.set_pricing(fab_pricing).await.unwrap();
        
        let usdc_price = manager.get_pricing_by_currency("llama-3.2-1b-instruct", Currency::USDC).await;
        assert!(usdc_price.is_some());
        
        let fab_price = manager.get_pricing_by_currency("mistral-7b", Currency::FAB).await;
        assert!(fab_price.is_some());
    }

    #[tokio::test]
    async fn test_bulk_pricing_update() {
        let mut manager = PricingManager::new();
        
        let models = vec!["llama-3.2-1b", "mistral-7b", "llama-70b"];
        for model in &models {
            let mut pricing = create_test_pricing();
            pricing.model_id = model.to_string();
            manager.set_pricing(pricing).await.unwrap();
        }
        
        // Update all prices by 10%
        let update = PriceUpdate {
            multiplier: 1.1,
            affected_models: models.iter().map(|s| s.to_string()).collect(),
            reason: "Market adjustment".to_string(),
            effective_from: Utc::now(),
        };
        
        let result = manager.apply_bulk_update(update).await;
        assert!(result.is_ok());
        
        // Verify prices increased
        for model in models {
            let price = manager.calculate_token_price(model, 1_000).await.unwrap();
            assert!((price - 0.0011).abs() < 0.0001); // 10% increase
        }
    }

    #[tokio::test]
    async fn test_pricing_history() {
        let mut manager = PricingManager::new();
        let pricing = create_test_pricing();
        
        manager.set_pricing(pricing).await.unwrap();
        
        // Update price multiple times
        for i in 1..=3 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            manager.update_base_price("llama-3.2-1b-instruct", 0.000001 * (i as f64)).await.unwrap();
        }
        
        let history = manager.get_pricing_history("llama-3.2-1b-instruct", 10).await;
        assert!(history.len() >= 3);
        
        // Verify chronological order
        for i in 1..history.len() {
            assert!(history[i].timestamp > history[i-1].timestamp);
        }
    }

    #[tokio::test]
    async fn test_promotional_pricing() {
        let mut manager = PricingManager::new();
        let pricing = create_test_pricing();
        
        manager.set_pricing(pricing).await.unwrap();
        
        // Apply promotional discount
        let promo = manager.create_promotion(
            "llama-3.2-1b-instruct",
            0.5, // 50% off
            Utc::now(),
            Utc::now() + Duration::hours(24),
        ).await;
        
        assert!(promo.is_ok());
        
        let price = manager.calculate_token_price("llama-3.2-1b-instruct", 1_000).await;
        assert!(price.is_ok());
        assert_eq!(price.unwrap(), 0.0005); // Half price
    }

    #[tokio::test]
    async fn test_minimum_price_enforcement() {
        let mut manager = PricingManager::new();
        let mut pricing = create_test_pricing();
        pricing.base_price_per_token = 0.0000001; // Very low price
        
        manager.set_minimum_price_per_token(0.0000005).await;
        
        let result = manager.set_pricing(pricing).await;
        assert!(matches!(result, Err(PricingError::BelowMinimum(_))));
    }
}