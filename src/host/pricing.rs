use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PricingModel {
    pub model_id: String,
    pub base_price_per_token: f64,
    pub base_price_per_minute: f64,
    pub currency: Currency,
    pub tiers: Vec<PricingTier>,
    pub dynamic_pricing: Option<DynamicPricingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PricingTier {
    pub min_tokens: u64,
    pub max_tokens: u64,
    pub multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Currency {
    USDC,
    FAB,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DynamicPricingConfig {
    pub enabled: bool,
    pub min_multiplier: f64,
    pub max_multiplier: f64,
    pub demand_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceUpdate {
    pub multiplier: f64,
    pub affected_models: Vec<String>,
    pub reason: String,
    pub effective_from: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceHistoryEntry {
    pub timestamp: DateTime<Utc>,
    pub price_per_token: f64,
    pub price_per_minute: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Promotion {
    pub model_id: String,
    pub discount_multiplier: f64,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum PricingError {
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Invalid pricing configuration: {0}")]
    InvalidConfiguration(String),
    #[error("Price below minimum: {0}")]
    BelowMinimum(f64),
    #[error("Currency not supported: {0:?}")]
    UnsupportedCurrency(Currency),
}

#[derive(Debug)]
pub struct PricingManager {
    models: HashMap<String, PricingModel>,
    price_history: HashMap<String, Vec<PriceHistoryEntry>>,
    current_demand: f64,
    minimum_price_per_token: f64,
    promotions: HashMap<String, Promotion>,
}

impl PricingManager {
    pub fn new() -> Self {
        Self {
            models: HashMap::new(),
            price_history: HashMap::new(),
            current_demand: 0.0,
            minimum_price_per_token: 0.0,
            promotions: HashMap::new(),
        }
    }

    pub async fn set_pricing(&mut self, pricing: PricingModel) -> Result<(), PricingError> {
        // Validate minimum price
        if pricing.base_price_per_token < self.minimum_price_per_token {
            return Err(PricingError::BelowMinimum(self.minimum_price_per_token));
        }

        // Validate tiers
        self.validate_tiers(&pricing.tiers)?;

        // Store pricing history
        let history_entry = PriceHistoryEntry {
            timestamp: Utc::now(),
            price_per_token: pricing.base_price_per_token,
            price_per_minute: pricing.base_price_per_minute,
            reason: "Price update".to_string(),
        };

        self.price_history
            .entry(pricing.model_id.clone())
            .or_insert_with(Vec::new)
            .push(history_entry);

        self.models.insert(pricing.model_id.clone(), pricing);
        Ok(())
    }

    pub async fn get_pricing(&self, model_id: &str) -> Option<PricingModel> {
        self.models.get(model_id).cloned()
    }

    pub async fn calculate_token_price(
        &self,
        model_id: &str,
        tokens: u64,
    ) -> Result<f64, PricingError> {
        let pricing = self
            .models
            .get(model_id)
            .ok_or_else(|| PricingError::ModelNotFound(model_id.to_string()))?;

        let base_price = pricing.base_price_per_token * tokens as f64;
        let tier_multiplier = self.get_tier_multiplier(&pricing.tiers, tokens);

        Ok(base_price * tier_multiplier)
    }

    pub async fn calculate_time_price(
        &self,
        model_id: &str,
        duration_minutes: f64,
    ) -> Result<f64, PricingError> {
        let pricing = self
            .models
            .get(model_id)
            .ok_or_else(|| PricingError::ModelNotFound(model_id.to_string()))?;

        Ok(pricing.base_price_per_minute * duration_minutes)
    }

    pub async fn calculate_token_price_with_demand(
        &self,
        model_id: &str,
        tokens: u64,
    ) -> Result<f64, PricingError> {
        let mut price = self.calculate_token_price(model_id, tokens).await?;

        let pricing = self.models.get(model_id).unwrap();

        if let Some(dynamic_config) = &pricing.dynamic_pricing {
            if dynamic_config.enabled {
                let demand_multiplier = self.calculate_demand_multiplier(dynamic_config);
                price *= demand_multiplier;
            }
        }

        // Apply promotions
        if let Some(promotion) = self.promotions.get(model_id) {
            let now = Utc::now();
            if now >= promotion.start_time && now <= promotion.end_time {
                price *= promotion.discount_multiplier;
            }
        }

        Ok(price)
    }

    pub async fn update_demand_level(&mut self, demand: f64) {
        self.current_demand = demand.clamp(0.0, 1.0);
    }

    pub async fn get_pricing_by_currency(
        &self,
        model_id: &str,
        currency: Currency,
    ) -> Option<PricingModel> {
        self.models
            .get(model_id)
            .filter(|pricing| pricing.currency == currency)
            .cloned()
    }

    pub async fn apply_bulk_update(&mut self, update: PriceUpdate) -> Result<(), PricingError> {
        for model_id in &update.affected_models {
            if let Some(pricing) = self.models.get_mut(model_id) {
                pricing.base_price_per_token *= update.multiplier;
                pricing.base_price_per_minute *= update.multiplier;

                // Add to history
                let history_entry = PriceHistoryEntry {
                    timestamp: update.effective_from,
                    price_per_token: pricing.base_price_per_token,
                    price_per_minute: pricing.base_price_per_minute,
                    reason: update.reason.clone(),
                };

                self.price_history
                    .entry(model_id.clone())
                    .or_insert_with(Vec::new)
                    .push(history_entry);
            }
        }
        Ok(())
    }

    pub async fn update_base_price(
        &mut self,
        model_id: &str,
        new_price: f64,
    ) -> Result<(), PricingError> {
        let pricing = self
            .models
            .get_mut(model_id)
            .ok_or_else(|| PricingError::ModelNotFound(model_id.to_string()))?;

        if new_price < self.minimum_price_per_token {
            return Err(PricingError::BelowMinimum(self.minimum_price_per_token));
        }

        pricing.base_price_per_token = new_price;

        // Add to history
        let history_entry = PriceHistoryEntry {
            timestamp: Utc::now(),
            price_per_token: new_price,
            price_per_minute: pricing.base_price_per_minute,
            reason: "Base price update".to_string(),
        };

        self.price_history
            .entry(model_id.to_string())
            .or_insert_with(Vec::new)
            .push(history_entry);

        Ok(())
    }

    pub async fn get_pricing_history(
        &self,
        model_id: &str,
        limit: usize,
    ) -> Vec<PriceHistoryEntry> {
        self.price_history
            .get(model_id)
            .map(|history| {
                let mut sorted = history.clone();
                sorted.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
                if sorted.len() > limit {
                    sorted.into_iter().rev().take(limit).rev().collect()
                } else {
                    sorted
                }
            })
            .unwrap_or_default()
    }

    pub async fn create_promotion(
        &mut self,
        model_id: &str,
        discount_multiplier: f64,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<(), PricingError> {
        if !self.models.contains_key(model_id) {
            return Err(PricingError::ModelNotFound(model_id.to_string()));
        }

        let promotion = Promotion {
            model_id: model_id.to_string(),
            discount_multiplier,
            start_time,
            end_time,
        };

        self.promotions.insert(model_id.to_string(), promotion);
        Ok(())
    }

    pub async fn set_minimum_price_per_token(&mut self, minimum: f64) {
        self.minimum_price_per_token = minimum;
    }

    fn get_tier_multiplier(&self, tiers: &[PricingTier], tokens: u64) -> f64 {
        for tier in tiers {
            if tokens >= tier.min_tokens && tokens <= tier.max_tokens {
                return tier.multiplier;
            }
        }
        1.0 // Default multiplier if no tier matches
    }

    fn calculate_demand_multiplier(&self, config: &DynamicPricingConfig) -> f64 {
        if self.current_demand > config.demand_threshold {
            // High demand - increase price
            let excess_demand = self.current_demand - config.demand_threshold;
            let max_excess = 1.0 - config.demand_threshold;
            let multiplier_increase = (excess_demand / max_excess) * (config.max_multiplier - 1.0);
            (1.0 + multiplier_increase).min(config.max_multiplier)
        } else {
            // Low demand - decrease price
            let demand_ratio = self.current_demand / config.demand_threshold;
            let multiplier_decrease = (1.0 - demand_ratio) * (1.0 - config.min_multiplier);
            (1.0 - multiplier_decrease).max(config.min_multiplier)
        }
    }

    fn validate_tiers(&self, tiers: &[PricingTier]) -> Result<(), PricingError> {
        for tier in tiers {
            if tier.min_tokens > tier.max_tokens {
                return Err(PricingError::InvalidConfiguration(
                    "Invalid tier range".to_string(),
                ));
            }
            if tier.multiplier <= 0.0 {
                return Err(PricingError::InvalidConfiguration(
                    "Tier multiplier must be positive".to_string(),
                ));
            }
        }
        Ok(())
    }
}

impl Default for PricingManager {
    fn default() -> Self {
        Self::new()
    }
}
