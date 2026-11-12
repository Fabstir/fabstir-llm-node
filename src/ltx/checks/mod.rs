// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Preventive content checks on the video path (IMPLEMENTATION-CONTENT-CHECKS-LTX.md). Three checks inside the job's
//! slot, all fail closed (D1): the prompt, every input, and the result before it is marked, uploaded or billed.
//!
//! The two token types are the compile-time half of D12: `ComfyClient::upload_input*` takes a [`CheckedInput`] and
//! `finalise_output` takes a [`ResultChecked`]. Their fields are private to this module, so only the check step
//! mints them; with `LTX_CONTENT_CHECKS=off` the same methods mint them without running any check code.
//!
//! Nothing here logs prompt text, picture data or a staged path (D14): log lines, alerts and the refusals file
//! carry derived figures and identifiers only. What the CLIENT is told is one fixed sentence per code: which check
//! or rule ended a job is in the log line and the refusals record, never on the wire (D10).

pub mod alerts;
pub mod client;
pub mod config;
pub mod cooldown;
pub mod deep_png;
pub mod files;
pub mod seal;
pub mod staging;

use self::alerts::AlertSink;
use self::client::{CallError, Outcome, ServiceClient, Verdict};
use self::config::{ChecksConfig, Mode, RendererDirs};
use self::files::{InUse, JobFiles};
use self::staging::{StageError, Staged, Staging};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// D10: final. The user is never told which check or rule fired.
pub const CONTENT_REFUSED: &str = "CONTENT_REFUSED";
/// D10: try later.
pub const CHECK_UNAVAILABLE: &str = "CHECK_UNAVAILABLE";

/// The one sentence a content refusal carries, whatever the check, the rule or the stage.
pub const MSG_REFUSED: &str = "stopped by the automated content check";
/// The one sentence for a check that produced no verdict after accept.
pub const MSG_UNAVAILABLE: &str =
    "the content check could not be completed; nothing was charged, try again later";
/// An item the check cannot read (input-caused; `VALIDATION_FAILED`).
pub const MSG_UNCHECKABLE: &str = "this input could not be checked (unsupported or damaged file)";
/// A prompt over the text check's limit (`VALIDATION_FAILED`).
pub const MSG_PROMPT_TOO_LONG: &str =
    "the prompt is too long for the content check (over 8000 characters)";
/// D13's wording for a paused address (before accept).
pub const MSG_PAUSED: &str = "paused after repeated refusals";
/// Before accept, while the breaker is open.
pub const MSG_BREAKER: &str = "content checks are paused: the check service is not ready";
/// Before accept, once `shadow` has run past its end date.
pub const MSG_SHADOW_ENDED: &str = "content checks: the shadow period has ended on this host";

/// The benign sentence the probe sends to the text check (it must pass).
pub const PROBE_TEXT: &str = "a calm lake at sunrise, gentle mist over the water";

/// D11: the seal and the refusals record have bounds of their own.
const SEAL_BOUND: Duration = Duration::from_secs(60);
const RECORD_BOUND: Duration = Duration::from_secs(10);
/// A per-job failure whose probe passed alerts at most this often per kind (fix F2).
const FAILURE_ALERT_EVERY: Duration = Duration::from_secs(300);
/// Fix F7: this many result-stage refusals inside an hour, node-wide, alert the operator.
pub const RESULT_REFUSALS_ALERT: usize = 5;
const RESULT_REFUSALS_WINDOW: Duration = Duration::from_secs(3600);

/// A refused job's wire answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

/// Proof that an input went through the check step. Only this module can build one.
#[derive(Debug)]
pub struct CheckedInput {
    _minted: (),
}

/// Proof that a job's result went through the check step. Only this module can build one.
#[derive(Debug)]
pub struct ResultChecked {
    _minted: (),
}

/// Which check. The tag appears in log lines, alerts and the refusals record, never in what the client is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Prompt,
    Still,
    Clip,
    DeepFrame,
    Result,
    Probe,
}

impl Stage {
    pub fn tag(self) -> &'static str {
        match self {
            Stage::Prompt => "c1",
            Stage::Still => "c2",
            Stage::Clip => "c3",
            Stage::DeepFrame => "c4",
            Stage::Result => "c5",
            Stage::Probe => "c0",
        }
    }
}

/// Why a check produced no verdict (logged and alerted; the client sees [`MSG_UNAVAILABLE`] for all of them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Unreachable,
    Timeout,
    Unrecognised,
    StagingFull,
    StagingFailed,
    BudgetUsed,
    NotVideo,
}

