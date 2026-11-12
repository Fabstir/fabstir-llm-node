// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks (N10) test support: a stub moderation service on a Unix socket that speaks the real wire shapes
//! (contract v1.1 addendum, sections A2 to A4 and A8). Like the real service it has a ROOT (the staging area as
//! IT sees it) and refuses any path outside it, and it records what each file it was given really was (length,
//! sha256, first bytes), so a node that sent the wrong path or the wrong bytes is caught. Every behaviour is
//! state-driven: a test sets what the stub answers, never how many polls it takes.
#![allow(dead_code)]

pub use super::checks_rig::*;
use bytes::Bytes;
use fabstir_llm_node::ltx::checks::PROBE_TEXT;
use fabstir_llm_node::ltx::provenance::SELF_TEST_MP4;
use http_body_util::{BodyExt, Full};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const BUNDLE: &str = "7f3a7f3a7f3a7f3a7f3a7f3a7f3a7f3a7f3a7f3a7f3a7f3a7f3a7f3a7f3a7f3a";

/// What the stub answers.
#[derive(Clone, Debug)]
pub enum Reply {
    Pass,
    /// A verdict word and its category.
    Verdict(&'static str, Option<&'static str>),
    /// A status and a JSON body, verbatim (error envelopes, damaged answers).
    Raw(u16, Value),
    NotJson,
    /// Never answers (inside any test's patience).
    Hang,
}

/// One file as the service found it when it was called.
#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    pub ext: String,
    pub len: usize,
    pub sha256: String,
    pub head: Vec<u8>,
    /// It was the node's built-in probe clip.
    pub probe: bool,
}

