// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The pre-train pipeline (interface C.3 order): session read (fail CLOSED,
//! FIRST) → consumed-session peek → template validation → schedule → sidecar
//! consult (B.6) → attempt claim → A.3 gates → manifest fetch →
//! C.6 plausibility → cross-checks → shard staging → scan → recount —
//! terminating either in a [`PreparedTrain`] (T4's slice loop takes over) or
//! a [`TrainReject`] whose wire code follows the sidecar CONTRACT §3.7 table
//! and interface C.3/C.4 exactly.
//!
//! TERMINAL-REJECT LAW (C.3's UNIVERSAL rule, realigned in the T3 converge
//! round — the first cut exempted template-shape and capacity rejects, which
//! contradicted the frozen text): once the session has been READ from chain,
//! EVERY terminal reject — template shape, sidecar-consult failure, TD14
//! capacity, C.6 address rules, A.3 gates, and all dataset legs — consumes
//! the session forever (registry), deletes the staging dir (TD15), and
//! schedules the ZERO-token settle no earlier than `sessionCreation +
//! disputeWindow + buffer`. The ONLY non-consuming rejects: a chain-READ
//! failure (`CAPACITY` — an unverified session must never be completed or
//! burned; retry is safe), and the registry's `SessionReused`/`TrainActive`
//! refusals (their settle is already scheduled / owned by the running
//! attempt — a second completion would be wrong). Both are recorded for the
//! interface changelog (SDK-visible clarification).
//!
//! Settle-loss gap (RECORDED, accepted for M0): the zero-settle retries on
//! completer error (bounded backoff below) but is an in-memory task — a node
//! restart inside the dispute wait LOSES it; the chain's `max_duration`
//! session timeout is the client's backstop (full refund via
//! `triggerSessionTimeout`, anyone-callable). A durable settle-intent record
//! replayed at boot is the D-track fix.
//!
//! Code decisions this module owns (recorded, not interface-frozen):
//! S5 transport death during staging and local disk failure →
//! `SIDECAR_UNAVAILABLE` class (host infra); an UNRECOGNISED scan verdict →
//! `SIDECAR_UNAVAILABLE` (version skew is a deployment fault — fail closed
//! without branding the dataset).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ethers::types::{Address, U256};

use crate::training::accept::{
    plausibility_gate, validate_session, AcceptConfig, AcceptReject, AttemptClaim, AttemptRegistry,
    SessionSnapshot,
};
use crate::training::schedule::slice_deltas;
use crate::training::staging::{cross_check_manifest, fetch_manifest, stage_shards, StageError};
use crate::training::submit::SessionComplete;
use crate::training::trainer_client::{SidecarFailure, TrainerClient};
use crate::training::types::TrainingJob;

// The wire codes this pipeline can emit (interface Contract C).
pub const VALIDATION_FAILED: &str = "VALIDATION_FAILED";
pub const CAPACITY: &str = "CAPACITY";
pub const SIDECAR_UNAVAILABLE: &str = "SIDECAR_UNAVAILABLE";
pub const DATASET_INTEGRITY: &str = "DATASET_INTEGRITY";
pub const DECLARED_TOKENS_MISMATCH: &str = "DECLARED_TOKENS_MISMATCH";
pub const CONTENT_BLOCKED: &str = "CONTENT_BLOCKED";
pub const CONTENT_FLAGGED: &str = "CONTENT_FLAGGED";
pub const MODERATION_UNAVAILABLE: &str = "MODERATION_UNAVAILABLE";

/// On-chain session reads, mockable (the production impl copies the
/// drift-proof raw-word pattern — NEVER the 17-field typed decode).
#[async_trait::async_trait]
pub trait SessionReader: Send + Sync {
    async fn session_snapshot(&self, job_id: u64) -> Result<SessionSnapshot, String>;
    async fn session_model(&self, job_id: u64) -> Result<[u8; 32], String>;
    async fn dispute_window_secs(&self) -> u64;
}

/// The node-side training template (T1.4's pinned JSON, typed). The
/// authoritative file is Jules's to author; tests build synthetic ones.
#[derive(Debug, Clone)]
pub struct TrainingTemplate {
    pub template_id: String,
    /// A.2/E.2 `baseServingModelId`: the bytes32 id of the already-registered
    /// GGUF inference model this adapter serves against. Read from
    /// `/base/baseServingModelId`, alongside `tokenizerSha256`, because A.2
    /// groups it with the base model. **Required**: without it E.2's equality
    /// check is uncomputable, so a template that omits it must fail at node
    /// boot rather than at a paying customer's session init.
    pub base_serving_model_id: String,
    /// "0x" + keccak256 of the canonical template JSON (the B.6 pin).
    pub template_hash: String,
    pub tokenizer_sha256: String,
    pub ranks: Vec<u32>,
    pub alphas: Vec<u32>,
    pub seq_lens: Vec<u32>,
    pub lrs: Option<Vec<String>>,
    pub max_epochs: u32,
    /// A.4 `maxTotalTokens` (declared × epochs ceiling; M0 pin 15,000,000).
    pub max_total_tokens: u64,
    /// B.1 `sliceTokens` (M0 pin 1,000,000).
    pub slice_tokens: u64,
}

/// Everything the pipeline needs, seams first (all mockable).
pub struct TrainingDeps {
    pub sessions: Arc<dyn SessionReader>,
    pub completer: Arc<dyn SessionComplete>,
    pub trainer: Arc<TrainerClient>,
    pub attempts: Arc<AttemptRegistry>,
    pub staging_root: PathBuf,
    pub s5_base: String,
    pub host_address: Address,
    pub model_id: [u8; 32],
    pub expected_price: U256,
    pub priced_tokens: Vec<Address>,
    pub template: TrainingTemplate,
    /// T5.3: the per-SESSION serve-back registry (E.2). Lives here so the
    /// session-init path and the WebSocket close path reach the same one.
    pub adapters: Arc<crate::training::serve::AdapterRegistry>,
    pub accept_cfg: AcceptConfig,
    pub cooldown_secs: u64,
    /// Buffer over the dispute window for the zero-settle timing (C.3).
    pub settle_buffer_secs: u64,
    /// TTL cache for the PUBLIC capacity-hint route (T3 converge round: the
    /// route must never open a sidecar connection per unauthenticated
    /// request — the transcode capacity route's cached-status rule).
    pub capacity_cache: std::sync::Mutex<Option<(std::time::Instant, bool)>>,
    // --- T4 (slice loop) seams ---
    /// The node's view of TRAINING_WORK_ROOT (checkpoint pickup + the §5
    /// consumption-by-deletion signal).
    pub work_root: PathBuf,
    /// Artifact/attestation uploads (capability blobs; mock in Band A).
    pub artifact_store: Arc<dyn crate::storage::s5_client::S5Storage>,
    /// Per-slice `submitProofOfWork` (the LTX trait; mock in Band A).
    pub proof: Arc<dyn crate::ltx::submit::ProofSubmit>,
    /// The run billing/race machine.
    pub tracker: Arc<crate::training::tracker::TrainTracker>,
    /// Node signing key for the B.5 attestation signature.
    pub node_key: [u8; 32],
    /// The B.3 `envHash` (the LTX honest-posture constant).
    pub env_hash: String,
    /// This model's registered rate limit (tokens/sec) — drives the
    /// "Too many" retry wait (10,000 for training; NOT LTX's 2,000).
    pub rate_limit_tokens_per_sec: u64,
    /// The tracker's completing-latch width.
    pub completing_latch: Duration,
    /// The bundle's allowlist version (echoed in `train_accepted`).
    pub allow_list_version: u64,
}

