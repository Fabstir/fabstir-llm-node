// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! `MemoHttp`: `EgressClient` GET + disk memo (design §12, gates A-15/A-16).
//! Implements `dcap_qvl::http::HttpClient`.
//!
//! One instance per HALF of a request (the collateral pass and the JWKS fetch never
//! share one; eight permits run concurrently and a shared session would let request
//! A's `Ok` commit request B's bodies). A cheap `Clone` over `Arc<Inner>`: dcap-qvl's
//! `CollateralClient::new` takes the client by value, the caller keeps a clone to
//! read and drive the session.
//!
//! Rules:
//! - normal mode, memo-first-when-fresh (collateral instances only): a COMMITTED memo
//!   younger than `fresh` is served without touching the network; else network;
//! - on a transport error or a non-2xx: serve the committed memo if present, else fail;
//! - only 2xx bodies are candidates; they are STAGED in the session and COMMITTED by
//!   the caller ([`MemoHttp::commit`]) only after `dcap_qvl::verify::verify` (or the
//!   JWKS parse + token verification) returned `Ok`;
//! - a mode switch DROPS the stage; a committed entry is never deleted, only
//!   replaced by a later verified pass;
//! - memo-only mode never touches the network; network-only mode never reads the memo.

use crate::kbs::egress::{EgressClient, EgressError, Response};
use dcap_qvl::http::{HttpClient, HttpResponse};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    NetworkOnly,
    MemoOnly,
}

/// The on-disk memo entry (`memo/<sha256(url)>.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoEntry {
    pub url: String,
    pub fetched_at: u64,
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body_b64: String,
}

impl MemoEntry {
    fn to_response(&self) -> Option<Response> {
        use base64::Engine;
        let body = base64::engine::general_purpose::STANDARD
            .decode(&self.body_b64)
            .ok()?;
        Some(Response {
            status: self.status,
            headers: self.headers.clone(),
            body,
        })
    }
}

#[derive(Debug, Default)]
struct Session {
    mode: Option<Mode>,
    staged: Vec<MemoEntry>,
    from_memo: Vec<String>,
    from_network: Vec<String>,
    /// URLs the network was tried for, successfully or not.
    attempted: Vec<String>,
}

/// What the retry logic reads after a pass (design §12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    /// Some input of this pass was served from the memo.
    pub from_memo: bool,
    /// Some input of this pass came from the network.
    pub from_network: bool,
    /// Every URL that returned a body in this pass has a committed memo entry.
    pub all_touched_have_memo: bool,
    /// Bodies staged in this pass.
    pub staged: usize,
    /// Any URL was asked for at all (memo or network); false = the consumer failed
    /// before its first request (e.g. dcap-qvl refusing the quote's own PCK chain).
    pub any_request: bool,
    /// Some URL was attempted, returned no body, and has no committed memo either:
    /// a memo-only pass cannot succeed.
    pub failed_without_memo: bool,
    /// The network was tried for at least one NON-optional URL (successfully or
    /// not); false = the whole pass ran on the memo alone. dcap-qvl's `rootcacrl`
    /// probe does not count: a PCCS that 404s it on every request must not hide a
    /// memo-alone pass.
    pub attempted_network: bool,
    /// Every URL asked for in this pass (memo or network), in order.
    pub urls: Vec<String>,
}

struct Inner {
    egress: EgressClient,
    memo_dir: PathBuf,
    /// Serve a committed memo younger than this without touching the network
    /// (collateral instances; `Duration::ZERO` for the JWKS instance).
    fresh: Duration,
    timeout: Duration,
    max_bytes: usize,
    session: Mutex<Session>,
}

#[derive(Clone)]
pub struct MemoHttp {
    inner: Arc<Inner>,
}

impl MemoHttp {
    /// A collateral instance: memo-first when the committed entry is younger than `fresh`.
    pub fn collateral(
        egress: EgressClient,
        memo_dir: PathBuf,
        fresh: Duration,
        timeout: Duration,
        max_bytes: usize,
    ) -> Self {
        Self::build(egress, memo_dir, fresh, timeout, max_bytes)
    }

