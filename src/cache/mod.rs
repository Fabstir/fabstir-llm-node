// Minimal cache module for tests
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct CacheConfig;

#[derive(Debug, Clone)]
pub struct CacheMetrics;

pub struct PromptCache;

impl PromptCache {
    pub fn new(_config: CacheConfig) -> Result<Self> {
        Ok(Self)
    }
}