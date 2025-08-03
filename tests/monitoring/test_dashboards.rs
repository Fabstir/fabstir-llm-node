// tests/monitoring/test_dashboards.rs

use anyhow::Result;
use fabstir_llm_node::monitoring::{
    DashboardManager, DashboardConfig, Dashboard, Panel, Widget,
    WidgetType, DataSource, TimeRange, RefreshInterval,
    Layout, GridPosition, Query, Visualization, DashboardExport
};
use std::time::Duration;
use std::collections::HashMap;
use tokio;

async fn create_test_dashboard_manager() -> Result<DashboardManager> {
    let config = DashboardConfig {
        enable_dashboards: true,
        default_refresh_seconds: 30,
        max_dashboards_per_user: 10,
        enable_public_dashboards: false,
        storage_backend: "local".to_string(),
        export_formats: vec!["json".to_string(), "yaml".to_string()],
        cache_ttl_seconds: 60,
        enable_annotations: true,
    };
    
    DashboardManager::new(config).await
}

#[tokio::test]
async fn test_dashboard_creation() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    // Create a new dashboard
    let dashboard = Dashboard::new(
        "inference_overview",
        "Inference Node Overview",
        "Monitor inference node performance and health",
    );
    
    let id = manager.create_dashboard(dashboard).await.unwrap();
    
    assert!(!id.is_empty());
    
    // Retrieve dashboard
    let retrieved = manager.get_dashboard(&id).await.unwrap();
    assert_eq!(retrieved.name, "inference_overview");
    assert_eq!(retrieved.title, "Inference Node Overview");
}

#[tokio::test]
async fn test_panel_management() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    let dashboard = Dashboard::new("test_dash", "Test Dashboard", "Test");
    let dash_id = manager.create_dashboard(dashboard).await.unwrap();
    
    // Add panels
    let cpu_panel = Panel::new(
        "cpu_usage",
        "CPU Usage",
        WidgetType::Graph,
        GridPosition { x: 0, y: 0, width: 6, height: 4 },
    );
    
    let memory_panel = Panel::new(
        "memory_usage",
        "Memory Usage",
        WidgetType::Gauge,
        GridPosition { x: 6, y: 0, width: 6, height: 4 },
    );
    
    manager.add_panel(&dash_id, cpu_panel).await.unwrap();
    manager.add_panel(&dash_id, memory_panel).await.unwrap();
    
    // Get panels
    let panels = manager.get_panels(&dash_id).await.unwrap();
    assert_eq!(panels.len(), 2);
    assert!(panels.iter().any(|p| p.name == "cpu_usage"));
    assert!(panels.iter().any(|p| p.name == "memory_usage"));
}

#[tokio::test]
async fn test_widget_types() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    let dashboard = Dashboard::new("widgets", "Widget Test", "Testing widgets");
    let dash_id = manager.create_dashboard(dashboard).await.unwrap();
    
    // Test different widget types
    let widgets = vec![
        ("graph", WidgetType::Graph),
        ("gauge", WidgetType::Gauge),
        ("table", WidgetType::Table),
        ("heatmap", WidgetType::Heatmap),
        ("stat", WidgetType::SingleStat),
        ("bar", WidgetType::BarChart),
        ("pie", WidgetType::PieChart),
        ("logs", WidgetType::LogViewer),
    ];
    
    for (i, (name, widget_type)) in widgets.iter().enumerate() {
        let panel = Panel::new(
            name,
            &format!("{} Widget", name),
            widget_type.clone(),
            GridPosition { 
                x: (i % 4) as i32 * 3, 
                y: (i / 4) as i32 * 4, 
                width: 3, 
                height: 4 
            },
        );
        
        manager.add_panel(&dash_id, panel).await.unwrap();
    }
    
    let panels = manager.get_panels(&dash_id).await.unwrap();
    assert_eq!(panels.len(), widgets.len());
}

#[tokio::test]
async fn test_data_queries() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    let dashboard = Dashboard::new("queries", "Query Test", "Testing queries");
    let dash_id = manager.create_dashboard(dashboard).await.unwrap();
    
    // Create panel with query
    let mut panel = Panel::new(
        "inference_rate",
        "Inference Rate",
        WidgetType::Graph,
        GridPosition { x: 0, y: 0, width: 12, height: 6 },
    );
    
    // Add query
    let query = Query::new(
        "prometheus",
        "rate(inference_requests_total[5m])",
    )
    .with_legend("Requests/sec")
    .with_aggregation("avg");
    
    panel.add_query(query);
    
    manager.add_panel(&dash_id, panel).await.unwrap();
    
    // Execute queries for panel
    let data = manager
        .execute_panel_queries(&dash_id, "inference_rate", TimeRange::LastHour)
        .await
        .unwrap();
    
    assert!(!data.is_empty());
}