/// A terminal wire rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainReject {
    pub code: &'static str,
    /// `detail.reason` for `VALIDATION_FAILED` shapes (`sessionParams`,
    /// `sessionReused`, `trainActive`, `datasetFormat`, `plausibility`…).
    pub reason: Option<&'static str>,
    pub detail: String,
    /// `DECLARED_TOKENS_MISMATCH` carries `{ declared, actual }`.
    pub declared_actual: Option<(u64, u64)>,
}

impl TrainReject {
    fn new(code: &'static str, reason: Option<&'static str>, detail: String) -> Self {
        TrainReject {
            code,
            reason,
            detail,
            declared_actual: None,
        }
    }
}

/// The successful pre-train outcome: everything T4's slice loop needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTrain {
    pub staged_dataset: PathBuf,
    pub training_tokens: u64,
    /// The pinned B.1 schedule deltas.
    pub schedule: Vec<u64>,
    /// The VERIFIED on-chain session price (never an env value — A.3).
    pub price_per_token: U256,
    pub verdict: String,
    pub policy_version: String,
}

fn map_accept_reject(reject: AcceptReject) -> TrainReject {
    match reject {
        AcceptReject::SessionParams { detail } => {
            TrainReject::new(VALIDATION_FAILED, Some("sessionParams"), detail)
        }
        AcceptReject::Plausibility { detail } => {
            TrainReject::new(VALIDATION_FAILED, Some("plausibility"), detail)
        }
        AcceptReject::SessionReused => TrainReject::new(
            VALIDATION_FAILED,
            Some("sessionReused"),
            "session reused".into(),
        ),
        AcceptReject::TrainActive => TrainReject::new(
            VALIDATION_FAILED,
            Some("trainActive"),
            "train active".into(),
        ),
    }
}

fn map_stage(error: StageError) -> TrainReject {
    // Round-6 F-R6-2: `Transport` and `Io` used to be echoed verbatim into the
    // client's reject frame. Their details are built from foreign errors —
    // `Io` renders the absolute `TRAINING_STAGING_ROOT` (`staging.rs` create),
    // `Transport` renders the full `ENHANCED_S5_URL` blob URL through
    // reqwest's Display — so a job submitted during any infra wobble handed
    // the client the node's topology. Same class as the serve-back leak, one
    // feature over. Whitelist the two classes built from the client's own
    // claims; log the rest and send fixed text.
    match error {
        StageError::Integrity(detail) => TrainReject::new(DATASET_INTEGRITY, None, detail),
        StageError::Validation(detail) => TrainReject::new(VALIDATION_FAILED, None, detail),
        StageError::Transport(detail) => {
            tracing::error!("dataset staging transport failure: {detail}");
            TrainReject::new(
                SIDECAR_UNAVAILABLE,
                None,
                "dataset staging transport failure (host infra)".to_string(),
            )
        }
        StageError::Io(detail) => {
            tracing::error!("dataset staging volume failure: {detail}");
            TrainReject::new(
                SIDECAR_UNAVAILABLE,
                None,
                "staging volume failure (host infra)".to_string(),
            )
        }
    }
}

/// The §3.7 table for sidecar scan/count interactions.
fn map_sidecar(failure: SidecarFailure, leg: &str) -> TrainReject {
    match failure {
        // Round-7 F-R7-2: `detail` here can be the sidecar's ENTIRE HTTP
        // response body (a framework traceback carries this node's absolute
        // install paths), so it is logged rather than echoed — the same
        // discipline F-R6-2 applied to `map_stage`, which is the mapper
        // directly above this one.
        SidecarFailure::Transport(detail) => TrainReject::new(
            SIDECAR_UNAVAILABLE,
            None,
            crate::training::redact::opaque(
                &format!("sidecar transport failure during {leg}"),
                detail,
            ),
        ),
        SidecarFailure::Envelope { kind, detail, .. } => match kind.as_str() {
            // The dataset genuinely is not jsonl-text-v1: terminal validation.
            // Sidecar-authored and about the CLIENT'S data, so it is echoed —
            // bounded, because it is not a node-authored constant.
            "DATASET_MALFORMED" => TrainReject::new(
                VALIDATION_FAILED,
                Some("datasetFormat"),
                crate::training::redact::echo(&detail),
            ),
            // A LIVE scanner's explicit no-verdict: terminal, never re-shopped.
            "SCAN_FAILURE" => TrainReject::new(
                MODERATION_UNAVAILABLE,
                None,
                crate::training::redact::echo(&detail),
            ),
            // SOURCE_* / TEMPLATE_BOUNDS / COUNT_FAILURE / SOURCE_MUTATED /
            // CLIENT_GONE / framework 422: deployment fault or node/pin skew.
            // The SOURCE_* details describe the dataset path this node handed
            // the sidecar, so they are node topology, not client information.
            other => TrainReject::new(
                SIDECAR_UNAVAILABLE,
                None,
                crate::training::redact::opaque(
                    // Round-8 F-R8-8: `other` is an arbitrary envelope kind
                    // from the sidecar, so it is bounded even here — the
                    // context half of `opaque` is the half that IS echoed.
                    &format!(
                        "sidecar {leg} rejected ({}) — deployment fault, operator alert",
                        crate::training::redact::echo(other)
                    ),
                    detail,
                ),
            ),
        },
    }
}

fn u256_secs(value: U256) -> u64 {
    if value > U256::from(u64::MAX) {
        u64::MAX
    } else {
        value.as_u64()
    }
}

