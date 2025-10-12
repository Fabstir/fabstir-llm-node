//! Session Key Storage
//!
//! Manages in-memory storage of session encryption keys. Keys are stored
//! per session ID and automatically cleared when sessions end or expire.
//!
//! **Security**: Keys are stored in memory only and never persisted to disk.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// In-memory storage for session encryption keys
///
/// Provides thread-safe storage and retrieval of session keys using
/// a session ID as the lookup key.
///
/// # Example
///
/// ```ignore
/// let store = SessionKeyStore::new();
/// store.store_key("session-123", [0u8; 32]).await;
/// let key = store.get_key("session-123").await;
/// store.clear_key("session-123").await;
/// ```
#[derive(Clone)]
pub struct SessionKeyStore {
    keys: Arc<RwLock<HashMap<String, [u8; 32]>>>,
}

impl SessionKeyStore {
    /// Create a new session key store
    pub fn new() -> Self {
        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Store a session key
    ///
    /// # Arguments
    ///
    /// * `session_id` - Unique session identifier
    /// * `key` - 32-byte encryption key
    pub async fn store_key(&self, session_id: String, key: [u8; 32]) {
        let mut keys = self.keys.write().await;
        keys.insert(session_id.clone(), key);
        tracing::info!(
            "🔑 Session key stored for session: {} (total keys: {})",
            session_id,
            keys.len()
        );
    }

    /// Retrieve a session key
    ///
    /// # Arguments
    ///
    /// * `session_id` - Unique session identifier
    ///
    /// # Returns
    ///
    /// The 32-byte encryption key if found, None otherwise
    pub async fn get_key(&self, session_id: &str) -> Option<[u8; 32]> {
        let keys = self.keys.read().await;
        keys.get(session_id).copied()
    }

    /// Clear a session key
    ///
    /// Removes the key from storage. Should be called when a session ends.
    ///
    /// # Arguments
    ///
    /// * `session_id` - Unique session identifier
    pub async fn clear_key(&self, session_id: &str) {
        let mut keys = self.keys.write().await;
        if keys.remove(session_id).is_some() {
            tracing::info!(
                "🗑️  Session key cleared for session: {} (remaining: {})",
                session_id,
                keys.len()
            );
        }
    }

    /// Get the number of stored session keys
    pub async fn count(&self) -> usize {
        let keys = self.keys.read().await;
        keys.len()
    }

    /// Clear all session keys
    ///
    /// Used for testing or shutdown scenarios
    pub async fn clear_all(&self) {
        let mut keys = self.keys.write().await;
        let count = keys.len();
        keys.clear();
        tracing::info!("🗑️  Cleared all session keys (count: {})", count);
    }
}

impl Default for SessionKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_store_and_retrieve_key() {
        let store = SessionKeyStore::new();
        let session_id = "test-session-1".to_string();
        let key = [42u8; 32];

        // Store key
        store.store_key(session_id.clone(), key).await;

        // Retrieve key
        let retrieved = store.get_key(&session_id).await;
        assert_eq!(retrieved, Some(key));
    }

    #[tokio::test]
    async fn test_get_nonexistent_key() {
        let store = SessionKeyStore::new();
        let result = store.get_key("nonexistent").await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_clear_key() {
        let store = SessionKeyStore::new();
        let session_id = "test-session-2".to_string();
        let key = [99u8; 32];

        store.store_key(session_id.clone(), key).await;
        store.clear_key(&session_id).await;

        let retrieved = store.get_key(&session_id).await;
        assert_eq!(retrieved, None);
    }

    #[tokio::test]
    async fn test_count() {
        let store = SessionKeyStore::new();

        assert_eq!(store.count().await, 0);

        store.store_key("session-1".to_string(), [1u8; 32]).await;
        assert_eq!(store.count().await, 1);

        store.store_key("session-2".to_string(), [2u8; 32]).await;
        assert_eq!(store.count().await, 2);

        store.clear_key("session-1").await;
        assert_eq!(store.count().await, 1);
    }

    #[tokio::test]
    async fn test_clear_all() {
        let store = SessionKeyStore::new();

        store.store_key("session-1".to_string(), [1u8; 32]).await;
        store.store_key("session-2".to_string(), [2u8; 32]).await;
        store.store_key("session-3".to_string(), [3u8; 32]).await;

        assert_eq!(store.count().await, 3);

        store.clear_all().await;
        assert_eq!(store.count().await, 0);
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let store = SessionKeyStore::new();
        let store_clone = store.clone();

        // Spawn concurrent tasks
        let handle1 = tokio::spawn(async move {
            for i in 0..10 {
                let session_id = format!("session-{}", i);
                store_clone.store_key(session_id, [i as u8; 32]).await;
            }
        });

        let store_clone2 = store.clone();
        let handle2 = tokio::spawn(async move {
            for i in 10..20 {
                let session_id = format!("session-{}", i);
                store_clone2.store_key(session_id, [i as u8; 32]).await;
            }
        });

        handle1.await.unwrap();
        handle2.await.unwrap();

        // Should have 20 keys stored
        assert_eq!(store.count().await, 20);
    }
}
