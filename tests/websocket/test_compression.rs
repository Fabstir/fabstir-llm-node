use fabstir_llm_node::api::websocket::{
    compression::{CompressionConfig, MessageCompressor},
    messages::{WebSocketMessage, ConversationMessage},
};
use flate2::read::{GzDecoder, DeflateDecoder};
use std::io::Read;

#[tokio::test]
async fn test_gzip_compression_large_message() {
    let config = CompressionConfig {
        enabled: true,
        threshold_bytes: 1024,
        compression_type: "gzip".to_string(),
        level: 6,
    };
    
    let compressor = MessageCompressor::new(config);
    
    // Create a large message that should be compressed
    let large_content = "x".repeat(2000);
    let message = WebSocketMessage::Response {
        session_id: "test".to_string(),
        content: large_content.clone(),
        tokens_used: 100,
        message_index: 1,
    };
    
    let compressed = compressor.compress(&message).await.unwrap();
    
    // Verify compression occurred
    let original_size = serde_json::to_vec(&message).unwrap().len();
    assert!(compressed.len() < original_size);
    
    // Verify we can decompress
    let mut decoder = GzDecoder::new(&compressed[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    
    let recovered: WebSocketMessage = serde_json::from_str(&decompressed).unwrap();
    match recovered {
        WebSocketMessage::Response { content, .. } => {
            assert_eq!(content, large_content);
        }
        _ => panic!("Wrong message type"),
    }
}

#[tokio::test]
async fn test_deflate_compression() {
    let config = CompressionConfig {
        enabled: true,
        threshold_bytes: 1024,
        compression_type: "deflate".to_string(),
        level: 6,
    };
    
    let compressor = MessageCompressor::new(config);
    let large_content = "a".repeat(1500);
    
    let message = WebSocketMessage::Prompt {
        session_id: "test".to_string(),
        content: large_content.clone(),
        message_index: 1,
    };
    
    let compressed = compressor.compress(&message).await.unwrap();
    
    // Verify compression
    let original = serde_json::to_vec(&message).unwrap();
    assert!(compressed.len() < original.len());
    
    // Decompress
    let mut decoder = DeflateDecoder::new(&compressed[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    
    let recovered: WebSocketMessage = serde_json::from_str(&decompressed).unwrap();
    match recovered {
        WebSocketMessage::Prompt { content, .. } => {
            assert_eq!(content, large_content);
        }
        _ => panic!("Wrong message type"),
    }
}

#[tokio::test]
async fn test_compression_ratio_verification() {
    let config = CompressionConfig {
        enabled: true,
        threshold_bytes: 1024,
        compression_type: "gzip".to_string(),
        level: 9, // Maximum compression
    };
    
    let compressor = MessageCompressor::new(config);
    
    // Highly compressible data (repeated pattern)
    let content = "abcdefghij".repeat(500); // 5000 bytes of repetitive data
    let message = WebSocketMessage::Response {
        session_id: "test".to_string(),
        content,
        tokens_used: 200,
        message_index: 1,
    };
    
    let compressed = compressor.compress(&message).await.unwrap();
    let original_size = serde_json::to_vec(&message).unwrap().len();
    
    // Calculate compression ratio
    let ratio = 1.0 - (compressed.len() as f64 / original_size as f64);
    
    // Should achieve >40% compression on repetitive data
    assert!(ratio > 0.4, "Compression ratio {} is less than 40%", ratio * 100.0);
}

#[tokio::test]
async fn test_small_message_bypass() {
    let config = CompressionConfig {
        enabled: true,
        threshold_bytes: 1024,
        compression_type: "gzip".to_string(),
        level: 6,
    };
    
    let compressor = MessageCompressor::new(config);
    
    // Small message under threshold
    let message = WebSocketMessage::Prompt {
        session_id: "test".to_string(),
        content: "Hello".to_string(),
        message_index: 1,
    };
    
    let result = compressor.compress(&message).await.unwrap();
    
    // Should return original JSON without compression
    let original = serde_json::to_vec(&message).unwrap();
    assert_eq!(result, original);
}

#[tokio::test]
async fn test_compression_disabled() {
    let config = CompressionConfig {
        enabled: false,
        threshold_bytes: 0,
        compression_type: "gzip".to_string(),
        level: 6,
    };
    
    let compressor = MessageCompressor::new(config);
    
    let message = WebSocketMessage::Response {
        session_id: "test".to_string(),
        content: "x".repeat(10000),
        tokens_used: 500,
        message_index: 1,
    };
    
    let result = compressor.compress(&message).await.unwrap();
    
    // Should return original JSON
    let original = serde_json::to_vec(&message).unwrap();
    assert_eq!(result, original);
}

#[tokio::test]
async fn test_compression_error_fallback() {
    let config = CompressionConfig {
        enabled: true,
        threshold_bytes: 1024,
        compression_type: "invalid".to_string(), // Invalid type
        level: 6,
    };
    
    let compressor = MessageCompressor::new(config);
    
    let message = WebSocketMessage::Response {
        session_id: "test".to_string(),
        content: "x".repeat(2000),
        tokens_used: 100,
        message_index: 1,
    };
    
    // Should fallback to uncompressed on error
    let result = compressor.compress(&message).await.unwrap();
    let original = serde_json::to_vec(&message).unwrap();
    assert_eq!(result, original);
}

#[tokio::test]
async fn test_compression_negotiation() {
    let compressor = MessageCompressor::from_headers(&[
        ("Accept-Encoding", "gzip, deflate"),
    ]);
    
    assert!(compressor.supports_compression());
    assert_eq!(compressor.compression_type(), "gzip"); // Prefer gzip
}

#[tokio::test]
async fn test_compression_with_conversation_context() {
    let config = CompressionConfig {
        enabled: true,
        threshold_bytes: 512,
        compression_type: "gzip".to_string(),
        level: 6,
    };
    
    let compressor = MessageCompressor::new(config);
    
    // Large conversation context
    let mut context = vec![];
    for i in 0..50 {
        context.push(ConversationMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
            content: format!("Message {}: {}", i, "x".repeat(50)),
            timestamp: Some(i),
            tokens: Some(10),
            proof: None,
        });
    }
    
    let message = WebSocketMessage::SessionInit {
        session_id: "test".to_string(),
        job_id: 123,
        chain_id: Some(84532), // Base Sepolia
        conversation_context: context,
    };
    
    let compressed = compressor.compress(&message).await.unwrap();
    let original_size = serde_json::to_vec(&message).unwrap().len();
    
    // Should compress large context
    assert!(compressed.len() < original_size);
    
    // Verify decompression
    let mut decoder = GzDecoder::new(&compressed[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    
    let recovered: WebSocketMessage = serde_json::from_str(&decompressed).unwrap();
    match recovered {
        WebSocketMessage::SessionInit { conversation_context, .. } => {
            assert_eq!(conversation_context.len(), 50);
        }
        _ => panic!("Wrong message type"),
    }
}

#[tokio::test]
async fn test_compression_levels() {
    for level in 1..=9 {
        let config = CompressionConfig {
            enabled: true,
            threshold_bytes: 1024,
            compression_type: "gzip".to_string(),
            level,
        };
        
        let compressor = MessageCompressor::new(config);
        let message = WebSocketMessage::Response {
            session_id: "test".to_string(),
            content: "test".repeat(500),
            tokens_used: 50,
            message_index: 1,
        };
        
        let compressed = compressor.compress(&message).await.unwrap();
        assert!(compressed.len() > 0);
    }
}