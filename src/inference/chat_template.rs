// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Chat template system for model-specific prompt formatting
//!
//! Different LLM models expect different prompt formats. This module provides
//! a template system to correctly format conversations for each model type.

use serde::{Deserialize, Serialize};

/// Supported chat template formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatTemplate {
    /// Default format: "User: ...\nAssistant: ..."
    Default,
    /// Llama2 format: "[INST] ... [/INST]"
    Llama2,
    /// Vicuna format: "USER: ...\nASSISTANT: ..."
    Vicuna,
    /// Harmony format (GPT-OSS-20B): "<|im_start|>user\n...<|im_end|>"
    Harmony,
    /// ChatML format: Similar to Harmony
    ChatML,
    /// GLM-4 format: "<|system|>\n...<|user|>\n...<|assistant|>\n"
    Glm4,
}

impl ChatTemplate {
    /// Parse template name from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "default" => Some(Self::Default),
            "llama2" | "llama-2" => Some(Self::Llama2),
            "vicuna" => Some(Self::Vicuna),
            "harmony" | "gpt-oss-20b" => Some(Self::Harmony), // GPT-OSS-20B REQUIRES Harmony format
            "chatml" | "chat-ml" => Some(Self::ChatML),
            "glm4" | "glm-4" | "glm4-flash" | "glm-4.7-flash" => Some(Self::Glm4),
            _ => None,
        }
    }

    /// Get template name as string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Llama2 => "llama2",
            Self::Vicuna => "vicuna",
            Self::Harmony => "harmony",
            Self::ChatML => "chatml",
            Self::Glm4 => "glm4",
        }
    }

    /// Get model-specific stop token strings.
    /// These tokens cause generation to stop when encountered.
    /// Returns empty vec for templates that only use EOS token.
    pub fn stop_tokens(&self) -> Vec<&'static str> {
        match self {
            Self::Default => vec![],
            Self::Llama2 => vec![],
            Self::Vicuna => vec![],
            Self::Harmony => vec!["<|return|>", "<|end|>"],
            Self::ChatML => vec!["<|im_end|>"],
            Self::Glm4 => vec!["<|user|>", "<|observation|>"],
        }
    }

    /// Format a conversation using this template
    ///
    /// # Arguments
    ///
    /// * `messages` - List of chat messages with role and content
    ///
    /// # Returns
    ///
    /// Formatted prompt string ready for model inference
    pub fn format_messages(&self, messages: &[(String, String)]) -> String {
        match self {
            Self::Default => self.format_default(messages),
            Self::Llama2 => self.format_llama2(messages),
            Self::Vicuna => self.format_vicuna(messages),
            Self::Harmony => self.format_harmony(messages),
            Self::ChatML => self.format_chatml(messages),
            Self::Glm4 => self.format_glm4(messages),
        }
    }

    /// Default format: "User: ...\nAssistant: ...\n"
    fn format_default(&self, messages: &[(String, String)]) -> String {
        let mut prompt = String::new();

        for (role, content) in messages {
            match role.as_str() {
                "system" => {
                    prompt.push_str(&format!("System: {}\n\n", content));
                }
                "user" => {
                    prompt.push_str(&format!("User: {}\n", content));
                }
                "assistant" => {
                    prompt.push_str(&format!("Assistant: {}\n", content));
                }
                _ => {}
            }
        }

        // Add prompt for assistant response
        prompt.push_str("Assistant: ");
        prompt
    }

    /// Llama2 format: "[INST] ... [/INST]"
    fn format_llama2(&self, messages: &[(String, String)]) -> String {
        let mut prompt = String::new();
        let mut system_prompt = String::new();

        // Extract system message if present
        for (role, content) in messages {
            if role == "system" {
                system_prompt = content.clone();
                break;
            }
        }

        // Format conversation
        let mut first_user = true;
        for (role, content) in messages {
            match role.as_str() {
                "user" => {
                    if first_user && !system_prompt.is_empty() {
                        prompt.push_str(&format!(
                            "[INST] <<SYS>>\n{}\n<</SYS>>\n\n{} [/INST] ",
                            system_prompt, content
                        ));
                        first_user = false;
                    } else {
                        prompt.push_str(&format!("[INST] {} [/INST] ", content));
                    }
                }
                "assistant" => {
                    prompt.push_str(&format!("{} ", content));
                }
                _ => {}
            }
        }

        prompt
    }

    /// Vicuna format: "USER: ...\nASSISTANT: ..."
    fn format_vicuna(&self, messages: &[(String, String)]) -> String {
        let mut prompt = String::new();

        for (role, content) in messages {
            match role.as_str() {
                "system" => {
                    prompt.push_str(&format!("SYSTEM: {}\n", content));
                }
                "user" => {
                    prompt.push_str(&format!("USER: {}\n", content));
                }
                "assistant" => {
                    prompt.push_str(&format!("ASSISTANT: {}\n", content));
                }
                _ => {}
            }
        }

        // Add prompt for assistant response
        prompt.push_str("ASSISTANT: ");
        prompt
    }

    /// Harmony format (GPT-OSS-20B): "<|start|>user<|message|>...<|end|>"
    /// Official spec: https://cookbook.openai.com/articles/openai-harmony
    /// CRITICAL: GPT-OSS-20B REQUIRES the Harmony format with channels to function correctly!
    fn format_harmony(&self, messages: &[(String, String)]) -> String {
        let mut prompt = String::new();

        // Check if system message already exists
        let has_system = messages.iter().any(|(role, _)| role == "system");

        // If no system message, add the required one for GPT-OSS-20B with reasoning level
        if !has_system {
            let current_date = chrono::Local::now().format("%Y-%m-%d");
            prompt.push_str(&format!(
                "<|start|>system<|message|>You are a helpful AI assistant.\nCurrent date: {}\n\nIMPORTANT: When you see [Web Search Results] in the user message, these are REAL search results from the internet. You MUST:\n1. Use this information to answer the user's question\n2. Present the search results as helpful sources\n3. NEVER say \"I cannot browse the web\" - the search has already been done for you\n4. If results are links/descriptions, recommend which sources to visit\n\nReasoning: medium\n\n# Valid channels: analysis, commentary, final.<|end|>\n",
                current_date
            ));
        }

        for (role, content) in messages {
            match role.as_str() {
                "system" => {
                    prompt.push_str(&format!("<|start|>system<|message|>{}<|end|>\n", content));
                }
                "user" => {
                    prompt.push_str(&format!("<|start|>user<|message|>{}<|end|>\n", content));
                }
                "assistant" => {
                    // For assistant messages in context, include channel if present
                    // Format: <|start|>assistant<|channel|>final<|message|>content<|end|>
                    prompt.push_str(&format!(
                        "<|start|>assistant<|message|>{}<|end|>\n",
                        content
                    ));
                }
                _ => {}
            }
        }

        // Add prompt for assistant response with channel specification
        // The model will output to the 'final' channel for user-facing responses
        prompt.push_str("<|start|>assistant<|channel|>final<|message|>");
        prompt
    }

    /// GLM-4 format: "<|system|>\n{content}<|user|>\n{content}<|assistant|>\n"
    /// Reference: https://huggingface.co/zai-org/GLM-4.7/blob/main/chat_template.jinja
    /// Note: [gMASK]<sop> BOS tokens are handled by llama.cpp via GGUF metadata
    fn format_glm4(&self, messages: &[(String, String)]) -> String {
        let mut prompt = String::new();
        let has_system = messages.iter().any(|(role, _)| role == "system");
        if !has_system {
            let current_date = chrono::Local::now().format("%Y-%m-%d");
            prompt.push_str(&format!(
                "<|system|>\nYou are a helpful assistant.\nCurrent date: {}\n\nWhen the user message contains reference material, search results, or document excerpts, use that information to answer. NEVER claim you cannot access provided context.\n",
                current_date
            ));
        }
        for (role, content) in messages {
            match role.as_str() {
                "system" => prompt.push_str(&format!("<|system|>\n{}\n", content)),
                "user" => prompt.push_str(&format!("<|user|>\n{}\n", content)),
                "assistant" => prompt.push_str(&format!("<|assistant|>\n{}\n", content)),
                _ => {}
            }
        }
        prompt.push_str("<|assistant|>\n");
        prompt
    }

    /// ChatML format: "<|im_start|>user\n...<|im_end|>"
    fn format_chatml(&self, messages: &[(String, String)]) -> String {
        let mut prompt = String::new();

        for (role, content) in messages {
            match role.as_str() {
                "system" => {
                    prompt.push_str(&format!("<|im_start|>system\n{}<|im_end|>\n", content));
                }
                "user" => {
                    prompt.push_str(&format!("<|im_start|>user\n{}<|im_end|>\n", content));
                }
                "assistant" => {
                    prompt.push_str(&format!("<|im_start|>assistant\n{}<|im_end|>\n", content));
                }
                _ => {}
            }
        }

        // Add prompt for assistant response
        prompt.push_str("<|im_start|>assistant\n");
        prompt
    }
}