#[tokio::test]
async fn test_dashboard_variables() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    let mut dashboard = Dashboard::new("vars", "Variable Test", "Testing variables");
    
    // Add dashboard variables
    dashboard.add_variable("gpu_id", vec!["0", "1", "2", "3"], "0");
    dashboard.add_variable("model", vec!["llama", "mistral", "gpt"], "llama");
    
    let dash_id = manager.create_dashboard(dashboard).await.unwrap();
    
    // Create panel using variables
    let mut panel = Panel::new(
        "gpu_memory",
        "GPU Memory Usage",
        WidgetType::Graph,
        GridPosition { x: 0, y: 0, width: 12, height: 6 },
    );
    
    let query = Query::new(
        "prometheus",
        "gpu_memory_used{gpu_id=\"$gpu_id\", model=\"$model\"}",
    );
    panel.add_query(query);
    
    manager.add_panel(&dash_id, panel).await.unwrap();
    
    // Update variable value
    manager.update_variable(&dash_id, "gpu_id", "2").await.unwrap();
    
    // Query should use updated variable
    let data = manager
        .execute_panel_queries(&dash_id, "gpu_memory", TimeRange::LastHour)
        .await
        .unwrap();
    
    // Verify variable substitution happened
    assert!(data.query_used.contains("gpu_id=\"2\""));
}

#[tokio::test]
async fn test_dashboard_layouts() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    let dashboard = Dashboard::new("layout", "Layout Test", "Testing layouts");
    let dash_id = manager.create_dashboard(dashboard).await.unwrap();
    
    // Create responsive layout
    let layout = Layout::new()
        .with_breakpoint("lg", 1200)
        .with_breakpoint("md", 996)
        .with_breakpoint("sm", 768)
        .with_columns(12);
    
    manager.set_layout(&dash_id, layout).await.unwrap();
    
    // Add panels with responsive positions
    let panel = Panel::new(
        "responsive",
        "Responsive Panel",
        WidgetType::Stat,
        GridPosition { x: 0, y: 0, width: 6, height: 4 },
    )
    .with_responsive_position("sm", GridPosition { x: 0, y: 0, width: 12, height: 4 });
    
    manager.add_panel(&dash_id, panel).await.unwrap();
    
    // Validate layout
    let valid = manager.validate_layout(&dash_id).await.unwrap();
    assert!(valid);
}

#[tokio::test]
async fn test_real_time_updates() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    let dashboard = Dashboard::new("realtime", "Real-time Test", "Testing updates")
        .with_refresh_interval(RefreshInterval::Seconds(1));
    
    let dash_id = manager.create_dashboard(dashboard).await.unwrap();
    
    // Subscribe to updates
    let mut updates = manager.subscribe_to_updates(&dash_id).await.unwrap();
    
    // Add panel that updates
    let panel = Panel::new(
        "live_metrics",
        "Live Metrics",
        WidgetType::Graph,
        GridPosition { x: 0, y: 0, width: 12, height: 6 },
    );
    
    manager.add_panel(&dash_id, panel).await.unwrap();
    
    // Simulate metric updates
    tokio::spawn(async move {
        for i in 0..5 {
            manager.update_panel_data(&dash_id, "live_metrics", vec![
                ("time", chrono::Utc::now().timestamp() as f64),
                ("value", i as f64 * 10.0),
            ]).await.unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    
    // Receive updates
    let mut received = 0;
    while let Ok(Some(update)) = tokio::time::timeout(
        Duration::from_secs(2),
        updates.recv(),
    ).await {
        received += 1;
        if received >= 3 {
            break;
        }
    }
    
    assert!(received >= 3);
}

#[tokio::test]
async fn test_dashboard_export_import() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    // Create dashboard with panels
    let mut dashboard = Dashboard::new("export_test", "Export Test", "Testing export");
    dashboard.add_tag("test");
    dashboard.add_tag("export");
    
    let dash_id = manager.create_dashboard(dashboard).await.unwrap();
    
    let panel = Panel::new(
        "test_panel",
        "Test Panel",
        WidgetType::Graph,
        GridPosition { x: 0, y: 0, width: 12, height: 6 },
    );
    manager.add_panel(&dash_id, panel).await.unwrap();
    
    // Export dashboard
    let export = manager.export_dashboard(&dash_id, "json").await.unwrap();
    
    assert!(export.contains("export_test"));
    assert!(export.contains("test_panel"));
    
    // Delete original
    manager.delete_dashboard(&dash_id).await.unwrap();
    
    // Import dashboard
    let imported_id = manager.import_dashboard(&export).await.unwrap();
    
    // Verify imported dashboard
    let imported = manager.get_dashboard(&imported_id).await.unwrap();
    assert_eq!(imported.name, "export_test");
    
    let panels = manager.get_panels(&imported_id).await.unwrap();
    assert_eq!(panels.len(), 1);
}

#[tokio::test]
async fn test_dashboard_annotations() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    let dashboard = Dashboard::new("annotated", "Annotated Dashboard", "With annotations");
    let dash_id = manager.create_dashboard(dashboard).await.unwrap();
    
    // Add annotation
    let annotation = manager
        .add_annotation(
            &dash_id,
            "Deployment",
            "New model deployed",
            chrono::Utc::now().timestamp() as u64,
            HashMap::from([
                ("model".to_string(), "llama-3.1".to_string()),
                ("version".to_string(), "1.0.0".to_string()),
            ]),
        )
        .await
        .unwrap();
    
    // Query annotations
    let annotations = manager
        .get_annotations(&dash_id, TimeRange::LastDay)
        .await
        .unwrap();
    
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].title, "Deployment");
    assert_eq!(annotations[0].tags.get("model").unwrap(), "llama-3.1");
}

