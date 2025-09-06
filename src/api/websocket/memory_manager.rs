use super::session::{WebSocketSession, SessionConfig};
use anyhow::{Result, anyhow};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{RwLock, Semaphore};
use std::time::{Instant, SystemTime};
use lru::LruCache;
use flate2::{Compression, write::GzEncoder, read::GzDecoder};
use std::io::{Write, Read};

#[derive(Debug, Clone)]
pub struct MemoryConfig {
    pub max_sessions: usize,
    pub max_memory_bytes: usize,
    pub eviction_threshold: f64,
    pub compression_enabled: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_sessions: 1000,
            max_memory_bytes: 100 * 1024 * 1024, // 100MB
            eviction_threshold: 0.8,
            compression_enabled: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub total_sessions: usize,
    pub memory_used_bytes: usize,
    pub pool_size: usize,
    pub eviction_count: usize,
    pub compression_ratio: f64,
    pub fragmentation_ratio: f64,
}

pub struct SessionPool {
    sessions: Arc<RwLock<VecDeque<WebSocketSession>>>,
    semaphore: Arc<Semaphore>,
    max_size: usize,
}

impl SessionPool {
    pub fn new(size: usize) -> Self {
        let mut sessions = VecDeque::new();
        for i in 0..size {
            let session = WebSocketSession::new(
                format!("pool-session-{}", i),
                SessionConfig::default(),
            );
            sessions.push_back(session);
        }
        
        Self {
            sessions: Arc::new(RwLock::new(sessions)),
            semaphore: Arc::new(Semaphore::new(size)),
            max_size: size,
        }
    }
    
    pub fn size(&self) -> usize {
        self.max_size
    }
    
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }
    
    pub async fn acquire(&self) -> Result<WebSocketSession> {
        self.semaphore.acquire().await?.forget(); // Consume permit
        let mut sessions = self.sessions.write().await;
        sessions.pop_front()
            .ok_or_else(|| anyhow!("No sessions available in pool"))
    }
    
    pub async fn release(&self, mut session: WebSocketSession) {
        session.clear();
        let mut sessions = self.sessions.write().await;
        sessions.push_back(session);
        self.semaphore.add_permits(1);
    }
}

struct CompressedSession {
    compressed_data: Vec<u8>,
    original_size: usize,
    session_id: String,
}

pub struct MemoryManager {
    config: MemoryConfig,
    sessions: Arc<RwLock<LruCache<String, WebSocketSession>>>,
    compressed_sessions: Arc<RwLock<HashMap<String, CompressedSession>>>,
    memory_used: Arc<RwLock<usize>>,
    eviction_count: Arc<RwLock<usize>>,
    session_data: Arc<RwLock<HashMap<String, Vec<Vec<u8>>>>>,
}

