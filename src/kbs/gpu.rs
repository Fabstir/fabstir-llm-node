// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! The GPU half: NRAS v3 (design §10, gate A-17).
//!
//! Order: JWKS first (network-first with memo fallback, so the retry budget need
//! not reserve it) → POST `{nonce, evidence_list, arch}` (the canned label already
//! checked and stripped by the pre-filter) with one retry while ≥ 75 s remain
//! before `deadline` → response shape → both signatures against the JWKS by `kid`
//! (unknown `kid` → refetch once) → overall claims → branch on the overall result.
//!
//! jsonwebtoken 9.3.1 traps the design records: `iss` is compared only when the
//! claim is present, so it must be in `required_spec_claims`; `validate_aud`
//! defaults to true and refuses any token that CARRIES an `aud` when none is set;
//! never call `Jwk::is_supported()` (it unwraps `key_algorithm`, which NVIDIA's
//! entries lack).

use crate::kbs::config::GpuEvidenceMode;
use crate::kbs::egress::{EgressClient, EgressError};
use crate::kbs::error::KbsError;
use crate::kbs::memo::MemoHttp;
use crate::kbs::nras_claims::{map_per_gpu, table_for, GpuOutcome};
use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, JwkSet};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

/// Margin on top of the configured NRAS + JWKS timeouts for an NRAS retry to be
/// worth spending: with the defaults (60 s + 10 s) the threshold is 75 s. It covers
/// [`RETRY_PAUSE`].
pub const RETRY_MARGIN: Duration = Duration::from_secs(5);
/// A 429/5xx retried microseconds later meets the same answer; a short pause first.
pub const RETRY_PAUSE: Duration = Duration::from_secs(2);

/// The remaining wall an NRAS retry needs: one more POST, one `kid` refetch, margin.
pub fn retry_min_remaining(nras_timeout: Duration, jwks_timeout: Duration) -> Duration {
    nras_timeout + jwks_timeout + RETRY_MARGIN
}
/// Response bodies are small (two JWTs); 1 MiB is generous.
pub const MAX_RESPONSE_BYTES: usize = 1_048_576;
pub const CANNED_ERROR_MESSAGE: &str = "NONCE_NOT_MATCHING";
pub const CANNED_LABEL: &str = "canned";

/// Per-request GPU-half configuration.
#[derive(Debug, Clone)]
pub struct GpuConfig {
    pub nras_gpu_url: String,
    pub nras_jwks_url: String,
    pub issuer: String,
    pub claims_version: String,
    pub nras_timeout: Duration,
    pub jwks_timeout: Duration,
    pub mode: GpuEvidenceMode,
}

/// The `Validation` for NRAS tokens. `validate_time = false` is the test seam for
/// the captured A-17 tokens (they expired an hour after capture).
pub fn nras_validation(issuer: &str, validate_time: bool) -> Validation {
    let mut v = Validation::new(Algorithm::ES384);
    v.set_issuer(&[issuer]);
    v.set_required_spec_claims(&["exp", "iss", "nbf"]);
    v.validate_exp = validate_time;
    v.validate_nbf = validate_time;
    v.validate_aud = false;
    v.leeway = 300;
    v
}

/// Strip the `canned` label before forwarding (the pre-filter already checked it
/// against the mode).
pub fn strip_label(payload: &Value) -> Result<Vec<u8>, KbsError> {
    let mut obj = payload
        .as_object()
        .cloned()
        .ok_or_else(|| KbsError::verification("gpu payload is not a JSON object"))?;
    obj.remove(CANNED_LABEL);
    serde_json::to_vec(&Value::Object(obj))
        .map_err(|e| KbsError::fault(format!("gpu payload: {e}")))
}

