// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Query extraction from user prompts
//!
//! Extracts search queries from user messages for search-augmented inference.

/// Extract potential search queries from a user message
///
/// This is a simple heuristic-based extractor. For more sophisticated
/// extraction, consider using an LLM to generate queries.
///
/// # Arguments
/// * `message` - The user's message
/// * `max_queries` - Maximum number of queries to extract
///
/// # Returns
/// A vector of search query strings
pub fn extract_search_queries(message: &str, max_queries: usize) -> Vec<String> {
    let mut queries = Vec::new();

    // Simple approach: use the message as-is if it looks like a question
    let trimmed = message.trim();

    if trimmed.is_empty() {
        return queries;
    }

    // If message is short enough, use it directly
    if trimmed.len() <= 100 {
        queries.push(clean_query(trimmed));
    } else {
        // For longer messages, extract key phrases
        // Split into sentences and use the most relevant ones
        let sentences: Vec<&str> = trimmed
            .split(|c| c == '.' || c == '?' || c == '!')
            .filter(|s| !s.trim().is_empty())
            .collect();

        for sentence in sentences.iter().take(max_queries) {
            let query = clean_query(sentence);
            if !query.is_empty() && query.len() >= 3 {
                queries.push(query);
            }
        }
    }

    queries.truncate(max_queries);
    queries
}

/// Clean a query string for search
fn clean_query(query: &str) -> String {
    query
        .trim()
        // Remove common question starters that don't help search
        .trim_start_matches("can you ")
        .trim_start_matches("could you ")
        .trim_start_matches("please ")
        .trim_start_matches("I want to ")
        .trim_start_matches("I need to ")
        .trim_start_matches("help me ")
        .trim_start_matches("tell me ")
        .trim_start_matches("what is ")
        .trim_start_matches("what are ")
        .trim_start_matches("how do I ")
        .trim_start_matches("how can I ")
        // Clean up
        .trim()
        .to_string()
}

/// Check if a message likely needs web search
///
/// Returns true if the message contains indicators that
/// current/recent information would be helpful.
pub fn needs_web_search(message: &str) -> bool {
    let lower = message.to_lowercase();

    // Time-sensitive indicators
    let time_indicators = [
        "latest",
        "recent",
        "current",
        "today",
        "this week",
        "this month",
        "this year",
        "2024",
        "2025",
        "2026",
        "now",
        "right now",
        "up to date",
        "up-to-date",
    ];

    // Action indicators
    let action_indicators = [
        "search for",
        "search the web",
        "look up",
        "find out",
        "google",
        "search online",
        "[search]",
    ];

    // Topic indicators that often need current info
    let topic_indicators = [
        "news",
        "price",
        "stock",
        "weather",
        "score",
        "result",
        "election",
        "update",
        "release",
        "announcement",
    ];

    for indicator in time_indicators.iter() {
        if lower.contains(indicator) {
            return true;
        }
    }

    for indicator in action_indicators.iter() {
        if lower.contains(indicator) {
            return true;
        }
    }

    for indicator in topic_indicators.iter() {
        if lower.contains(indicator) {
            return true;
        }
    }

    false
}

/// Format search results for injection into a prompt
///
/// # Arguments
/// * `results` - Search results to format
/// * `max_results` - Maximum number of results to include
///
/// # Returns
/// Formatted string suitable for prompt injection
pub fn format_results_for_prompt(
    results: &[super::types::SearchResult],
    max_results: usize,
) -> String {
    if results.is_empty() {
        return String::new();
    }

    let mut formatted = String::from("Web search results:\n\n");

    for (i, result) in results.iter().take(max_results).enumerate() {
        formatted.push_str(&format!(
            "[{}] {}\nURL: {}\n{}\n\n",
            i + 1,
            result.title,
            result.url,
            result.snippet
        ));
    }

    formatted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_query() {
        let queries = extract_search_queries("What is Rust programming?", 5);
        assert_eq!(queries.len(), 1);
        assert!(queries[0].contains("Rust"));
    }

    #[test]
    fn test_extract_empty_message() {
        let queries = extract_search_queries("", 5);
        assert!(queries.is_empty());
    }

    #[test]
    fn test_extract_multiple_sentences() {
        let message = "What is quantum computing? How does it work? When will it be practical?";
        let queries = extract_search_queries(message, 5);
        assert!(queries.len() >= 1);
    }

    #[test]
    fn test_extract_max_queries() {
        let message = "A. B. C. D. E. F.";
        let queries = extract_search_queries(message, 2);
        assert!(queries.len() <= 2);
    }

    #[test]
    fn test_clean_query() {
        assert_eq!(clean_query("can you tell me about Rust"), "about Rust");
        assert_eq!(clean_query("please help me with Python"), "with Python");
        assert_eq!(clean_query("  spaces  "), "spaces");
    }

    #[test]
    fn test_needs_web_search_time() {
        assert!(needs_web_search("What's the latest news?"));
        assert!(needs_web_search("Current Bitcoin price"));
        assert!(needs_web_search("What happened today?"));
    }

    #[test]
    fn test_needs_web_search_action() {
        assert!(needs_web_search("Search for quantum computing"));
        assert!(needs_web_search("Can you look up this topic?"));
        assert!(needs_web_search("[search] latest AI news"));
    }

    #[test]
    fn test_needs_web_search_topic() {
        assert!(needs_web_search("What's the weather like?"));
        assert!(needs_web_search("Stock price of Apple"));
        assert!(needs_web_search("Latest news about elections"));
    }

    #[test]
    fn test_needs_web_search_negative() {
        assert!(!needs_web_search("Explain how recursion works"));
        assert!(!needs_web_search("Write a function to sort a list"));
        assert!(!needs_web_search("What is 2 + 2?"));
    }

    #[test]
    fn test_format_results_empty() {
        let formatted = format_results_for_prompt(&[], 5);
        assert!(formatted.is_empty());
    }

    #[test]
    fn test_format_results() {
        use super::super::types::SearchResult;

        let results = vec![SearchResult {
            title: "Test Title".to_string(),
            url: "https://example.com".to_string(),
            snippet: "Test snippet".to_string(),
            published_date: None,
            source: "test".to_string(),
        }];

        let formatted = format_results_for_prompt(&results, 5);
        assert!(formatted.contains("Test Title"));
        assert!(formatted.contains("https://example.com"));
        assert!(formatted.contains("[1]"));
    }
}