/// Identifiers that reach log lines, alerts, records and map keys are cut to `[A-Za-z0-9_-]`, 64 characters (fix
/// F7): a session id is the client's own string.
pub fn safe_id(raw: &str) -> String {
    raw.chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// One job's identifiers and the check time it has used.
#[derive(Debug, Clone)]
pub struct JobChecks {
    /// Sanitised ([`safe_id`]).
    pub session_id: String,
    pub job_id: Option<u64>,
    pub client: Option<String>,
    /// Time used from the budget the prompt and the inputs share (D11: 300 s). The result check has a limit of
    /// its own (fix F1).
    pub spent: Duration,
    /// All the time this job has spent inside its checks. The handler takes it out of the deadlines that are
    /// about rendering and delivery (fix F5).
    pub total: Duration,
    /// The deep-input frames this job judges, drawn once per job (fix F6).
    deep_sample: Option<(usize, u32, BTreeSet<usize>)>,
}

impl JobChecks {
    fn ids(&self) -> String {
        format!(
            "session={} job={}",
            self.session_id,
            self.job_id
                .map_or_else(|| "none".to_string(), |j| j.to_string())
        )
    }
    /// The cool-down key: the authenticated client address, else the session.
    fn key(&self) -> String {
        cooldown_key(self.client.as_deref(), &self.session_id)
    }
    /// Whether frame `index` of a `total`-frame deep input is one this job judges.
    fn deep_sampled(&mut self, total: usize, fps: u32, index: usize) -> bool {
        let fresh = !matches!(&self.deep_sample, Some((t, f, _)) if *t == total && *f == fps);
        if fresh {
            let set = deep_png::sampled_indices(total, fps, &mut rand::rngs::OsRng);
            self.deep_sample = Some((total, fps, set));
        }
        self.deep_sample
            .as_ref()
            .is_some_and(|(_, _, set)| set.contains(&index))
    }
}

fn cooldown_key(client: Option<&str>, session_id: &str) -> String {
    match client {
        Some(a) if !a.is_empty() => safe_id(a).to_ascii_lowercase(),
        _ => format!("session:{}", safe_id(session_id)),
    }
}

enum Item<'a> {
    Text(&'a str),
    File { bytes: &'a [u8], ext: &'static str },
}

impl Item<'_> {
    fn bytes(&self) -> &[u8] {
        match self {
            Item::Text(t) => t.as_bytes(),
            Item::File { bytes, .. } => bytes,
        }
    }
    fn ext(&self) -> &'static str {
        match self {
            Item::Text(_) => "txt",
            Item::File { ext, .. } => ext,
        }
    }
}

enum Fail {
    Call(CallError),
    Stage(StageError),
}

struct Inner {
    cfg: ChecksConfig,
    service: Option<ServiceClient>,
    staging: Option<Staging>,
    alerts: Arc<dyn AlertSink>,
    cooldown: cooldown::Cooldown,
    /// `Some(reason)` = open: video jobs are rejected before accept (D11).
    breaker: Mutex<Option<String>>,
    recovering: AtomicBool,
    /// Service-class failures in a row, across jobs; any real verdict resets it.
    failures: AtomicUsize,
    /// Set by a trip on consecutive failures: the breaker stays open until then, whatever the probe says.
    hold_until: Mutex<Option<Instant>>,
    /// When each throttled alert kind last went out.
    throttle: Mutex<HashMap<String, Instant>>,
    /// Result-stage refusals of the last hour, node-wide (fix F7).
    result_refusals: Mutex<VecDeque<Instant>>,
    in_use: Arc<InUse>,
}

/// The node's content checks. Cheap to clone; one per server.
#[derive(Clone)]
pub struct ContentChecks {
    inner: Arc<Inner>,
}

impl ContentChecks {
    /// `LTX_CONTENT_CHECKS=off`: no check code runs and the tokens are minted as they are asked for.
    pub fn off() -> Self {
        Self::new(ChecksConfig::off(), Arc::new(alerts::LogAlerts))
    }

    /// Build from settings. In `shadow` and `enforce` the breaker starts OPEN: nothing is accepted in `enforce`
    /// until [`Self::start`] (or [`Self::probe_now`]) has seen the service healthy, on the pinned bundle, and
    /// passing the built-in probe.
    pub fn new(cfg: ChecksConfig, alerts: Arc<dyn AlertSink>) -> Self {
        Self::with_staging(cfg, alerts, |s| s)
    }

    /// [`Self::new`] with the staging area adjusted (tests: the slow-write seam).
    #[doc(hidden)]
    pub fn with_staging(
        cfg: ChecksConfig,
        alerts: Arc<dyn AlertSink>,
        adjust: impl FnOnce(Staging) -> Staging,
    ) -> Self {
        let service = cfg.socket.as_deref().map(ServiceClient::new);
        let staging = match (&cfg.staging_node, &cfg.staging_service) {
            (Some(n), Some(s)) => Some(adjust(Staging::new(n, s, cfg.staging_cap_bytes))),
            _ => None,
        };
        let breaker = (cfg.mode != Mode::Off).then(|| "starting".to_string());
        ContentChecks {
            inner: Arc::new(Inner {
                cfg,
                service,
                staging,
                alerts,
                cooldown: cooldown::Cooldown::default(),
                breaker: Mutex::new(breaker),
                recovering: AtomicBool::new(false),
                failures: AtomicUsize::new(0),
                hold_until: Mutex::new(None),
                throttle: Mutex::new(HashMap::new()),
                result_refusals: Mutex::new(VecDeque::new()),
                in_use: Arc::new(InUse::default()),
            }),
        }
    }

    pub fn mode(&self) -> Mode {
        self.inner.cfg.mode
    }

    pub fn config(&self) -> &ChecksConfig {
        &self.inner.cfg
    }

    pub fn cooldown(&self) -> &cooldown::Cooldown {
        &self.inner.cooldown
    }

    pub fn breaker_open(&self) -> bool {
        self.inner
            .breaker
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(true)
    }

