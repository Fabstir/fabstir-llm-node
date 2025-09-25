use crate::api::websocket::session_store::SessionStore;
use crate::settlement::manager::SettlementManager;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

pub struct DisconnectHandler {
    session_store: Arc<RwLock<SessionStore>>,
    settlement_manager: Option<Arc<SettlementManager>>,
}

impl DisconnectHandler {
    pub fn new(
        session_store: Arc<RwLock<SessionStore>>,
        settlement_manager: Option<Arc<SettlementManager>>,
    ) -> Self {
        Self {
            session_store,
            settlement_manager,
        }
    }

    /// Handle WebSocket disconnect event
    pub async fn handle_disconnect(&self, session_id: &str) -> Result<()> {
        info!("Handling WebSocket disconnect for session: {}", session_id);

        // Get session info before cleanup
        let mut store = self.session_store.write().await;
        let session_info = store.get_session(session_id).await;

        if let Some(session) = session_info {
            let chain_id = session.chain_id;
            let session_id_u64 = session_id.parse::<u64>().unwrap_or(0);

            // Clean up session from store
            store.destroy_session(session_id).await;

            drop(store); // Release lock before settlement

            // Trigger settlement if manager is available
            if let Some(settlement_manager) = &self.settlement_manager {
                info!(
                    "Triggering settlement for session {} on chain {}",
                    session_id, chain_id
                );

                match settlement_manager
                    .settle_session(session_id_u64, chain_id)
                    .await
                {
                    Ok(tx_hash) => {
                        info!(
                            "Settlement initiated for session {} with tx: {:?}",
                            session_id, tx_hash
                        );
                    }
                    Err(e) => {
                        error!(
                            "Failed to settle session {} on chain {}: {}",
                            session_id, chain_id, e
                        );
                        // Continue with cleanup even if settlement fails
                        // The settlement can be retried later
                    }
                }
            } else {
                warn!("No settlement manager available, skipping settlement");
            }
        } else {
            // Just clean up if session doesn't exist
            store.destroy_session(session_id).await;
            warn!("Session {} not found during disconnect", session_id);
        }

        Ok(())
    }

    /// Handle multiple disconnects (batch processing)
    pub async fn handle_batch_disconnect(&self, session_ids: Vec<String>) -> Result<()> {
        info!(
            "Handling batch disconnect for {} sessions",
            session_ids.len()
        );

        let mut results = Vec::new();
        for session_id in session_ids {
            let result = self.handle_disconnect(&session_id).await;
            results.push((session_id.clone(), result));
        }

        // Log results
        let mut success_count = 0;
        let mut failure_count = 0;

        for (session_id, result) in results {
            match result {
                Ok(_) => {
                    success_count += 1;
                    info!("Successfully handled disconnect for session {}", session_id);
                }
                Err(e) => {
                    failure_count += 1;
                    error!(
                        "Failed to handle disconnect for session {}: {}",
                        session_id, e
                    );
                }
            }
        }

        info!(
            "Batch disconnect complete: {} successful, {} failed",
            success_count, failure_count
        );

        if failure_count > 0 {
            return Err(anyhow!("Batch disconnect had {} failures", failure_count));
        }

        Ok(())
    }

    /// Check if settlement is enabled
    pub fn has_settlement_manager(&self) -> bool {
        self.settlement_manager.is_some()
    }
}
