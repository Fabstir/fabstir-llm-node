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
        info!("[DISCONNECT-HANDLER] 🔌 === WebSocket Disconnect Event === Session: {}", session_id);

        // Get session info before cleanup
        info!("[DISCONNECT-HANDLER] 🔍 Acquiring session store lock...");
        let mut store = self.session_store.write().await;
        let session_info = store.get_session(session_id).await;

        if let Some(session) = session_info {
            let chain_id = session.chain_id;
            let session_id_u64 = session_id.parse::<u64>().unwrap_or(0);

            info!("[DISCONNECT-HANDLER] ✓ Session found:");
            info!("  - Session ID: {} (u64: {})", session_id, session_id_u64);
            info!("  - Chain ID: {}", chain_id);
            info!("  - Created at: {:?}", session.created_at);

            // Clean up session from store
            info!("[DISCONNECT-HANDLER] 🧹 Cleaning up session from store...");
            store.destroy_session(session_id).await;
            info!("[DISCONNECT-HANDLER] ✓ Session removed from store");

            drop(store); // Release lock before settlement

            // Trigger settlement if manager is available
            if let Some(settlement_manager) = &self.settlement_manager {
                info!(
                    "[DISCONNECT-HANDLER] 💰 === Starting Payment Settlement ==="
                );
                info!(
                    "[DISCONNECT-HANDLER] Triggering settlement for session {} on chain {}",
                    session_id, chain_id
                );

                match settlement_manager
                    .settle_session(session_id_u64, chain_id)
                    .await
                {
                    Ok(tx_hash) => {
                        info!(
                            "[DISCONNECT-HANDLER] ✅ Settlement initiated successfully!",
                        );
                        info!(
                            "[DISCONNECT-HANDLER]   - Session: {}",
                            session_id
                        );
                        info!(
                            "[DISCONNECT-HANDLER]   - Transaction Hash: {:?}",
                            tx_hash
                        );
                        info!(
                            "[DISCONNECT-HANDLER] 💸 Payment settlement should now be processing..."
                        );
                    }
                    Err(e) => {
                        error!(
                            "[DISCONNECT-HANDLER] ❌ Settlement FAILED for session {} on chain {}",
                            session_id, chain_id
                        );
                        error!(
                            "[DISCONNECT-HANDLER]   - Error: {}",
                            e
                        );
                        error!(
                            "[DISCONNECT-HANDLER] ⚠️ Settlement will need to be retried later!"
                        );
                        // Continue with cleanup even if settlement fails
                        // The settlement can be retried later
                    }
                }
            } else {
                warn!("[DISCONNECT-HANDLER] ⚠️ NO SETTLEMENT MANAGER AVAILABLE!");
                warn!("[DISCONNECT-HANDLER] ⚠️ Payment settlement SKIPPED - this means payments won't be distributed!");
                warn!("[DISCONNECT-HANDLER] ⚠️ This session ({}) will need manual settlement later", session_id);
            }
        } else {
            // Just clean up if session doesn't exist
            warn!("[DISCONNECT-HANDLER] ⚠️ Session {} not found in store during disconnect", session_id);
            info!("[DISCONNECT-HANDLER] Attempting cleanup anyway...");
            store.destroy_session(session_id).await;
            warn!("[DISCONNECT-HANDLER] ⚠️ No settlement possible - session data missing");
        }

        info!("[DISCONNECT-HANDLER] ✔️ Disconnect handling completed for session {}", session_id);
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
