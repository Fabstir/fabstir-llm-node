// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
#[cfg(test)]
mod tests {
    use fabstir_llm_node::job_processor::Message;
    use fabstir_llm_node::utils::context::{
        build_prompt_with_context, count_context_tokens, is_context_within_limits,
    };

    #[test]
    fn test_context_formatting() {
        // Test build_prompt_with_context
        let context = vec![
            Message {
                role: "user".to_string(),
                content: "What is the capital of France?".to_string(),
                timestamp: None,
            },
            Message {
                role: "assistant".to_string(),
                content: "The capital of France is Paris.".to_string(),
                timestamp: None,
            },
        ];

        let prompt = "Tell me more about it.";
        let result = build_prompt_with_context(&context, prompt, None);

        assert!(result.contains("user: What is the capital of France?"));
        assert!(result.contains("assistant: The capital of France is Paris."));
        assert!(result.ends_with("user: Tell me more about it.\nassistant:"));
    }

    #[test]
    fn test_context_token_counting() {
        // Test token estimation
        let context = vec![
            Message {
                role: "user".to_string(),
                content: "This is a test message with some words".to_string(),
                timestamp: None,
            },
            Message {
                role: "assistant".to_string(),
                content: "Here is a response with more words to count".to_string(),
                timestamp: None,
            },
        ];

        let tokens = count_context_tokens(&context);
        // Each character is roughly 1/4 token, so we expect roughly
        // (4 + 39 + 9 + 45) / 4 = 97 / 4 = ~24 tokens
        assert!(tokens > 0);
        assert!(tokens < 100); // Reasonable upper bound
    }

    #[test]
    fn test_context_truncation() {
        // Test >10 messages truncation
        let mut context = Vec::new();
        for i in 0..15 {
            context.push(Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
                content: format!("Message {}", i),
                timestamp: None,
            });
        }

        let prompt = "Final prompt";
        let result = build_prompt_with_context(&context, prompt, None);

        // Should only include messages 5-14 (last 10)
        assert!(!result.contains("Message 0"));
        assert!(!result.contains("Message 4"));
        assert!(result.contains("Message 5"));
        assert!(result.contains("Message 14"));
        assert!(result.ends_with("user: Final prompt\nassistant:"));
    }

    #[test]
    fn test_empty_context() {
        let context = vec![];
        let prompt = "Hello world";
        let result = build_prompt_with_context(&context, prompt, None);

        assert_eq!(result, "user: Hello world\nassistant:");
    }

    #[test]
    fn test_context_within_limits() {
        let context = vec![Message {
            role: "user".to_string(),
            content: "Short".to_string(),
            timestamp: None,
        }];

        assert!(is_context_within_limits(&context, 100));
        assert!(!is_context_within_limits(&context, 1));
    }

    #[test]
    fn test_mixed_roles() {
        let context = vec![
            Message {
                role: "system".to_string(),
                content: "You are a helpful assistant.".to_string(),
                timestamp: None,
            },
            Message {
                role: "user".to_string(),
                content: "Hello".to_string(),
                timestamp: None,
            },
            Message {
                role: "assistant".to_string(),
                content: "Hi there!".to_string(),
                timestamp: None,
            },
        ];

        let prompt = "How are you?";
        let result = build_prompt_with_context(&context, prompt, None);

        assert!(result.contains("system: You are a helpful assistant."));
        assert!(result.contains("user: Hello"));
        assert!(result.contains("assistant: Hi there!"));
        assert!(result.ends_with("user: How are you?\nassistant:"));
    }

    #[test]
    fn test_large_context_token_count() {
        let mut context = Vec::new();
        for i in 0..100 {
            context.push(Message {
                role: "user".to_string(),
                content: format!("This is message number {} with some additional text", i),
                timestamp: None,
            });
        }

        let tokens = count_context_tokens(&context);
        assert!(tokens > 1000); // Should be a significant number

        // Test that it correctly identifies as exceeding limits
        assert!(!is_context_within_limits(&context, 100));
        assert!(is_context_within_limits(&context, 10000));
    }
}
