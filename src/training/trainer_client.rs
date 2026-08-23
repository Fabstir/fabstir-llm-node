// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The trainer-sidecar client — the repo's FIRST Unix-domain-socket HTTP
//! transport (CONTRACT-TRAINING-SERVICE.md v1.0.2 §1: UDS only, never TCP).
//!
//! T3 scope: `/v1/health`, `/v1/status`, `/v1/count`, `/v1/scan` (one-shot
//! buffered requests). The §3.5 held NDJSON `train` stream lands with the T4
//! slice loop. This layer speaks TRANSPORT + §3.6 envelope shapes only; the
//! §3.7 sidecar→wire-code mapping is `core.rs`'s job — keeping the split
//! means a transport bug can never silently re-brand a moderation verdict.
//!
//! One connection per request (hyper 1 http1 handshake over
//! `tokio::net::UnixStream`): count/scan happen once per job, health/status
//! at cadence — connection reuse is not worth a pool's failure modes here.

use std::path::PathBuf;
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Deserialize;

/// A sidecar interaction that produced no usable result (CONTRACT §3.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarFailure {
    /// Connect/IO/timeout/protocol death — the §3.7 `SIDECAR_UNAVAILABLE`
    /// class, before OR after acceptance per the caller's stage.
    Transport(String),
    /// A parsed §3.6 error envelope (4xx pre-attempt reject, or a
    /// 200-with-envelope attempt failure). `kind` drives the §3.7 mapping.
    Envelope {
        status: u16,
        kind: String,
        detail: String,
    },
}

/// `GET /v1/health` pin echo (§3.1) — the node's B.6 accept-time cross-check.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct HealthPins {
    #[serde(rename = "templateHash")]
    pub template_hash: String,
    #[serde(rename = "tokenizerSha256")]
    pub tokenizer_sha256: String,
    #[serde(rename = "stackDigest")]
    pub stack_digest: String,
}

/// `GET /v1/status` (§3.2).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SlotStatus {
    pub slot: String,
}

/// `POST /v1/count` success (§3.3).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub struct CountResult {
    pub tokens: u64,
    pub samples: u64,
}

/// `POST /v1/scan` success (§3.4): a LIVE scanner outcome (verdict or its
/// explicit fail-closed no-verdict) — never conflated with transport death.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ScanResult {
    pub verdict: String,
    #[serde(rename = "policyVersion")]
    pub policy_version: String,
}

#[derive(Debug, Clone)]
pub struct TrainerClient {
    socket_path: PathBuf,
    /// Whole-request deadline (connect + request + body). Count on a
    /// full-size dataset is minutes-scale on real hardware; callers pass a
    /// per-endpoint budget at construction.
    timeout: Duration,
}

#[derive(Deserialize)]
struct EnvelopeBody {
    error: EnvelopeError,
}

#[derive(Deserialize)]
struct EnvelopeError {
    kind: String,
    detail: String,
}

impl TrainerClient {
    pub fn new(socket_path: PathBuf, timeout: Duration) -> Self {
        TrainerClient {
            socket_path,
            timeout,
        }
    }

    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    async fn request_raw(
        &self,
        method: Method,
        path: &str,
        json_body: Option<String>,
    ) -> Result<(StatusCode, Bytes), SidecarFailure> {
        let op = async {
            let stream = tokio::net::UnixStream::connect(&self.socket_path)
                .await
                .map_err(|e| SidecarFailure::Transport(format!("connect: {e}")))?;
            let io = TokioIo::new(stream);
            let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
                .await
                .map_err(|e| SidecarFailure::Transport(format!("handshake: {e}")))?;
            tokio::spawn(conn); // drives the connection; dies with it
            let mut builder = Request::builder()
                .method(method)
                .uri(path)
                .header(hyper::header::HOST, "trainer");
            if json_body.is_some() {
                builder = builder.header(hyper::header::CONTENT_TYPE, "application/json");
            }
            let request = builder
                .body(Full::new(Bytes::from(json_body.unwrap_or_default())))
                .map_err(|e| SidecarFailure::Transport(format!("build request: {e}")))?;
            let response = sender
                .send_request(request)
                .await
                .map_err(|e| SidecarFailure::Transport(format!("request: {e}")))?;
            let status = response.status();
            let body = response
                .into_body()
                .collect()
                .await
                .map_err(|e| SidecarFailure::Transport(format!("body: {e}")))?
                .to_bytes();
            Ok((status, body))
        };
        tokio::time::timeout(self.timeout, op)
            .await
            .map_err(|_| SidecarFailure::Transport(format!("timeout after {:?}", self.timeout)))?
    }

