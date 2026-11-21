// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! A client's explicit "no" to web search must be honoured.
//!
//! Before v8.65.2 `web_search` was a plain bool, so an explicit `false` could
//! not be told from an absent field, and the streaming path searched whenever
//! the prompt merely looked like it wanted fresh information ("latest",
//! "today", "now"). A user who had switched search off still had their prompt
//! sent to the search provider.

use fabstir_llm_node::api::InferenceRequest;
use fabstir_llm_node::search::query_extractor::{search_decision, SearchDecision};

fn request(extra: &str) -> InferenceRequest {
    let json = format!(
        r#"{{"model":"m","prompt":"What's the latest news today?","max_tokens":16{}}}"#,
        extra
    );
    serde_json::from_str(&json).expect("request must deserialise")
}

#[test]
fn explicit_false_is_kept_distinct_from_absent() {
    assert_eq!(request(r#","web_search":false"#).web_search, Some(false));
    assert_eq!(request("").web_search, None);
    assert_eq!(request(r#","web_search":null"#).web_search, None);
    assert_eq!(request(r#","web_search":true"#).web_search, Some(true));
}

#[test]
fn the_camel_case_alias_carries_an_explicit_false_too() {
    assert_eq!(request(r#","webSearch":false"#).web_search, Some(false));
    assert_eq!(request(r#","webSearch":true"#).web_search, Some(true));
}

#[test]
fn an_explicit_no_is_never_searched_whatever_the_prompt_says() {
    for prompt in [
        "What's the latest news today?",
        "Current Bitcoin price right now",
        "Search for quantum computing",
        "[search] latest AI news 2026",
    ] {
        assert_eq!(
            search_decision(Some(false), prompt),
            SearchDecision::No,
            "explicit false must win for: {prompt}"
        );
    }
}

#[test]
fn an_explicit_yes_always_searches() {
    assert_eq!(
        search_decision(Some(true), "Explain how recursion works"),
        SearchDecision::Explicit
    );
}

#[test]
fn only_an_absent_field_leaves_the_decision_to_detection() {
    assert_eq!(
        search_decision(None, "What's the latest news today?"),
        SearchDecision::AutoDetected
    );
    assert_eq!(
        search_decision(None, "Explain how recursion works"),
        SearchDecision::No
    );
}

#[test]
fn the_decision_reports_whether_to_search() {
    assert!(SearchDecision::Explicit.should_search());
    assert!(SearchDecision::AutoDetected.should_search());
    assert!(!SearchDecision::No.should_search());
}

/// The streaming and WebSocket paths in `src/api/server.rs` must go through
/// `search_decision`, and must not flatten an absent field to `false` (which
/// would silently switch detection off) or bypass the rule with `||`.
#[test]
fn the_server_paths_use_the_decision_and_keep_the_field_optional() {
    let src = include_str!("../src/api/server.rs");
    assert!(
        !src.contains("request.web_search || crate::search::query_extractor::needs_web_search"),
        "the streaming path must not OR the flag with detection"
    );
    assert!(
        src.contains("search_decision(request.web_search"),
        "the streaming path must use search_decision"
    );
    let ws = src
        .split("// Extract web_search fields from outer message")
        .nth(1)
        .expect("WebSocket extraction block present");
    let block = &ws[..ws.find("let max_searches").expect("block end")];
    assert!(
        !block.contains("unwrap_or(false)"),
        "the WebSocket path must keep an absent web_search as absent"
    );
}
