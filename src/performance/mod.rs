// src/performance/mod.rs - Performance optimization modules

pub mod gpu_management;
pub mod batching;
pub mod caching;
pub mod load_balancing;

// Re-export GPU management types
pub use gpu_management::{
    GpuManager, GpuConfig, GpuDevice, GpuStatus, GpuAllocation,
    GpuMetrics, GpuError, AllocationStrategy, MemoryPool, MemoryPoolHandle,
    GpuScheduler, TaskPriority, GpuCapabilities
};

// Re-export batching types
pub use batching::{
    BatchProcessor, BatchConfig, BatchRequest, BatchResult,
    BatchStatus, BatchError, BatchingStrategy, QueueConfig,
    BatchMetrics, PaddingStrategy, BatchPriority, Batch
};

// Re-export caching types
pub use caching::{
    InferenceCache, CacheConfig, CacheKey, CacheEntry, CacheStatus,
    CacheStats, EvictionPolicy, CacheError, SemanticCache,
    EmbeddingGenerator, SimilarityThreshold, CacheWarming
};

// Re-export load balancing types
pub use load_balancing::{
    LoadBalancer, LoadBalancerConfig, WorkerNode, LoadStrategy,
    NodeStatus, WorkerMetrics, LoadDistribution, HealthCheck,
    RequestRouter, NodeCapabilities, LoadBalancerError, SessionAffinity
};

// Common performance types
#[derive(Debug, Clone, PartialEq)]
pub enum PerformanceMode {
    HighThroughput,
    LowLatency,
    Balanced,
    PowerEfficient,
}

#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub requests_per_second: f64,
    pub average_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub gpu_utilization_percent: f64,
    pub memory_utilization_percent: f64,
    pub cache_hit_rate: f64,
    pub batch_efficiency: f64,
}

#[derive(Debug, Clone)]
pub struct PerformanceConfig {
    pub mode: PerformanceMode,
    pub gpu_config: GpuConfig,
    pub batch_config: BatchConfig,
    pub cache_config: CacheConfig,
    pub load_balancer_config: LoadBalancerConfig,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            mode: PerformanceMode::Balanced,
            gpu_config: GpuConfig::default(),
            batch_config: BatchConfig::default(),
            cache_config: CacheConfig::default(),
            load_balancer_config: LoadBalancerConfig::default(),
        }
    }
}

// Performance optimizer that coordinates all components
pub struct PerformanceOptimizer {
    gpu_manager: GpuManager,
    batch_processor: BatchProcessor,
    inference_cache: InferenceCache,
    load_balancer: LoadBalancer,
    config: PerformanceConfig,
}

impl PerformanceOptimizer {
    pub async fn new(config: PerformanceConfig) -> anyhow::Result<Self> {
        let gpu_manager = GpuManager::new(config.gpu_config.clone()).await?;
        let batch_processor = BatchProcessor::new(config.batch_config.clone()).await?;
        let inference_cache = InferenceCache::new(config.cache_config.clone()).await?;
        let load_balancer = LoadBalancer::new(
            config.load_balancer_config.clone(),
            vec![],
        ).await?;

        Ok(Self {
            gpu_manager,
            batch_processor,
            inference_cache,
            load_balancer,
            config,
        })
    }

    pub async fn get_metrics(&self) -> PerformanceMetrics {
        // Aggregate metrics from all components
        let gpu_metrics = self.gpu_manager.get_aggregate_metrics().await;
        let batch_metrics = self.batch_processor.get_metrics().await;
        let cache_stats = self.inference_cache.get_stats().await;
        let load_metrics = self.load_balancer.get_metrics().await;

        PerformanceMetrics {
            requests_per_second: load_metrics.requests_per_second,
            average_latency_ms: load_metrics.average_latency_ms,
            p95_latency_ms: load_metrics.p95_latency_ms,
            p99_latency_ms: load_metrics.p99_latency_ms,
            gpu_utilization_percent: gpu_metrics.average_utilization,
            memory_utilization_percent: gpu_metrics.average_memory_usage,
            cache_hit_rate: cache_stats.hit_rate,
            batch_efficiency: batch_metrics.batch_efficiency,
        }
    }
}