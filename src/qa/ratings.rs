// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum RatingCategory {
    ResponseQuality,
    Speed,
    Reliability,
    ValueForMoney,
    Overall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FeedbackType {
    Positive,
    Neutral,
    Negative,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingsConfig {
    pub min_rating: u32,
    pub max_rating: u32,
    pub categories: Vec<RatingCategory>,
    pub reputation_impact_factor: f64,
    pub minimum_ratings_for_impact: u32,
    pub allow_anonymous: bool,
    pub require_verification: bool,
    pub decay_period_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRating {
    pub job_id: String,
    pub user_id: String,
    pub model_id: String,
    pub overall_rating: u32,
    pub category_ratings: HashMap<RatingCategory, u32>,
    pub feedback: Option<String>,
    pub verified: bool,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingsSummary {
    pub model_id: String,
    pub total_ratings: u32,
    pub average_overall: f64,
    pub category_averages: HashMap<RatingCategory, f64>,
    pub distribution: HashMap<u32, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationImpact {
    pub host_id: String,
    pub rating_count: u32,
    pub average_rating: f64,
    pub reputation_change: f64,
    pub new_reputation: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingTrend {
    pub model_id: String,
    pub is_improving: bool,
    pub trend_percentage: f64,
    pub period_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingAggregation {
    pub total_ratings: u32,
    pub average_by_category: HashMap<RatingCategory, f64>,
    pub recent_trend: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingAlert {
    pub timestamp: DateTime<Utc>,
    pub alert_type: String,
    pub model_id: String,
    pub average_rating: f64,
    pub threshold: f64,
    pub recent_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackAnalysis {
    pub total_feedback: u32,
    pub positive_count: u32,
    pub neutral_count: u32,
    pub negative_count: u32,
    pub common_themes: Vec<String>,
    pub sentiment_score: f64,
}

#[derive(Debug, Error)]
pub enum RatingsError {
    #[error("Invalid rating: {0}")]
    InvalidRating(String),
    #[error("Anonymous ratings not allowed")]
    AnonymousNotAllowed,
    #[error("Unverified rating not allowed")]
    UnverifiedNotAllowed,
    #[error("Model not found: {0}")]
    ModelNotFound(String),
    #[error("Rating not found: {0}")]
    RatingNotFound(String),
    #[error("Host not found: {0}")]
    HostNotFound(String),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[derive(Debug)]
pub struct RatingsManager {
    config: RatingsConfig,
    ratings: Arc<Mutex<HashMap<String, UserRating>>>,
    model_ratings: Arc<Mutex<HashMap<String, Vec<String>>>>,
    host_ratings: Arc<Mutex<HashMap<String, Vec<String>>>>,
    host_reputations: Arc<Mutex<HashMap<String, f64>>>,
    alert_sender: broadcast::Sender<RatingAlert>,
    moderation_queue: Arc<Mutex<HashMap<String, String>>>, // rating_id -> status
}

impl RatingsManager {
    pub fn new(config: RatingsConfig) -> Self {
        let (alert_sender, _) = broadcast::channel(100);

        Self {
            config,
            ratings: Arc::new(Mutex::new(HashMap::new())),
            model_ratings: Arc::new(Mutex::new(HashMap::new())),
            host_ratings: Arc::new(Mutex::new(HashMap::new())),
            host_reputations: Arc::new(Mutex::new(HashMap::new())),
            alert_sender,
            moderation_queue: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn submit_rating(&self, rating: UserRating) -> Result<String, RatingsError> {
        // Validate rating
        self.validate_rating(&rating)?;

        let rating_id = Uuid::new_v4().to_string();

        // Check for spam/inappropriate content
        let needs_moderation = self.needs_moderation(&rating).await;

        if needs_moderation {
            let mut moderation = self.moderation_queue.lock().await;
            moderation.insert(rating_id.clone(), "pending_moderation".to_string());
        }

        // Store rating
        {
            let mut ratings = self.ratings.lock().await;
            ratings.insert(rating_id.clone(), rating.clone());
        }

        // Update model index
        {
            let mut model_ratings = self.model_ratings.lock().await;
            model_ratings
                .entry(rating.model_id.clone())
                .or_insert_with(Vec::new)
                .push(rating_id.clone());
        }

        // Check for alerts
        self.check_rating_alerts(&rating.model_id).await;

        Ok(rating_id)
    }

    pub async fn get_rating(&self, rating_id: &str) -> Option<UserRating> {
        let ratings = self.ratings.lock().await;
        ratings.get(rating_id).cloned()
    }

    pub async fn get_ratings_summary(&self, model_id: &str) -> RatingsSummary {
        let ratings = self.ratings.lock().await;
        let model_ratings = self.model_ratings.lock().await;

        let rating_ids = model_ratings.get(model_id).cloned().unwrap_or_default();
        let model_rating_data: Vec<_> =
            rating_ids.iter().filter_map(|id| ratings.get(id)).collect();

        if model_rating_data.is_empty() {
            return RatingsSummary {
                model_id: model_id.to_string(),
                total_ratings: 0,
                average_overall: 0.0,
                category_averages: HashMap::new(),
                distribution: HashMap::new(),
            };
        }

        let total_ratings = model_rating_data.len() as u32;

        // Calculate overall average
        let sum_overall: u32 = model_rating_data.iter().map(|r| r.overall_rating).sum();
        let average_overall = sum_overall as f64 / total_ratings as f64;

        // Calculate category averages
        let mut category_averages = HashMap::new();
        for category in &self.config.categories {
            let category_sum: u32 = model_rating_data
                .iter()
                .filter_map(|r| r.category_ratings.get(category))
                .sum();
            let category_count = model_rating_data
                .iter()
                .filter(|r| r.category_ratings.contains_key(category))
                .count();

            if category_count > 0 {
                category_averages.insert(
                    category.clone(),
                    category_sum as f64 / category_count as f64,
                );
            }
        }

        // Calculate distribution
        let mut distribution = HashMap::new();
        for rating in self.config.min_rating..=self.config.max_rating {
            distribution.insert(rating, 0);
        }

        for rating_data in &model_rating_data {
            *distribution.entry(rating_data.overall_rating).or_insert(0) += 1;
        }

        RatingsSummary {
            model_id: model_id.to_string(),
            total_ratings,
            average_overall,
            category_averages,
            distribution,
        }
    }

    pub async fn get_rating_distribution(&self, model_id: &str) -> HashMap<u32, u32> {
        let summary = self.get_ratings_summary(model_id).await;
        summary.distribution
    }

    pub async fn get_host_reputation(&self, host_id: &str) -> f64 {
        let reputations = self.host_reputations.lock().await;
        reputations.get(host_id).copied().unwrap_or(100.0) // Default reputation
    }

    pub async fn submit_rating_for_host(
        &self,
        host_id: &str,
        rating: UserRating,
    ) -> Result<String, RatingsError> {
        let rating_id = self.submit_rating(rating).await?;

        // Update host index
        {
            let mut host_ratings = self.host_ratings.lock().await;
            host_ratings
                .entry(host_id.to_string())
                .or_insert_with(Vec::new)
                .push(rating_id.clone());
        }

        Ok(rating_id)
    }

    pub async fn calculate_reputation_impact(
        &self,
        host_id: &str,
    ) -> Result<ReputationImpact, RatingsError> {
        let host_ratings = self.host_ratings.lock().await;
        let ratings = self.ratings.lock().await;

        let rating_ids = host_ratings.get(host_id).cloned().unwrap_or_default();
        let host_rating_data: Vec<_> = rating_ids.iter().filter_map(|id| ratings.get(id)).collect();

        if host_rating_data.is_empty() {
            return Err(RatingsError::HostNotFound(host_id.to_string()));
        }

        let rating_count = host_rating_data.len() as u32;
        let sum_ratings: u32 = host_rating_data.iter().map(|r| r.overall_rating).sum();
        let average_rating = sum_ratings as f64 / rating_count as f64;

        let current_reputation = self.get_host_reputation(host_id).await;

        // Calculate reputation change based on ratings
        let reputation_change = if rating_count >= self.config.minimum_ratings_for_impact {
            (average_rating - 3.0) * self.config.reputation_impact_factor * rating_count as f64
        } else {
            0.0
        };

        let new_reputation = (current_reputation + reputation_change).max(0.0).min(200.0);

        // Update stored reputation
        {
            let mut reputations = self.host_reputations.lock().await;
            reputations.insert(host_id.to_string(), new_reputation);
        }

        Ok(ReputationImpact {
            host_id: host_id.to_string(),
            rating_count,
            average_rating,
            reputation_change,
            new_reputation,
        })
    }

    pub async fn analyze_feedback(&self, model_id: &str) -> FeedbackAnalysis {
        let ratings = self.ratings.lock().await;
        let model_ratings = self.model_ratings.lock().await;

        let rating_ids = model_ratings.get(model_id).cloned().unwrap_or_default();
        let feedback_data: Vec<_> = rating_ids
            .iter()
            .filter_map(|id| ratings.get(id))
            .filter_map(|r| r.feedback.as_ref())
            .collect();

        if feedback_data.is_empty() {
            return FeedbackAnalysis {
                total_feedback: 0,
                positive_count: 0,
                neutral_count: 0,
                negative_count: 0,
                common_themes: Vec::new(),
                sentiment_score: 0.0,
            };
        }

        let total_feedback = feedback_data.len() as u32;
        let mut positive_count = 0;
        let mut neutral_count = 0;
        let mut negative_count = 0;
        let mut theme_words = HashMap::new();

        for feedback in &feedback_data {
            let sentiment = self.analyze_sentiment(feedback);
            match sentiment {
                FeedbackType::Positive => positive_count += 1,
                FeedbackType::Neutral => neutral_count += 1,
                FeedbackType::Negative => negative_count += 1,
            }

            // Extract themes (simple word frequency)
            for word in feedback.to_lowercase().split_whitespace() {
                if word.len() > 3 {
                    *theme_words.entry(word.to_string()).or_insert(0) += 1;
                }
            }
        }

        // Get most common themes
        let mut common_themes: Vec<_> = theme_words
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .collect();
        common_themes.sort_by(|a, b| b.1.cmp(&a.1));
        let common_themes: Vec<_> = common_themes
            .into_iter()
            .take(5)
            .map(|(word, _)| word)
            .collect();

        let sentiment_score =
            (positive_count as f64 - negative_count as f64) / total_feedback as f64;

        FeedbackAnalysis {
            total_feedback,
            positive_count,
            neutral_count,
            negative_count,
            common_themes,
            sentiment_score,
        }
    }

    pub async fn get_recent_ratings(&self, model_id: &str, window: Duration) -> Vec<UserRating> {
        let ratings = self.ratings.lock().await;
        let model_ratings = self.model_ratings.lock().await;

        let cutoff = Utc::now() - window;
        let rating_ids = model_ratings.get(model_id).cloned().unwrap_or_default();

        rating_ids
            .iter()
            .filter_map(|id| ratings.get(id))
            .filter(|r| r.timestamp >= cutoff)
            .cloned()
            .collect()
    }

    pub async fn submit_rating_with_timestamp(
        &self,
        rating: UserRating,
    ) -> Result<String, RatingsError> {
        // Same as submit_rating but preserves the provided timestamp
        self.submit_rating(rating).await
    }

    pub async fn get_rating_trend(&self, model_id: &str, days: u32) -> RatingTrend {
        let recent_window = Duration::days(days as i64 / 2);
        let older_window = Duration::days(days as i64);

        let recent_ratings = self.get_recent_ratings(model_id, recent_window).await;
        let all_ratings = self.get_recent_ratings(model_id, older_window).await;

        let recent_avg = if !recent_ratings.is_empty() {
            recent_ratings.iter().map(|r| r.overall_rating).sum::<u32>() as f64
                / recent_ratings.len() as f64
        } else {
            0.0
        };

        let older_ratings: Vec<_> = all_ratings
            .iter()
            .filter(|r| {
                !recent_ratings
                    .iter()
                    .any(|recent| recent.job_id == r.job_id)
            })
            .collect();

        let older_avg = if !older_ratings.is_empty() {
            older_ratings.iter().map(|r| r.overall_rating).sum::<u32>() as f64
                / older_ratings.len() as f64
        } else {
            recent_avg
        };

        let trend_percentage = if older_avg > 0.0 {
            ((recent_avg - older_avg) / older_avg) * 100.0
        } else {
            0.0
        };

        RatingTrend {
            model_id: model_id.to_string(),
            is_improving: trend_percentage > 0.0,
            trend_percentage,
            period_days: days,
        }
    }

    pub async fn subscribe_to_alerts(&self) -> broadcast::Receiver<RatingAlert> {
        self.alert_sender.subscribe()
    }

    pub async fn compare_model_ratings(&self, model_ids: Vec<String>) -> Vec<(String, f64)> {
        let mut model_averages = Vec::new();

        for model_id in model_ids {
            let summary = self.get_ratings_summary(&model_id).await;
            model_averages.push((model_id, summary.average_overall));
        }

        // Sort by average rating (descending)
        model_averages.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        model_averages
    }

    pub async fn export_ratings(
        &self,
        format: &str,
        model_filter: Option<&str>,
    ) -> Result<String, RatingsError> {
        let ratings = self.ratings.lock().await;

        let filtered_ratings: Vec<_> = if let Some(model) = model_filter {
            ratings.values().filter(|r| r.model_id == model).collect()
        } else {
            ratings.values().collect()
        };

        match format {
            "csv" => {
                let mut csv = String::from("job_id,user_id,model_id,rating,timestamp\n");
                for rating in filtered_ratings {
                    csv.push_str(&format!(
                        "{},{},{},{},{}\n",
                        rating.job_id,
                        rating.user_id,
                        rating.model_id,
                        rating.overall_rating,
                        rating.timestamp.format("%Y-%m-%d %H:%M:%S")
                    ));
                }
                Ok(csv)
            }
            "json" => {
                let export_data = serde_json::json!({
                    "ratings": filtered_ratings,
                    "total_count": filtered_ratings.len(),
                    "export_timestamp": Utc::now()
                });
                Ok(serde_json::to_string_pretty(&export_data)?)
            }
            _ => Err(RatingsError::InvalidRating(
                "Unsupported format".to_string(),
            )),
        }
    }

    pub async fn get_rating_status(&self, rating_id: &str) -> String {
        let moderation = self.moderation_queue.lock().await;
        moderation
            .get(rating_id)
            .cloned()
            .unwrap_or("approved".to_string())
    }

    pub async fn moderate_rating(
        &self,
        rating_id: &str,
        approved: bool,
        reason: &str,
    ) -> Result<(), RatingsError> {
        let mut moderation = self.moderation_queue.lock().await;
        let status = if approved { "approved" } else { "rejected" };
        moderation.insert(rating_id.to_string(), format!("{}: {}", status, reason));
        Ok(())
    }

    fn validate_rating(&self, rating: &UserRating) -> Result<(), RatingsError> {
        // Check rating range
        if rating.overall_rating < self.config.min_rating
            || rating.overall_rating > self.config.max_rating
        {
            return Err(RatingsError::InvalidRating(format!(
                "Rating {} out of range {}-{}",
                rating.overall_rating, self.config.min_rating, self.config.max_rating
            )));
        }

        // Check category ratings
        for ((category, &category_rating), _) in rating
            .category_ratings
            .iter()
            .zip(self.config.categories.iter())
        {
            if category_rating < self.config.min_rating || category_rating > self.config.max_rating
            {
                return Err(RatingsError::InvalidRating(format!(
                    "Category rating for {:?} out of range",
                    category
                )));
            }
        }

        // Check anonymous policy
        if !self.config.allow_anonymous && rating.user_id == "anonymous" {
            return Err(RatingsError::AnonymousNotAllowed);
        }

        // Check verification requirement
        if self.config.require_verification && !rating.verified {
            return Err(RatingsError::UnverifiedNotAllowed);
        }

        Ok(())
    }

    async fn needs_moderation(&self, rating: &UserRating) -> bool {
        if let Some(feedback) = &rating.feedback {
            // Simple spam detection
            let spam_keywords = ["spam", "scam", "buy", "click", "free money"];
            let feedback_lower = feedback.to_lowercase();

            for keyword in &spam_keywords {
                if feedback_lower.contains(keyword) {
                    return true;
                }
            }
        }
        false
    }

    async fn check_rating_alerts(&self, model_id: &str) {
        let recent_ratings = self.get_recent_ratings(model_id, Duration::hours(24)).await;

        if recent_ratings.len() >= 5 {
            let avg_rating: f64 = recent_ratings.iter().map(|r| r.overall_rating).sum::<u32>()
                as f64
                / recent_ratings.len() as f64;

            if avg_rating < 3.0 {
                let alert = RatingAlert {
                    timestamp: Utc::now(),
                    alert_type: "low_ratings".to_string(),
                    model_id: model_id.to_string(),
                    average_rating: avg_rating,
                    threshold: 3.0,
                    recent_count: recent_ratings.len() as u32,
                };

                let _ = self.alert_sender.send(alert);
            }
        }
    }

    fn analyze_sentiment(&self, feedback: &str) -> FeedbackType {
        let positive_words = ["good", "great", "excellent", "amazing", "fast", "quality"];
        let negative_words = ["bad", "slow", "poor", "terrible", "expensive", "inaccurate"];

        let feedback_lower = feedback.to_lowercase();
        let mut positive_score = 0;
        let mut negative_score = 0;

        for word in &positive_words {
            if feedback_lower.contains(word) {
                positive_score += 1;
            }
        }

        for word in &negative_words {
            if feedback_lower.contains(word) {
                negative_score += 1;
            }
        }

        if positive_score > negative_score {
            FeedbackType::Positive
        } else if negative_score > positive_score {
            FeedbackType::Negative
        } else {
            FeedbackType::Neutral
        }
    }
}
