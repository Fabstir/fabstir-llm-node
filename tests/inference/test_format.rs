use fabstir_llm_node::inference::{
    ResultFormatter, FormatConfig, OutputFormat, InferenceResult,
    TokenInfo, Citation, SafetyCheck, ContentFilter
};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn test_basic_formatting() {
    let config = FormatConfig {
        output_format: OutputFormat::Text,
        include_metadata: false,
        include_citations: false,
        max_length: None,
        strip_whitespace: true,
    };
    
    let formatter = ResultFormatter::new(config);
    
    let result = InferenceResult {
        text: "  The capital of France is Paris.  \n\n".to_string(),
        tokens_generated: 8,
        generation_time: Duration::from_millis(250),
        tokens_per_second: 32.0,
        model_id: "llama-7b".to_string(),
        finish_reason: "stop".to_string(),
        token_info: vec![],
        was_cancelled: false,
    };
    
    let formatted = formatter.format(&result).await.expect("Failed to format");
    
    // Should strip whitespace
    assert_eq!(formatted, "The capital of France is Paris.");
}

#[tokio::test]
async fn test_json_formatting() {
    let config = FormatConfig {
        output_format: OutputFormat::Json,
        include_metadata: true,
        include_citations: false,
        max_length: None,
        strip_whitespace: false,
    };
    
    let formatter = ResultFormatter::new(config);
    
    let result = InferenceResult {
        text: "Paris is the capital.".to_string(),
        tokens_generated: 5,
        generation_time: Duration::from_millis(150),
        tokens_per_second: 33.3,
        model_id: "llama-7b".to_string(),
        finish_reason: "stop".to_string(),
        token_info: vec![
            TokenInfo { token_id: 1234, text: "Paris".to_string(), logprob: Some(-0.5) },
            TokenInfo { token_id: 338, text: " is".to_string(), logprob: Some(-0.2) },
        ],
        was_cancelled: false,
    };
    
    let formatted = formatter.format(&result).await.expect("Failed to format");
    
    // Parse as JSON
    let json: serde_json::Value = serde_json::from_str(&formatted).expect("Invalid JSON");
    
    assert_eq!(json["text"], "Paris is the capital.");
    assert_eq!(json["metadata"]["tokens_generated"], 5);
    assert_eq!(json["metadata"]["model_id"], "llama-7b");
    assert!(json["metadata"]["generation_time_ms"].as_u64().unwrap() >= 150);
}

#[tokio::test]
async fn test_structured_output_parsing() {
    let config = FormatConfig {
        output_format: OutputFormat::JsonStructured,
        include_metadata: false,
        include_citations: false,
        max_length: None,
        strip_whitespace: true,
    };
    
    let formatter = ResultFormatter::new(config);
    
    // Result with JSON in the text
    let result = InferenceResult {
        text: r#"Based on your request, here's the JSON:
        
        {
            "name": "John Doe",
            "age": 30,
            "city": "New York"
        }
        
        This represents a person object."#.to_string(),
        tokens_generated: 50,
        generation_time: Duration::from_millis(500),
        tokens_per_second: 100.0,
        model_id: "llama-7b".to_string(),
        finish_reason: "stop".to_string(),
        token_info: vec![],
        was_cancelled: false,
    };
    
    let formatted = formatter.format(&result).await.expect("Failed to format");
    
    // Should extract just the JSON
    let json: serde_json::Value = serde_json::from_str(&formatted).expect("Invalid JSON");
    assert_eq!(json["name"], "John Doe");
    assert_eq!(json["age"], 30);
    assert_eq!(json["city"], "New York");
}

#[tokio::test]
async fn test_markdown_formatting() {
    let config = FormatConfig {
        output_format: OutputFormat::Markdown,
        include_metadata: true,
        include_citations: true,
        max_length: None,
        strip_whitespace: false,
    };
    
    let formatter = ResultFormatter::new(config);
    
    let mut result = InferenceResult {
        text: "# Quantum Computing\n\nQuantum computing uses quantum bits.".to_string(),
        tokens_generated: 10,
        generation_time: Duration::from_millis(300),
        tokens_per_second: 33.3,
        model_id: "llama-7b".to_string(),
        finish_reason: "stop".to_string(),
        token_info: vec![],
        was_cancelled: false,
    };
    
    // Add citations
    formatter.add_citations(&mut result, vec![
        Citation {
            text: "quantum bits".to_string(),
            source: "Introduction to Quantum Computing, 2023".to_string(),
            url: Some("https://example.com/quantum".to_string()),
            confidence: 0.95,
        }
    ]).await;
    
    let formatted = formatter.format(&result).await.expect("Failed to format");
    
    // Should include markdown with citations
    assert!(formatted.contains("# Quantum Computing"));
    assert!(formatted.contains("[1]")); // Citation marker
    assert!(formatted.contains("## References"));
    assert!(formatted.contains("Introduction to Quantum Computing"));
}

