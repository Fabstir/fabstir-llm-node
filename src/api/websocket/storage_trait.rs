// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use super::session::WebSocketSession;
use anyhow::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Trait for session storage backends
#[async_trait]
pub trait SessionStorage: Send + Sync {
    /// Save a session to storage
    async fn save_session(&self, chain_id: u64, session: &WebSocketSession) -> Result<()>;

    /// Load a session from storage
    async fn load_session(&self, chain_id: u64, session_id: &str) -> Result<WebSocketSession>;

    /// List all session IDs for a chain
    async fn list_sessions(&self, chain_id: u64) -> Result<Vec<String>>;

    /// Delete a session from storage
    async fn delete_session(&self, chain_id: u64, session_id: &str) -> Result<()>;

    /// Check if a session exists
    async fn session_exists(&self, chain_id: u64, session_id: &str) -> Result<bool>;
}

/// File-based storage implementation
pub struct FileStorage {
    base_path: PathBuf,
}

impl FileStorage {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    fn get_chain_dir(&self, chain_id: u64) -> PathBuf {
        self.base_path.join("sessions").join(chain_id.to_string())
    }

    fn get_session_path(&self, chain_id: u64, session_id: &str) -> PathBuf {
        self.get_chain_dir(chain_id)
            .join(format!("{}.json", session_id))
    }

    async fn ensure_chain_dir(&self, chain_id: u64) -> Result<()> {
        let dir = self.get_chain_dir(chain_id);
        if !dir.exists() {
            fs::create_dir_all(&dir).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl SessionStorage for FileStorage {
    async fn save_session(&self, chain_id: u64, session: &WebSocketSession) -> Result<()> {
        self.ensure_chain_dir(chain_id).await?;

        let path = self.get_session_path(chain_id, &session.id);
        let json = session.to_json().await?;

        // Write atomically using a temp file
        let temp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&temp_path).await?;
        file.write_all(json.as_bytes()).await?;
        file.sync_all().await?;

        // Rename atomically
        fs::rename(temp_path, path).await?;

        Ok(())
    }

    async fn load_session(&self, chain_id: u64, session_id: &str) -> Result<WebSocketSession> {
        let path = self.get_session_path(chain_id, session_id);

        if !path.exists() {
            return Err(anyhow::anyhow!("Session file not found: {:?}", path));
        }

        let mut file = fs::File::open(&path).await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;

        let mut session = WebSocketSession::from_json(&contents).await?;

        // Ensure chain_id matches (in case of corruption)
        if session.chain_id != chain_id {
            tracing::warn!(
                "Session {} has mismatched chain_id: expected {}, got {}",
                session_id,
                chain_id,
                session.chain_id
            );
            session.chain_id = chain_id;
        }

        Ok(session)
    }

    async fn list_sessions(&self, chain_id: u64) -> Result<Vec<String>> {
        let dir = self.get_chain_dir(chain_id);

        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        let mut entries = fs::read_dir(&dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    sessions.push(stem.to_string());
                }
            }
        }

        Ok(sessions)
    }

    async fn delete_session(&self, chain_id: u64, session_id: &str) -> Result<()> {
        let path = self.get_session_path(chain_id, session_id);

        if path.exists() {
            fs::remove_file(&path).await?;
        }

        Ok(())
    }

    async fn session_exists(&self, chain_id: u64, session_id: &str) -> Result<bool> {
        let path = self.get_session_path(chain_id, session_id);
        Ok(path.exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::websocket::session::SessionConfig;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_file_storage_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path());

        let session = WebSocketSession::with_chain("test_session", SessionConfig::default(), 84532);

        // Save
        storage.save_session(84532, &session).await.unwrap();

        // Load
        let loaded = storage.load_session(84532, "test_session").await.unwrap();
        assert_eq!(loaded.id, "test_session");
        assert_eq!(loaded.chain_id, 84532);
    }

    #[tokio::test]
    async fn test_file_storage_list() {
        let temp_dir = TempDir::new().unwrap();
        let storage = FileStorage::new(temp_dir.path());

        // Save multiple sessions
        for i in 0..3 {
            let session = WebSocketSession::with_chain(
                format!("session_{}", i),
                SessionConfig::default(),
                84532,
            );
            storage.save_session(84532, &session).await.unwrap();
        }

        // List
        let sessions = storage.list_sessions(84532).await.unwrap();
        assert_eq!(sessions.len(), 3);
        assert!(sessions.contains(&"session_0".to_string()));
        assert!(sessions.contains(&"session_1".to_string()));
        assert!(sessions.contains(&"session_2".to_string()));
    }
}
