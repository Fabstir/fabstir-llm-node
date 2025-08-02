pub mod uptime;
pub mod response_time;
pub mod accuracy;
pub mod ratings;

// Re-export main types and traits for convenience
pub use uptime::{
    UptimeTracker, UptimeMetrics, DowntimeEvent, UptimeAlert,
    ServiceStatus, UptimeConfig, UptimeError, HistoricalUptime
};

pub use response_time::{
    ResponseTimeTracker, ResponseMetrics, LatencyBucket, 
    PerformanceAlert, ResponseTimeConfig, MetricsAggregation,
    ModelPerformance, ResponseTimeError
};

pub use accuracy::{
    AccuracyVerifier, VerificationConfig, VerificationResult,
    AccuracyMetrics, QualityScore, VerificationMethod,
    SamplingStrategy, AccuracyAlert, AccuracyError,
    ValidationRule, ConsistencyCheck
};

pub use ratings::{
    RatingsManager, UserRating, RatingsSummary, RatingsConfig,
    RatingCategory, ReputationImpact, RatingsError, RatingTrend,
    FeedbackType, RatingAggregation, RatingAlert
};