/// C.3: schedule the zero-token settle no earlier than
/// `sessionCreation + disputeWindow + buffer` (the delay runs on the tokio
/// clock; paused-clock tests prove the timing).
async fn schedule_zero_settle(
    deps: &TrainingDeps,
    job_id: u64,
    snapshot: &SessionSnapshot,
    now_secs: u64,
) {
    let dispute = deps.sessions.dispute_window_secs().await;
    let due = u256_secs(snapshot.start_time)
        .saturating_add(dispute)
        .saturating_add(deps.settle_buffer_secs);
    let delay = due.saturating_sub(now_secs);
    let completer = deps.completer.clone();
    tokio::spawn(async move {
        if delay > 0 {
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
        // Bounded retry on completer error (transient RPC / nonce / a
        // "Dispute wait" the buffer under-estimated). Restart-loss remains
        // the recorded M0 gap (module docstring).
        for attempt in 1..=5u32 {
            match completer.complete_session(job_id).await {
                Ok(()) => return,
                Err(error) if attempt < 5 => {
                    tracing::warn!(
                        "training zero-settle for job {job_id} failed (attempt {attempt}/5): {error}"
                    );
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
                Err(error) => {
                    tracing::error!(
                        "training zero-settle for job {job_id} EXHAUSTED retries: {error} — \
                         the chain max_duration timeout is the client's backstop"
                    );
                }
            }
        }
    });
}

/// Template-shape validation (cheap, BEFORE any session consumption).
fn validate_against_template(
    template: &TrainingTemplate,
    job: &TrainingJob,
) -> Result<u64, TrainReject> {
    let fail = |detail: String| Err(TrainReject::new(VALIDATION_FAILED, None, detail));
    if job.template_id != template.template_id {
        // Round-8 F-R8-2: these four are raw wire strings echoed at accept
        // step 3, BEFORE the A.3 gates, so no funding has been verified. With
        // no `max_message_size` set, `{:?}` on a 64 MiB String roughly doubles
        // it — the amplifier `redact.rs`'s own docstring describes.
        return fail(format!(
            "unknown templateId {:?}",
            crate::training::redact::echo(&job.template_id)
        ));
    }
    if !job
        .template_hash
        .eq_ignore_ascii_case(&template.template_hash)
    {
        return fail(format!(
            "templateHash mismatch: wire {} vs pinned {}",
            crate::training::redact::echo(&job.template_hash),
            template.template_hash
        ));
    }
    if !template.ranks.contains(&job.hyper.rank) {
        return fail(format!(
            "rank {} not in {:?}",
            job.hyper.rank, template.ranks
        ));
    }
    if !template.alphas.contains(&job.hyper.alpha) {
        return fail(format!(
            "alpha {} not in {:?}",
            job.hyper.alpha, template.alphas
        ));
    }
    if !template.seq_lens.contains(&job.hyper.seq_len) {
        return fail(format!(
            "seqLen {} not in {:?}",
            job.hyper.seq_len, template.seq_lens
        ));
    }
    if job.epochs == 0 || job.epochs > template.max_epochs {
        return fail(format!(
            "epochs {} outside 1..={}",
            job.epochs, template.max_epochs
        ));
    }
    if !job.hyper.lr_is_canonical() {
        return fail(format!(
            "lr {:?} is not canonical decimal",
            crate::training::redact::echo(&job.hyper.lr)
        ));
    }
    if let Some(lrs) = &template.lrs {
        if !lrs.contains(&job.hyper.lr) {
            // Round-9 F-R9-2: the FIFTH echo in this function, five lines
            // below one F-R8-2 bounded, on the SAME field. `lr_is_canonical`
            // imposes no length bound, so a 200,000-digit lr is canonical and
            // lands here. Live wherever a template pins `method.lrs`, which
            // the sidecar supports and enforces.
            return fail(format!(
                "lr {:?} not in the template's pinned list",
                crate::training::redact::echo(&job.hyper.lr)
            ));
        }
    }
    if job.output != "adapter-v1" {
        return fail(format!(
            "output {:?} is not adapter-v1",
            crate::training::redact::echo(&job.output)
        ));
    }
    let Some(total) =
        crate::training::schedule::training_tokens(job.dataset.declared_tokens, job.epochs)
    else {
        return fail("declaredTokens × epochs overflows".to_string());
    };
    if total == 0 || total > template.max_total_tokens {
        return fail(format!(
            "trainingTokens {total} outside 1..=maxTotalTokens {}",
            template.max_total_tokens
        ));
    }
    Ok(total)
}

/// The fallible pipeline AFTER the attempt claim (every Err here triggers
/// the terminal-reject side effects in `accept_and_prepare`).
/// The session-level acceptance outcome: everything the `train_accepted`
/// ack needs (verified price, pinned schedule) BEFORE any dataset work —
/// the staging/scan/count legs run afterwards under progress frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedSession {
    pub snapshot: SessionSnapshot,
    pub training_tokens: u64,
    /// The pinned B.1 schedule deltas (computable from job + template alone).
    pub schedule: Vec<u64>,
}

/// The shared terminal-reject side effects for CONSUMING rejects (C.3's
/// universal rule — with or without a prior attempt claim): the session is
/// consumed forever (registry; the depositor's cooldown starts only when
/// `arm_cooldown` — the C.6 real-work rule), the staging dir is swept
/// (TD15), and the zero-token settle is scheduled exactly once.
pub async fn terminal_reject_effects(
    deps: &TrainingDeps,
    job_id: u64,
    snapshot: &SessionSnapshot,
    now_secs: u64,
    arm_cooldown: bool,
) {
    // The registry transition is the EXACTLY-ONCE key: only the first
    // terminal record for a session schedules its settle (round-2 catch —
    // concurrent consult failures double-scheduled it; the second on-chain
    // completion would just revert, but exactly-once is exactly-once).
    let newly_recorded =
        deps.attempts
            .record_terminal_reject(job_id, snapshot.depositor, now_secs, arm_cooldown);
    let staging_dir = deps.staging_root.join(format!("job-{job_id}"));
    let _ = tokio::fs::remove_dir_all(&staging_dir).await;
    if newly_recorded {
        schedule_zero_settle(deps, job_id, snapshot, now_secs).await;
    }
}

/// Wrap a reject with the consuming side effects (post-read rejects only).
/// `arm_cooldown` follows the C.6 rule on `record_terminal_reject`.
async fn consuming(
    deps: &TrainingDeps,
    job_id: u64,
    snapshot: &SessionSnapshot,
    now_secs: u64,
    arm_cooldown: bool,
    reject: TrainReject,
) -> TrainReject {
    terminal_reject_effects(deps, job_id, snapshot, now_secs, arm_cooldown).await;
    reject
}

/// Session-level acceptance: template shape → chain reads (fail CLOSED,
/// retryable, consuming nothing) → attempt claim (keyed on the DEPOSITOR —
/// the funding address is the C.6 "client address") → the A.3 gates. An A.3
/// failure is a CONSUMING reject (the claim succeeded) and runs the terminal
/// side effects.
pub async fn accept_session(
    deps: &TrainingDeps,
    job_id: u64,
    job: &TrainingJob,
    now_secs: u64,
) -> Result<AcceptedSession, TrainReject> {
    // 1. Chain reads FIRST — fail CLOSED but consuming/settling NOTHING (an
    // unverified session must never be completed or burned; retry is safe).
    // Round-7 F-R7-1 (HIGH): these echoed the provider error, whose reqwest
    // Display writes " for url ({url})" — the node's RPC endpoint, commonly
    // holding an API key — to any client that can open a session and name a
    // job id, with no funding checked at this point. Same class as F-R6-1,
    // in the feature F-R6-1 was found beside.
    let snapshot = deps.sessions.session_snapshot(job_id).await.map_err(|e| {
        TrainReject::new(
            CAPACITY,
            // The SDK cannot otherwise tell this from a busy slot, and the
            // two demand OPPOSITE client behaviour: here the node could not
            // read the session, so it consumed and settled NOTHING and a
            // retry is safe. Sharpened by round-7 F-R7-1, which necessarily
            // made the message opaque and so removed the only other signal.
            Some("chainUnavailable"),
            crate::training::redact::opaque("session read unavailable (fail closed, retry)", e),
        )
    })?;
    let session_model = deps.sessions.session_model(job_id).await.map_err(|e| {
        TrainReject::new(
            CAPACITY,
            Some("chainUnavailable"),
            crate::training::redact::opaque(
                "sessionModel read unavailable (fail closed, retry)",
                e,
            ),
        )
    })?;

    // 2. Consumed-session short-circuit (advisory peek; `try_begin` below
    // stays the atomic claim): a session the registry already ended must
    // answer `sessionReused` — NOT re-run the consult and schedule a SECOND
    // zero-settle (converge-round catch).
    match deps.attempts.peek(job_id) {
        AttemptClaim::SessionReused => {
            return Err(TrainReject::new(
                VALIDATION_FAILED,
                Some("sessionReused"),
                "one `train` per session, ever (A.3)".to_string(),
            ))
        }
        AttemptClaim::TrainActive => {
            return Err(TrainReject::new(
                VALIDATION_FAILED,
                Some("trainActive"),
                "a `train` already runs on this session".to_string(),
            ))
        }
        _ => {}
    }

    // 3. Template shape (a funded session terminally rejected here IS
    // consumed + settled — C.3's universal rule; the frozen A.4 allowlist-
    // drift scenario rides this arm).
    let training_tokens = match validate_against_template(&deps.template, job) {
        Ok(total) => total,
        Err(reject) => return Err(consuming(deps, job_id, &snapshot, now_secs, true, reject).await),
    };
    // 3. The pinned schedule (pure; loader validation makes failure a
    // template-misconfig — still a consuming reject, never a registry leak).
    let schedule = match slice_deltas(training_tokens, deps.template.slice_tokens) {
        Ok(schedule) => schedule,
        Err(e) => {
            let reject = TrainReject::new(VALIDATION_FAILED, None, format!("schedule: {e}"));
            return Err(consuming(deps, job_id, &snapshot, now_secs, true, reject).await);
        }
    };

    // 4. Accept-time sidecar consult (B.6 pin echo + run-slot check — the
    // capacity clause: "not only its own semaphore"). A pin skew is a HOST
    // deployment fault: SIDECAR_UNAVAILABLE + operator alert, never a
    // dataset brand; per C.3 it still consumes + settles the session.
    match deps.trainer.health().await {
        Ok(pins) => {
            if !pins
                .template_hash
                .eq_ignore_ascii_case(&deps.template.template_hash)
                || !pins
                    .tokenizer_sha256
                    .eq_ignore_ascii_case(&deps.template.tokenizer_sha256)
            {
                let reject = TrainReject::new(
                    SIDECAR_UNAVAILABLE,
                    None,
                    format!(
                        "sidecar pin skew (B.6): sidecar {{{}, {}}} vs node {{{}, {}}} — operator alert",
                        pins.template_hash,
                        pins.tokenizer_sha256,
                        deps.template.template_hash,
                        deps.template.tokenizer_sha256
                    ),
                );
                return Err(consuming(deps, job_id, &snapshot, now_secs, true, reject).await);
            }
        }
        Err(failure) => {
            let reject = map_sidecar(failure, "health");
            return Err(consuming(deps, job_id, &snapshot, now_secs, true, reject).await);
        }
    }
    match deps.trainer.status().await {
        Ok(status) if status.slot == "free" => {}
        Ok(_) => {
            let reject = TrainReject::new(
                CAPACITY,
                Some("slotBusy"),
                "sidecar run slot busy (accept-time consult)".to_string(),
            );
            // Capacity back-off (the TD14 permit's sibling): consume + settle,
            // never arm the cooldown (round-3 coherence fix).
            return Err(consuming(deps, job_id, &snapshot, now_secs, false, reject).await);
        }
        Err(failure) => {
            let reject = map_sidecar(failure, "status");
            return Err(consuming(deps, job_id, &snapshot, now_secs, true, reject).await);
        }
    }

    // 5. The attempt claim. SessionReused/TrainActive are the two
    // NON-consuming refusals (their settle already exists / is owned by the
    // running attempt); AddressBusy/Cooldown consume + settle per C.3.
    match deps
        .attempts
        .try_begin(job_id, snapshot.depositor, now_secs, deps.cooldown_secs)
    {
        AttemptClaim::Ok => {}
        AttemptClaim::SessionReused => {
            return Err(TrainReject::new(
                VALIDATION_FAILED,
                Some("sessionReused"),
                "one `train` per session, ever (A.3)".to_string(),
            ))
        }
        AttemptClaim::TrainActive => {
            return Err(TrainReject::new(
                VALIDATION_FAILED,
                Some("trainActive"),
                "a `train` already runs on this session".to_string(),
            ))
        }
        AttemptClaim::AddressBusy => {
            let reject = TrainReject::new(
                CAPACITY,
                Some("addressBusy"),
                "one training job per client address at a time (C.6)".to_string(),
            );
            return Err(consuming(deps, job_id, &snapshot, now_secs, false, reject).await);
        }
        AttemptClaim::Cooldown => {
            let reject = TrainReject::new(
                CAPACITY,
                Some("cooldown"),
                "post-reject cooldown for this address (C.6)".to_string(),
            );
            return Err(consuming(deps, job_id, &snapshot, now_secs, false, reject).await);
        }
    }

    // 6. The A.3 gates.
    if let Err(reject) = validate_session(
        &snapshot,
        session_model,
        now_secs,
        U256::from(training_tokens),
        deps.expected_price,
        &deps.priced_tokens,
        deps.model_id,
        deps.host_address,
        &deps.accept_cfg,
    )
    .map_err(map_accept_reject)
    {
        // Ops visibility (round-1 F7): a price-gate reject is the signature
        // of TRAINING_PRICE_PER_TOKEN drifting from the on-chain
        // registration — every honest session burns until fixed.
        if reject.detail.contains("price") {
            tracing::warn!(
                "training A.3 PRICE reject for job {job_id}: {} — check TRAINING_PRICE_PER_TOKEN vs the registration",
                reject.detail
            );
        }
        return Err(consuming(deps, job_id, &snapshot, now_secs, true, reject).await);
    }

    Ok(AcceptedSession {
        snapshot,
        training_tokens,
        schedule,
    })
}

/// The dataset legs (run AFTER the `train_accepted` ack, under progress
/// frames): manifest → plausibility → cross-checks → staging → scan →
/// recount. Every failure here is a CONSUMING terminal reject.
pub async fn prepare_dataset(
    deps: &TrainingDeps,
    job_id: u64,
    job: &TrainingJob,
    accepted: &AcceptedSession,
    now_secs: u64,
    progress: Option<&tokio::sync::mpsc::Sender<RunProgress>>,
) -> Result<PreparedTrain, TrainReject> {
    match dataset_body(deps, job_id, job, accepted, progress).await {
        Ok(prepared) => Ok(prepared),
        Err(reject) => {
            terminal_reject_effects(deps, job_id, &accepted.snapshot, now_secs, true).await;
            Err(reject)
        }
    }
}

async fn dataset_body(
    deps: &TrainingDeps,
    job_id: u64,
    job: &TrainingJob,
    accepted: &AcceptedSession,
    progress: Option<&tokio::sync::mpsc::Sender<RunProgress>>,
) -> Result<PreparedTrain, TrainReject> {
    let stage = |name: &'static str| async move {
        if let Some(sender) = progress {
            let _ = sender.send(RunProgress::Stage { stage: name }).await;
        }
    };
    // Manifest (small) → C.6 plausibility on its numbers → cross-checks.
    stage("staging").await;
    let manifest = fetch_manifest(
        &deps.s5_base,
        &job.dataset.manifest_cid,
        &job.dataset.manifest_sha256,
    )
    .await
    .map_err(map_stage)?;
    plausibility_gate(manifest.total_bytes, manifest.declared_tokens).map_err(map_accept_reject)?;
    cross_check_manifest(&manifest, job, &deps.template.tokenizer_sha256).map_err(map_stage)?;

    // Shards → the staged dataset the sidecar reads.
    let staged = stage_shards(&deps.s5_base, &deps.staging_root, job_id, &manifest)
        .await
        .map_err(map_stage)?;
    let staged_str = staged.to_string_lossy().to_string();

    // Scan (C.4 boundary), then recount (C.3).
    stage("scanning").await;
    let scan = deps
        .trainer
        .scan(&staged_str)
        .await
        .map_err(|f| map_sidecar(f, "scan"))?;
    match scan.verdict.as_str() {
        "cleared" => {}
        "blocked" => {
            return Err(TrainReject::new(
                CONTENT_BLOCKED,
                None,
                "scan verdict: blocked".into(),
            ))
        }
        "flagged" => {
            return Err(TrainReject::new(
                CONTENT_FLAGGED,
                None,
                "scan verdict: flagged".into(),
            ))
        }
        other => {
            // Version skew is a deployment fault — fail closed WITHOUT
            // branding the dataset.
            return Err(TrainReject::new(
                SIDECAR_UNAVAILABLE,
                None,
                format!(
                    "unrecognised scan verdict {:?} (version skew)",
                    crate::training::redact::echo(other)
                ),
            ));
        }
    }
    stage("counting").await;
    let count = deps
        .trainer
        .count(&staged_str)
        .await
        .map_err(|f| map_sidecar(f, "count"))?;
    if count.tokens != job.dataset.declared_tokens {
        let mut reject = TrainReject::new(
            DECLARED_TOKENS_MISMATCH,
            None,
            format!(
                "declaredTokens {} vs recount {}",
                job.dataset.declared_tokens, count.tokens
            ),
        );
        reject.declared_actual = Some((job.dataset.declared_tokens, count.tokens));
        return Err(reject);
    }

    Ok(PreparedTrain {
        staged_dataset: staged,
        training_tokens: accepted.training_tokens,
        schedule: accepted.schedule.clone(),
        price_per_token: accepted.snapshot.price_per_token,
        verdict: scan.verdict,
        policy_version: scan.policy_version,
    })
}

