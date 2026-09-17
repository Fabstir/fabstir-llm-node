// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 (P2.3) — thin client for the dstack guest agent.
//!
//! Inside a Phala CVM the guest agent listens on `/var/run/dstack.sock` and
//! speaks JSON over HTTP/1.1: `POST /GetQuote {"report_data": "<hex>"}` returns
//! the TDX quote, the event log and the VM config; `POST /Info {}` returns the
//! instance's own view of its measurements. Two endpoints, two structs, one
//! transport; nothing else of the dstack API is needed for attested inference.
//!
//! Hand-rolled on the crate's existing UDS pattern (`training::trainer_client`,
//! hyper 1 over `tokio::net::UnixStream`) rather than the `dstack-sdk` crate,
//! which would pull `alloy` and a second `reqwest` into the build (expert
//! decision, 2026-09-17). `DSTACK_SIMULATOR_ENDPOINT` is honoured exactly as
//! the SDK honours it: a path means a Unix socket, an `http://` URL means the
//! simulator's TCP listener.
//!
//! Everything returned here is UNVERIFIED. The quote is bytes the broker will
//! verify with dcap-qvl; `/Info` is what the guest says about itself and is
//! used only for logs and for choosing what to send. No security decision is
//! made on this side of the socket.

use crate::tee::types::{TeeError, TeeResult, REPORT_DATA_LEN};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

/// Default guest-agent socket inside a dstack CVM.
pub const DEFAULT_SOCKET: &str = "/var/run/dstack.sock";
/// Environment variable the simulator (and the official SDKs) use to redirect.
pub const SIMULATOR_ENV: &str = "DSTACK_SIMULATOR_ENDPOINT";

/// Where the guest agent is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// A Unix socket path (the real CVM, or a simulator socket).
    Unix(PathBuf),
    /// `host:port` of an HTTP simulator listener (no TLS; local only), plus an
    /// optional base path the RPC paths are appended to (the official SDKs use
    /// the value as a base URL, so `http://h:p/prpc` posts to `/prpc/GetQuote`).
    Http {
        host: String,
        port: u16,
        base_path: String,
    },
}

/// The dstack guest-agent client.
#[derive(Debug, Clone)]
pub struct DstackClient {
    endpoint: Endpoint,
    /// Whole-request deadline (connect + request + body). A real quote takes
    /// well under a second; the simulator is instant.
    timeout: Duration,
}

/// `POST /GetQuote` result, hex fields decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteResponse {
    /// The raw TDX quote (DCAP v4 format), as the broker's dcap-qvl expects it.
    pub quote: Vec<u8>,
    /// The event log, UTF-8 JSON exactly as the agent returned it.
    pub event_log: String,
    /// The report_data the agent embedded (echo of what we sent). Empty on
    /// older agents that do not echo it.
    pub report_data: Vec<u8>,
    /// The VM configuration JSON (vCPU, RAM, GPU count, knobs), exactly as
    /// returned. Empty on agents that do not report it.
    pub vm_config: String,
}

/// One RTMR event as the guest agent reports it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EventLogEntry {
    pub imr: u32,
    pub event_type: u32,
    pub digest: String,
    pub event: String,
    pub event_payload: String,
}

/// The guest's own view of its TCB (`/Info` → `tcb_info`, which the agent
/// returns as a JSON *string* inside the JSON response).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TcbInfo {
    pub mrtd: String,
    pub rtmr0: String,
    pub rtmr1: String,
    pub rtmr2: String,
    pub rtmr3: String,
    #[serde(default)]
    pub os_image_hash: String,
    pub compose_hash: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub app_compose: String,
    #[serde(default)]
    pub event_log: Vec<EventLogEntry>,
}

/// `POST /Info` result.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InfoResponse {
    pub app_id: String,
    pub instance_id: String,
    #[serde(default)]
    pub app_name: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub mr_aggregated: String,
    #[serde(default)]
    pub os_image_hash: String,
    #[serde(default)]
    pub key_provider_info: String,
    pub compose_hash: String,
    #[serde(default)]
    pub vm_config: String,
    pub tcb_info: TcbInfo,
}

#[derive(Deserialize)]
struct RawQuoteResponse {
    quote: String,
    event_log: String,
    #[serde(default)]
    report_data: String,
    #[serde(default)]
    vm_config: String,
}

impl DstackClient {
    /// Client for `endpoint` with a whole-request `timeout`.
    pub fn new(endpoint: Endpoint, timeout: Duration) -> Self {
        Self { endpoint, timeout }
    }

    /// The production default: `/var/run/dstack.sock`, or whatever
    /// `DSTACK_SIMULATOR_ENDPOINT` names (a socket path, or `http://host:port`).
    pub fn from_env() -> TeeResult<Self> {
        let endpoint = match std::env::var(SIMULATOR_ENV) {
            Ok(v) if !v.trim().is_empty() => Self::parse_endpoint(v.trim())?,
            _ => Endpoint::Unix(PathBuf::from(DEFAULT_SOCKET)),
        };
        Ok(Self::new(endpoint, Duration::from_secs(30)))
    }

