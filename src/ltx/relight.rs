// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! VFX Passes (milestone VP1): the relight sidecar family — NVIDIA Cosmos DiffusionRenderer inverse passes behind the same
//! ComfyUI-shaped transport as the LTX sidecar (docs/development/IMPLEMENTATION-VFX-PASSES.md). Pure decision functions live
//! here so tests reach them; `handlers/ltx.rs` calls them (pinned by source checks).

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;

use crate::ltx::attestation::EnvMeta;
use crate::ltx::template::{template_model_id, TemplateEntry};

/// D6: the registry repo of the relight templates; model id = keccak256(family + "/" + template id).
pub const NVIDIA_FAMILY: &str = "NVIDIA/Cosmos-DiffusionRenderer";

pub fn relight_model_id(template_id: &str) -> [u8; 32] {
    ethers::utils::keccak256(format!("{NVIDIA_FAMILY}/{template_id}").as_bytes())
}

pub fn is_relight(entry: Option<&TemplateEntry>) -> bool {
    entry.and_then(|e| e.sidecar.as_deref()) == Some("relight")
}

/// Batch D21: the compiled deadline defaults. The env may only LOWER the six
/// (so the helper, whose shared vector carries the same numbers, always outlasts
/// the node); the proof rate must equal its default (either way refuses boot).
pub const RELIGHT_TIMEOUT_145_SECS: u64 = 2600;
/// TODO-probe-16: placeholder slopes (s per frame above 145), CONSERVATIVE
/// (Full's quiet 14–19 s/frame × 1.4, used for all three) until BM1.7 probe 16
/// measures ceil((t361 − t145) ÷ 216 × 1.4) per preset (Key = Standard's).
pub const RELIGHT_SECS_PER_EXTRA_FRAME_KEY: u64 = 21;
/// TODO-probe-16 (see [`RELIGHT_SECS_PER_EXTRA_FRAME_KEY`]).
pub const RELIGHT_SECS_PER_EXTRA_FRAME_STD: u64 = 21;
/// TODO-probe-16 (see [`RELIGHT_SECS_PER_EXTRA_FRAME_KEY`]).
pub const RELIGHT_SECS_PER_EXTRA_FRAME_FULL: u64 = 21;
pub const RELIGHT_DEADLINE_MARGIN_SECS: u64 = 600;
/// TODO-probe-16: placeholder upload growth per frame above 145 (Full's five
/// passes); probe 16 sets ceil(1.4 × (u361 − u145) ÷ 216).
pub const RELIGHT_UPLOAD_SECS_PER_EXTRA_FRAME: u64 = 2;
/// BM1.0 probe 15: the registered proof rate of the three relight model ids.
pub const RELIGHT_RATE_LIMIT_TOKENS_PER_SEC: u64 = 10_000;
/// Four chances inside the contract's 3600 s proof window; also the boot cap.
pub const RELIGHT_KEEPALIVE_SECS: u64 = 900;
/// The keep-alive's boot floor (a faster one burns the job in claims and gas).
pub const RELIGHT_KEEPALIVE_MIN_SECS: u64 = 300;
/// The length the base timeout covers; slopes apply to frames above it.
pub const RELIGHT_BASE_FRAMES: u32 = 145;
/// The final proof's grace after the deadline (before the rate wait).
pub const FINAL_PROOF_GRACE_SECS: u64 = 120;

/// D8 / D3 / Batch D21: admission and timing for relight jobs (filled from env at
/// startup by [`AdmitCfg::from_vars`]; tests set it directly).
#[derive(Debug, Clone)]
pub struct AdmitCfg {
    pub budget: Duration,
    pub interval: Duration,
    pub min_free_vram: u64,
    pub comfy_handshake: bool,
    /// `RELIGHT_TIMEOUT_145_SECS`: the watch timeout at ≤ 145 frames.
    pub timeout_145_secs: u64,
    pub secs_per_extra_frame_key: u64,
    pub secs_per_extra_frame_std: u64,
    pub secs_per_extra_frame_full: u64,
    /// `RELIGHT_DEADLINE_MARGIN_SECS`: deadline − watch timeout at ≤ 145 frames.
    pub deadline_margin_secs: u64,
    pub upload_secs_per_extra_frame: u64,
    /// `RELIGHT_RATE_LIMIT_TOKENS_PER_SEC`: the final proof's rate wait (exactly the
    /// registered rate).
    pub rate_tokens_per_sec: u64,
    /// `RELIGHT_KEEPALIVE_SECS`: the keep-alive proof interval (300–900).
    pub keepalive_secs: u64,
}

