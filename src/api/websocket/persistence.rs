// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use super::session::WebSocketSession;
use super::storage_trait::{FileStorage, SessionStorage};
use crate::config::chains::ChainRegistry;
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceConfig {
    pub base_path: PathBuf,
    pub enable_backups: bool,
    pub backup_interval_seconds: u64,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            base_path: PathBuf::from("./data"),
            enable_backups: true,
            backup_interval_seconds: 3600, // 1 hour
        }
    }
}

pub struct SessionPersistence {
    config: PersistenceConfig,
    storage: Box<dyn SessionStorage>,
}

impl SessionPersistence {
    pub fn new(config: PersistenceConfig) -> Self {
        let storage = Box::new(FileStorage::new(config.base_path.clone()));
        Self { config, storage }
    }

    pub fn with_storage(config: PersistenceConfig, storage: Box<dyn SessionStorage>) -> Self {
        Self { config, storage }
    }

    /// Save a session to persistent storage
    pub async fn save_session(&self, session: &WebSocketSession) -> Result<()> {
        self.storage.save_session(session.chain_id, session).await?;
        info!("Saved session {} on chain {}", session.id, session.chain_id);
        Ok(())
    }

    /// Load a session from persistent storage
    pub async fn load_session(&self, chain_id: u64, session_id: &str) -> Result<WebSocketSession> {
        let session = self.storage.load_session(chain_id, session_id).await?;
        info!("Loaded session {} from chain {}", session_id, chain_id);
        Ok(session)
    }

