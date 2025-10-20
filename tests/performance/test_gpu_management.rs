// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use fabstir_llm_node::performance::{
    GpuManager, GpuConfig, GpuDevice, GpuStatus, GpuAllocation,
    GpuMetrics, GpuError, AllocationStrategy, MemoryPool,
    GpuScheduler, TaskPriority, GpuCapabilities
};
use std::sync::Arc;
use tokio;
use futures::StreamExt;

async fn create_test_gpu_manager() -> Result<GpuManager> {
    let config = GpuConfig {
        enable_gpu: true,
        gpu_device_ids: vec![0, 1], // Simulate 2 GPUs
        memory_fraction: 0.9,
        allow_gpu_growth: true,
        gpu_scheduling: AllocationStrategy::BestFit,
        max_concurrent_models: 4,
        fallback_to_cpu: true,
        nvidia_visible_devices: None,
    };
    
    GpuManager::new(config).await
}

#[tokio::test]
async fn test_gpu_discovery() {
    let manager = create_test_gpu_manager().await.unwrap();
    
    let devices = manager.discover_gpus().await.unwrap();
    
    assert!(!devices.is_empty());
    for device in &devices {
        assert!(device.device_id >= 0);
        assert!(!device.name.is_empty());
        assert!(device.total_memory > 0);
        assert!(device.compute_capability.major >= 6); // Modern GPUs
        assert!(device.is_available);
    }
}

#[tokio::test]
async fn test_gpu_allocation() {
    let manager = create_test_gpu_manager().await.unwrap();
    
    // Request GPU allocation
    let allocation = manager
        .allocate_gpu("model_1", 4_000_000_000) // 4GB
        .await
        .unwrap();
    
    assert_eq!(allocation.model_id, "model_1");
    assert!(allocation.gpu_device_id >= 0);
    assert_eq!(allocation.memory_allocated, 4_000_000_000);
    assert!(allocation.is_active);
    
    // Check GPU is marked as in-use
    let status = manager.get_gpu_status(allocation.gpu_device_id).await.unwrap();
    assert_eq!(status, GpuStatus::InUse);
}

#[tokio::test]
async fn test_gpu_deallocation() {
    let manager = create_test_gpu_manager().await.unwrap();
    
    // Allocate then deallocate
    let allocation = manager
        .allocate_gpu("model_1", 2_000_000_000)
        .await
        .unwrap();
    
    let gpu_id = allocation.gpu_device_id;
    
    manager.deallocate_gpu(&allocation.allocation_id).await.unwrap();
    
    // GPU should be available again
    let status = manager.get_gpu_status(gpu_id).await.unwrap();
    assert_eq!(status, GpuStatus::Available);
    
    // Memory should be freed
    let metrics = manager.get_gpu_metrics(gpu_id).await.unwrap();
    assert_eq!(metrics.memory_used, 0);
}

#[tokio::test]
async fn test_multi_gpu_allocation() {
    let manager = create_test_gpu_manager().await.unwrap();
    
    // Allocate models across multiple GPUs
    let alloc1 = manager.allocate_gpu("model_1", 6_000_000_000).await.unwrap();
    let alloc2 = manager.allocate_gpu("model_2", 6_000_000_000).await.unwrap();
    
    // Should be on different GPUs
    assert_ne!(alloc1.gpu_device_id, alloc2.gpu_device_id);
    
    // Both GPUs should be in use
    let status1 = manager.get_gpu_status(alloc1.gpu_device_id).await.unwrap();
    let status2 = manager.get_gpu_status(alloc2.gpu_device_id).await.unwrap();
    assert_eq!(status1, GpuStatus::InUse);
    assert_eq!(status2, GpuStatus::InUse);
}

#[tokio::test]
async fn test_gpu_memory_limits() {
    let manager = create_test_gpu_manager().await.unwrap();
    
    // Try to allocate more memory than available
    let result = manager
        .allocate_gpu("huge_model", 100_000_000_000) // 100GB
        .await;
    
    assert!(result.is_err());
    match result.unwrap_err().downcast::<GpuError>() {
        Ok(GpuError::InsufficientMemory { requested, available }) => {
            assert_eq!(requested, 100_000_000_000);
            assert!(available < requested);
        }
        _ => panic!("Expected InsufficientMemory error"),
    }
}

#[tokio::test]
async fn test_fallback_to_cpu() {
    let mut config = GpuConfig::default();
    config.fallback_to_cpu = true;
    config.gpu_device_ids = vec![]; // No GPUs available
    
    let manager = GpuManager::new(config).await.unwrap();
    
    // Should fallback to CPU allocation
    let allocation = manager
        .allocate_gpu("model_1", 2_000_000_000)
        .await
        .unwrap();
    
    assert_eq!(allocation.gpu_device_id, -1); // CPU indicator
    assert!(allocation.is_cpu_fallback);
}

