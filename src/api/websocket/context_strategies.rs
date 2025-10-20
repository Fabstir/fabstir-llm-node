// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OverflowStrategy {
    Truncate,
    Summarize(SummarizationConfig),
    Dynamic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummarizationConfig {
    pub trigger_threshold: usize,
    pub target_reduction: f32,
    pub preserve_recent: usize,
}

impl Default for SummarizationConfig {
    fn default() -> Self {
        Self {
            trigger_threshold: 80,
            target_reduction: 0.5,
            preserve_recent: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompressionStrategy {
    None,
    Automatic,
    Manual,
    Adaptive,
}

impl Default for CompressionStrategy {
    fn default() -> Self {
        CompressionStrategy::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TruncationStrategy {
    pub keep_first: usize,
    pub keep_last: usize,
    pub preserve_system: bool,
}

impl Default for TruncationStrategy {
    fn default() -> Self {
        Self {
            keep_first: 2,
            keep_last: 10,
            preserve_system: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowStrategy {
    pub window_size: usize,
    pub step_size: usize,
    pub overlap: usize,
}

impl Default for WindowStrategy {
    fn default() -> Self {
        Self {
            window_size: 20,
            step_size: 15,
            overlap: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveStrategy {
    pub min_context: usize,
    pub max_context: usize,
    pub target_density: f32,
}

impl Default for AdaptiveStrategy {
    fn default() -> Self {
        Self {
            min_context: 100,
            max_context: 2000,
            target_density: 0.7,
        }
    }
}

pub trait ContextStrategy {
    fn apply(
        &self,
        messages: Vec<crate::job_processor::Message>,
        max_tokens: usize,
    ) -> Vec<crate::job_processor::Message>;
    fn estimate_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }
}

pub struct TruncateStrategy {
    config: TruncationStrategy,
}

impl TruncateStrategy {
    pub fn new(config: TruncationStrategy) -> Self {
        Self { config }
    }
}

impl ContextStrategy for TruncateStrategy {
    fn apply(
        &self,
        mut messages: Vec<crate::job_processor::Message>,
        max_tokens: usize,
    ) -> Vec<crate::job_processor::Message> {
        let total_tokens: usize = messages
            .iter()
            .map(|m| self.estimate_tokens(&format!("{}: {}", m.role, m.content)))
            .sum();

        if total_tokens <= max_tokens {
            return messages;
        }

        // Preserve system messages if configured
        let mut result = Vec::new();
        let mut used_tokens = 0;

        if self.config.preserve_system {
            for msg in &messages {
                if msg.role == "system" {
                    let tokens = self.estimate_tokens(&format!("{}: {}", msg.role, msg.content));
                    if used_tokens + tokens <= max_tokens {
                        result.push(msg.clone());
                        used_tokens += tokens;
                    }
                }
            }
        }

        // Keep first N messages
        let mut added_first = 0;
        for msg in &messages {
            if msg.role != "system" && added_first < self.config.keep_first {
                let tokens = self.estimate_tokens(&format!("{}: {}", msg.role, msg.content));
                if used_tokens + tokens <= max_tokens {
                    result.push(msg.clone());
                    used_tokens += tokens;
                    added_first += 1;
                }
            }
        }

        // Keep last N messages
        let start_idx = messages.len().saturating_sub(self.config.keep_last);
        for msg in &messages[start_idx..] {
            if !result
                .iter()
                .any(|m| m.content == msg.content && m.role == msg.role)
            {
                let tokens = self.estimate_tokens(&format!("{}: {}", msg.role, msg.content));
                if used_tokens + tokens <= max_tokens {
                    result.push(msg.clone());
                    used_tokens += tokens;
                }
            }
        }

        result
    }
}

pub struct SummarizeStrategy {
    config: SummarizationConfig,
}

impl SummarizeStrategy {
    pub fn new(config: SummarizationConfig) -> Self {
        Self { config }
    }

    pub fn create_summary(&self, messages: &[crate::job_processor::Message]) -> String {
        // In production, this would call an LLM to generate a summary
        // For now, return a simple summary
        format!(
            "Previous conversation summary: {} messages exchanged covering various topics.",
            messages.len()
        )
    }
}

impl ContextStrategy for SummarizeStrategy {
    fn apply(
        &self,
        messages: Vec<crate::job_processor::Message>,
        max_tokens: usize,
    ) -> Vec<crate::job_processor::Message> {
        let total_tokens: usize = messages
            .iter()
            .map(|m| self.estimate_tokens(&format!("{}: {}", m.role, m.content)))
            .sum();

        if total_tokens <= max_tokens {
            return messages;
        }

        // Check if we should trigger summarization
        let threshold = (max_tokens * self.config.trigger_threshold) / 100;
        if total_tokens < threshold {
            return messages;
        }

        // Split messages
        let preserve_count = self.config.preserve_recent;
        if messages.len() <= preserve_count {
            return messages;
        }

        let split_point = messages.len() - preserve_count;
        let to_summarize = &messages[..split_point];
        let to_preserve = &messages[split_point..];

        // Create summary
        let summary_text = self.create_summary(to_summarize);
        let summary_message = crate::job_processor::Message {
            role: "system".to_string(),
            content: format!("[Summary] {}", summary_text),
            timestamp: None,
        };

        // Combine summary with recent messages
        let mut result = vec![summary_message];
        result.extend_from_slice(to_preserve);

        result
    }
}

pub struct WindowingStrategy {
    config: WindowStrategy,
}

impl WindowingStrategy {
    pub fn new(config: WindowStrategy) -> Self {
        Self { config }
    }

    pub fn create_windows(
        &self,
        messages: &[crate::job_processor::Message],
    ) -> Vec<Vec<crate::job_processor::Message>> {
        let mut windows = Vec::new();
        let mut start = 0;

        while start < messages.len() {
            let end = std::cmp::min(start + self.config.window_size, messages.len());
            windows.push(messages[start..end].to_vec());

            if end >= messages.len() {
                break;
            }

            start += self.config.step_size;
        }

        windows
    }
}

impl ContextStrategy for WindowingStrategy {
    fn apply(
        &self,
        messages: Vec<crate::job_processor::Message>,
        max_tokens: usize,
    ) -> Vec<crate::job_processor::Message> {
        // Return the most recent window that fits within token limit
        let windows = self.create_windows(&messages);

        for window in windows.iter().rev() {
            let tokens: usize = window
                .iter()
                .map(|m| self.estimate_tokens(&format!("{}: {}", m.role, m.content)))
                .sum();

            if tokens <= max_tokens {
                return window.clone();
            }
        }

        // If no window fits, return empty or truncated version
        Vec::new()
    }
}
