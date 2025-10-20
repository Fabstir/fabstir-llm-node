// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Connection statistics for a specific chain
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectionStats {
    pub total_connections: usize,
    pub active_connections: usize,
    pub messages_sent: usize,
    pub messages_received: usize,
    pub bytes_sent: usize,
    pub bytes_received: usize,
    pub errors: usize,
    pub last_activity: Option<u64>,
    pub average_message_size: usize,
    pub peak_connections: usize,
}

/// Detailed connection metrics
#[derive(Debug, Clone)]
struct ConnectionMetrics {
    connection_id: String,
    chain_id: u64,
    connected_at: Instant,
    last_activity: Instant,
    messages_sent: usize,
    messages_received: usize,
    bytes_sent: usize,
    bytes_received: usize,
    errors: usize,
}

impl ConnectionMetrics {
    fn new(connection_id: String, chain_id: u64) -> Self {
        let now = Instant::now();
        Self {
            connection_id,
            chain_id,
            connected_at: now,
            last_activity: now,
            messages_sent: 0,
            messages_received: 0,
            bytes_sent: 0,
            bytes_received: 0,
            errors: 0,
        }
    }

    fn record_sent(&mut self, bytes: usize) {
        self.messages_sent += 1;
        self.bytes_sent += bytes;
        self.last_activity = Instant::now();
    }

    fn record_received(&mut self, bytes: usize) {
        self.messages_received += 1;
        self.bytes_received += bytes;
        self.last_activity = Instant::now();
    }

    fn record_error(&mut self) {
        self.errors += 1;
        self.last_activity = Instant::now();
    }

    fn connection_duration(&self) -> Duration {
        self.connected_at.elapsed()
    }

    fn is_idle(&self, idle_threshold: Duration) -> bool {
        self.last_activity.elapsed() > idle_threshold
    }
}

/// Chain connection statistics tracker
pub struct ChainConnectionStats {
    // chain_id -> connection_id -> metrics
    connections: Arc<RwLock<HashMap<u64, HashMap<String, ConnectionMetrics>>>>,
    // chain_id -> aggregated stats
    chain_stats: Arc<RwLock<HashMap<u64, ConnectionStats>>>,
    // chain_id -> peak connections
    peak_connections: Arc<RwLock<HashMap<u64, usize>>>,
}

impl ChainConnectionStats {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            chain_stats: Arc::new(RwLock::new(HashMap::new())),
            peak_connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a new connection
    pub async fn record_connection(&self, chain_id: u64, connection_id: &str) {
        let mut connections = self.connections.write().await;
        let chain_connections = connections.entry(chain_id).or_insert_with(HashMap::new);

        chain_connections.insert(
            connection_id.to_string(),
            ConnectionMetrics::new(connection_id.to_string(), chain_id),
        );

        // Update peak connections
        let current_count = chain_connections.len();
        let mut peaks = self.peak_connections.write().await;
        let peak = peaks.entry(chain_id).or_insert(0);
        if current_count > *peak {
            *peak = current_count;
            info!(
                "New peak connections for chain {}: {}",
                chain_id, current_count
            );
        }

        // Update chain stats
        let mut stats = self.chain_stats.write().await;
        let chain_stat = stats
            .entry(chain_id)
            .or_insert_with(ConnectionStats::default);
        chain_stat.total_connections += 1;
        chain_stat.active_connections = current_count;
        chain_stat.peak_connections = *peak;
        chain_stat.last_activity = Some(chrono::Utc::now().timestamp() as u64);

        debug!(
            "Recorded connection {} for chain {}",
            connection_id, chain_id
        );
    }

    /// Record a disconnection
    pub async fn record_disconnection(&self, chain_id: u64, connection_id: &str) {
        let mut connections = self.connections.write().await;
        if let Some(chain_connections) = connections.get_mut(&chain_id) {
            if let Some(metrics) = chain_connections.remove(connection_id) {
                // Update aggregated stats with final metrics
                let mut stats = self.chain_stats.write().await;
                if let Some(chain_stat) = stats.get_mut(&chain_id) {
                    chain_stat.active_connections = chain_connections.len();
                    chain_stat.last_activity = Some(chrono::Utc::now().timestamp() as u64);

                    debug!(
                        "Connection {} on chain {} closed. Duration: {:?}, Messages: {}/{}, Bytes: {}/{}",
                        connection_id,
                        chain_id,
                        metrics.connection_duration(),
                        metrics.messages_sent,
                        metrics.messages_received,
                        metrics.bytes_sent,
                        metrics.bytes_received
                    );
                }
            }
        }
    }

