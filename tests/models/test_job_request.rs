// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
#[cfg(test)]
mod tests {
    use ethers::types::{Address, H256, U256};
    use fabstir_llm_node::job_processor::{JobRequest, Message};
    use serde_json;

    #[test]
    fn test_job_request_with_empty_context() {
        // Test deserialization without context field
        let json = r#"{
            "job_id": "0x0000000000000000000000000000000000000000000000000000000000000001",
            "requester": "0x0000000000000000000000000000000000000000",
            "model_id": "test-model",
            "max_tokens": 100,
            "parameters": "{}",
            "payment_amount": "1000000000000000000",
            "deadline": "1234567890",
            "timestamp": "1234567890"
        }"#;

        let job_request: JobRequest = serde_json::from_str(json).unwrap();
        assert!(job_request.conversation_context.is_empty());
    }

    #[test]
    fn test_job_request_with_context() {
        // Test with conversation_context
        let json = r#"{
            "job_id": "0x0000000000000000000000000000000000000000000000000000000000000001",
            "requester": "0x0000000000000000000000000000000000000000",
            "model_id": "test-model",
            "max_tokens": 100,
            "parameters": "{}",
            "payment_amount": "1000000000000000000",
            "deadline": "1234567890",
            "timestamp": "1234567890",
            "conversation_context": [
                {
                    "role": "user",
                    "content": "Hello"
                },
                {
                    "role": "assistant",
                    "content": "Hi there!"
                }
            ]
        }"#;

        let job_request: JobRequest = serde_json::from_str(json).unwrap();
        assert_eq!(job_request.conversation_context.len(), 2);
        assert_eq!(job_request.conversation_context[0].role, "user");
        assert_eq!(job_request.conversation_context[0].content, "Hello");
        assert_eq!(job_request.conversation_context[1].role, "assistant");
        assert_eq!(job_request.conversation_context[1].content, "Hi there!");
    }

    #[test]
    fn test_message_serialization() {
        // Test Message struct
        let msg = Message {
            role: "user".to_string(),
            content: "Test message".to_string(),
            timestamp: Some(1234567890),
        };

        let serialized = serde_json::to_string(&msg).unwrap();
        let deserialized: Message = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.role, msg.role);
        assert_eq!(deserialized.content, msg.content);
        assert_eq!(deserialized.timestamp, msg.timestamp);
    }

    #[test]
    fn test_message_without_timestamp() {
        let msg = Message {
            role: "assistant".to_string(),
            content: "Response".to_string(),
            timestamp: None,
        };

        let serialized = serde_json::to_string(&msg).unwrap();
        assert!(!serialized.contains("timestamp"));

        let deserialized: Message = serde_json::from_str(&serialized).unwrap();
        assert!(deserialized.timestamp.is_none());
    }

    #[test]
    fn test_job_request_default() {
        let job = JobRequest::default();
        assert!(job.conversation_context.is_empty());
        assert_eq!(job.job_id, H256::zero());
        assert_eq!(job.requester, Address::zero());
    }
}
