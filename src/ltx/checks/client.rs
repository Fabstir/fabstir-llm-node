// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! HTTP over a Unix socket to the moderation service (wire addendum, section 10b). Every await is bounded: the
//! connect has its own limit and the whole request (handshake, send, response head, response body) runs inside the
//! caller's limit. A caller that times out abandons the wait; the connection is closed with it.

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Verdict bodies are a few hundred bytes; anything larger is not an answer this node recognises.
const MAX_BODY: usize = 256 * 1024;

/// Why a call produced no verdict. The text never carries content, paths or the service's `detail`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError {
    /// The socket is missing, refused or closed mid-call.
    Unreachable,
    Timeout,
    /// Non-200, an unknown error kind, a body that is not JSON, or a field value outside the wire addendum.
    Unrecognised(String),
    /// The service says THIS item cannot be checked (an error kind in [`INPUT_KINDS`]): the job fails, the
    /// service is healthy. Carries the kind.
    Input(String),
}

/// Error kinds that blame the item, not the service. One table, so a kind the service builder confirms later is
/// one line. Every kind NOT listed here is service-caused: `CHECK_UNAVAILABLE` plus the breaker.
pub const INPUT_KINDS: &[&str] = &["UNSUPPORTED_INPUT", "TEXT_TOO_LONG"];

/// The service's limit on a prompt, in characters; the node refuses longer prompts before calling.
pub const MAX_TEXT_CHARS: usize = 8000;

/// A verdict under the `generation` profile (D3, 10b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    /// `BLOCK_POLICY`, `BLOCK_UNRESOLVED`, or any other `BLOCK_*` word: refused, not sealed.
    Block,
    /// `BLOCK_ILLEGAL`: refused and sealed (D15).
    BlockIllegal,
}

/// Derived figures only (D14).
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub verdict: Verdict,
    /// The verdict word as the service sent it (a fixed vocabulary, safe to log).
    pub word: String,
    pub category: Option<String>,
    pub frames: u64,
    pub top_score: f64,
    pub bundle_hash: Option<String>,
    /// Text endpoint only: term rule, explicit, minor (always present on a pass; best effort on a block).
    pub text: Option<(bool, bool, bool)>,
    /// A `BLOCK_*` word outside the wire addendum: refused all the same, and worth an alert (a service that
    /// blocks everything under a new word would otherwise go unnoticed).
    pub unknown_block: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleInfo {
    pub bundle_hash: String,
    pub profile: String,
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Debug, Clone)]
pub struct ServiceClient {
    socket: PathBuf,
}

impl ServiceClient {
    pub fn new(socket: &Path) -> Self {
        ServiceClient {
            socket: socket.to_path_buf(),
        }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        limit: Duration,
    ) -> Result<(u16, Value), CallError> {
        let call = async {
            let stream = tokio::time::timeout(
                CONNECT_TIMEOUT.min(limit),
                tokio::net::UnixStream::connect(&self.socket),
            )
            .await
            .map_err(|_| CallError::Timeout)?
            .map_err(|_| CallError::Unreachable)?;
            let (mut sender, conn) =
                hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream))
                    .await
                    .map_err(|_| CallError::Unreachable)?;
            // dropped (and so aborted) with this future: a timed-out call closes its connection
            let _conn = AbortOnDrop(tokio::spawn(async move {
                let _ = conn.await;
            }));
            let payload = match body {
                Some(v) => serde_json::to_vec(v).map_err(|_| CallError::Unreachable)?,
                None => Vec::new(),
            };
            let req = hyper::Request::builder()
                .method(method)
                .uri(path)
                .header("host", "localhost")
                .header("content-type", "application/json")
                .header("connection", "close")
                .body(Full::new(Bytes::from(payload)))
                .map_err(|_| CallError::Unreachable)?;
            let resp = sender
                .send_request(req)
                .await
                .map_err(|_| CallError::Unreachable)?;
            let status = resp.status().as_u16();
            let bytes = Limited::new(resp.into_body(), MAX_BODY)
                .collect()
                .await
                .map_err(|_| CallError::Unrecognised("body unreadable or too large".into()))?
                .to_bytes();
            let json: Value = serde_json::from_slice(&bytes)
                .map_err(|_| CallError::Unrecognised(format!("status {status}, body not JSON")))?;
            Ok((status, json))
        };
        tokio::time::timeout(limit, call)
            .await
            .map_err(|_| CallError::Timeout)?
    }

    /// `GET /v1/health`: `{"status": "ok"}`.
    pub async fn health(&self, limit: Duration) -> Result<(), CallError> {
        let (status, body) = self.request("GET", "/v1/health", None, limit).await?;
        if status == 200 && body.get("status").and_then(Value::as_str) == Some("ok") {
            Ok(())
        } else {
            Err(CallError::Unrecognised(format!("health status {status}")))
        }
    }

    /// `GET /v1/bundle`: the hash (prefix stripped, lower case) and the profile.
    pub async fn bundle(&self, limit: Duration) -> Result<BundleInfo, CallError> {
        let (status, body) = self.request("GET", "/v1/bundle", None, limit).await?;
        let hash = body
            .get("bundleHash")
            .and_then(Value::as_str)
            .and_then(|h| super::config::normalise_bundle_hash(h).ok());
        let profile = body.get("profile").and_then(Value::as_str);
        match (status, hash, profile) {
            (200, Some(bundle_hash), Some(profile)) => Ok(BundleInfo {
                bundle_hash,
                profile: profile.to_string(),
            }),
            _ => Err(CallError::Unrecognised(format!("bundle status {status}"))),
        }
    }

    /// `POST /v1/moderate` on a staged file (the path as the SERVICE sees it).
    pub async fn moderate(&self, source_path: &str, limit: Duration) -> Result<Outcome, CallError> {
        let body = serde_json::json!({ "sourcePath": source_path });
        let (status, body) = self
            .request("POST", "/v1/moderate", Some(&body), limit)
            .await?;
        parse_outcome(status, &body, false)
    }

    /// `POST /v1/moderate-text`. The text travels only inside this request body.
    pub async fn moderate_text(&self, text: &str, limit: Duration) -> Result<Outcome, CallError> {
        let body = serde_json::json!({ "text": text });
        let (status, body) = self
            .request("POST", "/v1/moderate-text", Some(&body), limit)
            .await?;
        parse_outcome(status, &body, true)
    }
}

