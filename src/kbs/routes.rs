// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! The HTTP surface (design §4) and the `request_key` pipeline (design §6).
//!
//! Mounted at `/v1/kbs` (nginx forwards the prefix). The wire is the node's
//! frozen `tee::kbs_http` shape, mirrored here with `deny_unknown_fields` so a
//! drift is a loud 400. Handlers take `Result<Json<T>, JsonRejection>` and map
//! every rejection to `invalid` (400; 413 for the body limit): axum's own
//! 422/415 plain-text answers never reach the node.
//!
//! Serve with `into_make_service_with_connect_info::<SocketAddr>()`: the source
//! of a request is the LAST `X-Forwarded-For` element (nginx overwrites the header
//! with `$remote_addr`) or, absent the header, the peer address.

use crate::crypto::recover_client_address;
use crate::kbs::authority::{Authority, AuthorityStore};
use crate::kbs::capture::{Capture, CaptureRecord, Ring};
use crate::kbs::config::{CpuEvidenceMode, KbsConfig};
use crate::kbs::cpu::{self, TdxEvidence};
use crate::kbs::egress::EgressClient;
use crate::kbs::error::{KbsError, Kind};
use crate::kbs::eventlog::Replayed;
use crate::kbs::gpu::{self, GpuConfig};
use crate::kbs::keyring::{Keyring, KeyringClass};
use crate::kbs::memo::MemoHttp;
use crate::kbs::nonce::NonceStore;
use crate::kbs::nras_claims::{CcAssertion, GpuOutcome};
use crate::kbs::policy_file::{load_policy, FilePolicySource};
use crate::kbs::verify::{check_policy, prefilter, CcRecord, Expected, Verified};
use crate::tee::host_profile::{
    canonical_bytes, eip191_digest, recover_document_signer, HostProfile, SignedAuthorityManifest,
    SignedHostProfileSet,
};
use crate::tee::kbs_http::{
    ChallengeResponse, ErrorBody, ErrorInner, EvidenceWire, RequestKeyRequest, RequestKeyResponse,
    WrappedKeyWire,
};
use crate::tee::keywrap::wrap_key;
use crate::tee::policy_source::ProviderRegistry;
use crate::tee::types::{version_at_least, Policy};
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::Semaphore;

