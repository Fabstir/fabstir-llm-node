// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GpuConfig {
    pub enable_gpu: bool,
    pub gpu_device_ids: Vec<i32>,
    pub memory_fraction: f32,
    pub allow_gpu_growth: bool,
    pub gpu_scheduling: AllocationStrategy,
    pub max_concurrent_models: usize,
    pub fallback_to_cpu: bool,
    pub nvidia_visible_devices: Option<String>,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            enable_gpu: true,
            gpu_device_ids: vec![0],
            memory_fraction: 0.9,
            allow_gpu_growth: true,
            gpu_scheduling: AllocationStrategy::BestFit,
            max_concurrent_models: 4,
            fallback_to_cpu: true,
            nvidia_visible_devices: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AllocationStrategy {
    FirstFit,
    BestFit,
    RoundRobin,
    LeastUtilized,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GpuStatus {
    Available,
    InUse,
    Scheduled,
    Maintenance,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskPriority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Debug, Clone)]
pub struct GpuDevice {
    pub device_id: i32,
    pub name: String,
    pub total_memory: u64,
    pub available_memory: u64,
    pub compute_capability: ComputeCapability,
    pub is_available: bool,
    pub current_allocations: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ComputeCapability {
    pub major: u32,
    pub minor: u32,
}

#[derive(Debug, Clone)]
pub struct GpuAllocation {
    pub allocation_id: String,
    pub model_id: String,
    pub gpu_device_id: i32,
    pub memory_allocated: u64,
    pub is_active: bool,
    pub is_cpu_fallback: bool,
    pub allocated_at: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct GpuMetrics {
    pub temperature_celsius: f32,
    pub utilization_percent: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub power_draw_watts: f32,
    pub processes: Vec<ProcessInfo>,
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub memory_used: u64,
}

#[derive(Debug, Clone)]
pub struct GpuCapabilities {
    pub cuda_cores: u32,
    pub tensor_cores: u32,
    pub memory_bandwidth_gb_per_sec: f32,
    pub max_threads_per_block: u32,
    pub warp_size: u32,
    pub supports_fp16: bool,
    pub supports_int8: bool,
    pub tensor_core_available: bool,
    pub max_grid_dimensions: [u32; 3],
    pub cuda_version: (u32, u32),
}

#[derive(Debug, Clone)]
pub struct AggregateMetrics {
    pub average_utilization: f64,
    pub average_memory_usage: f64,
    pub total_allocations: usize,
    pub failed_allocations: usize,
}

#[derive(Debug, Clone)]
pub struct GpuHealthStatus {
    pub device_id: i32,
    pub is_healthy: bool,
    pub temperature_celsius: f32,
    pub fan_speed_percent: f32,
    pub memory_errors: u64,
    pub pcie_errors: u64,
    pub alerts: Vec<GpuAlert>,
}

#[derive(Debug, Clone)]
pub struct GpuAlert {
    pub device_id: i32,
    pub severity: AlertSeverity,
    pub message: String,
    pub timestamp: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct OverallHealthStatus {
    pub healthy_gpus: usize,
    pub total_gpus: usize,
    pub last_check_time: u64,
    pub alerts: Vec<GpuAlert>,
}

#[derive(Debug, Clone)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Error, Debug)]
pub enum GpuError {
    #[error("Insufficient memory - requested: {requested}, available: {available}")]
    InsufficientMemory { requested: u64, available: u64 },
    #[error("GPU not available: {device_id}")]
    GpuNotAvailable { device_id: i32 },
    #[error("Allocation not found: {allocation_id}")]
    AllocationNotFound { allocation_id: String },
    #[error("No GPU devices found")]
    NoGpuDevices,
    #[error("GPU operation failed: {reason}")]
    OperationFailed { reason: String },
}

// Wrapper type for easier API access to MemoryPool
#[derive(Clone)]
pub struct MemoryPoolHandle {
    inner: Arc<Mutex<MemoryPool>>,
}

impl MemoryPoolHandle {
    pub fn new(pool: MemoryPool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(pool)),
        }
    }

    pub async fn allocate(&self, model_id: &str, size: u64) -> Result<String> {
        let mut pool = self.inner.lock().await;
        pool.allocate(model_id, size).await
    }

    pub async fn deallocate(&self, allocation_id: String) -> Result<()> {
        let mut pool = self.inner.lock().await;
        pool.deallocate(allocation_id).await
    }

    pub async fn available_memory(&self) -> u64 {
        let pool = self.inner.lock().await;
        pool.available_memory().await
    }
}

#[derive(Clone)]
pub struct MemoryPool {
    device_id: i32,
    total_size: u64,
    allocated_size: u64,
    chunks: HashMap<String, MemoryChunk>,
}

#[derive(Debug, Clone)]
struct MemoryChunk {
    id: String,
    size: u64,
    is_allocated: bool,
}

impl MemoryPool {
    pub fn new(device_id: i32, total_size: u64) -> Self {
        Self {
            device_id,
            total_size,
            allocated_size: 0,
            chunks: HashMap::new(),
        }
    }

