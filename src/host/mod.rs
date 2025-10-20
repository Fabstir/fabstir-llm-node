// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
pub mod availability;
pub mod model_config;
pub mod pricing;
pub mod registration;
pub mod registry;
pub mod resources;
pub mod selection;

pub use model_config::{
    HostingError, ModelConfig, ModelHostingManager, ModelMetadata, ModelParameters, ModelStatus,
};

pub use pricing::{
    Currency, DynamicPricingConfig, PriceUpdate, PricingError, PricingManager, PricingModel,
    PricingTier,
};

pub use availability::{
    AvailabilityManager, AvailabilitySchedule, AvailabilityStatus, CapacityConfig,
    MaintenanceWindow, ScheduleError,
};

pub use registration::{NodeMetadata, NodeRegistration, RegistrationConfig};

pub use registry::{HostInfo, HostRegistry, RegistryStats};

pub use resources::{
    AlertLevel, AlertThreshold, CpuMetrics, GpuMetrics, MemoryMetrics, MonitoringError,
    NetworkMetrics, ResourceAlert, ResourceMetrics, ResourceMonitor,
};

pub use selection::{HostSelector, JobRequirements, PerformanceMetrics, ScoringWeights};
