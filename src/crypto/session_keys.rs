// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Session Key Storage
//!
//! Manages in-memory storage of session encryption keys. Keys are stored
//! per session ID and automatically cleared when sessions end or expire.
//!
//! **Security**: Keys are stored in memory only and never persisted to disk.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Entry storing a session key with optional expiration
#[derive(Clone, Copy)]
struct KeyEntry {
    key: [u8; 32],
    stored_at: Instant,
}

/// In-memory storage for session encryption keys
///
/// Provides thread-safe storage and retrieval of session keys using
/// a session ID as the lookup key. Supports optional TTL for automatic
/// key expiration.
///
/// # Example
///
/// ```ignore
/// // Without TTL
/// let store = SessionKeyStore::new();
/// store.store_key("session-123", [0u8; 32]).await;
/// let key = store.get_key("session-123").await;
/// store.clear_key("session-123").await;
///
/// // With TTL
/// let store = SessionKeyStore::with_ttl(Duration::from_secs(3600));
/// store.store_key("session-456", [0u8; 32]).await;
/// // Key will expire after 1 hour
/// ```
#[derive(Clone)]
pub struct SessionKeyStore {
    keys: Arc<RwLock<HashMap<String, KeyEntry>>>,
    /// Keys stored per (connection id, session id) — SCI-C D18. Two connections
    /// on one session each keep the key their own init sent; the session-only
    /// map above let the latest init overwrite a key another connection was
    /// still decrypting with.
    connection_keys: Arc<RwLock<HashMap<(String, String), KeyEntry>>>,
    ttl: Option<Duration>,
}

impl SessionKeyStore {
    /// Create a new session key store without TTL
    pub fn new() -> Self {
        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
            connection_keys: Arc::new(RwLock::new(HashMap::new())),
            ttl: None,
        }
    }

    /// Create a new session key store with TTL
    ///
    /// Keys will expire after the specified duration from when they were stored.
    ///
    /// # Arguments
    ///
    /// * `ttl` - Time-to-live duration for keys
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
            connection_keys: Arc::new(RwLock::new(HashMap::new())),
            ttl: Some(ttl),
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
        let entry = KeyEntry {
            key,
            stored_at: Instant::now(),
        };
        keys.insert(session_id.clone(), entry);
        tracing::info!(
            "🔑 Session key stored for session: {} (total keys: {})",
            session_id,
            keys.len()
        );
    }

    /// Retrieve a session key
    ///
    /// Returns None if the key doesn't exist or has expired.
    ///
    /// # Arguments
    ///
    /// * `session_id` - Unique session identifier
    ///
    /// # Returns
    ///
    /// The 32-byte encryption key if found and not expired, None otherwise
    pub async fn get_key(&self, session_id: &str) -> Option<[u8; 32]> {
        let keys = self.keys.read().await;
        if let Some(entry) = keys.get(session_id) {
            // Check if expired
            if let Some(ttl) = self.ttl {
                if entry.stored_at.elapsed() > ttl {
                    return None; // Expired
                }
            }
            Some(entry.key)
        } else {
            None
        }
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
    /// Number of stored key entries: legacy session-only keys plus one per
    /// (connection, session) pair, so two sockets on one session count twice.
    pub async fn count(&self) -> usize {
        let keys = self.keys.read().await;
        keys.len() + self.connection_keys.read().await.len()
    }

    /// Store the key connection `conn_id`'s init sent for `session_id`.
    pub async fn store_key_for(&self, conn_id: &str, session_id: String, key: [u8; 32]) {
        let mut keys = self.connection_keys.write().await;
        let entry = KeyEntry {
            key,
            stored_at: Instant::now(),
        };
        keys.insert((conn_id.to_string(), session_id.clone()), entry);
        tracing::info!(
            "🔑 Session key stored for session: {} on connection {} (connection keys: {})",
            session_id,
            conn_id,
            keys.len()
        );
    }

    /// The key connection `conn_id` stored for `session_id`; never another
    /// connection's key for the same session.
    pub async fn get_key_for(&self, conn_id: &str, session_id: &str) -> Option<[u8; 32]> {
        let keys = self.connection_keys.read().await;
        let entry = keys.get(&(conn_id.to_string(), session_id.to_string()))?;
        if let Some(ttl) = self.ttl {
            if entry.stored_at.elapsed() > ttl {
                return None; // Expired
            }
        }
        Some(entry.key)
    }

    /// Drop every key connection `conn_id` stored. Returns how many went.
    pub async fn remove_connection(&self, conn_id: &str) -> usize {
        let mut keys = self.connection_keys.write().await;
        let before = keys.len();
        keys.retain(|(conn, _), _| conn != conn_id);
        let removed = before - keys.len();
        if removed > 0 {
            tracing::info!(
                "🗑️  Cleared {} session key(s) for closed connection {} (remaining: {})",
                removed,
                conn_id,
                keys.len()
            );
        }
        removed
    }

    /// Clear all session keys
    ///
    /// Used for testing or shutdown scenarios
    pub async fn clear_all(&self) {
        let mut keys = self.keys.write().await;
        let mut connection_keys = self.connection_keys.write().await;
        let count = keys.len() + connection_keys.len();
        keys.clear();
        connection_keys.clear();
        tracing::info!("🗑️  Cleared all session keys (count: {})", count);
    }

    /// Clear expired session keys
    ///
    /// Removes all keys that have exceeded their TTL. If no TTL is configured,
    /// this method does nothing.
    ///
    /// # Returns
    ///
    /// The number of keys that were cleared
    pub async fn clear_expired_keys(&self) -> usize {
        if let Some(ttl) = self.ttl {
            let mut keys = self.keys.write().await;
            let now = Instant::now();
            let initial_count = keys.len();

            // Remove expired keys
            keys.retain(|_, entry| now.duration_since(entry.stored_at) <= ttl);
            let mut connection_keys = self.connection_keys.write().await;
            let initial_connection_count = connection_keys.len();
            connection_keys.retain(|_, entry| now.duration_since(entry.stored_at) <= ttl);

            let cleared_count =
                initial_count - keys.len() + initial_connection_count - connection_keys.len();
            if cleared_count > 0 {
                tracing::info!(
                    "🗑️  Cleared {} expired session keys (remaining: {})",
                    cleared_count,
                    keys.len()
                );
            }
            cleared_count
        } else {
            0 // No TTL configured, nothing to clear
        }
    }
}

impl Default for SessionKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Clears a connection's keys when its handler exits, on every path including
/// early returns (SCI-C D18).
pub struct ConnectionKeys {
    store: Arc<SessionKeyStore>,
    conn_id: String,
}

impl ConnectionKeys {
    pub fn new(store: Arc<SessionKeyStore>, conn_id: String) -> Self {
        Self { store, conn_id }
    }

    pub fn conn_id(&self) -> &str {
        &self.conn_id
    }
}

impl Drop for ConnectionKeys {
    fn drop(&mut self) {
        let store = self.store.clone();
        let conn_id = std::mem::take(&mut self.conn_id);
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    store.remove_connection(&conn_id).await;
                });
            }
            Err(_) => tracing::warn!(
                "Connection {} dropped outside a tokio runtime; its session keys were not cleared",
                conn_id
            ),
        }
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