    /// A JWKS instance: network-first (the `kid` rotates), memo only as the fallback.
    pub fn network_first(
        egress: EgressClient,
        memo_dir: PathBuf,
        timeout: Duration,
        max_bytes: usize,
    ) -> Self {
        Self::build(egress, memo_dir, Duration::ZERO, timeout, max_bytes)
    }

    fn build(
        egress: EgressClient,
        memo_dir: PathBuf,
        fresh: Duration,
        timeout: Duration,
        max_bytes: usize,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                egress,
                memo_dir,
                fresh,
                timeout,
                max_bytes,
                session: Mutex::new(Session {
                    mode: Some(Mode::Normal),
                    ..Default::default()
                }),
            }),
        }
    }

    /// Switch mode for the next pass. DROPS the stage and the served sets.
    pub fn set_mode(&self, mode: Mode) {
        let mut s = self.lock();
        *s = Session {
            mode: Some(mode),
            ..Default::default()
        };
    }

    pub fn mode(&self) -> Mode {
        self.lock().mode.unwrap_or(Mode::Normal)
    }

    pub fn summary(&self) -> SessionSummary {
        let s = self.lock();
        let touched: Vec<&String> = s.from_memo.iter().chain(s.from_network.iter()).collect();
        SessionSummary {
            from_memo: !s.from_memo.is_empty(),
            from_network: !s.from_network.is_empty(),
            all_touched_have_memo: !touched.is_empty()
                && touched.iter().all(|u| self.read_committed(u).is_some()),
            staged: s.staged.len(),
            any_request: !s.attempted.is_empty() || !s.from_memo.is_empty(),
            attempted_network: s.attempted.iter().any(|u| !is_optional_probe(u)),
            failed_without_memo: s
                .attempted
                .iter()
                .filter(|u| !is_optional_probe(u))
                .any(|u| {
                    !s.from_network.contains(u)
                        && !s.from_memo.contains(u)
                        && self.read_committed(u).is_none()
                }),
            urls: {
                let mut v = s.attempted.clone();
                for u in &s.from_memo {
                    if !v.contains(u) {
                        v.push(u.clone());
                    }
                }
                v
            },
        }
    }

    /// Commit the staged bodies (a unique temp file per writer, then `persist`).
    /// Returns the number of entries written; a write failure is logged, not fatal.
    pub fn commit(&self) -> usize {
        let staged: Vec<MemoEntry> = std::mem::take(&mut self.lock().staged);
        let mut n = 0;
        for e in staged {
            match self.write_entry(&e) {
                Ok(()) => n += 1,
                Err(err) => {
                    tracing::warn!(url = %e.url, error = %err, "memo write failed (non-fatal)")
                }
            }
        }
        n
    }

    /// Drop the staged body for `url` (a consumer whose 2xx body did not parse must
    /// call this BEFORE falling back to the memo, or a later `commit` writes the
    /// garbage over the good entry). The URL no longer counts as served from the
    /// network; a following [`MemoHttp::committed`] read counts as served from the memo.
    pub fn discard(&self, url: &str) {
        let mut s = self.lock();
        s.staged.retain(|e| e.url != url);
        s.from_network.retain(|u| u != url);
    }

    /// The committed entry for `url`, if any (for a consumer whose 2xx body did not
    /// parse and wants the fallback the transport path would have taken); recorded
    /// as served from the memo.
    pub fn committed(&self, url: &str) -> Option<Response> {
        let r = self.read_committed(url).and_then(|e| e.to_response());
        if r.is_some() {
            self.lock().from_memo.push(url.to_string());
        }
        r
    }

    pub fn memo_path(&self, url: &str) -> PathBuf {
        memo_path(&self.inner.memo_dir, url)
    }

    /// The GET with the §12 rules; used by the `HttpClient` impl and by the JWKS fetch.
    pub async fn get_with_rules(&self, url: &str) -> Result<Response, EgressError> {
        // The allow-list is checked before anything, memo reads included.
        self.inner.egress.check_url(url)?;
        let mode = self.mode();
        if mode == Mode::MemoOnly {
            self.lock().attempted.push(url.to_string());
            return match self.read_committed(url) {
                Some(e) => {
                    tracing::error!(
                        url,
                        fetched_at = e.fetched_at,
                        "CRITICAL: served from memo (memo-only pass)"
                    );
                    self.lock().from_memo.push(url.to_string());
                    e.to_response()
                        .ok_or_else(|| EgressError::Transport("memo entry is not decodable".into()))
                }
                None => Err(EgressError::Transport(format!(
                    "memo-only: no committed memo for {url}"
                ))),
            };
        }
        if mode == Mode::Normal && !self.inner.fresh.is_zero() {
            if let Some(e) = self.read_committed(url) {
                let now = now_unix();
                // A future `fetched_at` (the clock stepped back after a commit) is not
                // fresh: fall through to the network rather than serve it indefinitely.
                if e.fetched_at <= now && now - e.fetched_at < self.inner.fresh.as_secs() {
                    self.lock().from_memo.push(url.to_string());
                    return e.to_response().ok_or_else(|| {
                        EgressError::Transport("memo entry is not decodable".into())
                    });
                }
            }
        }
        self.lock().attempted.push(url.to_string());
        let net = self
            .inner
            .egress
            .get(url, self.inner.timeout, self.inner.max_bytes)
            .await;
        match net {
            Ok(resp) if resp.is_success() => {
                use base64::Engine;
                let entry = MemoEntry {
                    url: url.to_string(),
                    fetched_at: now_unix(),
                    status: resp.status,
                    headers: resp.headers.clone(),
                    body_b64: base64::engine::general_purpose::STANDARD.encode(&resp.body),
                };
                let mut s = self.lock();
                s.staged.retain(|e| e.url != url);
                s.staged.push(entry);
                s.from_network.push(url.to_string());
                Ok(resp)
            }
            other => {
                let why = match &other {
                    Ok(resp) => format!("HTTP {}", resp.status),
                    Err(e) => e.to_string(),
                };
                if mode != Mode::NetworkOnly {
                    if let Some(e) = self.read_committed(url) {
                        tracing::error!(
                            url,
                            fetched_at = e.fetched_at,
                            why,
                            "CRITICAL: served from memo"
                        );
                        self.lock().from_memo.push(url.to_string());
                        return e.to_response().ok_or_else(|| {
                            EgressError::Transport("memo entry is not decodable".into())
                        });
                    }
                }
                match other {
                    Ok(resp) => Err(EgressError::Transport(format!(
                        "{url}: HTTP {} and no memo",
                        resp.status
                    ))),
                    Err(e) => Err(e),
                }
            }
        }
    }

    /// A committed, DECODABLE entry for `url`. A corrupted file (disk, a hand edit)
    /// is treated as absent so the fresh path falls through to the network and a
    /// verified pass overwrites it; it must never pin a URL to a permanent failure.
    fn read_committed(&self, url: &str) -> Option<MemoEntry> {
        let bytes = std::fs::read(self.memo_path(url)).ok()?;
        let e: MemoEntry = serde_json::from_slice(&bytes).ok()?;
        if e.url != url || e.to_response().is_none() {
            return None;
        }
        Some(e)
    }

    fn write_entry(&self, e: &MemoEntry) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.inner.memo_dir)?;
        let mut tmp = tempfile::NamedTempFile::new_in(&self.inner.memo_dir)?;
        tmp.write_all(&serde_json::to_vec(e)?)?;
        tmp.flush()?;
        tmp.persist(self.memo_path(&e.url)).map_err(|e| e.error)?;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Session> {
        self.inner.session.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl HttpClient for MemoHttp {
    async fn get(&self, url: &str) -> anyhow::Result<HttpResponse> {
        let r = self
            .get_with_rules(url)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(HttpResponse {
            status: r.status,
            headers: r.headers,
            body: r.body,
        })
    }
}

/// dcap-qvl 0.6.3 probes the PCCS `rootcacrl` path with `.ok()` and falls back to
/// the root certificate's CRL distribution point; a PCCS that never serves it must
/// not make every pass look like "a URL failed without a memo".
pub fn is_optional_probe(url: &str) -> bool {
    url.split('?').next().unwrap_or(url).ends_with("/rootcacrl")
}

/// `memo/<sha256(url)>.json`.
pub fn memo_path(memo_dir: &Path, url: &str) -> PathBuf {
    memo_dir.join(format!(
        "{}.json",
        hex::encode(Sha256::digest(url.as_bytes()))
    ))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