    /// `http://host:port[/base]` → HTTP, exactly as the official SDKs read it: a
    /// base URL, so a path prefix is kept and the RPC paths are appended to it
    /// (an IPv6 literal may be bracketed); anything else → a Unix socket path.
    pub fn parse_endpoint(s: &str) -> TeeResult<Endpoint> {
        if let Some(rest) = s.strip_prefix("http://") {
            let (authority, base_path) = match rest.split_once('/') {
                Some((a, p)) => (a, format!("/{}", p.trim_matches('/'))),
                None => (rest, String::new()),
            };
            let base_path = if base_path == "/" {
                String::new()
            } else {
                base_path
            };
            let (host, port) = authority.rsplit_once(':').ok_or_else(|| {
                TeeError::Dstack(format!(
                    "{SIMULATOR_ENV}: expected http://host:port, got {s}"
                ))
            })?;
            let host = host.trim_start_matches('[').trim_end_matches(']');
            if host.is_empty() {
                return Err(TeeError::Dstack(format!(
                    "{SIMULATOR_ENV}: empty host in {s}"
                )));
            }
            let port: u16 = port
                .parse()
                .map_err(|_| TeeError::Dstack(format!("{SIMULATOR_ENV}: bad port in {s}")))?;
            Ok(Endpoint::Http {
                host: host.to_string(),
                port,
                base_path,
            })
        } else if s.starts_with("https://") {
            Err(TeeError::Dstack(format!(
                "{SIMULATOR_ENV}: https is not supported (the agent is local); got {s}"
            )))
        } else {
            Ok(Endpoint::Unix(PathBuf::from(s)))
        }
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// `POST /GetQuote`: ask the CPU TEE to sign `report_data` (exactly 64
    /// bytes; see `types::report_data`). Returns the quote and its companions.
    pub async fn get_quote(&self, report_data: &[u8; REPORT_DATA_LEN]) -> TeeResult<QuoteResponse> {
        let body = serde_json::json!({ "report_data": hex::encode(report_data) }).to_string();
        let raw: RawQuoteResponse = self.post_json("/GetQuote", body).await?;
        let quote = hex::decode(raw.quote.trim_start_matches("0x"))
            .map_err(|e| TeeError::Dstack(format!("GetQuote: quote is not hex: {e}")))?;
        if quote.is_empty() {
            return Err(TeeError::Dstack("GetQuote: empty quote".into()));
        }
        let echoed = if raw.report_data.is_empty() {
            Vec::new()
        } else {
            hex::decode(raw.report_data.trim_start_matches("0x"))
                .map_err(|e| TeeError::Dstack(format!("GetQuote: report_data is not hex: {e}")))?
        };
        // The agent may pad report_data to 64 bytes; when it echoes it, it must
        // be OURS. A different value here means the quote was not made for us.
        if !echoed.is_empty() && echoed != report_data {
            return Err(TeeError::Dstack(
                "GetQuote: echoed report_data differs from the one requested".into(),
            ));
        }
        Ok(QuoteResponse {
            quote,
            event_log: raw.event_log,
            report_data: echoed,
            vm_config: raw.vm_config,
        })
    }

    /// `POST /Info`: the instance's self-reported identity and measurements.
    pub async fn info(&self) -> TeeResult<InfoResponse> {
        let mut value: serde_json::Value = self.post_json("/Info", "{}".to_string()).await?;
        // `tcb_info` arrives as a JSON string; inline it so one deserialise does the rest.
        if let Some(s) = value.get("tcb_info").and_then(|v| v.as_str()) {
            let parsed: serde_json::Value = serde_json::from_str(s)
                .map_err(|e| TeeError::Dstack(format!("Info: tcb_info is not JSON: {e}")))?;
            value["tcb_info"] = parsed;
        }
        serde_json::from_value(value).map_err(|e| TeeError::Dstack(format!("Info: {e}")))
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        json_body: String,
    ) -> TeeResult<T> {
        let (status, bytes) = self.request_raw(path, json_body).await?;
        if status != StatusCode::OK {
            let text = String::from_utf8_lossy(&bytes);
            let text = text.chars().take(200).collect::<String>();
            return Err(TeeError::Dstack(format!("{path}: HTTP {status}: {text}")));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            TeeError::Dstack(format!("{path}: response is not the expected JSON: {e}"))
        })
    }

    async fn request_raw(&self, path: &str, json_body: String) -> TeeResult<(StatusCode, Bytes)> {
        let op = async {
            let uri = match &self.endpoint {
                Endpoint::Http { base_path, .. } => format!("{base_path}{path}"),
                Endpoint::Unix(_) => path.to_string(),
            };
            let request = Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(hyper::header::HOST, "dstack")
                .header(hyper::header::CONTENT_TYPE, "application/json")
                .body(Full::new(Bytes::from(json_body)))
                .map_err(|e| TeeError::Dstack(format!("build request: {e}")))?;
            match &self.endpoint {
                Endpoint::Unix(p) => {
                    let stream = tokio::net::UnixStream::connect(p)
                        .await
                        .map_err(|e| TeeError::Dstack(format!("connect {}: {e}", p.display())))?;
                    Self::send(TokioIo::new(stream), request).await
                }
                Endpoint::Http { host, port, .. } => {
                    let stream = tokio::net::TcpStream::connect((host.as_str(), *port))
                        .await
                        .map_err(|e| TeeError::Dstack(format!("connect {host}:{port}: {e}")))?;
                    Self::send(TokioIo::new(stream), request).await
                }
            }
        };
        tokio::time::timeout(self.timeout, op)
            .await
            .map_err(|_| TeeError::Dstack(format!("timeout after {:?}", self.timeout)))?
    }

    async fn send<I>(io: I, request: Request<Full<Bytes>>) -> TeeResult<(StatusCode, Bytes)>
    where
        I: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
    {
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| TeeError::Dstack(format!("handshake: {e}")))?;
        tokio::spawn(conn); // drives the connection; dies with it
        let response = sender
            .send_request(request)
            .await
            .map_err(|e| TeeError::Dstack(format!("request: {e}")))?;
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .map_err(|e| TeeError::Dstack(format!("body: {e}")))?
            .to_bytes();
        Ok((status, body))
    }
}