// ---------- wire mirrors (deny_unknown_fields) ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeRequestWire {
    pub model_id: String,
    pub pk_att: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceWireIn {
    pub gpu_report_b64: String,
    pub cpu_quote_hex: String,
    pub event_log: String,
    pub vm_config: String,
    pub image_measurement_hex: String,
    pub pk_att_hex: String,
    pub nonce_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestKeyRequestWire {
    pub model_id: String,
    pub evidence: EvidenceWireIn,
}

impl RequestKeyRequestWire {
    /// The node's frozen struct (same field names, no `deny_unknown_fields`).
    pub fn into_node(self) -> RequestKeyRequest {
        RequestKeyRequest {
            model_id: self.model_id,
            evidence: EvidenceWire {
                gpu_report_b64: self.evidence.gpu_report_b64,
                cpu_quote_hex: self.evidence.cpu_quote_hex,
                event_log: self.evidence.event_log,
                vm_config: self.evidence.vm_config,
                image_measurement_hex: self.evidence.image_measurement_hex,
                pk_att_hex: self.evidence.pk_att_hex,
                nonce_hex: self.evidence.nonce_hex,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfoResponse {
    pub keyring: String,
    pub gpu_evidence: String,
    pub cpu_evidence: String,
    pub nonce_ttl_seconds: u32,
    pub nras_claims_version: String,
    pub version: String,
}

// ---------- state ----------

pub struct AppState {
    pub cfg: KbsConfig,
    pub keyring: Keyring,
    pub class: KeyringClass,
    pub nonces: NonceStore,
    pub egress: EgressClient,
    pub policy: FilePolicySource,
    pub registry: ProviderRegistry,
    pub capture: Capture,
    /// Policy schema 3: the authority bundles, their HWMs and archive (D6, D10).
    pub authority: AuthorityStore,
    /// The clock, read ONCE per request (D7). Production: `tee::types::now_unix`,
    /// which returns `u64::MAX` on a broken clock (refused as a fault). Tests
    /// inject a fixed or broken clock.
    pub now_fn: Arc<dyn Fn() -> u64 + Send + Sync>,
    permits: Arc<Semaphore>,
    per_source: Mutex<HashMap<String, usize>>,
}

pub type Shared = Arc<AppState>;

impl AppState {
    /// `Keyring::load` already refuses a mixed keyring; this refuses it again so a
    /// caller that built the keyring some other way cannot panic the broker.
    pub fn new(cfg: KbsConfig, keyring: Keyring, egress: EgressClient) -> Result<Self, KbsError> {
        let class = keyring
            .class()
            .ok_or_else(|| KbsError::fault("keyring is neither all-test nor all-real"))?;
        let registry = keyring.provider_registry();
        Ok(Self {
            nonces: NonceStore::new(cfg.nonce_ttl, cfg.nonce_cap, cfg.nonce_per_source_cap),
            policy: FilePolicySource::new(cfg.policy_dir.clone()),
            capture: Capture::new(
                cfg.capture_dir(),
                cfg.capture_max,
                cfg.capture_preverify_max,
            ),
            authority: AuthorityStore::new(
                cfg.authority_dir.clone(),
                cfg.hwm_dir(),
                cfg.archive_dir(),
                false,
            ),
            now_fn: Arc::new(crate::tee::types::now_unix),
            permits: Arc::new(Semaphore::new(cfg.request_concurrency)),
            per_source: Mutex::new(HashMap::new()),
            cfg,
            keyring,
            class,
            egress,
            registry,
        })
    }

    /// Replace the clock (tests: a fixed `now`, or `u64::MAX` for a broken one).
    pub fn with_now_fn(mut self, f: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        self.now_fn = Arc::new(f);
        self
    }

    /// `now` for one request (D7): a broken clock is a `fault`, never 0.
    pub fn request_now(&self) -> Result<u64, KbsError> {
        let now = (self.now_fn)();
        if now == u64::MAX {
            return Err(KbsError::fault("system clock unavailable"));
        }
        Ok(now)
    }

    fn gpu_config(&self) -> GpuConfig {
        GpuConfig {
            nras_gpu_url: self.cfg.nras_gpu_url.to_string(),
            nras_jwks_url: self.cfg.nras_jwks_url.to_string(),
            issuer: self.cfg.nras_issuer.clone(),
            claims_version: self.cfg.nras_claims_version.clone(),
            nras_timeout: self.cfg.nras_timeout,
            jwks_timeout: self.cfg.jwks_timeout,
            mode: self.cfg.gpu_evidence,
        }
    }
}

/// Decrements the per-source in-flight count on drop.
struct SourcePermit {
    state: Shared,
    source: String,
}

impl Drop for SourcePermit {
    fn drop(&mut self) {
        let mut m = self
            .state
            .per_source
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(n) = m.get_mut(&self.source) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                m.remove(&self.source);
            }
        }
    }
}

fn acquire_source(state: &Shared, source: &str) -> Option<SourcePermit> {
    let mut m = state.per_source.lock().unwrap_or_else(|p| p.into_inner());
    let n = m.entry(source.to_string()).or_insert(0);
    if *n >= state.cfg.inflight_per_source_cap {
        return None;
    }
    *n += 1;
    Some(SourcePermit {
        state: state.clone(),
        source: source.to_string(),
    })
}

// ---------- router ----------

pub fn router(state: Shared) -> Router {
    let limit = state.cfg.max_body_bytes;
    let api = Router::new()
        .route("/challenge", post(challenge))
        .route("/request_key", post(request_key))
        .route("/info", get(info))
        .layer(DefaultBodyLimit::max(limit))
        .with_state(state);
    Router::new().nest("/v1/kbs", api)
}

/// The request's source for the per-source caps: the LAST `X-Forwarded-For` element
/// (correct under both `$remote_addr` and `$proxy_add_x_forwarded_for`) when the
/// peer is the loopback reverse proxy on this box, else the peer address itself.
/// A non-loopback peer never gets to name its own source (it could rotate sources
/// past the caps or exhaust another node's budget).
pub fn source_of(headers: &HeaderMap, peer: SocketAddr) -> String {
    // `[::]` listeners see the proxy as the IPv4-mapped `::ffff:127.0.0.1`.
    let loopback = match peer.ip() {
        std::net::IpAddr::V4(v4) => v4.is_loopback(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    };
    if !loopback {
        return peer.ip().to_string();
    }
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.rsplit(',').next())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| peer.ip().to_string())
}

fn error_response(e: &KbsError) -> Response {
    let body = ErrorBody {
        error: ErrorInner {
            kind: e.kind.wire().to_string(),
            detail: e.detail.clone(),
        },
    };
    (
        StatusCode::from_u16(e.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(body),
    )
        .into_response()
}

fn rejection_to_error(r: JsonRejection) -> KbsError {
    match r {
        JsonRejection::BytesRejection(b) if b.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            KbsError::invalid("body over the limit").with_status(413)
        }
        other => KbsError::invalid(format!("body: {}", other.body_text())),
    }
}

/// `pk_att` must be a valid compressed secp256k1 point, checked BEFORE the nonce is
/// issued or burned: a 33-byte non-point would otherwise burn the nonce, run the
/// collateral fetch and the paid NRAS round trip, and fail at `wrap_key`.
fn pk_att_point(name: &str, bytes: &[u8]) -> Result<[u8; 33], KbsError> {
    let pk: [u8; 33] = bytes
        .try_into()
        .map_err(|_| KbsError::invalid(format!("{name} is not 33 bytes")))?;
    k256::PublicKey::from_sec1_bytes(&pk)
        .map_err(|_| KbsError::invalid(format!("{name} is not a secp256k1 point")))?;
    Ok(pk)
}

fn hex32(name: &str, s: &str) -> Result<[u8; 32], KbsError> {
    let v = hex::decode(s).map_err(|_| KbsError::invalid(format!("{name} is not hex")))?;
    v.try_into()
        .map_err(|_| KbsError::invalid(format!("{name} is not 32 bytes")))
}

// ---------- handlers ----------

async fn info(State(state): State<Shared>) -> Json<InfoResponse> {
    Json(InfoResponse {
        keyring: state.class.wire().to_string(),
        gpu_evidence: match state.cfg.gpu_evidence {
            crate::kbs::config::GpuEvidenceMode::Real => "real",
            crate::kbs::config::GpuEvidenceMode::Canned => "canned",
        }
        .to_string(),
        cpu_evidence: match state.cfg.cpu_evidence {
            CpuEvidenceMode::Real => "real",
            CpuEvidenceMode::Simulator => "simulator",
        }
        .to_string(),
        nonce_ttl_seconds: state.cfg.nonce_ttl.as_secs() as u32,
        nras_claims_version: state.cfg.nras_claims_version.clone(),
        version: crate::version::VERSION.to_string(),
    })
}

async fn challenge(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<ChallengeRequestWire>, JsonRejection>,
) -> Response {
    let source = source_of(&headers, peer);
    match challenge_inner(&state, &source, body).await {
        Ok(r) => (StatusCode::OK, Json(r)).into_response(),
        Err(e) => {
            tracing::info!(source, kind = e.kind.wire(), detail = %e.detail, "challenge refused");
            error_response(&e)
        }
    }
}

async fn challenge_inner(
    state: &Shared,
    source: &str,
    body: Result<Json<ChallengeRequestWire>, JsonRejection>,
) -> Result<ChallengeResponse, KbsError> {
    let Json(req) = body.map_err(rejection_to_error)?;
    let model_id = hex32("model_id", &req.model_id)?;
    let pk = hex::decode(&req.pk_att).map_err(|_| KbsError::invalid("pk_att is not hex"))?;
    let pk_att = pk_att_point("pk_att", &pk)?;
    let Some(entry) = state.keyring.get(&model_id) else {
        return Err(KbsError::no_provider("unknown model"));
    };
    // A policy request_key would refuse (missing, expired, wrong signer, below the
    // keyring floor) costs a burned nonce and a node's evidence cycle; the same
    // clock-and-signature check says so here, before anything is issued. The file
    // is read again at step 3: a swap in between is judged there.
    let signed = load_policy(
        &state.policy,
        &state.registry,
        model_id,
        entry.min_policy_version,
    )
    .await?;
    // Schema 3 (D7): the three documents must be valid (and the HWM not rolled
    // back) before a nonce is spent, but NO candidate is required: a brand-new
    // host shape must reach `request_key` so its refusal is captured (D18).
    let now = state.request_now()?;
    state.authority.load(&signed.policy, now)?;
    let nonce = state
        .nonces
        .issue(model_id, pk_att, source, Instant::now())
        .map_err(|e| KbsError::freshness(e.to_string()))?;
    Ok(ChallengeResponse {
        nonce: hex::encode(nonce),
        ttl_seconds: state.cfg.nonce_ttl.as_secs() as u32,
    })
}

async fn request_key(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<RequestKeyRequestWire>, JsonRejection>,
) -> Response {
    let source = source_of(&headers, peer);
    // Step 0: permits.
    let Ok(_global) = state.permits.clone().try_acquire_owned() else {
        return error_response(&KbsError::busy());
    };
    let Some(_per_source) = acquire_source(&state, &source) else {
        return error_response(&KbsError::busy());
    };
    let started = Instant::now();
    let mut rec = CaptureRecord::default();
    let outcome = run_pipeline(&state, body, &mut rec).await;
    let elapsed = started.elapsed();
    match outcome {
        Ok((resp, model_id, log)) => {
            rec.decision = format!("released\n{log}\nelapsed_ms={}", elapsed.as_millis());
            capture_blocking(&state, Ring::Verified, model_id, rec).await;
            tracing::info!(source, model = %hex::encode(model_id), elapsed_ms = elapsed.as_millis() as u64, %log, "released");
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err((e, ring, model_id)) => {
            rec.decision = format!(
                "{} {}: {}\nelapsed_ms={}",
                e.kind.wire(),
                e.status,
                e.detail,
                elapsed.as_millis()
            );
            if let Some(id) = model_id {
                capture_blocking(&state, ring, id, rec).await;
            }
            tracing::warn!(source, kind = e.kind.wire(), status = e.status, detail = %e.detail, elapsed_ms = elapsed.as_millis() as u64, "request_key refused");
            error_response(&e)
        }
    }
}

/// The capture write (files + ring eviction) is disk I/O: off the worker threads.
async fn capture_blocking(state: &Shared, ring: Ring, model_id: [u8; 32], rec: CaptureRecord) {
    let st = state.clone();
    if let Err(e) =
        tokio::task::spawn_blocking(move || st.capture.write(ring, &model_id, &rec)).await
    {
        tracing::warn!(error = %e, "capture task failed (non-fatal)");
    }
}

type PipelineErr = (KbsError, Ring, Option<[u8; 32]>);

/// Steps 1–9 of design §6. Returns the response, the model id and the one-line log.
async fn run_pipeline(
    state: &Shared,
    body: Result<Json<RequestKeyRequestWire>, JsonRejection>,
    rec: &mut CaptureRecord,
) -> Result<(RequestKeyResponse, [u8; 32], String), PipelineErr> {
    // Step 1: decode.
    let Json(req) = body.map_err(|r| (rejection_to_error(r), Ring::Preverify, None))?;
    rec.request = serde_json::to_vec(&req).unwrap_or_default();
    let model_id = hex32("model_id", &req.model_id).map_err(|e| (e, Ring::Preverify, None))?;
    let pre = |e: KbsError| (e, Ring::Preverify, Some(model_id));
    let node_req = req.into_node();
    let evidence = node_req
        .evidence
        .decode()
        .map_err(|e| pre(KbsError::invalid(format!("evidence: {e}"))))?;
    let entry = state
        .keyring
        .get(&model_id)
        .ok_or_else(|| pre(KbsError::no_provider("unknown model")))?;
    let pk_att = pk_att_point("evidence pk_att", &evidence.pk_att).map_err(pre)?;

    // Step 2: burn + bind; the deadline starts here.
    let now = Instant::now();
    let issued = state
        .nonces
        .take(&evidence.nonce, now)
        .map_err(|e| pre(KbsError::freshness(e.to_string())))?;
    issued
        .bind(&model_id, &pk_att)
        .map_err(|e| pre(KbsError::freshness(e.to_string())))?;
    let deadline = now + state.cfg.request_wall;
    let expected = Expected {
        model_id,
        pk_att,
        nonce: evidence.nonce,
    };

    // Steps 3–9 under the wall. `stage` (the step running when the wall fires)
    // names the culprit in the detail; the capture is an outage's (no verdict) and
    // goes to `preverify/` whatever the step. `nras_sink` outlives the inner future
    // so a body received before the wall fired is captured even when the future is
    // dropped mid-retry.
    let stage = std::sync::atomic::AtomicU8::new(3);
    let nras_sink = gpu::RawSink::default();
    let inner = async {
        let mut timings: Vec<String> = Vec::new();
        let t = Instant::now();

        // Step 3: policy, then the authority bundle at ONE `now` (D7).
        let now = state.request_now().map_err(|e| (e, Ring::Preverify))?;
        let signed = load_policy(
            &state.policy,
            &state.registry,
            model_id,
            entry.min_policy_version,
        )
        .await
        .map_err(|e| (e, Ring::Preverify))?;
        let auth = state
            .authority
            .load(&signed.policy, now)
            .map_err(|e| (e, Ring::Preverify))?;
        let policy_hash = signed.policy_hash().map(hex::encode).unwrap_or_default();
        timings.push(format!("policy={}ms", t.elapsed().as_millis()));

        // Step 4: pre-filter.
        stage.store(4, std::sync::atomic::Ordering::Relaxed);
        let t = Instant::now();
        let pf = prefilter(
            &signed.policy,
            &auth,
            &expected,
            &evidence.cpu_quote,
            &evidence.event_log,
            &evidence.gpu_report,
            state.cfg.gpu_evidence,
            now,
        )
        .map_err(|e| (e, Ring::Preverify))?;
        timings.push(format!("prefilter={}ms", t.elapsed().as_millis()));

        // Step 5: CPU half.
        stage.store(5, std::sync::atomic::Ordering::Relaxed);
        let t = Instant::now();
        let cpu_ev: TdxEvidence = match state.cfg.cpu_evidence {
            CpuEvidenceMode::Real => {
                let memo = MemoHttp::collateral(
                    state.egress.clone(),
                    state.cfg.memo_dir(),
                    state.cfg.collateral_memo_fresh,
                    state.cfg.pccs_timeout,
                    4 * 1_048_576,
                );
                let (ev, trace) =
                    cpu::verify_real(&evidence.cpu_quote, &memo, state.cfg.pccs_url.as_str(), now)
                        .await
                        .map_err(|e| (e, Ring::Preverify))?;
                timings.push(format!(
                    "collateral_passes={} second={:?} committed={}",
                    trace.passes, trace.second_pass_mode, trace.committed
                ));
                ev
            }
            CpuEvidenceMode::Simulator => {
                tracing::error!(
                    "CRITICAL: simulator quote accepted under KBS_CPU_EVIDENCE=simulator"
                );
                cpu::simulator(&evidence.cpu_quote).map_err(|e| (e, Ring::Preverify))?
            }
        };
        timings.push(format!("cpu={}ms", t.elapsed().as_millis()));

        // Step 6: GPU half.
        stage.store(6, std::sync::atomic::Ordering::Relaxed);
        let t = Instant::now();
        let jwks_memo = MemoHttp::network_first(
            state.egress.clone(),
            state.cfg.memo_dir(),
            state.cfg.jwks_timeout,
            4 * 1_048_576,
        );
        let gpu_result = gpu::attest_with_sink(
            &state.egress,
            &jwks_memo,
            &state.gpu_config(),
            &pf.gpu_payload,
            expected.nonce,
            deadline,
            &nras_sink,
        )
        .await;
        // An NRAS/JWKS outage carries no verdict: it goes to `preverify/`, so a node
        // restart-looping through an outage cannot cycle the verified ring and evict
        // the first real EAT. NRAS's own refusal (400, the claim rows) is a verdict.
        let gpu_outcome = gpu_result.map_err(|e| {
            let ring = if e.kind == Kind::Unavailable {
                Ring::Preverify
            } else {
                Ring::Verified
            };
            (e, ring)
        })?;
        timings.push(format!("gpu={}ms", t.elapsed().as_millis()));

        // Step 7: the authoritative checks.
        stage.store(7, std::sync::atomic::Ordering::Relaxed);
        let verified = check_policy(
            &signed.policy,
            &auth,
            &expected,
            &cpu_ev,
            &pf.replayed,
            &gpu_outcome,
            entry.test,
            now,
        );
        rec.verified = Some(verified_json(
            &cpu_ev,
            &pf.replayed,
            &gpu_outcome,
            verified.as_ref().ok(),
            &auth,
        ));
        let verified = verified.map_err(|e| (e, Ring::Verified))?;

        // Step 8: the independent second gate, over the RAW documents (D21).
        release_gate(
            &auth.manifest,
            &auth.set,
            now,
            &cpu_ev,
            &pf.replayed,
            &gpu_outcome,
            &signed.policy,
            &verified,
            entry.test,
        )
        .map_err(|e| (e, Ring::Verified))?;

        // Step 9: wrap.
        let wrapped = wrap_key(&entry.dek, &expected.pk_att)
            .map_err(|e| (KbsError::fault(format!("wrap: {e}")), Ring::Verified))?;
        let log = format!(
            "policy_version={} policy_hash={policy_hash} {} tcb={} advisories={:?} hwmodel={:?} driver={:?} vbios={:?} cc_mode={} test_release={} {}",
            signed.policy.policy_version,
            profile_fields(&verified, &auth),
            verified.tcb_status,
            verified.advisory_ids,
            verified.hwmodel,
            verified.driver_version,
            verified.vbios_version,
            verified.cc_mode.label(),
            entry.test,
            timings.join(" ")
        );
        Ok::<_, (KbsError, Ring)>((
            RequestKeyResponse {
                wrapped_key: WrappedKeyWire::encode(&wrapped),
                test_release: entry.test,
            },
            log,
        ))
    };
    let result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), inner).await;
    // Captured on every path once NRAS answered (a claim-table refusal, a wall timeout
    // mid-retry): the day-one runbook reads the real claim names from it.
    rec.nras = nras_sink.take();
    match result {
        Ok(Ok((resp, log))) => Ok((resp, model_id, log)),
        Ok(Err((e, ring))) => Err((e, ring, Some(model_id))),
        Err(_) => {
            let step = stage.load(std::sync::atomic::Ordering::Relaxed);
            Err((
                KbsError::unavailable_egress(format!(
                    "request wall exceeded during {}",
                    step_name(step)
                )),
                Ring::Preverify,
                Some(model_id),
            ))
        }
    }
}

fn step_name(step: u8) -> &'static str {
    match step {
        3 => "policy",
        4 => "prefilter",
        5 => "collateral (PCCS)",
        6 => "gpu (NRAS)",
        _ => "release",
    }
}

