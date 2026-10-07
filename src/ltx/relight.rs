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

/// D8 / D3: admission and timing for relight jobs (filled from env at startup; tests set it directly).
#[derive(Debug, Clone)]
pub struct AdmitCfg {
    pub budget: Duration,
    pub interval: Duration,
    pub min_free_vram: u64,
    pub deadline_secs: u64,
    pub watch_timeout_secs: u64,
    pub comfy_handshake: bool,
}

impl AdmitCfg {
    pub fn from_env() -> Self {
        let num = |k: &str, d: u64| {
            std::env::var(k)
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(d)
        };
        AdmitCfg {
            budget: Duration::from_secs(num("RELIGHT_ADMIT_SECS", 180)),
            interval: Duration::from_secs(2),
            min_free_vram: num("RELIGHT_MIN_FREE_VRAM", 32_212_254_720),
            deadline_secs: num("RELIGHT_JOB_DEADLINE_SECS", 2700),
            watch_timeout_secs: num("RELIGHT_JOB_TIMEOUT_SECS", 2100),
            comfy_handshake: std::env::var("RELIGHT_COMFY_HANDSHAKE")
                .map(|v| v == "1")
                .unwrap_or(false),
        }
    }
}

impl Default for AdmitCfg {
    fn default() -> Self {
        AdmitCfg {
            budget: Duration::from_secs(180),
            interval: Duration::from_secs(2),
            min_free_vram: 32_212_254_720,
            deadline_secs: 2700,
            watch_timeout_secs: 2100,
            comfy_handshake: false,
        }
    }
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
