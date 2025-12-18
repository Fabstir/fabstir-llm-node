// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use crate::inference::ChatTemplate;
use crate::job_processor::Message;

/// Build a prompt with conversation context
///
/// Uses MODEL_CHAT_TEMPLATE environment variable to select template.
/// Supported values: default, llama2, vicuna, harmony, chatml
/// Defaults to "harmony" if not set (for GPT-OSS-20B compatibility)
pub fn build_prompt_with_context(context: &[Message], prompt: &str) -> String {
    // Get template from environment variable
    let template_name = std::env::var("MODEL_CHAT_TEMPLATE")
        .unwrap_or_else(|_| "harmony".to_string());

    let template = ChatTemplate::from_str(&template_name)
        .unwrap_or(ChatTemplate::Harmony); // Default to Harmony for GPT-OSS-20B

    // Take last 10 messages maximum
    let recent_context = if context.len() > 10 {
        &context[context.len() - 10..]
    } else {
        context
    };

    // Build message list for template
    let mut messages: Vec<(String, String)> = recent_context
        .iter()
        .map(|msg| (msg.role.clone(), msg.content.clone()))
        .collect();

    // Add current prompt as user message
    messages.push(("user".to_string(), prompt.to_string()));

    // Format using template
    let formatted = template.format_messages(&messages);

    tracing::debug!(
        "🎨 Formatted prompt using {} template (context: {} messages):\n{}",
        template.as_str(),
        recent_context.len(),
        formatted
    );

    formatted
}

/// Estimate token count for context
pub fn count_context_tokens(context: &[Message]) -> usize {
    context
        .iter()
        .map(|msg| (msg.content.len() + msg.role.len()) / 4)
        .sum()
}

/// Check if context is within limits
pub fn is_context_within_limits(context: &[Message], max_tokens: usize) -> bool {
    count_context_tokens(context) <= max_tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_prompt_with_empty_context() {
        // Set template to default for predictable test output
        std::env::set_var("MODEL_CHAT_TEMPLATE", "default");

        let context = vec![];
        let prompt = "Hello";
        let result = build_prompt_with_context(&context, prompt);

        // Should contain user message and assistant prompt
        assert!(result.contains("Hello"));
        assert!(result.contains("Assistant:"));
    }

    #[test]
    fn test_build_prompt_harmony_format() {
        // Set template to harmony
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");

        let context = vec![];
        let prompt = "Hello";
        let result = build_prompt_with_context(&context, prompt);

        // Should use Harmony format (GPT-OSS-20B compatible)
        // Harmony uses: <|start|>role<|message|>content<|end|>
        assert!(result.contains("<|start|>user<|message|>"));
        assert!(result.contains("Hello"));
        assert!(result.contains("<|end|>"));
        assert!(result.contains("<|start|>assistant"));
    }

    #[test]
    fn test_token_counting() {
        let context = vec![Message {
            role: "user".to_string(),
            content: "test message".to_string(),
            timestamp: None,
        }];
        let tokens = count_context_tokens(&context);
        assert!(tokens > 0);
    }
}
