// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for WebSocket vision pre-processing (Phase 6)
//! Validates image extraction from decrypted JSON payloads and prompt augmentation

use serde_json::json;

// --- Image extraction from decrypted JSON ---

fn extract_images(decrypted: &serde_json::Value) -> Vec<&serde_json::Value> {
    decrypted
        .get("images")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().collect())
        .unwrap_or_default()
}

#[test]
fn test_extract_images_from_decrypted_json() {
    let payload = json!({
        "prompt": "Describe this image",
        "images": [
            {"data": "iVBORw0KGgo...", "format": "png"},
            {"data": "/9j/4AAQ...", "format": "jpeg"}
        ]
    });
    let images = extract_images(&payload);
    assert_eq!(images.len(), 2);
    assert_eq!(images[0]["format"], "png");
    assert_eq!(images[1]["format"], "jpeg");
    assert!(images[0]["data"].as_str().unwrap().starts_with("iVBOR"));
}

#[test]
fn test_extract_images_missing_field() {
    let payload = json!({
        "prompt": "Hello world"
    });
    let images = extract_images(&payload);
    assert!(images.is_empty());
}

#[test]
fn test_extract_images_empty_array() {
    let payload = json!({
        "prompt": "No images here",
        "images": []
    });
    let images = extract_images(&payload);
    assert!(images.is_empty());
}

// --- Prompt augmentation with vision context ---

use fabstir_llm_node::vision::augment_prompt_with_vision;

#[test]
fn test_augment_prompt_with_vision_single_turn() {
    let descriptions = vec!["Visual: A photo of a cat sitting on a table.".to_string()];
    let prompt = "What do you see?";
    let result = augment_prompt_with_vision(&descriptions, prompt);
    // Vision context should be included
    assert!(result.contains("A photo of a cat sitting on a table."));
    // User's question preserved
    assert!(result.contains("What do you see?"));
    // No raw [Image Analysis] markers that could leak
    assert!(!result.contains("[Image Analysis"));
    assert!(!result.contains("[/Image Analysis]"));
}

#[test]
fn test_augment_prompt_no_descriptions() {
    let descriptions: Vec<String> = vec![];
    let prompt = "Hello world";
    let result = augment_prompt_with_vision(&descriptions, prompt);
    assert_eq!(result, "Hello world");
}

#[test]
fn test_augment_prompt_multiple_images() {
    let descriptions = vec![
        "Text content:\nHello World".to_string(),
        "Visual: A blue house.".to_string(),
    ];
    let prompt = "Compare these images.";
    let result = augment_prompt_with_vision(&descriptions, prompt);
    assert!(result.contains("Hello World"));
    assert!(result.contains("A blue house."));
    assert!(result.contains("Compare these images."));
}

#[test]
fn test_augment_prompt_injects_into_last_user_turn() {
    // Simulates conversation history - vision context should be injected
    // into the LAST "User:" turn, not as a separate block
    let descriptions = vec![
        "Text content:\ngettyimages\nCredit: James Warwick".to_string(),
        "Visual: A puffin on a cliff.".to_string(),
    ];
    let prompt = "User: What is in image 1?\n\nAssistant: It shows a cat.\n\nUser: Now describe this new image.";
    let result = augment_prompt_with_vision(&descriptions, prompt);

    // Previous conversation preserved
    assert!(result.contains("Assistant: It shows a cat."));

    // Vision context injected into the last User turn
    assert!(result.contains("gettyimages"));
    assert!(result.contains("A puffin on a cliff."));

    // User's actual question preserved
    assert!(result.contains("Now describe this new image."));

    // No raw [Image Analysis] markers
    assert!(!result.contains("[Image Analysis"));

    // Vision context appears AFTER the previous assistant response
    let assistant_pos = result.find("Assistant: It shows a cat.").unwrap();
    let vision_pos = result.find("gettyimages").unwrap();
    assert!(vision_pos > assistant_pos);

    // Multi-turn: must contain override instruction to ignore previous descriptions
    assert!(result.contains("IGNORE any previous image descriptions"));
    assert!(result.contains("NEW image"));
}

#[test]
fn test_augment_prompt_multi_turn_overrides_previous_image() {
    // Simulates the exact scenario: user sent puffin image first, then a new image.
    // The conversation history contains the puffin description from the assistant.
    // The new vision context should override with explicit instruction.
    let new_descriptions = vec![
        "Text content:\nPlatformless AI decentralized infrastructure".to_string(),
        "Visual: A text document about AI.".to_string(),
    ];
    let prompt = "User: Describe what you see in the attached image\n\n\
                  Assistant: The image features a puffin on a cliff. Text: gettyimages, Credit: James Warwick\n\n\
                  User: Do you see any text in the attached image?";
    let result = augment_prompt_with_vision(&new_descriptions, prompt);

    // NEW image context must be present
    assert!(result.contains("Platformless AI decentralized infrastructure"));
    assert!(result.contains("A text document about AI."));

    // Must contain explicit override instruction
    assert!(result.contains("IGNORE any previous image descriptions"));

    // User's question preserved
    assert!(result.contains("Do you see any text in the attached image?"));

    // NEW context must appear AFTER the previous assistant response
    let assistant_pos = result.find("puffin on a cliff").unwrap();
    let new_context_pos = result.find("Platformless AI").unwrap();
    assert!(new_context_pos > assistant_pos);
}

#[test]
fn test_augment_prompt_single_turn_no_override() {
    // Single-turn (no conversation history) should NOT have the override instruction
    let descriptions = vec!["Visual: A cat.".to_string()];
    let prompt = "What do you see?";
    let result = augment_prompt_with_vision(&descriptions, prompt);

    // Should use normal intro, not override
    assert!(result.contains("The attached image contains the following:"));
    assert!(!result.contains("IGNORE any previous"));
}

#[test]
fn test_augment_prompt_ocr_text_is_visible() {
    // Ensure OCR text content is clearly included so LLM doesn't say "no text"
    let descriptions = vec![
        "Text content:\ngettyimages\nCredit: James Warwick\n599365999".to_string(),
        "Visual: A puffin on a cliff.".to_string(),
    ];
    let prompt = "User: Can you read any text in this image?";
    let result = augment_prompt_with_vision(&descriptions, prompt);

    // OCR text must be present and clearly labeled
    assert!(result.contains("Text content:"));
    assert!(result.contains("gettyimages"));
    assert!(result.contains("Credit: James Warwick"));
    assert!(result.contains("599365999"));
}