#[tokio::test]
async fn test_length_truncation() {
    let config = FormatConfig {
        output_format: OutputFormat::Text,
        include_metadata: false,
        include_citations: false,
        max_length: Some(50),
        strip_whitespace: true,
    };
    
    let formatter = ResultFormatter::new(config);
    
    let result = InferenceResult {
        text: "This is a very long response that continues for many words and should be truncated at some point to fit within the limit.".to_string(),
        tokens_generated: 30,
        generation_time: Duration::from_millis(500),
        tokens_per_second: 60.0,
        model_id: "llama-7b".to_string(),
        finish_reason: "length".to_string(),
        token_info: vec![],
        was_cancelled: false,
    };
    
    let formatted = formatter.format(&result).await.expect("Failed to format");
    
    // Should be truncated with ellipsis
    assert!(formatted.len() <= 50);
    assert!(formatted.ends_with("..."));
}

#[tokio::test]
async fn test_safety_filtering() {
    let mut config = FormatConfig::default();
    config.safety_check = Some(SafetyCheck {
        filter_toxic: true,
        filter_pii: true,
        filter_copyrighted: true,
        confidence_threshold: 0.8,
    });
    
    let formatter = ResultFormatter::new(config);
    
    let result = InferenceResult {
        text: "Contact John at john@example.com or call 555-1234.".to_string(),
        tokens_generated: 10,
        generation_time: Duration::from_millis(200),
        tokens_per_second: 50.0,
        model_id: "llama-7b".to_string(),
        finish_reason: "stop".to_string(),
        token_info: vec![],
        was_cancelled: false,
    };
    
    let formatted = formatter.format(&result).await.expect("Failed to format");
    
    // Should redact PII
    assert!(!formatted.contains("john@example.com"));
    assert!(!formatted.contains("555-1234"));
    assert!(formatted.contains("[EMAIL]") || formatted.contains("[REDACTED]"));
    assert!(formatted.contains("[PHONE]") || formatted.contains("[REDACTED]"));
}

#[tokio::test]
async fn test_token_info_formatting() {
    let config = FormatConfig {
        output_format: OutputFormat::JsonVerbose,
        include_metadata: true,
        include_citations: false,
        max_length: None,
        strip_whitespace: false,
    };
    
    let formatter = ResultFormatter::new(config);
    
    let result = InferenceResult {
        text: "Hello world".to_string(),
        tokens_generated: 2,
        generation_time: Duration::from_millis(50),
        tokens_per_second: 40.0,
        model_id: "llama-7b".to_string(),
        finish_reason: "stop".to_string(),
        token_info: vec![
            TokenInfo {
                token_id: 15043,
                text: "Hello".to_string(),
                logprob: Some(-0.3),
            },
            TokenInfo {
                token_id: 3186,
                text: " world".to_string(),
                logprob: Some(-0.5),
            },
        ],
        was_cancelled: false,
    };
    
    let formatted = formatter.format(&result).await.expect("Failed to format");
    let json: serde_json::Value = serde_json::from_str(&formatted).expect("Invalid JSON");
    
    // Should include detailed token information
    assert!(json["tokens"].is_array());
    assert_eq!(json["tokens"][0]["id"], 15043);
    assert_eq!(json["tokens"][0]["text"], "Hello");
    assert_eq!(json["tokens"][0]["logprob"], -0.3);
}

