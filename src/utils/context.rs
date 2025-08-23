use crate::job_processor::Message;

/// Build a prompt with conversation context
/// Limits to last 10 messages for context window
pub fn build_prompt_with_context(
    context: &[Message], 
    prompt: &str
) -> String {
    let mut full_prompt = String::new();
    
    // Take last 10 messages maximum
    let recent_context = if context.len() > 10 {
        &context[context.len() - 10..]
    } else {
        context
    };
    
    // Format each message
    for msg in recent_context {
        full_prompt.push_str(&format!("{}: {}\n", 
            msg.role.to_lowercase(), 
            msg.content
        ));
    }
    
    // Add current prompt
    full_prompt.push_str(&format!("user: {}\nassistant:", prompt));
    
    full_prompt
}

/// Estimate token count for context
pub fn count_context_tokens(context: &[Message]) -> usize {
    context.iter()
        .map(|msg| (msg.content.len() + msg.role.len()) / 4)
        .sum()
}

/// Check if context is within limits
pub fn is_context_within_limits(
    context: &[Message], 
    max_tokens: usize
) -> bool {
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
        let context = vec![
            Message {
                role: "user".to_string(),
                content: "test message".to_string(),
                timestamp: None,
            }
        ];
        let tokens = count_context_tokens(&context);
        assert!(tokens > 0);
    }
}