// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use crate::inference::ChatTemplate;
use crate::job_processor::Message;

/// Build a prompt with conversation context
///
/// Uses MODEL_CHAT_TEMPLATE environment variable to select template.
/// Supported values: default, llama2, vicuna, harmony, chatml
/// Defaults to "harmony" if not set (for GPT-OSS-20B compatibility)
///
/// Note: Chat template markers are automatically stripped from message content
/// to prevent double-formatting issues when SDK/client pre-formats messages.
pub fn build_prompt_with_context(context: &[Message], prompt: &str) -> String {
    // Get template from environment variable
    let template_name =
        std::env::var("MODEL_CHAT_TEMPLATE").unwrap_or_else(|_| "harmony".to_string());

    let template = ChatTemplate::from_str(&template_name).unwrap_or(ChatTemplate::Harmony); // Default to Harmony for GPT-OSS-20B

    // Take last 10 messages maximum
    let recent_context = if context.len() > 10 {
        &context[context.len() - 10..]
    } else {
        context
    };

    // Build message list for template, stripping any pre-existing chat markers
    // This prevents double-formatting when SDK/client pre-formats messages
    let mut messages: Vec<(String, String)> = recent_context
        .iter()
        .map(|msg| {
            let cleaned_content = strip_chat_template_markers(&msg.content);
            if cleaned_content != msg.content {
                tracing::debug!(
                    "🔧 Stripped chat markers from context message (role: {}, original len: {}, cleaned len: {})",
                    msg.role, msg.content.len(), cleaned_content.len()
                );
            }
            (msg.role.clone(), cleaned_content)
        })
        .collect();

    // Add current prompt as user message, also stripping any pre-existing markers
    let cleaned_prompt = strip_chat_template_markers(prompt);
    if cleaned_prompt != prompt {
        tracing::debug!(
            "🔧 Stripped chat markers from prompt (original len: {}, cleaned len: {})",
            prompt.len(),
            cleaned_prompt.len()
        );
    }
    messages.push(("user".to_string(), cleaned_prompt));

    // Format using template
    let formatted = template.format_messages(&messages);

    tracing::debug!(
        "🎨 Formatted prompt using {} template (context: {} messages, {} chars)",
        template.as_str(),
        recent_context.len(),
        formatted.len()
    );

    formatted
}

/// Strip chat template markers from content to prevent double-formatting
///
/// Handles common chat template formats:
/// - Harmony: `<|start|>role<|message|>content<|end|>` → `content`
/// - ChatML: `<|im_start|>role\ncontent<|im_end|>` → `content`
/// - Llama2: `[INST] content [/INST]` → `content`
fn strip_chat_template_markers(content: &str) -> String {
    let mut result = content.to_string();

    // Harmony format: <|start|>role<|message|>content<|end|>
    // We need to extract just the content part
    if result.contains("<|start|>") || result.contains("<|message|>") || result.contains("<|end|>")
    {
        // Remove complete Harmony message wrappers
        // Pattern: <|start|>user<|message|>...<|end|> or <|start|>assistant<|message|>...<|end|>
        let harmony_patterns = [
            "<|start|>user<|message|>",
            "<|start|>assistant<|message|>",
            "<|start|>system<|message|>",
            "<|start|>assistant<|channel|>final<|message|>",
            "<|start|>assistant<|channel|>analysis<|message|>",
            "<|start|>assistant<|channel|>commentary<|message|>",
            "<|end|>",
            "<|start|>",
            "<|message|>",
            "<|channel|>final",
            "<|channel|>analysis",
            "<|channel|>commentary",
        ];

        for pattern in harmony_patterns {
            result = result.replace(pattern, "");
        }

        // Also handle partial patterns like just "user" or "assistant" left over
        // after stripping markers (e.g., "<|start|>user<|message|>" leaves nothing)
        result = result.trim().to_string();
    }

    // ChatML format: <|im_start|>role\ncontent<|im_end|>
    if result.contains("<|im_start|>") || result.contains("<|im_end|>") {
        let chatml_patterns = [
            "<|im_start|>user\n",
            "<|im_start|>assistant\n",
            "<|im_start|>system\n",
            "<|im_start|>user",
            "<|im_start|>assistant",
            "<|im_start|>system",
            "<|im_end|>",
            "<|im_start|>",
        ];

        for pattern in chatml_patterns {
            result = result.replace(pattern, "");
        }
        result = result.trim().to_string();
    }

    // GLM-4 format: <|system|>\ncontent<|user|>\ncontent<|assistant|>\n
    if result.contains("<|system|>")
        || result.contains("<|user|>")
        || result.contains("<|observation|>")
    {
        let glm4_patterns = [
            "<|system|>\n",
            "<|user|>\n",
            "<|assistant|>\n",
            "<|observation|>\n",
            "<|system|>",
            "<|user|>",
            "<|assistant|>",
            "<|observation|>",
        ];
        for pattern in glm4_patterns {
            result = result.replace(pattern, "");
        }
        result = result.trim().to_string();
    }

    // Llama2 format: [INST] content [/INST]
    if result.contains("[INST]") || result.contains("[/INST]") {
        let llama2_patterns = ["[INST]", "[/INST]", "<<SYS>>", "<</SYS>>"];

        for pattern in llama2_patterns {
            result = result.replace(pattern, "");
        }
        result = result.trim().to_string();
    }

    result
}

