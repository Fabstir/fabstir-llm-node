use super::manager::SettlementManager;
use super::types::{SettlementError, SettlementRequest, SettlementStatus};
use crate::api::websocket::session_store::SessionStore;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_retries: u8,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub exponential_base: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            exponential_base: 2.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementConfig {
    pub retry_config: RetryConfig,
    pub enable_auto_settlement: bool,
    pub settlement_timeout: Duration,
    pub concurrent_settlements: usize,
}

impl Default for SettlementConfig {
    fn default() -> Self {
        Self {
            retry_config: RetryConfig::default(),
            enable_auto_settlement: true,
            settlement_timeout: Duration::from_secs(30),
            concurrent_settlements: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub enum EventType {
    SettlementInitiated,
    SettlementQueued,
    SettlementProcessing,
    SettlementCompleted,
    SettlementFailed,
    RetryAttempt,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettlementEvent {
    pub session_id: String,
    pub event_type: EventType,
    pub chain_id: Option<u64>,
    pub timestamp: DateTime<Utc>,
    pub details: String,
}

pub struct AutoSettlement {
    settlement_manager: Arc<SettlementManager>,
    session_store: Arc<RwLock<SessionStore>>,
    config: SettlementConfig,
    retry_counts: Arc<RwLock<HashMap<String, u8>>>,
    event_tracking: Arc<RwLock<bool>>,
    events: Arc<RwLock<HashMap<String, Vec<SettlementEvent>>>>,
}

impl AutoSettlement {
    pub fn new(
        settlement_manager: Arc<SettlementManager>,
        session_store: Arc<RwLock<SessionStore>>,
        config: SettlementConfig,
    ) -> Self {
        Self {
            settlement_manager,
            session_store,
            config,
            retry_counts: Arc::new(RwLock::new(HashMap::new())),
            event_tracking: Arc::new(RwLock::new(false)),
            events: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Handle WebSocket disconnect and trigger settlement
    pub async fn handle_disconnect(&self, session_id: &str) -> Result<(), SettlementError> {
        info!("[AUTO-SETTLEMENT] 🔌 Started handling disconnect for session: {}", session_id);

        self.log_event(
            session_id,
            EventType::SettlementInitiated,
            None,
            "WebSocket disconnect detected".to_string(),
        )
        .await;

        // Get session info to determine chain
        info!("[AUTO-SETTLEMENT] 🔍 Looking up session {} in store...", session_id);
        let store = self.session_store.read().await;
        let session = store
            .get_session(session_id)
            .await
            .ok_or_else(|| {
                error!("[AUTO-SETTLEMENT] ❌ Session {} not found in store!", session_id);
                SettlementError::SessionNotFound(session_id.parse().unwrap_or(0))
            })?;

        let chain_id = session.chain_id;
        let session_id_u64 = session_id.parse::<u64>().unwrap_or(0);
        info!("[AUTO-SETTLEMENT] ✓ Session found - Chain ID: {}, Session ID (u64): {}", chain_id, session_id_u64);

        drop(store); // Release lock early

        // Queue settlement for processing
        info!("[AUTO-SETTLEMENT] 📦 Creating settlement request...");
        let request = SettlementRequest {
            session_id: session_id_u64,
            chain_id,
            priority: 1, // Default priority
            retry_count: 0,
            status: SettlementStatus::Pending,
        };

        info!("[AUTO-SETTLEMENT] 📤 Queueing settlement request...");
        self.settlement_manager
            .queue_settlement(request.clone())
            .await
            .map_err(|e| {
                error!("[AUTO-SETTLEMENT] ❌ Failed to queue settlement: {}", e);
                SettlementError::SettlementFailed {
                    chain: chain_id,
                    reason: e.to_string(),
                }
            })?;

        info!("[AUTO-SETTLEMENT] ✓ Settlement queued successfully: {:?}", request);

        self.log_event(
            session_id,
            EventType::SettlementQueued,
            Some(chain_id),
            format!("Settlement queued for chain {}", chain_id),
        )
        .await;

        // Optionally trigger immediate processing
        if self.config.enable_auto_settlement {
            info!("[AUTO-SETTLEMENT] ⚡ Auto-settlement enabled, triggering immediate processing...");
            match self.trigger_settlement_processing().await {
                Ok(()) => info!("[AUTO-SETTLEMENT] ✓ Settlement processing triggered successfully"),
                Err(e) => error!("[AUTO-SETTLEMENT] ❌ Failed to trigger settlement processing: {:?}", e),
            }
        } else {
            warn!("[AUTO-SETTLEMENT] ⚠️ Auto-settlement is DISABLED - settlement will not be processed automatically!");
        }

        info!("[AUTO-SETTLEMENT] ✅ Disconnect handling completed for session {}", session_id);
        Ok(())
    }

    /// Settle a session with specific chain
    pub async fn settle_session_with_chain(
        &self,
        session_id: &str,
        expected_chain_id: u64,
    ) -> Result<(), SettlementError> {
        // Verify session is on expected chain
        let store = self.session_store.read().await;
        let session = store
            .get_session(session_id)
            .await
            .ok_or_else(|| SettlementError::SessionNotFound(session_id.parse().unwrap_or(0)))?;

        if session.chain_id != expected_chain_id {
            return Err(SettlementError::SettlementFailed {
                chain: expected_chain_id,
                reason: format!(
                    "Session is on chain {}, not {}",
                    session.chain_id, expected_chain_id
                ),
            });
        }

        drop(store);

        // Proceed with settlement
        let session_id_u64 = session_id.parse::<u64>().unwrap_or(0);
        self.settlement_manager
            .settle_session(session_id_u64, expected_chain_id)
            .await?;

        self.log_event(
            session_id,
            EventType::SettlementCompleted,
            Some(expected_chain_id),
            "Settlement completed successfully".to_string(),
        )
        .await;

        Ok(())
    }

    /// Settle with retry logic
    pub async fn settle_with_retry(&self, session_id: &str) -> Result<(), SettlementError> {
        let mut retry_count = 0;
        let mut delay = self.config.retry_config.initial_delay;

        loop {
            match self.handle_disconnect(session_id).await {
                Ok(_) => {
                    self.retry_counts.write().await.remove(session_id);
                    return Ok(());
                }
                Err(e) if retry_count < self.config.retry_config.max_retries => {
                    retry_count += 1;
                    self.retry_counts
                        .write()
                        .await
                        .insert(session_id.to_string(), retry_count);

                    self.log_event(
                        session_id,
                        EventType::RetryAttempt,
                        None,
                        format!(
                            "Retry attempt {} of {}",
                            retry_count, self.config.retry_config.max_retries
                        ),
                    )
                    .await;

                    warn!(
                        "Settlement failed for session {}, retry {}/{}: {}",
                        session_id, retry_count, self.config.retry_config.max_retries, e
                    );

                    sleep(delay).await;

                    // Exponential backoff
                    delay = Duration::from_secs_f64(
                        (delay.as_secs_f64() * self.config.retry_config.exponential_base)
                            .min(self.config.retry_config.max_delay.as_secs_f64()),
                    );
                }
                Err(e) => {
                    error!(
                        "Settlement failed after {} retries for session {}: {}",
                        retry_count, session_id, e
                    );

                    self.log_event(
                        session_id,
                        EventType::SettlementFailed,
                        None,
                        format!("Failed after {} retries: {}", retry_count, e),
                    )
                    .await;

                    return Err(SettlementError::MaxRetriesExceeded(
                        session_id.parse().unwrap_or(0),
                    ));
                }
            }
        }
    }

    /// Queue failed settlement for later retry
    pub async fn queue_failed_settlement(&self, session_id: &str, chain_id: u64) -> Result<()> {
        let session_id_u64 = session_id.parse::<u64>().unwrap_or(0);

        let request = SettlementRequest {
            session_id: session_id_u64,
            chain_id,
            priority: 0, // Lower priority for retries
            retry_count: self.get_retry_count(session_id).await,
            status: SettlementStatus::Failed,
        };

        self.settlement_manager.queue_settlement(request).await?;

        info!(
            "Queued failed settlement for session {} on chain {}",
            session_id, chain_id
        );
        Ok(())
    }

    /// Get retry count for a session
    pub async fn get_retry_count(&self, session_id: &str) -> u8 {
        *self.retry_counts.read().await.get(session_id).unwrap_or(&0)
    }

    /// Trigger processing of queued settlements
    async fn trigger_settlement_processing(&self) -> Result<(), SettlementError> {
        // Process up to concurrent_settlements at once
        let results = self
            .settlement_manager
            .process_settlement_queue()
            .await
            .map_err(|e| SettlementError::SettlementFailed {
                chain: 0,
                reason: e.to_string(),
            })?;

        for result in results {
            debug!(
                "Processed settlement for session {} on chain {}: {:?}",
                result.session_id, result.chain_id, result.status
            );
        }

        Ok(())
    }

    /// Enable event tracking
    pub async fn enable_event_tracking(&self) {
        let mut tracking = self.event_tracking.write().await;
        *tracking = true;
    }

    /// Log a settlement event
    async fn log_event(
        &self,
        session_id: &str,
        event_type: EventType,
        chain_id: Option<u64>,
        details: String,
    ) {
        if !*self.event_tracking.read().await {
            return;
        }

        let event = SettlementEvent {
            session_id: session_id.to_string(),
            event_type,
            chain_id,
            timestamp: Utc::now(),
            details,
        };

        self.events
            .write()
            .await
            .entry(session_id.to_string())
            .or_insert_with(Vec::new)
            .push(event);
    }

    /// Get settlement events for a session
    pub async fn get_settlement_events(&self, session_id: &str) -> Vec<SettlementEvent> {
        self.events
            .read()
            .await
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Clear old events
    pub async fn clear_old_events(&self, older_than: Duration) {
        let cutoff = Utc::now() - chrono::Duration::from_std(older_than).unwrap();

        let mut events = self.events.write().await;
        for session_events in events.values_mut() {
            session_events.retain(|e| e.timestamp > cutoff);
        }

        // Remove empty entries
        events.retain(|_, v| !v.is_empty());
    }
}