#[tokio::test]
async fn test_dashboard_sharing() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    let dashboard = Dashboard::new("shared", "Shared Dashboard", "For sharing");
    let dash_id = manager.create_dashboard(dashboard).await.unwrap();
    
    // Create public link
    let public_link = manager
        .create_public_link(&dash_id, Duration::from_secs(3600))
        .await
        .unwrap();
    
    assert!(!public_link.token.is_empty());
    assert!(public_link.expires_at > chrono::Utc::now().timestamp() as u64);
    
    // Access via public link
    let public_dash = manager.get_public_dashboard(&public_link.token).await;
    assert!(public_dash.is_ok());
}

#[tokio::test]
async fn test_dashboard_templates() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    // Use predefined template
    let dashboard = manager
        .create_from_template("inference_node_monitoring")
        .await
        .unwrap();
    
    // Template should include standard panels
    let panels = manager.get_panels(&dashboard.id).await.unwrap();
    
    let expected_panels = vec!["cpu_usage", "memory_usage", "gpu_metrics", "inference_rate"];
    for panel_name in expected_panels {
        assert!(panels.iter().any(|p| p.name == panel_name));
    }
}

#[tokio::test]
async fn test_dashboard_access_control() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    let dashboard = Dashboard::new("private", "Private Dashboard", "Restricted access");
    let dash_id = manager.create_dashboard(dashboard).await.unwrap();
    
    // Set permissions
    manager
        .set_permissions(&dash_id, vec!["user:alice", "role:admin"])
        .await
        .unwrap();
    
    // Check access
    assert!(manager.check_access(&dash_id, "user:alice").await.unwrap());
    assert!(manager.check_access(&dash_id, "role:admin").await.unwrap());
    assert!(!manager.check_access(&dash_id, "user:bob").await.unwrap());
}

#[tokio::test]
async fn test_dashboard_performance() {
    let manager = create_test_dashboard_manager().await.unwrap();
    
    let dashboard = Dashboard::new("perf", "Performance Test", "Many panels");
    let dash_id = manager.create_dashboard(dashboard).await.unwrap();
    
    // Add many panels
    for i in 0..20 {
        let panel = Panel::new(
            &format!("panel_{}", i),
            &format!("Panel {}", i),
            WidgetType::Stat,
            GridPosition { 
                x: (i % 4) * 3, 
                y: (i / 4) * 4, 
                width: 3, 
                height: 4 
            },
        );
        manager.add_panel(&dash_id, panel).await.unwrap();
    }
    
    // Measure query performance
    let start = std::time::Instant::now();
    let _ = manager.render_dashboard(&dash_id).await.unwrap();
    let elapsed = start.elapsed();
    
    // Should render quickly even with many panels
    assert!(elapsed.as_millis() < 1000);
}