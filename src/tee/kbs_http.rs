// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 (P3.1) — the node's HTTPS client to the key broker (`kbs.fabstir.net`).
//!
//! Two calls behind [`KeyBrokerClient`]: `POST {base}/challenge` and
//! `POST {base}/request_key`. The wire format ([`ChallengeRequest`],
//! [`RequestKeyRequest`], [`EvidenceWire`], [`RequestKeyResponse`], [`ErrorBody`])
//! is frozen here and consumed unchanged by the broker (P4). Binary fields travel
//! as strings: hex for the fixed-size ones, base64 for the `nvidia_payload`
//! bytes (forwarded to NRAS byte-for-byte on the broker), UTF-8 text for the two
//! JSON documents. Never serde's integer arrays.
//!
//! **TLS (expert decision 2026-09-17): a private root, nothing else.** The
//! client is built with the built-in roots OFF and exactly one trust anchor,
//! the PEM at `TEE_KBS_CA_FILE` (baked into the image at
//! `/etc/fabstir/kbs-root.pem`, so it is measured into `compose_hash`). TLS 1.3
//! minimum; hostname verification is reqwest's default and is not touched. A
//! plaintext `http://` base URL is refused at construction.
//!
//! Every response body is bounded ([`MAX_BODY`]); a broker that streams more is
//! cut off and the release fails closed. A `test_release: true` in a release is
//! logged CRITICAL and remembered ([`Self::last_release_was_test`]): it means the
//! broker ran with `KBS_GPU_EVIDENCE=canned` and a test keyring.

use crate::tee::key_broker::KeyBrokerClient;
use crate::tee::types::{Evidence, TeeError, TeeResult, WrappedKey, TEST_ID_PREFIX};
use async_trait::async_trait;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use tokio_stream::StreamExt;

/// Where the pinned root lives in the Phala image.
pub const DEFAULT_CA_FILE: &str = "/etc/fabstir/kbs-root.pem";
/// Env: base URL of the broker API, e.g. `https://kbs.fabstir.net/v1/kbs`.
pub const URL_ENV: &str = "TEE_KBS_URL";
/// Env: path of the root PEM (default [`DEFAULT_CA_FILE`]).
pub const CA_FILE_ENV: &str = "TEE_KBS_CA_FILE";
/// Largest response body accepted from the broker. A wrapped key is ~200 bytes
/// and an error a few hundred; 64 KiB leaves room for nothing but growth.
pub const MAX_BODY: usize = 64 * 1024;
/// Per-attempt budget for `GET /info` (P4.5): its own, not the 30 s challenge
/// budget, so three attempts against a dead broker cost at most 3 × 10 s plus
/// the two pauses between them, 50 s at the default interval.
pub const INFO_TIMEOUT: Duration = Duration::from_secs(10);
/// Attempts `preflight` makes against transport-class `/info` failures.
pub const INFO_ATTEMPTS: u32 = 3;
/// Default pause between those attempts (`with_preflight_retry_interval`).
pub const DEFAULT_PREFLIGHT_RETRY_INTERVAL: Duration = Duration::from_secs(10);

// ---------- wire format (frozen; the broker uses these same types) ----------

/// `POST {base}/challenge`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengeRequest {
    /// hex, 32 bytes
    pub model_id: String,
    /// hex, 33-byte compressed secp256k1 key the nonce is bound to
    pub pk_att: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengeResponse {
    /// hex, 32 bytes
    pub nonce: String,
    /// seconds the nonce stays redeemable
    pub ttl_seconds: u32,
}

/// [`Evidence`] on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceWire {
    /// base64 (standard, padded) of the exact `nvidia_payload` bytes
    pub gpu_report_b64: String,
    /// hex of the TDX quote bytes
    pub cpu_quote_hex: String,
    /// dstack event log, UTF-8 JSON text
    pub event_log: String,
    /// dstack VM config, UTF-8 JSON text
    pub vm_config: String,
    /// hex, 48 bytes (mock-era field; the broker ignores it, see Policy v2)
    pub image_measurement_hex: String,
    /// hex, 33 bytes
    pub pk_att_hex: String,
    /// hex, 32 bytes
    pub nonce_hex: String,
}