    /// Record a message sent
    pub async fn record_message_sent(&self, chain_id: u64, connection_id: &str, bytes: usize) {
        let mut connections = self.connections.write().await;
        if let Some(chain_connections) = connections.get_mut(&chain_id) {
            if let Some(metrics) = chain_connections.get_mut(connection_id) {
                metrics.record_sent(bytes);
            }
        }

        // Update chain stats
        let mut stats = self.chain_stats.write().await;
        let chain_stat = stats
            .entry(chain_id)
            .or_insert_with(ConnectionStats::default);
        chain_stat.messages_sent += 1;
        chain_stat.bytes_sent += bytes;
        chain_stat.last_activity = Some(chrono::Utc::now().timestamp() as u64);

        // Update average message size
        if chain_stat.messages_sent > 0 {
            chain_stat.average_message_size = chain_stat.bytes_sent / chain_stat.messages_sent;
        }
    }

    /// Record a message received
    pub async fn record_message_received(&self, chain_id: u64, connection_id: &str, bytes: usize) {
        let mut connections = self.connections.write().await;
        if let Some(chain_connections) = connections.get_mut(&chain_id) {
            if let Some(metrics) = chain_connections.get_mut(connection_id) {
                metrics.record_received(bytes);
            }
        }

        // Update chain stats
        let mut stats = self.chain_stats.write().await;
        let chain_stat = stats
            .entry(chain_id)
            .or_insert_with(ConnectionStats::default);
        chain_stat.messages_received += 1;
        chain_stat.bytes_received += bytes;
        chain_stat.last_activity = Some(chrono::Utc::now().timestamp() as u64);
    }

    /// Record an error
    pub async fn record_error(&self, chain_id: u64, connection_id: &str, _error: &str) {
        let mut connections = self.connections.write().await;
        if let Some(chain_connections) = connections.get_mut(&chain_id) {
            if let Some(metrics) = chain_connections.get_mut(connection_id) {
                metrics.record_error();
            }
        }

        // Update chain stats
        let mut stats = self.chain_stats.write().await;
        let chain_stat = stats
            .entry(chain_id)
            .or_insert_with(ConnectionStats::default);
        chain_stat.errors += 1;
        chain_stat.last_activity = Some(chrono::Utc::now().timestamp() as u64);
    }

