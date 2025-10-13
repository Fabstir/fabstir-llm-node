//! EZKL Prometheus Metrics
//!
//! Provides Prometheus metrics for EZKL proof generation and caching.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// EZKL metrics for Prometheus
#[derive(Debug, Clone)]
pub struct EzklMetrics {
    /// Number of proof generation attempts
    proof_generation_total: Arc<AtomicU64>,
    /// Number of successful proof generations
    proof_generation_success: Arc<AtomicU64>,
    /// Number of failed proof generations
    proof_generation_errors: Arc<AtomicU64>,
    /// Number of proof cache hits
    cache_hits: Arc<AtomicU64>,
    /// Number of proof cache misses
    cache_misses: Arc<AtomicU64>,
    /// Number of cache evictions
    cache_evictions: Arc<AtomicU64>,
    /// Number of key cache hits
    key_cache_hits: Arc<AtomicU64>,
    /// Number of key cache misses
    key_cache_misses: Arc<AtomicU64>,
    /// Total proof generation time in milliseconds
    proof_generation_duration_ms: Arc<AtomicU64>,
    /// Number of proofs generated (for averaging)
    proof_count: Arc<AtomicU64>,
    /// Number of proof verification attempts
    verification_total: Arc<AtomicU64>,
    /// Number of successful verifications
    verification_success: Arc<AtomicU64>,
    /// Number of failed verifications
    verification_failures: Arc<AtomicU64>,
    /// Total verification time in milliseconds
    verification_duration_ms: Arc<AtomicU64>,
    /// Number of verifications (for averaging)
    verification_count: Arc<AtomicU64>,
}