/// `POST {base}/request_key`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestKeyRequest {
    /// hex, 32 bytes
    pub model_id: String,
    pub evidence: EvidenceWire,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedKeyWire {
    /// hex, 33 bytes
    pub eph_pub_hex: String,
    /// hex, 24 bytes
    pub nonce_hex: String,
    /// hex
    pub ciphertext_hex: String,
}

/// `GET {base}/info` → 200 (P4.5). Lenient on purpose: a newer broker may add
/// fields, and only `keyring` carries a decision; the rest is logged.
#[derive(Debug, Clone, Deserialize)]
pub struct BrokerInfo {
    pub keyring: String,
    #[serde(default)]
    pub gpu_evidence: String,
    #[serde(default)]
    pub cpu_evidence: String,
    #[serde(default)]
    pub nonce_ttl_seconds: u32,
    #[serde(default)]
    pub nras_claims_version: String,
    #[serde(default)]
    pub version: String,
}

/// How a `GET` failed (P4.5): `Transport` (a send error or any 5xx, whatever the
/// body) is retried by `preflight`; `Final` (4xx, 3xx, an over-bound or unparseable
/// 2xx) is not.
#[derive(Debug)]
pub enum GetError {
    Transport(String),
    Final(TeeError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestKeyResponse {
    pub wrapped_key: WrappedKeyWire,
    /// `true` only from a broker running the test keyring under
    /// `KBS_GPU_EVIDENCE=canned` (gate A-23 labelling). REQUIRED on the wire: a
    /// release that does not say which keyring it came from is refused (a
    /// missing field is a body error, never "real").
    pub test_release: bool,
}

/// Any non-2xx from the broker carries this. `kind` is the contract; `detail`
/// is for the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: ErrorInner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorInner {
    /// `freshness` | `verification` | `no_provider` | `invalid` | `unavailable`
    pub kind: String,
    #[serde(default)]
    pub detail: String,
}

impl EvidenceWire {
    pub fn encode(ev: &Evidence) -> TeeResult<Self> {
        let text = |name: &str, b: &[u8]| -> TeeResult<String> {
            String::from_utf8(b.to_vec())
                .map_err(|e| TeeError::Kbs(format!("evidence {name} is not UTF-8: {e}")))
        };
        Ok(Self {
            gpu_report_b64: base64::engine::general_purpose::STANDARD.encode(&ev.gpu_report),
            cpu_quote_hex: hex::encode(&ev.cpu_quote),
            event_log: text("event_log", &ev.event_log)?,
            vm_config: text("vm_config", &ev.vm_config)?,
            image_measurement_hex: hex::encode(ev.image_measurement),
            pk_att_hex: hex::encode(&ev.pk_att),
            nonce_hex: hex::encode(ev.nonce),
        })
    }

    /// The broker's side of the same contract (P4 uses it; kept here so the
    /// encoding and decoding can never drift apart).
    pub fn decode(&self) -> TeeResult<Evidence> {
        // One rule for the whole wire: bare lowercase-or-uppercase hex, no `0x`
        // (the nonce and the wrapped key are decoded the same way).
        let hex_n = |name: &str, s: &str, n: usize| -> TeeResult<Vec<u8>> {
            let v = hex::decode(s)
                .map_err(|e| TeeError::Kbs(format!("evidence {name} is not hex: {e}")))?;
            if n != 0 && v.len() != n {
                return Err(TeeError::Kbs(format!(
                    "evidence {name}: expected {n} bytes, got {}",
                    v.len()
                )));
            }
            Ok(v)
        };
        let gpu_report = base64::engine::general_purpose::STANDARD
            .decode(&self.gpu_report_b64)
            .map_err(|e| TeeError::Kbs(format!("evidence gpu_report is not base64: {e}")))?;
        let mut image_measurement = [0u8; 48];
        image_measurement.copy_from_slice(&hex_n(
            "image_measurement",
            &self.image_measurement_hex,
            48,
        )?);
        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(&hex_n("nonce", &self.nonce_hex, 32)?);
        Ok(Evidence {
            gpu_report,
            cpu_quote: hex_n("cpu_quote", &self.cpu_quote_hex, 0)?,
            event_log: self.event_log.clone().into_bytes(),
            vm_config: self.vm_config.clone().into_bytes(),
            image_measurement,
            pk_att: hex_n("pk_att", &self.pk_att_hex, 33)?,
            nonce,
        })
    }
}

impl WrappedKeyWire {
    pub fn encode(w: &WrappedKey) -> Self {
        Self {
            eph_pub_hex: hex::encode(&w.eph_pub),
            nonce_hex: hex::encode(w.nonce),
            ciphertext_hex: hex::encode(&w.ciphertext),
        }
    }

