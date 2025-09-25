// src/monitoring/mod.rs - Main monitoring module

pub mod alerting;
pub mod dashboards;
pub mod health_checks;
pub mod metrics;

// Re-export main types
pub use metrics::{
    AggregationType, Counter, Gauge, Histogram, HistogramStatistics, MetricLabel, MetricType,
    MetricValue, MetricsCollector, MetricsConfig, MetricsExporter, MetricsRegistry,
    PrometheusExporter, Summary, SummaryStatistics, TimeWindow,
};

pub use health_checks::{
    CheckType, ComponentHealth, DependencyCheck, DependencyHealth, HealthCheck, HealthChecker,
    HealthConfig, HealthEndpoint, HealthReport, HealthResponse, HealthStatus, LivenessProbe,
    ReadinessProbe, ResourceCheck, ResourceInfo, ResourceReport, ThresholdConfig,
};

pub use alerting::{
    Alert, AlertChannel, AlertCondition, AlertConfig, AlertError, AlertGroup, AlertHistory,
    AlertLevel, AlertManager, AlertNotification, AlertRule, AlertSeverity, AlertSilence,
    AlertState, AlertStatus, AlertTemplate, EscalationPolicy, LogChannel, NotificationChannel,
    RateAlert, ThresholdAlert, WebhookChannel,
};

pub use dashboards::{
    Annotation, BarChartWidget, Dashboard, DashboardConfig, DashboardError, DashboardExport,
    DashboardManager, DashboardUpdate, DataSource, GaugeWidget, GraphWidget, GridPosition,
    HeatmapWidget, Layout, LogWidget, Panel, PieChartWidget, PublicLink, Query as DashboardQuery,
    Query, QueryResult, RefreshInterval, StatWidget, TableWidget, TimeRange, Variable,
    Visualization, Widget, WidgetType,
};
