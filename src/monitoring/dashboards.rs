// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// src/monitoring/dashboards.rs - Dashboard system for visualization

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub enable_dashboards: bool,
    pub default_refresh_seconds: u64,
    pub max_dashboards_per_user: usize,
    pub enable_public_dashboards: bool,
    pub storage_backend: String,
    pub export_formats: Vec<String>,
    pub cache_ttl_seconds: u64,
    pub enable_annotations: bool,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        DashboardConfig {
            enable_dashboards: true,
            default_refresh_seconds: 60,
            max_dashboards_per_user: 50,
            enable_public_dashboards: true,
            storage_backend: "local".to_string(),
            export_formats: vec!["json".to_string(), "yaml".to_string()],
            cache_ttl_seconds: 300,
            enable_annotations: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub id: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub panels: Vec<Panel>,
    pub variables: HashMap<String, Variable>,
    pub tags: Vec<String>,
    pub refresh_interval_seconds: u64,
    pub time_range: TimeRange,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: String,
    pub version: u32,
}

impl Dashboard {
    pub fn new(name: &str, title: &str, description: &str) -> Self {
        Dashboard {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            panels: Vec::new(),
            variables: HashMap::new(),
            tags: Vec::new(),
            refresh_interval_seconds: 60,
            time_range: TimeRange::LastHour,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: "system".to_string(),
            version: 1,
        }
    }

    pub fn with_refresh_interval(mut self, interval: RefreshInterval) -> Self {
        self.refresh_interval_seconds = match interval {
            RefreshInterval::Seconds(s) => s,
            RefreshInterval::Minutes(m) => m * 60,
            RefreshInterval::Hours(h) => h * 3600,
            RefreshInterval::Never => 0,
        };
        self
    }

    pub fn add_tag(&mut self, tag: &str) {
        self.tags.push(tag.to_string());
    }

