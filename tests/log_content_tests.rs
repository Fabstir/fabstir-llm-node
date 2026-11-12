// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Legal 3.10: a host's log never carries what a user wrote, what an image showed or what a model answered.
//! This reads the source of the modules that handle such text and fails on any log line that is handed the text
//! itself rather than a measure of it (a length, a count). It is a lint, not a proof: it knows the names these
//! modules give to content. A new name for content belongs in `CONTENT_NAMES`.
use regex::Regex;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
/// The modules through which prompts, search queries, image text and descriptions pass.
const FILES: &[&str] = &[
    "src/api/server.rs",
    "src/api/search/handler.rs",
    "src/search/service.rs",
    "src/api/describe_image/handler.rs",
    "src/api/ocr/handler.rs",
    "src/vision/florence/decoder.rs",
    "src/vision/florence/model.rs",
    "src/vision/ocr/model.rs",
    "src/vision/vlm_client.rs",
    "src/api/websocket/server.rs",
];
/// What these modules call user content.
const CONTENT_NAMES: &str = "query|queries|prompt|user_prompt|clean_prompt|plaintext_prompt|prompt_text|\
generation_prompt|prompt_tokens|augmented|description|descriptions|desc|text|texts|content|\
full_text|message_text|response_text";

/// Every `info!`/`debug!`/`warn!`/`error!`/`trace!` call in `src`, as `(line, call text)`.
fn log_calls(src: &str) -> Vec<(usize, String)> {
    let start = Regex::new(r"\b(info|debug|warn|error|trace)!\s*\(").unwrap();
    let bytes = src.as_bytes();
    let mut out = vec![];
    for m in start.find_iter(src) {
        let (mut i, mut depth) = (m.end(), 1usize);
        while i < bytes.len() && depth > 0 {
            match bytes[i] {
                b'"' => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        i += if bytes[i] == b'\\' { 2 } else { 1 };
                    }
                }
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let line = src[..m.start()].matches('\n').count() + 1;
        out.push((line, src[m.start()..i.min(src.len())].to_string()));
    }
    out
}

/// The content name a log call is handed, if any. Measures of content are fine: `x.len()`, `x.chars().count()`,
/// `x.is_empty()`, and `x.as_deref().map_or(0, |p| p.chars().count())`.
fn content_in(call: &str) -> Option<String> {
    let squeezed: String = call.chars().filter(|c| !c.is_whitespace()).collect();
    // the format string: only its inline captures (`{name}`, `{name:?}`) can carry content
    let first = squeezed.find('"')?;
    let mut end = first + 1;
    let b = squeezed.as_bytes();
    while end < b.len() && b[end] != b'"' {
        end += if b[end] == b'\\' { 2 } else { 1 };
    }
    let end = end.min(b.len());
    let captures: String = Regex::new(r"\{([A-Za-z_][A-Za-z0-9_]*)")
        .unwrap()
        .captures_iter(&squeezed[first..end])
        .map(|c| format!(",{}", &c[1]))
        .collect();
    let args = format!("{}{}", &squeezed[(end + 1).min(squeezed.len())..], captures);
    let path = r"[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*";
    let measured = Regex::new(&format!(
        r"{path}(?:\.as_deref\(\))?\.map_or\(0,\|p\|p\.chars\(\)\.count\(\)\)|{path}\.(?:chars\(\)\.count\(\)|len\(\)|is_empty\(\)|is_some\(\)|is_none\(\))"
    ))
    .unwrap();
    let rest = measured.replace_all(&args, "_");
    Regex::new(&format!(r"\b({CONTENT_NAMES})\b"))
        .unwrap()
        .find(&rest)
        .map(|m| m.as_str().to_string())
}

#[test]
fn the_lint_tells_content_from_a_measure_of_it() {
    // a lint that passes everything -> the first block -> red
    for bad in [
        r#"info!("Search complete for '{}'", request.query)"#,
        r#"warn!("Search failed for query '{}': {}", query, e)"#,
        r#"debug!("Received text from {}: {}", conn_id, text)"#,
        r#"info!("preview: {:?}", &augmented[..augmented.len().min(500)])"#,
        r#"info!("Generated text: '{}'", if description.len() > 50 { &description[..50] } else { &description })"#,
        r#"debug!("Recognized: '{}'", recognized.text)"#,
        r#"info!("prompt={prompt:?}")"#,
        r#"info!("tokens: {:?}", prompt_tokens)"#,
    ] {
        assert!(content_in(bad).is_some(), "{bad}");
    }
    for good in [
        r#"info!("Search complete: {} results for a query of {} chars", n, request.query.chars().count())"#,
        r#"debug!("Received text from {}: {} bytes", conn_id, text.len())"#,
        r#"info!("prompt_chars={}", request.prompt.as_deref().map_or(0, |p| p.chars().count()))"#,
        r#"info!("Step 4: Generating text from embeddings...")"#,
        r#"warn!("the prompt was empty: {}", e)"#,
        r#"info!("Prompt of {} chars tokenized to {} tokens", prompt_text.chars().count(), prompt_tokens.len())"#,
    ] {
        assert_eq!(content_in(good), None, "{good}");
    }
    assert_eq!(
        log_calls("fn f() {\n    info!(\"a ) b\", x(1));\n}").len(),
        1
    );
}

#[test]
fn no_log_line_in_the_content_modules_is_handed_content() {
    // any of the eleven lines fixed for 3.10 put back -> listed here -> red
    let mut found = vec![];
    let mut scanned = 0;
    for file in FILES {
        let src = std::fs::read_to_string(format!("{ROOT}/{file}")).unwrap();
        for (line, call) in log_calls(&src) {
            scanned += 1;
            if let Some(name) = content_in(&call) {
                found.push(format!("{file}:{line}: `{name}`"));
            }
        }
    }
    assert!(
        scanned > 200,
        "the modules' log lines were read ({scanned})"
    );
    assert!(
        found.is_empty(),
        "log lines handed user content:\n{}",
        found.join("\n")
    );
}
