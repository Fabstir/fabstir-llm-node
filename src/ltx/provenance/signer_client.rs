// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The node side of `AP-SIGNER-CONTRACT.md` v1.0: reserve at accept (D10 pre-flight), sign after generation
//! with a bounded retry (a fresh nonce per attempt, at most [`RETRY_WINDOW`] and never past the job deadline).

use async_trait::async_trait;
use fabstir_provenance::contract::{
    self, ErrorBody, ErrorCode, Limits, ReserveRequest, ReserveResponse, SignRequest,
    SignSequenceRequest,
};
use std::time::{Duration, Instant};

pub const ENV_URL: &str = "PROVENANCE_SIGNER_URL";
pub const RETRY_WINDOW: Duration = Duration::from_secs(600);
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(90);
/// `reserve` and `limits` run on the accept path while the VRAM permit is held: a short bound (like K4's 10 s).
const ACCEPT_TIMEOUT: Duration = Duration::from_secs(12);

/// A typed signer refusal, or a transport failure (`code == None`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("signer: {code:?} {message}")]
pub struct SignerError {
    pub code: Option<ErrorCode>,
    pub message: String,
    pub retryable: bool,
    pub limits: Option<Limits>,
}

impl SignerError {
    pub fn transport(m: impl Into<String>) -> SignerError {
        SignerError {
            code: None,
            message: m.into(),
            retryable: true,
            limits: None,
        }
    }
}

/// What the delivery path needs from the signer (a fake in tests).
#[async_trait]
pub trait ProvenanceSigner: Send + Sync {
    async fn limits(&self) -> Result<Limits, SignerError>;
    async fn reserve(&self, req: &ReserveRequest) -> Result<ReserveResponse, SignerError>;
    async fn sign(&self, req: &SignRequest) -> Result<Vec<u8>, SignerError>;
    async fn sign_sequence(&self, req: &SignSequenceRequest) -> Result<Vec<u8>, SignerError>;
}

pub struct HttpSigner {
    base: String,
    key: k256::ecdsa::SigningKey,
    http: reqwest::Client,
}

impl HttpSigner {
    pub fn new(base: &str, host_private_key: &[u8; 32]) -> anyhow::Result<HttpSigner> {
        let url = reqwest::Url::parse(base)?;
        let loopback = url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
        anyhow::ensure!(
            url.scheme() == "https" || loopback,
            "{ENV_URL} must be https (or http to 127.0.0.1 in tests)"
        );
        let key = k256::ecdsa::SigningKey::from_bytes(host_private_key.into())?;
        let http = reqwest::Client::builder()
            .timeout(ATTEMPT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(HttpSigner {
            base: base.trim_end_matches('/').to_string(),
            key,
            http,
        })
    }

    async fn post(
        &self,
        path: &str,
        body: Vec<u8>,
        timeout: Duration,
    ) -> Result<Vec<u8>, SignerError> {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let nonce: [u8; 16] = rand::random();
        let h = contract::sign_request(&self.key, "POST", path, ts, nonce, &body);
        let resp = self
            .http
            .post(format!("{}{path}", self.base))
            .timeout(timeout)
            .header("content-type", "application/json")
            .header(contract::HDR_TIMESTAMP, h.timestamp)
            .header(contract::HDR_NONCE, h.nonce)
            .header(contract::HDR_SIGNATURE, h.signature)
            .body(body)
            .send()
            .await
            .map_err(|e| SignerError::transport(e.to_string()))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| SignerError::transport(e.to_string()))?
            .to_vec();
        if status.is_success() {
            return Ok(bytes);
        }
        Err(match serde_json::from_slice::<ErrorBody>(&bytes) {
            Ok(b) => SignerError {
                code: Some(b.error.code),
                message: b.error.message,
                retryable: b.error.retryable,
                limits: b.limits,
            },
            Err(_) => SignerError {
                code: None,
                message: format!("HTTP {status}"),
                retryable: status.is_server_error(),
                limits: None,
            },
        })
    }
}

fn json<T: serde::de::DeserializeOwned>(b: &[u8]) -> Result<T, SignerError> {
    serde_json::from_slice(b).map_err(|e| SignerError {
        code: None,
        message: format!("bad signer response: {e}"),
        retryable: false,
        limits: None,
    })
}

#[async_trait]
impl ProvenanceSigner for HttpSigner {
    async fn limits(&self) -> Result<Limits, SignerError> {
        let r = self
            .http
            .get(format!("{}/v1/limits", self.base))
            .timeout(ACCEPT_TIMEOUT)
            .send()
            .await
            .map_err(|e| SignerError::transport(e.to_string()))?;
        json(
            &r.bytes()
                .await
                .map_err(|e| SignerError::transport(e.to_string()))?,
        )
    }
    async fn reserve(&self, req: &ReserveRequest) -> Result<ReserveResponse, SignerError> {
        json(
            &self
                .post(
                    "/v1/reserve",
                    serde_json::to_vec(req).expect("serialises"),
                    ACCEPT_TIMEOUT,
                )
                .await?,
        )
    }
    async fn sign(&self, req: &SignRequest) -> Result<Vec<u8>, SignerError> {
        self.post(
            "/v1/sign",
            serde_json::to_vec(req).expect("serialises"),
            ATTEMPT_TIMEOUT,
        )
        .await
    }
    async fn sign_sequence(&self, req: &SignSequenceRequest) -> Result<Vec<u8>, SignerError> {
        self.post(
            "/v1/sign-sequence",
            serde_json::to_vec(req).expect("serialises"),
            ATTEMPT_TIMEOUT,
        )
        .await
    }
}

/// Retry `op` while it fails retryably, with backoff, until `RETRY_WINDOW` from now or `deadline`, whichever is
/// first (D10). Each attempt is a new HTTP request, hence a new nonce. A non-retryable refusal returns at once.
pub async fn with_retry<T, F, Fut>(deadline: Instant, mut op: F) -> Result<T, SignerError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, SignerError>>,
{
    let stop = deadline.min(Instant::now() + RETRY_WINDOW);
    let mut wait = Duration::from_secs(2);
    loop {
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) if !e.retryable => return Err(e),
            Err(e) => {
                if Instant::now() + wait >= stop {
                    return Err(e);
                }
                tracing::warn!("provenance signer: {e}; retrying in {wait:?}");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(60));
            }
        }
    }
}