/// Design §6 step 8 (D4's independent second gate), for a non-test entry only.
/// Step 7 already refuses the canned and the DevTools-open cases; the gate
/// exists so that a bug in step 7 cannot release a real DEK. It is NOT dead
/// code even if step 7 is perfect: the disagreement arm is one step 7 cannot
/// raise, being the thing that produces the record being disagreed with. It is
/// its own function so it can be tested directly. It takes the RAW
/// [`GpuOutcome`] and derives its own verdict from `map_per_gpu`'s output,
/// never from what step 7 produced or copied.
///
/// Refuses: canned tolerance; GPU claims that do not rule DevTools out,
/// whatever the policy asked (D14a), naming both claims; and any disagreement
/// between those claims and step 7's record, named as such so a broker bug
/// never reads as a bad GPU. All are `verification` (403): the wire has no
/// `internal` kind (D10), and of the five that exist `unavailable` (500) is
/// the one that tells the reader to come back later, which none of these
/// refusals will reward. What BOUNDS the waste of a repeated refusal is the
/// compose's `restart: on-failure:5`, not the kind: the node exits 78 on any
/// refused attested load, so 403 and 500 cost the same six nonces and six
/// rounds of paid NRAS egress (`on-failure:5` = the first start plus five
/// restarts; PCCS is memo-served after the first).
#[allow(clippy::too_many_arguments)]
pub fn release_gate(
    manifest: &SignedAuthorityManifest,
    set: &SignedHostProfileSet,
    now: u64,
    cpu: &TdxEvidence,
    replayed: &Replayed,
    gpu: &GpuOutcome,
    policy: &Policy,
    verified: &Verified,
    entry_test: bool,
) -> Result<(), KbsError> {
    // A test entry may release under canned evidence or unprotected claims;
    // that is what `test: true` means and step 7 has already said so.
    if entry_test {
        return Ok(());
    }
    // Which refusal is which matters on the paid day: the capture in
    // `verified/` is what an operator reads, and "chase the GPU" must never
    // look like "chase the broker", nor the reverse. Every arm below is a
    // `verification`, never an `unavailable`: none of them will pass on a
    // later attempt (design §6 step 8). The detail line says which it is.
    //
    // Everything below derives from `map_per_gpu`'s OWN output rather than
    // from anything step 7 produced or copied: the gate exists so that a bug
    // in step 7's derivation or in its transcription cannot release a real
    // DEK, which it could not do if it shared either.
    match gpu {
        GpuOutcome::CannedTolerated => {
            if verified.cc_mode == CcRecord::Canned {
                return Err(KbsError::verification(
                    "release gate: canned tolerance for a non-test keyring entry",
                ));
            }
            return Err(KbsError::verification(format!(
                "release gate: the evidence was canned but the policy step recorded {}; \
                 refusing rather than releasing on a disagreement inside the broker",
                verified.cc_mode.label(),
            )));
        }
        GpuOutcome::Real(f) => {
            // Order matters for the capture an operator reads on the paid day.
            // A bad GPU is named FIRST, because no key can be released to it
            // whatever the broker did; a broker that also mis-recorded the
            // verdict is named in the same message rather than hiding the GPU.
            let derived = f.cc_assertion();
            let expected = match derived {
                CcAssertion::SignedNotDevTools => CcRecord::SignedNotDevTools,
                CcAssertion::SignedDevToolsOrNoSecureBoot { .. } => {
                    CcRecord::SignedDevToolsOrNoSecureBoot
                }
            };
            // D14a: a real DEK never goes to a GPU whose signed claims leave
            // DevTools open, whatever the policy asked for, so
            // `require_cc_mode: null` is not don't-care for a real entry.
            if !matches!(derived, CcAssertion::SignedNotDevTools) {
                return Err(KbsError::verification(format!(
                    "release gate: the signed GPU claims do not rule DevTools out for a \
                     non-test keyring entry (secboot {}, debug disabled {}; both must \
                     hold){}",
                    f.secure_boot,
                    f.debug_disabled,
                    if verified.cc_mode == expected {
                        String::new()
                    } else {
                        format!(
                            "; the policy step also recorded {}, a disagreement inside \
                             the broker",
                            verified.cc_mode.label()
                        )
                    }
                )));
            }
            // The claims are good, so any disagreement is the broker arguing
            // with itself. Refused as `verification` and not as a fault (500):
            // the wire has no `internal` kind (D10), and of the five it has,
            // `unavailable` is the one that says come back later. This one
            // will read the same way for ever.
            if verified.cc_mode != expected {
                return Err(KbsError::verification(format!(
                    "release gate: the claims rule DevTools out (secboot {}, debug \
                     disabled {}) but the policy step recorded {}; refusing rather than \
                     releasing on a disagreement inside the broker",
                    f.secure_boot,
                    f.debug_disabled,
                    verified.cc_mode.label(),
                )));
            }
        }
    }
    gate_authority(manifest, set, now, cpu, replayed, gpu, policy, verified)
}