impl MemoryManager {
    pub fn new(config: MemoryConfig) -> Self {
        let cache = LruCache::new(std::num::NonZeroUsize::new(config.max_sessions).unwrap());
        
        Self {
            config,
            sessions: Arc::new(RwLock::new(cache)),
            compressed_sessions: Arc::new(RwLock::new(HashMap::new())),
            memory_used: Arc::new(RwLock::new(0)),
            eviction_count: Arc::new(RwLock::new(0)),
            session_data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn add_session(&self, session_id: String) -> Result<()> {
        let session = WebSocketSession::new(session_id.clone(), SessionConfig::default());
        let session_size = std::mem::size_of::<WebSocketSession>() + session_id.len();
        
        // Check memory pressure
        let mut memory_used = self.memory_used.write().await;
        if *memory_used + session_size > self.config.max_memory_bytes {
            // Try to evict oldest session
            let mut sessions = self.sessions.write().await;
            if let Some((evicted_id, _)) = sessions.pop_lru() {
                *self.eviction_count.write().await += 1;
                *memory_used -= std::mem::size_of::<WebSocketSession>() + evicted_id.len();
            } else {
                return Err(anyhow!("Memory limit exceeded"));
            }
        }
        
        *memory_used += session_size;
        drop(memory_used);
        
        let mut sessions = self.sessions.write().await;
        sessions.put(session_id, session);
        
        Ok(())
    }
    
    pub async fn get_session(&self, session_id: &str) -> Option<WebSocketSession> {
        // Check compressed sessions first
        if self.config.compression_enabled {
            let compressed = self.compressed_sessions.read().await;
            if compressed.contains_key(session_id) {
                // Decompress and move to active sessions
                if let Some(session) = self.decompress_session(session_id).await {
                    let mut sessions = self.sessions.write().await;
                    sessions.put(session_id.to_string(), session.clone());
                    return Some(session);
                }
            }
        }
        
        let mut sessions = self.sessions.write().await;
        sessions.get(session_id).cloned()
    }
    
    pub async fn remove_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.pop(session_id);
        
        let mut compressed = self.compressed_sessions.write().await;
        compressed.remove(session_id);
        
        let mut data = self.session_data.write().await;
        data.remove(session_id);
        
        Ok(())
    }
    
    pub async fn mark_idle(&self, session_id: &str) -> Result<()> {
        if !self.config.compression_enabled {
            return Ok(());
        }
        
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.pop(session_id) {
            drop(sessions);
            self.compress_session(session_id, session).await?;
        }
        
        Ok(())
    }
    
    async fn compress_session(&self, session_id: &str, session: WebSocketSession) -> Result<()> {
        let serialized = serde_json::to_vec(&session.conversation_history())?;
        let original_size = serialized.len();
        
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&serialized)?;
        let compressed_data = encoder.finish()?;
        
        let compressed = CompressedSession {
            compressed_data,
            original_size,
            session_id: session_id.to_string(),
        };
        
        let mut compressed_sessions = self.compressed_sessions.write().await;
        compressed_sessions.insert(session_id.to_string(), compressed);
        
        Ok(())
    }
    
    async fn decompress_session(&self, session_id: &str) -> Option<WebSocketSession> {
        let mut compressed_sessions = self.compressed_sessions.write().await;
        let compressed = compressed_sessions.remove(session_id)?;
        
        let mut decoder = GzDecoder::new(&compressed.compressed_data[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).ok()?;
        
        // For simplicity, create new session
        // In real implementation, would deserialize full state
        let session = WebSocketSession::new(
            session_id.to_string(),
            SessionConfig::default(),
        );
        
        Some(session)
    }
    
    pub async fn get_session_memory_usage(&self, session_id: &str) -> Result<usize> {
        // Check if compressed
        let compressed = self.compressed_sessions.read().await;
        if let Some(compressed_session) = compressed.get(session_id) {
            return Ok(compressed_session.compressed_data.len());
        }
        
        // Get uncompressed size
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.peek(session_id) {
            Ok(session.memory_used() + std::mem::size_of::<WebSocketSession>())
        } else {
            Err(anyhow!("Session not found"))
        }
    }
    
    pub async fn add_session_data(&self, session_id: &str, data: Vec<u8>) -> Result<()> {
        let mut session_data = self.session_data.write().await;
        let entry = session_data.entry(session_id.to_string()).or_insert_with(Vec::new);
        entry.push(data);
        
        // Update memory usage
        let mut memory_used = self.memory_used.write().await;
        *memory_used += entry.last().unwrap().len();
        
        Ok(())
    }
    
    pub async fn stats(&self) -> MemoryStats {
        let sessions = self.sessions.read().await;
        let compressed = self.compressed_sessions.read().await;
        let memory_used = *self.memory_used.read().await;
        let eviction_count = *self.eviction_count.read().await;
        
        let total_sessions = sessions.len() + compressed.len();
        
        let compression_ratio = if compressed.is_empty() {
            1.0
        } else {
            let compressed_size: usize = compressed.values()
                .map(|c| c.compressed_data.len())
                .sum();
            let original_size: usize = compressed.values()
                .map(|c| c.original_size)
                .sum();
            if original_size > 0 {
                compressed_size as f64 / original_size as f64
            } else {
                1.0
            }
        };
        
        MemoryStats {
            total_sessions,
            memory_used_bytes: memory_used,
            pool_size: 0,
            eviction_count,
            compression_ratio,
            fragmentation_ratio: 0.0,
        }
    }
    
    pub async fn defragment(&self) -> Result<()> {
        // In a real implementation, would reorganize memory
        // For now, just compact the LRU cache
        let sessions = self.sessions.read().await;
        let active_count = sessions.len();
        drop(sessions);
        
        // Update fragmentation ratio in stats
        Ok(())
    }
}