    /// Parse a buffered response: success deserialises to `T`; a §3.6
    /// envelope (any status) becomes `SidecarFailure::Envelope`; anything
    /// unparseable is a Transport-class failure (a diverged sidecar).
    fn parse<T: serde::de::DeserializeOwned>(
        status: StatusCode,
        body: &Bytes,
    ) -> Result<T, SidecarFailure> {
        if let Ok(envelope) = serde_json::from_slice::<EnvelopeBody>(body) {
            return Err(SidecarFailure::Envelope {
                status: status.as_u16(),
                kind: envelope.error.kind,
                detail: envelope.error.detail,
            });
        }
        if status.is_success() {
            return serde_json::from_slice::<T>(body)
                .map_err(|e| SidecarFailure::Transport(format!("unparseable success body: {e}")));
        }
        SidecarFailure::envelope_less(status, body)
    }

    pub async fn health(&self) -> Result<HealthPins, SidecarFailure> {
        #[derive(Deserialize)]
        struct Health {
            pins: HealthPins,
        }
        let (status, body) = self.request_raw(Method::GET, "/v1/health", None).await?;
        Self::parse::<Health>(status, &body).map(|h| h.pins)
    }

    pub async fn status(&self) -> Result<SlotStatus, SidecarFailure> {
        let (status, body) = self.request_raw(Method::GET, "/v1/status", None).await?;
        Self::parse(status, &body)
    }

    pub async fn count(&self, dataset_path: &str) -> Result<CountResult, SidecarFailure> {
        let body = serde_json::json!({ "datasetPath": dataset_path }).to_string();
        let (status, bytes) = self
            .request_raw(Method::POST, "/v1/count", Some(body))
            .await?;
        Self::parse(status, &bytes)
    }

    pub async fn scan(&self, dataset_path: &str) -> Result<ScanResult, SidecarFailure> {
        let body = serde_json::json!({ "datasetPath": dataset_path }).to_string();
        let (status, bytes) = self
            .request_raw(Method::POST, "/v1/scan", Some(body))
            .await?;
        Self::parse(status, &bytes)
    }
}

impl SidecarFailure {
    fn envelope_less<T>(status: StatusCode, body: &Bytes) -> Result<T, SidecarFailure> {
        // A non-2xx WITHOUT the §3.6 envelope: FastAPI's framework 422 shape
        // (`{"detail": …}`) is the one contract-sanctioned case; anything
        // else is a diverged sidecar → transport class.
        if status.as_u16() == 422 {
            return Err(SidecarFailure::Envelope {
                status: 422,
                kind: "FRAMEWORK_422".to_string(),
                detail: String::from_utf8_lossy(body).into_owned(),
            });
        }
        Err(SidecarFailure::Transport(format!(
            "non-envelope status {status}: {}",
            String::from_utf8_lossy(body)
        )))
    }
}

// ---------------------------------------------------------------------------
// The §3.5 held NDJSON train stream (T4). Pre-stream rejects surface as
// Envelope (4xx / 409 SLOT_BUSY); the in-band terminal line surfaces as
// Err(Envelope{status: 200, kind: "TRAIN_FAILURE"}); a died/silent stream is
// Transport — the caller (core's slice loop) holds k and applies §3.7's
// k-split. The silence watchdog enforces the CONTRACT's 120 s rule per read.
// ---------------------------------------------------------------------------