/// The response shape: `[["JWT", overall], {"GPU-0": token}]`, exactly one per-GPU token.
/// A 2xx that is not this shape (a CDN challenge page, a truncated body) is an
/// OUTAGE (`unavailable`, the node retries), consistent with the JWKS and the
/// HTTP-400 rules; only a well-formed answer with the wrong token count refuses.
pub fn parse_response(body: &[u8]) -> Result<(String, String, String), KbsError> {
    let malformed = |what: &str| KbsError::unavailable_egress(format!("nras response: {what}"));
    let v: Value = serde_json::from_slice(body).map_err(|e| malformed(&e.to_string()))?;
    let arr = v
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or_else(|| malformed("not a 2-element array"))?;
    let head = arr[0]
        .as_array()
        .filter(|h| h.len() == 2 && h[0].as_str() == Some("JWT"))
        .ok_or_else(|| malformed("missing [\"JWT\", overall]"))?;
    let overall = head[1]
        .as_str()
        .ok_or_else(|| malformed("overall token is not a string"))?
        .to_string();
    let gpus = arr[1]
        .as_object()
        .ok_or_else(|| malformed("per-GPU map missing"))?;
    if gpus.len() != 1 {
        return Err(KbsError::verification(format!(
            "gpu count: {} tokens, want 1",
            gpus.len()
        )));
    }
    let (name, tok) = gpus.iter().next().expect("one");
    let tok = tok
        .as_str()
        .ok_or_else(|| malformed("per-GPU token is not a string"))?
        .to_string();
    Ok((overall, name.clone(), tok))
}

/// The key for `kid`: EC P-384, built from the JWK's base64url `x`/`y`.
fn key_for(jwks: &JwkSet, kid: &str) -> Result<Option<DecodingKey>, KbsError> {
    let Some(jwk) = jwks.find(kid) else {
        return Ok(None);
    };
    match &jwk.algorithm {
        AlgorithmParameters::EllipticCurve(p) if p.curve == EllipticCurve::P384 => {
            DecodingKey::from_ec_components(&p.x, &p.y)
                .map(Some)
                .map_err(|e| KbsError::verification(format!("jwks key {kid}: {e}")))
        }
        _ => Err(KbsError::verification(format!(
            "jwks key {kid} is not EC P-384"
        ))),
    }
}

fn kid_of(token: &str) -> Result<String, KbsError> {
    let h =
        decode_header(token).map_err(|e| KbsError::verification(format!("token header: {e}")))?;
    if h.alg != Algorithm::ES384 {
        return Err(KbsError::verification(format!(
            "token alg {:?}, want ES384",
            h.alg
        )));
    }
    h.kid
        .ok_or_else(|| KbsError::verification("token has no kid"))
}

/// Verify BOTH tokens against `jwks` with `validation`; `Err(None)` means an
/// unknown `kid` (the caller refetches once). Claims are read only from the
/// verified output.
pub fn verify_tokens(
    overall: &str,
    per_gpu: &str,
    jwks: &JwkSet,
    validation: &Validation,
) -> Result<(Map<String, Value>, Map<String, Value>), Option<KbsError>> {
    let mut out = Vec::with_capacity(2);
    for tok in [overall, per_gpu] {
        let kid = kid_of(tok).map_err(Some)?;
        let key = key_for(jwks, &kid).map_err(Some)?.ok_or(None)?;
        let data = decode::<Value>(tok, &key, validation)
            .map_err(|e| Some(KbsError::verification(format!("token (kid {kid}): {e}"))))?;
        let claims = data
            .claims
            .as_object()
            .cloned()
            .ok_or_else(|| Some(KbsError::verification("token claims are not an object")))?;
        out.push(claims);
    }
    let per = out.pop().expect("two");
    let ov = out.pop().expect("two");
    Ok((ov, per))
}

