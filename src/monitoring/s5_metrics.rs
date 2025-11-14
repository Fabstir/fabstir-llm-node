// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! S5 vector loading metrics
//!
//! Provides Prometheus-compatible metrics for tracking S5 download performance,
//! vector loading statistics, and index cache efficiency.

use crate::monitoring::{Counter, Histogram, MetricsCollector};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

/// S5Metrics structure for tracking S5 vector loading performance
///
/// Tracks metrics for vector loading:
/// - `s5_download_duration_seconds` - Histogram of S5 download times
/// - `s5_download_errors_total` - Counter of download failures
/// - `s5_vectors_loaded_total` - Counter of vectors successfully loaded
/// - `vector_index_build_duration_seconds` - Histogram of index build times
/// - `vector_index_cache_hits_total` - Counter of cache hits
/// - `vector_index_cache_misses_total` - Counter of cache misses
/// - `vector_loading_success_total` - Counter of successful async loading operations (Phase 6)
/// - `vector_loading_failure_total` - Counter of failed async loading operations (Phase 6)
/// - `vector_loading_timeout_total` - Counter of timeout events (Phase 6)
/// - `vector_loading_duration_seconds` - Histogram of total loading times (Phase 6)
#[derive(Clone)]
pub struct S5Metrics {
    /// Histogram for S5 download durations
    pub download_duration: Arc<Histogram>,
    /// Counter for S5 download errors
    pub download_errors: Arc<Counter>,
    /// Counter for total vectors loaded
    pub vectors_loaded: Arc<Counter>,
    /// Histogram for vector index build durations
    pub index_build_duration: Arc<Histogram>,
    /// Counter for index cache hits
    pub cache_hits: Arc<Counter>,
    /// Counter for index cache misses
    pub cache_misses: Arc<Counter>,
    /// Counter for successful async loading operations
    pub loading_success: Arc<Counter>,
    /// Counter for failed async loading operations
    pub loading_failure: Arc<Counter>,
    /// Counter for timeout events
    pub loading_timeout: Arc<Counter>,
    /// Histogram for total async loading durations
    pub loading_duration: Arc<Histogram>,
}

impl S5Metrics {
    /// Create new S5Metrics instance registered with collector
    ///
    /// # Arguments
    /// * `collector` - MetricsCollector to register metrics with
    ///
    /// # Returns
    /// Result containing S5Metrics instance with all metrics registered
    ///
    /// # Example
    /// ```no_run
    /// use fabstir_llm_node::monitoring::{MetricsCollector, MetricsConfig, TimeWindow};
    /// use fabstir_llm_node::monitoring::s5_metrics::S5Metrics;
    ///
    /// # async fn example() -> anyhow::Result<()> {
    /// let config = MetricsConfig {
    ///     enable_metrics: true,
    ///     collection_interval_ms: 100,
    ///     retention_period_hours: 24,
    ///     aggregation_windows: vec![TimeWindow::OneMinute, TimeWindow::OneHour],
    ///     export_format: "prometheus".to_string(),
    ///     export_endpoint: "http://localhost:9090".to_string(),
    ///     buffer_size: 10000,
    /// };
    /// let collector = MetricsCollector::new(config).await?;
    /// let metrics = S5Metrics::new(&collector).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(collector: &MetricsCollector) -> Result<Self> {
        let download_duration = collector
            .register_histogram(
                "s5_download_duration_seconds",
                "S5 download duration in seconds",
                vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0],
            )
            .await?;

        let download_errors = collector
            .register_counter("s5_download_errors_total", "Total S5 download errors")
            .await?;

        let vectors_loaded = collector
            .register_counter("s5_vectors_loaded_total", "Total vectors loaded from S5")
            .await?;

        let index_build_duration = collector
            .register_histogram(
                "vector_index_build_duration_seconds",
                "Vector index build duration in seconds",
                vec![0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
            )
            .await?;

        let cache_hits = collector
            .register_counter(
                "vector_index_cache_hits_total",
                "Total vector index cache hits",
            )
            .await?;

        let cache_misses = collector
            .register_counter(
                "vector_index_cache_misses_total",
                "Total vector index cache misses",
            )
            .await?;

        let loading_success = collector
            .register_counter(
                "vector_loading_success_total",
                "Total successful async vector loading operations",
            )
            .await?;

        let loading_failure = collector
            .register_counter(
                "vector_loading_failure_total",
                "Total failed async vector loading operations",
            )
            .await?;

        let loading_timeout = collector
            .register_counter(
                "vector_loading_timeout_total",
                "Total async vector loading timeout events",
            )
            .await?;

        let loading_duration = collector
            .register_histogram(
                "vector_loading_duration_seconds",
                "Total async vector loading duration in seconds",
                vec![1.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0],
            )
            .await?;

        Ok(Self {
            download_duration,
            download_errors,
            vectors_loaded,
            index_build_duration,
            cache_hits,
            cache_misses,
            loading_success,
            loading_failure,
            loading_timeout,
            loading_duration,
        })
    }

    /// Record S5 download duration
    ///
    /// # Arguments
    /// * `duration` - Time taken to download from S5
    pub async fn record_download(&self, duration: Duration) {
        self.download_duration.observe(duration.as_secs_f64()).await;
    }

    /// Record S5 download error
    pub async fn record_download_error(&self) {
        self.download_errors.inc().await;
    }

    /// Record vectors loaded from S5
    ///
    /// # Arguments
    /// * `count` - Number of vectors loaded
    pub async fn record_vectors_loaded(&self, count: u64) {
        self.vectors_loaded.inc_by(count as f64).await;
    }

    /// Record vector index build duration
    ///
    /// # Arguments
    /// * `duration` - Time taken to build the index
    pub async fn record_index_build(&self, duration: Duration) {
        self.index_build_duration
            .observe(duration.as_secs_f64())
            .await;
    }

    /// Record cache hit
    pub async fn record_cache_hit(&self) {
        self.cache_hits.inc().await;
    }

    /// Record cache miss
    pub async fn record_cache_miss(&self) {
        self.cache_misses.inc().await;
    }

    /// Get cache hit rate
    ///
    /// # Returns
    /// Cache hit rate as a percentage (0.0 to 1.0), or 0.0 if no cache accesses
    pub async fn get_cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.get().await;
        let misses = self.cache_misses.get().await;
        let total = hits + misses;

        if total == 0.0 {
            0.0
        } else {
            hits / total
        }
    }

    /// Record successful async vector loading operation
    ///
    /// # Arguments
    /// * `duration` - Total time taken for loading and indexing
    pub async fn record_loading_success(&self, duration: Duration) {
        self.loading_success.inc().await;
        self.loading_duration.observe(duration.as_secs_f64()).await;
    }

    /// Record failed async vector loading operation
    pub async fn record_loading_failure(&self) {
        self.loading_failure.inc().await;
    }

    /// Record async vector loading timeout event
    pub async fn record_loading_timeout(&self) {
        self.loading_timeout.inc().await;
    }

    /// Get loading success rate
    ///
    /// # Returns
    /// Success rate as a percentage (0.0 to 1.0), or 0.0 if no loading attempts
    pub async fn get_loading_success_rate(&self) -> f64 {
        let success = self.loading_success.get().await;
        let failure = self.loading_failure.get().await;
        let timeout = self.loading_timeout.get().await;
        let total = success + failure + timeout;

        if total == 0.0 {
            0.0
        } else {
            success / total
        }
    }
}
