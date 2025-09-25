use crate::inference::InferenceResult;
use anyhow::{anyhow, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct FormatConfig {
    pub output_format: OutputFormat,
    pub include_metadata: bool,
    pub include_citations: bool,
    pub max_length: Option<usize>,
    pub strip_whitespace: bool,
    pub highlight_code: bool,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            output_format: OutputFormat::Text,
            include_metadata: false,
            include_citations: false,
            max_length: None,
            strip_whitespace: true,
            highlight_code: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Text,
    Json,
    Markdown,
    Html,
    Xml,
    StreamingJson,
    Multi(Vec<OutputFormat>),
    JsonStructured,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub source: String,
    pub url: Option<String>,
    pub title: Option<String>,
    pub snippet: Option<String>,
    pub relevance_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyCheck {
    pub is_safe: bool,
    pub categories: HashMap<String, f32>,
    pub explanation: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ContentFilter {
    pub check_pii: bool,
    pub check_profanity: bool,
    pub check_bias: bool,
    pub redact_pii: bool,
    pub custom_patterns: Vec<String>,
}

impl Default for ContentFilter {
    fn default() -> Self {
        Self {
            check_pii: true,
            check_profanity: true,
            check_bias: false,
            redact_pii: true,
            custom_patterns: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct ResultFormatter {
    config: FormatConfig,
    pii_patterns: Vec<Regex>,
}

impl ResultFormatter {
    pub fn new(config: FormatConfig) -> Self {
        let pii_patterns = vec![
            // Email addresses
            Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
            // Phone numbers (various formats)
            Regex::new(r"\b\d{3}[-.]?\d{3}[-.]?\d{4}\b").unwrap(),
            // SSN
            Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap(),
            // Credit card
            Regex::new(r"\b\d{4}[\s-]?\d{4}[\s-]?\d{4}[\s-]?\d{4}\b").unwrap(),
        ];

        Self {
            config,
            pii_patterns,
        }
    }

    pub async fn format(&self, result: &InferenceResult) -> Result<String> {
        let text = if self.config.strip_whitespace {
            result.text.trim().to_string()
        } else {
            result.text.clone()
        };

        let text = if let Some(max_len) = self.config.max_length {
            self.truncate_text(&text, max_len)
        } else {
            text
        };

        match &self.config.output_format {
            OutputFormat::Text => Ok(text),
            OutputFormat::Json => self.format_json(result, text).await,
            OutputFormat::Markdown => self.format_markdown(result, text).await,
            OutputFormat::Html => self.format_html(result, text).await,
            OutputFormat::Xml => self.format_xml(result, text).await,
            OutputFormat::StreamingJson => self.format_json(result, text).await, // Similar to Json for non-streaming
            OutputFormat::Multi(_formats) => {
                // For multi-format, just return JSON with all formats
                // In real implementation, would format for each and combine
                self.format_json(result, text).await
            }
            OutputFormat::JsonStructured => self.format_json(result, text).await, // Similar to Json
        }
    }

    pub async fn format_json(&self, result: &InferenceResult, text: String) -> Result<String> {
        let mut output = json!({
            "text": text,
        });

        if self.config.include_metadata {
            output["metadata"] = json!({
                "model_id": result.model_id,
                "tokens_generated": result.tokens_generated,
                "generation_time_ms": result.generation_time.as_millis(),
                "tokens_per_second": result.tokens_per_second,
                "finish_reason": result.finish_reason,
            });
        }

        if self.config.include_citations {
            output["citations"] = json!([]);
        }

        serde_json::to_string_pretty(&output)
            .map_err(|e| anyhow!("Failed to serialize JSON: {}", e))
    }

    pub async fn format_markdown(&self, _result: &InferenceResult, text: String) -> Result<String> {
        let mut output = String::new();

        // Add main content
        output.push_str(&text);

        if self.config.include_citations {
            output.push_str("\n\n## References\n\n");
            output.push_str("*No citations available*\n");
        }

        Ok(output)
    }

    pub async fn format_html(&self, _result: &InferenceResult, text: String) -> Result<String> {
        let escaped = ammonia::clean(&text);

        let html = format!(
            r#"<div class="llm-response">
    <div class="content">{}</div>
</div>"#,
            escaped
        );

        Ok(html)
    }

    pub async fn format_xml(&self, result: &InferenceResult, text: String) -> Result<String> {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<response>
    <text>{}</text>
    <metadata>
        <model_id>{}</model_id>
        <tokens_generated>{}</tokens_generated>
        <generation_time_ms>{}</generation_time_ms>
    </metadata>
</response>"#,
            xml_escape(&text),
            xml_escape(&result.model_id),
            result.tokens_generated,
            result.generation_time.as_millis()
        );

        Ok(xml)
    }

    pub async fn format_with_filter(
        &self,
        result: &InferenceResult,
        filter: &ContentFilter,
    ) -> Result<String> {
        let mut text = result.text.clone();

        if filter.check_pii || filter.redact_pii {
            text = self.handle_pii(text, filter.redact_pii)?;
        }

        if filter.check_profanity {
            text = self.filter_profanity(text)?;
        }

        // Apply custom patterns
        for pattern in &filter.custom_patterns {
            if let Ok(regex) = Regex::new(pattern) {
                text = regex.replace_all(&text, "[REDACTED]").to_string();
            }
        }

        // Format with modified text
        let mut modified_result = result.clone();
        modified_result.text = text;
        self.format(&modified_result).await
    }

    pub fn detect_pii(&self, text: &str) -> Vec<String> {
        let mut detected = Vec::new();

        for pattern in &self.pii_patterns {
            for mat in pattern.find_iter(text) {
                detected.push(mat.as_str().to_string());
            }
        }

        detected
    }

    pub async fn add_citations(&self, text: String, citations: Vec<Citation>) -> Result<String> {
        if citations.is_empty() {
            return Ok(text);
        }

        match &self.config.output_format {
            OutputFormat::Json | OutputFormat::JsonStructured => {
                let output = json!({
                    "text": text,
                    "citations": citations,
                });
                serde_json::to_string_pretty(&output)
                    .map_err(|e| anyhow!("Failed to serialize JSON: {}", e))
            }
            OutputFormat::Markdown => {
                let mut output = text;
                output.push_str("\n\n## References\n\n");

                for (i, citation) in citations.iter().enumerate() {
                    output.push_str(&format!("{}. {}", i + 1, citation.source));
                    if let Some(url) = &citation.url {
                        output.push_str(&format!(" [{}]", url));
                    }
                    output.push_str("\n");
                }

                Ok(output)
            }
            _ => Ok(text),
        }
    }

    pub async fn check_safety(&self, _text: &str) -> SafetyCheck {
        // Mock safety check
        let mut categories = HashMap::new();
        categories.insert("toxicity".to_string(), 0.05);
        categories.insert("violence".to_string(), 0.02);
        categories.insert("hate_speech".to_string(), 0.01);

        SafetyCheck {
            is_safe: true,
            categories,
            explanation: None,
        }
    }

    pub async fn apply_template(&self, text: String, template: &str) -> Result<String> {
        // Simple template replacement
        let result = template.replace("{response}", &text);
        Ok(result)
    }

    fn truncate_text(&self, text: &str, max_length: usize) -> String {
        if text.len() <= max_length {
            return text.to_string();
        }

        // Try to truncate at word boundary
        if let Some(last_space) = text[..max_length].rfind(' ') {
            format!("{}...", &text[..last_space])
        } else {
            format!("{}...", &text[..max_length - 3])
        }
    }

    fn handle_pii(&self, text: String, redact: bool) -> Result<String> {
        if !redact {
            return Ok(text);
        }

        let mut result = text;

        for pattern in &self.pii_patterns {
            result = pattern.replace_all(&result, "[PII_REDACTED]").to_string();
        }

        Ok(result)
    }

    fn filter_profanity(&self, text: String) -> Result<String> {
        // In real implementation, would use a profanity list
        Ok(text)
    }

    pub async fn format_stream_chunk(&self, token: &crate::inference::TokenInfo) -> Result<String> {
        match &self.config.output_format {
            OutputFormat::StreamingJson => {
                let output = json!({
                    "token": token.text,
                    "id": token.token_id,
                    "logprob": token.logprob,
                    "timestamp": token.timestamp,
                });
                serde_json::to_string(&output)
                    .map_err(|e| anyhow!("Failed to serialize JSON: {}", e))
            }
            _ => Ok(token.text.clone()),
        }
    }

    pub async fn format_stream_end(&self) -> Result<String> {
        match &self.config.output_format {
            OutputFormat::StreamingJson => {
                let output = json!({
                    "finished": true,
                });
                serde_json::to_string(&output)
                    .map_err(|e| anyhow!("Failed to serialize JSON: {}", e))
            }
            _ => Ok("".to_string()),
        }
    }
}

fn xml_escape(text: &str) -> String {
    text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&apos;")
}
