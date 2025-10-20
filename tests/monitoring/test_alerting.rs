// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// tests/monitoring/test_alerting.rs

use anyhow::Result;
use fabstir_llm_node::monitoring::{
    Alert, AlertChannel, AlertCondition, AlertConfig, AlertGroup, AlertHistory, AlertLevel,
    AlertManager, AlertRule, AlertStatus, NotificationChannel, RateAlert, ThresholdAlert,
    WebhookChannel,
};
use std::collections::HashMap;
use std::time::Duration;
use tokio;

async fn create_test_alert_manager() -> Result<AlertManager> {
    let config = AlertConfig {
        enable_alerts: true,
        evaluation_interval_seconds: 1,
        alert_history_retention_days: 7,
        notification_channels: vec![
            NotificationChannel::Log,
            NotificationChannel::Webhook {
                url: "http://localhost:8080/alerts".to_string(),
                headers: HashMap::new(),
            },
        ],
        silence_duration_minutes: 5,
        group_wait_seconds: 10,
        group_interval_seconds: 60,
        repeat_interval_minutes: 30,
    };

    AlertManager::new(config).await
}

#[tokio::test]
async fn test_basic_alert_creation() {
    let manager = create_test_alert_manager().await.unwrap();

    // Create a simple threshold alert
    let rule = AlertRule::new(
        "high_cpu_usage",
        "CPU usage is too high",
        AlertCondition::Threshold {
            metric: "cpu_usage_percent".to_string(),
            operator: ">".to_string(),
            value: 80.0,
            duration: Duration::from_secs(60),
        },
        AlertLevel::Warning,
    );

    manager.add_rule(rule).await.unwrap();

    // Verify rule was added
    let rules = manager.list_rules().await;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].name, "high_cpu_usage");
}

#[tokio::test]
async fn test_alert_triggering() {
    let manager = create_test_alert_manager().await.unwrap();

    // Add threshold rule
    let rule = ThresholdAlert::new(
        "memory_alert",
        "memory_usage_percent",
        85.0,
        AlertLevel::Critical,
    );

    manager.add_threshold_alert(rule).await.unwrap();

    // Simulate metric exceeding threshold
    manager.update_metric("memory_usage_percent", 90.0).await;

    // Evaluate alerts
    manager.evaluate_alerts().await.unwrap();

    // Check active alerts
    let active = manager.get_active_alerts().await;
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].rule_name, "memory_alert");
    assert_eq!(active[0].level, AlertLevel::Critical);
}

#[tokio::test]
async fn test_alert_resolution() {
    let manager = create_test_alert_manager().await.unwrap();

    let rule = ThresholdAlert::new(
        "disk_space",
        "disk_usage_percent",
        90.0,
        AlertLevel::Warning,
    );

    manager.add_threshold_alert(rule).await.unwrap();

    // Trigger alert
    manager.update_metric("disk_usage_percent", 95.0).await;
    manager.evaluate_alerts().await.unwrap();

    let active = manager.get_active_alerts().await;
    assert_eq!(active.len(), 1);
    let alert_id = active[0].id.clone();

    // Resolve by reducing metric
    manager.update_metric("disk_usage_percent", 80.0).await;
    manager.evaluate_alerts().await.unwrap();

    // Alert should be resolved
    let active = manager.get_active_alerts().await;
    assert_eq!(active.len(), 0);

    // Check history
    let history = manager.get_alert_history(Duration::from_secs(300)).await;
    let resolved = history.iter().find(|a| a.id == alert_id).unwrap();
    assert_eq!(resolved.status, AlertStatus::Resolved);
}

