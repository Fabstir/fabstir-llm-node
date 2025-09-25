pub mod accuracy;
pub mod ratings;
pub mod response_time;
pub mod uptime;

// Re-export main types and traits for convenience
pub use uptime::{
    DowntimeEvent, HistoricalUptime, ServiceStatus, UptimeAlert, UptimeConfig, UptimeError,
    UptimeMetrics, UptimeTracker,
};

pub use response_time::{
    LatencyBucket, MetricsAggregation, ModelPerformance, PerformanceAlert, ResponseMetrics,
    ResponseTimeConfig, ResponseTimeError, ResponseTimeTracker,
};

pub use accuracy::{
    AccuracyAlert, AccuracyError, AccuracyMetrics, AccuracyVerifier, ConsistencyCheck,
    QualityScore, SamplingStrategy, ValidationRule, VerificationConfig, VerificationMethod,
    VerificationResult,
};

pub use ratings::{
    FeedbackType, RatingAggregation, RatingAlert, RatingCategory, RatingTrend, RatingsConfig,
    RatingsError, RatingsManager, RatingsSummary, ReputationImpact, UserRating,
};