/// The overall token's rows (design §10): claims version, `eat_nonce`, `submods`
/// digest over the per-GPU token STRING, boolean result. Every row evaluated.
pub fn check_overall(
    overall: &Map<String, Value>,
    gpu_name: &str,
    per_gpu_token: &str,
    issued_nonce: [u8; 32],
    claims_version: &str,
) -> Result<bool, Vec<String>> {
    let mut rows = Vec::new();
    match overall.get("x-nvidia-ver").and_then(Value::as_str) {
        Some(v) if v == claims_version => {}
        Some(v) => rows.push(format!("nras claims version: {v}, want {claims_version}")),
        None => rows.push("nras claims version: x-nvidia-ver missing".into()),
    }
    let want_nonce = hex::encode(issued_nonce);
    match overall.get("eat_nonce").and_then(Value::as_str) {
        Some(s) if s.eq_ignore_ascii_case(&want_nonce) => {}
        Some(s) => rows.push(format!("overall eat_nonce: got {s}, want {want_nonce}")),
        None => rows.push("overall eat_nonce missing".into()),
    }
    let want_digest = hex::encode(Sha256::digest(per_gpu_token.as_bytes()));
    let sub = overall
        .get("submods")
        .and_then(Value::as_object)
        .and_then(|m| m.get(gpu_name))
        .and_then(Value::as_array);
    let digest_ok = sub
        .filter(|a| a.len() == 2 && a[0].as_str() == Some("DIGEST"))
        .and_then(|a| a[1].as_array())
        .filter(|d| d.len() == 2 && d[0].as_str() == Some("SHA-256"))
        .and_then(|d| d[1].as_str())
        .map(|h| h.eq_ignore_ascii_case(&want_digest))
        .unwrap_or(false);
    if !digest_ok {
        rows.push(format!(
            "submods[{gpu_name}] digest does not cover the per-GPU token"
        ));
    }
    let result = match overall.get("x-nvidia-overall-att-result") {
        Some(Value::Bool(b)) => Some(*b),
        Some(other) => {
            rows.push(format!(
                "x-nvidia-overall-att-result is {other}, not a boolean"
            ));
            None
        }
        None => {
            rows.push("x-nvidia-overall-att-result missing".into());
            None
        }
    };
    if !rows.is_empty() {
        return Err(rows);
    }
    Ok(result.expect("checked"))
}

/// Branch on the overall result (design §10). `label_present` = the request carried
/// `"canned": true` (already required by the pre-filter in canned mode).
pub fn decide(
    overall_true: bool,
    per_gpu: &Map<String, Value>,
    issued_nonce: [u8; 32],
    claims_version: &str,
    mode: GpuEvidenceMode,
    label_present: bool,
) -> Result<GpuOutcome, KbsError> {
    let names = table_for(claims_version).ok_or_else(|| {
        KbsError::verification(format!(
            "no claim table for claims version {claims_version}"
        ))
    })?;
    if overall_true {
        return map_per_gpu(per_gpu, names, issued_nonce)
            .map(GpuOutcome::Real)
            .map_err(|rows| KbsError::verification_rows(&rows));
    }
    let canned_message = per_gpu
        .get(names.error_details)
        .and_then(Value::as_object)
        .and_then(|d| d.get("message"))
        .and_then(Value::as_str)
        == Some(CANNED_ERROR_MESSAGE);
    if mode == GpuEvidenceMode::Canned && label_present && canned_message {
        tracing::error!("CRITICAL: canned GPU evidence accepted under KBS_GPU_EVIDENCE=canned");
        return Ok(GpuOutcome::CannedTolerated);
    }
    Err(KbsError::verification(format!(
        "gpu attestation: overall false ({})",
        per_gpu
            .get(names.error_details)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "no error details".into())
    )))
}

/// Parse a JWKS entry by entry: jsonwebtoken's `AlgorithmParameters` is untagged and
/// all-or-nothing, so one entry of a kind it does not know (a new `kty`, `crv` or
/// `alg` NVIDIA adds) would otherwise make the WHOLE set unparseable and, after the
/// next `kid` rotation, park every node on `unknown kid`. Entries that do not parse
/// are skipped (logged); the set must still have at least one key.
pub fn parse_jwks(body: &[u8]) -> Result<JwkSet, String> {
    let v: Value = serde_json::from_slice(body).map_err(|e| e.to_string())?;
    let entries = v
        .get("keys")
        .and_then(Value::as_array)
        .ok_or_else(|| "no keys array".to_string())?;
    let mut keys = Vec::with_capacity(entries.len());
    let mut skipped = 0usize;
    for e in entries {
        match serde_json::from_value::<jsonwebtoken::jwk::Jwk>(e.clone()) {
            Ok(k) => keys.push(k),
            Err(err) => {
                skipped += 1;
                tracing::warn!(kid = ?e.get("kid"), error = %err, "jwks entry skipped");
            }
        }
    }
    if keys.is_empty() {
        return Err(format!("no usable keys ({skipped} skipped)"));
    }
    Ok(JwkSet { keys })
}