pub struct CheckSvc {
    pub socket: PathBuf,
    /// The staging area as the service sees it. A path outside it is refused, as the real service does.
    pub root: PathBuf,
    /// One line per request: `health`, `bundle`, `moderate <ext>`, `moderate-text`, `probe-text`,
    /// `moderate outside-root`, `moderate missing`.
    pub calls: Mutex<Vec<String>>,
    /// Every file `/v1/moderate` was given.
    pub files: Mutex<Vec<Seen>>,
    /// Every text the text endpoint received (the probe sentence excluded).
    pub texts: Mutex<Vec<String>>,
    /// Media rule: files with this extension, from the nth such call on (0-based), get this reply. Others pass.
    pub media_rule: Mutex<Option<(&'static str, usize, Reply)>>,
    /// With this set the media rule spares the node's probe clip: "one customer's odd file", not an outage.
    pub spare_probe: AtomicBool,
    /// With this set EVERY `/v1/moderate` call gets it: the service itself is broken.
    pub fail_all: Mutex<Option<Reply>>,
    /// With this set every media file gets the reply EXCEPT the probe clip and the files whose sha256 is listed
    /// (a job's inputs): a service that would refuse any result it was ever shown.
    pub block_all_but: Mutex<Option<(Vec<String>, Reply)>>,
    pub text_reply: Mutex<Reply>,
    /// A pause before each moderate or text answer (a slow but healthy service).
    pub delay_ms: AtomicU64,
    pub profile: Mutex<String>,
    pub bundle_hash: Mutex<String>,
    seen: Mutex<HashMap<String, usize>>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

type Resp = hyper::Response<Full<Bytes>>;

fn resp(status: u16, body: String) -> Resp {
    hyper::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap()
}

impl CheckSvc {
    pub fn count(&self, what: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.as_str() == what)
            .count()
    }

    /// The files of one extension the service was given, the probe clip left out.
    pub fn seen(&self, ext: &str) -> Vec<Seen> {
        self.files
            .lock()
            .unwrap()
            .iter()
            .filter(|f| f.ext == ext && !f.probe)
            .cloned()
            .collect()
    }

    pub fn set_media(&self, ext: &'static str, from_nth: usize, reply: Reply) {
        *self.media_rule.lock().unwrap() = Some((ext, from_nth, reply));
    }

    pub fn set_text(&self, reply: Reply) {
        *self.text_reply.lock().unwrap() = reply;
    }

    /// The service goes away: the socket is removed and nothing accepts.
    pub fn stop(&self) {
        if let Some(t) = self.task.lock().unwrap().take() {
            t.abort();
        }
        let _ = std::fs::remove_file(&self.socket);
    }

    async fn render(&self, reply: Reply, text: bool) -> Resp {
        tokio::time::sleep(Duration::from_millis(self.delay_ms.load(Ordering::SeqCst))).await;
        let (word, category) = match reply {
            Reply::Pass => ("PASS", None),
            Reply::Verdict(w, c) => (w, c),
            Reply::Raw(status, body) => return resp(status, body.to_string()),
            Reply::NotJson => return resp(200, "<html>bad gateway</html>".to_string()),
            Reply::Hang => {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                ("PASS", None)
            }
        };
        let body = if text {
            json!({"verdict": word, "category": category,
                   "stats": {"termRule": word == "BLOCK_ILLEGAL", "explicit": word == "BLOCK_POLICY",
                             "minor": word == "BLOCK_ILLEGAL" || word == "BLOCK_UNRESOLVED"}})
        } else {
            json!({"schemaVersion": 1, "bundleVersion": 2, "samplerVersion": 2,
                   "bundleHash": format!("sha256:{}", self.bundle_hash.lock().unwrap()),
                   "verdict": word, "rating": null, "descriptors": [], "category": category,
                   "stats": {"framesSampled": 24, "maxScore": if word == "PASS" { 0.02 } else { 0.97 },
                             "escalated": 8}})
        };
        resp(200, body.to_string())
    }

    async fn moderate(&self, src: &str) -> Resp {
        let note = |line: &str| self.calls.lock().unwrap().push(line.to_string());
        if !std::path::Path::new(src).starts_with(&self.root) {
            // the node handed over ITS view of the staging area, not the service's
            note("moderate outside-root");
            let e = json!({"error": {"kind": "SOURCE_OUTSIDE_ROOT", "detail": src}});
            return resp(403, e.to_string());
        }
        let Ok(bytes) = std::fs::read(src) else {
            note("moderate missing");
            let e = json!({"error": {"kind": "SOURCE_NOT_FOUND", "detail": src}});
            return resp(404, e.to_string());
        };
        let ext = src.rsplit('.').next().unwrap_or("").to_string();
        let probe = bytes == SELF_TEST_MP4;
        note(&format!("moderate {ext}"));
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let blocked = self.block_all_but.lock().unwrap().clone();
        let blocked = blocked.filter(|(spared, _)| !probe && !spared.contains(&sha256));
        self.files.lock().unwrap().push(Seen {
            path: src.to_string(),
            ext: ext.clone(),
            len: bytes.len(),
            sha256,
            head: bytes.iter().take(12).copied().collect(),
            probe,
        });
        let nth = {
            let mut seen = self.seen.lock().unwrap();
            let n = seen.entry(ext.clone()).or_insert(0);
            *n += 1;
            *n - 1
        };
        // cloned out first: a lock guard must not be held across the await
        let broken = self.fail_all.lock().unwrap().clone();
        if let Some(reply) = broken {
            return self.render(reply, false).await;
        }
        if let Some((_, reply)) = blocked {
            return self.render(reply, false).await;
        }
        let rule = self.media_rule.lock().unwrap().clone();
        let spared = probe && self.spare_probe.load(Ordering::SeqCst);
        let reply = match rule {
            Some((e, from, reply)) if e == ext && nth >= from && !spared => reply,
            _ => Reply::Pass,
        };
        self.render(reply, false).await
    }

    async fn handle(self: Arc<Self>, req: hyper::Request<hyper::body::Incoming>) -> Resp {
        let (method, path) = (
            req.method().as_str().to_string(),
            req.uri().path().to_string(),
        );
        let raw = req
            .into_body()
            .collect()
            .await
            .map(|b| b.to_bytes())
            .unwrap_or_default();
        let body: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
        let note = |line: &str| self.calls.lock().unwrap().push(line.to_string());
        match (method.as_str(), path.as_str()) {
            ("GET", "/v1/health") => {
                note("health");
                let ok = json!({"status": "ok", "models": {"screen": true, "judge": true}});
                resp(200, ok.to_string())
            }
            ("GET", "/v1/bundle") => {
                note("bundle");
                let body = json!({"bundleHash": format!("sha256:{}", self.bundle_hash.lock().unwrap()),
                                  "bundleVersion": 2, "profile": *self.profile.lock().unwrap()});
                resp(200, body.to_string())
            }
            ("POST", "/v1/moderate") => match body.get("sourcePath").and_then(Value::as_str) {
                Some(src) => self.moderate(src).await,
                None => resp(
                    422,
                    json!({"detail": [{"msg": "field required"}]}).to_string(),
                ),
            },
            ("POST", "/v1/moderate-text") => {
                let Some(text) = body.get("text").and_then(Value::as_str) else {
                    note("moderate-text bad-request");
                    let e = json!({"error": {"kind": "BAD_REQUEST", "detail": "text must be a string"}});
                    return resp(422, e.to_string());
                };
                let reply = self.text_reply.lock().unwrap().clone();
                if text == PROBE_TEXT {
                    // a healthy service passes the benign sentence whatever it says about a customer's prompt
                    note("probe-text");
                    let reply = match reply {
                        Reply::Verdict(..) => Reply::Pass,
                        other => other,
                    };
                    return self.render(reply, true).await;
                }
                note("moderate-text");
                self.texts.lock().unwrap().push(text.to_string());
                self.render(reply, true).await
            }
            _ => resp(404, json!({"detail": "Not Found"}).to_string()),
        }
    }
}

/// Start the stub service on `socket`, with `root` as its view of the staging area.
pub fn spawn_service(socket: PathBuf, root: PathBuf) -> Arc<CheckSvc> {
    let _ = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let svc = Arc::new(CheckSvc {
        socket,
        root,
        calls: Mutex::new(vec![]),
        files: Mutex::new(vec![]),
        texts: Mutex::new(vec![]),
        media_rule: Mutex::new(None),
        spare_probe: AtomicBool::new(false),
        fail_all: Mutex::new(None),
        block_all_but: Mutex::new(None),
        text_reply: Mutex::new(Reply::Pass),
        delay_ms: AtomicU64::new(0),
        profile: Mutex::new("generation".into()),
        bundle_hash: Mutex::new(BUNDLE.into()),
        seen: Mutex::new(HashMap::new()),
        task: Mutex::new(None),
    });
    let accept = svc.clone();
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let svc = accept.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let svc = svc.clone();
                    async move { Ok::<_, std::convert::Infallible>(svc.handle(req).await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    *svc.task.lock().unwrap() = Some(task);
    svc
}