/// Check if a prompt contains chat template markers
fn is_prompt_already_formatted(prompt: &str) -> bool {
    // Harmony format markers (GPT-OSS-20B)
    let has_harmony = prompt.contains("<|start|>")
        || prompt.contains("<|message|>")
        || prompt.contains("<|end|>");

    // ChatML format markers
    let has_chatml = prompt.contains("<|im_start|>") || prompt.contains("<|im_end|>");

    // Llama2 format markers
    let has_llama2 = prompt.contains("[INST]") || prompt.contains("[/INST]");

    // GLM-4 format markers
    let has_glm4 = prompt.contains("<|system|>") && prompt.contains("<|user|>");

    has_harmony || has_chatml || has_llama2 || has_glm4
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

    #[test]
    fn test_is_prompt_already_formatted_harmony() {
        // Harmony format markers
        assert!(is_prompt_already_formatted(
            "<|start|>user<|message|>Hello<|end|>"
        ));
        assert!(is_prompt_already_formatted(
            "Some text with <|message|> in it"
        ));
        assert!(is_prompt_already_formatted("Ending with <|end|>"));
    }

    #[test]
    fn test_is_prompt_already_formatted_chatml() {
        // ChatML format markers
        assert!(is_prompt_already_formatted(
            "<|im_start|>user\nHello<|im_end|>"
        ));
        assert!(is_prompt_already_formatted("Text with <|im_end|> marker"));
    }

    #[test]
    fn test_is_prompt_already_formatted_llama2() {
        // Llama2 format markers
        assert!(is_prompt_already_formatted("[INST] Hello [/INST]"));
        assert!(is_prompt_already_formatted("Text with [INST] marker"));
    }

    #[test]
    fn test_is_prompt_not_formatted() {
        // Plain text should not be considered formatted
        assert!(!is_prompt_already_formatted("Hello, how are you?"));
        assert!(!is_prompt_already_formatted("What is 2+2?"));
        assert!(!is_prompt_already_formatted("Tell me about Iron Man movie"));
    }

    #[test]
    fn test_strip_harmony_markers() {
        // Test stripping Harmony format markers
        let input = "<|start|>user<|message|>What is 2+2?<|end|>";
        let result = strip_chat_template_markers(input);
        assert_eq!(result, "What is 2+2?");

        // Test with channel markers
        let input2 = "<|start|>assistant<|channel|>final<|message|>Hello world<|end|>";
        let result2 = strip_chat_template_markers(input2);
        assert_eq!(result2, "Hello world");
    }

    #[test]
    fn test_strip_chatml_markers() {
        // Test stripping ChatML format markers
        let input = "<|im_start|>user\nWhat is 2+2?<|im_end|>";
        let result = strip_chat_template_markers(input);
        assert_eq!(result, "What is 2+2?");
    }

    #[test]
    fn test_strip_llama2_markers() {
        // Test stripping Llama2 format markers
        let input = "[INST] What is 2+2? [/INST]";
        let result = strip_chat_template_markers(input);
        assert_eq!(result, "What is 2+2?");
    }

    #[test]
    fn test_strip_glm4_markers() {
        let input = "<|system|>\nYou are helpful.\n<|user|>\nWhat is 2+2?\n<|assistant|>\n";
        let result = strip_chat_template_markers(input);
        assert_eq!(result, "You are helpful.\nWhat is 2+2?");
    }

    #[test]
    fn test_strip_glm4_preserves_content() {
        let input = "<|user|>\nHello world\n";
        let result = strip_chat_template_markers(input);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_strip_glm4_no_false_positives() {
        // Plain text mentioning user should NOT be stripped
        let input = "The user asked a question";
        let result = strip_chat_template_markers(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_is_formatted_glm4() {
        assert!(is_prompt_already_formatted(
            "<|system|>\nYou are helpful.\n<|user|>\nHello\n"
        ));
    }

    #[test]
    fn test_is_formatted_glm4_negative() {
        // Just <|user|> alone should NOT trigger (needs both <|system|> and <|user|>)
        assert!(!is_prompt_already_formatted("The <|user|> typed something"));
    }

    #[test]
    fn test_strip_preserves_plain_text() {
        // Plain text should remain unchanged
        let input = "What is the plot of Iron Man?";
        let result = strip_chat_template_markers(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_build_prompt_strips_and_reformats() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");

        let context = vec![];
        // SDK sends pre-formatted prompt
        let pre_formatted_prompt = "<|start|>user<|message|>What is 2+2?<|end|>";

        let result = build_prompt_with_context(&context, pre_formatted_prompt);

        // Should strip markers and reformat properly
        assert!(result.contains("<|start|>user<|message|>What is 2+2?<|end|>"));
        // Verify it does NOT have double formatting
        assert!(!result.contains("<|start|>user<|message|><|start|>user<|message|>"));
    }

    #[test]
    fn test_build_prompt_strips_context_messages() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");

        // Context contains pre-formatted message (simulating SDK behavior)
        let context = vec![Message {
            role: "user".to_string(),
            content: "<|start|>user<|message|>Previous question<|end|>".to_string(),
            timestamp: None,
        }];
        let prompt = "Follow-up question";

        let result = build_prompt_with_context(&context, prompt);

        // Should contain properly formatted messages without double markers
        assert!(result.contains("Previous question"));
        assert!(result.contains("Follow-up question"));
        // Verify no double formatting
        assert!(!result.contains("<|start|>user<|message|><|start|>user<|message|>"));
        // Count occurrences of user message marker - should be exactly 2 (one for each message)
        let user_marker_count = result.matches("<|start|>user<|message|>").count();
        assert_eq!(
            user_marker_count, 2,
            "Expected exactly 2 user message markers, got {}",
            user_marker_count
        );
    }

    #[test]
    fn test_build_prompt_formats_plain_text() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");

        let context = vec![];
        let plain_prompt = "What is the plot of Iron Man?";

        let result = build_prompt_with_context(&context, plain_prompt);

        // Should format with Harmony template
        assert!(result.contains("<|start|>user<|message|>"));
        assert!(result.contains("What is the plot of Iron Man?"));
        assert!(result.contains("<|end|>"));
        // Verify it does NOT have double user prefix
        assert!(!result.contains("<|start|>user<|message|><|start|>user<|message|>"));
    }
}