fn extra_frames(frames: u32) -> u64 {
    u64::from(frames.saturating_sub(RELIGHT_BASE_FRAMES))
}

impl AdmitCfg {
    /// The compute slope for a template; an unknown relight id takes the largest.
    fn slope_for(&self, template_id: &str) -> u64 {
        match template_id {
            "cosmos-passes-key" => self.secs_per_extra_frame_key,
            "cosmos-passes-std" => self.secs_per_extra_frame_std,
            "cosmos-passes-full" => self.secs_per_extra_frame_full,
            _ => self
                .secs_per_extra_frame_key
                .max(self.secs_per_extra_frame_std)
                .max(self.secs_per_extra_frame_full),
        }
    }

    /// `watch`'s timeout: base + slope × max(0, frames − 145).
    pub fn watch_timeout_for(&self, template_id: &str, frames: u32) -> u64 {
        self.timeout_145_secs.saturating_add(
            self.slope_for(template_id)
                .saturating_mul(extra_frames(frames)),
        )
    }

    /// The whole-job deadline: the watch timeout + margin + upload slope ×
    /// max(0, frames − 145) (so 3200 at ≤ 145 frames).
    pub fn deadline_for(&self, template_id: &str, frames: u32) -> u64 {
        self.watch_timeout_for(template_id, frames)
            .saturating_add(self.deadline_margin_secs)
            .saturating_add(
                self.upload_secs_per_extra_frame
                    .saturating_mul(extra_frames(frames)),
            )
    }

    /// ceil(tokens ÷ rate): the final proof's rate-limit wait.
    pub fn rate_wait_secs(&self, tokens: u64) -> u64 {
        tokens.div_ceil(self.rate_tokens_per_sec.max(1))
    }

    /// The final proof's whole budget from the job's start: deadline + 120 s +
    /// tokens ÷ rate (admission's `SESSION_TOO_SHORT` floor, the retry cap and
    /// the base of the helper's timeout).
    pub fn final_budget_secs(&self, template_id: &str, frames: u32, tokens: u64) -> u64 {
        self.deadline_for(template_id, frames)
            .saturating_add(FINAL_PROOF_GRACE_SECS)
            .saturating_add(self.rate_wait_secs(tokens))
    }