    /// The longest a job's checks can take before it reaches signing: the shared budget, the result check's own
    /// limit, and the seal and record bounds. The handler adds it to the signer reservation (fix F5). Zero when
    /// checks are `off`.
    pub fn max_check_secs(&self) -> u64 {
        if self.mode() == Mode::Off {
            return 0;
        }
        (self.inner.cfg.job_budget + self.inner.cfg.check_timeout + SEAL_BOUND + RECORD_BOUND)
            .as_secs()
    }

    /// Mode, pinned bundle hash and breaker state for the version endpoint (D12).
    pub fn status_json(&self) -> serde_json::Value {
        let cfg = &self.inner.cfg;
        let mut v = serde_json::json!({
            "mode": cfg.mode.as_str(),
            "bundleHash": cfg.bundle_hash.as_ref().map(|h| format!("sha256:{h}")),
            "breaker": if cfg.mode != Mode::Off && self.breaker_open() { "open" } else { "closed" },
        });
        if let Some(d) = cfg.shadow_until {
            v["shadowUntil"] = serde_json::json!(d.to_string());
        }
        v
    }

    /// A job's check context.
    pub fn job(&self, session_id: &str, job_id: Option<u64>, client: Option<&str>) -> JobChecks {
        JobChecks {
            session_id: safe_id(session_id),
            job_id,
            client: client.map(safe_id),
            spent: Duration::ZERO,
            total: Duration::ZERO,
            deep_sample: None,
        }
    }

    /// Send `kind` at most once per `every` (the first always goes out).
    fn alert_limited(&self, kind: &str, detail: &str, every: Duration) {
        let due = match self.inner.throttle.lock() {
            Ok(mut g) => {
                let due = g.get(kind).is_none_or(|t| t.elapsed() >= every);
                if due {
                    g.insert(kind.to_string(), Instant::now());
                }
                due
            }
            Err(_) => false,
        };
        if due {
            self.inner.alerts.alert(kind, detail);
        }
    }

    // ------------------------------------------------------------------ before accept (D2): in-memory only

    /// The instant gates, run just before the generation slot is taken. No await, no I/O.
    pub fn pre_accept(&self, client: Option<&str>, session_id: &str) -> Result<(), Refusal> {
        let cfg = &self.inner.cfg;
        let unavailable = |message: &str| Refusal {
            code: CHECK_UNAVAILABLE,
            message: message.to_string(),
        };
        match cfg.mode {
            Mode::Off => Ok(()),
            Mode::Shadow => {
                if cfg.shadow_expired(chrono::Utc::now().date_naive()) {
                    // an operator who forgot the end date hears of it, once an hour while jobs keep arriving
                    self.alert_limited(
                        "shadow-ended",
                        "video jobs are being rejected: the shadow period is over",
                        Duration::from_secs(3600),
                    );
                    return Err(unavailable(MSG_SHADOW_ENDED));
                }
                Ok(())
            }
            Mode::Enforce => {
                if self.breaker_open() {
                    return Err(unavailable(MSG_BREAKER));
                }
                if self
                    .inner
                    .cooldown
                    .paused(&cooldown_key(client, session_id))
                {
                    return Err(unavailable(MSG_PAUSED));
                }
                Ok(())
            }
        }
    }

    // ------------------------------------------------------------------ the three checks

    /// D7: the prompt, first step after the session gate. Fixed prompts of ours and empty prompts are skipped.
    pub async fn check_prompt(
        &self,
        job: &mut JobChecks,
        prompt: &str,
        fixed_prompt: bool,
    ) -> Result<(), Refusal> {
        if self.mode() == Mode::Off || fixed_prompt || prompt.trim().is_empty() {
            return Ok(());
        }
        let began = Instant::now();
        let result = if prompt.chars().count() > client::MAX_TEXT_CHARS {
            // the service would answer TEXT_TOO_LONG: refuse before calling it
            self.uncheckable(job, Stage::Prompt, MSG_PROMPT_TOO_LONG)
        } else {
            self.judge(job, Stage::Prompt, Item::Text(prompt)).await
        };
        job.total += began.elapsed();
        result
    }

    /// D6: a still (png, jpeg or webp), judged before it is uploaded.
    pub async fn check_still(
        &self,
        job: &mut JobChecks,
        bytes: &[u8],
    ) -> Result<CheckedInput, Refusal> {
        if self.mode() == Mode::Off {
            return Ok(CheckedInput { _minted: () });
        }
        let began = Instant::now();
        let result = match sniff_still(bytes) {
            // not a png, jpeg or webp: the service could not read it either
            None => self.uncheckable(job, Stage::Still, MSG_UNCHECKABLE),
            Some(ext) => {
                self.judge(job, Stage::Still, Item::File { bytes, ext })
                    .await
            }
        };
        job.total += began.elapsed();
        result.map(|()| CheckedInput { _minted: () })
    }

    /// D6: a clip (a chained part's previous-part clip included), judged whole before it is uploaded.
    pub async fn check_clip(
        &self,
        job: &mut JobChecks,
        bytes: &[u8],
    ) -> Result<CheckedInput, Refusal> {
        if self.mode() == Mode::Off {
            return Ok(CheckedInput { _minted: () });
        }
        let began = Instant::now();
        let result = self
            .judge(job, Stage::Clip, Item::File { bytes, ext: "mp4" })
            .await;
        job.total += began.elapsed();
        result.map(|()| CheckedInput { _minted: () })
    }

