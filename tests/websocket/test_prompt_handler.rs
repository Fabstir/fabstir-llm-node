use fabstir_llm_node::api::websocket::{
    handlers::{prompt::PromptHandler, session_init::SessionInitHandler},
    messages::{ConversationMessage, WebSocketMessage},
};
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_prompt_handler_adds_to_cache() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let prompt_handler = PromptHandler::new(session_handler.clone());
    
    // Initialize session first
    let context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: None,
            tokens: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
            timestamp: None,
            tokens: Some(3),
        },
    ];
    
    session_handler
        .handle_session_init("prompt-session", 100, context)
        .await
        .unwrap();
    
    // Send new prompt
    let result = prompt_handler
        .handle_prompt("prompt-session", "What is machine learning?", 3)
        .await
        .unwrap();
    
    assert_eq!(result.session_id, "prompt-session");
    assert_eq!(result.message_index, 3);
    assert!(result.added_to_cache);
    
    // Verify message was added to cache
    let cache = session_handler.get_cache("prompt-session").await.unwrap();
    let messages = cache.get_messages().await;
    assert_eq!(messages.len(), 3); // 2 original + 1 new
    assert_eq!(messages[2].content, "What is machine learning?");
}

#[tokio::test]
async fn test_prompt_handler_requires_session() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let prompt_handler = PromptHandler::new(session_handler);
    
    // Try to send prompt without session
    let result = prompt_handler
        .handle_prompt("non-existent", "Hello?", 1)
        .await;
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Session not found"));
}

#[tokio::test]
async fn test_prompt_handler_validates_message_index() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let prompt_handler = PromptHandler::new(session_handler.clone());
    
    // Initialize session
    session_handler
        .handle_session_init("index-session", 200, vec![])
        .await
        .unwrap();
    
    // Send prompt with wrong index
    let result = prompt_handler
        .handle_prompt("index-session", "Test", 99)
        .await;
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid message index"));
}

#[tokio::test]
async fn test_prompt_handler_maintains_order() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let prompt_handler = PromptHandler::new(session_handler.clone());
    
    // Initialize session
    session_handler
        .handle_session_init("order-session", 300, vec![])
        .await
        .unwrap();
    
    // Send multiple prompts in sequence
    prompt_handler
        .handle_prompt("order-session", "First prompt", 1)
        .await
        .unwrap();
    
    prompt_handler
        .handle_prompt("order-session", "Second prompt", 2)
        .await
        .unwrap();
    
    prompt_handler
        .handle_prompt("order-session", "Third prompt", 3)
        .await
        .unwrap();
    
    // Verify order in cache
    let cache = session_handler.get_cache("order-session").await.unwrap();
    let messages = cache.get_messages().await;
    
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].content, "First prompt");
    assert_eq!(messages[1].content, "Second prompt");
    assert_eq!(messages[2].content, "Third prompt");
}

#[tokio::test]
async fn test_prompt_handler_with_context_window() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let prompt_handler = PromptHandler::new(session_handler.clone());
    
    // Initialize with large context
    let mut context = vec![];
    for i in 0..100 {
        context.push(ConversationMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
            content: format!("Message {}", i),
            timestamp: None,
            tokens: if i % 2 == 1 { Some(10) } else { None },
        });
    }
    
    session_handler
        .handle_session_init("window-session", 400, context)
        .await
        .unwrap();
    
    // Add new prompt
    let result = prompt_handler
        .handle_prompt("window-session", "New prompt after large context", 101)
        .await
        .unwrap();
    
    assert!(result.added_to_cache);
    
    // Cache should maintain context window limits
    let cache = session_handler.get_cache("window-session").await.unwrap();
    assert!(cache.is_within_token_limit().await);
}

#[tokio::test]
async fn test_concurrent_prompts_same_session() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let prompt_handler = Arc::new(PromptHandler::new(session_handler.clone()));
    
    // Initialize session
    session_handler
        .handle_session_init("concurrent-prompts", 500, vec![])
        .await
        .unwrap();
    
    let mut handles = vec![];
    
    // Send 5 concurrent prompts
    for i in 0..5 {
        let ph = prompt_handler.clone();
        let handle = tokio::spawn(async move {
            // Small delay to ensure some overlap
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            ph.handle_prompt(
                "concurrent-prompts",
                &format!("Concurrent prompt {}", i),
                i + 1,
            )
            .await
        });
        handles.push(handle);
    }
    
    // Collect results
    let mut successes = 0;
    let mut failures = 0;
    
    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => successes += 1,
            Err(_) => failures += 1,
        }
    }
    
    // At least one should succeed, others may fail due to index conflicts
    assert!(successes >= 1);
}

#[tokio::test]
async fn test_prompt_handler_empty_content() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let prompt_handler = PromptHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("empty-session", 600, vec![])
        .await
        .unwrap();
    
    // Empty prompt should be rejected
    let result = prompt_handler
        .handle_prompt("empty-session", "", 1)
        .await;
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Empty prompt"));
}

#[tokio::test]
async fn test_prompt_handler_preserves_role() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let prompt_handler = PromptHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("role-session", 700, vec![])
        .await
        .unwrap();
    
    prompt_handler
        .handle_prompt("role-session", "User message", 1)
        .await
        .unwrap();
    
    let cache = session_handler.get_cache("role-session").await.unwrap();
    let messages = cache.get_messages().await;
    
    assert_eq!(messages[0].role, "user");
}