    pub fn decode(&self) -> TeeResult<WrappedKey> {
        let dec = |name: &str, s: &str| {
            hex::decode(s).map_err(|e| TeeError::Kbs(format!("wrapped_key {name} is not hex: {e}")))
        };
        let nonce_v = dec("nonce", &self.nonce_hex)?;
        if nonce_v.len() != 24 {
            return Err(TeeError::Kbs(format!(
                "wrapped_key nonce: expected 24 bytes, got {}",
                nonce_v.len()
            )));
        }
        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&nonce_v);
        Ok(WrappedKey {
            eph_pub: dec("eph_pub", &self.eph_pub_hex)?,
            nonce,
            ciphertext: dec("ciphertext", &self.ciphertext_hex)?,
        })
    }
}

// ---------- the client ----------

/// HTTPS client to the broker, pinned to one private root.
pub struct HttpKeyBrokerClient {
    base: String,
    client: reqwest::Client,
    /// Whole-request budget for `challenge` (the client default).
    timeout: Duration,
    /// Whole-request budget for `request_key`, which on the broker side spans
    /// the DCAP collateral fetch and the NRAS round trip. A nonce is burned on
    /// arrival, so a client timeout here cannot be retried; be generous.
    request_key_timeout: Duration,
    /// Whether a `test_release: true` (test-keyring, canned-evidence) release
    /// is acceptable. Off unless `TEE_ACCEPT_TEST_RELEASE` says so: only the
    /// CPU gate compose sets it, so a GPU node meeting a broker left in
    /// `KBS_GPU_EVIDENCE=canned` refuses the key instead of serving under it.
    accept_test_release: bool,
    /// Pause between `preflight`'s attempts on a transport-class `/info` failure.
    preflight_retry_interval: Duration,
    /// TTL the broker reported with the most recent nonce (300 until then).
    ttl_seconds: AtomicU32,
    last_release_was_test: AtomicBool,
}

/// Env: `1`/`true` accepts a test-keyring release (CPU gate rounds only).
pub const ACCEPT_TEST_RELEASE_ENV: &str = "TEE_ACCEPT_TEST_RELEASE";
/// Default `request_key` budget in `from_env`.
pub const DEFAULT_REQUEST_KEY_TIMEOUT: Duration = Duration::from_secs(180);

impl std::fmt::Debug for HttpKeyBrokerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpKeyBrokerClient")
            .field("base", &self.base)
            .finish()
    }
}

/// The private root's path: `TEE_KBS_CA_FILE`, else [`DEFAULT_CA_FILE`]. One
/// rule for every client that trusts it (the broker client and the policy/blob
/// sources), so they never disagree about which file is the root.
pub fn ca_file_path() -> PathBuf {
    std::env::var(CA_FILE_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| PathBuf::from(s.trim()))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CA_FILE))
}

/// The private root PEM from [`ca_file_path`]; a missing or unreadable file is
/// an error (the attested path has no meaning without it).
pub fn read_ca_pem() -> TeeResult<Vec<u8>> {
    let ca_path = ca_file_path();
    std::fs::read(&ca_path)
        .map_err(|e| TeeError::Kbs(format!("read {} ({CA_FILE_ENV}): {e}", ca_path.display())))
}