    /// D6: one frame of a deep EXR input. The job's sampled frames (the first, the last, and random ones in
    /// between: at most 64, one per second on average) are converted to an 8-bit PNG and judged as stills before
    /// that frame is uploaded; the others pass through.
    pub async fn check_deep_frame(
        &self,
        job: &mut JobChecks,
        index: usize,
        total: usize,
        fps: u32,
        scene_linear: bool,
        bytes: &[u8],
    ) -> Result<CheckedInput, Refusal> {
        if self.mode() == Mode::Off || !job.deep_sampled(total, fps, index) {
            return Ok(CheckedInput { _minted: () });
        }
        let began = Instant::now();
        let result = self.judge_deep_frame(job, scene_linear, bytes).await;
        job.total += began.elapsed();
        result.map(|()| CheckedInput { _minted: () })
    }

    async fn judge_deep_frame(
        &self,
        job: &mut JobChecks,
        scene_linear: bool,
        bytes: &[u8],
    ) -> Result<(), Refusal> {
        let left = self.inner.cfg.job_budget.saturating_sub(job.spent);
        if left.is_zero() {
            return self
                .no_verdict(job, Stage::DeepFrame, Cause::BudgetUsed)
                .await;
        }
        let started = Instant::now();
        let owned = bytes.to_vec();
        let convert =
            tokio::task::spawn_blocking(move || deep_png::exr_to_png(&owned, scene_linear));
        let converted = tokio::time::timeout(self.inner.cfg.check_timeout.min(left), convert).await;
        job.spent += started.elapsed();
        let png = match converted {
            Ok(Ok(Ok(png))) => png,
            // an EXR this node cannot convert cannot be judged
            Ok(_) => return self.uncheckable(job, Stage::DeepFrame, MSG_UNCHECKABLE),
            // the conversion is this node's own work: slow here says nothing about the service
            Err(_) => {
                return self
                    .no_verdict(job, Stage::DeepFrame, Cause::BudgetUsed)
                    .await
            }
        };
        // what the service is sent is the PNG, never the EXR
        self.judge(
            job,
            Stage::DeepFrame,
            Item::File {
                bytes: &png,
                ext: "png",
            },
        )
        .await
    }

    /// D5: the job's first artefact (the mp4, or the mp4 preview of an EXR job), judged before anything is marked,
    /// uploaded or billed. The token covers the job's remaining outputs, which come from the same pinned graph.
    pub async fn check_result(
        &self,
        job: &mut JobChecks,
        filename: &str,
        bytes: &[u8],
    ) -> Result<ResultChecked, Refusal> {
        if self.mode() == Mode::Off {
            return Ok(ResultChecked { _minted: () });
        }
        let began = Instant::now();
        let result = if filename.to_ascii_lowercase().ends_with(".mp4") {
            self.judge(job, Stage::Result, Item::File { bytes, ext: "mp4" })
                .await
        } else {
            // every kind delivers its mp4 first; anything else here cannot be judged, so it is not delivered
            self.no_verdict(job, Stage::Result, Cause::NotVideo).await
        };
        job.total += began.elapsed();
        result.map(|()| ResultChecked { _minted: () })
    }

    // ------------------------------------------------------------------ the engine

    async fn call(
        &self,
        item: &Item<'_>,
        limit: Duration,
    ) -> Result<(Outcome, Option<Staged>), Fail> {
        let service = self
            .inner
            .service
            .as_ref()
            .ok_or(Fail::Call(CallError::Unreachable))?;
        match item {
            Item::Text(text) => service
                .moderate_text(text, limit)
                .await
                .map(|o| (o, None))
                .map_err(Fail::Call),
            Item::File { bytes, ext } => {
                let staging = self
                    .inner
                    .staging
                    .as_ref()
                    .ok_or_else(|| Fail::Stage(StageError::Io("no staging area".into())))?;
                let staged = staging.stage(bytes, ext).await.map_err(Fail::Stage)?;
                // the service is given the path as IT sees the staging area, never the node's own
                let outcome = service
                    .moderate(staged.service_path(), limit)
                    .await
                    .map_err(Fail::Call)?;
                self.same_bundle(&outcome).map_err(Fail::Call)?;
                Ok((outcome, Some(staged)))
            }
        }
    }

    /// With a pinned bundle, a media answer must name that bundle (fix F13: no hash is not an answer).
    fn same_bundle(&self, outcome: &Outcome) -> Result<(), CallError> {
        match (&self.inner.cfg.bundle_hash, &outcome.bundle_hash) {
            (None, _) => Ok(()),
            (Some(pin), Some(got)) if pin == got => Ok(()),
            (Some(_), Some(_)) => Err(CallError::Unrecognised("another bundle".into())),
            (Some(_), None) => Err(CallError::Unrecognised("no bundle hash".into())),
        }
    }