#[tokio::test]
async fn test_rate_based_alerts() {
    let manager = create_test_alert_manager().await.unwrap();

    // Alert on request rate
    let rule = RateAlert::new(
        "high_error_rate",
        "http_errors_total",
        100.0, // 100 errors per minute
        Duration::from_secs(60),
        AlertLevel::Critical,
    );

    manager.add_rate_alert(rule).await.unwrap();

    // Simulate rapid errors
    for i in 0..150 {
        manager.increment_counter("http_errors_total", 1.0).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    manager.evaluate_alerts().await.unwrap();

    let active = manager.get_active_alerts().await;
    assert!(active.iter().any(|a| a.rule_name == "high_error_rate"));
}

#[tokio::test]
async fn test_alert_grouping() {
    let manager = create_test_alert_manager().await.unwrap();

    // Create multiple related alerts
    for i in 0..5 {
        let rule = ThresholdAlert::new(
            &format!("service_{}_down", i),
            &format!("service_{}_health", i),
            0.5,
            AlertLevel::Critical,
        )
        .with_labels(vec![("service_group", "backend")]);

        manager.add_threshold_alert(rule).await.unwrap();
        manager
            .update_metric(&format!("service_{}_health", i), 0.0)
            .await;
    }

    manager.evaluate_alerts().await.unwrap();

    // Alerts should be grouped
    let groups = manager.get_groups().await;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, "service_group=backend");
    assert_eq!(groups[0].alerts.len(), 5);
}

#[tokio::test]
async fn test_alert_silencing() {
    let manager = create_test_alert_manager().await.unwrap();

    let rule = ThresholdAlert::new("noisy_alert", "flaky_metric", 50.0, AlertLevel::Info);

    manager.add_threshold_alert(rule).await.unwrap();

    // Trigger alert
    manager.update_metric("flaky_metric", 60.0).await;
    manager.evaluate_alerts().await.unwrap();

    let active = manager.get_active_alerts().await;
    assert_eq!(active.len(), 1);
    let alert_id = active[0].id.clone();

    // Silence the alert
    manager
        .silence_alert(&alert_id, Duration::from_secs(300), "Testing silence")
        .await
        .unwrap();

    // Alert should not appear in active list
    let active = manager.get_active_alerts().await;
    assert_eq!(active.len(), 0);

    // But should be in silenced list
    let silenced = manager.list_silences().await;
    assert!(!silenced.is_empty());
}

#[tokio::test]
async fn test_notification_channels() {
    let manager = create_test_alert_manager().await.unwrap();

    // Add webhook channel with mock server
    let webhook = NotificationChannel::Webhook {
        url: "http://localhost:9999/webhook".to_string(),
        headers: HashMap::from([("Authorization".to_string(), "Bearer test-token".to_string())]),
    };

    manager.add_notification_channel(webhook).await.unwrap();

    // Create and trigger alert
    let rule = ThresholdAlert::new(
        "test_notification",
        "test_metric",
        10.0,
        AlertLevel::Warning,
    );

    manager.add_threshold_alert(rule).await.unwrap();
    manager.update_metric("test_metric", 20.0).await;
    manager.evaluate_alerts().await.unwrap();

    // Check notification was attempted
    let notifications = manager.get_notification_history().await;
    assert!(notifications.iter().any(|n| n.channel.contains("webhook")));
}

#[tokio::test]
async fn test_alert_templates() {
    let manager = create_test_alert_manager().await.unwrap();

    // Create alert with custom template
    let mut rule = ThresholdAlert::new(
        "custom_alert",
        "important_metric",
        100.0,
        AlertLevel::Critical,
    );

    rule.set_template(
        "Alert: {{ .RuleName }} - Metric {{ .MetricName }} is at {{ .Value }} (threshold: {{ .Threshold }})"
    );

    manager.add_threshold_alert(rule).await.unwrap();

    // Trigger and check formatted message
    manager.update_metric("important_metric", 150.0).await;
    manager.evaluate_alerts().await.unwrap();

    let active = manager.get_active_alerts().await;
    assert!(active[0].message.contains("150"));
    assert!(active[0].message.contains("important_metric"));
}

#[tokio::test]
async fn test_alert_dependencies() {
    let manager = create_test_alert_manager().await.unwrap();

    // Create parent alert
    let parent = ThresholdAlert::new(
        "database_down",
        "database_health",
        0.5,
        AlertLevel::Critical,
    );
    manager.add_threshold_alert(parent).await.unwrap();

    // Create dependent alert
    let mut child = ThresholdAlert::new("api_errors", "api_error_rate", 50.0, AlertLevel::Warning);
    child.depends_on("database_down");
    manager.add_threshold_alert(child).await.unwrap();

    // Trigger both conditions
    manager.update_metric("database_health", 0.0).await;
    manager.update_metric("api_error_rate", 100.0).await;
    manager.evaluate_alerts().await.unwrap();

    // Only parent should be active (child suppressed)
    let active = manager.get_active_alerts().await;
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].rule_name, "database_down");
}