impl HttpKeyBrokerClient {
    /// From the container environment: `TEE_KBS_URL`, `TEE_KBS_CA_FILE` and
    /// `TEE_ACCEPT_TEST_RELEASE`.
    pub fn from_env() -> TeeResult<Self> {
        let base = std::env::var(URL_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| TeeError::Kbs(format!("{URL_ENV} is not set")))?;
        let pem = read_ca_pem()?;
        let accept = std::env::var(ACCEPT_TEST_RELEASE_ENV)
            .map(|v| v.trim() == "1" || v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Ok(Self::new(base.trim(), &pem, None, Duration::from_secs(30))?
            .with_request_key_timeout(DEFAULT_REQUEST_KEY_TIMEOUT)
            .with_accept_test_release(accept))
    }

    /// Budget for `request_key` (default: the `new` timeout).
    pub fn with_request_key_timeout(mut self, d: Duration) -> Self {
        self.request_key_timeout = d;
        self
    }

    /// Accept (`true`) or refuse (`false`, the default) a test-keyring release.
    pub fn with_accept_test_release(mut self, accept: bool) -> Self {
        self.accept_test_release = accept;
        self
    }

    /// Pause between `preflight` attempts (default 10 s; tests shorten it).
    pub fn with_preflight_retry_interval(mut self, d: Duration) -> Self {
        self.preflight_retry_interval = d;
        self
    }

    /// `base` must be `https://…` (the API prefix, e.g. `https://kbs.fabstir.net/v1/kbs`);
    /// `root_ca_pem` is the ONLY certificate the client will trust; `resolve` pins
    /// a hostname to an address (tests; the hostname is still verified against the
    /// certificate).
    pub fn new(
        base: &str,
        root_ca_pem: &[u8],
        resolve: Option<(String, SocketAddr)>,
        timeout: Duration,
    ) -> TeeResult<Self> {
        let base = base.trim().trim_end_matches('/').to_string();
        if !base.starts_with("https://") {
            return Err(TeeError::Kbs(format!(
                "{URL_ENV} must be https:// (the broker is reached only over TLS pinned to the private root); got {base}"
            )));
        }
        // Exactly one certificate: `from_pem` would add EVERY certificate in a
        // chain file as a trust anchor, quietly widening the pin.
        let mut roots = reqwest::Certificate::from_pem_bundle(root_ca_pem)
            .map_err(|e| TeeError::Kbs(format!("root CA PEM: {e}")))?;
        if roots.len() != 1 {
            return Err(TeeError::Kbs(format!(
                "root CA PEM must hold exactly one certificate (the private root), found {}",
                roots.len()
            )));
        }
        let root = roots.remove(0);
        // The crate enables both native-tls (reqwest's default feature) and rustls;
        // the builder defaults to native-tls, which cannot express a TLS 1.3 floor
        // and would ignore the pinning semantics below. Select rustls explicitly.
        let mut b = reqwest::Client::builder()
            .use_rustls_tls()
            // No redirects: a 30x would turn the POST into a GET elsewhere, and the
            // broker never redirects.
            .redirect(reqwest::redirect::Policy::none())
            .tls_built_in_root_certs(false)
            .add_root_certificate(root)
            .min_tls_version(reqwest::tls::Version::TLS_1_3)
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(10))
            .no_proxy()
            .user_agent(format!(
                "fabstir-llm-node/{}",
                crate::version::VERSION_NUMBER
            ));
        if let Some((host, addr)) = resolve {
            b = b.resolve(&host, addr);
        }
        let client = b
            .build()
            .map_err(|e| TeeError::Kbs(format!("build https client: {e}")))?;
        Ok(Self {
            base,
            client,
            timeout,
            request_key_timeout: timeout,
            accept_test_release: false,
            preflight_retry_interval: DEFAULT_PREFLIGHT_RETRY_INTERVAL,
            ttl_seconds: AtomicU32::new(300),
            last_release_was_test: AtomicBool::new(false),
        })
    }