    /// Batch D21: the config from an env lookup, or the boot refusal. Each of the
    /// six deadline values may only be LOWERED (a higher or unparsable value
    /// refuses boot); the rate must equal its default; the keep-alive is
    /// capped at 900 (below 300 refuses). The old flat `RELIGHT_JOB_*_SECS` are
    /// ignored with a warning. Returns `(cfg, warnings)`.
    pub fn from_vars(get: &dyn Fn(&str) -> Option<String>) -> Result<(Self, Vec<String>), String> {
        let mut warnings = Vec::new();
        let parse =
            |k: &str| -> Result<Option<u64>, String> {
                match get(k) {
                    None => Ok(None),
                    Some(v) => v.trim().parse::<u64>().map(Some).map_err(|e| {
                        format!("{k}={v:?} is not a whole number of seconds/tokens: {e}")
                    }),
                }
            };
        let num = |k: &str, d: u64| -> u64 { parse(k).ok().flatten().unwrap_or(d) };
        let lower_only = |k: &str, d: u64| -> Result<u64, String> {
            match parse(k)? {
                None => Ok(d),
                Some(v) if v <= d => Ok(v),
                Some(v) => Err(format!(
                    "{k}={v} is above its compiled default {d}: the env may only lower it (the \
                     helper's timeout is built on the compiled value) — refusing to start"
                )),
            }
        };
        // the registered rate exactly: below it the final's wait outruns the
        // helper's; above it every final send is "Too many" and forfeits
        let rate = match parse("RELIGHT_RATE_LIMIT_TOKENS_PER_SEC")? {
            None => RELIGHT_RATE_LIMIT_TOKENS_PER_SEC,
            Some(v) if v == RELIGHT_RATE_LIMIT_TOKENS_PER_SEC => v,
            Some(v) => {
                return Err(format!(
                    "RELIGHT_RATE_LIMIT_TOKENS_PER_SEC={v} differs from the registered proof rate \
                     {RELIGHT_RATE_LIMIT_TOKENS_PER_SEC}: lower lengthens the final proof's wait \
                     past the helper's, higher makes every final \"Too many\" — refusing to start"
                ))
            }
        };
        let keepalive_secs = match parse("RELIGHT_KEEPALIVE_SECS")? {
            None => RELIGHT_KEEPALIVE_SECS,
            Some(v) if v < RELIGHT_KEEPALIVE_MIN_SECS => {
                return Err(format!(
                    "RELIGHT_KEEPALIVE_SECS={v} is below {RELIGHT_KEEPALIVE_MIN_SECS} — refusing \
                     to start"
                ))
            }
            Some(v) if v > RELIGHT_KEEPALIVE_SECS => {
                warnings.push(format!(
                    "RELIGHT_KEEPALIVE_SECS={v} capped at {RELIGHT_KEEPALIVE_SECS} (four proofs \
                     inside the 3600 s window)"
                ));
                RELIGHT_KEEPALIVE_SECS
            }
            Some(v) => v,
        };
        for old in ["RELIGHT_JOB_DEADLINE_SECS", "RELIGHT_JOB_TIMEOUT_SECS"] {
            if get(old).is_some() {
                warnings.push(format!(
                    "{old} is ignored since node 8.61.0 (deadlines scale with frames: \
                     RELIGHT_TIMEOUT_145_SECS + per-frame slopes) — remove it from .env"
                ));
            }
        }
        let cfg = AdmitCfg {
            budget: Duration::from_secs(num("RELIGHT_ADMIT_SECS", 180)),
            interval: Duration::from_secs(2),
            min_free_vram: num("RELIGHT_MIN_FREE_VRAM", 32_212_254_720),
            comfy_handshake: get("RELIGHT_COMFY_HANDSHAKE").is_some_and(|v| v == "1"),
            timeout_145_secs: lower_only("RELIGHT_TIMEOUT_145_SECS", RELIGHT_TIMEOUT_145_SECS)?,
            secs_per_extra_frame_key: lower_only(
                "RELIGHT_SECS_PER_EXTRA_FRAME_KEY",
                RELIGHT_SECS_PER_EXTRA_FRAME_KEY,
            )?,
            secs_per_extra_frame_std: lower_only(
                "RELIGHT_SECS_PER_EXTRA_FRAME_STD",
                RELIGHT_SECS_PER_EXTRA_FRAME_STD,
            )?,
            secs_per_extra_frame_full: lower_only(
                "RELIGHT_SECS_PER_EXTRA_FRAME_FULL",
                RELIGHT_SECS_PER_EXTRA_FRAME_FULL,
            )?,
            deadline_margin_secs: lower_only(
                "RELIGHT_DEADLINE_MARGIN_SECS",
                RELIGHT_DEADLINE_MARGIN_SECS,
            )?,
            upload_secs_per_extra_frame: lower_only(
                "RELIGHT_UPLOAD_SECS_PER_EXTRA_FRAME",
                RELIGHT_UPLOAD_SECS_PER_EXTRA_FRAME,
            )?,
            rate_tokens_per_sec: rate,
            keepalive_secs,
        };
        Ok((cfg, warnings))
    }

    /// The process env, for `ApiServer`'s constructors. `main` runs
    /// [`AdmitCfg::from_vars`] itself first and refuses to boot on its error, so
    /// the fallback to the compiled defaults here is never served.
    pub fn from_env() -> Self {
        Self::from_vars(&|k| std::env::var(k).ok())
            .map(|(c, _)| c)
            .unwrap_or_default()
    }
}

impl Default for AdmitCfg {
    fn default() -> Self {
        Self::from_vars(&|_| None)
            .expect("the compiled defaults are valid")
            .0
    }
}