    /// One bounded check. The prompt and the inputs share the job's budget (D11); the result check, which runs
    /// after the render, always has a full limit of its own (fix F1).
    async fn judge(
        &self,
        job: &mut JobChecks,
        stage: Stage,
        item: Item<'_>,
    ) -> Result<(), Refusal> {
        let cfg = &self.inner.cfg;
        let limit = if stage == Stage::Result {
            cfg.check_timeout
        } else {
            let left = cfg.job_budget.saturating_sub(job.spent);
            if left.is_zero() {
                return self.no_verdict(job, stage, Cause::BudgetUsed).await;
            }
            cfg.check_timeout.min(left)
        };
        let started = Instant::now();
        let answer = tokio::time::timeout(limit, self.call(&item, limit)).await;
        let took = started.elapsed();
        if stage != Stage::Result {
            job.spent += took;
        }
        let cause = match answer {
            Ok(Ok((outcome, staged))) => {
                // a real verdict, pass or block: the service judges real items
                self.inner.failures.store(0, Ordering::SeqCst);
                // D14: one line of derived figures per check
                tracing::info!(
                    "ltx-check {} stage={} verdict={} frames={} top={:.3} ms={} bundle={}",
                    job.ids(),
                    stage.tag(),
                    outcome.word,
                    outcome.frames,
                    outcome.top_score,
                    took.as_millis(),
                    cfg.bundle_hash.as_deref().unwrap_or("unpinned"),
                );
                if outcome.verdict == Verdict::Pass {
                    return Ok(());
                }
                return self.refuse(job, stage, &item, &outcome, staged, took).await;
            }
            Err(_) | Ok(Err(Fail::Call(CallError::Timeout))) => Cause::Timeout,
            Ok(Err(Fail::Call(CallError::Unreachable))) => Cause::Unreachable,
            Ok(Err(Fail::Call(CallError::Input(kind)))) => {
                tracing::warn!(
                    "ltx-check {} stage={} input kind={kind}",
                    job.ids(),
                    stage.tag()
                );
                let message = if kind == "TEXT_TOO_LONG" {
                    MSG_PROMPT_TOO_LONG
                } else {
                    MSG_UNCHECKABLE
                };
                return self.uncheckable(job, stage, message);
            }
            Ok(Err(Fail::Call(CallError::Unrecognised(why)))) => {
                tracing::warn!(
                    "ltx-check {} stage={} unrecognised: {why}",
                    job.ids(),
                    stage.tag()
                );
                Cause::Unrecognised
            }
            Ok(Err(Fail::Stage(StageError::Full))) => Cause::StagingFull,
            Ok(Err(Fail::Stage(StageError::Io(kind)))) => {
                tracing::warn!(
                    "ltx-check {} stage={} staging failed: {kind}",
                    job.ids(),
                    stage.tag()
                );
                Cause::StagingFailed
            }
        };
        self.no_verdict(job, stage, cause).await
    }

    /// A `BLOCK_*` verdict. `enforce`: seal the strongest signal (D15), record (D14), count (D13), refuse (D1).
    /// `shadow` (a soak with test jobs, fix F14): the line in the log and nothing else.
    async fn refuse(
        &self,
        job: &JobChecks,
        stage: Stage,
        item: &Item<'_>,
        outcome: &Outcome,
        staged: Option<Staged>,
        took: Duration,
    ) -> Result<(), Refusal> {
        let cfg = &self.inner.cfg;
        if cfg.mode != Mode::Enforce {
            tracing::warn!(
                "ltx-check {} stage={} refused={} sealed=false shadow=true",
                job.ids(),
                stage.tag(),
                outcome.word
            );
            return Ok(());
        }
        if outcome.unknown_block {
            // refused all the same; but a service blocking under a word this node does not know is misdeployed
            self.inner.alerts.alert(
                "unknown-verdict",
                &format!(
                    "{} stage={} verdict={}",
                    job.ids(),
                    stage.tag(),
                    outcome.word
                ),
            );
        }
        let mut record = serde_json::json!({
            "time": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "session": job.session_id,
            "job": job.job_id,
            "client": job.client,
            "stage": stage.tag(),
            "verdict": outcome.word,
            "category": outcome.category,
            "frames": outcome.frames,
            "topScore": outcome.top_score,
            "ms": took.as_millis() as u64,
            "bundleHash": cfg.bundle_hash,
            "sha256": hex::encode(Sha256::digest(item.bytes())),
        });
        if let Some((term, explicit, minor)) = outcome.text {
            record["text"] =
                serde_json::json!({"termRule": term, "explicit": explicit, "minor": minor});
        }
        let mut sealed = false;
        let mut staged = staged;
        if outcome.verdict == Verdict::BlockIllegal {
            sealed = self.seal_item(job, stage, item, &record).await;
            if sealed {
                self.inner
                    .alerts
                    .alert("sealed", &format!("{} stage={}", job.ids(), stage.tag()));
            } else {
                // fix F8: never delete the only copy of an item that should have been sealed
                let kept = self.keep_unsealed(job, stage, item, staged.take()).await;
                record["unsealed"] = serde_json::json!(kept);
                self.inner.alerts.alert(
                    "seal-failed",
                    &format!("{} stage={} kept={kept}", job.ids(), stage.tag()),
                );
            }
        }
        // D15: the staging copy goes only once the sealed copy is durably on disk
        drop(staged);
        record["sealed"] = serde_json::json!(sealed);
        self.append_refusal(&record).await;
        tracing::warn!(
            "ltx-check {} stage={} refused={} sealed={sealed} shadow=false",
            job.ids(),
            stage.tag(),
            outcome.word,
        );
        let counted = self.inner.cooldown.record(&job.key());
        if counted.alert {
            self.inner.alerts.alert(
                "repeated-refusals",
                &format!(
                    "{} client={} refusals24h={}",
                    job.ids(),
                    job.client.as_deref().unwrap_or("unknown"),
                    counted.count
                ),
            );
        }
        if stage == Stage::Result {
            self.count_result_refusal(job);
        }
        Err(Refusal {
            code: CONTENT_REFUSED,
            message: MSG_REFUSED.to_string(),
        })
    }

