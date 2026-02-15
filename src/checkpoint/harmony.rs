// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Harmony Format Parser for Checkpoint Messages
//!
//! Parses Harmony-formatted prompts (from GPT-OSS-20B) into individual messages.
//! Used for checkpoint tracking to ensure clean message storage.

use crate::checkpoint::CheckpointMessage;

/// Extract the last user message content from Harmony format without any cleaning
///
/// Returns the raw content of the last user message, or the original message
/// if no Harmony markers are found. Used for checkpoint tracking where we want
/// the exact user input, not a cleaned version.
///
/// # Arguments
/// * `prompt` - The prompt potentially containing Harmony markers
///
/// # Returns
/// The content of the last user message, or the trimmed original if no markers found
///
/// # Example
/// ```ignore
/// let prompt = "<|start|>user<|message|>Hello<|end|> <|start|>assistant<|message|>Hi<|end|> <|start|>user<|message|>How are you?<|end|>";
/// let last_user = extract_last_user_message(prompt);
/// assert_eq!(last_user, "How are you?");
/// ```
pub fn extract_last_user_message(prompt: &str) -> String {
    let mut last_user_content = String::new();
    let mut search_pos = 0;

    // Find all user messages and keep the last one
    while let Some(start_pos) = prompt[search_pos..].find("<|start|>user<|message|>") {
        let abs_start = search_pos + start_pos + "<|start|>user<|message|>".len();
        if let Some(end_pos) = prompt[abs_start..].find("<|end|>") {
            let content = &prompt[abs_start..abs_start + end_pos];
            last_user_content = content.trim().to_string();
            search_pos = abs_start + end_pos;
        } else {
            // No closing tag, take rest as content
            last_user_content = prompt[abs_start..].trim().to_string();
            break;
        }
    }

    // Return raw content if found, otherwise return trimmed original
    if !last_user_content.is_empty() {
        last_user_content
    } else {
        prompt.trim().to_string()
    }
}