async fn fetch_jwks(jwks_memo: &MemoHttp, url: &str) -> Result<(JwkSet, Vec<u8>), KbsError> {
    let resp = jwks_memo
        .get_with_rules(url)
        .await
        .map_err(|e| KbsError::unavailable_egress(format!("jwks: {e}")))?;
    match parse_jwks(&resp.body) {
        Ok(set) => Ok((set, resp.body)),
        Err(e) => {
            // A 2xx that does not parse is treated like a non-2xx: the memo fallback.
            // Its staged body is dropped first, or the commit after token verification
            // would write the garbage over the good entry.
            jwks_memo.discard(url);
            let Some(m) = jwks_memo.committed(url) else {
                return Err(KbsError::unavailable_egress(format!(
                    "jwks: {e} and no memo"
                )));
            };
            tracing::error!(url, "CRITICAL: jwks body did not parse; served from memo");
            let set = parse_jwks(&m.body)
                .map_err(|e| KbsError::unavailable_egress(format!("jwks memo: {e}")))?;
            Ok((set, m.body))
        }
    }
}

async fn post_with_retry(
    egress: &EgressClient,
    url: &str,
    body: Vec<u8>,
    timeout: Duration,
    min_remaining: Duration,
    deadline: Instant,
    sink: &RawSink,
) -> Result<Vec<u8>, KbsError> {
    let mut attempt = 0u8;
    loop {
        attempt += 1;
        let why = match egress
            .post_json(url, body.clone(), timeout, MAX_RESPONSE_BYTES)
            .await
        {
            Ok(resp) if resp.is_success() => {
                sink.set(Some(resp.body.clone()));
                return Ok(resp.body);
            }
            Ok(resp) if resp.status >= 500 || resp.status == 429 || resp.status == 408 => {
                sink.set(Some(resp.body.clone()));
                format!("HTTP {}", resp.status)
            }
            Ok(resp) => {
                // 400 is NRAS refusing the evidence; 401/403/404 and the rest are the
                // endpoint, our configuration or NVIDIA's side, not the GPU. The body is
                // captured either way: on the paid day it names the problem.
                sink.set(Some(resp.body.clone()));
                let text = String::from_utf8_lossy(&resp.body)
                    .chars()
                    .take(200)
                    .collect::<String>();
                return Err(if resp.status == 400 {
                    KbsError::verification(format!("nras: HTTP 400 {text}"))
                } else {
                    KbsError::unavailable_egress(format!("nras: HTTP {} {text}", resp.status))
                });
            }
            Err(EgressError::Transport(m)) => {
                // Nothing was received on this attempt: an earlier attempt's 5xx body
                // must not pose as "what NRAS last said" in the capture.
                sink.set(None);
                m
            }
            Err(e) => return Err(KbsError::unavailable_egress(format!("nras: {e}"))),
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if attempt == 1 && remaining >= min_remaining {
            tracing::warn!(
                why,
                remaining_secs = remaining.as_secs(),
                "nras failed; retrying once"
            );
            tokio::time::sleep(RETRY_PAUSE).await;
            continue;
        }
        return Err(KbsError::unavailable_egress(format!("nras: {why}")));
    }
}

/// The whole GPU half. `payload` is the label-checked collector JSON; `jwks_memo`
/// is this request's JWKS instance (committed here after both tokens verified).
/// The raw NRAS body comes back on EVERY path once a response was received: a
/// refusal on the claim table is exactly what the day-one capture must record.
pub async fn attest(
    egress: &EgressClient,
    jwks_memo: &MemoHttp,
    cfg: &GpuConfig,
    payload: &Value,
    issued_nonce: [u8; 32],
    deadline: Instant,
) -> (Result<GpuOutcome, KbsError>, Option<Vec<u8>>) {
    let sink = RawSink::default();
    let r = attest_with_sink(
        egress,
        jwks_memo,
        cfg,
        payload,
        issued_nonce,
        deadline,
        &sink,
    )
    .await;
    (r, sink.take())
}

/// Where the raw NRAS body lands the moment it is received. Owned by the CALLER, so
/// a request wall that drops the in-flight future (a retry under way) still leaves
/// the last received body for the capture.
#[derive(Debug, Default, Clone)]
pub struct RawSink(std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>);

impl RawSink {
    pub fn set(&self, body: Option<Vec<u8>>) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = body;
    }
    pub fn take(&self) -> Option<Vec<u8>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
}