    /// Fix F7: the per-address cool-down costs nothing to dodge with a fresh address, and each result-stage
    /// refusal is a full render given away. So these are also counted node-wide, and five in an hour alert the
    /// operator (and every fifth after that). Nobody else is throttled.
    fn count_result_refusal(&self, job: &JobChecks) {
        let now = Instant::now();
        let count = match self.inner.result_refusals.lock() {
            Ok(mut g) => {
                while g
                    .front()
                    .is_some_and(|t| now.duration_since(*t) >= RESULT_REFUSALS_WINDOW)
                {
                    g.pop_front();
                }
                g.push_back(now);
                g.len()
            }
            Err(_) => return,
        };
        if count >= RESULT_REFUSALS_ALERT && count % RESULT_REFUSALS_ALERT == 0 {
            self.inner.alerts.alert(
                "result-refusals",
                &format!("{} resultRefusals1h={count}", job.ids()),
            );
        }
    }

    /// Input-caused (10b as amended): THIS item cannot be checked. Only this job fails: no breaker, no probe, no
    /// cool-down count, no alert (a client could otherwise page the named person with junk files).
    fn uncheckable(
        &self,
        job: &JobChecks,
        stage: Stage,
        message: &'static str,
    ) -> Result<(), Refusal> {
        let enforce = self.inner.cfg.mode == Mode::Enforce;
        tracing::warn!(
            "ltx-check {} stage={} verdict=NONE cause=InputUncheckable shadow={}",
            job.ids(),
            stage.tag(),
            !enforce
        );
        if enforce {
            Err(Refusal {
                code: "VALIDATION_FAILED",
                message: message.to_string(),
            })
        } else {
            Ok(())
        }
    }

    /// No verdict. The job fails with `CHECK_UNAVAILABLE` (in `enforce`); whether the breaker opens is decided by
    /// the service, not by this one job (fix F2/F3): an unreachable socket opens it at once; a timeout, an
    /// unrecognised answer or a staging fault is followed by the benign probe, and only a failed probe opens it.
    /// A used-up budget or an artefact that is not a video is this job's own affair.
    async fn no_verdict(&self, job: &JobChecks, stage: Stage, cause: Cause) -> Result<(), Refusal> {
        let enforce = self.inner.cfg.mode == Mode::Enforce;
        tracing::warn!(
            "ltx-check {} stage={} verdict=NONE cause={cause:?} shadow={}",
            job.ids(),
            stage.tag(),
            !enforce
        );
        let detail = format!("{} stage={} cause={cause:?}", job.ids(), stage.tag());
        // A service that fails real items but passes the tiny probe clip (out of GPU memory on large clips, say)
        // would otherwise take every job's slot and signer reservation for nothing, for ever. So service-class
        // failures in a row are counted across jobs, and the third opens the breaker whatever the probe says.
        let counted = matches!(
            cause,
            Cause::Timeout | Cause::Unrecognised | Cause::StagingFailed
        );
        let in_a_row = if counted {
            self.inner.failures.fetch_add(1, Ordering::SeqCst) + 1
        } else {
            0
        };
        // `opened` = THIS outcome opened the breaker (an alert of its own); otherwise the rate-limited one
        let mut opened = false;
        match cause {
            Cause::Unreachable => {
                opened = self.trip(&format!("{cause:?} at {}", stage.tag()));
            }
            Cause::Timeout | Cause::Unrecognised | Cause::StagingFull | Cause::StagingFailed
                if !self.breaker_open() =>
            {
                if in_a_row >= config::FAILURES_TO_TRIP {
                    self.hold();
                    opened = self.trip(&format!(
                        "{in_a_row} checks in a row without a verdict, the last {cause:?} at {}",
                        stage.tag()
                    ));
                } else if let Err(why) = self.probe_once().await {
                    opened = self.trip(&format!(
                        "{cause:?} at {}, then the probe: {why}",
                        stage.tag()
                    ));
                }
            }
            _ => {}
        }
        if opened {
            self.inner.alerts.alert("check-unavailable", &detail);
        } else {
            // the service answered the probe: one job's trouble, reported without flooding
            self.alert_limited(
                &format!("check-unavailable-{cause:?}"),
                &detail,
                FAILURE_ALERT_EVERY,
            );
        }
        if enforce {
            Err(Refusal {
                code: CHECK_UNAVAILABLE,
                message: MSG_UNAVAILABLE.to_string(),
            })
        } else {
            Ok(())
        }
    }

    fn evidence_name(&self, job: &JobChecks, stage: Stage) -> String {
        format!(
            "{}-{}-{}-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
            job.job_id
                .map_or_else(|| "nojob".to_string(), |j| j.to_string()),
            stage.tag(),
            uuid::Uuid::new_v4().simple()
        )
    }