    pub async fn allocate(&mut self, model_id: &str, size: u64) -> Result<String> {
        if self.allocated_size + size > self.total_size {
            return Err(GpuError::InsufficientMemory {
                requested: size,
                available: self.total_size - self.allocated_size,
            }
            .into());
        }

        let allocation_id = Uuid::new_v4().to_string();
        let chunk = MemoryChunk {
            id: allocation_id.clone(),
            size,
            is_allocated: true,
        };

        self.chunks.insert(allocation_id.clone(), chunk);
        self.allocated_size += size;

        Ok(allocation_id)
    }

    pub async fn deallocate(&mut self, allocation_id: String) -> Result<()> {
        if let Some(chunk) = self.chunks.get_mut(&allocation_id) {
            if chunk.is_allocated {
                self.allocated_size -= chunk.size;
                chunk.is_allocated = false;
            }
            Ok(())
        } else {
            Err(anyhow::anyhow!("Allocation not found"))
        }
    }

    pub async fn available_memory(&self) -> u64 {
        self.total_size - self.allocated_size
    }

    pub async fn allocate_model(&mut self, model_id: &str, size: u64) -> Result<String> {
        self.allocate(model_id, size).await
    }
}

pub struct GpuScheduler {
    tasks: Arc<RwLock<HashMap<String, ScheduledTask>>>,
    device_queues: Arc<RwLock<HashMap<i32, Vec<String>>>>,
}

#[derive(Debug, Clone)]
struct ScheduledTask {
    id: String,
    name: String,
    priority: TaskPriority,
    memory_required: u64,
    status: GpuStatus,
    device_id: Option<i32>,
}

impl GpuScheduler {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            device_queues: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn schedule_task(
        &self,
        name: &str,
        priority: TaskPriority,
        memory_required: u64,
    ) -> Result<String> {
        let task_id = Uuid::new_v4().to_string();
        let task = ScheduledTask {
            id: task_id.clone(),
            name: name.to_string(),
            priority,
            memory_required,
            status: GpuStatus::Scheduled,
            device_id: None,
        };

        let mut tasks = self.tasks.write().await;
        tasks.insert(task_id.clone(), task);

        Ok(task_id)
    }

