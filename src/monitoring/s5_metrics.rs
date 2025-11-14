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
/// Tracks six key metrics:
/// - `s5_download_duration_seconds` - Histogram of S5 download times
/// - `s5_download_errors_total` - Counter of download failures
/// - `s5_vectors_loaded_total` - Counter of vectors successfully loaded
/// - `vector_index_build_duration_seconds` - Histogram of index build times
/// - `vector_index_cache_hits_total` - Counter of cache hits
/// - `vector_index_cache_misses_total` - Counter of cache misses
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

        Ok(Self {
            download_duration,
            download_errors,
            vectors_loaded,
            index_build_duration,
            cache_hits,
            cache_misses,
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
}