/// The full C.3 pre-train pipeline in one call (the T3 test surface; the
/// handler runs the two halves separately with the ack between them).
/// `now_secs` is wall-clock at entry (explicit for testability; the settle
/// DELAY runs on the tokio clock, so paused-clock tests can prove timing).
pub async fn accept_and_prepare(
    deps: &TrainingDeps,
    job_id: u64,
    job: &TrainingJob,
    now_secs: u64,
) -> Result<PreparedTrain, TrainReject> {
    let accepted = accept_session(deps, job_id, job, now_secs).await?;
    prepare_dataset(deps, job_id, job, &accepted, now_secs, None).await
}

/// Load + type the node-side training template from T1.4's pinned JSON,
/// enforcing the SAME numeric authoring rule the sidecar enforces (CONTRACT
/// v1.0.1: no floats, no >u64 ints — the twins' canonicalisers provably
/// diverge on those) and computing `templateHash` from the canonical bytes
/// (never trusting a stored copy).
pub fn load_training_template(path: &std::path::Path) -> Result<TrainingTemplate, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("training template read failed at {path:?}: {e}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("training template parse: {e}"))?;
    reject_unsafe_numbers(&value, "template")?;

    let canonical = crate::checkpoint::delta::sort_json_keys(&value).to_string();
    let template_hash = {
        use tiny_keccak::{Hasher, Keccak};
        let mut keccak = Keccak::v256();
        let mut out = [0u8; 32];
        keccak.update(canonical.as_bytes());
        keccak.finalize(&mut out);
        format!("0x{}", hex::encode(out))
    };

    let get = |pointer: &str| -> Result<&serde_json::Value, String> {
        value
            .pointer(pointer)
            .ok_or_else(|| format!("training template missing {pointer}"))
    };
    let u64_at = |pointer: &str| -> Result<u64, String> {
        get(pointer)?
            .as_u64()
            .ok_or_else(|| format!("training template {pointer} must be a u64"))
    };
    let u32_list = |pointer: &str| -> Result<Vec<u32>, String> {
        let list = get(pointer)?
            .as_array()
            .ok_or_else(|| format!("training template {pointer} must be a list"))?;
        if list.is_empty() {
            return Err(format!("training template {pointer} must be non-empty"));
        }
        list.iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| format!("training template {pointer} entries must be u32"))
            })
            .collect()
    };

    let lrs = match value.pointer("/method/lrs") {
        None => None,
        Some(serde_json::Value::Array(list)) => Some(
            list.iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| "template method.lrs entries must be strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Some(_) => return Err("template method.lrs must be a list when present".to_string()),
    };

    Ok(TrainingTemplate {
        template_id: get("/templateId")?
            .as_str()
            .ok_or("templateId must be a string")?
            .to_string(),
        template_hash,
        tokenizer_sha256: get("/base/tokenizerSha256")?
            .as_str()
            .ok_or("base.tokenizerSha256 must be a string")?
            .to_string(),
        base_serving_model_id: {
            let raw = get("/base/baseServingModelId")?
                .as_str()
                .ok_or("base.baseServingModelId must be a string")?;
            let hex = raw.strip_prefix("0x").unwrap_or(raw);
            if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!(
                    "base.baseServingModelId {raw:?} is not a 0x-prefixed bytes32"
                ));
            }
            // Round-2 F-G: `query_session_model` returns all-zero for a
            // NON-model session, so an all-zero pin would make E.2's base
            // check pass for every such session — a silent fail-open inside
            // the very gate that exists to stop an adapter reaching the
            // wrong base.
            if hex.chars().all(|c| c == '0') {
                return Err(
                    "base.baseServingModelId is all-zero, which is what the chain returns                      for a non-model session — that would make the serve-back base check                      pass for every one of them"
                        .to_string(),
                );
            }
            format!("0x{}", hex.to_ascii_lowercase())
        },
        ranks: u32_list("/method/ranks")?,
        alphas: u32_list("/method/alphas")?,
        seq_lens: u32_list("/method/seqLens")?,
        lrs,
        max_epochs: u32::try_from(u64_at("/bounds/maxEpochs")?)
            .map_err(|_| "bounds.maxEpochs out of u32".to_string())?,
        max_total_tokens: {
            let value = u64_at("/bounds/maxTotalTokens")?;
            if value == 0 || value > crate::training::schedule::MAX_SCHEDULABLE_TOKENS {
                return Err(format!(
                    "bounds.maxTotalTokens {value} outside 1..={} (schedule ceiling)",
                    crate::training::schedule::MAX_SCHEDULABLE_TOKENS
                ));
            }
            value
        },
        slice_tokens: {
            let value = u64_at("/sliceTokens")?;
            if value == 0 {
                return Err("sliceTokens must be >= 1 (the B.1 pin)".to_string());
            }
            value
        },
    })
}

