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
pub fn build_prompt_with_context(
    context: &[Message],
    prompt: &str,
    thinking: Option<&str>,
) -> String {
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

    // Inject thinking directive if specified (v8.17.0+)
    // Returns Some(level) when Harmony post-processing is needed
    // v8.22.3: Removed GLM-4 auto-/think injection — causes degenerate meta-reasoning
    // loops on multi-turn conversations. /think must be explicitly requested via SDK.
    let effective_mode = resolve_thinking_mode(thinking);
    let post_process_level = if let Some(ref mode) = effective_mode {
        tracing::info!(
            "🧠 Injecting thinking directive: mode={}, template={}",
            mode,
            template.as_str()
        );
        inject_thinking_directive(&template, &mut messages, mode)
    } else {
        tracing::debug!(
            "🧠 No thinking mode specified (explicit={:?}, DEFAULT_THINKING_MODE not set)",
            thinking
        );
        None
    };

    // Format using template
    let mut formatted = template.format_messages(&messages);

    // Post-process: replace "Reasoning: medium" with desired level in the
    // formatted output. This preserves the full default system prompt
    // (AI identity, date, web search, Valid channels) while changing the level.
    if let Some(ref level) = post_process_level {
        if level == "none" {
            formatted = formatted.replace(
                "Reasoning: medium",
                "Reasoning: none\nProvide brief, direct answers without extensive analysis.",
            );
        } else if level != "medium" {
            formatted = formatted.replace("Reasoning: medium", &format!("Reasoning: {}", level));
        }
    }

    tracing::debug!(
        "🎨 Formatted prompt using {} template (context: {} messages, {} chars)",
        template.as_str(),
        recent_context.len(),
        formatted.len()
    );

    formatted
}

/// Resolve thinking mode: explicit value takes priority over env var default
///
/// Empty strings are treated as None (no thinking mode) to handle
/// docker-compose `${DEFAULT_THINKING_MODE:-}` resolving to "".
pub(crate) fn resolve_thinking_mode(explicit: Option<&str>) -> Option<String> {
    if let Some(mode) = explicit {
        if !mode.is_empty() {
            return Some(mode.to_string());
        }
        return None;
    }
    std::env::var("DEFAULT_THINKING_MODE")
        .ok()
        .filter(|s| !s.is_empty())
}

/// Dispatch thinking injection to the appropriate template handler
///
/// Returns `Some(level)` when Harmony post-processing is needed (no system
/// message exists, so the template's default must be preserved and the
/// `Reasoning: medium` line replaced after formatting).
pub(crate) fn inject_thinking_directive(
    template: &ChatTemplate,
    messages: &mut Vec<(String, String)>,
    thinking_mode: &str,
) -> Option<String> {
    match template {
        ChatTemplate::Harmony => inject_harmony_thinking(messages, thinking_mode),
        ChatTemplate::Glm4 => {
            inject_glm4_thinking(messages, thinking_mode);
            None
        }
        _ => {
            tracing::debug!("Thinking mode ignored for {} template", template.as_str());
            None
        }
    }
}

/// Inject Reasoning level into Harmony system message
///
/// Maps thinking values: enabled/medium → medium, disabled → none, low → low, high → high
/// If user already provided "Reasoning:" in a system message, skip injection.
/// If a system message exists without "Reasoning:", append to it, return None.
/// If no system message exists, return Some(level) for post-processing —
/// the caller must replace "Reasoning: medium" in the formatted output to
/// preserve the template's full default system prompt.
fn inject_harmony_thinking(
    messages: &mut Vec<(String, String)>,
    thinking_mode: &str,
) -> Option<String> {
    let level = match thinking_mode {
        "enabled" | "medium" => "medium",
        "disabled" => "none",
        "low" => "low",
        "high" => "high",
        _ => "medium",
    };

    // Check if any system message already contains "Reasoning:"
    let has_reasoning = messages
        .iter()
        .any(|(role, content)| role == "system" && content.contains("Reasoning:"));
    if has_reasoning {
        tracing::debug!(
            "User system message already contains Reasoning: directive, skipping injection"
        );
        return None;
    }

    // Find existing system message index
    let sys_idx = messages.iter().position(|(role, _)| role == "system");

    if let Some(idx) = sys_idx {
        // Append reasoning to existing system message
        messages[idx]
            .1
            .push_str(&format!("\n\nReasoning: {}", level));
        None
    } else {
        // No system message — return level for post-processing.
        // Do NOT insert a bare system message here; that would cause
        // format_harmony() to skip its rich default system prompt.
        Some(level.to_string())
    }
}