    /// Seal and write DURABLY (file and directory synced). `true` only once that has happened.
    async fn seal_item(
        &self,
        job: &JobChecks,
        stage: Stage,
        item: &Item<'_>,
        record: &serde_json::Value,
    ) -> bool {
        let (Some(key), Some(dir)) = (
            self.inner.cfg.evidence_pubkey,
            self.inner.cfg.evidence_dir.clone(),
        ) else {
            return false;
        };
        let mut record = record.clone();
        record["ext"] = serde_json::json!(item.ext());
        let bytes = item.bytes().to_vec();
        let name = format!("{}.sealed", self.evidence_name(job, stage));
        let write = tokio::task::spawn_blocking(move || {
            seal::seal(&key, &record, &bytes).and_then(|s| seal::write_sealed(&dir, &name, &s))
        });
        match tokio::time::timeout(SEAL_BOUND, write).await {
            Ok(Ok(Ok(_))) => true,
            Ok(Ok(Err(e))) => {
                tracing::error!("ltx-check {} sealing failed: {e}", job.ids());
                false
            }
            _ => {
                tracing::error!("ltx-check {} sealing did not finish", job.ids());
                false
            }
        }
    }

    /// Sealing failed: keep the item in the owner-only `unsealed/` folder of the evidence directory; if even that
    /// cannot be written, leave the staged copy where it is. Returns where the copy is: `unsealed`, `staging` or
    /// `nowhere` (a prompt with no folder to write to).
    async fn keep_unsealed(
        &self,
        job: &JobChecks,
        stage: Stage,
        item: &Item<'_>,
        staged: Option<Staged>,
    ) -> &'static str {
        if let Some(dir) = self.inner.cfg.evidence_dir.clone() {
            let name = format!("{}.{}", self.evidence_name(job, stage), item.ext());
            let bytes = item.bytes().to_vec();
            let write =
                tokio::task::spawn_blocking(move || seal::write_unsealed(&dir, &name, &bytes));
            if let Ok(Ok(Ok(_))) = tokio::time::timeout(SEAL_BOUND, write).await {
                return "unsealed";
            }
        }
        match staged {
            Some(s) => {
                s.keep();
                "staging"
            }
            None => "nowhere",
        }
    }

    /// D14: one JSON line per refusal (figures and identifiers only). Bounded; a failure is logged, never fatal.
    async fn append_refusal(&self, record: &serde_json::Value) {
        let Some(path) = self.inner.cfg.refusals_file.clone() else {
            return;
        };
        let mut line = record.to_string();
        line.push('\n');
        let write = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(path)?
                .write_all(line.as_bytes())
        });
        match tokio::time::timeout(RECORD_BOUND, write).await {
            Ok(Ok(Ok(()))) => {}
            _ => tracing::error!("ltx-check refusals file could not be written"),
        }
    }

    // ------------------------------------------------------------------ breaker and probe (D11)

    /// Keep the breaker open for the waiting period from now (a trip on consecutive failures).
    fn hold(&self) {
        if let Ok(mut g) = self.inner.hold_until.lock() {
            *g = Some(Instant::now() + self.inner.cfg.breaker_hold);
        }
    }

    /// How much of a waiting period is left.
    fn held_for(&self) -> Option<Duration> {
        let until = (*self.inner.hold_until.lock().ok()?)?;
        let left = until.saturating_duration_since(Instant::now());
        (!left.is_zero()).then_some(left)
    }

    /// Open the breaker. Returns whether this call opened it (it was closed before).
    fn trip(&self, reason: &str) -> bool {
        let newly = match self.inner.breaker.lock() {
            Ok(mut g) => {
                let newly = g.is_none();
                if newly {
                    *g = Some(reason.to_string());
                }
                newly
            }
            Err(_) => false,
        };
        if newly {
            self.inner.alerts.alert("breaker-open", reason);
        }
        self.spawn_recovery();
        newly
    }

    /// Close the breaker, unless a waiting period is still running. Returns whether it is now closed.
    fn close(&self) -> bool {
        if self.held_for().is_some() {
            return false;
        }
        self.inner.failures.store(0, Ordering::SeqCst);
        match self.inner.breaker.lock() {
            Ok(mut g) => {
                *g = None;
                true
            }
            Err(_) => false,
        }
    }

    /// Health, the pinned bundle under the `generation` profile, the built-in benign clip passing through the
    /// staging area, and a benign sentence passing the text check.
    pub async fn probe_once(&self) -> Result<(), String> {
        let cfg = &self.inner.cfg;
        let service = self
            .inner
            .service
            .as_ref()
            .ok_or("no check service configured")?;
        let short = Duration::from_secs(10).min(cfg.check_timeout);
        service
            .health(short)
            .await
            .map_err(|e| format!("health: {e:?}"))?;
        let bundle = service
            .bundle(short)
            .await
            .map_err(|e| format!("bundle: {e:?}"))?;
        if bundle.profile != "generation" {
            return Err("the service is not running the generation profile".to_string());
        }
        if let Some(pin) = &cfg.bundle_hash {
            if *pin != bundle.bundle_hash {
                return Err(
                    "the service bundle hash differs from LTX_CHECKS_BUNDLE_HASH".to_string(),
                );
            }
        }
        let clip = Item::File {
            bytes: crate::ltx::provenance::SELF_TEST_MP4,
            ext: "mp4",
        };
        for (what, item) in [("clip", clip), ("text", Item::Text(PROBE_TEXT))] {
            let answer =
                tokio::time::timeout(cfg.check_timeout, self.call(&item, cfg.check_timeout)).await;
            match answer {
                Ok(Ok((o, _staged))) if o.verdict == Verdict::Pass => {}
                Ok(Ok((o, _staged))) => {
                    return Err(format!("the probe {what} answered {}", o.word))
                }
                Ok(Err(Fail::Call(e))) => return Err(format!("probe {what}: {e:?}")),
                Ok(Err(Fail::Stage(e))) => return Err(format!("probe {what} staging: {e:?}")),
                Err(_) => return Err(format!("probe {what} timed out")),
            }
        }
        Ok(())
    }

    /// Probe once and close the breaker on success. Returns whether the breaker is now closed.
    pub async fn probe_now(&self) -> bool {
        match self.probe_once().await {
            // a passing probe closes it only once any waiting period is over
            Ok(()) => self.close(),
            Err(why) => {
                tracing::warn!("ltx-check probe failed: {why}");
                false
            }
        }
    }

    /// Start-up (D8, D11): sweep the staging area, then probe until the service is ready.
    pub fn start(&self) {
        if self.mode() == Mode::Off {
            return;
        }
        if let Some(s) = &self.inner.staging {
            let n = s.sweep();
            if n > 0 {
                tracing::warn!("ltx-check staging sweep removed {n} leftover item(s)");
            }
        }
        self.spawn_recovery();
    }

    fn spawn_recovery(&self) {
        if self.inner.recovering.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            self.inner.recovering.store(false, Ordering::SeqCst);
            return;
        };
        let me = self.clone();
        rt.spawn(async move {
            loop {
                // after a trip on consecutive failures the probe proves little: wait the period out first
                if let Some(left) = me.held_for() {
                    tokio::time::sleep(left).await;
                    continue;
                }
                match me.probe_once().await {
                    Ok(()) => {
                        if !me.close() {
                            continue;
                        }
                        me.inner
                            .alerts
                            .alert("breaker-closed", "the check service is ready");
                        me.inner.recovering.store(false, Ordering::SeqCst);
                        // tripped again between the close and the flag: keep probing
                        if me.breaker_open() && !me.inner.recovering.swap(true, Ordering::SeqCst) {
                            continue;
                        }
                        return;
                    }
                    Err(why) => {
                        tracing::warn!("ltx-check probe failed: {why}");
                        // at once, then at most every ten minutes while the service stays down
                        me.alert_limited("probe-failed", &why, Duration::from_secs(600));
                    }
                }
                tokio::time::sleep(me.inner.cfg.probe_interval).await;
            }
        });
    }

    // ------------------------------------------------------------------ renderer files (D9, question 3)

    fn dirs(&self, relight: bool) -> Option<&RendererDirs> {
        if relight {
            self.inner.cfg.relight_dirs.as_ref()
        } else {
            self.inner.cfg.comfy_dirs.as_ref()
        }
    }

    /// Whether this renderer's folders are mounted (so its job files are tracked and deleted).
    pub fn tracks_files(&self, relight: bool) -> bool {
        self.dirs(relight).is_some()
    }

    /// Record an input name and take a hold on it. Call it BEFORE the upload (fix F10): with a second generation
    /// slot, another job ending in between could otherwise delete the file this job has just been handed.
    pub fn note_input(&self, files: &mut JobFiles, relight: bool, name: &str) {
        if self.tracks_files(relight) && !files.inputs.iter().any(|n| n == name) {
            self.inner.in_use.hold(&format!("f:{name}"));
            files.inputs.push(name.to_string());
        }
    }

    pub fn note_input_dir(&self, files: &mut JobFiles, relight: bool, sub: &str) {
        if self.tracks_files(relight) && !files.input_dirs.iter().any(|n| n == sub) {
            self.inner.in_use.hold(&format!("d:{sub}"));
            files.input_dirs.push(sub.to_string());
        }
    }

    pub fn note_output(
        &self,
        files: &mut JobFiles,
        relight: bool,
        subfolder: &str,
        filename: &str,
    ) {
        if self.tracks_files(relight) {
            files
                .outputs
                .push((subfolder.to_string(), filename.to_string()));
        }
    }

    /// Delete everything the job left on the renderer's disk. Bounded (it runs while the slot is still held).
    pub async fn delete_job_files(&self, relight: bool, files: JobFiles, ids: &str) {
        let Some(dirs) = self.dirs(relight).cloned() else {
            return;
        };
        if files.is_empty() {
            return;
        }
        let limit = Duration::from_secs(20);
        match files::delete_job_files_bounded(dirs, files, self.inner.in_use.clone(), limit).await {
            Ok((removed, 0)) => tracing::info!("ltx job files deleted: {ids} removed={removed}"),
            Ok((removed, failed)) => {
                tracing::error!("ltx job files: {ids} removed={removed} failed={failed}");
                self.inner
                    .alerts
                    .alert("delete-failed", &format!("{ids} failed={failed}"));
            }
            Err(e) => {
                tracing::error!("ltx job files: {ids} {e}");
                self.inner
                    .alerts
                    .alert("delete-failed", &format!("{ids} {e}"));
            }
        }
    }
}

/// The still formats the service reads (wire addendum): png, jpeg, webp.
pub fn sniff_still(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// Build the alert sink from settings: always the log line, plus the push when a URL is set.
pub fn alerts_from(cfg: &ChecksConfig) -> Result<Arc<dyn AlertSink>, String> {
    let mut sinks: Vec<Arc<dyn AlertSink>> = vec![Arc::new(alerts::LogAlerts)];
    if let Some(url) = &cfg.alert_url {
        sinks.push(Arc::new(alerts::HttpAlerts::new(url)?));
    }
    Ok(Arc::new(alerts::Fanout(sinks)))
}