#[tokio::test]
async fn test_alert_escalation() {
    let manager = create_test_alert_manager().await.unwrap();

    // Create escalating alert
    let mut rule = ThresholdAlert::new(
        "escalating_issue",
        "problem_metric",
        50.0,
        AlertLevel::Warning,
    );

    rule.add_escalation(
        Duration::from_secs(300), // 5 minutes
        AlertLevel::Critical,
    );

    manager.add_threshold_alert(rule).await.unwrap();

    // Trigger alert
    manager.update_metric("problem_metric", 60.0).await;
    manager.evaluate_alerts().await.unwrap();

    // Initially warning
    let active = manager.get_active_alerts().await;
    assert_eq!(active[0].level, AlertLevel::Warning);

    // Simulate time passing (in real implementation)
    // After escalation period, level should increase
}

#[tokio::test]
async fn test_alert_analytics() {
    let manager = create_test_alert_manager().await.unwrap();

    // Generate some alert history
    let rule = ThresholdAlert::new("test_alert", "test_metric", 50.0, AlertLevel::Warning);
    manager.add_threshold_alert(rule).await.unwrap();

    // Trigger and resolve multiple times
    for i in 0..10 {
        manager
            .update_metric("test_metric", if i % 2 == 0 { 60.0 } else { 40.0 })
            .await;
        manager.evaluate_alerts().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Get analytics
    let analytics = manager.get_alert_analytics(Duration::from_secs(3600)).await;

    assert!(analytics.total_alerts > 0);
    assert!(analytics.alerts_by_level.contains_key(&AlertLevel::Warning));
    assert!(analytics.mean_time_to_resolve > Duration::from_secs(0));
    assert!(analytics.most_frequent_alerts.len() > 0);
}

#[tokio::test]
async fn test_complex_alert_conditions() {
    let manager = create_test_alert_manager().await.unwrap();

    // Create composite alert condition
    let condition = AlertCondition::And(vec![
        Box::new(AlertCondition::Threshold {
            metric: "cpu_usage".to_string(),
            operator: ">".to_string(),
            value: 80.0,
            duration: Duration::from_secs(60),
        }),
        Box::new(AlertCondition::Threshold {
            metric: "memory_usage".to_string(),
            operator: ">".to_string(),
            value: 90.0,
            duration: Duration::from_secs(60),
        }),
    ]);

    let rule = AlertRule::new(
        "resource_pressure",
        "High CPU and Memory usage",
        condition,
        AlertLevel::Critical,
    );

    manager.add_rule(rule).await.unwrap();

    // Only one condition met
    manager.update_metric("cpu_usage", 85.0).await;
    manager.update_metric("memory_usage", 70.0).await;
    manager.evaluate_alerts().await.unwrap();

    assert_eq!(manager.get_active_alerts().await.len(), 0);

    // Both conditions met
    manager.update_metric("memory_usage", 95.0).await;
    manager.evaluate_alerts().await.unwrap();

    assert_eq!(manager.get_active_alerts().await.len(), 1);
}

#[tokio::test]
async fn test_alert_recovery_actions() {
    let manager = create_test_alert_manager().await.unwrap();

    // Create alert with recovery action
    let mut rule = ThresholdAlert::new(
        "service_health",
        "service_health_score",
        0.5,
        AlertLevel::Critical,
    );

    rule.add_recovery_action(Box::new(|| {
        Box::pin(async {
            // Simulate service restart
            println!("Executing recovery: Restarting service...");
            Ok(())
        })
    }));

    manager.add_threshold_alert(rule).await.unwrap();

    // Trigger alert
    manager.update_metric("service_health_score", 0.2).await;
    manager.evaluate_alerts().await.unwrap();

    // Recovery action should be executed
    let history = manager.get_action_history().await;
    assert!(history.iter().any(|a| a.action_type == "recovery"));
}