/// The §3.5 request body (field names are the CONTRACT's; the sidecar is
/// extra-forbid, so this struct is the exact wire shape).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainWireRequest {
    pub job_id: u64,
    pub dataset_path: String,
    pub declared_tokens: u64,
    pub epochs: u32,
    pub hyper: crate::training::types::TrainingHyper,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SliceFileRef {
    pub name: String,
    #[serde(rename = "relPath")]
    pub rel_path: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: u64,
}

/// One parsed NDJSON event (§3.5 taxonomy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrainStreamEvent {
    Tick {
        stage: String,
        step: Option<u64>,
        paused: bool,
    },
    Slice {
        index: u64,
        step_from: u64,
        step_to: u64,
        dir: String,
        files: Vec<SliceFileRef>,
    },
    Finalise {
        dir: String,
        files: Vec<SliceFileRef>,
        warnings: Vec<String>,
    },
    Done,
}

/// A live train stream. `next_event` yields events until `Done` (after which
/// it returns `Ok(None)`); the in-band terminal line and every transport
/// death arrive as `Err`.
#[derive(Debug)]
pub struct TrainStream {
    body: hyper::body::Incoming,
    buffer: Vec<u8>,
    silence: Duration,
    /// Set after `done` or the terminal line: further polls answer Ok(None).
    finished: bool,
}