/// [`attest`] with a caller-owned sink for the raw NRAS body.
pub async fn attest_with_sink(
    egress: &EgressClient,
    jwks_memo: &MemoHttp,
    cfg: &GpuConfig,
    payload: &Value,
    issued_nonce: [u8; 32],
    deadline: Instant,
    sink: &RawSink,
) -> Result<GpuOutcome, KbsError> {
    let label_present = payload.get(CANNED_LABEL).and_then(Value::as_bool) == Some(true);
    let body = strip_label(payload)?;
    let (mut jwks, _) = fetch_jwks(jwks_memo, &cfg.nras_jwks_url).await?;
    let raw = post_with_retry(
        egress,
        &cfg.nras_gpu_url,
        body,
        cfg.nras_timeout,
        retry_min_remaining(cfg.nras_timeout, cfg.jwks_timeout),
        deadline,
        sink,
    )
    .await?;
    let (overall_tok, gpu_name, per_tok) = parse_response(&raw)?;
    let validation = nras_validation(&cfg.issuer, true);
    let (overall, per_gpu) = match verify_tokens(&overall_tok, &per_tok, &jwks, &validation) {
        Ok(v) => v,
        Err(Some(e)) => return Err(e),
        Err(None) => {
            // Unknown kid: refetch once. A fresh session so the summary says whether
            // the refetch actually reached the network; a memo-served refetch (an
            // egress outage during NVIDIA's key rotation) is an outage, not a bad token.
            jwks_memo.set_mode(crate::kbs::memo::Mode::Normal);
            let (set, _) = fetch_jwks(jwks_memo, &cfg.nras_jwks_url).await?;
            jwks = set;
            match verify_tokens(&overall_tok, &per_tok, &jwks, &validation) {
                Ok(v) => v,
                Err(Some(e)) => return Err(e),
                Err(None) => {
                    // NVIDIA rotates the signing key about every two days; a key that
                    // signs before the JWKS endpoint reflects it is a lag, not a bad
                    // token. `unavailable` lets the node retry instead of parking; a
                    // forged token with a made-up kid is refused either way.
                    if !jwks_memo.summary().from_network {
                        return Err(KbsError::unavailable_egress(
                            "jwks: unknown kid and the refetch could not reach the network",
                        ));
                    }
                    return Err(KbsError::unavailable_egress(
                        "jwks: unknown kid after a fresh refetch",
                    ));
                }
            }
        }
    };
    // Both tokens verified against this JWKS: commit its staged body.
    jwks_memo.commit();
    let overall_true = check_overall(
        &overall,
        &gpu_name,
        &per_tok,
        issued_nonce,
        &cfg.claims_version,
    )
    .map_err(|rows| KbsError::verification_rows(&rows))?;
    decide(
        overall_true,
        &per_gpu,
        issued_nonce,
        &cfg.claims_version,
        cfg.mode,
        label_present,
    )
}
