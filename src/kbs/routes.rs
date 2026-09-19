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
use crate::kbs::nras_claims::GpuOutcome;
use crate::kbs::policy_file::{load_policy, FilePolicySource};
use crate::kbs::verify::{check_policy, prefilter, CcRecord, Expected, Verified};
use crate::tee::kbs_http::{
    ChallengeResponse, ErrorBody, ErrorInner, EvidenceWire, RequestKeyRequest, RequestKeyResponse,
    WrappedKeyWire,
};
use crate::tee::keywrap::wrap_key;
use crate::tee::policy_source::ProviderRegistry;
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
use std::time::{Instant, SystemTime, UNIX_EPOCH};
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
            permits: Arc::new(Semaphore::new(cfg.request_concurrency)),
            per_source: Mutex::new(HashMap::new()),
            cfg,
            keyring,
            class,
            egress,
            registry,
        })
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
    load_policy(
        &state.policy,
        &state.registry,
        model_id,
        entry.min_policy_version,
    )
    .await?;
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

        // Step 3: policy.
        let signed = load_policy(
            &state.policy,
            &state.registry,
            model_id,
            entry.min_policy_version,
        )
        .await
        .map_err(|e| (e, Ring::Preverify))?;
        let policy_hash = signed.policy_hash().map(hex::encode).unwrap_or_default();
        timings.push(format!("policy={}ms", t.elapsed().as_millis()));

        // Step 4: pre-filter.
        stage.store(4, std::sync::atomic::Ordering::Relaxed);
        let t = Instant::now();
        let pf = prefilter(
            &signed.policy,
            &expected,
            &evidence.cpu_quote,
            &evidence.event_log,
            &evidence.gpu_report,
            state.cfg.gpu_evidence,
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
                let (ev, trace) = cpu::verify_real(
                    &evidence.cpu_quote,
                    &memo,
                    state.cfg.pccs_url.as_str(),
                    now_unix(),
                )
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
            &expected,
            &cpu_ev,
            &pf.replayed,
            &gpu_outcome,
            entry.test,
        );
        rec.verified = Some(verified_json(
            &cpu_ev,
            &pf.replayed,
            &gpu_outcome,
            verified.as_ref().ok(),
        ));
        let verified = verified.map_err(|e| (e, Ring::Verified))?;

        // Step 8: the independent second gate.
        release_gate(&verified, entry.test).map_err(|e| (e, Ring::Verified))?;

        // Step 9: wrap.
        let wrapped = wrap_key(&entry.dek, &expected.pk_att)
            .map_err(|e| (KbsError::fault(format!("wrap: {e}")), Ring::Verified))?;
        let log = format!(
            "policy_version={} policy_hash={policy_hash} tcb={} advisories={:?} hwmodel={:?} driver={:?} vbios={:?} cc_mode={:?} test_release={} {}",
            signed.policy.policy_version,
            verified.tcb_status,
            verified.advisory_ids,
            verified.hwmodel,
            verified.driver_version,
            verified.vbios_version,
            verified.cc_mode,
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

/// Design §6 step 8 (D4's independent second gate): canned tolerance releases
/// only a `test: true` entry. Step 7 already refuses this case; the gate exists so
/// that a mode-plumbing bug upstream cannot release a real DEK, and it is its own
/// function so it can be tested directly.
pub fn release_gate(verified: &Verified, entry_test: bool) -> Result<(), KbsError> {
    if verified.cc_mode == CcRecord::Canned && !entry_test {
        return Err(KbsError::verification(
            "release gate: canned tolerance for a non-test keyring entry",
        ));
    }
    Ok(())
}

fn verified_json(
    cpu: &TdxEvidence,
    r: &Replayed,
    gpu: &GpuOutcome,
    v: Option<&Verified>,
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
                "driver_version": f.driver_version, "vbios_version": f.vbios_version, "cc_mode": "node-asserted",
            }),
            GpuOutcome::CannedTolerated => serde_json::json!({"canned_tolerated": true}),
        },
        "verified": v.map(|v| serde_json::json!({
            "tcb_status": v.tcb_status, "advisory_ids": v.advisory_ids, "hwmodel": v.hwmodel,
            "driver_version": v.driver_version, "vbios_version": v.vbios_version,
            "cc_mode": format!("{:?}", v.cc_mode), "td_debug_off": v.td_debug_off,
        })),
    })
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
