use fabstir_llm_node::api::websocket::{
    chain_connection_pool::{ChainConnectionPool, ChainConnectionConfig},
    chain_rate_limiter::{ChainRateLimiter, ChainRateLimitConfig},
    connection_stats::{ConnectionStats, ChainConnectionStats},
    health::{ChainHealthStatus, ReadinessCheck},
    connection::{ConnectionHandler, ConnectionState},
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use std::collections::HashMap;

#[tokio::test]
async fn test_connection_pool_per_chain() {
    // Test that separate connection pools are maintained per chain
    let config_base = ChainConnectionConfig {
        chain_id: 84532,
        max_connections: 100,
        rate_limit_per_minute: 600,
        burst_size: 100,
        health_check_interval: Duration::from_secs(30),
        connection_timeout: Duration::from_secs(5),
    };

    let config_opbnb = ChainConnectionConfig {
        chain_id: 5611,
        max_connections: 50,
        rate_limit_per_minute: 300,
        burst_size: 50,
        health_check_interval: Duration::from_secs(60),
        connection_timeout: Duration::from_secs(10),
    };

    let pool_manager = ChainConnectionPool::new();

    // Create pools for different chains
    pool_manager.add_chain_config(config_base).await;
    pool_manager.add_chain_config(config_opbnb).await;

    // Get or create pool for Base Sepolia
    let base_pool = pool_manager.get_or_create_pool(84532).await.unwrap();
    assert_eq!(base_pool.max_connections(), 100);

    // Get or create pool for opBNB
    let opbnb_pool = pool_manager.get_or_create_pool(5611).await.unwrap();
    assert_eq!(opbnb_pool.max_connections(), 50);

    // Verify pools are independent
    let base_conn = base_pool.acquire_connection("conn1").await.unwrap();
    let opbnb_conn = opbnb_pool.acquire_connection("conn2").await.unwrap();

    assert_ne!(base_conn.id(), opbnb_conn.id());
    assert_eq!(base_conn.chain_id(), 84532);
    assert_eq!(opbnb_conn.chain_id(), 5611);

    // Verify connection counts are tracked separately
    let base_stats = pool_manager.get_connection_stats(84532).await.unwrap();
    assert_eq!(base_stats.active_connections, 1);

    let opbnb_stats = pool_manager.get_connection_stats(5611).await.unwrap();
    assert_eq!(opbnb_stats.active_connections, 1);
}

#[tokio::test]
async fn test_connection_stats_by_chain() {
    // Test that connection statistics are tracked per chain
    let stats_tracker = ChainConnectionStats::new();

    // Track Base Sepolia connections
    stats_tracker.record_connection(84532, "conn1").await;
    stats_tracker.record_message_sent(84532, "conn1", 100).await;
    stats_tracker.record_message_received(84532, "conn1", 150).await;

    // Track opBNB connections
    stats_tracker.record_connection(5611, "conn2").await;
    stats_tracker.record_message_sent(5611, "conn2", 200).await;
    stats_tracker.record_message_received(5611, "conn2", 250).await;

    // Get stats for Base Sepolia
    let base_stats = stats_tracker.get_chain_stats(84532).await;
    assert_eq!(base_stats.total_connections, 1);
    assert_eq!(base_stats.messages_sent, 1);
    assert_eq!(base_stats.bytes_sent, 100);
    assert_eq!(base_stats.messages_received, 1);
    assert_eq!(base_stats.bytes_received, 150);

    // Get stats for opBNB
    let opbnb_stats = stats_tracker.get_chain_stats(5611).await;
    assert_eq!(opbnb_stats.total_connections, 1);
    assert_eq!(opbnb_stats.messages_sent, 1);
    assert_eq!(opbnb_stats.bytes_sent, 200);
    assert_eq!(opbnb_stats.messages_received, 1);
    assert_eq!(opbnb_stats.bytes_received, 250);

    // Get aggregate stats
    let all_stats = stats_tracker.get_all_stats().await;
    assert_eq!(all_stats.len(), 2);
    assert!(all_stats.contains_key(&84532));
    assert!(all_stats.contains_key(&5611));

    // Verify error tracking per chain
    stats_tracker.record_error(84532, "conn1", "timeout").await;
    let base_stats_with_error = stats_tracker.get_chain_stats(84532).await;
    assert_eq!(base_stats_with_error.errors, 1);

    // Verify opBNB errors tracked separately
    let opbnb_stats_no_error = stats_tracker.get_chain_stats(5611).await;
    assert_eq!(opbnb_stats_no_error.errors, 0);
}

#[tokio::test]
async fn test_rate_limiting_per_chain() {
    // Test that rate limits are enforced per chain
    let base_config = ChainRateLimitConfig {
        chain_id: 84532,
        requests_per_minute: 600,
        burst_size: 100,
        per_ip_limit: true,
        per_session_limit: false,
    };

    let opbnb_config = ChainRateLimitConfig {
        chain_id: 5611,
        requests_per_minute: 300,
        burst_size: 50,
        per_ip_limit: true,
        per_session_limit: false,
    };

    let rate_limiter = ChainRateLimiter::new();
    rate_limiter.add_chain_config(base_config).await;
    rate_limiter.add_chain_config(opbnb_config).await;

    let test_ip = "192.168.1.1";

    // Test Base Sepolia rate limit
    for i in 0..100 {
        let result = rate_limiter.check_rate_limit(84532, test_ip).await;
        assert!(result.is_ok(), "Request {} should succeed on Base", i);
    }

    // 101st request should fail (exceeds burst size of 100)
    let result = rate_limiter.check_rate_limit(84532, test_ip).await;
    assert!(result.is_err(), "Should hit rate limit on Base after burst");

    // Test opBNB rate limit (different limits)
    let test_ip_2 = "192.168.1.2";
    for i in 0..50 {
        let result = rate_limiter.check_rate_limit(5611, test_ip_2).await;
        assert!(result.is_ok(), "Request {} should succeed on opBNB", i);
    }

    // 51st request should fail (exceeds burst size of 50)
    let result = rate_limiter.check_rate_limit(5611, test_ip_2).await;
    assert!(result.is_err(), "Should hit rate limit on opBNB after burst");

    // Verify limits are independent between chains
    // Base still rate limited for test_ip
    let base_result = rate_limiter.check_rate_limit(84532, test_ip).await;
    assert!(base_result.is_err(), "Base should still be rate limited");

    // But opBNB should work for test_ip (different chain)
    let opbnb_result = rate_limiter.check_rate_limit(5611, test_ip).await;
    assert!(opbnb_result.is_ok(), "opBNB should work for same IP on different chain");

    // Test reset functionality per chain
    rate_limiter.reset_chain_limits(84532).await;
    let result_after_reset = rate_limiter.check_rate_limit(84532, test_ip).await;
    assert!(result_after_reset.is_ok(), "Should work after reset");
}

#[tokio::test]
async fn test_connection_health_check() {
    // Test health monitoring for each chain
    let health_monitor = ChainHealthMonitor::new();

    // Set initial health status
    health_monitor.set_chain_health(84532, ChainHealthStatus {
        chain_id: 84532,
        chain_name: "Base Sepolia".to_string(),
        is_healthy: true,
        rpc_responsive: true,
        last_block_time: chrono::Utc::now().timestamp() as u64,
        connection_count: 10,
        error_rate: 0.01,
        average_latency_ms: 50,
    }).await;

    health_monitor.set_chain_health(5611, ChainHealthStatus {
        chain_id: 5611,
        chain_name: "opBNB Testnet".to_string(),
        is_healthy: true,
        rpc_responsive: true,
        last_block_time: chrono::Utc::now().timestamp() as u64,
        connection_count: 5,
        error_rate: 0.02,
        average_latency_ms: 100,
    }).await;

    // Check individual chain health
    let base_health = health_monitor.get_chain_health(84532).await.unwrap();
    assert!(base_health.is_healthy);
    assert_eq!(base_health.connection_count, 10);
    assert_eq!(base_health.average_latency_ms, 50);

    let opbnb_health = health_monitor.get_chain_health(5611).await.unwrap();
    assert!(opbnb_health.is_healthy);
    assert_eq!(opbnb_health.connection_count, 5);
    assert_eq!(opbnb_health.average_latency_ms, 100);

    // Test overall readiness (all chains must be healthy)
    let overall_ready = health_monitor.is_all_chains_ready().await;
    assert!(overall_ready);

    // Simulate Base Sepolia RPC failure
    health_monitor.update_chain_health(84532, |health| {
        health.rpc_responsive = false;
        health.is_healthy = false;
    }).await;

    // Overall readiness should now be false
    let overall_ready_after = health_monitor.is_all_chains_ready().await;
    assert!(!overall_ready_after);

    // But opBNB should still be healthy
    let opbnb_still_healthy = health_monitor.get_chain_health(5611).await.unwrap();
    assert!(opbnb_still_healthy.is_healthy);

    // Test health check intervals
    let check_intervals = health_monitor.get_check_intervals().await;
    assert_eq!(check_intervals.get(&84532), Some(&Duration::from_secs(30)));
    assert_eq!(check_intervals.get(&5611), Some(&Duration::from_secs(60)));
}

#[tokio::test]
async fn test_chain_specific_disconnect() {
    // Test that disconnects are handled per chain
    let disconnect_handler = ChainDisconnectHandler::new();

    // Register disconnect callbacks for each chain
    let base_counter = Arc::new(RwLock::new(0));
    let base_counter_clone = base_counter.clone();
    disconnect_handler.register_chain_callback(84532, Box::new(move |conn_id| {
        let counter = base_counter_clone.clone();
        tokio::spawn(async move {
            let mut count = counter.write().await;
            *count += 1;
            println!("Base Sepolia disconnect for connection: {}", conn_id);
        });
    })).await;

    let opbnb_counter = Arc::new(RwLock::new(0));
    let opbnb_counter_clone = opbnb_counter.clone();
    disconnect_handler.register_chain_callback(5611, Box::new(move |conn_id| {
        let counter = opbnb_counter_clone.clone();
        tokio::spawn(async move {
            let mut count = counter.write().await;
            *count += 1;
            println!("opBNB disconnect for connection: {}", conn_id);
        });
    })).await;

    // Trigger disconnects on Base Sepolia
    disconnect_handler.handle_disconnect(84532, "base_conn_1").await;
    disconnect_handler.handle_disconnect(84532, "base_conn_2").await;

    // Trigger disconnect on opBNB
    disconnect_handler.handle_disconnect(5611, "opbnb_conn_1").await;

    // Wait for callbacks to execute
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify callbacks were called correctly
    assert_eq!(*base_counter.read().await, 2, "Base should have 2 disconnects");
    assert_eq!(*opbnb_counter.read().await, 1, "opBNB should have 1 disconnect");

    // Test disconnect with settlement trigger
    let settlement_triggered = Arc::new(RwLock::new(false));
    let settlement_clone = settlement_triggered.clone();

    disconnect_handler.register_settlement_trigger(84532, Box::new(move |conn_id| {
        let triggered = settlement_clone.clone();
        tokio::spawn(async move {
            *triggered.write().await = true;
            println!("Triggering settlement for connection: {}", conn_id);
        });
    })).await;

    // Disconnect with settlement
    disconnect_handler.handle_disconnect_with_settlement(84532, "base_conn_3").await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(*settlement_triggered.read().await, "Settlement should be triggered");

    // Test cleanup of chain-specific resources
    let resources_cleaned = Arc::new(RwLock::new(HashMap::new()));
    let resources_clone = resources_cleaned.clone();

    disconnect_handler.register_cleanup_callback(5611, Box::new(move |conn_id| {
        let resources = resources_clone.clone();
        tokio::spawn(async move {
            resources.write().await.insert(conn_id.to_string(), true);
        });
    })).await;

    disconnect_handler.handle_disconnect(5611, "opbnb_conn_2").await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let cleaned = resources_cleaned.read().await;
    assert!(cleaned.get("opbnb_conn_2").is_some(), "Resources should be cleaned");
}

// Helper structures for testing (these will be implemented in production code)

struct ChainHealthMonitor {
    health_status: Arc<RwLock<HashMap<u64, ChainHealthStatus>>>,
    check_intervals: Arc<RwLock<HashMap<u64, Duration>>>,
}

impl ChainHealthMonitor {
    fn new() -> Self {
        let mut intervals = HashMap::new();
        intervals.insert(84532, Duration::from_secs(30));
        intervals.insert(5611, Duration::from_secs(60));

        Self {
            health_status: Arc::new(RwLock::new(HashMap::new())),
            check_intervals: Arc::new(RwLock::new(intervals)),
        }
    }

    async fn set_chain_health(&self, chain_id: u64, health: ChainHealthStatus) {
        self.health_status.write().await.insert(chain_id, health);
    }

    async fn get_chain_health(&self, chain_id: u64) -> Option<ChainHealthStatus> {
        self.health_status.read().await.get(&chain_id).cloned()
    }

    async fn update_chain_health<F>(&self, chain_id: u64, update_fn: F)
    where
        F: FnOnce(&mut ChainHealthStatus),
    {
        let mut status = self.health_status.write().await;
        if let Some(health) = status.get_mut(&chain_id) {
            update_fn(health);
        }
    }

    async fn is_all_chains_ready(&self) -> bool {
        self.health_status
            .read()
            .await
            .values()
            .all(|h| h.is_healthy && h.rpc_responsive)
    }

    async fn get_check_intervals(&self) -> HashMap<u64, Duration> {
        self.check_intervals.read().await.clone()
    }
}

struct ChainDisconnectHandler {
    callbacks: Arc<RwLock<HashMap<u64, Vec<Box<dyn Fn(&str) + Send + Sync>>>>>,
    settlement_triggers: Arc<RwLock<HashMap<u64, Box<dyn Fn(&str) + Send + Sync>>>>,
    cleanup_callbacks: Arc<RwLock<HashMap<u64, Box<dyn Fn(&str) + Send + Sync>>>>,
}

impl ChainDisconnectHandler {
    fn new() -> Self {
        Self {
            callbacks: Arc::new(RwLock::new(HashMap::new())),
            settlement_triggers: Arc::new(RwLock::new(HashMap::new())),
            cleanup_callbacks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn register_chain_callback(&self, chain_id: u64, callback: Box<dyn Fn(&str) + Send + Sync>) {
        let mut callbacks = self.callbacks.write().await;
        callbacks.entry(chain_id).or_insert_with(Vec::new).push(callback);
    }

    async fn register_settlement_trigger(&self, chain_id: u64, trigger: Box<dyn Fn(&str) + Send + Sync>) {
        self.settlement_triggers.write().await.insert(chain_id, trigger);
    }

    async fn register_cleanup_callback(&self, chain_id: u64, cleanup: Box<dyn Fn(&str) + Send + Sync>) {
        self.cleanup_callbacks.write().await.insert(chain_id, cleanup);
    }

    async fn handle_disconnect(&self, chain_id: u64, conn_id: &str) {
        if let Some(callbacks) = self.callbacks.read().await.get(&chain_id) {
            for callback in callbacks {
                callback(conn_id);
            }
        }

        if let Some(cleanup) = self.cleanup_callbacks.read().await.get(&chain_id) {
            cleanup(conn_id);
        }
    }

    async fn handle_disconnect_with_settlement(&self, chain_id: u64, conn_id: &str) {
        self.handle_disconnect(chain_id, conn_id).await;

        if let Some(trigger) = self.settlement_triggers.read().await.get(&chain_id) {
            trigger(conn_id);
        }
    }
}