/// Inject /think prefix for GLM-4 template
///
/// Maps: disabled → skip (natural non-thinking), all others → /think
/// GLM-4 naturally defaults to non-thinking when no directive is present,
/// so "Off" simply skips injection for concise output (~483 tokens).
fn inject_glm4_thinking(messages: &mut Vec<(String, String)>, thinking_mode: &str) {
    if thinking_mode == "disabled" {
        tracing::debug!("GLM-4: disabled mode — skipping injection (natural non-thinking)");
        return;
    }

    // All other modes inject /think
    if let Some(last_user) = messages.iter_mut().rev().find(|(role, _)| role == "user") {
        last_user.1 = format!("/think\n{}", last_user.1);
    }
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
        let result = build_prompt_with_context(&context, prompt, None);

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
        let result = build_prompt_with_context(&context, prompt, None);

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

        let result = build_prompt_with_context(&context, pre_formatted_prompt, None);

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

        let result = build_prompt_with_context(&context, prompt, None);

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

        let result = build_prompt_with_context(&context, plain_prompt, None);

        // Should format with Harmony template
        assert!(result.contains("<|start|>user<|message|>"));
        assert!(result.contains("What is the plot of Iron Man?"));
        assert!(result.contains("<|end|>"));
        // Verify it does NOT have double user prefix
        assert!(!result.contains("<|start|>user<|message|><|start|>user<|message|>"));
    }

    // === Thinking Mode Tests (v8.17.0) ===

    #[test]
    fn test_harmony_thinking_high_injects_reasoning_high() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let result = build_prompt_with_context(&[], "Hello", Some("high"));
        assert!(
            result.contains("Reasoning: high"),
            "Expected 'Reasoning: high' in: {}",
            result
        );
    }

    #[test]
    fn test_harmony_thinking_disabled_injects_reasoning_none() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let result = build_prompt_with_context(&[], "Hello", Some("disabled"));
        assert!(
            result.contains("Reasoning: none"),
            "Expected 'Reasoning: none' in: {}",
            result
        );
    }

    #[test]
    fn test_harmony_no_thinking_preserves_default_medium() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let result = build_prompt_with_context(&[], "Hello", None);
        assert!(
            result.contains("Reasoning: medium"),
            "Expected 'Reasoning: medium' in: {}",
            result
        );
    }

    #[test]
    fn test_harmony_user_system_message_with_reasoning_not_overridden() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let context = vec![Message {
            role: "system".to_string(),
            content: "You are helpful.\n\nReasoning: low".to_string(),
            timestamp: None,
        }];
        let result = build_prompt_with_context(&context, "Hello", Some("high"));
        assert!(
            result.contains("Reasoning: low"),
            "Should preserve user's Reasoning: low"
        );
        assert!(
            !result.contains("Reasoning: high"),
            "Should NOT inject Reasoning: high"
        );
    }

    #[test]
    fn test_glm4_thinking_enabled_prepends_think_tag() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "glm4");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let result = build_prompt_with_context(&[], "Hello", Some("enabled"));
        assert!(
            result.contains("/think\n"),
            "Expected /think prefix in: {}",
            result
        );
    }

    #[test]
    fn test_glm4_thinking_disabled_skips_injection() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "glm4");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let result = build_prompt_with_context(&[], "Hello", Some("disabled"));
        assert!(
            !result.contains("/no_think"),
            "Disabled should NOT inject /no_think: {}",
            result
        );
        assert!(
            !result.contains("/think"),
            "Disabled should NOT inject /think: {}",
            result
        );
    }

    #[test]
    fn test_glm4_default_no_auto_think() {
        // v8.22.4: GLM-4 no longer auto-injects /think — it caused degenerate
        // meta-reasoning loops on multi-turn conversations
        std::env::set_var("MODEL_CHAT_TEMPLATE", "glm4");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let result = build_prompt_with_context(&[], "Hello", None);
        assert!(
            !result.contains("/think"),
            "GLM-4 must NOT auto-inject /think (v8.22.4): {}",
            result
        );
    }

    #[test]
    fn test_default_template_thinking_ignored() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "default");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let with_thinking = build_prompt_with_context(&[], "Hello", Some("high"));
        let without_thinking = build_prompt_with_context(&[], "Hello", None);
        assert_eq!(with_thinking, without_thinking);
    }

    #[test]
    fn test_env_var_default_thinking_mode() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::set_var("DEFAULT_THINKING_MODE", "high");
        let result = build_prompt_with_context(&[], "Hello", None);
        std::env::remove_var("DEFAULT_THINKING_MODE");
        assert!(
            result.contains("Reasoning: high"),
            "Expected env var default 'Reasoning: high' in: {}",
            result
        );
    }

    #[test]
    fn test_explicit_thinking_overrides_env_var() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::set_var("DEFAULT_THINKING_MODE", "low");
        let result = build_prompt_with_context(&[], "Hello", Some("high"));
        std::env::remove_var("DEFAULT_THINKING_MODE");
        assert!(
            result.contains("Reasoning: high"),
            "Explicit should override env var"
        );
    }

    // === v8.17.1 Bugfix Tests ===

    #[test]
    fn test_empty_string_thinking_mode_treated_as_none() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::set_var("DEFAULT_THINKING_MODE", "");
        let result = build_prompt_with_context(&[], "Hello", None);
        std::env::remove_var("DEFAULT_THINKING_MODE");
        assert!(
            result.contains("Reasoning: medium"),
            "Empty env var should fall through to template default: {}",
            result
        );
        assert!(
            result.contains("Valid channels"),
            "Empty env var must preserve full default system prompt: {}",
            result
        );
    }

    #[test]
    fn test_explicit_empty_string_treated_as_none() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let result = build_prompt_with_context(&[], "Hello", Some(""));
        assert!(
            result.contains("Reasoning: medium"),
            "Explicit empty string should fall through to template default: {}",
            result
        );
        assert!(
            result.contains("Valid channels"),
            "Explicit empty string must preserve full default system prompt: {}",
            result
        );
    }

    #[test]
    fn test_harmony_thinking_high_preserves_valid_channels() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let result = build_prompt_with_context(&[], "Hello", Some("high"));
        assert!(
            result.contains("Reasoning: high"),
            "Expected Reasoning: high in: {}",
            result
        );
        assert!(
            result.contains("Valid channels: analysis, commentary, final"),
            "thinking=high must preserve full system prompt with Valid channels: {}",
            result
        );
    }

    #[test]
    fn test_harmony_thinking_disabled_preserves_valid_channels() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let result = build_prompt_with_context(&[], "Hello", Some("disabled"));
        assert!(
            result.contains("Reasoning: none"),
            "Expected Reasoning: none in: {}",
            result
        );
        assert!(
            result.contains("Valid channels: analysis, commentary, final"),
            "thinking=disabled must preserve full system prompt with Valid channels: {}",
            result
        );
    }

    // === v8.17.2 Conciseness Directive Tests ===

    #[test]
    fn test_harmony_thinking_off_includes_conciseness_directive() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let result = build_prompt_with_context(&[], "Hello", Some("disabled"));
        assert!(
            result.contains("Provide brief, direct answers"),
            "thinking=disabled should include conciseness directive: {}",
            result
        );
        assert!(
            result.contains("Reasoning: none"),
            "Expected Reasoning: none"
        );
        assert!(
            result.contains("Valid channels"),
            "Must preserve full system prompt"
        );
    }

    #[test]
    fn test_harmony_thinking_with_env_default_preserves_system_prompt() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "harmony");
        std::env::set_var("DEFAULT_THINKING_MODE", "high");
        let result = build_prompt_with_context(&[], "Hello", None);
        std::env::remove_var("DEFAULT_THINKING_MODE");
        assert!(
            result.contains("Reasoning: high"),
            "Expected Reasoning: high in: {}",
            result
        );
        assert!(
            result.contains("You are a helpful AI assistant"),
            "Env default must preserve full default system prompt: {}",
            result
        );
    }

    #[test]
    fn test_glm4_rag_context_in_prompt_preserves_context() {
        std::env::set_var("MODEL_CHAT_TEMPLATE", "glm4");
        std::env::remove_var("DEFAULT_THINKING_MODE");
        let rag_prompt = "[Relevant Context]\nPlatformless AI is a decentralized compute marketplace.\n[End Context]\n\nWhat is Platformless AI?";
        let result = build_prompt_with_context(&[], rag_prompt, None);
        // v8.22.0: System prompt is simple (no RAG instruction)
        assert!(
            result.contains("You are a helpful assistant"),
            "GLM-4 should have simple system prompt: {}",
            result
        );
        // The RAG context should be in the user message
        assert!(
            result.contains("Platformless AI is a decentralized compute marketplace"),
            "RAG context should be preserved in user message: {}",
            result
        );
    }
}