/// D21: the authority half of the independent second gate. In its own few
/// lines, from the RAW documents and the verified evidence, never from the
/// loader's or step 7's derived values: recover the manifest's signer and
/// require all four roots equal; the manifest window and the policy's
/// minimums; re-count distinct current signers from the raw signatures; the
/// headline profile is in the set, in its window, of an allowed class and
/// pinned where required; its identity rows against the verified values
/// (compared as lowercase hex strings, a different technique from the
/// matcher's byte compare); the RTMR3 replay; the MATCHED SET re-derived over
/// every profile (must equal step 7's, headline first); the GPU floors of the
/// policy and of every profile in that re-derived set (D7, D15). What it shares with step 7 is stated in
/// the plan (one parse, the crypto primitives incl. `version_at_least`,
/// `Policy`, `Replayed`, the verified TDX values, `now`, the loader's HWM
/// decision).
#[allow(clippy::too_many_arguments)]
fn gate_authority(
    manifest: &SignedAuthorityManifest,
    set: &SignedHostProfileSet,
    now: u64,
    cpu: &TdxEvidence,
    replayed: &Replayed,
    gpu: &GpuOutcome,
    policy: &Policy,
    verified: &Verified,
) -> Result<(), KbsError> {
    let refuse = |what: String| Err(KbsError::verification(format!("release gate: {what}")));
    let m = &manifest.manifest;
    let s = &set.set;
    let a = &policy.host_authority;
    // Roots: recovered manifest signer == policy root == manifest.root == set.root.
    let signer = match recover_document_signer(m, &manifest.signature) {
        Ok(addr) => addr,
        Err(e) => return refuse(format!("manifest signature: {e}")),
    };
    if signer != a.root || m.root != a.root || s.root != a.root {
        return refuse(format!(
            "authority roots disagree: manifest signed by {signer}, manifest.root {}, set.root {}, policy root {}",
            m.root, s.root, a.root
        ));
    }
    // Manifest window and the policy's minimums.
    if now < m.not_before || now > m.expiry {
        return refuse(format!(
            "manifest v{} outside its window {}..={} at {now}",
            m.manifest_version, m.not_before, m.expiry
        ));
    }
    if m.manifest_version < a.min_manifest_version {
        return refuse(format!(
            "manifest v{} below min_manifest_version {}",
            m.manifest_version, a.min_manifest_version
        ));
    }
    if m.threshold < a.min_threshold.max(1) {
        return refuse(format!(
            "manifest threshold {} below min_threshold {}",
            m.threshold, a.min_threshold
        ));
    }
    if s.epoch < a.min_profile_epoch {
        return refuse(format!(
            "set epoch {} below min_profile_epoch {}",
            s.epoch, a.min_profile_epoch
        ));
    }
    // Re-count distinct current signers from the raw signatures.
    let digest = match canonical_bytes(s) {
        Ok(c) => eip191_digest(&c),
        Err(e) => return refuse(format!("set canonical bytes: {e}")),
    };
    let mut counted: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for sig in &set.signatures {
        if let Ok(addr) = recover_client_address(sig, &digest) {
            let addr = addr.to_lowercase();
            if m.signers.iter().any(|x| x == &addr) {
                counted.insert(addr);
            }
        }
    }
    if (counted.len() as u64) < u64::from(m.threshold) {
        return refuse(format!(
            "{} distinct current signer(s) on the set, threshold {}",
            counted.len(),
            m.threshold
        ));
    }
    // The headline profile, re-judged.
    let id = &verified.profile.headline;
    let Some(p) = s.profiles.iter().find(|p| &p.id == id) else {
        return refuse(format!(
            "headline profile {id} is not in epoch {}; refusing rather than releasing on a \
             disagreement inside the broker",
            s.epoch
        ));
    };
    if !(p.not_before <= now && now <= p.expiry) {
        return refuse(format!(
            "profile {id} outside its window {}..={} at {now}",
            p.not_before, p.expiry
        ));
    }
    if !a.allowed_classes.iter().any(|c| c == &p.class) {
        return refuse(format!(
            "profile {id} class {} not allowed by the policy",
            p.class
        ));
    }
    if (policy.cvm.require_app_id_pinned && p.cvm.app_id.is_none())
        || (policy.cvm.require_key_provider_pinned && p.cvm.key_provider.is_none())
    {
        return refuse(format!("profile {id} is not pinned as the policy requires"));
    }
    // Identity rows of a profile against the verified values, in the gate's
    // own code (lowercase hex strings, not the matcher's byte compare).
    let mismatches = |q: &HostProfile| -> Vec<&'static str> {
        let mut bad = Vec::new();
        if hex::encode(cpu.mr_td) != q.cvm.mrtd {
            bad.push("mrtd");
        }
        if hex::encode(cpu.rt_mr0) != q.cvm.rtmr0 {
            bad.push("rtmr0");
        }
        if hex::encode(cpu.rt_mr1) != q.cvm.rtmr1 {
            bad.push("rtmr1");
        }
        if hex::encode(cpu.rt_mr2) != q.cvm.rtmr2 {
            bad.push("rtmr2");
        }
        if hex::encode(replayed.os_image_hash) != q.cvm.os_image_hash {
            bad.push("os image");
        }
        if !q
            .cvm
            .compose_hashes
            .contains(&hex::encode(replayed.compose_hash))
        {
            bad.push("compose hash");
        }
        if let Some(want) = &q.cvm.app_id {
            if replayed.app_id.as_ref().map(hex::encode).as_ref() != Some(want) {
                bad.push("app id");
            }
        }
        if let Some(want) = &q.cvm.key_provider {
            if replayed.key_provider.as_ref().map(hex::encode).as_ref() != Some(want) {
                bad.push("key provider");
            }
        }
        if let GpuOutcome::Real(f) = gpu {
            if !q.gpu.hwmodels.contains(&f.hwmodel)
                || !policy.gpu.allowed_hwmodels.contains(&f.hwmodel)
            {
                bad.push("hwmodel");
            }
        }
        bad
    };
    // A candidate (D7): in its window, of an allowed class, pinned as required.
    let admitted = |q: &HostProfile| {
        q.not_before <= now
            && now <= q.expiry
            && a.allowed_classes.iter().any(|c| c == &q.class)
            && !(policy.cvm.require_app_id_pinned && q.cvm.app_id.is_none())
            && !(policy.cvm.require_key_provider_pinned && q.cvm.key_provider.is_none())
    };
    let mut bad = mismatches(p);
    if replayed.rtmr3 != cpu.rt_mr3 {
        bad.push("rtmr3 replay");
    }
    if !bad.is_empty() {
        return refuse(format!(
            "the verified evidence does not match headline profile {id} on {}; refusing \
             rather than releasing on a disagreement inside the broker",
            bad.join(", ")
        ));
    }
    // The matched set, re-derived over EVERY profile: it must equal step 7's
    // (sorted by id, the headline first). Its floors come from here, never
    // from step 7's list, so a step-7 bug that drops a match cannot drop a floor.
    let mut matched: Vec<&HostProfile> = s
        .profiles
        .iter()
        .filter(|q| admitted(q) && mismatches(q).is_empty())
        .collect();
    matched.sort_by(|x, y| x.id.cmp(&y.id));
    let ids: Vec<&str> = matched.iter().map(|q| q.id.as_str()).collect();
    let recorded: Vec<&str> = verified.profile.all.iter().map(String::as_str).collect();
    if ids != recorded || ids.first() != Some(&id.as_str()) {
        return refuse(format!(
            "the policy step recorded matched profiles {recorded:?} (headline {id}), the \
             documents say {ids:?}; refusing rather than releasing on a disagreement inside \
             the broker"
        ));
    }
    // GPU floors: the policy's and every matched profile's, each applied
    // independently (D7; under D15 the mainnet floors live only on profiles).
    if let GpuOutcome::Real(f) = gpu {
        let mut floors = vec![(
            "the policy".to_string(),
            &policy.gpu.min_driver_version,
            &policy.gpu.min_vbios_version,
        )];
        for q in &matched {
            floors.push((
                format!("profile {}", q.id),
                &q.gpu.min_driver_version,
                &q.gpu.min_vbios_version,
            ));
        }
        for (whose, driver, vbios) in floors {
            if let Some(min) = driver {
                if !version_at_least(&f.driver_version, min) {
                    return refuse(format!(
                        "driver {} below floor {min} of {whose}",
                        f.driver_version
                    ));
                }
            }
            if let Some(min) = vbios {
                if !version_at_least(&f.vbios_version, min) {
                    return refuse(format!(
                        "vbios {} below floor {min} of {whose}",
                        f.vbios_version
                    ));
                }
            }
        }
    }
    // Step 7's record against the raw documents.
    let mut want: Vec<String> = counted.into_iter().collect();
    let mut got = verified.profile.counted_signers.clone();
    want.sort();
    got.sort();
    if verified.profile.epoch != s.epoch
        || verified.profile.manifest_version != m.manifest_version
        || want != got
    {
        return refuse(format!(
            "the policy step recorded epoch {} manifest v{} signers {:?}, the documents say \
             epoch {} manifest v{} signers {:?}; refusing rather than releasing on a \
             disagreement inside the broker",
            verified.profile.epoch,
            verified.profile.manifest_version,
            got,
            s.epoch,
            m.manifest_version,
            want
        ));
    }
    Ok(())
}