/// Parse all messages from a Harmony-formatted prompt
///
/// Extracts all user and assistant messages from the prompt.
/// Handles optional channel markers like `<|channel|>final`.
///
/// # Arguments
/// * `prompt` - The Harmony-formatted prompt
///
/// # Returns
/// Vector of CheckpointMessage structs with role and content
///
/// # Example
/// ```ignore
/// let prompt = "<|start|>user<|message|>Hello<|end|> <|start|>assistant<|channel|>final<|message|>Hi there!<|end|>";
/// let messages = parse_harmony_messages(prompt);
/// assert_eq!(messages.len(), 2);
/// assert_eq!(messages[0].role, "user");
/// assert_eq!(messages[0].content, "Hello");
/// ```
pub fn parse_harmony_messages(prompt: &str) -> Vec<CheckpointMessage> {
    let mut messages = Vec::new();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Use regex-like parsing to extract messages
    // Format: <|start|>(user|assistant)[<|channel|>final]<|message|>content<|end|>
    let mut search_pos = 0;

    while search_pos < prompt.len() {
        // Find next message start
        if let Some(start_idx) = prompt[search_pos..].find("<|start|>") {
            let abs_start = search_pos + start_idx + "<|start|>".len();

            // Determine role (user or assistant)
            let (role, role_end) = if prompt[abs_start..].starts_with("user") {
                ("user", abs_start + 4)
            } else if prompt[abs_start..].starts_with("assistant") {
                ("assistant", abs_start + 9)
            } else if prompt[abs_start..].starts_with("system") {
                // Skip system messages for checkpoint purposes
                if let Some(end_idx) = prompt[abs_start..].find("<|end|>") {
                    search_pos = abs_start + end_idx + "<|end|>".len();
                    continue;
                } else {
                    break;
                }
            } else {
                // Unknown role, skip to next potential message
                search_pos = abs_start;
                continue;
            };

            // Skip optional channel marker
            let content_start = if prompt[role_end..].starts_with("<|channel|>") {
                if let Some(msg_idx) = prompt[role_end..].find("<|message|>") {
                    role_end + msg_idx + "<|message|>".len()
                } else {
                    search_pos = role_end;
                    continue;
                }
            } else if prompt[role_end..].starts_with("<|message|>") {
                role_end + "<|message|>".len()
            } else {
                // No message marker found, skip
                search_pos = role_end;
                continue;
            };

            // Find end marker
            if let Some(end_idx) = prompt[content_start..].find("<|end|>") {
                let content = prompt[content_start..content_start + end_idx]
                    .trim()
                    .to_string();

                if !content.is_empty() {
                    let message = if role == "user" {
                        CheckpointMessage::new_user(content, timestamp)
                    } else {
                        CheckpointMessage::new_assistant(content, timestamp, false)
                    };
                    messages.push(message);
                }

                search_pos = content_start + end_idx + "<|end|>".len();
            } else {
                // No end marker, take rest as content
                let content = prompt[content_start..].trim().to_string();
                if !content.is_empty() {
                    let message = if role == "user" {
                        CheckpointMessage::new_user(content, timestamp)
                    } else {
                        CheckpointMessage::new_assistant(content, timestamp, false)
                    };
                    messages.push(message);
                }
                break;
            }
        } else {
            // No more message markers
            break;
        }
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_last_user_message_single() {
        let prompt = "<|start|>user<|message|>Hello world<|end|>";
        let result = extract_last_user_message(prompt);
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_extract_last_user_message_multiple() {
        let prompt = "<|start|>user<|message|>First message<|end|> <|start|>assistant<|message|>Response<|end|> <|start|>user<|message|>Second message<|end|>";
        let result = extract_last_user_message(prompt);
        assert_eq!(result, "Second message");
    }

    #[test]
    fn test_extract_last_user_message_no_markers() {
        let prompt = "Plain text without markers";
        let result = extract_last_user_message(prompt);
        assert_eq!(result, "Plain text without markers");
    }

    #[test]
    fn test_extract_last_user_message_with_whitespace() {
        let prompt = "<|start|>user<|message|>  Trimmed content  <|end|>";
        let result = extract_last_user_message(prompt);
        assert_eq!(result, "Trimmed content");
    }

    #[test]
    fn test_parse_harmony_messages_single_user() {
        let prompt = "<|start|>user<|message|>Hello<|end|>";
        let messages = parse_harmony_messages(prompt);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Hello");
    }

    #[test]
    fn test_parse_harmony_messages_conversation() {
        let prompt = "<|start|>user<|message|>Hello<|end|> <|start|>assistant<|message|>Hi there!<|end|> <|start|>user<|message|>How are you?<|end|>";
        let messages = parse_harmony_messages(prompt);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content, "Hi there!");
        assert_eq!(messages[2].role, "user");
        assert_eq!(messages[2].content, "How are you?");
    }

    #[test]
    fn test_parse_harmony_messages_with_channel() {
        let prompt = "<|start|>assistant<|channel|>final<|message|>Final response<|end|>";
        let messages = parse_harmony_messages(prompt);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
        assert_eq!(messages[0].content, "Final response");
    }

    #[test]
    fn test_parse_harmony_messages_skips_system() {
        let prompt =
            "<|start|>system<|message|>System prompt<|end|> <|start|>user<|message|>Hello<|end|>";
        let messages = parse_harmony_messages(prompt);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Hello");
    }

    #[test]
    fn test_parse_harmony_messages_empty() {
        let prompt = "No harmony markers here";
        let messages = parse_harmony_messages(prompt);
        assert!(messages.is_empty());
    }

    #[test]
    fn test_parse_harmony_messages_complex() {
        // Real-world example from SDK
        let prompt = "<|start|>user<|message|>Why do photons have momentum when they have no mass?<|end|> <|start|>assistant<|channel|>final<|message|>Photons carry momentum through their energy...<|end|>";
        let messages = parse_harmony_messages(prompt);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert!(messages[0].content.contains("photons"));
        assert_eq!(messages[1].role, "assistant");
        assert!(messages[1].content.contains("momentum"));
    }
}