#[tokio::test]
async fn test_gpu_scheduling_best_fit() {
    let manager = create_test_gpu_manager().await.unwrap();
    
    // Pre-allocate on GPU 0
    manager.allocate_gpu("existing", 3_000_000_000).await.unwrap();
    
    // New allocation should go to GPU with most free memory
    let allocation = manager
        .allocate_gpu("new_model", 2_000_000_000)
        .await
        .unwrap();
    
    assert_eq!(allocation.gpu_device_id, 1); // Should use GPU 1
}

#[tokio::test]
async fn test_gpu_metrics_collection() {
    let manager = create_test_gpu_manager().await.unwrap();
    
    let allocation = manager
        .allocate_gpu("model_1", 4_000_000_000)
        .await
        .unwrap();
    
    let metrics = manager.get_gpu_metrics(allocation.gpu_device_id).await.unwrap();
    
    assert!(metrics.temperature_celsius > 0.0);
    assert!(metrics.utilization_percent >= 0.0 && metrics.utilization_percent <= 100.0);
    assert_eq!(metrics.memory_used, 4_000_000_000);
    assert!(metrics.memory_total > metrics.memory_used);
    assert!(metrics.power_draw_watts > 0.0);
    assert!(!metrics.processes.is_empty());
}

#[tokio::test]
async fn test_gpu_task_scheduling() {
    let manager = create_test_gpu_manager().await.unwrap();
    
    let scheduler = manager.get_scheduler();
    
    // Schedule high priority task
    let task_id = scheduler
        .schedule_task("urgent_inference", TaskPriority::High, 2_000_000_000)
        .await
        .unwrap();
    
    assert!(!task_id.is_empty());
    
    // Check task status
    let status = scheduler.get_task_status(&task_id).await.unwrap();
    assert!(matches!(status, GpuStatus::Scheduled | GpuStatus::InUse));
}

#[tokio::test]
async fn test_gpu_memory_pool() {
    let manager = create_test_gpu_manager().await.unwrap();
    
    // Create memory pool for efficient allocation
    let pool = manager
        .create_memory_pool(0, 8_000_000_000) // 8GB pool on GPU 0
        .await
        .unwrap();
    
    // Allocate from pool
    let alloc1 = pool.allocate("model_1", 2_000_000_000).await.unwrap();
    let alloc2 = pool.allocate("model_2", 3_000_000_000).await.unwrap();
    
    assert_eq!(pool.available_memory().await, 3_000_000_000);
    
    // Deallocate and check pool
    pool.deallocate(alloc1).await.unwrap();
    assert_eq!(pool.available_memory().await, 5_000_000_000);
}

#[tokio::test]
async fn test_gpu_capabilities_check() {
    let manager = create_test_gpu_manager().await.unwrap();
    
    let capabilities = manager.check_capabilities(0).await.unwrap();
    
    assert!(capabilities.supports_fp16);
    assert!(capabilities.supports_int8);
    assert!(capabilities.tensor_core_available);
    assert!(capabilities.max_threads_per_block > 0);
    assert!(capabilities.max_grid_dimensions.len() == 3);
    assert!(capabilities.cuda_version.0 > 0 || capabilities.cuda_version.1 > 0);
}

#[tokio::test]
async fn test_concurrent_gpu_operations() {
    let manager = Arc::new(create_test_gpu_manager().await.unwrap());
    
    // Simulate concurrent allocation requests
    let mut handles = vec![];
    
    for i in 0..5 {
        let mgr = manager.clone();
        let handle = tokio::spawn(async move {
            mgr.allocate_gpu(&format!("model_{}", i), 1_000_000_000).await
        });
        handles.push(handle);
    }
    
    let results: Vec<_> = futures::future::join_all(handles).await;
    
    // Count successful allocations
    let successful = results
        .iter()
        .filter(|r| r.is_ok() && r.as_ref().unwrap().is_ok())
        .count();
    
    assert!(successful >= 2); // At least 2 should succeed with 2 GPUs
}

#[tokio::test]
async fn test_gpu_health_monitoring() {
    let manager = create_test_gpu_manager().await.unwrap();
    
    manager.start_health_monitoring().await.unwrap();
    
    // Wait for health check
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    
    let health = manager.get_health_status().await.unwrap();
    
    assert!(health.healthy_gpus >= 0);
    assert_eq!(health.total_gpus, 2);
    assert!(health.last_check_time > 0);
    
    for alert in &health.alerts {
        println!("GPU Alert: {}", alert.message);
    }
}