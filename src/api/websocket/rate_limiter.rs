use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Rate limit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub requests_per_minute: usize,
    pub burst_size: usize,
    pub per_ip_limit: bool,
    pub per_session_limit: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            requests_per_minute: 600,
            burst_size: 100,
            per_ip_limit: true,
            per_session_limit: false,
        }
    }
}

/// Rate limit error types
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("Too many requests. Retry after {retry_after:?} (limit: {limit}/window: {window:?})")]
    TooManyRequests {
        retry_after: Duration,
        limit: usize,
        window: Duration,
    },

    #[error("Rate limiter error: {0}")]
    Internal(String),
}

/// Result type for rate limiting
pub type RateLimitResult<T> = std::result::Result<T, RateLimitError>;

/// Token bucket implementation for rate limiting
pub struct TokenBucket {
    capacity: usize,
    tokens: usize,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    pub fn new(capacity: usize, refill_duration: Duration) -> Self {
        let refill_rate = capacity as f64 / refill_duration.as_secs_f64();
        Self {
            capacity,
            tokens: capacity,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    pub fn try_consume(&mut self, tokens: usize) -> RateLimitResult<()> {
        self.refill();

        if self.tokens >= tokens {
            self.tokens -= tokens;
            Ok(())
        } else {
            let retry_after =
                Duration::from_secs_f64((tokens - self.tokens) as f64 / self.refill_rate);
            Err(RateLimitError::TooManyRequests {
                retry_after,
                limit: self.capacity,
                window: Duration::from_secs(60),
            })
        }
    }

    pub fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let tokens_to_add = (elapsed.as_secs_f64() * self.refill_rate) as usize;

        if tokens_to_add > 0 {
            self.tokens = (self.tokens + tokens_to_add).min(self.capacity);
            self.last_refill = now;
        }
    }

    pub fn available_tokens(&self) -> usize {
        self.tokens
    }
}

/// Sliding window implementation
pub struct SlidingWindow {
    window_duration: Duration,
    max_requests: usize,
    requests: VecDeque<Instant>,
}

impl SlidingWindow {
    pub fn new(window_duration: Duration, max_requests: usize) -> Self {
        Self {
            window_duration,
            max_requests,
            requests: VecDeque::new(),
        }
    }

    pub fn try_acquire(&mut self) -> RateLimitResult<()> {
        self.cleanup();

        if self.requests.len() >= self.max_requests {
            let oldest = self.requests.front().unwrap();
            let retry_after = self.window_duration - oldest.elapsed();

            Err(RateLimitError::TooManyRequests {
                retry_after,
                limit: self.max_requests,
                window: self.window_duration,
            })
        } else {
            self.requests.push_back(Instant::now());
            Ok(())
        }
    }

    fn cleanup(&mut self) {
        let cutoff = Instant::now() - self.window_duration;
        while let Some(&front) = self.requests.front() {
            if front < cutoff {
                self.requests.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn current_count(&self) -> usize {
        self.requests.len()
    }
}

/// Main rate limiter
pub struct RateLimiter {
    config: RateLimitConfig,
    ip_limiters: Arc<RwLock<HashMap<IpAddr, TokenBucket>>>,
    session_limiters: Arc<RwLock<HashMap<String, TokenBucket>>>,
    global_limiter: Arc<RwLock<Option<TokenBucket>>>,
    whitelist: Arc<RwLock<Vec<IpAddr>>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            ip_limiters: Arc::new(RwLock::new(HashMap::new())),
            session_limiters: Arc::new(RwLock::new(HashMap::new())),
            global_limiter: Arc::new(RwLock::new(None)),
            whitelist: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn with_global_limit(config: RateLimitConfig) -> Self {
        let mut limiter = Self::new(config.clone());
        let global = TokenBucket::new(config.burst_size, Duration::from_secs(60));

        let global_limiter = limiter.global_limiter.clone();
        tokio::spawn(async move {
            *global_limiter.write().await = Some(global);
        });

        limiter
    }

    pub async fn with_redis(_url: &str) -> Result<Self> {
        // Mock implementation for testing
        Ok(Self::new(RateLimitConfig::default()))
    }

    pub async fn check_ip(&self, ip: &IpAddr) -> RateLimitResult<()> {
        // Check whitelist
        if self.whitelist.read().await.contains(ip) {
            return Ok(());
        }

        if !self.config.enabled || !self.config.per_ip_limit {
            return Ok(());
        }

        // Check global limit first
        if let Some(global) = self.global_limiter.write().await.as_mut() {
            global.try_consume(1)?;
        }

        // Check per-IP limit
        let mut limiters = self.ip_limiters.write().await;
        let limiter = limiters
            .entry(*ip)
            .or_insert_with(|| TokenBucket::new(self.config.burst_size, Duration::from_secs(60)));

        limiter.try_consume(1)
    }

    pub async fn check_session(&self, session_id: &str) -> RateLimitResult<()> {
        if !self.config.enabled || !self.config.per_session_limit {
            return Ok(());
        }

        let mut limiters = self.session_limiters.write().await;
        let limiter = limiters
            .entry(session_id.to_string())
            .or_insert_with(|| TokenBucket::new(self.config.burst_size, Duration::from_secs(60)));

        limiter.try_consume(1)
    }

    pub async fn add_whitelist(&self, ip: &IpAddr) {
        self.whitelist.write().await.push(*ip);
    }

    pub async fn get_headers(&self, ip: &IpAddr) -> HashMap<String, String> {
        let mut headers = HashMap::new();

        headers.insert(
            "X-RateLimit-Limit".to_string(),
            self.config.requests_per_minute.to_string(),
        );

        if let Some(limiter) = self.ip_limiters.read().await.get(ip) {
            headers.insert(
                "X-RateLimit-Remaining".to_string(),
                limiter.available_tokens().to_string(),
            );
        } else {
            headers.insert(
                "X-RateLimit-Remaining".to_string(),
                self.config.burst_size.to_string(),
            );
        }

        headers.insert(
            "X-RateLimit-Reset".to_string(),
            (Instant::now() + Duration::from_secs(60))
                .elapsed()
                .as_secs()
                .to_string(),
        );

        headers
    }

    pub async fn get_request_count(&self, ip: &IpAddr) -> usize {
        if let Some(limiter) = self.ip_limiters.read().await.get(ip) {
            self.config.burst_size - limiter.available_tokens()
        } else {
            0
        }
    }

    pub async fn get_stats(&self) -> RateLimiterStats {
        RateLimiterStats {
            tracked_ips: self.ip_limiters.read().await.len(),
            tracked_sessions: self.session_limiters.read().await.len(),
        }
    }

    pub async fn cleanup_old_entries(&self, _age: Duration) {
        // Clear all entries for testing
        self.ip_limiters.write().await.clear();
        self.session_limiters.write().await.clear();
    }
}

#[derive(Debug)]
pub struct RateLimiterStats {
    pub tracked_ips: usize,
    pub tracked_sessions: usize,
}