    /// Recover all sessions from storage
    pub async fn recover_all_sessions(&self) -> Result<Vec<WebSocketSession>> {
        let mut all_sessions = Vec::new();
        let registry = ChainRegistry::new();

        for chain_id in registry.list_supported_chains() {
            match self.storage.list_sessions(chain_id).await {
                Ok(session_ids) => {
                    for session_id in session_ids {
                        match self.storage.load_session(chain_id, &session_id).await {
                            Ok(session) => {
                                all_sessions.push(session);
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to load session {} from chain {}: {}",
                                    session_id, chain_id, e
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to list sessions for chain {}: {}", chain_id, e);
                }
            }
        }

        info!("Recovered {} sessions from storage", all_sessions.len());
        Ok(all_sessions)
    }

    /// Create a backup of all sessions for a specific chain
    pub async fn create_chain_backup(&self, chain_id: u64) -> Result<String> {
        let backup_id = format!("{}", Utc::now().timestamp());
        let backup_dir = self.get_backup_path(chain_id, &backup_id);

        // Create backup directory
        fs::create_dir_all(&backup_dir).await?;

        // Get all sessions for this chain
        let session_ids = self.storage.list_sessions(chain_id).await?;
        let mut backed_up = 0;

        for session_id in session_ids {
            match self.storage.load_session(chain_id, &session_id).await {
                Ok(session) => {
                    let backup_path = backup_dir.join(format!("{}.json", session_id));
                    let json = session.to_json().await?;
                    fs::write(&backup_path, json).await?;
                    backed_up += 1;
                }
                Err(e) => {
                    warn!("Failed to backup session {}: {}", session_id, e);
                }
            }
        }

        info!(
            "Created backup {} for chain {} with {} sessions",
            backup_id, chain_id, backed_up
        );
        Ok(backup_id)
    }

    /// Restore sessions from a backup
    pub async fn restore_from_backup(&self, chain_id: u64, backup_id: &str) -> Result<usize> {
        let backup_dir = self.get_backup_path(chain_id, backup_id);

        if !backup_dir.exists() {
            return Err(anyhow!("Backup directory not found: {:?}", backup_dir));
        }

        let mut restored = 0;
        let mut entries = fs::read_dir(&backup_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                let contents = fs::read_to_string(&path).await?;
                match WebSocketSession::from_json(&contents).await {
                    Ok(session) => {
                        self.storage.save_session(chain_id, &session).await?;
                        restored += 1;
                    }
                    Err(e) => {
                        error!("Failed to restore session from {:?}: {}", path, e);
                    }
                }
            }
        }

        info!(
            "Restored {} sessions from backup {} for chain {}",
            restored, backup_id, chain_id
        );
        Ok(restored)
    }

    /// Migrate a session to a different chain
    pub async fn migrate_session_chain(
        &self,
        session_id: &str,
        from_chain: u64,
        to_chain: u64,
    ) -> Result<()> {
        // Load session from old chain
        let mut session = self.storage.load_session(from_chain, session_id).await?;

        // Update chain_id
        let registry = ChainRegistry::new();
        session.switch_chain(to_chain, &registry)?;

        // Save to new chain
        self.storage.save_session(to_chain, &session).await?;

        // Delete from old chain
        self.storage.delete_session(from_chain, session_id).await?;

        info!(
            "Migrated session {} from chain {} to chain {}",
            session_id, from_chain, to_chain
        );
        Ok(())
    }

    /// List all sessions for a specific chain
    pub async fn list_sessions_by_chain(&self, chain_id: u64) -> Result<Vec<String>> {
        self.storage.list_sessions(chain_id).await
    }

    /// Delete a session
    pub async fn delete_session(&self, chain_id: u64, session_id: &str) -> Result<()> {
        self.storage.delete_session(chain_id, session_id).await?;
        info!("Deleted session {} from chain {}", session_id, chain_id);
        Ok(())
    }

    /// Delete expired sessions
    pub async fn delete_expired_sessions(&mut self) -> Result<usize> {
        let mut deleted = 0;
        let registry = ChainRegistry::new();

        for chain_id in registry.list_supported_chains() {
            let session_ids = self.storage.list_sessions(chain_id).await?;

            for session_id in session_ids {
                match self.storage.load_session(chain_id, &session_id).await {
                    Ok(session) => {
                        if session.is_expired()
                            || session.state == super::session::SessionState::Closed
                        {
                            self.storage.delete_session(chain_id, &session_id).await?;
                            deleted += 1;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to check session {} for expiry: {}", session_id, e);
                    }
                }
            }
        }

        info!("Deleted {} expired sessions", deleted);
        Ok(deleted)
    }

    /// Get the path for a session file
    pub fn get_session_path(&self, chain_id: u64, session_id: &str) -> PathBuf {
        self.config
            .base_path
            .join("sessions")
            .join(chain_id.to_string())
            .join(format!("{}.json", session_id))
    }

    /// Get the path for a backup directory
    pub fn get_backup_path(&self, chain_id: u64, backup_id: &str) -> PathBuf {
        self.config
            .base_path
            .join("backups")
            .join(chain_id.to_string())
            .join(backup_id)
    }

    /// Create automatic backup if interval has passed
    pub async fn auto_backup_if_needed(
        &mut self,
        chain_id: u64,
        last_backup: Option<u64>,
    ) -> Result<Option<String>> {
        if !self.config.enable_backups {
            return Ok(None);
        }

        let now = Utc::now().timestamp() as u64;
        let should_backup = match last_backup {
            Some(last) => (now - last) >= self.config.backup_interval_seconds,
            None => true,
        };

        if should_backup {
            let backup_id = self.create_chain_backup(chain_id).await?;
            Ok(Some(backup_id))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::websocket::session::SessionConfig;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_persistence_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let config = PersistenceConfig {
            base_path: temp_dir.path().to_path_buf(),
            enable_backups: false,
            backup_interval_seconds: 60,
        };
        let persistence = SessionPersistence::new(config);

        let session = WebSocketSession::with_chain("test", SessionConfig::default(), 84532);

        // Save
        persistence.save_session(&session).await.unwrap();

        // Load
        let loaded = persistence.load_session(84532, "test").await.unwrap();
        assert_eq!(loaded.id, "test");
        assert_eq!(loaded.chain_id, 84532);
    }
}
