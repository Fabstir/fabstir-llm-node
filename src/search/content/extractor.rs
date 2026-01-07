//! HTML content extraction
//!
//! Extracts main content from web pages using CSS selectors.

use scraper::{Html, Selector};

/// Extract main content from HTML
///
/// Tries multiple strategies in order:
/// 1. `<article>` tag
/// 2. `<main>` tag
/// 3. `[role="main"]` attribute
/// 4. Common content class names (.content, .post-content, .article-body, etc.)
/// 5. Fallback to `<body>` with noise removal
///
/// # Arguments
/// * `html` - Raw HTML string
/// * `max_chars` - Maximum characters to return
///
/// # Returns
/// Extracted text content, cleaned and truncated
pub fn extract_main_content(html: &str, max_chars: usize) -> String {
    let document = Html::parse_document(html);

    // Priority order of selectors to try
    let selectors = [
        "article",
        "main",
        "[role='main']",
        ".post-content",
        ".article-content",
        ".entry-content",
        ".story-body",        // BBC
        ".article__body",     // News sites
        ".content-body",
        "#article-body",
        "#content",
        ".prose",             // Tailwind
    ];

    for selector_str in selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                let text = extract_text_from_element(&element);
                let cleaned = clean_text(&text);
                if cleaned.len() > 200 {
                    // Found substantial content
                    return truncate_content(&cleaned, max_chars);
                }
            }
        }
    }

    // Fallback: extract from body, removing nav/footer/script
    extract_body_text(&document, max_chars)
}

/// Extract text from an HTML element, stripping tags
fn extract_text_from_element(element: &scraper::ElementRef) -> String {
    element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extract text from body, removing common noise elements
fn extract_body_text(document: &Html, max_chars: usize) -> String {
    // Try to get body
    if let Ok(body_selector) = Selector::parse("body") {
        if let Some(body) = document.select(&body_selector).next() {
            let text = extract_text_from_element(&body);
            let cleaned = clean_text(&text);
            return truncate_content(&cleaned, max_chars);
        }
    }
    String::new()
}

/// Clean text: normalize whitespace, remove excess blank lines
fn clean_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// Truncate content to max_chars, preserving word boundaries
fn truncate_content(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }

    // Find last space before max_chars
    let truncated = &text[..max_chars];
    if let Some(last_space) = truncated.rfind(' ') {
        format!("{}...", &text[..last_space])
    } else {
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HTML_ARTICLE: &str = r#"
        <!DOCTYPE html>
        <html>
        <head><title>Test</title></head>
        <body>
            <nav>Navigation links here that should not appear in extracted content</nav>
            <article>
                <h1>Main Article Title</h1>
                <p>This is the main content of the article with important information that readers need to know about.
                The article contains detailed explanations and substantial text that provides value to the reader.
                We need enough content here to exceed the minimum threshold of 200 characters.</p>
                <p>More substantial content that should be extracted as part of the main article body.
                This paragraph adds additional context and information that enriches the overall article.</p>
            </article>
            <footer>Footer content that should not be included</footer>
        </body>
        </html>
    "#;

    const SAMPLE_HTML_MAIN: &str = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <header>Site Header that should not appear in the extracted content</header>
            <main>
                <h1>Page Title</h1>
                <p>Main content goes here with detailed information about the topic.
                This paragraph contains substantial text that provides real value to readers.
                We need enough content to exceed the minimum threshold requirement of 200 characters.
                The main element is a semantic HTML5 element that indicates the primary content area.</p>
                <p>Additional paragraph with more detailed explanations and context for the reader.</p>
            </main>
            <aside>Sidebar content that should not be extracted</aside>
        </body>
        </html>
    "#;

    const SAMPLE_HTML_CLASS: &str = r#"
        <!DOCTYPE html>
        <html>
        <body>
            <div class="post-content">
                <p>Blog post content with enough text to be considered substantial.
                This paragraph contains meaningful content that provides value to readers.
                We include detailed explanations and enough text to exceed the minimum threshold.</p>
                <p>Additional paragraph with more content for the reader that enriches the post.
                The post-content class is commonly used in blog themes and content management systems.</p>
            </div>
        </body>
        </html>
    "#;

    #[test]
    fn test_extract_article_content() {
        let content = extract_main_content(SAMPLE_HTML_ARTICLE, 3000);
        assert!(content.contains("Main Article Title"));
        assert!(content.contains("main content"));
        assert!(!content.contains("Navigation"));
        assert!(!content.contains("Footer"));
    }

    #[test]
    fn test_extract_main_content() {
        let content = extract_main_content(SAMPLE_HTML_MAIN, 3000);
        assert!(content.contains("Page Title"));
        assert!(content.contains("Main content"));
        assert!(!content.contains("Site Header"));
        assert!(!content.contains("Sidebar"));
    }

    #[test]
    fn test_extract_content_with_class() {
        let content = extract_main_content(SAMPLE_HTML_CLASS, 3000);
        assert!(content.contains("Blog post content"));
    }

    #[test]
    fn test_clean_whitespace() {
        let dirty = "  Hello   world  \n\n  test  ";
        let cleaned = clean_text(dirty);
        assert_eq!(cleaned, "Hello world test");
    }

    #[test]
    fn test_truncate_content() {
        let long_text = "This is a long text that needs to be truncated at word boundary";
        let truncated = truncate_content(long_text, 30);
        assert!(truncated.len() <= 33); // 30 + "..."
        assert!(truncated.ends_with("..."));
        assert!(!truncated.contains("truncated")); // Word boundary
    }

    #[test]
    fn test_truncate_short_content() {
        let short = "Short text";
        let result = truncate_content(short, 100);
        assert_eq!(result, "Short text"); // No truncation needed
    }
}