/// The node mapping of section 10b (as amended 2026-11-12), pure. An error envelope whose kind is in
/// [`INPUT_KINDS`] blames the item, whatever the HTTP status. Otherwise a body is a verdict only with status 200, a
/// `verdict` key and no `error` key: `PASS` passes; every `BLOCK_*` verdict refuses (contract A5) and
/// `BLOCK_ILLEGAL` is also sealed; everything else (`PASS_RATED`, an unknown word, a `PASS` carrying a category)
/// is unrecognised, which is service-caused. The full kind list is section A8 of the service contract.
pub fn parse_outcome(status: u16, body: &Value, text: bool) -> Result<Outcome, CallError> {
    if let Some(kind) = body.pointer("/error/kind").and_then(Value::as_str) {
        if let Some(known) = INPUT_KINDS.iter().find(|k| **k == kind) {
            return Err(CallError::Input((*known).to_string()));
        }
        // `kind` is the service's fixed vocabulary; `detail` (which may name a path) is never read
        let kind: String = kind
            .chars()
            .filter(|c| c.is_ascii_uppercase() || *c == '_')
            .take(40)
            .collect();
        return Err(CallError::Unrecognised(format!(
            "status {status}, error {kind}"
        )));
    }
    if status != 200 {
        return Err(CallError::Unrecognised(format!("status {status}")));
    }
    if body.get("error").is_some() {
        return Err(CallError::Unrecognised("error envelope".into()));
    }
    let Some(word) = body.get("verdict").and_then(Value::as_str) else {
        return Err(CallError::Unrecognised("no verdict".into()));
    };
    let category = body.get("category").and_then(Value::as_str);
    let verdict = match (word, category) {
        ("PASS", None) => Verdict::Pass,
        ("BLOCK_ILLEGAL", _) => Verdict::BlockIllegal,
        // contract A5: any `BLOCK_*` verdict refuses the job (an unknown one is never mistaken for a pass, and
        // never for an outage either)
        (w, _) if w.starts_with("BLOCK_") => Verdict::Block,
        _ => {
            return Err(CallError::Unrecognised(
                "verdict outside the generation profile".into(),
            ))
        }
    };
    let stats = body.get("stats");
    let frames = stats
        .and_then(|s| s.get("framesSampled"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if !text && verdict == Verdict::Pass && frames == 0 {
        // a pass that looked at nothing is not a check
        return Err(CallError::Unrecognised("pass with no frames judged".into()));
    }
    let flag = |k: &str| stats.and_then(|s| s.get(k)).and_then(Value::as_bool);
    // A pass must show its three signals. A block is a block even with damaged stats: refusing (and sealing a
    // BLOCK_ILLEGAL) must never depend on an optional figure.
    let text_stats = match (flag("termRule"), flag("explicit"), flag("minor")) {
        (Some(t), Some(e), Some(m)) if text => Some((t, e, m)),
        _ if text && verdict == Verdict::Pass => {
            return Err(CallError::Unrecognised("text stats missing".into()))
        }
        _ => None,
    };
    let unknown_block = verdict != Verdict::Pass
        && !matches!(word, "BLOCK_POLICY" | "BLOCK_UNRESOLVED" | "BLOCK_ILLEGAL");
    Ok(Outcome {
        verdict,
        // the word is echoed to logs: keep only the wire alphabet
        word: word
            .chars()
            .filter(|c| c.is_ascii_uppercase() || *c == '_')
            .take(32)
            .collect(),
        category: category.map(|c| {
            c.chars()
                .filter(|c| c.is_ascii_lowercase() || *c == '_')
                .take(32)
                .collect()
        }),
        frames,
        top_score: stats
            .and_then(|s| s.get("maxScore"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        bundle_hash: body
            .get("bundleHash")
            .and_then(Value::as_str)
            .and_then(|h| super::config::normalise_bundle_hash(h).ok()),
        text: text_stats,
        unknown_block,
    })
}