/// Batch D21: admission at 0 tokens, on the D20 gate's snapshot (relight jobs).
/// `SESSION_TOO_SHORT`: the session's remaining lifetime is below the final
/// proof's budget (deadline + 120 s + tokens ÷ rate). `SESSION_TERMS`: its
/// `proofInterval` exceeds a quarter of the job (the first keep-alive would claim
/// most of it), or — above 145 frames — its proof window is under two keep-alive
/// intervals or it is already within one interval + 120 s of timing out.
pub fn relight_session_admission(
    cfg: &AdmitCfg,
    template_id: &str,
    frames: u32,
    tokens: u64,
    snap: &crate::training::accept::SessionSnapshot,
    now_unix: u64,
) -> Result<(), AdmitRefusal> {
    use ethers::types::U256;
    let now = U256::from(now_unix);
    let budget = cfg.final_budget_secs(template_id, frames, tokens);
    let ends = snap.start_time.saturating_add(snap.max_duration);
    let left = ends.saturating_sub(now);
    if left < U256::from(budget) {
        return Err((
            "SESSION_TOO_SHORT",
            format!(
                "the session has {left} s left; this {frames}-frame job needs {budget} s (its \
                 deadline + {FINAL_PROOF_GRACE_SECS} s + the final proof's rate wait) — open a \
                 longer session"
            ),
        ));
    }
    if snap.proof_interval.saturating_mul(U256::from(4u64)) > U256::from(tokens) {
        return Err((
            "SESSION_TERMS",
            format!(
                "the session's proofInterval {} is above a quarter of this {tokens}-token job",
                snap.proof_interval
            ),
        ));
    }
    if frames > RELIGHT_BASE_FRAMES {
        let keepalive = U256::from(cfg.keepalive_secs);
        let window = snap.proof_timeout_window;
        if window < keepalive.saturating_mul(U256::from(2u64)) {
            return Err((
                "SESSION_TERMS",
                format!(
                    "the session's proof window {window} s is under two keep-alive intervals \
                     ({} s): it could time out mid-render",
                    cfg.keepalive_secs
                ),
            ));
        }
        let since = snap.last_proof_time.max(snap.start_time);
        let until_timeout = since.saturating_add(window).saturating_sub(now);
        if until_timeout < keepalive.saturating_add(U256::from(FINAL_PROOF_GRACE_SECS)) {
            return Err((
                "SESSION_TERMS",
                format!(
                    "the session times out in {until_timeout} s, before the first keep-alive \
                     proof could land"
                ),
            ));
        }
    }
    Ok(())
}

/// D3: the node refuses to start with a relight sidecar unless jobs are serialised (one generation slot).
pub fn check_relight_start(
    relight_url: Option<&str>,
    max_concurrent_generations: usize,
) -> Result<(), String> {
    if relight_url.is_some() && max_concurrent_generations != 1 {
        return Err(format!(
            "RELIGHT_URL is set but MAX_CONCURRENT_GENERATIONS={max_concurrent_generations}: the relight sidecar needs \
             exactly one generation slot (VRAM admission, IMPLEMENTATION-VFX-PASSES D8)"
        ));
    }
    Ok(())
}

/// D11: the pins the node expects (`RELIGHT_PINS`, one-line JSON) and the echo it compares.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RelightPins {
    pub weights: BTreeMap<String, String>,
    pub stack: String,
}