    pub async fn get_task_status(&self, task_id: &str) -> Result<GpuStatus> {
        let tasks = self.tasks.read().await;
        if let Some(task) = tasks.get(task_id) {
            Ok(task.status.clone())
        } else {
            Err(anyhow::anyhow!("Task not found"))
        }
    }
}

struct GpuState {
    devices: HashMap<i32, GpuDevice>,
    allocations: HashMap<String, GpuAllocation>,
    memory_pools: HashMap<i32, MemoryPool>,
    scheduler: GpuScheduler,
    next_device_idx: usize,
}

pub struct GpuManager {
    config: GpuConfig,
    state: Arc<RwLock<GpuState>>,
}

impl GpuManager {
    pub async fn new(config: GpuConfig) -> Result<Self> {
        let mut devices = HashMap::new();

        // Mock GPU devices based on config
        for &device_id in &config.gpu_device_ids {
            let device = GpuDevice {
                device_id,
                name: format!("NVIDIA RTX 4090 #{}", device_id),
                total_memory: 24 * 1024 * 1024 * 1024, // 24GB
                available_memory: 24 * 1024 * 1024 * 1024,
                compute_capability: ComputeCapability { major: 8, minor: 9 },
                is_available: true,
                current_allocations: Vec::new(),
            };
            devices.insert(device_id, device);
        }

        let state = GpuState {
            devices,
            allocations: HashMap::new(),
            memory_pools: HashMap::new(),
            scheduler: GpuScheduler::new(),
            next_device_idx: 0,
        };

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(state)),
        })
    }

    pub async fn discover_gpus(&self) -> Result<Vec<GpuDevice>> {
        let state = self.state.read().await;
        Ok(state.devices.values().cloned().collect())
    }

    pub async fn allocate_gpu(
        &self,
        model_id: &str,
        memory_required: u64,
    ) -> Result<GpuAllocation> {
        let mut state = self.state.write().await;

        // Check if we should fallback to CPU
        if state.devices.is_empty() && self.config.fallback_to_cpu {
            let allocation = GpuAllocation {
                allocation_id: Uuid::new_v4().to_string(),
                model_id: model_id.to_string(),
                gpu_device_id: -1,
                memory_allocated: memory_required,
                is_active: true,
                is_cpu_fallback: true,
                allocated_at: std::time::Instant::now(),
            };
            state
                .allocations
                .insert(allocation.allocation_id.clone(), allocation.clone());
            return Ok(allocation);
        }

        // Find suitable GPU based on allocation strategy
        let device_id = match self.config.gpu_scheduling {
            AllocationStrategy::FirstFit => state
                .devices
                .iter()
                .find(|(_, device)| {
                    device.is_available && device.available_memory >= memory_required
                })
                .map(|(id, _)| *id),
            AllocationStrategy::BestFit => state
                .devices
                .iter()
                .filter(|(_, device)| {
                    device.is_available && device.available_memory >= memory_required
                })
                .max_by_key(|(_, device)| device.available_memory)
                .map(|(id, _)| *id),
            AllocationStrategy::RoundRobin => {
                let device_ids: Vec<_> = state.devices.keys().cloned().collect();
                if device_ids.is_empty() {
                    None
                } else {
                    let idx = state.next_device_idx % device_ids.len();
                    state.next_device_idx = (state.next_device_idx + 1) % device_ids.len();
                    Some(device_ids[idx])
                }
            }
            AllocationStrategy::LeastUtilized => state
                .devices
                .iter()
                .filter(|(_, device)| {
                    device.is_available && device.available_memory >= memory_required
                })
                .max_by_key(|(_, device)| device.available_memory)
                .map(|(id, _)| *id),
        };

        let device_id = device_id.ok_or_else(|| GpuError::InsufficientMemory {
            requested: memory_required,
            available: state
                .devices
                .values()
                .map(|d| d.available_memory)
                .max()
                .unwrap_or(0),
        })?;

        // Allocate memory on selected device
        if let Some(device) = state.devices.get_mut(&device_id) {
            if device.available_memory < memory_required {
                return Err(GpuError::InsufficientMemory {
                    requested: memory_required,
                    available: device.available_memory,
                }
                .into());
            }

            device.available_memory -= memory_required;
            device.current_allocations.push(model_id.to_string());

            let allocation = GpuAllocation {
                allocation_id: Uuid::new_v4().to_string(),
                model_id: model_id.to_string(),
                gpu_device_id: device_id,
                memory_allocated: memory_required,
                is_active: true,
                is_cpu_fallback: false,
                allocated_at: std::time::Instant::now(),
            };

            state
                .allocations
                .insert(allocation.allocation_id.clone(), allocation.clone());
            Ok(allocation)
        } else {
            Err(GpuError::GpuNotAvailable { device_id }.into())
        }
    }

    pub async fn deallocate_gpu(&self, allocation_id: &str) -> Result<()> {
        let mut state = self.state.write().await;

        if let Some(allocation) = state.allocations.remove(allocation_id) {
            if allocation.gpu_device_id >= 0 {
                if let Some(device) = state.devices.get_mut(&allocation.gpu_device_id) {
                    device.available_memory += allocation.memory_allocated;
                    device
                        .current_allocations
                        .retain(|id| id != &allocation.model_id);
                }
            }
            Ok(())
        } else {
            Err(GpuError::AllocationNotFound {
                allocation_id: allocation_id.to_string(),
            }
            .into())
        }
    }

    pub async fn get_gpu_status(&self, device_id: i32) -> Result<GpuStatus> {
        let state = self.state.read().await;

        if let Some(device) = state.devices.get(&device_id) {
            if device.current_allocations.is_empty() {
                Ok(GpuStatus::Available)
            } else {
                Ok(GpuStatus::InUse)
            }
        } else {
            Err(GpuError::GpuNotAvailable { device_id }.into())
        }
    }

    pub async fn get_gpu_metrics(&self, device_id: i32) -> Result<GpuMetrics> {
        let state = self.state.read().await;

        if let Some(device) = state.devices.get(&device_id) {
            let memory_used = device.total_memory - device.available_memory;
            let utilization = if device.total_memory > 0 {
                (memory_used as f32 / device.total_memory as f32) * 100.0
            } else {
                0.0
            };

            let processes: Vec<ProcessInfo> = device
                .current_allocations
                .iter()
                .map(|model_id| ProcessInfo {
                    pid: 1000 + device_id as u32,
                    name: model_id.clone(),
                    memory_used: memory_used / device.current_allocations.len() as u64,
                })
                .collect();

            Ok(GpuMetrics {
                temperature_celsius: 65.0 + utilization * 0.2,
                utilization_percent: utilization,
                memory_used,
                memory_total: device.total_memory,
                power_draw_watts: 200.0 + utilization * 2.0,
                processes,
            })
        } else {
            Err(GpuError::GpuNotAvailable { device_id }.into())
        }
    }

    pub fn get_scheduler(&self) -> GpuScheduler {
        GpuScheduler::new()
    }

    pub async fn create_memory_pool(&self, device_id: i32, size: u64) -> Result<MemoryPoolHandle> {
        let mut state = self.state.write().await;

        if !state.devices.contains_key(&device_id) {
            return Err(GpuError::GpuNotAvailable { device_id }.into());
        }

        let pool = MemoryPool::new(device_id, size);
        state.memory_pools.insert(device_id, pool.clone());

        Ok(MemoryPoolHandle::new(pool))
    }

    pub async fn check_capabilities(&self, device_id: i32) -> Result<GpuCapabilities> {
        let state = self.state.read().await;

        if let Some(_device) = state.devices.get(&device_id) {
            Ok(GpuCapabilities {
                cuda_cores: 10496, // Mock RTX 4090 specs
                tensor_cores: 328,
                memory_bandwidth_gb_per_sec: 1008.0,
                max_threads_per_block: 1024,
                warp_size: 32,
                supports_fp16: true,
                supports_int8: true,
                tensor_core_available: true,
                max_grid_dimensions: [2147483647, 65535, 65535],
                cuda_version: (12, 3),
            })
        } else {
            Err(GpuError::GpuNotAvailable { device_id }.into())
        }
    }

    pub async fn health_check(&self, device_id: i32) -> Result<bool> {
        let state = self.state.read().await;

        if let Some(device) = state.devices.get(&device_id) {
            Ok(device.is_available)
        } else {
            Ok(false)
        }
    }

    pub async fn reset_gpu(&self, device_id: i32) -> Result<()> {
        let mut state = self.state.write().await;

        // Check if device exists
        if !state.devices.contains_key(&device_id) {
            return Err(GpuError::GpuNotAvailable { device_id }.into());
        }

        // Clear all allocations for this device
        let allocations_to_remove: Vec<_> = state
            .allocations
            .iter()
            .filter(|(_, alloc)| alloc.gpu_device_id == device_id)
            .map(|(id, _)| id.clone())
            .collect();

        for id in allocations_to_remove {
            state.allocations.remove(&id);
        }

        // Reset device state
        if let Some(device) = state.devices.get_mut(&device_id) {
            device.available_memory = device.total_memory;
            device.current_allocations.clear();
            device.is_available = true;
        }

        Ok(())
    }

    pub async fn get_health_status(&self) -> Result<OverallHealthStatus> {
        let state = self.state.read().await;

        let mut healthy_gpus = 0;
        let total_gpus = state.devices.len();
        let mut alerts = Vec::new();

        for device in state.devices.values() {
            if device.is_available {
                healthy_gpus += 1;
            } else {
                alerts.push(GpuAlert {
                    device_id: device.device_id,
                    severity: AlertSeverity::Warning,
                    message: format!("GPU {} is unavailable", device.device_id),
                    timestamp: std::time::Instant::now(),
                });
            }
        }

        Ok(OverallHealthStatus {
            healthy_gpus,
            total_gpus,
            last_check_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            alerts,
        })
    }

    pub async fn start_health_monitoring(&self) -> Result<()> {
        // Mock implementation - in real implementation would start monitoring thread
        Ok(())
    }

    pub async fn get_device_health_status(&self, device_id: i32) -> Result<GpuHealthStatus> {
        let state = self.state.read().await;

        if let Some(device) = state.devices.get(&device_id) {
            let metrics = self.get_gpu_metrics(device_id).await?;

            Ok(GpuHealthStatus {
                device_id,
                is_healthy: device.is_available && metrics.temperature_celsius < 85.0,
                temperature_celsius: metrics.temperature_celsius as f32,
                fan_speed_percent: (50.0 + (metrics.temperature_celsius - 60.0).max(0.0)) as f32,
                memory_errors: 0,
                pcie_errors: 0,
                alerts: vec![],
            })
        } else {
            Err(GpuError::GpuNotAvailable { device_id }.into())
        }
    }

    pub async fn get_aggregate_metrics(&self) -> AggregateMetrics {
        let state = self.state.read().await;

        let total_utilization: f32 = state
            .devices
            .values()
            .map(|d| {
                let used = d.total_memory - d.available_memory;
                if d.total_memory > 0 {
                    (used as f32 / d.total_memory as f32) * 100.0
                } else {
                    0.0
                }
            })
            .sum();

        let device_count = state.devices.len() as f64;
        let average_utilization = if device_count > 0.0 {
            total_utilization as f64 / device_count
        } else {
            0.0
        };

        let total_memory: u64 = state.devices.values().map(|d| d.total_memory).sum();
        let used_memory: u64 = state
            .devices
            .values()
            .map(|d| d.total_memory - d.available_memory)
            .sum();

        let average_memory_usage = if total_memory > 0 {
            (used_memory as f64 / total_memory as f64) * 100.0
        } else {
            0.0
        };

        AggregateMetrics {
            average_utilization,
            average_memory_usage,
            total_allocations: state.allocations.len(),
            failed_allocations: 0, // Would track this in real implementation
        }
    }
}