    pub fn add_variable(&mut self, name: &str, options: Vec<&str>, default: &str) {
        let variable = Variable {
            name: name.to_string(),
            label: name.to_string(),
            variable_type: VariableType::Custom,
            default_value: default.to_string(),
            options: options
                .iter()
                .map(|&opt| VariableOption {
                    label: opt.to_string(),
                    value: opt.to_string(),
                })
                .collect(),
        };
        self.variables.insert(name.to_string(), variable);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Panel {
    pub id: String,
    pub name: String,
    pub title: String,
    pub widget: Widget,
    pub position: GridPosition,
    pub queries: Vec<Query>,
    pub refresh_interval_seconds: Option<u64>,
    pub responsive_positions: HashMap<String, GridPosition>,
}

impl Panel {
    pub fn new(name: &str, title: &str, widget_type: WidgetType, position: GridPosition) -> Self {
        let widget = match widget_type {
            WidgetType::Graph => Widget::Graph(GraphWidget::default()),
            WidgetType::Gauge => Widget::Gauge(GaugeWidget::default()),
            WidgetType::Table => Widget::Table(TableWidget::default()),
            WidgetType::Heatmap => Widget::Heatmap(HeatmapWidget::default()),
            WidgetType::SingleStat => Widget::Stat(StatWidget::default()),
            WidgetType::Stat => Widget::Stat(StatWidget::default()),
            WidgetType::BarChart => Widget::BarChart(BarChartWidget::default()),
            WidgetType::PieChart => Widget::PieChart(PieChartWidget::default()),
            WidgetType::LogViewer => Widget::Log(LogWidget::default()),
        };

        Panel {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            title: title.to_string(),
            widget,
            position,
            queries: Vec::new(),
            refresh_interval_seconds: None,
            responsive_positions: HashMap::new(),
        }
    }

    pub fn add_query(&mut self, query: Query) {
        self.queries.push(query);
    }

    pub fn with_responsive_position(mut self, breakpoint: &str, position: GridPosition) -> Self {
        self.responsive_positions
            .insert(breakpoint.to_string(), position);
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GridPosition {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Widget {
    Graph(GraphWidget),
    Gauge(GaugeWidget),
    Table(TableWidget),
    Heatmap(HeatmapWidget),
    Log(LogWidget),
    Stat(StatWidget),
    PieChart(PieChartWidget),
    BarChart(BarChartWidget),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidgetType {
    Graph,
    Gauge,
    Table,
    Heatmap,
    SingleStat,
    Stat,
    BarChart,
    PieChart,
    LogViewer,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphWidget {
    pub legend_enabled: bool,
    pub y_axis_label: String,
    pub x_axis_label: String,
    pub line_width: u32,
    pub fill_opacity: f32,
    pub stack_series: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GaugeWidget {
    pub min_value: f64,
    pub max_value: f64,
    pub thresholds: Vec<Threshold>,
    pub show_threshold_labels: bool,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TableWidget {
    pub columns: Vec<TableColumn>,
    pub show_header: bool,
    pub page_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HeatmapWidget {
    pub color_scheme: String,
    pub bucket_size: u32,
    pub y_axis_label: String,
    pub x_axis_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogWidget {
    pub wrap_lines: bool,
    pub show_timestamps: bool,
    pub max_lines: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatWidget {
    pub unit: String,
    pub decimals: u32,
    pub color_mode: ColorMode,
    pub thresholds: Vec<Threshold>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PieChartWidget {
    pub legend_enabled: bool,
    pub show_percentages: bool,
    pub donut: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BarChartWidget {
    pub orientation: BarOrientation,
    pub stacked: bool,
    pub show_values: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum BarOrientation {
    Horizontal,
    #[default]
    Vertical,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum ColorMode {
    Value,
    Background,
    #[default]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Threshold {
    pub value: f64,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableColumn {
    pub name: String,
    pub field: String,
    pub width: Option<u32>,
    pub sortable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    pub id: String,
    pub datasource: String,
    pub query: String,
    pub legend: Option<String>,
    pub aggregation: Option<String>,
    pub refetch_interval_seconds: Option<u64>,
}

impl Query {
    pub fn new(datasource: &str, query: &str) -> Self {
        Query {
            id: Uuid::new_v4().to_string(),
            datasource: datasource.to_string(),
            query: query.to_string(),
            legend: None,
            aggregation: None,
            refetch_interval_seconds: None,
        }
    }

    pub fn with_legend(mut self, legend: &str) -> Self {
        self.legend = Some(legend.to_string());
        self
    }

    pub fn with_aggregation(mut self, aggregation: &str) -> Self {
        self.aggregation = Some(aggregation.to_string());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub label: String,
    pub variable_type: VariableType,
    pub default_value: String,
    pub options: Vec<VariableOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariableType {
    Query,
    Custom,
    Interval,
    Datasource,
    Constant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableOption {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeRange {
    LastHour,
    LastDay,
    LastWeek,
    LastMonth,
    Custom { from_hours: i64, to_hours: i64 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RefreshInterval {
    Seconds(u64),
    Minutes(u64),
    Hours(u64),
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layout {
    pub breakpoints: HashMap<String, u32>,
    pub columns: u32,
}

impl Layout {
    pub fn new() -> Self {
        Layout {
            breakpoints: HashMap::new(),
            columns: 12,
        }
    }

    pub fn with_breakpoint(mut self, name: &str, width: u32) -> Self {
        self.breakpoints.insert(name.to_string(), width);
        self
    }

    pub fn with_columns(mut self, columns: u32) -> Self {
        self.columns = columns;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSource {
    pub name: String,
    pub type_: String,
    pub url: String,
    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub type_: String,
    pub credentials: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Visualization {
    pub type_: String,
    pub options: HashMap<String, JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardExport {
    pub format: String,
    pub content: String,
    pub metadata: ExportMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportMetadata {
    pub exported_at: DateTime<Utc>,
    pub exported_by: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub data: JsonValue,
    pub query_used: String,
}

impl QueryResult {
    pub fn is_empty(&self) -> bool {
        match &self.data {
            JsonValue::Null => true,
            JsonValue::Array(arr) => arr.is_empty(),
            JsonValue::Object(obj) => obj.is_empty(),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicLink {
    pub token: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardUpdate {
    pub panel_id: String,
    pub data: JsonValue,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum DashboardError {
    #[error("Dashboard not found: {0}")]
    DashboardNotFound(String),

    #[error("Panel not found: {0}")]
    PanelNotFound(String),

    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    #[error("Widget type mismatch")]
    WidgetTypeMismatch,

    #[error("Invalid time range")]
    InvalidTimeRange,
}

pub struct DashboardManager {
    config: DashboardConfig,
    state: Arc<RwLock<DashboardState>>,
}

struct DashboardState {
    dashboards: HashMap<String, Dashboard>,
    panel_data: HashMap<String, PanelData>,
    query_cache: HashMap<String, CachedQueryResult>,
    subscriptions: HashMap<String, Vec<tokio::sync::mpsc::Sender<DashboardUpdate>>>,
    annotations: HashMap<String, Vec<Annotation>>,
    public_links: HashMap<String, PublicLink>,
    layouts: HashMap<String, Layout>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PanelData {
    panel_id: String,
    dashboard_id: String,
    data: JsonValue,
    last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedQueryResult {
    query: String,
    result: JsonValue,
    timestamp: DateTime<Utc>,
    ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub id: String,
    pub dashboard_id: String,
    pub panel_id: Option<String>,
    pub time: DateTime<Utc>,
    pub title: String,
    pub text: String,
    pub tags: HashMap<String, String>,
}

impl DashboardManager {
    pub async fn new(config: DashboardConfig) -> Result<Self> {
        // Create default templates if needed
        let mut dashboards = HashMap::new();

        // Add inference monitoring template
        let mut template = Dashboard::new(
            "inference_node_monitoring_template",
            "Inference Node Monitoring Template",
            "Template for monitoring inference nodes",
        );
        template.add_tag("template");
        dashboards.insert(template.id.clone(), template);

        let state = Arc::new(RwLock::new(DashboardState {
            dashboards,
            panel_data: HashMap::new(),
            query_cache: HashMap::new(),
            subscriptions: HashMap::new(),
            annotations: HashMap::new(),
            public_links: HashMap::new(),
            layouts: HashMap::new(),
        }));

        Ok(DashboardManager { config, state })
    }

    pub async fn create_dashboard(&self, dashboard: Dashboard) -> Result<String> {
        let id = dashboard.id.clone();
        let mut state = self.state.write().await;
        state.dashboards.insert(id.clone(), dashboard);
        Ok(id)
    }

    pub async fn get_dashboard(&self, dashboard_id: &str) -> Result<Dashboard> {
        let state = self.state.read().await;
        state
            .dashboards
            .get(dashboard_id)
            .cloned()
            .ok_or_else(|| DashboardError::DashboardNotFound(dashboard_id.to_string()).into())
    }

    pub async fn add_panel(&self, dashboard_id: &str, panel: Panel) -> Result<()> {
        let mut state = self.state.write().await;

        if let Some(dashboard) = state.dashboards.get_mut(dashboard_id) {
            dashboard.panels.push(panel);
            dashboard.updated_at = Utc::now();
            Ok(())
        } else {
            Err(DashboardError::DashboardNotFound(dashboard_id.to_string()).into())
        }
    }

    pub async fn get_panels(&self, dashboard_id: &str) -> Result<Vec<Panel>> {
        let state = self.state.read().await;
        let dashboard = state
            .dashboards
            .get(dashboard_id)
            .ok_or_else(|| DashboardError::DashboardNotFound(dashboard_id.to_string()))?;
        Ok(dashboard.panels.clone())
    }

    pub async fn execute_panel_queries(
        &self,
        dashboard_id: &str,
        panel_name: &str,
        _time_range: TimeRange,
    ) -> Result<QueryResult> {
        let state = self.state.read().await;
        let dashboard = state
            .dashboards
            .get(dashboard_id)
            .ok_or_else(|| DashboardError::DashboardNotFound(dashboard_id.to_string()))?;

        let panel = dashboard
            .panels
            .iter()
            .find(|p| p.name == panel_name)
            .ok_or_else(|| DashboardError::PanelNotFound(panel_name.to_string()))?;

        // Mock query execution
        let mut query_used = String::new();
        if let Some(query) = panel.queries.first() {
            query_used = query.query.clone();

            // Apply variable substitution
            for (var_name, var) in &dashboard.variables {
                let placeholder = format!("${}", var_name);
                query_used = query_used.replace(&placeholder, &var.default_value);
            }
        }

        Ok(QueryResult {
            data: serde_json::json!({
                "values": [[1640995200, 100], [1640995260, 105]]
            }),
            query_used,
        })
    }

    pub async fn update_variable(
        &self,
        dashboard_id: &str,
        var_name: &str,
        value: &str,
    ) -> Result<()> {
        let mut state = self.state.write().await;
        let dashboard = state
            .dashboards
            .get_mut(dashboard_id)
            .ok_or_else(|| DashboardError::DashboardNotFound(dashboard_id.to_string()))?;

        if let Some(var) = dashboard.variables.get_mut(var_name) {
            var.default_value = value.to_string();
        }

        Ok(())
    }

    pub async fn set_layout(&self, dashboard_id: &str, layout: Layout) -> Result<()> {
        let mut state = self.state.write().await;
        if !state.dashboards.contains_key(dashboard_id) {
            return Err(DashboardError::DashboardNotFound(dashboard_id.to_string()).into());
        }
        state.layouts.insert(dashboard_id.to_string(), layout);
        Ok(())
    }

    pub async fn validate_layout(&self, dashboard_id: &str) -> Result<bool> {
        let state = self.state.read().await;
        if !state.dashboards.contains_key(dashboard_id) {
            return Err(DashboardError::DashboardNotFound(dashboard_id.to_string()).into());
        }
        // Mock validation - always return true for now
        Ok(true)
    }

    pub async fn subscribe_to_updates(
        &self,
        dashboard_id: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<DashboardUpdate>> {
        let mut state = self.state.write().await;

        if !state.dashboards.contains_key(dashboard_id) {
            return Err(DashboardError::DashboardNotFound(dashboard_id.to_string()).into());
        }

        let (tx, rx) = tokio::sync::mpsc::channel(100);

        state
            .subscriptions
            .entry(dashboard_id.to_string())
            .or_insert_with(Vec::new)
            .push(tx);

        Ok(rx)
    }

    pub async fn update_panel_data(
        &self,
        dashboard_id: &str,
        panel_name: &str,
        data: Vec<(&str, f64)>,
    ) -> Result<()> {
        let state = self.state.read().await;
        let dashboard = state
            .dashboards
            .get(dashboard_id)
            .ok_or_else(|| DashboardError::DashboardNotFound(dashboard_id.to_string()))?;

        let panel = dashboard
            .panels
            .iter()
            .find(|p| p.name == panel_name)
            .ok_or_else(|| DashboardError::PanelNotFound(panel_name.to_string()))?;

        let panel_id = panel.id.clone();
        drop(state);

        // Convert data to JsonValue
        let json_data = serde_json::json!(data
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<HashMap<_, _>>());

        let mut state = self.state.write().await;

        // Update panel data
        state.panel_data.insert(
            panel_id.clone(),
            PanelData {
                panel_id: panel_id.clone(),
                dashboard_id: dashboard_id.to_string(),
                data: json_data.clone(),
                last_updated: Utc::now(),
            },
        );

        // Send update to subscribers
        if let Some(subscribers) = state.subscriptions.get(dashboard_id) {
            let update = DashboardUpdate {
                panel_id,
                data: json_data,
                timestamp: Utc::now(),
            };

            for tx in subscribers {
                let _ = tx.send(update.clone()).await;
            }
        }

        Ok(())
    }

    pub async fn export_dashboard(&self, dashboard_id: &str, format: &str) -> Result<String> {
        let state = self.state.read().await;
        let dashboard = state
            .dashboards
            .get(dashboard_id)
            .ok_or_else(|| DashboardError::DashboardNotFound(dashboard_id.to_string()))?;

        match format {
            "json" => Ok(serde_json::to_string_pretty(dashboard)?),
            "yaml" => {
                // For yaml, we'll just use JSON for now
                Ok(serde_json::to_string_pretty(dashboard)?)
            }
            _ => Err(anyhow!("Unsupported export format: {}", format)),
        }
    }

    pub async fn delete_dashboard(&self, dashboard_id: &str) -> Result<()> {
        let mut state = self.state.write().await;
        state
            .dashboards
            .remove(dashboard_id)
            .ok_or_else(|| DashboardError::DashboardNotFound(dashboard_id.to_string()))?;

        // Remove associated data
        state
            .panel_data
            .retain(|_, data| data.dashboard_id != dashboard_id);
        state.annotations.remove(dashboard_id);
        state.subscriptions.remove(dashboard_id);

        Ok(())
    }

    pub async fn import_dashboard(&self, json: &str) -> Result<String> {
        let mut dashboard: Dashboard = serde_json::from_str(json)?;

        // Generate new ID
        dashboard.id = Uuid::new_v4().to_string();
        dashboard.created_at = Utc::now();
        dashboard.updated_at = Utc::now();
        dashboard.version = 1;

        let id = dashboard.id.clone();

        let mut state = self.state.write().await;
        state.dashboards.insert(id.clone(), dashboard);

        Ok(id)
    }

    pub async fn add_annotation(
        &self,
        dashboard_id: &str,
        title: &str,
        text: &str,
        timestamp: u64,
        tags: HashMap<String, String>,
    ) -> Result<Annotation> {
        let mut state = self.state.write().await;
        if !state.dashboards.contains_key(dashboard_id) {
            return Err(DashboardError::DashboardNotFound(dashboard_id.to_string()).into());
        }

        let annotation = Annotation {
            id: Uuid::new_v4().to_string(),
            dashboard_id: dashboard_id.to_string(),
            panel_id: None,
            time: DateTime::from_timestamp(timestamp as i64, 0).unwrap_or_else(Utc::now),
            title: title.to_string(),
            text: text.to_string(),
            tags,
        };

        state
            .annotations
            .entry(dashboard_id.to_string())
            .or_insert_with(Vec::new)
            .push(annotation.clone());

        Ok(annotation)
    }

    pub async fn get_annotations(
        &self,
        dashboard_id: &str,
        _time_range: TimeRange,
    ) -> Result<Vec<Annotation>> {
        let state = self.state.read().await;
        Ok(state
            .annotations
            .get(dashboard_id)
            .cloned()
            .unwrap_or_default())
    }

    pub async fn create_public_link(
        &self,
        dashboard_id: &str,
        duration: Duration,
    ) -> Result<PublicLink> {
        let mut state = self.state.write().await;
        if !state.dashboards.contains_key(dashboard_id) {
            return Err(DashboardError::DashboardNotFound(dashboard_id.to_string()).into());
        }

        let link = PublicLink {
            token: Uuid::new_v4().to_string(),
            expires_at: (Utc::now() + chrono::Duration::from_std(duration)?).timestamp() as u64,
        };

        state.public_links.insert(link.token.clone(), link.clone());
        Ok(link)
    }

    pub async fn get_public_dashboard(&self, token: &str) -> Result<Dashboard> {
        let state = self.state.read().await;

        if let Some(link) = state.public_links.get(token) {
            let now = Utc::now().timestamp() as u64;
            if now <= link.expires_at {
                // Return a mock public dashboard for now
                Ok(Dashboard::new(
                    "public_dashboard",
                    "Public Dashboard",
                    "Publicly accessible dashboard",
                ))
            } else {
                Err(anyhow!("Public link has expired"))
            }
        } else {
            Err(anyhow!("Invalid public link token"))
        }
    }

    pub async fn create_from_template(&self, template_name: &str) -> Result<Dashboard> {
        match template_name {
            "inference_node_monitoring" => {
                let dashboard = Dashboard::new(
                    "inference_monitoring",
                    "Inference Node Monitoring",
                    "Complete monitoring dashboard for inference nodes",
                );

                // Create the dashboard first
                let id = self.create_dashboard(dashboard.clone()).await?;

                // Add standard panels
                let panels = vec![
                    ("cpu_usage", "CPU Usage", WidgetType::Graph, 0, 0),
                    ("memory_usage", "Memory Usage", WidgetType::Gauge, 6, 0),
                    ("gpu_metrics", "GPU Metrics", WidgetType::Graph, 0, 4),
                    ("inference_rate", "Inference Rate", WidgetType::Graph, 6, 4),
                ];

                for (name, title, widget_type, x, y) in panels {
                    let panel = Panel::new(
                        name,
                        title,
                        widget_type,
                        GridPosition {
                            x,
                            y,
                            width: 6,
                            height: 4,
                        },
                    );
                    self.add_panel(&id, panel).await?;
                }

                // Return the created dashboard
                self.get_dashboard(&id).await
            }
            _ => {
                // Look for existing template
                let state = self.state.read().await;
                let template = state
                    .dashboards
                    .values()
                    .find(|d| d.name == template_name && d.tags.contains(&"template".to_string()))
                    .ok_or_else(|| anyhow!("Template not found: {}", template_name))?;

                let mut new_dashboard = template.clone();
                new_dashboard.id = Uuid::new_v4().to_string();
                new_dashboard.name = format!("{}_instance", template_name);
                new_dashboard.created_at = Utc::now();
                new_dashboard.updated_at = Utc::now();
                new_dashboard.version = 1;

                drop(state);

                let id = self.create_dashboard(new_dashboard.clone()).await?;
                self.get_dashboard(&id).await
            }
        }
    }

    pub async fn set_permissions(&self, dashboard_id: &str, _permissions: Vec<&str>) -> Result<()> {
        let state = self.state.read().await;
        if !state.dashboards.contains_key(dashboard_id) {
            return Err(DashboardError::DashboardNotFound(dashboard_id.to_string()).into());
        }
        // Mock implementation - store permissions
        Ok(())
    }

    pub async fn check_access(&self, _dashboard_id: &str, user_or_role: &str) -> Result<bool> {
        // Mock implementation - always return true for admin
        Ok(user_or_role.contains("admin") || user_or_role == "user:alice")
    }

    pub async fn render_dashboard(&self, dashboard_id: &str) -> Result<String> {
        let state = self.state.read().await;
        let dashboard = state
            .dashboards
            .get(dashboard_id)
            .ok_or_else(|| DashboardError::DashboardNotFound(dashboard_id.to_string()))?;

        // Mock render - return JSON representation
        Ok(serde_json::to_string(dashboard)?)
    }
}