/// The template numeric authoring rule (CONTRACT v1.0.1), node-side twin of
/// the sidecar's startup check.
fn reject_unsafe_numbers(value: &serde_json::Value, at: &str) -> Result<(), String> {
    match value {
        serde_json::Value::Number(number) => {
            if number.as_u64().is_none() {
                return Err(format!(
                    "template numeric rule violated at {at}: {number} (floats and \
                     out-of-u64 integers are banned — decimal knobs are strings)"
                ));
            }
            Ok(())
        }
        serde_json::Value::Object(map) => {
            for (key, inner) in map {
                reject_unsafe_numbers(inner, &format!("{at}.{key}"))?;
            }
            Ok(())
        }
        serde_json::Value::Array(list) => {
            for (index, inner) in list.iter().enumerate() {
                reject_unsafe_numbers(inner, &format!("{at}[{index}]"))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// T4.e — the slice loop (interface B.2: the checkpoint IS the settlement
// slice; delivery-before-settlement; forfeits bill; cancel at slice
// boundary). Runs on the accepted task after `prepare_dataset`; the caller
// (dispatch, T4.f) serialises RunProgress into the wire frames and owns the
// end-of-run `completeSessionJob` after the dispute wait.
// ---------------------------------------------------------------------------

use crate::training::artifact::{
    upload_artifact_manifest, upload_file_sharded, ArtifactManifestRef,
};
use crate::training::attestation::{
    build_slice_attestation, upload_slice_attestation, SliceAttestationInputs,
};
use crate::training::tracker::TrainRunInfo;
use crate::training::trainer_client::{TrainStream, TrainStreamEvent};

/// Progress the dispatch layer serialises to `train_progress` frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunProgress {
    Stage {
        stage: &'static str,
    },
    /// Delivery-before-settlement: the checkpoint pointer BEFORE its proof.
    Uploading {
        slice_index: u64,
        manifest_cid: String,
        manifest_sha256: String,
        size_bytes: u64,
    },
    SliceSettled {
        index: u64,
        step_from: u64,
        step_to: u64,
        tokens_delta: u64,
        cumulative_tokens: u64,
        checkpoint: ArtifactManifestRef,
        proof_cid: String,
        submitted: bool,
    },
    /// The adapter pointer BEFORE the final proof.
    FinalisingAdapter {
        manifest_cid: String,
        manifest_sha256: String,
    },
}

/// How a run ended (the dispatch layer maps these to
/// `train_complete`/`train_error` + the completion path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    Complete {
        adapter: ArtifactManifestRef,
        billing: TrainRunEndBilling,
        proof_cids: Vec<String>,
        warnings: Vec<String>,
    },
    Failed {
        code: &'static str,
        detail: String,
        billing: TrainRunEndBilling,
        last_checkpoint: Option<ArtifactManifestRef>,
    },
    Cancelled {
        billing: TrainRunEndBilling,
        last_checkpoint: Option<ArtifactManifestRef>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrainRunEndBilling {
    pub settled_slices: u32,
    pub forfeited_slices: u32,
    /// The WIRE bill: Σ executed deltas (forfeits included, B.2/C.1).
    pub billed_tokens: u64,
    /// On-chain truth: Σ landed deltas.
    pub settled_tokens: u64,
}

fn billing_from(info: Option<TrainRunInfo>) -> TrainRunEndBilling {
    match info {
        Some(info) => TrainRunEndBilling {
            settled_slices: info.slices_submitted,
            forfeited_slices: info.slices_forfeited,
            billed_tokens: info.billed_tokens,
            settled_tokens: info.settled_tokens,
        },
        None => TrainRunEndBilling::default(),
    }
}

/// Bounded proof submission (interface B.2): 3 attempts; a "Too many"
/// rate-limit revert waits ≈ `tokens / rate_limit` (+5 s buffer) between
/// attempts (recomputed from THIS model's 10,000 rate, not LTX's 2,000);
/// other errors retry after 2 s. Exhaustion = conservative forfeit (the
/// artifacts are already delivered — revenue only).
pub async fn submit_proof_with_retry(
    deps: &TrainingDeps,
    job_id: u64,
    tokens: u64,
    proof_hash: [u8; 32],
    proof_cid: &str,
) -> bool {
    for attempt in 1..=3u32 {
        match deps
            .proof
            .submit_ltx_proof(job_id, tokens, proof_hash, proof_cid.to_string())
            .await
        {
            Ok(_) => return true,
            Err(error) => {
                let text = error.to_string();
                // ONLY the "Too many" rate-limit REVERT retries (provably
                // nothing landed). Any other error — including an
                // unconfirmed tx that may still mine — forfeits
                // CONSERVATIVELY at once: a retry there can double-claim
                // the same delta on-chain (T4 converge round, the LTX law).
                if text.contains("Too many") && attempt < 3 {
                    let wait =
                        Duration::from_secs(tokens / deps.rate_limit_tokens_per_sec.max(1) + 5);
                    tracing::warn!(
                        "training proof submit for job {job_id} rate-limited (attempt {attempt}/3); retrying in {wait:?}"
                    );
                    tokio::time::sleep(wait).await;
                    continue;
                }
                tracing::error!(
                    "training proof submit for job {job_id} failed ({text}) — forfeiting the slice's revenue (artifacts delivered)"
                );
                return false;
            }
        }
    }
    false
}

/// Join a sidecar-supplied relative path onto the work root, refusing
/// absolute paths and `..`/root components (converge round F10 — everything
/// else in this pipeline fails closed on divergence; path traversal must
/// too).
fn safe_work_path(work_root: &std::path::Path, rel: &str) -> Result<PathBuf, String> {
    let path = std::path::Path::new(rel);
    let mut clean = work_root.to_path_buf();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => clean.push(part),
            other => {
                return Err(format!(
                    "sidecar path {rel:?} carries a non-normal component {other:?} — refused"
                ))
            }
        }
    }
    Ok(clean)
}

struct SliceArtifacts {
    manifest: ArtifactManifestRef,
    step_from: u64,
    step_to: u64,
}

/// Upload one checkpoint dir's files (sharded, fresh keys) + its manifest.
async fn upload_checkpoint(
    deps: &TrainingDeps,
    job_id: u64,
    kind: &str,
    slice_index: Option<u64>,
    dir: &str,
    files: &[crate::training::trainer_client::SliceFileRef],
) -> Result<ArtifactManifestRef, String> {
    let prefix = format!("home/training/job_{job_id}");
    let mut entries = Vec::with_capacity(files.len());
    for file in files {
        let path = safe_work_path(&deps.work_root, &file.rel_path)?;
        let bytes = tokio::fs::read(&path)
            .await
            // Round-7 F-R7-3: `path` is the absolute TRAINING_WORK_ROOT join.
            .map_err(|e| crate::training::redact::opaque("checkpoint file read failed", e))?;
        entries.push(
            upload_file_sharded(deps.artifact_store.as_ref(), &prefix, &file.name, &bytes).await?,
        );
    }
    let _ = dir;
    upload_artifact_manifest(
        deps.artifact_store.as_ref(),
        &prefix,
        kind,
        slice_index,
        &entries,
    )
    .await
}

/// Settle one slice: attestation (B.3/B.5) → upload (proofCID) → bounded
/// submit → tracker. `extras` carries the FINAL slice's adapter hash +
/// moderation. Returns (proof_cid, submitted).
#[allow(clippy::too_many_arguments)]
async fn settle_slice(
    deps: &TrainingDeps,
    job_id: u64,
    job: &TrainingJob,
    prepared: &PreparedTrain,
    slice_index: u64,
    artifacts: &SliceArtifacts,
    cumulative_tokens: u64,
    extras: Option<(&ArtifactManifestRef, &str, &str)>,
) -> Result<(String, bool), String> {
    let tokens_delta = prepared.schedule[slice_index as usize];
    let inputs = SliceAttestationInputs {
        job,
        model_id: format!("0x{}", hex::encode(deps.model_id)),
        template_hash: deps.template.template_hash.clone(),
        env_hash: deps.env_hash.clone(),
        slice_index,
        step_from: artifacts.step_from,
        step_to: artifacts.step_to,
        tokens_delta,
        cumulative_tokens,
        checkpoint_manifest_sha256: artifacts.manifest.manifest_sha256.clone(),
        adapter_manifest_sha256: extras.map(|(adapter, _, _)| adapter.manifest_sha256.clone()),
        moderation: extras.map(|(_, status, policy)| (status.to_string(), policy.to_string())),
        session_id: job_id,
        host: format!("{:?}", deps.host_address),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default(),
    };
    let (_, stored) =
        build_slice_attestation(&inputs, &deps.node_key).map_err(|e| e.to_string())?;
    let s5_path = format!("home/training/job_{job_id}_slice_{slice_index}_attestation.json");
    let (proof_cid, proof_hash) =
        upload_slice_attestation(deps.artifact_store.as_ref(), &s5_path, stored)
            .await
            .map_err(|e| e.to_string())?;

    if !deps
        .tracker
        .mark_slice_pending(job_id, deps.completing_latch)
        .await
    {
        // Completion latched (a completion is dispatching): conservative
        // forfeit — and the executed slice still BILLS (the §B triple;
        // converge round F9 caught this branch skipping the forfeit mark).
        deps.tracker
            .mark_slice_forfeited(job_id, tokens_delta)
            .await;
        return Ok((proof_cid, false));
    }
    let submitted =
        submit_proof_with_retry(deps, job_id, tokens_delta, proof_hash, &proof_cid).await;
    if submitted {
        deps.tracker
            .mark_slice_submitted(job_id, tokens_delta)
            .await;
    } else {
        deps.tracker
            .mark_slice_forfeited(job_id, tokens_delta)
            .await;
    }
    Ok((proof_cid, submitted))
}

/// The k-split (§3.7): a died/failed stream with landed slices is
/// `TRAIN_FAILED`; with none it is the `SIDECAR_UNAVAILABLE` class — and the
/// k = 0 case runs the consuming zero-settle (C.3).
async fn end_failed(
    deps: &TrainingDeps,
    job_id: u64,
    snapshot: &SessionSnapshot,
    now_secs: u64,
    detail: String,
    last_checkpoint: Option<ArtifactManifestRef>,
) -> RunEnd {
    let billing = billing_from(deps.tracker.info(job_id).await);
    let code = if billing.settled_slices > 0 {
        "TRAIN_FAILED"
    } else {
        terminal_reject_effects(deps, job_id, snapshot, now_secs, true).await;
        SIDECAR_UNAVAILABLE
    };
    RunEnd::Failed {
        code,
        detail,
        billing,
        last_checkpoint,
    }
}

/// Drive one accepted run over the sidecar stream to its end (B.2 per-slice
/// law). `cancel` is polled at slice boundaries (interface: cancel/
/// disconnect/write-timeout all mean "abort at the next slice boundary,
/// settle completed slices").
#[allow(clippy::too_many_arguments)]
pub async fn run_training_session(
    deps: &TrainingDeps,
    job_id: u64,
    job: &TrainingJob,
    prepared: &PreparedTrain,
    accepted: &AcceptedSession,
    mut stream: TrainStream,
    progress: tokio::sync::mpsc::Sender<RunProgress>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    now_secs: u64,
) -> RunEnd {
    use std::sync::atomic::Ordering;

    let total_slices = prepared.schedule.len() as u64;
    let mut cumulative: u64 = 0;
    let mut slices_seen: u64 = 0;
    let mut last_checkpoint: Option<ArtifactManifestRef> = None;
    let mut proof_cids: Vec<String> = Vec::new();
    let mut pending_final: Option<(u64, SliceArtifacts)> = None;

    loop {
        // Cancel/disconnect: abort at the SLICE BOUNDARY (before the next
        // stream read); already-settled slices stand, the stream drop fires
        // the sidecar's §4 abort.
        if cancel.load(Ordering::SeqCst) {
            return RunEnd::Cancelled {
                billing: billing_from(deps.tracker.info(job_id).await),
                last_checkpoint,
            };
        }
        match stream.next_event().await {
            Ok(Some(TrainStreamEvent::Tick { stage, .. })) => {
                // CONTRACT §3.5: `loading` maps to wire `training`; the
                // other three map 1:1 (converge round F8).
                let wire_stage = match stage.as_str() {
                    "checkpointing" => "checkpointing",
                    "finalising" => "finalising",
                    _ => "training",
                };
                let _ = progress
                    .send(RunProgress::Stage { stage: wire_stage })
                    .await;
            }
            Ok(Some(TrainStreamEvent::Slice {
                index,
                step_from,
                step_to,
                dir,
                files,
            })) => {
                // A slice ARRIVAL is a boundary too: a cancel set while the
                // sidecar trained this slice aborts here — the slice stays
                // unsettled (conservative; the stream drop fires §4).
                if cancel.load(Ordering::SeqCst) {
                    return RunEnd::Cancelled {
                        billing: billing_from(deps.tracker.info(job_id).await),
                        last_checkpoint,
                    };
                }
                // A diverged sidecar's index is a handled failure, never a
                // panic or a double-bill: it must be EXACTLY the next
                // sequential slice (converge round: schedule[index] panicked
                // the permit-holding task; a duplicate re-settled a delta).
                if index != slices_seen || index >= total_slices {
                    return end_failed(
                        deps,
                        job_id,
                        &accepted.snapshot,
                        now_secs,
                        format!(
                            "diverged sidecar: slice index {index} (expected {slices_seen} of {total_slices})"
                        ),
                        last_checkpoint,
                    )
                    .await;
                }
                slices_seen += 1;
                let _ = progress
                    .send(RunProgress::Stage {
                        stage: "checkpointing",
                    })
                    .await;
                let manifest =
                    match upload_checkpoint(deps, job_id, "checkpoint", Some(index), &dir, &files)
                        .await
                    {
                        Ok(manifest) => manifest,
                        Err(e) => {
                            return end_failed(
                                deps,
                                job_id,
                                &accepted.snapshot,
                                now_secs,
                                crate::training::redact::opaque("checkpoint upload failed", e),
                                last_checkpoint,
                            )
                            .await
                        }
                    };
                // Delivery-before-settlement (B.2): the pointer frame FIRST.
                let _ = progress
                    .send(RunProgress::Uploading {
                        slice_index: index,
                        manifest_cid: manifest.manifest_cid.clone(),
                        manifest_sha256: manifest.manifest_sha256.clone(),
                        size_bytes: manifest.total_bytes,
                    })
                    .await;
                let artifacts = SliceArtifacts {
                    manifest: manifest.clone(),
                    step_from,
                    step_to,
                };
                cumulative += prepared.schedule[index as usize];
                if index + 1 == total_slices {
                    // The FINAL slice waits for the adapter manifest (its
                    // attestation carries adapterManifestSha256 + moderation).
                    pending_final = Some((cumulative, artifacts));
                } else {
                    match settle_slice(
                        deps, job_id, job, prepared, index, &artifacts, cumulative, None,
                    )
                    .await
                    {
                        Ok((proof_cid, submitted)) => {
                            proof_cids.push(proof_cid.clone());
                            let _ = progress
                                .send(RunProgress::SliceSettled {
                                    index,
                                    step_from,
                                    step_to,
                                    tokens_delta: prepared.schedule[index as usize],
                                    cumulative_tokens: cumulative,
                                    checkpoint: manifest.clone(),
                                    proof_cid,
                                    submitted,
                                })
                                .await;
                        }
                        Err(e) => {
                            return end_failed(
                                deps,
                                job_id,
                                &accepted.snapshot,
                                now_secs,
                                crate::training::redact::opaque("slice settle failed", e),
                                Some(manifest),
                            )
                            .await
                        }
                    }
                }
                last_checkpoint = Some(manifest);
                // §5 consumption signal: delete the slice dir AFTER upload.
                if let Ok(slice_dir) = safe_work_path(&deps.work_root, &dir) {
                    let _ = tokio::fs::remove_dir_all(&slice_dir).await;
                }
            }
            Ok(Some(TrainStreamEvent::Finalise {
                dir,
                files,
                warnings,
            })) => {
                let _ = progress
                    .send(RunProgress::Stage {
                        stage: "finalising",
                    })
                    .await;
                let adapter =
                    match upload_checkpoint(deps, job_id, "adapter", None, &dir, &files).await {
                        Ok(adapter) => adapter,
                        Err(e) => {
                            return end_failed(
                                deps,
                                job_id,
                                &accepted.snapshot,
                                now_secs,
                                crate::training::redact::opaque("adapter upload failed", e),
                                last_checkpoint,
                            )
                            .await
                        }
                    };
                let _ = progress
                    .send(RunProgress::FinalisingAdapter {
                        manifest_cid: adapter.manifest_cid.clone(),
                        manifest_sha256: adapter.manifest_sha256.clone(),
                    })
                    .await;
                let Some((final_cumulative, artifacts)) = pending_final.take() else {
                    return end_failed(
                        deps,
                        job_id,
                        &accepted.snapshot,
                        now_secs,
                        "finalise before the final slice event".to_string(),
                        last_checkpoint,
                    )
                    .await;
                };
                let final_index = total_slices - 1;
                match settle_slice(
                    deps,
                    job_id,
                    job,
                    prepared,
                    final_index,
                    &artifacts,
                    final_cumulative,
                    Some((
                        &adapter,
                        prepared.verdict.as_str(),
                        prepared.policy_version.as_str(),
                    )),
                )
                .await
                {
                    Ok((proof_cid, submitted)) => {
                        proof_cids.push(proof_cid.clone());
                        let _ = progress
                            .send(RunProgress::SliceSettled {
                                index: final_index,
                                step_from: artifacts.step_from,
                                step_to: artifacts.step_to,
                                tokens_delta: prepared.schedule[final_index as usize],
                                cumulative_tokens: final_cumulative,
                                checkpoint: artifacts.manifest.clone(),
                                proof_cid,
                                submitted,
                            })
                            .await;
                    }
                    Err(e) => {
                        return end_failed(
                            deps,
                            job_id,
                            &accepted.snapshot,
                            now_secs,
                            crate::training::redact::opaque("final slice settle failed", e),
                            Some(adapter),
                        )
                        .await
                    }
                }
                // The adapter dir is consumed too (§5 tail).
                if let Ok(adapter_dir) = safe_work_path(&deps.work_root, &dir) {
                    let _ = tokio::fs::remove_dir_all(adapter_dir).await;
                }
                // Await the sidecar's `done`, then finish.
                match stream.next_event().await {
                    Ok(Some(TrainStreamEvent::Done)) | Ok(None) => {}
                    other => {
                        tracing::warn!("post-finalise stream anomaly for job {job_id}: {other:?}");
                    }
                }
                // TD15: nothing of this job remains under the work root.
                let _ =
                    tokio::fs::remove_dir_all(deps.work_root.join(format!("job-{job_id}"))).await;
                return RunEnd::Complete {
                    adapter,
                    billing: billing_from(deps.tracker.info(job_id).await),
                    proof_cids,
                    warnings,
                };
            }
            Ok(Some(TrainStreamEvent::Done)) => {
                // `done` without finalise = a diverged sidecar.
                return end_failed(
                    deps,
                    job_id,
                    &accepted.snapshot,
                    now_secs,
                    "stream finished without a finalise event".to_string(),
                    last_checkpoint,
                )
                .await;
            }
            Ok(None) => {
                return end_failed(
                    deps,
                    job_id,
                    &accepted.snapshot,
                    now_secs,
                    "stream ended early".to_string(),
                    last_checkpoint,
                )
                .await;
            }
            Err(SidecarFailure::Envelope { kind, detail, .. }) => {
                // The in-band terminal (sidecar TRAIN_FAILURE).
                return end_failed(
                    deps,
                    job_id,
                    &accepted.snapshot,
                    now_secs,
                    crate::training::redact::opaque(
                        &format!(
                        "sidecar terminal {}",
                        crate::training::redact::echo(&kind)
                    ),
                        detail,
                    ),
                    last_checkpoint,
                )
                .await;
            }
            Err(SidecarFailure::Transport(detail)) => {
                return end_failed(
                    deps,
                    job_id,
                    &accepted.snapshot,
                    now_secs,
                    // Round-8 F-R8-1: this arm echoed the transport detail
                    // verbatim three lines below an Envelope arm that already
                    // redacted it. `Transport` carries the sidecar's raw
                    // stream line, which for a framework traceback is this
                    // node's absolute install paths.
                    crate::training::redact::opaque("sidecar stream failed", detail),
                    last_checkpoint,
                )
                .await;
            }
        }
    }
}
