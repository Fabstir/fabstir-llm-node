use crate::job_processor::Message;

/// Build a prompt with conversation context
/// Uses a format that works well with the model
pub fn build_prompt_with_context(context: &[Message], prompt: &str) -> String {
    // If no context, still format consistently
    if context.is_empty() {
        return format!("user: {}\nassistant:", prompt);
    }

    let mut full_prompt = String::new();

    // Take last 10 messages maximum
    let recent_context = if context.len() > 10 {
        &context[context.len() - 10..]
    } else {
        context
    };

    // Format messages with lowercase role names
    for msg in recent_context {
        match msg.role.to_lowercase().as_str() {
            "user" => full_prompt.push_str(&format!("user: {}\n", msg.content)),
            "assistant" => full_prompt.push_str(&format!("assistant: {}\n", msg.content)),
            "system" => full_prompt.push_str(&format!("system: {}\n", msg.content)),
            _ => {}
        }
    }

    // Add current prompt with clear marker
    full_prompt.push_str(&format!("user: {}\nassistant:", prompt));

    full_prompt
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
        let context = vec![];
        let prompt = "Hello";
        let result = build_prompt_with_context(&context, prompt);
        assert_eq!(result, "user: Hello\nassistant:");
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
