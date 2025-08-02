use fabstir_llm_node::qa::{
    RatingsManager, UserRating, RatingsSummary, RatingsConfig,
    RatingCategory, ReputationImpact, RatingsError, RatingTrend,
    FeedbackType, RatingAggregation, RatingAlert
};
use chrono::{DateTime, Utc, Duration};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_config() -> RatingsConfig {
        RatingsConfig {
            min_rating: 1,
            max_rating: 5,
            categories: vec![
                RatingCategory::ResponseQuality,
                RatingCategory::Speed,
                RatingCategory::Reliability,
                RatingCategory::ValueForMoney,
                RatingCategory::Overall,
            ],
            reputation_impact_factor: 0.1,
            minimum_ratings_for_impact: 5,
            allow_anonymous: false,
            require_verification: true,
            decay_period_days: 90,
        }
    }

    fn create_test_rating() -> UserRating {
        UserRating {
            job_id: "job-123".to_string(),
            user_id: "user-456".to_string(),
            model_id: "llama-3.2-1b".to_string(),
            overall_rating: 4,
            category_ratings: HashMap::from([
                (RatingCategory::ResponseQuality, 5),
                (RatingCategory::Speed, 4),
                (RatingCategory::Reliability, 4),
                (RatingCategory::ValueForMoney, 3),
            ]),
            feedback: Some("Great response quality, good speed.".to_string()),
            verified: true,
            timestamp: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_submit_rating() {
        let config = create_test_config();
        let manager = RatingsManager::new(config);
        
        let rating = create_test_rating();
        
        let result = manager.submit_rating(rating.clone()).await;
        assert!(result.is_ok());
        
        let rating_id = result.unwrap();
        assert!(!rating_id.is_empty());
        
        // Verify rating was stored
        let stored = manager.get_rating(&rating_id).await;
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().job_id, "job-123");
    }

    #[tokio::test]
    async fn test_rating_validation() {
        let config = create_test_config();
        let manager = RatingsManager::new(config);
        
        // Test invalid rating (out of range)
        let mut invalid_rating = create_test_rating();
        invalid_rating.overall_rating = 6; // Max is 5
        
        let result = manager.submit_rating(invalid_rating).await;
        assert!(matches!(result, Err(RatingsError::InvalidRating(_))));
        
        // Test anonymous rating when not allowed
        let mut anon_rating = create_test_rating();
        anon_rating.user_id = "anonymous".to_string();
        
        let result = manager.submit_rating(anon_rating).await;
        assert!(matches!(result, Err(RatingsError::AnonymousNotAllowed)));
    }

    #[tokio::test]
    async fn test_calculate_average_ratings() {
        let config = create_test_config();
        let manager = RatingsManager::new(config);
        
        // Submit multiple ratings
        let ratings = vec![
            (4, 5, 4, 4, 3),  // overall, quality, speed, reliability, value
            (5, 5, 5, 5, 4),
            (3, 4, 3, 3, 3),
            (4, 4, 4, 4, 4),
            (5, 5, 4, 5, 5),
        ];
        
        for (i, (overall, quality, speed, reliability, value)) in ratings.iter().enumerate() {
            let mut rating = create_test_rating();
            rating.job_id = format!("job-{}", i);
            rating.overall_rating = *overall;
            rating.category_ratings = HashMap::from([
                (RatingCategory::ResponseQuality, *quality),
                (RatingCategory::Speed, *speed),
                (RatingCategory::Reliability, *reliability),
                (RatingCategory::ValueForMoney, *value),
            ]);
            manager.submit_rating(rating).await.unwrap();
        }
        
        let summary = manager.get_ratings_summary("llama-3.2-1b").await;
        
        assert_eq!(summary.total_ratings, 5);
        assert_eq!(summary.average_overall, 4.2);
        assert_eq!(summary.category_averages[&RatingCategory::ResponseQuality], 4.6);
        assert_eq!(summary.category_averages[&RatingCategory::Speed], 4.0);
    }

    #[tokio::test]
    async fn test_rating_distribution() {
        let config = create_test_config();
        let manager = RatingsManager::new(config);
        
        // Submit ratings with known distribution
        let rating_values = vec![5, 5, 5, 4, 4, 4, 4, 3, 3, 2];
        
        for (i, value) in rating_values.iter().enumerate() {
            let mut rating = create_test_rating();
            rating.job_id = format!("job-{}", i);
            rating.overall_rating = *value;
            manager.submit_rating(rating).await.unwrap();
        }
        
        let distribution = manager.get_rating_distribution("llama-3.2-1b").await;
        
        assert_eq!(distribution.get(&5), Some(&3)); // Three 5-star ratings
        assert_eq!(distribution.get(&4), Some(&4)); // Four 4-star ratings
        assert_eq!(distribution.get(&3), Some(&2)); // Two 3-star ratings
        assert_eq!(distribution.get(&2), Some(&1)); // One 2-star rating
        assert_eq!(distribution.get(&1), Some(&0)); // Zero 1-star ratings
    }

    #[tokio::test]
    async fn test_reputation_impact() {
        let config = create_test_config();
        let manager = RatingsManager::new(config);
        
        // Track initial reputation
        let initial_reputation = manager.get_host_reputation("host-123").await;
        
        // Submit multiple positive ratings
        for i in 0..10 {
            let mut rating = create_test_rating();
            rating.job_id = format!("job-{}", i);
            rating.overall_rating = 5;
            manager.submit_rating_for_host("host-123", rating).await.unwrap();
        }
        
        let impact = manager.calculate_reputation_impact("host-123").await;
        
        assert!(impact.is_ok());
        let impact_data = impact.unwrap();
        assert!(impact_data.reputation_change > 0.0);
        assert_eq!(impact_data.rating_count, 10);
        assert!(impact_data.new_reputation > initial_reputation);
    }

    #[tokio::test]
    async fn test_feedback_analysis() {
        let config = create_test_config();
        let manager = RatingsManager::new(config);
        
        // Submit ratings with various feedback
        let feedback_samples = vec![
            ("Excellent quality, very fast responses!", 5, FeedbackType::Positive),
            ("Good overall but a bit expensive", 4, FeedbackType::Neutral),
            ("Responses were slow and sometimes inaccurate", 2, FeedbackType::Negative),
            ("Amazing service, will use again!", 5, FeedbackType::Positive),
            ("Average performance, nothing special", 3, FeedbackType::Neutral),
        ];
        
        for (i, (feedback, rating_value, _expected_type)) in feedback_samples.iter().enumerate() {
            let mut rating = create_test_rating();
            rating.job_id = format!("job-{}", i);
            rating.overall_rating = *rating_value;
            rating.feedback = Some(feedback.to_string());
            manager.submit_rating(rating).await.unwrap();
        }
        
        let analysis = manager.analyze_feedback("llama-3.2-1b").await;
        
        assert_eq!(analysis.total_feedback, 5);
        assert_eq!(analysis.positive_count, 2);
        assert_eq!(analysis.neutral_count, 2);
        assert_eq!(analysis.negative_count, 1);
        assert!(!analysis.common_themes.is_empty());
    }

    #[tokio::test]
    async fn test_time_based_ratings() {
        let config = create_test_config();
        let manager = RatingsManager::new(config);
        
        // Submit ratings over different time periods
        let time_periods = vec![
            (Utc::now() - Duration::hours(1), 5),    // 1 hour ago
            (Utc::now() - Duration::days(1), 4),     // 1 day ago
            (Utc::now() - Duration::days(7), 4),     // 1 week ago
            (Utc::now() - Duration::days(30), 3),    // 1 month ago
            (Utc::now() - Duration::days(90), 3),    // 3 months ago
        ];
        
        for (i, (timestamp, rating_value)) in time_periods.iter().enumerate() {
            let mut rating = create_test_rating();
            rating.job_id = format!("job-{}", i);
            rating.overall_rating = *rating_value;
            rating.timestamp = *timestamp;
            manager.submit_rating_with_timestamp(rating).await.unwrap();
        }
        
        // Get recent ratings (last 24 hours)
        let recent = manager.get_recent_ratings("llama-3.2-1b", Duration::hours(24)).await;
        assert_eq!(recent.len(), 1);
        
        // Get ratings trend
        let trend = manager.get_rating_trend("llama-3.2-1b", 30).await;
        assert!(trend.is_improving); // Recent ratings are better
    }

    #[tokio::test]
    async fn test_rating_alerts() {
        let config = create_test_config();
        let manager = RatingsManager::new(config);
        
        // Subscribe to alerts
        let mut alert_receiver = manager.subscribe_to_alerts().await;
        
        // Submit multiple low ratings
        for i in 0..5 {
            let mut rating = create_test_rating();
            rating.job_id = format!("job-{}", i);
            rating.overall_rating = 2;
            rating.feedback = Some("Poor performance".to_string());
            manager.submit_rating(rating).await.unwrap();
        }
        
        // Should receive low rating alert
        let alert = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            alert_receiver.recv()
        ).await;
        
        assert!(alert.is_ok());
        let alert_data = alert.unwrap().unwrap();
        assert_eq!(alert_data.alert_type, "low_ratings");
        assert!(alert_data.average_rating < 3.0);
    }

    #[tokio::test]
    async fn test_comparative_ratings() {
        let config = create_test_config();
        let manager = RatingsManager::new(config);
        
        // Submit ratings for multiple models
        let models = vec![
            ("llama-3.2-1b", vec![4, 4, 5, 4, 3]),    // Average: 4.0
            ("mistral-7b", vec![5, 5, 4, 5, 4]),      // Average: 4.6
            ("llama-70b", vec![3, 3, 4, 3, 3]),       // Average: 3.2
        ];
        
        for (model, ratings) in models {
            for (i, rating_value) in ratings.iter().enumerate() {
                let mut rating = create_test_rating();
                rating.job_id = format!("{}-job-{}", model, i);
                rating.model_id = model.to_string();
                rating.overall_rating = *rating_value;
                manager.submit_rating(rating).await.unwrap();
            }
        }
        
        let comparison = manager.compare_model_ratings(vec![
            "llama-3.2-1b".to_string(),
            "mistral-7b".to_string(),
            "llama-70b".to_string(),
        ]).await;
        
        assert_eq!(comparison.len(), 3);
        assert_eq!(comparison[0].0, "mistral-7b"); // Highest rated
        assert_eq!(comparison[2].0, "llama-70b");  // Lowest rated
    }

    #[tokio::test]
    async fn test_rating_export() {
        let config = create_test_config();
        let manager = RatingsManager::new(config);
        
        // Submit some ratings
        for i in 0..5 {
            let rating = create_test_rating();
            manager.submit_rating(rating).await.unwrap();
        }
        
        // Export ratings data
        let csv_export = manager.export_ratings("csv", None).await;
        assert!(csv_export.is_ok());
        assert!(csv_export.unwrap().contains("job_id,user_id,rating"));
        
        let json_export = manager.export_ratings("json", Some("llama-3.2-1b")).await;
        assert!(json_export.is_ok());
        
        let parsed: serde_json::Value = serde_json::from_str(&json_export.unwrap()).unwrap();
        assert!(parsed["ratings"].as_array().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn test_rating_moderation() {
        let config = create_test_config();
        let manager = RatingsManager::new(config);
        
        // Submit rating with inappropriate feedback
        let mut rating = create_test_rating();
        rating.feedback = Some("This is spam! Buy crypto at scam.com".to_string());
        
        let result = manager.submit_rating(rating).await;
        
        // Should be flagged for moderation
        assert!(result.is_ok());
        let rating_id = result.unwrap();
        
        let status = manager.get_rating_status(&rating_id).await;
        assert_eq!(status, "pending_moderation");
        
        // Moderate the rating
        let moderation_result = manager.moderate_rating(
            &rating_id,
            false, // Reject
            "Spam content"
        ).await;
        
        assert!(moderation_result.is_ok());
    }
}