impl RelightPins {
    pub fn from_env() -> Option<Self> {
        std::env::var("RELIGHT_PINS")
            .ok()
            .and_then(|v| serde_json::from_str(&v).ok())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelightEcho {
    pub gpu: String,
    pub cuda: String,
}

/// D11: relight `EnvMeta` — each field from its own source.
pub fn relight_env_meta(pins: &RelightPins, echo: &RelightEcho, node_commit: &str) -> EnvMeta {
    let canonical = serde_json::to_vec(&pins.weights).expect("map serialises");
    EnvMeta {
        weights_hash: format!("0x{}", hex::encode(ethers::utils::keccak256(canonical))),
        lora_hash: String::new(),
        comfy_commit: pins.stack.clone(),
        node_commit: node_commit.to_string(),
        cuda_version: echo.cuda.clone(),
        gpu_class: echo.gpu.clone(),
    }
}

/// D7 (OQ-V5 relight-only fallback): the attestation's model id for relight templates; `None` = keep `LTX_MODEL_ID`.
pub fn attestation_model_id(entry: &TemplateEntry) -> Option<String> {
    if entry.sidecar.as_deref() == Some("relight") {
        Some(format!("0x{}", hex::encode(template_model_id(entry))))
    } else {
        None
    }
}

/// D3 + D18: what `run()` does at the finalising gate for a relight job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinaliseDecision {
    Proceed,
    Abandon(String),
}

pub fn relight_finalise_decision(
    elapsed: Duration,
    deadline: Duration,
    terms: Result<(), String>,
) -> FinaliseDecision {
    if elapsed > deadline {
        return FinaliseDecision::Abandon(format!(
            "DEADLINE: the job ran {:.0} s, past its {:.0} s whole-job deadline",
            elapsed.as_secs_f64(),
            deadline.as_secs_f64()
        ));
    }
    match terms {
        Ok(()) => FinaliseDecision::Proceed,
        Err(e) => FinaliseDecision::Abandon(format!("SESSION_CLOSED: {e}")),
    }
}

/// D8: the pin comparison (exact equality of the weights map and `stack`).
pub fn pins_match(
    expected: Option<&RelightPins>,
    echoed: Option<&RelightPins>,
) -> Result<(), String> {
    match (expected, echoed) {
        (None, _) => Err("SIDECAR_PIN_MISMATCH: no pins configured (RELIGHT_PINS)".to_string()),
        (Some(_), None) => {
            Err("SIDECAR_PIN_MISMATCH: the relight sidecar echoed no pins".to_string())
        }
        (Some(a), Some(b)) if a == b => Ok(()),
        (Some(_), Some(_)) => Err(
            "SIDECAR_PIN_MISMATCH: the relight sidecar's weights or stack differ from RELIGHT_PINS"
                .to_string(),
        ),
    }
}

/// D8: a refusal from admission — the wire code and its message.
pub type AdmitRefusal = (&'static str, String);

fn gpu_busy(why: String) -> AdmitRefusal {
    ("CAPACITY", format!("GPU_BUSY: {why}"))
}

/// D8 (a)-(c): admit one relight job. Runs AFTER D20 and BEFORE `prepare_inputs`. Returns the sidecar's GPU/CUDA echo
/// (D11 `EnvMeta`). `comfy` is consulted only when `cfg.comfy_handshake` is on.
pub async fn admit_relight(
    relight: &crate::ltx::ComfyClient,
    comfy: Option<&crate::ltx::ComfyClient>,
    cfg: &AdmitCfg,
    pins: Option<&RelightPins>,
) -> Result<RelightEcho, AdmitRefusal> {
    use crate::ltx::client::StatsError;
    if pins.is_none() {
        return Err(("SIDECAR_UNAVAILABLE", pins_match(None, None).unwrap_err()));
    }
    let start = tokio::time::Instant::now();
    let out_of_time = |start: tokio::time::Instant| start.elapsed() >= cfg.budget;
    // (a) the per-job pin check: the first readable stats decide it; unreachable fails at once.
    let mut last: String;
    let mut checked = false;
    // the ComfyUI handshake (opt-in) runs once, after the pin check
    let mut handshake_done = !cfg.comfy_handshake || comfy.is_none();
    loop {
        // each read is bounded by what is left of the budget (a hung sidecar must not hold the slot past it)
        let left = cfg
            .budget
            .saturating_sub(start.elapsed())
            .max(Duration::from_millis(100));
        let read = match tokio::time::timeout(left, relight.stats()).await {
            Ok(r) => r,
            Err(_) => Err(StatsError::Bad("timed out".into())),
        };
        match read {
            Err(StatsError::Unreachable(e)) => {
                return Err((
                    "SIDECAR_UNAVAILABLE",
                    format!("relight sidecar unreachable: {e}"),
                ));
            }
            Err(StatsError::NotReady) => {
                last = "the relight sidecar is still hashing its weights".into()
            }
            Err(StatsError::Bad(e)) => last = format!("unreadable /system_stats: {e}"),
            Ok(st) => {
                let Some(r) = st.relight.as_ref() else {
                    return Err((
                        "SIDECAR_UNAVAILABLE",
                        "RELIGHT_URL does not answer as a relight sidecar".into(),
                    ));
                };
                if !checked {
                    pins_match(pins, r.pins.as_ref()).map_err(|e| ("SIDECAR_UNAVAILABLE", e))?;
                    checked = true;
                }
                if !handshake_done {
                    comfy_handshake(comfy.expect("checked"), cfg, start).await?;
                    handshake_done = true;
                    continue;
                }
                let free = st.devices.first().map_or(0, |d| d.vram_free);
                if r.busy {
                    last = "the relight sidecar is busy".into();
                } else if free < cfg.min_free_vram {
                    last = format!("{free} bytes of VRAM free, {} needed", cfg.min_free_vram);
                } else {
                    return Ok(RelightEcho {
                        gpu: r.gpu.clone(),
                        cuda: r.cuda.clone(),
                    });
                }
            }
        }
        if out_of_time(start) {
            return Err(gpu_busy(format!("{last} after {} s", cfg.budget.as_secs())));
        }
        tokio::time::sleep(cfg.interval).await;
    }
}

/// D8 handshake (opt-in): ComfyUI must be idle; then `/free` with the unload body; then its `vram_free` polled within the same
/// budget. An unreachable ComfyUI holds nothing → proceed.
async fn comfy_handshake(
    comfy: &crate::ltx::ComfyClient,
    cfg: &AdmitCfg,
    start: tokio::time::Instant,
) -> Result<(), AdmitRefusal> {
    use crate::ltx::client::{FreeError, StatsError};
    // every ComfyUI call is bounded by what is left of the admission budget (a hung ComfyUI must not hold the slot)
    let left = || {
        cfg.budget
            .saturating_sub(start.elapsed())
            .max(Duration::from_millis(100))
    };
    loop {
        let idle = tokio::time::timeout(left(), comfy.queue_idle())
            .await
            .unwrap_or_else(|_| Err(StatsError::Bad("timed out".into())));
        match idle {
            Err(StatsError::Unreachable(_)) => return Ok(()),
            Ok(true) => break,
            Ok(false) | Err(_) => {}
        }
        if start.elapsed() >= cfg.budget {
            return Err(gpu_busy("ComfyUI's queue is not idle".into()));
        }
        tokio::time::sleep(cfg.interval).await;
    }
    let freed = tokio::time::timeout(
        left(),
        comfy.free(Some(
            serde_json::json!({"unload_models": true, "free_memory": true}),
        )),
    )
    .await
    .unwrap_or_else(|_| Err(FreeError::Other("timed out".into())));
    match freed {
        Ok(()) => {}
        Err(FreeError::Unreachable(_)) => return Ok(()),
        Err(e) => return Err(gpu_busy(format!("ComfyUI /free failed: {e:?}"))),
    }
    loop {
        let read = tokio::time::timeout(left(), comfy.stats())
            .await
            .unwrap_or_else(|_| Err(StatsError::Bad("timed out".into())));
        match read {
            Err(StatsError::Unreachable(_)) => return Ok(()),
            Ok(st)
                if st
                    .devices
                    .first()
                    .is_some_and(|d| d.vram_free >= cfg.min_free_vram) =>
            {
                return Ok(())
            }
            _ => {}
        }
        if start.elapsed() >= cfg.budget {
            return Err(gpu_busy("ComfyUI still holds the VRAM after /free".into()));
        }
        tokio::time::sleep(cfg.interval).await;
    }
}

/// D8 (c) once more, immediately before `submit` (inputs were staged meanwhile).
pub async fn repoll_vram(
    relight: &crate::ltx::ComfyClient,
    cfg: &AdmitCfg,
) -> Result<(), AdmitRefusal> {
    match relight.stats().await {
        Ok(st) => {
            let free = st.devices.first().map_or(0, |d| d.vram_free);
            let busy = st.relight.as_ref().is_none_or(|r| r.busy);
            if busy || free < cfg.min_free_vram {
                Err(gpu_busy(format!(
                    "VRAM fell to {free} bytes (need {}) or the sidecar turned busy before submit",
                    cfg.min_free_vram
                )))
            } else {
                Ok(())
            }
        }
        Err(e) => Err(gpu_busy(format!(
            "relight /system_stats before submit: {e:?}"
        ))),
    }
}

/// D8: before an LTX job, empty the relight sidecar. Only `Unreachable` proceeds; an HTTP error, any other error or a stall
/// past the admission budget refuses the LTX job (`CAPACITY` + `GPU_BUSY:`).
pub async fn free_relight_before_ltx(
    relight: &crate::ltx::ComfyClient,
    cfg: &AdmitCfg,
) -> Result<(), AdmitRefusal> {
    use crate::ltx::client::FreeError;
    let res = match tokio::time::timeout(cfg.budget, relight.free(None)).await {
        Ok(r) => r,
        Err(_) => Err(FreeError::Other("timed out".into())),
    };
    match res {
        Ok(()) | Err(FreeError::Unreachable(_)) => Ok(()),
        Err(e) => Err(gpu_busy(format!(
            "the relight sidecar did not release the GPU: {e:?}"
        ))),
    }
}

/// D8: a `watch` error whose text carries the sidecar's `GPU_BUSY` maps to the retryable `CAPACITY`.
pub fn watch_error_code(err: &str) -> &'static str {
    if err.contains(": GPU_BUSY") {
        "CAPACITY"
    } else if err.contains("timed out") {
        "TIMEOUT"
    } else {
        "GENERATION_FAILED"
    }
}
