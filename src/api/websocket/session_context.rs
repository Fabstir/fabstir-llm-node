use crate::api::websocket::messages::ChainInfo;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Session context with chain awareness
#[derive(Debug, Clone)]
pub struct SessionContext {
    pub session_id: String,
    pub job_id: u64,
    pub chain_id: u64,
    pub chain_info: ChainInfo,
    pub is_active: bool,
    pub created_at: u64,
}

impl SessionContext {
    /// Create new session context with chain info
    pub fn new(session_id: String, job_id: u64, chain_id: u64) -> Self {
        let chain_info = Self::get_chain_info(chain_id);

        Self {
            session_id,
            job_id,
            chain_id,
            chain_info,
            is_active: true,
            created_at: chrono::Utc::now().timestamp() as u64,
        }
    }

    /// Get chain information for a chain ID
    fn get_chain_info(chain_id: u64) -> ChainInfo {
        match chain_id {
            84532 => ChainInfo {
                chain_id,
                chain_name: "Base Sepolia".to_string(),
                native_token: "ETH".to_string(),
                rpc_url: "https://sepolia.base.org".to_string(),
            },
            5611 => ChainInfo {
                chain_id,
                chain_name: "opBNB Testnet".to_string(),
                native_token: "BNB".to_string(),
                rpc_url: "https://opbnb-testnet-rpc.bnbchain.org".to_string(),
            },
            _ => ChainInfo {
                chain_id,
                chain_name: "Unknown".to_string(),
                native_token: "UNKNOWN".to_string(),
                rpc_url: String::new(),
            },
        }
    }

    /// Check if chain is supported
    pub fn is_chain_supported(chain_id: u64) -> bool {
        matches!(chain_id, 84532 | 5611)
    }

    /// Get gas price multiplier for the chain
    pub fn gas_multiplier(&self) -> f64 {
        match self.chain_id {
            84532 => 1.1, // Base Sepolia
            5611 => 1.2,  // opBNB
            _ => 1.0,
        }
    }

    /// Get block confirmation requirements
    pub fn confirmations_required(&self) -> u64 {
        match self.chain_id {
            84532 => 3, // Base Sepolia - faster
            5611 => 15, // opBNB - more confirmations needed
            _ => 10,
        }
    }
}

/// Manager for chain-aware sessions
pub struct ChainAwareSessionManager {
    sessions: Arc<RwLock<HashMap<String, SessionContext>>>,
    job_chain_map: Arc<RwLock<HashMap<u64, u64>>>, // job_id -> chain_id
}

impl ChainAwareSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            job_chain_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session with chain validation
    pub async fn create_session(
        &self,
        session_id: String,
        job_id: u64,
        chain_id: u64,
    ) -> Result<SessionContext> {
        // Validate chain is supported
        if !SessionContext::is_chain_supported(chain_id) {
            return Err(anyhow!("Chain {} is not supported", chain_id));
        }

        // Check if job already has active session on different chain
        let job_map = self.job_chain_map.read().await;
        if let Some(existing_chain) = job_map.get(&job_id) {
            if *existing_chain != chain_id {
                // Check if that session is still active
                let sessions = self.sessions.read().await;
                let has_active = sessions
                    .values()
                    .any(|s| s.job_id == job_id && s.chain_id == *existing_chain && s.is_active);

                if has_active {
                    return Err(anyhow!(
                        "Job {} already has active session on chain {}. Cannot create on chain {}",
                        job_id,
                        existing_chain,
                        chain_id
                    ));
                }
            }
        }
        drop(job_map);

        // Create session context
        let context = SessionContext::new(session_id.clone(), job_id, chain_id);

        // Store session
        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.clone(), context.clone());

        // Update job-chain mapping
        let mut job_map = self.job_chain_map.write().await;
        job_map.insert(job_id, chain_id);

        Ok(context)
    }

    /// Get session context
    pub async fn get_session(&self, session_id: &str) -> Option<SessionContext> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// End a session
    pub async fn end_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.is_active = false;
            Ok(())
        } else {
            Err(anyhow!("Session {} not found", session_id))
        }
    }

    /// Check if chain switch is allowed (it's not for active sessions)
    pub async fn can_switch_chain(&self, session_id: &str, new_chain_id: u64) -> Result<bool> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_id) {
            if session.is_active && session.chain_id != new_chain_id {
                return Ok(false); // Cannot switch chains mid-session
            }
            Ok(true)
        } else {
            Err(anyhow!("Session {} not found", session_id))
        }
    }

    /// Get all sessions for a chain
    pub async fn get_sessions_by_chain(&self, chain_id: u64) -> Vec<SessionContext> {
        self.sessions
            .read()
            .await
            .values()
            .filter(|s| s.chain_id == chain_id && s.is_active)
            .cloned()
            .collect()
    }

    /// Get session count per chain
    pub async fn get_chain_statistics(&self) -> HashMap<u64, usize> {
        let mut stats = HashMap::new();
        for session in self.sessions.read().await.values() {
            if session.is_active {
                *stats.entry(session.chain_id).or_insert(0) += 1;
            }
        }
        stats
    }
}

impl Default for ChainAwareSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_context_creation() {
        let context = SessionContext::new("test-session".to_string(), 100, 84532);
        assert_eq!(context.chain_id, 84532);
        assert_eq!(context.chain_info.native_token, "ETH");
        assert_eq!(context.gas_multiplier(), 1.1);
        assert_eq!(context.confirmations_required(), 3);
    }

    #[tokio::test]
    async fn test_chain_aware_session_manager() {
        let manager = ChainAwareSessionManager::new();

        // Create session on Base Sepolia
        let result = manager
            .create_session("session-1".to_string(), 100, 84532)
            .await;
        assert!(result.is_ok());

        // Try to create session for same job on different chain
        let result = manager
            .create_session("session-2".to_string(), 100, 5611)
            .await;
        assert!(result.is_err());

        // End first session
        manager.end_session("session-1").await.unwrap();

        // Now can create on different chain
        let result = manager
            .create_session("session-3".to_string(), 100, 5611)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_chain_statistics() {
        let manager = ChainAwareSessionManager::new();

        // Create sessions on different chains
        manager
            .create_session("s1".to_string(), 1, 84532)
            .await
            .unwrap();
        manager
            .create_session("s2".to_string(), 2, 84532)
            .await
            .unwrap();
        manager
            .create_session("s3".to_string(), 3, 5611)
            .await
            .unwrap();

        let stats = manager.get_chain_statistics().await;
        assert_eq!(stats.get(&84532), Some(&2));
        assert_eq!(stats.get(&5611), Some(&1));
    }
}
