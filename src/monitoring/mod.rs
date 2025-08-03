// src/monitoring/mod.rs - Main monitoring module

pub mod metrics;
pub mod health_checks;
pub mod alerting;
pub mod dashboards;

// Re-export main types
pub use metrics::{
    MetricsCollector, MetricsConfig, MetricType, MetricValue,
    AggregationType, TimeWindow, MetricsExporter, PrometheusExporter,
    MetricsRegistry, Counter, Gauge, Histogram, MetricLabel,
    HistogramStatistics, Summary, SummaryStatistics,
};

pub use health_checks::{
    HealthChecker, HealthConfig, HealthStatus, ComponentHealth,
    CheckType, HealthCheck, HealthReport, DependencyCheck,
    ResourceCheck, ThresholdConfig, HealthEndpoint,
    ResourceType, HealthHistory, LivenessProbe, ReadinessProbe, HealthError,
};

pub use alerting::{
    AlertManager, AlertConfig, Alert, AlertLevel, AlertRule,
    AlertCondition, AlertChannel, NotificationChannel, AlertStatus,
    AlertHistory, AlertGroup, ThresholdAlert, RateAlert, WebhookChannel,
    AlertSeverity, AlertState, AlertTemplate, AlertSilence,
    EscalationPolicy, AlertNotification, AlertError, LogChannel,
};

pub use dashboards::{
    DashboardManager, Dashboard, Panel, Widget, WidgetType,
    GraphWidget, GaugeWidget, TableWidget, HeatmapWidget, LogWidget,
    StatWidget, PieChartWidget, BarChartWidget, TimeRange, Variable,
    Query as DashboardQuery, DashboardUpdate, DashboardError, GridPosition,
    RefreshInterval, Annotation, DashboardConfig, Query, Layout, 
    PublicLink, DataSource, Visualization, DashboardExport, QueryResult,
};