    /// Get statistics for a specific chain
    pub async fn get_chain_stats(&self, chain_id: u64) -> ConnectionStats {
        self.chain_stats
            .read()
            .await
            .get(&chain_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get statistics for all chains
    pub async fn get_all_stats(&self) -> HashMap<u64, ConnectionStats> {
        self.chain_stats.read().await.clone()
    }

    /// Get detailed metrics for all connections on a chain
    pub async fn get_chain_connections(&self, chain_id: u64) -> Vec<ConnectionInfo> {
        let connections = self.connections.read().await;
        if let Some(chain_connections) = connections.get(&chain_id) {
            chain_connections
                .values()
                .map(|metrics| ConnectionInfo {
                    connection_id: metrics.connection_id.clone(),
                    chain_id: metrics.chain_id,
                    duration_seconds: metrics.connection_duration().as_secs(),
                    messages_sent: metrics.messages_sent,
                    messages_received: metrics.messages_received,
                    bytes_sent: metrics.bytes_sent,
                    bytes_received: metrics.bytes_received,
                    errors: metrics.errors,
                    is_idle: metrics.is_idle(Duration::from_secs(60)),
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Clean up idle connections
    pub async fn cleanup_idle_connections(&self, idle_threshold: Duration) {
        let mut connections = self.connections.write().await;
        let mut stats = self.chain_stats.write().await;

        for (chain_id, chain_connections) in connections.iter_mut() {
            let initial_count = chain_connections.len();

            // Remove idle connections
            chain_connections.retain(|_, metrics| !metrics.is_idle(idle_threshold));

            let removed = initial_count - chain_connections.len();
            if removed > 0 {
                debug!(
                    "Cleaned up {} idle connections on chain {}",
                    removed, chain_id
                );

                // Update stats
                if let Some(chain_stat) = stats.get_mut(chain_id) {
                    chain_stat.active_connections = chain_connections.len();
                }
            }
        }
    }

    /// Reset statistics for a chain
    pub async fn reset_chain_stats(&self, chain_id: u64) {
        self.connections.write().await.remove(&chain_id);
        self.chain_stats.write().await.remove(&chain_id);
        self.peak_connections.write().await.remove(&chain_id);
        info!("Reset statistics for chain {}", chain_id);
    }

    /// Reset all statistics
    pub async fn reset_all_stats(&self) {
        self.connections.write().await.clear();
        self.chain_stats.write().await.clear();
        self.peak_connections.write().await.clear();
        info!("Reset all connection statistics");
    }
}

impl Default for ChainConnectionStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a single connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub connection_id: String,
    pub chain_id: u64,
    pub duration_seconds: u64,
    pub messages_sent: usize,
    pub messages_received: usize,
    pub bytes_sent: usize,
    pub bytes_received: usize,
    pub errors: usize,
    pub is_idle: bool,
}

/// Summary statistics across all chains
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalStats {
    pub total_chains: usize,
    pub total_connections: usize,
    pub total_messages_sent: usize,
    pub total_messages_received: usize,
    pub total_bytes_sent: usize,
    pub total_bytes_received: usize,
    pub total_errors: usize,
    pub chain_breakdown: HashMap<u64, ConnectionStats>,
}

impl GlobalStats {
    pub fn from_chain_stats(chain_stats: HashMap<u64, ConnectionStats>) -> Self {
        let mut total_connections = 0;
        let mut total_messages_sent = 0;
        let mut total_messages_received = 0;
        let mut total_bytes_sent = 0;
        let mut total_bytes_received = 0;
        let mut total_errors = 0;

        for stats in chain_stats.values() {
            total_connections += stats.active_connections;
            total_messages_sent += stats.messages_sent;
            total_messages_received += stats.messages_received;
            total_bytes_sent += stats.bytes_sent;
            total_bytes_received += stats.bytes_received;
            total_errors += stats.errors;
        }

        Self {
            total_chains: chain_stats.len(),
            total_connections,
            total_messages_sent,
            total_messages_received,
            total_bytes_sent,
            total_bytes_received,
            total_errors,
            chain_breakdown: chain_stats,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_tracking() {
        let stats = ChainConnectionStats::new();

        // Record connections for Base Sepolia
        stats.record_connection(84532, "conn1").await;
        stats.record_connection(84532, "conn2").await;

        // Record connection for opBNB
        stats.record_connection(5611, "conn3").await;

        // Check stats
        let base_stats = stats.get_chain_stats(84532).await;
        assert_eq!(base_stats.total_connections, 2);
        assert_eq!(base_stats.active_connections, 2);

        let opbnb_stats = stats.get_chain_stats(5611).await;
        assert_eq!(opbnb_stats.total_connections, 1);
        assert_eq!(opbnb_stats.active_connections, 1);

        // Disconnect one from Base
        stats.record_disconnection(84532, "conn1").await;

        let base_stats = stats.get_chain_stats(84532).await;
        assert_eq!(base_stats.active_connections, 1);
    }

    #[tokio::test]
    async fn test_message_tracking() {
        let stats = ChainConnectionStats::new();

        stats.record_connection(84532, "conn1").await;

        // Record messages
        stats.record_message_sent(84532, "conn1", 100).await;
        stats.record_message_sent(84532, "conn1", 200).await;
        stats.record_message_received(84532, "conn1", 150).await;

        let chain_stats = stats.get_chain_stats(84532).await;
        assert_eq!(chain_stats.messages_sent, 2);
        assert_eq!(chain_stats.bytes_sent, 300);
        assert_eq!(chain_stats.messages_received, 1);
        assert_eq!(chain_stats.bytes_received, 150);
        assert_eq!(chain_stats.average_message_size, 150); // 300/2
    }

    #[tokio::test]
    async fn test_error_tracking() {
        let stats = ChainConnectionStats::new();

        stats.record_connection(5611, "conn1").await;
        stats.record_error(5611, "conn1", "timeout").await;
        stats.record_error(5611, "conn1", "connection reset").await;

        let chain_stats = stats.get_chain_stats(5611).await;
        assert_eq!(chain_stats.errors, 2);
    }

    #[tokio::test]
    async fn test_global_stats() {
        let mut chain_stats = HashMap::new();

        chain_stats.insert(
            84532,
            ConnectionStats {
                active_connections: 10,
                messages_sent: 1000,
                bytes_sent: 100000,
                ..Default::default()
            },
        );

        chain_stats.insert(
            5611,
            ConnectionStats {
                active_connections: 5,
                messages_sent: 500,
                bytes_sent: 50000,
                ..Default::default()
            },
        );

        let global = GlobalStats::from_chain_stats(chain_stats);
        assert_eq!(global.total_chains, 2);
        assert_eq!(global.total_connections, 15);
        assert_eq!(global.total_messages_sent, 1500);
        assert_eq!(global.total_bytes_sent, 150000);
    }
}