impl EzklMetrics {
    /// Create new metrics instance
    pub fn new() -> Self {
        Self {
            proof_generation_total: Arc::new(AtomicU64::new(0)),
            proof_generation_success: Arc::new(AtomicU64::new(0)),
            proof_generation_errors: Arc::new(AtomicU64::new(0)),
            cache_hits: Arc::new(AtomicU64::new(0)),
            cache_misses: Arc::new(AtomicU64::new(0)),
            cache_evictions: Arc::new(AtomicU64::new(0)),
            key_cache_hits: Arc::new(AtomicU64::new(0)),
            key_cache_misses: Arc::new(AtomicU64::new(0)),
            proof_generation_duration_ms: Arc::new(AtomicU64::new(0)),
            proof_count: Arc::new(AtomicU64::new(0)),
            verification_total: Arc::new(AtomicU64::new(0)),
            verification_success: Arc::new(AtomicU64::new(0)),
            verification_failures: Arc::new(AtomicU64::new(0)),
            verification_duration_ms: Arc::new(AtomicU64::new(0)),
            verification_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Record proof generation attempt
    pub fn record_proof_generation_attempt(&self) {
        self.proof_generation_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record successful proof generation
    pub fn record_proof_generation_success(&self, duration_ms: u64) {
        self.proof_generation_success
            .fetch_add(1, Ordering::Relaxed);
        self.proof_generation_duration_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
        self.proof_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record proof generation error
    pub fn record_proof_generation_error(&self) {
        self.proof_generation_errors
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Record proof cache hit
    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record proof cache miss
    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record cache eviction
    pub fn record_cache_eviction(&self) {
        self.cache_evictions.fetch_add(1, Ordering::Relaxed);
    }

    /// Record key cache hit
    pub fn record_key_cache_hit(&self) {
        self.key_cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record key cache miss
    pub fn record_key_cache_miss(&self) {
        self.key_cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record proof verification attempt
    pub fn record_verification_attempt(&self) {
        self.verification_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record successful proof verification
    pub fn record_verification_success(&self, duration_ms: u64) {
        self.verification_success.fetch_add(1, Ordering::Relaxed);
        self.verification_duration_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
        self.verification_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record proof verification failure
    pub fn record_verification_failure(&self) {
        self.verification_failures.fetch_add(1, Ordering::Relaxed);
    }

    // Getters for metrics values

    /// Get total proof generation attempts
    pub fn proof_generation_total(&self) -> u64 {
        self.proof_generation_total.load(Ordering::Relaxed)
    }

    /// Get successful proof generations
    pub fn proof_generation_success(&self) -> u64 {
        self.proof_generation_success.load(Ordering::Relaxed)
    }

    /// Get proof generation errors
    pub fn proof_generation_errors(&self) -> u64 {
        self.proof_generation_errors.load(Ordering::Relaxed)
    }

    /// Get cache hits
    pub fn cache_hits(&self) -> u64 {
        self.cache_hits.load(Ordering::Relaxed)
    }

    /// Get cache misses
    pub fn cache_misses(&self) -> u64 {
        self.cache_misses.load(Ordering::Relaxed)
    }

    /// Get cache evictions
    pub fn cache_evictions(&self) -> u64 {
        self.cache_evictions.load(Ordering::Relaxed)
    }

    /// Get key cache hits
    pub fn key_cache_hits(&self) -> u64 {
        self.key_cache_hits.load(Ordering::Relaxed)
    }

    /// Get key cache misses
    pub fn key_cache_misses(&self) -> u64 {
        self.key_cache_misses.load(Ordering::Relaxed)
    }

    /// Get total verification attempts
    pub fn verification_total(&self) -> u64 {
        self.verification_total.load(Ordering::Relaxed)
    }

    /// Get successful verifications
    pub fn verification_success(&self) -> u64 {
        self.verification_success.load(Ordering::Relaxed)
    }

    /// Get verification failures
    pub fn verification_failures(&self) -> u64 {
        self.verification_failures.load(Ordering::Relaxed)
    }

    /// Get average verification time in milliseconds
    pub fn avg_verification_ms(&self) -> f64 {
        let total_ms = self.verification_duration_ms.load(Ordering::Relaxed);
        let count = self.verification_count.load(Ordering::Relaxed);

        if count == 0 {
            0.0
        } else {
            total_ms as f64 / count as f64
        }
    }

    /// Get verification success rate (0.0 to 1.0)
    pub fn verification_success_rate(&self) -> f64 {
        let total = self.verification_total();
        let success = self.verification_success();

        if total == 0 {
            0.0
        } else {
            success as f64 / total as f64
        }
    }

    /// Get average proof generation time in milliseconds
    pub fn avg_proof_generation_ms(&self) -> f64 {
        let total_ms = self.proof_generation_duration_ms.load(Ordering::Relaxed);
        let count = self.proof_count.load(Ordering::Relaxed);

        if count == 0 {
            0.0
        } else {
            total_ms as f64 / count as f64
        }
    }

    /// Get proof cache hit rate (0.0 to 1.0)
    pub fn cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits();
        let misses = self.cache_misses();
        let total = hits + misses;

        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Get key cache hit rate (0.0 to 1.0)
    pub fn key_cache_hit_rate(&self) -> f64 {
        let hits = self.key_cache_hits();
        let misses = self.key_cache_misses();
        let total = hits + misses;

        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// Get proof generation success rate (0.0 to 1.0)
    pub fn proof_generation_success_rate(&self) -> f64 {
        let total = self.proof_generation_total();
        let success = self.proof_generation_success();

        if total == 0 {
            0.0
        } else {
            success as f64 / total as f64
        }
    }

    /// Reset all metrics
    pub fn reset(&self) {
        self.proof_generation_total.store(0, Ordering::Relaxed);
        self.proof_generation_success.store(0, Ordering::Relaxed);
        self.proof_generation_errors.store(0, Ordering::Relaxed);
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
        self.cache_evictions.store(0, Ordering::Relaxed);
        self.key_cache_hits.store(0, Ordering::Relaxed);
        self.key_cache_misses.store(0, Ordering::Relaxed);
        self.proof_generation_duration_ms
            .store(0, Ordering::Relaxed);
        self.proof_count.store(0, Ordering::Relaxed);
        self.verification_total.store(0, Ordering::Relaxed);
        self.verification_success.store(0, Ordering::Relaxed);
        self.verification_failures.store(0, Ordering::Relaxed);
        self.verification_duration_ms.store(0, Ordering::Relaxed);
        self.verification_count.store(0, Ordering::Relaxed);
    }

    /// Export metrics in Prometheus text format
    pub fn export_prometheus(&self) -> String {
        format!(
            r#"# HELP ezkl_proof_generation_total Total number of proof generation attempts
# TYPE ezkl_proof_generation_total counter
ezkl_proof_generation_total {}

# HELP ezkl_proof_generation_success Number of successful proof generations
# TYPE ezkl_proof_generation_success counter
ezkl_proof_generation_success {}

# HELP ezkl_proof_generation_errors Number of proof generation errors
# TYPE ezkl_proof_generation_errors counter
ezkl_proof_generation_errors {}

# HELP ezkl_cache_hits Number of proof cache hits
# TYPE ezkl_cache_hits counter
ezkl_cache_hits {}

# HELP ezkl_cache_misses Number of proof cache misses
# TYPE ezkl_cache_misses counter
ezkl_cache_misses {}

# HELP ezkl_cache_evictions Number of cache evictions
# TYPE ezkl_cache_evictions counter
ezkl_cache_evictions {}

# HELP ezkl_key_cache_hits Number of key cache hits
# TYPE ezkl_key_cache_hits counter
ezkl_key_cache_hits {}

# HELP ezkl_key_cache_misses Number of key cache misses
# TYPE ezkl_key_cache_misses counter
ezkl_key_cache_misses {}

# HELP ezkl_cache_hit_rate Proof cache hit rate
# TYPE ezkl_cache_hit_rate gauge
ezkl_cache_hit_rate {:.4}

# HELP ezkl_key_cache_hit_rate Key cache hit rate
# TYPE ezkl_key_cache_hit_rate gauge
ezkl_key_cache_hit_rate {:.4}

# HELP ezkl_avg_proof_generation_ms Average proof generation time in milliseconds
# TYPE ezkl_avg_proof_generation_ms gauge
ezkl_avg_proof_generation_ms {:.2}

# HELP ezkl_proof_generation_success_rate Proof generation success rate
# TYPE ezkl_proof_generation_success_rate gauge
ezkl_proof_generation_success_rate {:.4}

# HELP ezkl_verification_total Total number of proof verification attempts
# TYPE ezkl_verification_total counter
ezkl_verification_total {}

# HELP ezkl_verification_success Number of successful proof verifications
# TYPE ezkl_verification_success counter
ezkl_verification_success {}

# HELP ezkl_verification_failures Number of failed proof verifications
# TYPE ezkl_verification_failures counter
ezkl_verification_failures {}

# HELP ezkl_avg_verification_ms Average verification time in milliseconds
# TYPE ezkl_avg_verification_ms gauge
ezkl_avg_verification_ms {:.2}

# HELP ezkl_verification_success_rate Verification success rate
# TYPE ezkl_verification_success_rate gauge
ezkl_verification_success_rate {:.4}
"#,
            self.proof_generation_total(),
            self.proof_generation_success(),
            self.proof_generation_errors(),
            self.cache_hits(),
            self.cache_misses(),
            self.cache_evictions(),
            self.key_cache_hits(),
            self.key_cache_misses(),
            self.cache_hit_rate(),
            self.key_cache_hit_rate(),
            self.avg_proof_generation_ms(),
            self.proof_generation_success_rate(),
            self.verification_total(),
            self.verification_success(),
            self.verification_failures(),
            self.avg_verification_ms(),
            self.verification_success_rate(),
        )
    }
}

impl Default for EzklMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Get global metrics instance
///
/// Creates a single global metrics instance using lazy initialization.
/// This is thread-safe and only initializes once.
pub fn global_metrics() -> &'static EzklMetrics {
    use std::sync::OnceLock;
    static GLOBAL_METRICS: OnceLock<EzklMetrics> = OnceLock::new();
    GLOBAL_METRICS.get_or_init(EzklMetrics::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = EzklMetrics::new();
        assert_eq!(metrics.proof_generation_total(), 0);
        assert_eq!(metrics.cache_hits(), 0);
    }

    #[test]
    fn test_record_proof_generation() {
        let metrics = EzklMetrics::new();

        metrics.record_proof_generation_attempt();
        assert_eq!(metrics.proof_generation_total(), 1);

        metrics.record_proof_generation_success(100);
        assert_eq!(metrics.proof_generation_success(), 1);

        metrics.record_proof_generation_error();
        assert_eq!(metrics.proof_generation_errors(), 1);
    }

    #[test]
    fn test_cache_metrics() {
        let metrics = EzklMetrics::new();

        metrics.record_cache_hit();
        metrics.record_cache_hit();
        metrics.record_cache_miss();

        assert_eq!(metrics.cache_hits(), 2);
        assert_eq!(metrics.cache_misses(), 1);
        assert_eq!(metrics.cache_hit_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_key_cache_metrics() {
        let metrics = EzklMetrics::new();

        metrics.record_key_cache_hit();
        metrics.record_key_cache_miss();

        assert_eq!(metrics.key_cache_hits(), 1);
        assert_eq!(metrics.key_cache_misses(), 1);
        assert_eq!(metrics.key_cache_hit_rate(), 0.5);
    }

    #[test]
    fn test_average_proof_generation_time() {
        let metrics = EzklMetrics::new();

        metrics.record_proof_generation_success(100);
        metrics.record_proof_generation_success(200);

        assert_eq!(metrics.avg_proof_generation_ms(), 150.0);
    }

    #[test]
    fn test_success_rate() {
        let metrics = EzklMetrics::new();

        metrics.record_proof_generation_attempt();
        metrics.record_proof_generation_success(100);
        metrics.record_proof_generation_attempt();
        metrics.record_proof_generation_error();

        assert_eq!(metrics.proof_generation_success_rate(), 0.5);
    }

    #[test]
    fn test_metrics_reset() {
        let metrics = EzklMetrics::new();

        metrics.record_cache_hit();
        metrics.record_cache_miss();
        assert_eq!(metrics.cache_hits(), 1);

        metrics.reset();
        assert_eq!(metrics.cache_hits(), 0);
        assert_eq!(metrics.cache_misses(), 0);
    }

    #[test]
    fn test_prometheus_export() {
        let metrics = EzklMetrics::new();

        metrics.record_proof_generation_attempt();
        metrics.record_proof_generation_success(100);
        metrics.record_cache_hit();

        let export = metrics.export_prometheus();

        assert!(export.contains("ezkl_proof_generation_total 1"));
        assert!(export.contains("ezkl_proof_generation_success 1"));
        assert!(export.contains("ezkl_cache_hits 1"));
    }

    #[test]
    fn test_zero_division() {
        let metrics = EzklMetrics::new();

        // Should not panic with zero values
        assert_eq!(metrics.cache_hit_rate(), 0.0);
        assert_eq!(metrics.key_cache_hit_rate(), 0.0);
        assert_eq!(metrics.avg_proof_generation_ms(), 0.0);
        assert_eq!(metrics.proof_generation_success_rate(), 0.0);
    }

    #[test]
    fn test_global_metrics() {
        let metrics = global_metrics();
        metrics.record_cache_hit();

        // Should access same instance
        let metrics2 = global_metrics();
        assert!(metrics2.cache_hits() >= 1);
    }

    #[test]
    fn test_verification_metrics() {
        let metrics = EzklMetrics::new();

        metrics.record_verification_attempt();
        assert_eq!(metrics.verification_total(), 1);

        metrics.record_verification_success(5);
        assert_eq!(metrics.verification_success(), 1);

        metrics.record_verification_failure();
        assert_eq!(metrics.verification_failures(), 1);
    }

    #[test]
    fn test_average_verification_time() {
        let metrics = EzklMetrics::new();

        metrics.record_verification_success(10);
        metrics.record_verification_success(20);

        assert_eq!(metrics.avg_verification_ms(), 15.0);
    }

    #[test]
    fn test_verification_success_rate() {
        let metrics = EzklMetrics::new();

        metrics.record_verification_attempt();
        metrics.record_verification_success(5);
        metrics.record_verification_attempt();
        metrics.record_verification_failure();

        assert_eq!(metrics.verification_success_rate(), 0.5);
    }

    #[test]
    fn test_verification_zero_division() {
        let metrics = EzklMetrics::new();

        // Should not panic with zero values
        assert_eq!(metrics.verification_total(), 0);
        assert_eq!(metrics.avg_verification_ms(), 0.0);
        assert_eq!(metrics.verification_success_rate(), 0.0);
    }

    #[test]
    fn test_verification_prometheus_export() {
        let metrics = EzklMetrics::new();

        metrics.record_verification_attempt();
        metrics.record_verification_success(5);

        let export = metrics.export_prometheus();

        assert!(export.contains("ezkl_verification_total 1"));
        assert!(export.contains("ezkl_verification_success 1"));
        assert!(export.contains("ezkl_verification_failures 0"));
    }
}