    /// Whether the most recent successful release was labelled `test_release`.
    pub fn last_release_was_test(&self) -> bool {
        self.last_release_was_test.load(Ordering::SeqCst)
    }

    /// `GET {base}/{path}` (P4.5): the same bound as `post`, but the failure is
    /// classified for `preflight`'s retry and NEVER mapped through `map_error` (a
    /// contract-shaped 5xx body must read as its status, not as a freshness or
    /// verification verdict).
    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        timeout: Duration,
    ) -> Result<T, GetError> {
        let url = format!("{}/{}", self.base, path);
        let resp = self
            .client
            .get(&url)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| GetError::Transport(format!("GET {path}: {e}")))?;
        let status = resp.status();
        // A body that stops mid-stream (the broker or nginx restarting under a
        // 200) is the transient the retry exists for, whatever the status; the
        // size bound is a final answer on a non-5xx (a 5xx retries whatever its
        // body, per `GetError`).
        let bytes = match read_bounded(resp, MAX_BODY).await {
            Ok(b) => b,
            Err(ReadError::Transport(e)) | Err(ReadError::OverBound(e))
                if status.is_server_error() =>
            {
                return Err(GetError::Transport(format!(
                    "GET {path}: HTTP {status}: {e}"
                )))
            }
            Err(ReadError::Transport(e)) => {
                return Err(GetError::Transport(format!(
                    "GET {path}: HTTP {status}: {e}"
                )))
            }
            Err(ReadError::OverBound(e)) => {
                return Err(GetError::Final(TeeError::Kbs(format!(
                    "GET {path}: HTTP {status}: {e}"
                ))))
            }
        };
        if status.is_success() {
            return serde_json::from_slice(&bytes).map_err(|e| {
                GetError::Final(TeeError::Kbs(format!("GET {path}: bad response body: {e}")))
            });
        }
        let text = String::from_utf8_lossy(&bytes)
            .chars()
            .take(200)
            .collect::<String>();
        let msg = format!("GET {path}: HTTP {status}: {text}");
        if status.is_server_error() {
            Err(GetError::Transport(msg))
        } else {
            Err(GetError::Final(TeeError::Kbs(msg)))
        }
    }

    /// `GET /info` with the P4.5 retry: transport-class failures up to
    /// [`INFO_ATTEMPTS`] times, [`Self::with_preflight_retry_interval`] apart; a
    /// `Final` failure and a parsed answer are decided on the first attempt.
    async fn fetch_info(&self) -> TeeResult<BrokerInfo> {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match self.get::<BrokerInfo>("info", INFO_TIMEOUT).await {
                Ok(info) => return Ok(info),
                Err(GetError::Final(e)) => {
                    return Err(match e {
                        TeeError::Kbs(m) => TeeError::Kbs(format!("broker /info: {m}")),
                        other => other,
                    })
                }
                Err(GetError::Transport(m)) => {
                    if attempt >= INFO_ATTEMPTS {
                        return Err(TeeError::Kbs(format!(
                            "broker /info: {m} (after {attempt} attempts: the broker or its proxy is down, \
                             DNS failed, or the pinned root does not match the served certificate)"
                        )));
                    }
                    tracing::warn!(target: "tee", attempt, "broker /info: {m}; retrying");
                    tokio::time::sleep(self.preflight_retry_interval).await;
                }
            }
        }
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl Serialize,
        timeout: Duration,
    ) -> TeeResult<T> {
        let url = format!("{}/{}", self.base, path);
        let resp = self
            .client
            .post(&url)
            .timeout(timeout)
            .json(body)
            .send()
            .await
            .map_err(|e| TeeError::Kbs(format!("POST {path}: {e}")))?;
        let status = resp.status();
        // Keep the status in a body-bound error: a proxy's large 502 page must
        // read as a 502 in the log, not as "body exceeds the bound".
        let bytes = read_bounded(resp, MAX_BODY)
            .await
            .map_err(|e| TeeError::Kbs(format!("POST {path}: HTTP {status}: {}", e.message())))?;
        if status.is_success() {
            return serde_json::from_slice(&bytes)
                .map_err(|e| TeeError::Kbs(format!("POST {path}: bad response body: {e}")));
        }
        // Non-2xx: a contract error body, mapped to the TeeError the callers
        // already handle; anything else is a transport-class failure.
        match serde_json::from_slice::<ErrorBody>(&bytes) {
            Ok(ErrorBody { error }) => Err(map_error(&error, path)),
            Err(_) => Err(TeeError::Kbs(format!(
                "POST {path}: HTTP {status}: {}",
                String::from_utf8_lossy(&bytes)
                    .chars()
                    .take(200)
                    .collect::<String>()
            ))),
        }
    }
}