impl TrainStream {
    fn parse_line(&mut self, line: &[u8]) -> Result<Option<TrainStreamEvent>, SidecarFailure> {
        let value: serde_json::Value = serde_json::from_slice(line).map_err(|e| {
            SidecarFailure::Transport(format!(
                "unparseable stream line ({e}): {}",
                String::from_utf8_lossy(line)
            ))
        })?;
        if let Some(error) = value.get("error") {
            // The in-band terminal (§3.5 item 4): the stream is over.
            self.finished = true;
            return Err(SidecarFailure::Envelope {
                status: 200,
                kind: error
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .unwrap_or("TRAIN_FAILURE")
                    .to_string(),
                detail: error
                    .get("detail")
                    .and_then(|d| d.as_str())
                    .unwrap_or_default()
                    .to_string(),
            });
        }
        match value.get("event").and_then(|e| e.as_str()) {
            Some("tick") => Ok(Some(TrainStreamEvent::Tick {
                stage: value
                    .get("stage")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string(),
                step: value.get("step").and_then(|s| s.as_u64()),
                paused: value
                    .get("paused")
                    .and_then(|p| p.as_bool())
                    .unwrap_or(false),
            })),
            Some("slice") => {
                let files: Vec<SliceFileRef> =
                    serde_json::from_value(value.get("files").cloned().unwrap_or_default())
                        .map_err(|e| {
                            SidecarFailure::Transport(format!("slice files unparseable: {e}"))
                        })?;
                Ok(Some(TrainStreamEvent::Slice {
                    index: value.get("index").and_then(|i| i.as_u64()).ok_or_else(|| {
                        SidecarFailure::Transport("slice event without index".to_string())
                    })?,
                    step_from: value.get("stepFrom").and_then(|i| i.as_u64()).unwrap_or(0),
                    step_to: value.get("stepTo").and_then(|i| i.as_u64()).unwrap_or(0),
                    dir: value
                        .get("dir")
                        .and_then(|d| d.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    files,
                }))
            }
            Some("finalise") => {
                let adapter = value.get("adapter").cloned().unwrap_or_default();
                let files: Vec<SliceFileRef> =
                    serde_json::from_value(adapter.get("files").cloned().unwrap_or_default())
                        .map_err(|e| {
                            SidecarFailure::Transport(format!("finalise files unparseable: {e}"))
                        })?;
                let warnings: Vec<String> = serde_json::from_value(
                    value
                        .get("warnings")
                        .cloned()
                        .unwrap_or_else(|| serde_json::Value::Array(Vec::new())),
                )
                .unwrap_or_default();
                Ok(Some(TrainStreamEvent::Finalise {
                    dir: adapter
                        .get("dir")
                        .and_then(|d| d.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    files,
                    warnings,
                }))
            }
            Some("done") => {
                self.finished = true;
                Ok(Some(TrainStreamEvent::Done))
            }
            other => Err(SidecarFailure::Transport(format!(
                "unknown stream event {other:?}"
            ))),
        }
    }

    /// Next NDJSON event. `Ok(None)` only AFTER a clean `done`; a body that
    /// ends without `done`/terminal is the §3.7 died-stream Transport case;
    /// a read quieter than the watchdog is the silence-rule Transport case.
    pub async fn next_event(&mut self) -> Result<Option<TrainStreamEvent>, SidecarFailure> {
        use http_body_util::BodyExt;
        loop {
            if let Some(pos) = self.buffer.iter().position(|b| *b == b'\n') {
                let mut split: Vec<u8> = self.buffer.drain(..=pos).collect();
                split.pop(); // the newline
                if split.is_empty() {
                    continue;
                }
                return self.parse_line(&split);
            }
            if self.finished {
                return Ok(None);
            }
            let frame = tokio::time::timeout(self.silence, self.body.frame())
                .await
                .map_err(|_| {
                    SidecarFailure::Transport(format!(
                        "silence rule: no stream bytes within {:?}",
                        self.silence
                    ))
                })?;
            match frame {
                Some(Ok(frame)) => {
                    if let Some(data) = frame.data_ref() {
                        self.buffer.extend_from_slice(data);
                    }
                }
                Some(Err(e)) => return Err(SidecarFailure::Transport(format!("stream read: {e}"))),
                None => {
                    return Err(SidecarFailure::Transport(
                        "stream ended without a terminal line (died stream)".to_string(),
                    ))
                }
            }
        }
    }
}

impl TrainerClient {
    /// Open the §3.5 held stream. `silence_timeout` is the per-read watchdog
    /// (the CONTRACT's 120 s rule; tests shrink it). Pre-stream rejects
    /// (4xx / 409 SLOT_BUSY) surface as `Envelope`.
    pub async fn train(
        &self,
        body: &TrainWireRequest,
        silence_timeout: Duration,
    ) -> Result<TrainStream, SidecarFailure> {
        use http_body_util::BodyExt;
        let stream = tokio::net::UnixStream::connect(&self.socket_path)
            .await
            .map_err(|e| SidecarFailure::Transport(format!("connect: {e}")))?;
        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| SidecarFailure::Transport(format!("handshake: {e}")))?;
        tokio::spawn(conn);
        let json_body = serde_json::to_string(body)
            .map_err(|e| SidecarFailure::Transport(format!("encode request: {e}")))?;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/train")
            .header(hyper::header::HOST, "trainer")
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .body(Full::new(Bytes::from(json_body)))
            .map_err(|e| SidecarFailure::Transport(format!("build request: {e}")))?;
        let response = tokio::time::timeout(self.timeout, sender.send_request(request))
            .await
            .map_err(|_| SidecarFailure::Transport(format!("timeout after {:?}", self.timeout)))?
            .map_err(|e| SidecarFailure::Transport(format!("request: {e}")))?;
        let status = response.status();
        if status != StatusCode::OK {
            // Pre-stream reject: buffered envelope (or framework 422).
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| SidecarFailure::Transport(format!("body: {e}")))?
                .to_bytes();
            return match Self::parse::<serde_json::Value>(status, &bytes) {
                Err(failure) => Err(failure),
                Ok(_) => Err(SidecarFailure::Transport(format!(
                    "non-200 train response without an envelope: {status}"
                ))),
            };
        }
        Ok(TrainStream {
            body: response.into_body(),
            buffer: Vec::new(),
            silence: silence_timeout,
            finished: false,
        })
    }
}