#[tokio::test]
async fn test_streaming_format() {
    let config = FormatConfig {
        output_format: OutputFormat::StreamingJson,
        include_metadata: false,
        include_citations: false,
        max_length: None,
        strip_whitespace: false,
    };
    
    let formatter = ResultFormatter::new(config);
    
    // Format individual tokens for streaming
    let tokens = vec![
        TokenInfo { token_id: 1, text: "The".to_string(), logprob: Some(-0.1) },
        TokenInfo { token_id: 2, text: " answer".to_string(), logprob: Some(-0.2) },
        TokenInfo { token_id: 3, text: " is".to_string(), logprob: Some(-0.1) },
        TokenInfo { token_id: 4, text: " 42".to_string(), logprob: Some(-0.5) },
    ];
    
    let mut stream_output = Vec::new();
    
    for token in tokens {
        let chunk = formatter.format_stream_chunk(&token).await.expect("Failed to format chunk");
        stream_output.push(chunk);
    }
    
    // Each chunk should be valid JSON
    for chunk in &stream_output {
        let json: serde_json::Value = serde_json::from_str(chunk).expect("Invalid JSON");
        assert!(json["token"].is_string());
        assert!(json["id"].is_number());
    }
    
    // Add end marker
    let end_chunk = formatter.format_stream_end().await.expect("Failed to format end");
    let end_json: serde_json::Value = serde_json::from_str(&end_chunk).expect("Invalid JSON");
    assert_eq!(end_json["finished"], true);
}

#[tokio::test]
async fn test_code_block_formatting() {
    let config = FormatConfig {
        output_format: OutputFormat::Markdown,
        include_metadata: false,
        include_citations: false,
        max_length: None,
        strip_whitespace: false,
        highlight_code: true,
    };
    
    let formatter = ResultFormatter::new(config);
    
    let result = InferenceResult {
        text: r#"Here's a Python function:

```python
def fibonacci(n):
    if n <= 1:
        return n
    return fibonacci(n-1) + fibonacci(n-2)
```

This implements the Fibonacci sequence."#.to_string(),
        tokens_generated: 50,
        generation_time: Duration::from_millis(500),
        tokens_per_second: 100.0,
        model_id: "codellama-7b".to_string(),
        finish_reason: "stop".to_string(),
        token_info: vec![],
        was_cancelled: false,
    };
    
    let formatted = formatter.format(&result).await.expect("Failed to format");
    
    // Should preserve code blocks
    assert!(formatted.contains("```python"));
    assert!(formatted.contains("def fibonacci(n):"));
    assert!(formatted.contains("```"));
}

#[tokio::test]
async fn test_multi_format_output() {
    let config = FormatConfig {
        output_format: OutputFormat::Multi(vec![
            OutputFormat::Text,
            OutputFormat::Json,
            OutputFormat::Markdown,
        ]),
        include_metadata: true,
        include_citations: false,
        max_length: None,
        strip_whitespace: true,
    };
    
    let formatter = ResultFormatter::new(config);
    
    let result = InferenceResult {
        text: "Test output".to_string(),
        tokens_generated: 2,
        generation_time: Duration::from_millis(50),
        tokens_per_second: 40.0,
        model_id: "llama-7b".to_string(),
        finish_reason: "stop".to_string(),
        token_info: vec![],
        was_cancelled: false,
    };
    
    let formatted = formatter.format(&result).await.expect("Failed to format");
    
    // Should return multiple formats
    let multi: serde_json::Value = serde_json::from_str(&formatted).expect("Invalid JSON");
    assert!(multi["text"].is_string());
    assert!(multi["json"].is_object());
    assert!(multi["markdown"].is_string());
}

#[tokio::test]
async fn test_error_handling_in_formatting() {
    let config = FormatConfig {
        output_format: OutputFormat::JsonStructured,
        include_metadata: false,
        include_citations: false,
        max_length: None,
        strip_whitespace: true,
    };
    
    let formatter = ResultFormatter::new(config);
    
    // Result with invalid JSON
    let result = InferenceResult {
        text: "This is not valid JSON: {invalid: json, missing: quotes}".to_string(),
        tokens_generated: 10,
        generation_time: Duration::from_millis(100),
        tokens_per_second: 100.0,
        model_id: "llama-7b".to_string(),
        finish_reason: "stop".to_string(),
        token_info: vec![],
        was_cancelled: false,
    };
    
    let formatted_result = formatter.format(&result).await;
    
    // Should handle gracefully
    match formatted_result {
        Ok(formatted) => {
            // Might return the original text or an error object
            assert!(!formatted.is_empty());
        }
        Err(e) => {
            // Should have meaningful error
            assert!(e.to_string().contains("JSON") || e.to_string().contains("parse"));
        }
    }
}