use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Configuration for chain-specific rate limiting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainRateLimitConfig {
    pub chain_id: u64,
    pub requests_per_minute: usize,
    pub burst_size: usize,
    pub per_ip_limit: bool,
    pub per_session_limit: bool,
}

impl ChainRateLimitConfig {
    /// Create config for Base Sepolia
    pub fn base_sepolia() -> Self {
        Self {
            chain_id: 84532,
            requests_per_minute: 600,
            burst_size: 100,
            per_ip_limit: true,
            per_session_limit: false,
        }
    }

    /// Create config for opBNB Testnet
    pub fn opbnb_testnet() -> Self {
        Self {
            chain_id: 5611,
            requests_per_minute: 300,
            burst_size: 50,
            per_ip_limit: true,
            per_session_limit: false,
        }
    }
}

/// Token bucket for rate limiting
#[derive(Debug, Clone)]
struct TokenBucket {
    capacity: usize,
    tokens: usize,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: usize, requests_per_minute: usize) -> Self {
        Self {
            capacity,
            tokens: capacity,
            refill_rate: requests_per_minute as f64 / 60.0,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self, tokens: usize) -> bool {
        self.refill();

        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let new_tokens = (elapsed * self.refill_rate) as usize;

        if new_tokens > 0 {
            self.tokens = (self.tokens + new_tokens).min(self.capacity);
            self.last_refill = now;
        }
    }

    fn reset(&mut self) {
        self.tokens = self.capacity;
        self.last_refill = Instant::now();
    }

    fn time_until_available(&self, tokens: usize) -> Duration {
        if self.tokens >= tokens {
            Duration::from_secs(0)
        } else {
            let needed = tokens - self.tokens;
            let seconds = needed as f64 / self.refill_rate;
            Duration::from_secs_f64(seconds)
        }
    }
}

/// Rate limit error
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("Rate limit exceeded for chain {chain_id}. Retry after {retry_after:?}")]
    RateLimitExceeded {
        chain_id: u64,
        retry_after: Duration,
    },

    #[error("Chain {0} not configured")]
    ChainNotConfigured(u64),
}

/// Per-chain rate limiter
struct SingleChainRateLimiter {
    config: ChainRateLimitConfig,
    // IP -> TokenBucket
    ip_buckets: Arc<RwLock<HashMap<String, TokenBucket>>>,
    // Session -> TokenBucket
    session_buckets: Arc<RwLock<HashMap<String, TokenBucket>>>,
    // Global bucket for the chain
    global_bucket: Arc<RwLock<TokenBucket>>,
}

impl SingleChainRateLimiter {
    fn new(config: ChainRateLimitConfig) -> Self {
        let global_bucket = TokenBucket::new(
            config.burst_size * 10, // Global has higher capacity
            config.requests_per_minute * 10,
        );

        Self {
            config,
            ip_buckets: Arc::new(RwLock::new(HashMap::new())),
            session_buckets: Arc::new(RwLock::new(HashMap::new())),
            global_bucket: Arc::new(RwLock::new(global_bucket)),
        }
    }

    async fn check_rate_limit(&self, identifier: &str, is_ip: bool) -> Result<(), RateLimitError> {
        // Check global rate limit first
        let mut global = self.global_bucket.write().await;
        if !global.try_consume(1) {
            let retry_after = global.time_until_available(1);
            return Err(RateLimitError::RateLimitExceeded {
                chain_id: self.config.chain_id,
                retry_after,
            });
        }
        drop(global);

        // Check per-IP or per-session limit
        if is_ip && self.config.per_ip_limit {
            let mut ip_buckets = self.ip_buckets.write().await;
            let bucket = ip_buckets.entry(identifier.to_string()).or_insert_with(|| {
                TokenBucket::new(self.config.burst_size, self.config.requests_per_minute)
            });

            if !bucket.try_consume(1) {
                let retry_after = bucket.time_until_available(1);
                return Err(RateLimitError::RateLimitExceeded {
                    chain_id: self.config.chain_id,
                    retry_after,
                });
            }
        } else if !is_ip && self.config.per_session_limit {
            let mut session_buckets = self.session_buckets.write().await;
            let bucket = session_buckets
                .entry(identifier.to_string())
                .or_insert_with(|| {
                    TokenBucket::new(self.config.burst_size, self.config.requests_per_minute)
                });

            if !bucket.try_consume(1) {
                let retry_after = bucket.time_until_available(1);
                return Err(RateLimitError::RateLimitExceeded {
                    chain_id: self.config.chain_id,
                    retry_after,
                });
            }
        }

        Ok(())
    }