fn map_error(e: &ErrorInner, path: &str) -> TeeError {
    match e.kind.as_str() {
        "freshness" => TeeError::FreshnessFailure,
        "verification" => TeeError::VerificationFailed(format!("broker: {}", e.detail)),
        "no_provider" => TeeError::VerificationFailed(format!(
            "broker: no key/policy for this model: {}",
            e.detail
        )),
        other => TeeError::Kbs(format!("POST {path}: broker error {other}: {}", e.detail)),
    }
}

/// Read at most `max` bytes of the body; more is a failure, never a truncation.
/// Why a bounded body read failed: the size bound (a final answer) or the
/// stream (a transient, for `get`'s retry classification).
enum ReadError {
    OverBound(String),
    Transport(String),
}

impl ReadError {
    fn message(&self) -> &str {
        match self {
            ReadError::OverBound(m) | ReadError::Transport(m) => m,
        }
    }
}

async fn read_bounded(resp: reqwest::Response, max: usize) -> Result<Vec<u8>, ReadError> {
    if let Some(len) = resp.content_length() {
        if len as usize > max {
            return Err(ReadError::OverBound(format!(
                "response body {len} bytes exceeds the {max}-byte bound"
            )));
        }
    }
    let mut out = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| ReadError::Transport(format!("response body: {e}")))?;
        if out.len() + chunk.len() > max {
            return Err(ReadError::OverBound(format!(
                "response body exceeds the {max}-byte bound"
            )));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

#[async_trait]
impl KeyBrokerClient for HttpKeyBrokerClient {
    async fn challenge(&self, model_id: [u8; 32], pk_att: &[u8]) -> TeeResult<[u8; 32]> {
        let req = ChallengeRequest {
            model_id: hex::encode(model_id),
            pk_att: hex::encode(pk_att),
        };
        let resp: ChallengeResponse = self.post("challenge", &req, self.timeout).await?;
        self.ttl_seconds.store(resp.ttl_seconds, Ordering::SeqCst);
        let v = hex::decode(&resp.nonce)
            .map_err(|e| TeeError::Kbs(format!("challenge: nonce is not hex: {e}")))?;
        if v.len() != 32 {
            return Err(TeeError::Kbs(format!(
                "challenge: nonce must be 32 bytes, got {}",
                v.len()
            )));
        }
        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(&v);
        Ok(nonce)
    }

    async fn request_key(&self, model_id: [u8; 32], ev: &Evidence) -> TeeResult<WrappedKey> {
        let req = RequestKeyRequest {
            model_id: hex::encode(model_id),
            evidence: EvidenceWire::encode(ev)?,
        };
        let resp: RequestKeyResponse = self
            .post("request_key", &req, self.request_key_timeout)
            .await?;
        if resp.test_release && !self.accept_test_release {
            // The wrapped key is never decoded: a GPU node must not serve under
            // a release that verified canned evidence.
            return Err(TeeError::Kbs(format!(
                "broker released model {} under its TEST keyring (KBS_GPU_EVIDENCE=canned) and \
                 this node does not accept test releases ({ACCEPT_TEST_RELEASE_ENV} unset); \
                 refusing the key",
                hex::encode(model_id)
            )));
        }
        // P4.5 witness rule: the label must agree with the id's `t5t:` prefix in
        // BOTH directions, so the on-chain witness (`model_hash` = this id) and the
        // advert decision carry the label by construction. Before the store: a
        // refused release remembers nothing.
        let is_test_id = model_id.starts_with(TEST_ID_PREFIX);
        if resp.test_release != is_test_id {
            return Err(TeeError::Kbs(format!(
                "witness labelling: broker says test_release={} but TEE_MODEL_ID {} {} the t5t: prefix; \
                 refusing the key",
                resp.test_release,
                hex::encode(model_id),
                if is_test_id { "carries" } else { "does not carry" }
            )));
        }
        self.last_release_was_test
            .store(resp.test_release, Ordering::SeqCst);
        if resp.test_release {
            tracing::warn!(
                target: "tee",
                "CRITICAL: DEK released under KBS_GPU_EVIDENCE=canned (test keyring) for model {}",
                hex::encode(model_id)
            );
        }
        resp.wrapped_key.decode()
    }

    fn challenge_nonce_ttl_seconds(&self) -> u32 {
        self.ttl_seconds.load(Ordering::SeqCst)
    }

    /// P4.5: `GET /info` before the container fetch. Decides on `keyring` ×
    /// `accept_test_release` × the id's `t5t:` prefix only; the modes are logged
    /// (unconditionally, before the decision, so a refusal says WHICH mode the
    /// broker was left in) but never judged: the broker's own start-up coupling
    /// already makes any test evidence mode ⇒ `keyring: test`.
    async fn preflight(&self, model_id: [u8; 32]) -> TeeResult<()> {
        let info = self.fetch_info().await?;
        tracing::info!(
            target: "tee",
            "broker /info: version={} keyring={} gpu_evidence={} cpu_evidence={} nonce_ttl_seconds={} nras_claims_version={}",
            info.version, info.keyring, info.gpu_evidence, info.cpu_evidence, info.nonce_ttl_seconds, info.nras_claims_version
        );
        let is_test_id = model_id.starts_with(TEST_ID_PREFIX);
        let id = hex::encode(model_id);
        // The modes ride in the refusal itself: the INFO line above is filtered
        // out under RUST_LOG=warn, and the exit-78 log must still say which mode
        // the broker was left in.
        let modes = format!(
            "gpu_evidence={} cpu_evidence={} version={}",
            info.gpu_evidence, info.cpu_evidence, info.version
        );
        match info.keyring.as_str() {
            "test" if !self.accept_test_release => Err(TeeError::Kbs(format!(
                "broker /info: keyring is TEST ({modes}) and this node does not accept test releases \
                 ({ACCEPT_TEST_RELEASE_ENV} unset); not fetching the container"
            ))),
            "test" if !is_test_id => Err(TeeError::Kbs(format!(
                "broker /info: keyring is TEST ({modes}) but TEE_MODEL_ID {id} does not carry the t5t: \
                 prefix; a test keyring cannot hold this model"
            ))),
            "test" => {
                tracing::warn!(target: "tee", "CRITICAL: broker /info: keyring is TEST; this is a gate run");
                Ok(())
            }
            "real" if is_test_id => Err(TeeError::Kbs(format!(
                "broker /info: TEE_MODEL_ID {id} carries the t5t: prefix but the keyring is REAL ({modes}); \
                 a real keyring never holds a test id"
            ))),
            "real" => {
                if self.accept_test_release {
                    tracing::warn!(
                        target: "tee",
                        "broker /info: {ACCEPT_TEST_RELEASE_ENV} is set against a REAL-keyring broker (harmless; a \
                         CPU-gate node pointed at production?)"
                    );
                }
                Ok(())
            }
            other => Err(TeeError::Kbs(format!("broker /info: keyring {other:?} is not test|real"))),
        }
    }
}