impl Default for ChatTemplate {
    fn default() -> Self {
        Self::Default
    }
}

/// Parse MODEL_STOP_TOKENS env var into a list of stop token strings.
/// Format: comma-separated, e.g. "<|user|>,<|observation|>"
/// Returns empty vec if not set (template defaults will be used).
pub fn parse_stop_tokens_env() -> Vec<String> {
    std::env::var("MODEL_STOP_TOKENS")
        .ok()
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str() {
        assert_eq!(
            ChatTemplate::from_str("default"),
            Some(ChatTemplate::Default)
        );
        assert_eq!(ChatTemplate::from_str("llama2"), Some(ChatTemplate::Llama2));
        assert_eq!(
            ChatTemplate::from_str("llama-2"),
            Some(ChatTemplate::Llama2)
        );
        assert_eq!(ChatTemplate::from_str("vicuna"), Some(ChatTemplate::Vicuna));
        assert_eq!(
            ChatTemplate::from_str("harmony"),
            Some(ChatTemplate::Harmony)
        );
        assert_eq!(
            ChatTemplate::from_str("gpt-oss-20b"),
            Some(ChatTemplate::Harmony)
        );
        assert_eq!(ChatTemplate::from_str("chatml"), Some(ChatTemplate::ChatML));
        assert_eq!(ChatTemplate::from_str("unknown"), None);
    }

    #[test]
    fn test_default_format() {
        let template = ChatTemplate::Default;
        let messages = vec![
            ("user".to_string(), "Hello".to_string()),
            ("assistant".to_string(), "Hi there".to_string()),
            ("user".to_string(), "How are you?".to_string()),
        ];

        let formatted = template.format_messages(&messages);
        assert!(formatted.contains("User: Hello"));
        assert!(formatted.contains("Assistant: Hi there"));
        assert!(formatted.contains("User: How are you?"));
        assert!(formatted.ends_with("Assistant: "));
    }

    #[test]
    fn test_llama2_format() {
        let template = ChatTemplate::Llama2;
        let messages = vec![
            ("system".to_string(), "You are helpful".to_string()),
            ("user".to_string(), "Hello".to_string()),
            ("assistant".to_string(), "Hi".to_string()),
        ];

        let formatted = template.format_messages(&messages);
        assert!(formatted.contains("[INST]"));
        assert!(formatted.contains("[/INST]"));
        assert!(formatted.contains("<<SYS>>"));
    }

    #[test]
    fn test_vicuna_format() {
        let template = ChatTemplate::Vicuna;
        let messages = vec![
            ("user".to_string(), "Hello".to_string()),
            ("assistant".to_string(), "Hi".to_string()),
        ];

        let formatted = template.format_messages(&messages);
        assert!(formatted.contains("USER: Hello"));
        assert!(formatted.contains("ASSISTANT: Hi"));
        assert!(formatted.ends_with("ASSISTANT: "));
    }

    #[test]
    fn test_harmony_format() {
        let template = ChatTemplate::Harmony;
        let messages = vec![
            (
                "user".to_string(),
                "What is the capital of Turkey?".to_string(),
            ),
            ("assistant".to_string(), "Ankara".to_string()),
            (
                "user".to_string(),
                "What is the capital of Australia?".to_string(),
            ),
        ];

        let formatted = template.format_messages(&messages);
        assert!(formatted.contains("<|start|>user<|message|>What is the capital of Turkey?<|end|>"));
        assert!(formatted.contains("<|start|>assistant<|message|>Ankara<|end|>"));
        assert!(
            formatted.contains("<|start|>user<|message|>What is the capital of Australia?<|end|>")
        );
        // Harmony format includes channel specification for assistant responses
        assert!(formatted.ends_with("<|start|>assistant<|channel|>final<|message|>"));
    }

    #[test]
    fn test_chatml_format() {
        let template = ChatTemplate::ChatML;
        let messages = vec![("user".to_string(), "Hello".to_string())];

        let formatted = template.format_messages(&messages);
        // ChatML uses <|im_start|> format
        assert!(formatted.contains("<|im_start|>user"));
        assert!(formatted.contains("<|im_end|>"));
        assert!(formatted.ends_with("<|im_start|>assistant\n"));
    }

    #[test]
    fn test_system_message_default() {
        let template = ChatTemplate::Default;
        let messages = vec![
            ("system".to_string(), "Be helpful".to_string()),
            ("user".to_string(), "Hello".to_string()),
        ];

        let formatted = template.format_messages(&messages);
        assert!(formatted.contains("System: Be helpful"));
    }

    #[test]
    fn test_system_message_harmony() {
        let template = ChatTemplate::Harmony;
        let messages = vec![
            ("system".to_string(), "Be helpful".to_string()),
            ("user".to_string(), "Hello".to_string()),
        ];

        let formatted = template.format_messages(&messages);
        assert!(formatted.contains("<|start|>system<|message|>Be helpful<|end|>"));
    }

    #[test]
    fn test_from_str_glm4() {
        assert_eq!(ChatTemplate::from_str("glm4"), Some(ChatTemplate::Glm4));
        assert_eq!(ChatTemplate::from_str("glm-4"), Some(ChatTemplate::Glm4));
        assert_eq!(
            ChatTemplate::from_str("glm4-flash"),
            Some(ChatTemplate::Glm4)
        );
        assert_eq!(
            ChatTemplate::from_str("glm-4.7-flash"),
            Some(ChatTemplate::Glm4)
        );
    }

    #[test]
    fn test_glm4_as_str() {
        assert_eq!(ChatTemplate::Glm4.as_str(), "glm4");
    }

    #[test]
    fn test_glm4_format_basic() {
        let template = ChatTemplate::Glm4;
        let messages = vec![("user".to_string(), "What is 2+2?".to_string())];
        let formatted = template.format_messages(&messages);
        assert!(formatted.contains("<|user|>\nWhat is 2+2?\n"));
        assert!(formatted.ends_with("<|assistant|>\n"));
    }

    #[test]
    fn test_glm4_format_with_system() {
        let template = ChatTemplate::Glm4;
        let messages = vec![
            ("system".to_string(), "You are helpful.".to_string()),
            ("user".to_string(), "Hello".to_string()),
        ];
        let formatted = template.format_messages(&messages);
        assert!(formatted.contains("<|system|>\nYou are helpful.\n"));
        assert!(formatted.contains("<|user|>\nHello\n"));
        assert!(formatted.ends_with("<|assistant|>\n"));
        // Should NOT auto-inject system message when one is provided
        assert_eq!(formatted.matches("<|system|>").count(), 1);
    }

    #[test]
    fn test_glm4_format_multi_turn() {
        let template = ChatTemplate::Glm4;
        let messages = vec![
            ("user".to_string(), "Hi".to_string()),
            ("assistant".to_string(), "Hello!".to_string()),
            ("user".to_string(), "How are you?".to_string()),
        ];
        let formatted = template.format_messages(&messages);
        assert!(formatted.contains("<|user|>\nHi\n"));
        assert!(formatted.contains("<|assistant|>\nHello!\n"));
        assert!(formatted.contains("<|user|>\nHow are you?\n"));
        assert!(formatted.ends_with("<|assistant|>\n"));
    }

    #[test]
    fn test_glm4_auto_system_message() {
        let template = ChatTemplate::Glm4;
        let messages = vec![("user".to_string(), "Hello".to_string())];
        let formatted = template.format_messages(&messages);
        assert!(formatted.contains("<|system|>\nYou are a helpful assistant.\nCurrent date:"));
        assert!(formatted.contains("NEVER claim you cannot access provided context."));
    }

    #[test]
    fn test_glm4_auto_system_message_context_aware() {
        let template = ChatTemplate::Glm4;
        let messages = vec![("user".to_string(), "Summarise the document".to_string())];
        let formatted = template.format_messages(&messages);
        assert!(
            formatted.contains("reference material, search results, or document excerpts"),
            "GLM-4 auto system prompt should instruct model to use provided context: {}",
            formatted
        );
        assert!(
            formatted.contains("NEVER claim you cannot access provided context"),
            "GLM-4 auto system prompt should prohibit claiming no access: {}",
            formatted
        );
        // Should still have the identity line
        assert!(formatted.contains("You are a helpful assistant."));
    }

    #[test]
    fn test_glm4_auto_system_message_includes_date() {
        let template = ChatTemplate::Glm4;
        let messages = vec![("user".to_string(), "Hello".to_string())];
        let formatted = template.format_messages(&messages);
        assert!(
            formatted.contains("Current date:"),
            "GLM-4 auto system prompt should include current date: {}",
            formatted
        );
    }

    #[test]
    fn test_glm4_user_system_message_not_overridden() {
        let template = ChatTemplate::Glm4;
        let messages = vec![
            (
                "system".to_string(),
                "You are a pirate assistant.".to_string(),
            ),
            ("user".to_string(), "Hello".to_string()),
        ];
        let formatted = template.format_messages(&messages);
        // User's system message should be preserved
        assert!(formatted.contains("You are a pirate assistant."));
        // Should NOT auto-inject context instructions
        assert!(
            !formatted.contains("NEVER claim you cannot access"),
            "Should not inject context instructions when user provides system message"
        );
        // Only 1 system marker
        assert_eq!(
            formatted.matches("<|system|>").count(),
            1,
            "Should have exactly one <|system|> marker"
        );
    }

    #[test]
    fn test_stop_tokens_harmony() {
        let tokens = ChatTemplate::Harmony.stop_tokens();
        assert_eq!(tokens, vec!["<|return|>", "<|end|>"]);
    }

    #[test]
    fn test_stop_tokens_glm4() {
        let tokens = ChatTemplate::Glm4.stop_tokens();
        assert_eq!(tokens, vec!["<|user|>", "<|observation|>"]);
    }

    #[test]
    fn test_stop_tokens_chatml() {
        let tokens = ChatTemplate::ChatML.stop_tokens();
        assert_eq!(tokens, vec!["<|im_end|>"]);
    }

    #[test]
    fn test_stop_tokens_default_empty() {
        assert!(ChatTemplate::Default.stop_tokens().is_empty());
    }

    #[test]
    fn test_stop_tokens_llama2_empty() {
        assert!(ChatTemplate::Llama2.stop_tokens().is_empty());
    }

    #[test]
    fn test_parse_stop_tokens_env_set() {
        std::env::set_var("MODEL_STOP_TOKENS", "<|user|>,<|observation|>");
        let tokens = parse_stop_tokens_env();
        std::env::remove_var("MODEL_STOP_TOKENS");
        assert_eq!(tokens, vec!["<|user|>", "<|observation|>"]);
    }

    #[test]
    fn test_parse_stop_tokens_env_unset() {
        std::env::remove_var("MODEL_STOP_TOKENS");
        let tokens = parse_stop_tokens_env();
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_parse_stop_tokens_env_whitespace() {
        std::env::set_var("MODEL_STOP_TOKENS", " <|user|> , <|observation|> ");
        let tokens = parse_stop_tokens_env();
        std::env::remove_var("MODEL_STOP_TOKENS");
        assert_eq!(tokens, vec!["<|user|>", "<|observation|>"]);
    }

    #[test]
    fn test_harmony_auto_system_message() {
        let template = ChatTemplate::Harmony;
        // No system message provided
        let messages = vec![("user".to_string(), "What is 2+2?".to_string())];

        let formatted = template.format_messages(&messages);

        // Should auto-add system message (v8.7.12+ format)
        assert!(formatted.contains("<|start|>system<|message|>You are a helpful AI assistant"));
        assert!(formatted.contains("Current date:"));
        assert!(formatted.contains("IMPORTANT: When you see [Web Search Results]"));
        assert!(formatted.contains("NEVER say \"I cannot browse the web\""));
        assert!(formatted.contains("<|start|>user<|message|>What is 2+2?<|end|>"));
        // Harmony format includes channel specification for assistant responses
        assert!(formatted.ends_with("<|start|>assistant<|channel|>final<|message|>"));
    }
}