    async fn reset(&self) {
        self.global_bucket.write().await.reset();
        self.ip_buckets.write().await.clear();
        self.session_buckets.write().await.clear();
    }

    async fn cleanup_old_buckets(&self) {
        // Remove buckets that haven't been used in 5 minutes
        let cutoff = Instant::now() - Duration::from_secs(300);

        let mut ip_buckets = self.ip_buckets.write().await;
        ip_buckets.retain(|_, bucket| bucket.last_refill > cutoff);

        let mut session_buckets = self.session_buckets.write().await;
        session_buckets.retain(|_, bucket| bucket.last_refill > cutoff);
    }
}

/// Multi-chain rate limiter
pub struct ChainRateLimiter {
    limiters: Arc<RwLock<HashMap<u64, Arc<SingleChainRateLimiter>>>>,
    configs: Arc<RwLock<HashMap<u64, ChainRateLimitConfig>>>,
}

impl ChainRateLimiter {
    pub fn new() -> Self {
        Self {
            limiters: Arc::new(RwLock::new(HashMap::new())),
            configs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_chain_config(&self, config: ChainRateLimitConfig) {
        let chain_id = config.chain_id;
        self.configs.write().await.insert(chain_id, config.clone());

        let limiter = Arc::new(SingleChainRateLimiter::new(config));
        self.limiters.write().await.insert(chain_id, limiter);

        debug!("Added rate limiter for chain {}", chain_id);
    }

    pub async fn check_rate_limit(&self, chain_id: u64, identifier: &str) -> Result<()> {
        self.check_rate_limit_with_type(chain_id, identifier, true)
            .await
    }

    pub async fn check_rate_limit_with_type(
        &self,
        chain_id: u64,
        identifier: &str,
        is_ip: bool,
    ) -> Result<()> {
        // Get or create limiter for chain
        let limiter = self.get_or_create_limiter(chain_id).await?;

        match limiter.check_rate_limit(identifier, is_ip).await {
            Ok(()) => Ok(()),
            Err(e) => {
                warn!("Rate limit exceeded on chain {}: {:?}", chain_id, e);
                Err(anyhow!("{}", e))
            }
        }
    }

    async fn get_or_create_limiter(&self, chain_id: u64) -> Result<Arc<SingleChainRateLimiter>> {
        // Check if limiter exists
        if let Some(limiter) = self.limiters.read().await.get(&chain_id) {
            return Ok(limiter.clone());
        }

        // Check if we have config for this chain
        let configs = self.configs.read().await;
        if let Some(config) = configs.get(&chain_id) {
            let limiter = Arc::new(SingleChainRateLimiter::new(config.clone()));
            drop(configs);

            self.limiters
                .write()
                .await
                .insert(chain_id, limiter.clone());
            debug!("Created new rate limiter for chain {}", chain_id);
            Ok(limiter)
        } else {
            // Use default config based on chain
            let config = match chain_id {
                84532 => ChainRateLimitConfig::base_sepolia(),
                5611 => ChainRateLimitConfig::opbnb_testnet(),
                _ => return Err(anyhow!("No rate limit config for chain {}", chain_id)),
            };

            drop(configs);

            let limiter = Arc::new(SingleChainRateLimiter::new(config.clone()));
            self.limiters
                .write()
                .await
                .insert(chain_id, limiter.clone());
            self.configs.write().await.insert(chain_id, config);
            debug!("Created default rate limiter for chain {}", chain_id);
            Ok(limiter)
        }
    }

    pub async fn reset_chain_limits(&self, chain_id: u64) {
        if let Some(limiter) = self.limiters.read().await.get(&chain_id) {
            limiter.reset().await;
            debug!("Reset rate limits for chain {}", chain_id);
        }
    }

    pub async fn reset_all_limits(&self) {
        let limiters = self.limiters.read().await;
        for limiter in limiters.values() {
            limiter.reset().await;
        }
        debug!("Reset rate limits for all chains");
    }

    pub async fn get_chain_limits(&self, chain_id: u64) -> Option<ChainRateLimitConfig> {
        self.configs.read().await.get(&chain_id).cloned()
    }

    pub async fn cleanup_old_buckets(&self) {
        let limiters = self.limiters.read().await;
        for limiter in limiters.values() {
            limiter.cleanup_old_buckets().await;
        }
    }

    pub async fn shutdown_chain(&self, chain_id: u64) -> Result<()> {
        if let Some(_) = self.limiters.write().await.remove(&chain_id) {
            self.configs.write().await.remove(&chain_id);
            debug!("Shut down rate limiter for chain {}", chain_id);
            Ok(())
        } else {
            Err(anyhow!("No rate limiter found for chain {}", chain_id))
        }
    }
}

impl Default for ChainRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_bucket() {
        let mut bucket = TokenBucket::new(10, 60);
        assert_eq!(bucket.tokens, 10);

        // Consume tokens
        assert!(bucket.try_consume(5));
        assert_eq!(bucket.tokens, 5);

        // Cannot consume more than available
        assert!(!bucket.try_consume(6));
        assert_eq!(bucket.tokens, 5);

        // Can consume exactly available
        assert!(bucket.try_consume(5));
        assert_eq!(bucket.tokens, 0);

        // Reset
        bucket.reset();
        assert_eq!(bucket.tokens, 10);
    }

    #[tokio::test]
    async fn test_chain_rate_limiter() {
        let limiter = ChainRateLimiter::new();

        // Add Base Sepolia config
        limiter
            .add_chain_config(ChainRateLimitConfig::base_sepolia())
            .await;

        // Should allow requests up to burst size
        for i in 0..100 {
            let result = limiter.check_rate_limit(84532, "192.168.1.1").await;
            assert!(result.is_ok(), "Request {} should succeed", i);
        }

        // 101st request should fail
        let result = limiter.check_rate_limit(84532, "192.168.1.1").await;
        assert!(result.is_err(), "Should hit rate limit after burst");

        // Different IP should work
        let result = limiter.check_rate_limit(84532, "192.168.1.2").await;
        assert!(result.is_ok(), "Different IP should have its own limit");

        // Reset limits
        limiter.reset_chain_limits(84532).await;
        let result = limiter.check_rate_limit(84532, "192.168.1.1").await;
        assert!(result.is_ok(), "Should work after reset");
    }

    #[tokio::test]
    async fn test_multi_chain_rate_limiting() {
        let limiter = ChainRateLimiter::new();

        // Add configs for both chains
        limiter
            .add_chain_config(ChainRateLimitConfig::base_sepolia())
            .await;
        limiter
            .add_chain_config(ChainRateLimitConfig::opbnb_testnet())
            .await;

        // Test Base Sepolia (burst size 100)
        for _ in 0..100 {
            limiter.check_rate_limit(84532, "test-ip").await.unwrap();
        }
        assert!(limiter.check_rate_limit(84532, "test-ip").await.is_err());

        // opBNB should still work with same IP (different chain)
        for _ in 0..50 {
            limiter.check_rate_limit(5611, "test-ip").await.unwrap();
        }
        assert!(limiter.check_rate_limit(5611, "test-ip").await.is_err());

        // Verify limits are independent
        limiter.reset_chain_limits(84532).await;
        assert!(limiter.check_rate_limit(84532, "test-ip").await.is_ok());
        assert!(limiter.check_rate_limit(5611, "test-ip").await.is_err());
    }
}
