// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use ethers::types::{Address, U256};
use reqwest::Client;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::blockchain::multi_chain_registrar::RegistrationStatus;

/// Performs detailed health checks for node registrations
pub struct RegistrationHealthChecker {
    http_client: Client,
    check_timeout: Duration,
}

impl RegistrationHealthChecker {
    pub fn new() -> Self {
        RegistrationHealthChecker {
            http_client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            check_timeout: Duration::from_secs(30),
        }
    }

    /// Check if API endpoint is accessible
    pub async fn check_api_health(&self, api_url: &str) -> Result<bool> {
        debug!("Checking API health at: {}", api_url);

        let health_endpoint = format!("{}/health", api_url.trim_end_matches('/'));

        match self.http_client.get(&health_endpoint).send().await {
            Ok(response) => {
                let is_healthy = response.status().is_success();
                if !is_healthy {
                    warn!("API health check failed with status: {}", response.status());
                }
                Ok(is_healthy)
            }
            Err(e) => {
                warn!("API health check failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Validate FAB token balance
    pub async fn check_fab_balance(
        &self,
        balance: U256,
        required_stake: U256,
    ) -> Result<BalanceHealth> {
        let percentage = if required_stake > U256::zero() {
            (balance.as_u128() as f64 / required_stake.as_u128() as f64) * 100.0
        } else {
            0.0
        };

        Ok(BalanceHealth {
            current: balance,
            required: required_stake,
            is_sufficient: balance >= required_stake,
            percentage,
            warning_level: if percentage < 50.0 {
                WarningLevel::Critical
            } else if percentage < 100.0 {
                WarningLevel::Warning
            } else {
                WarningLevel::None
            },
        })
    }

    /// Check chain connectivity
    pub async fn check_chain_connectivity(&self, provider_url: &str) -> Result<ConnectivityHealth> {
        let start = Instant::now();

        // Try to get block number as connectivity test
        let client = reqwest::Client::new();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        let response = client.post(provider_url).json(&request).send().await;

        let latency = start.elapsed();

        match response {
            Ok(resp) if resp.status().is_success() => Ok(ConnectivityHealth {
                is_connected: true,
                latency,
                last_successful_check: Some(Instant::now()),
                consecutive_failures: 0,
            }),
            _ => Ok(ConnectivityHealth {
                is_connected: false,
                latency,
                last_successful_check: None,
                consecutive_failures: 1,
            }),
        }
    }

    /// Analyze registration status for issues
    pub fn analyze_registration_status(
        &self,
        status: &RegistrationStatus,
    ) -> Vec<RegistrationIssue> {
        let mut issues = Vec::new();

        match status {
            RegistrationStatus::NotRegistered => {
                issues.push(RegistrationIssue {
                    severity: IssueSeverity::Critical,
                    category: IssueCategory::Registration,
                    message: "Node is not registered".to_string(),
                    action_required: Some("Register the node to start accepting jobs".to_string()),
                });
            }
            RegistrationStatus::Failed { error } => {
                issues.push(RegistrationIssue {
                    severity: IssueSeverity::Critical,
                    category: IssueCategory::Registration,
                    message: format!("Registration failed: {}", error),
                    action_required: Some("Review error and retry registration".to_string()),
                });
            }
            RegistrationStatus::Pending { .. } => {
                issues.push(RegistrationIssue {
                    severity: IssueSeverity::Info,
                    category: IssueCategory::Registration,
                    message: "Registration is pending confirmation".to_string(),
                    action_required: None,
                });
            }
            RegistrationStatus::Confirmed { .. } => {
                // No issues for confirmed registration
            }
        }

        issues
    }

    /// Calculate time until expiry (if applicable)
    pub fn calculate_expiry_time(
        &self,
        registered_at_block: u64,
        current_block: u64,
        blocks_per_day: u64,
        expiry_days: u64,
    ) -> Option<Duration> {
        let blocks_since_registration = current_block.saturating_sub(registered_at_block);
        let expiry_blocks = blocks_per_day * expiry_days;

        if blocks_since_registration >= expiry_blocks {
            // Already expired
            Some(Duration::from_secs(0))
        } else {
            let blocks_remaining = expiry_blocks - blocks_since_registration;
            // Estimate 12 seconds per block (approximate)
            let seconds_remaining = blocks_remaining * 12;
            Some(Duration::from_secs(seconds_remaining))
        }
    }
}

/// Balance health information
#[derive(Debug, Clone)]
pub struct BalanceHealth {
    pub current: U256,
    pub required: U256,
    pub is_sufficient: bool,
    pub percentage: f64,
    pub warning_level: WarningLevel,
}

/// Connectivity health information
#[derive(Debug, Clone)]
pub struct ConnectivityHealth {
    pub is_connected: bool,
    pub latency: Duration,
    pub last_successful_check: Option<Instant>,
    pub consecutive_failures: u32,
}

/// Registration issue details
#[derive(Debug, Clone)]
pub struct RegistrationIssue {
    pub severity: IssueSeverity,
    pub category: IssueCategory,
    pub message: String,
    pub action_required: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueCategory {
    Registration,
    Balance,
    Connectivity,
    Configuration,
    Performance,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WarningLevel {
    None,
    Warning,
    Critical,
}

impl Default for RegistrationHealthChecker {
    fn default() -> Self {
        Self::new()
    }
}