fn verified_json(
    cpu: &TdxEvidence,
    r: &Replayed,
    gpu: &GpuOutcome,
    v: Option<&Verified>,
    auth: &Authority,
) -> serde_json::Value {
    serde_json::json!({
        "tdx": {
            "mr_td": hex::encode(cpu.mr_td),
            "rt_mr0": hex::encode(cpu.rt_mr0),
            "rt_mr1": hex::encode(cpu.rt_mr1),
            "rt_mr2": hex::encode(cpu.rt_mr2),
            "rt_mr3": hex::encode(cpu.rt_mr3),
            "report_data": hex::encode(cpu.report_data),
            "td_debug": cpu.td_debug,
            "tcb_status": cpu.tcb_status,
            "advisory_ids": cpu.advisory_ids,
        },
        "events": {
            "rtmr3": hex::encode(r.rtmr3),
            "compose_hash": hex::encode(r.compose_hash),
            "os_image_hash": hex::encode(r.os_image_hash),
            "app_id": r.app_id.as_ref().map(hex::encode),
            "key_provider": r.key_provider.as_ref().map(hex::encode),
        },
        "gpu": match gpu {
            GpuOutcome::Real(f) => serde_json::json!({
                "hwmodel": f.hwmodel, "secure_boot": f.secure_boot, "debug_disabled": f.debug_disabled,
                "driver_version": f.driver_version, "vbios_version": f.vbios_version,
                "cc_mode": f.cc_assertion().label(),
            }),
            GpuOutcome::CannedTolerated => serde_json::json!({"canned_tolerated": true}),
        },
        // Schema 3 (D17d): which documents judged this request, and (on a
        // pass) which profile released it.
        "profile": {
            "epoch": auth.set.set.epoch,
            "manifest_version": auth.manifest.manifest.manifest_version,
            "set_digest": hex::encode(auth.set_digest),
            "manifest_digest": hex::encode(auth.manifest_digest),
            "signers": auth.counted.signers,
            "ignored_signers": auth.counted.ignored,
            "headline": v.map(|v| v.profile.headline.clone()),
            "matched": v.map(|v| v.profile.all.clone()),
            "class": v.map(|v| v.profile.class.clone()),
        },
        "verified": v.map(|v| serde_json::json!({
            "tcb_status": v.tcb_status, "advisory_ids": v.advisory_ids, "hwmodel": v.hwmodel,
            "driver_version": v.driver_version, "vbios_version": v.vbios_version,
            "cc_mode": v.cc_mode.label(), "td_debug_off": v.td_debug_off,
        })),
    })
}

/// The `released` line's and `decision.txt`'s schema-3 fields (D17d).
fn profile_fields(v: &Verified, auth: &Authority) -> String {
    format!(
        "profile={} class={} epoch={} manifest={} set_digest={} manifest_digest={} signers={}",
        v.profile.headline,
        v.profile.class,
        v.profile.epoch,
        v.profile.manifest_version,
        &hex::encode(auth.set_digest)[..16],
        &hex::encode(auth.manifest_digest)[..16],
        v.profile.counted_signers.join(",")
    )
}

/// Every kind maps to a status the node understands (design D10); the table is
/// what the routes tests assert.
pub fn status_of(kind: Kind) -> u16 {
    kind.default_status()
}

/// A helper for the bin: bind and serve with connect info, until `shutdown`.
pub async fn serve(
    state: Shared,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(state.cfg.listen).await?;
    tracing::info!(listen = %state.cfg.listen, "fabstir-kbs serving");
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}
