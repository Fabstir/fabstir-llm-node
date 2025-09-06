use fabstir_llm_node::api::websocket::{
    memory_manager::{MemoryManager, MemoryConfig, MemoryStats, SessionPool},
    session::{WebSocketSession, SessionConfig},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration;

#[tokio::test]
async fn test_memory_manager_creation() {
    let config = MemoryConfig::default();
    let manager = MemoryManager::new(config);
    
    let stats = manager.stats().await;
    assert_eq!(stats.total_sessions, 0);
    assert_eq!(stats.memory_used_bytes, 0);
    assert_eq!(stats.pool_size, 0);
}

#[tokio::test]
async fn test_session_pool_allocation() {
    let mut pool = SessionPool::new(10); // Pool of 10 sessions
    
    assert_eq!(pool.size(), 10);
    assert_eq!(pool.available(), 10);
    
    let session = pool.acquire().await.unwrap();
    assert_eq!(pool.available(), 9);
    
    pool.release(session).await;
    assert_eq!(pool.available(), 10);
}

#[tokio::test]
async fn test_lru_eviction() {
    let config = MemoryConfig {
        max_sessions: 3,
        max_memory_bytes: 1024 * 1024, // 1MB
        eviction_threshold: 0.8,
        compression_enabled: true,
    };
    
    let manager = MemoryManager::new(config);
    
    // Add 4 sessions (exceeds max of 3)
    for i in 0..4 {
        let session_id = format!("session-{}", i);
        manager.add_session(session_id).await.unwrap();
    }
    
    // First session should be evicted
    assert!(manager.get_session("session-0").await.is_none());
    assert!(manager.get_session("session-1").await.is_some());
    assert!(manager.get_session("session-2").await.is_some());
    assert!(manager.get_session("session-3").await.is_some());
}

#[tokio::test]
async fn test_session_access_updates_lru() {
    let config = MemoryConfig {
        max_sessions: 3,
        max_memory_bytes: 1024 * 1024,
        eviction_threshold: 0.8,
        compression_enabled: false,
    };
    
    let manager = MemoryManager::new(config);
    
    // Add 3 sessions
    manager.add_session("session-1".to_string()).await.unwrap();
    manager.add_session("session-2".to_string()).await.unwrap();
    manager.add_session("session-3".to_string()).await.unwrap();
    
    // Access session-1 to make it most recently used
    manager.get_session("session-1").await.unwrap();
    
    // Add another session, should evict session-2 (least recently used)
    manager.add_session("session-4".to_string()).await.unwrap();
    
    assert!(manager.get_session("session-1").await.is_some());
    assert!(manager.get_session("session-2").await.is_none()); // Evicted
    assert!(manager.get_session("session-3").await.is_some());
    assert!(manager.get_session("session-4").await.is_some());
}

#[tokio::test]
async fn test_memory_pressure_handling() {
    let config = MemoryConfig {
        max_sessions: 100,
        max_memory_bytes: 1024, // Very small: 1KB
        eviction_threshold: 0.8,
        compression_enabled: true,
    };
    
    let manager = MemoryManager::new(config);
    
    // Add sessions until memory pressure
    for i in 0..10 {
        let result = manager.add_session(format!("session-{}", i)).await;
        if i < 5 {
            assert!(result.is_ok());
        }
    }
    
    let stats = manager.stats().await;
    assert!(stats.memory_used_bytes <= 1024);
    assert!(stats.eviction_count > 0);
}

#[tokio::test]
async fn test_session_compression() {
    let config = MemoryConfig {
        max_sessions: 10,
        max_memory_bytes: 10 * 1024 * 1024,
        eviction_threshold: 0.8,
        compression_enabled: true,
    };
    
    let manager = MemoryManager::new(config);
    
    let session_id = "test-session".to_string();
    manager.add_session(session_id.clone()).await.unwrap();
    
    // Mark session as idle for compression
    manager.mark_idle(&session_id).await.unwrap();
    
    let compressed_size = manager.get_session_memory_usage(&session_id).await.unwrap();
    
    // Access session (should decompress)
    manager.get_session(&session_id).await.unwrap();
    
    let uncompressed_size = manager.get_session_memory_usage(&session_id).await.unwrap();
    
    // Compressed should be smaller
    assert!(compressed_size < uncompressed_size);
}

#[tokio::test]
async fn test_memory_stats_tracking() {
    let config = MemoryConfig::default();
    let manager = MemoryManager::new(config);
    
    manager.add_session("session-1".to_string()).await.unwrap();
    manager.add_session("session-2".to_string()).await.unwrap();
    
    let stats = manager.stats().await;
    assert_eq!(stats.total_sessions, 2);
    assert!(stats.memory_used_bytes > 0);
    assert_eq!(stats.compression_ratio, 1.0); // No compression by default
    assert_eq!(stats.eviction_count, 0);
}

#[tokio::test]
async fn test_session_memory_limits() {
    let config = MemoryConfig {
        max_sessions: 10,
        max_memory_bytes: 10 * 1024 * 1024,
        eviction_threshold: 0.8,
        compression_enabled: false,
    };
    
    let manager = MemoryManager::new(config);
    
    let session_id = "test-session".to_string();
    manager.add_session(session_id.clone()).await.unwrap();
    
    // Try to add large amount of data
    let large_data = vec![0u8; 1024 * 1024]; // 1MB
    for _ in 0..5 {
        manager.add_session_data(&session_id, large_data.clone()).await.unwrap();
    }
    
    // Check memory usage
    let usage = manager.get_session_memory_usage(&session_id).await.unwrap();
    assert!(usage >= 5 * 1024 * 1024);
}

#[tokio::test]
async fn test_concurrent_session_access() {
    let config = MemoryConfig::default();
    let manager = Arc::new(MemoryManager::new(config));
    
    // Create multiple sessions
    for i in 0..10 {
        manager.add_session(format!("session-{}", i)).await.unwrap();
    }
    
    // Concurrent access
    let mut handles = vec![];
    for i in 0..10 {
        let manager_clone = manager.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..100 {
                manager_clone.get_session(&format!("session-{}", i)).await;
            }
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.await.unwrap();
    }
    
    let stats = manager.stats().await;
    assert_eq!(stats.total_sessions, 10);
}

#[tokio::test]
async fn test_memory_defragmentation() {
    let config = MemoryConfig::default();
    let manager = MemoryManager::new(config);
    
    // Add and remove sessions to create fragmentation
    for i in 0..20 {
        manager.add_session(format!("session-{}", i)).await.unwrap();
    }
    
    for i in 0..20 {
        if i % 2 == 0 {
            manager.remove_session(&format!("session-{}", i)).await.unwrap();
        }
    }
    
    // Trigger defragmentation
    manager.defragment().await.unwrap();
    
    let stats = manager.stats().await;
    assert_eq!(stats.total_sessions, 10);
    assert!(stats.fragmentation_ratio < 0.2);
}

#[tokio::test]
async fn test_session_pool_exhaustion() {
    let mut pool = SessionPool::new(2);
    
    let session1 = pool.acquire().await.unwrap();
    let session2 = pool.acquire().await.unwrap();
    
    // Pool exhausted, should timeout
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        pool.acquire()
    ).await;
    
    assert!(result.is_err()); // Timeout
    
    pool.release(session1).await;
    
    // Should succeed now
    let session3 = pool.acquire().await.unwrap();
    assert!(session3.id().len() > 0);
}

#[tokio::test]
async fn test_memory_cleanup_on_drop() {
    let config = MemoryConfig::default();
    
    {
        let manager = MemoryManager::new(config);
        manager.add_session("temp-session".to_string()).await.unwrap();
        
        let stats = manager.stats().await;
        assert_eq!(stats.total_sessions, 1);
        // manager drops here
    }
    
    // Memory should be cleaned up after drop
    // (In real implementation, would verify via system metrics)
}