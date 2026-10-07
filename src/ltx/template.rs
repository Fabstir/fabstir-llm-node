// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Pinned template store + versioned allow-list bundle. The sidecar runs ONLY
//! graphs whose keccak256 hash is in its allow-list (Design Decision 4): a
//! ComfyUI graph can execute arbitrary Python via custom nodes, so pinning is
//! what makes the registered model id a truthful provenance claim.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::checkpoint::delta::sort_json_keys;
use crate::ltx::types::Resolution;

/// A parsed ComfyUI API-format graph: a flat `node_id -> { class_type, inputs,
/// _meta }` object, kept as a `serde_json::Value` so the patcher (Phase 4) can
/// substitute input values without a rigid schema.
#[derive(Debug, Clone)]
pub struct Graph(pub serde_json::Value);

/// Param bounds advertised in the bundle; the handler validates against these.
/// `rename_all = camelCase` is a no-op on the single-word M0 fields (so the wire
/// is byte-unchanged), and gives the M1a image fields their `imageMaxBytes` /
/// `imageFormats` keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub frames: FrameBounds,
    pub fps: Vec<u32>,
    pub resolutions: Vec<Resolution>,
    /// Max plaintext bytes for ONE input image (M1a). `default` keeps a t2v-only
    /// allow-list (no image fields) parsing to 0.
    #[serde(default)]
    pub image_max_bytes: u64,
    /// Accepted input-image container formats (M1a; advisory).
    #[serde(default)]
    pub image_formats: Vec<String>,
    /// Max plaintext bytes for ONE input video (BL3). `default` keeps v5-shaped
    /// bundles (no video fields) parsing to 0.
    #[serde(default)]
    pub video_max_bytes: u64,
    /// Accepted input-video container formats (BL3; advisory).
    #[serde(default)]
    pub video_formats: Vec<String>,
    /// Max plaintext bytes for a deep-conform EXR tar (v8.44.0, `inputWire`
    /// jobs — videos[0] is a 16-bit sequence, not an mp4, and dwarfs the mp4
    /// bound by design). `default` 0 = bundles that predate the deep wire
    /// refuse every deep job, fail-closed.
    #[serde(default)]
    pub deep_video_max_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameBounds {
    pub min: u32,
    pub max: u32,
}

/// One allow-listed template with its computed keccak256 hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateEntry {
    pub template_id: String,
    pub template_hash: String,
    /// Number of input images the template consumes (M1a). This is the
    /// `inputCommitment` FORMAT SELECTOR: 0 ⇒ M0 seven-field, >0 ⇒ v2. ALWAYS
    /// serialised (even 0 for t2v) so the selector is explicit on the wire.
    #[serde(default)]
    pub image_inputs: u32,
    /// Advisory per-slot meaning (e.g. `["firstFrame","lastFrame"]`), in the same
    /// order the node binds `images[i]` to `LoadImage` nodes. Empty ⇒ omitted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_semantics: Vec<String>,
    /// Number of input videos the template consumes (BL3). Together with
    /// `image_inputs` this selects the `inputCommitment` format (>0 ⇒ v3).
    /// ALWAYS serialised (even 0) so the selector is explicit on the wire,
    /// mirroring `image_inputs`; `default` keeps v5-shaped bundles parsing.
    #[serde(default)]
    pub video_inputs: u32,
    /// Advisory per-slot meaning (e.g. `["controlVideo"]`), in the same order
    /// the node binds `videos[i]` to `LoadVideo` nodes. Empty ⇒ omitted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub video_semantics: Vec<String>,
    /// NM1 D7 per-template rules, advertised so clients refuse before escrow and
    /// enforced by the node before accept ([`check_template_rules`]). A missing
    /// field means NO restriction, and is omitted on the wire, so entries without
    /// rules serialise byte-identically. The template's allowed fps, a subset of
    /// the bundle's `bounds.fps`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<Vec<u32>>,
    /// The longest job the template may run, in frames (measured on host1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_frames: Option<u32>,
    /// A named resolution rule; `"div64-fhd"` is the only one defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_rule: Option<String>,
    /// D3: the graph caps the control clip at the billed frames, so where LTX
    /// can deliver every billed frame the clip must carry all of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_control: Option<bool>,
    /// D21: lengths are exact LTX frame counts (8k+1), not whole seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_grid: Option<bool>,
    /// VFX Passes D3: which sidecar runs the template. Absent = the ComfyUI LTX sidecar; `"relight"` = the Cosmos
    /// DiffusionRenderer sidecar. Any other value refuses at load. Omitted on the wire when absent, so existing entries
    /// serialise byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidecar: Option<String>,
}

/// Versioned allow-list bundle: advertised in NodeRegistry metadata and echoed
/// (`allowListVersion`) in `ltx_accepted` so a client can detect drift and
/// refetch BEFORE escrow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowListBundle {
    pub allow_list_version: u32,
    pub bundle_hash: String,
    pub templates: Vec<TemplateEntry>,
    pub loras: Vec<String>,
    pub bounds: Bounds,
}

/// On-disk `allowlist.json` (lists which pinned files are active; no hashes).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AllowListConfig {
    allow_list_version: u32,
    templates: Vec<ConfigEntry>,
    loras: Vec<String>,
    bounds: Bounds,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigEntry {
    template_id: String,
    version: String,
    /// M1a; optional so a t2v-only entry (no image fields) still parses to 0.
    #[serde(default)]
    image_inputs: u32,
    #[serde(default)]
    image_semantics: Vec<String>,
    /// BL3; optional so pre-video entries still parse to 0.
    #[serde(default)]
    video_inputs: u32,
    #[serde(default)]
    video_semantics: Vec<String>,
    /// NM1 D7/D21 rules, copied into the advertised [`TemplateEntry`].
    #[serde(default)]
    fps: Option<Vec<u32>>,
    #[serde(default)]
    max_frames: Option<u32>,
    #[serde(default)]
    resolution_rule: Option<String>,
    #[serde(default)]
    exact_control: Option<bool>,
    #[serde(default)]
    frame_grid: Option<bool>,
    #[serde(default)]
    sidecar: Option<String>,
}

/// Loads and pins the allow-listed templates at startup.
pub struct TemplateStore {
    graphs: HashMap<String, (Graph, String)>, // id -> (graph, "0x"+keccak hex)
    bundle: AllowListBundle,
}

impl TemplateStore {
    /// Load `<dir>/allowlist.json` and every pinned template it lists, computing
    /// each template's canonical keccak256 hash and the bundle hash.
    pub fn new<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let dir = dir.as_ref();
        let cfg_path = dir.join("allowlist.json");
        let cfg_bytes = std::fs::read(&cfg_path)
            .with_context(|| format!("reading allow-list {}", cfg_path.display()))?;
        let cfg: AllowListConfig = serde_json::from_slice(&cfg_bytes)
            .with_context(|| format!("parsing allow-list {}", cfg_path.display()))?;

        let mut graphs = HashMap::new();
        let mut templates = Vec::new();
        for entry in &cfg.templates {
            validate_segment(&entry.template_id)?;
            validate_segment(&entry.version)?;
            if let Some(sc) = entry.sidecar.as_deref() {
                if sc != "relight" {
                    return Err(anyhow!(
                        "unknown sidecar {:?} for template {:?} — refusing",
                        sc,
                        entry.template_id
                    ));
                }
            }
            if graphs.contains_key(&entry.template_id) {
                return Err(anyhow!(
                    "duplicate templateId {:?} in allow-list",
                    entry.template_id
                ));
            }
            let path: PathBuf = dir
                .join(&entry.template_id)
                .join(format!("{}.json", entry.version));
            let raw = std::fs::read(&path)
                .with_context(|| format!("reading template {}", path.display()))?;
            let value: serde_json::Value = serde_json::from_slice(&raw)
                .with_context(|| format!("parsing template {}", path.display()))?;
            let hash = canonical_keccak(&value);
            graphs.insert(entry.template_id.clone(), (Graph(value), hash.clone()));
            templates.push(TemplateEntry {
                template_id: entry.template_id.clone(),
                template_hash: hash,
                image_inputs: entry.image_inputs,
                image_semantics: entry.image_semantics.clone(),
                video_inputs: entry.video_inputs,
                video_semantics: entry.video_semantics.clone(),
                fps: entry.fps.clone(),
                max_frames: entry.max_frames,
                resolution_rule: entry.resolution_rule.clone(),
                exact_control: entry.exact_control,
                frame_grid: entry.frame_grid,
                sidecar: entry.sidecar.clone(),
            });
        }
        // Canonical order so bundleHash is independent of allowlist.json ordering.
        templates.sort_by(|a, b| a.template_id.cmp(&b.template_id));

        let mut bundle = AllowListBundle {
            allow_list_version: cfg.allow_list_version,
            bundle_hash: String::new(),
            templates,
            loras: cfg.loras,
            bounds: cfg.bounds,
        };
        bundle.bundle_hash = compute_bundle_hash(&bundle);

        Ok(Self { graphs, bundle })
    }

    /// Verify a client-supplied `(templateId, templateHash)` against the
    /// allow-list and return the pinned graph. Fails closed: unknown id or any
    /// hash mismatch is a hard reject.
    pub fn verify(&self, id: &str, hash: &str) -> Result<Graph> {
        let (graph, pinned) = self
            .graphs
            .get(id)
            .ok_or_else(|| anyhow!("unknown templateId {:?}", id))?;
        if !hash.eq_ignore_ascii_case(pinned) {
            return Err(anyhow!("templateHash mismatch for {:?}", id));
        }
        Ok(graph.clone())
    }

    /// The computed hash of a pinned template (for advertisement / tests).
    pub fn template_hash(&self, id: &str) -> Option<&str> {
        self.graphs.get(id).map(|(_, h)| h.as_str())
    }

    /// The pinned graph for `id`, without a hash challenge — for host-side
    /// derivation over templates the store itself loaded (W1 weight manifests).
    /// Client-supplied ids must still go through [`Self::verify`].
    pub fn graph(&self, id: &str) -> Option<Graph> {
        self.graphs.get(id).map(|(g, _)| g.clone())
    }

    /// The number of input images template `id` consumes — the `inputCommitment`
    /// format selector the handler validates `job.images.len()` against (M1a).
    /// `None` for an unknown id.
    pub fn image_inputs(&self, id: &str) -> Option<u32> {
        self.entry(id).map(|t| t.image_inputs)
    }

    /// The advertised entry for `id`, rules included. `None` for an unknown id.
    pub fn entry(&self, id: &str) -> Option<&TemplateEntry> {
        self.bundle.templates.iter().find(|t| t.template_id == id)
    }

    /// The number of input videos template `id` consumes — the BL3 analogue of
    /// `image_inputs` the handler validates `job.videos.len()` against. `None`
    /// for an unknown id.
    pub fn video_inputs(&self, id: &str) -> Option<u32> {
        self.entry(id).map(|t| t.video_inputs)
    }

    pub fn bundle(&self) -> &AllowListBundle {
        &self.bundle
    }
}

/// NM1 D7: refuse a job outside its template's own rules, before accept. Pure, so
/// the integration tests reach it. A missing field restricts nothing; an unknown
/// `resolutionRule` refuses (fail closed). Lengths on `frameGrid` templates are
/// checked by the handler's `check_length`; `maxFrames` is checked here for every
/// template that carries it.
pub fn check_template_rules(
    entry: &TemplateEntry,
    w: u32,
    h: u32,
    fps: u32,
    frames: u32,
) -> Result<(), String> {
    let id = &entry.template_id;
    if let Some(allowed) = &entry.fps {
        if !allowed.contains(&fps) {
            return Err(format!(
                "fps {fps} is not allowed for {id} (allowed: {allowed:?})"
            ));
        }
    }
    if let Some(max) = entry.max_frames {
        if frames > max {
            return Err(format!(
                "{frames} frames is over the {max}-frame maximum for {id}"
            ));
        }
    }
    match entry.resolution_rule.as_deref() {
        None => {}
        Some("div64-fhd") => {
            let fits = w % 64 == 0
                && h % 64 == 0
                && u64::from(w) * u64::from(h) <= 1920 * 1088
                && w.max(h) <= 1920;
            if !fits {
                return Err(format!(
                    "resolution {w}x{h} is not allowed for {id} (sides divisible by 64, at most 1920x1088)"
                ));
            }
        }
        // VFX Passes D5: the relight model runs at a FIXED 1280x704, so smaller or portrait jobs would pay less for the
        // same GPU work; only 1920x1088 (a 1920x1080 scene runs as 1088, like Cut-out).
        Some("relight-fhd") => {
            if (w, h) != (1920, 1088) {
                return Err(format!(
                    "resolution {w}x{h} is not allowed for {id} (exactly 1920x1088)"
                ));
            }
        }
        Some(other) => {
            return Err(format!(
                "unknown resolution rule {other:?} for {id} — refusing"
            ));
        }
    }
    Ok(())
}

/// NM1 D20: the on-chain model id of an LTX template,
/// `keccak256("Lightricks/LTX-Video/" + template_id)` — the derivation every live
/// LTX model id follows.
pub fn ltx_model_id(template_id: &str) -> [u8; 32] {
    ethers::utils::keccak256(format!("Lightricks/LTX-Video/{template_id}").as_bytes())
}

/// VFX Passes D6: the on-chain model id of any allow-listed template — the NVIDIA family for `sidecar == "relight"`,
/// the Lightricks family otherwise (byte-identical to [`ltx_model_id`] for every existing id).
pub fn template_model_id(entry: &TemplateEntry) -> [u8; 32] {
    match entry.sidecar.as_deref() {
        Some("relight") => crate::ltx::relight::relight_model_id(&entry.template_id),
        _ => ltx_model_id(&entry.template_id),
    }
}

/// VFX Passes D6: the model id `run()`'s D20 gate expects (the entry's family; Lightricks when no entry is known).
pub fn expected_session_model(entry: Option<&TemplateEntry>, template_id: &str) -> [u8; 32] {
    match entry {
        Some(e) => template_model_id(e),
        None => ltx_model_id(template_id),
    }
}

/// NM1 D20 with an explicit expected id (VFX Passes D6). [`check_session_model`] is the Lightricks-family wrapper.
pub fn check_session_model_for(
    session: Option<[u8; 32]>,
    expected: [u8; 32],
    template_id: &str,
) -> Result<(), String> {
    let Some(model) = session else {
        return Err(format!(
            "{template_id} job has no on-chain job id — its session model cannot be checked"
        ));
    };
    if model == [0u8; 32] {
        return Err(format!("{template_id} job's session has no model id"));
    }
    if model != expected {
        return Err(format!(
            "the session was opened for model 0x{}, not {template_id}'s model",
            hex::encode(model)
        ));
    }
    Ok(())
}

/// NM1 D20: an LTX job must run under its OWN template's model id. Settlement pays
/// at the session's model price, so a session opened for a cheaper model must not
/// buy this render. `None` = the job has no on-chain id: nobody pays for the GPU
/// work, so it is refused too. All-zero (unset) refuses.
pub fn check_session_model(session: Option<[u8; 32]>, template_id: &str) -> Result<(), String> {
    check_session_model_for(session, ltx_model_id(template_id), template_id)
}

/// NM1 D20: the session must be one this node's proof can land on — otherwise the
/// render delivers and the proof reverts, so the clip is free. `submitProofOfWork`
/// reverts unless the session is Active, `msg.sender` is its host, and the
/// cumulative claim stays within `deposit × 1000 / price` (the same terms
/// training's A.3 gate checks, `training::accept::validate_session`).
/// `tracked_tokens` is this node's `LtxTracker` total for the session — every
/// clip it completed there, a forfeited one included (so it can over-count,
/// which only ever refuses): the chain's `tokensUsed` can lag it, so the larger
/// counts.
pub fn check_session_terms(
    snap: &crate::training::accept::SessionSnapshot,
    this_host: ethers::types::Address,
    job_tokens: u64,
    tracked_tokens: u64,
) -> Result<(), String> {
    use crate::training::accept::SessionStatus;
    use ethers::types::U256;
    if snap.status != SessionStatus::Active {
        return Err(format!("the session is not Active ({:?})", snap.status));
    }
    if snap.host != this_host {
        return Err(format!(
            "the session belongs to host {:?}, not this host",
            snap.host
        ));
    }
    if snap.price_per_token.is_zero() {
        return Err("the session has a zero price per token".to_string());
    }
    let capacity = snap.deposit.saturating_mul(U256::from(1000u64)) / snap.price_per_token;
    let used = snap.tokens_used.max(U256::from(tracked_tokens));
    let remaining = capacity.saturating_sub(used);
    if remaining < U256::from(job_tokens) {
        return Err(format!(
            "the session's remaining deposit covers {remaining} tokens; this job is {job_tokens}"
        ));
    }
    Ok(())
}

/// NM1 D20: the whole pre-staging session decision, with the two chain reads
/// injected (`run()` passes `CheckpointManager::query_session_model` and
/// `query_session_jobs_raw`). No job id → refused without reading anything; else
/// both reads run concurrently (each retried, each attempt bounded), and the job
/// passes only if the session's model is the template's (`check_session_model`),
/// its record decodes (training's fixed-offset decoder, fail closed) and its terms
/// let this node's proof land (`check_session_terms`).
#[allow(clippy::too_many_arguments)]
pub async fn session_gate<MF, MFut, JF, JFut>(
    job_id: Option<u64>,
    template_id: &str,
    job_tokens: u64,
    tracked_tokens: u64,
    this_host: &str,
    read_model: MF,
    read_session: JF,
) -> std::result::Result<(), String>
where
    MF: Fn(u64) -> MFut,
    MFut: std::future::Future<Output = Result<[u8; 32]>>,
    JF: Fn(u64) -> JFut,
    JFut: std::future::Future<Output = Result<Vec<u8>>>,
{
    session_gate_with(
        job_id,
        template_id,
        job_tokens,
        tracked_tokens,
        this_host,
        read_model,
        read_session,
        GateTiming::PRODUCTION,
    )
    .await
}

/// NM1 D24: the gate's timing. `budget`/`gap` bound the wait for a session the
/// node's RPC has not seen yet; the last three feed every `read_with_retry`.
#[derive(Debug, Clone, Copy)]
pub struct GateTiming {
    pub budget: std::time::Duration,
    pub gap: std::time::Duration,
    pub read_attempts: u32,
    pub read_delay: std::time::Duration,
    pub per_attempt: std::time::Duration,
}

/// NM1 D24: how long the gate waits for a session its RPC has not seen yet, and
/// how often it re-reads meanwhile (Base blocks are 2 s; the lag seen was a block
/// or two). A session that never appears holds the generation slot for about the
/// budget plus one gap and the reads (≈ 16-18 s) before it is refused.
pub const VISIBILITY_BUDGET: std::time::Duration = std::time::Duration::from_secs(15);
pub const VISIBILITY_GAP: std::time::Duration = std::time::Duration::from_secs(2);

impl GateTiming {
    /// Production: the visibility budget and gap above; each read keeps D20's 3
    /// attempts, 2 s apart, 10 s each.
    pub const PRODUCTION: GateTiming = GateTiming {
        budget: VISIBILITY_BUDGET,
        gap: VISIBILITY_GAP,
        read_attempts: 3,
        read_delay: std::time::Duration::from_secs(2),
        per_attempt: std::time::Duration::from_secs(10),
    };
}

/// `session_gate` with explicit timing (tests pass millisecond values).
#[allow(clippy::too_many_arguments)]
pub async fn session_gate_with<MF, MFut, JF, JFut>(
    job_id: Option<u64>,
    template_id: &str,
    job_tokens: u64,
    tracked_tokens: u64,
    this_host: &str,
    read_model: MF,
    read_session: JF,
    timing: GateTiming,
) -> std::result::Result<(), String>
where
    MF: Fn(u64) -> MFut,
    MFut: std::future::Future<Output = Result<[u8; 32]>>,
    JF: Fn(u64) -> JFut,
    JFut: std::future::Future<Output = Result<Vec<u8>>>,
{
    session_gate_for_model(
        job_id,
        ltx_model_id(template_id),
        template_id,
        job_tokens,
        tracked_tokens,
        this_host,
        read_model,
        read_session,
        timing,
    )
    .await
}

/// VFX Passes D6: the D20/D24 gate against an explicit expected model id (`run()` passes
/// `expected_session_model(store.entry(id), id)`).
#[allow(clippy::too_many_arguments)]
pub async fn session_gate_for_model<MF, MFut, JF, JFut>(
    job_id: Option<u64>,
    expected: [u8; 32],
    template_id: &str,
    job_tokens: u64,
    tracked_tokens: u64,
    this_host: &str,
    read_model: MF,
    read_session: JF,
    timing: GateTiming,
) -> std::result::Result<(), String>
where
    MF: Fn(u64) -> MFut,
    MFut: std::future::Future<Output = Result<[u8; 32]>>,
    JF: Fn(u64) -> JFut,
    JFut: std::future::Future<Output = Result<Vec<u8>>>,
{
    let Some(jid) = job_id else {
        return check_session_model_for(None, expected, template_id);
    };
    let (attempts, delay, per_attempt) =
        (timing.read_attempts, timing.read_delay, timing.per_attempt);
    // D24: a session the node's RPC has not seen yet reads as an all-zero model and
    // an all-zero record (status 0 = Active, host 0x0). That is "not visible yet",
    // not a verdict: re-read both every `gap` until the pair looks set or `budget`
    // has passed (no re-read starts after it), then judge the latest pair as D20
    // always did — so a session that never appears, or a genuinely model-less one,
    // is still refused (fail closed), only later. An Err, or a record that does
    // not decode, refuses at once.
    let start = tokio::time::Instant::now();
    let mut waited = false;
    let (model, snap) = loop {
        let (model, raw) = tokio::join!(
            read_with_retry(attempts, delay, per_attempt, || read_model(jid)),
            read_with_retry(attempts, delay, per_attempt, || read_session(jid)),
        );
        let model = model?;
        let snap = crate::training::accept::decode_session_snapshot(&raw?)?;
        if model != [0u8; 32] && !snap.host.is_zero() {
            if waited {
                tracing::info!(
                    "LTX job {jid}: session visible after {:.1} s (RPC lag)",
                    start.elapsed().as_secs_f64()
                );
            }
            break (model, snap);
        }
        if start.elapsed() >= timing.budget {
            break (model, snap);
        }
        tokio::time::sleep(timing.gap).await;
        waited = true;
        if start.elapsed() >= timing.budget {
            break (model, snap);
        }
    };
    check_session_model_for(Some(model), expected, template_id)?;
    let host = this_host
        .parse::<ethers::types::Address>()
        .map_err(|e| format!("this node's host address {this_host:?} is unreadable: {e}"))?;
    check_session_terms(&snap, host, job_tokens, tracked_tokens)
}

/// NM1 D20: one chain read, retried (`query_session_model` and
/// `query_session_jobs_raw` are single `eth_call`s with no retry of their own),
/// each attempt bounded by `per_attempt` — the provider has no request timeout,
/// and an RPC that never answers would otherwise hold the job task, its VRAM
/// permit and its pending-proof mark. Returns the last error as text after
/// `attempts` failures — the caller then refuses, fail closed.
pub async fn read_with_retry<T, F, Fut>(
    attempts: u32,
    delay: std::time::Duration,
    per_attempt: std::time::Duration,
    mut read: F,
) -> std::result::Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last = String::from("no attempt made");
    for attempt in 0..attempts.max(1) {
        if attempt > 0 && !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        match tokio::time::timeout(per_attempt, read()).await {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(e)) => last = format!("{e:#}"),
            Err(_) => last = format!("timed out after {} s", per_attempt.as_secs()),
        }
    }
    Err(format!(
        "chain read failed after {} attempts: {last}",
        attempts.max(1)
    ))
}

/// Canonical keccak256 of a JSON value: alphabetically sort all object keys (via
/// the repo's shared `sort_json_keys`, robust whether or not serde_json's
/// `preserve_order` feature is on), serialise compactly, then keccak256.
/// `templateHash`/`bundleHash` are NODE-AUTHORED and advertised; the client
/// ECHOES them (it never recomputes keccak from the graph JSON), so a
/// language-neutral JSON form is not required here. The cross-language
/// fixed-field commitments are `inputCommitment`/`sigDigest` (Phase 6).
fn canonical_keccak(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(&sort_json_keys(value)).expect("json re-serialises");
    format!("0x{}", hex::encode(ethers::utils::keccak256(bytes)))
}

/// Reject path-traversal / separator chars in a config-supplied path segment
/// (defence-in-depth: the allow-list is image-baked and trusted today, but this
/// keeps template loading safe if that assumption ever weakens).
fn validate_segment(seg: &str) -> Result<()> {
    if seg.is_empty() || seg.contains('/') || seg.contains('\\') || seg.contains("..") {
        return Err(anyhow!("invalid allow-list path segment {:?}", seg));
    }
    Ok(())
}

/// keccak256 over the canonical bundle with the `bundleHash` field removed.
fn compute_bundle_hash(bundle: &AllowListBundle) -> String {
    let mut value = serde_json::to_value(bundle).expect("bundle serialises");
    if let Some(obj) = value.as_object_mut() {
        obj.remove("bundleHash");
    }
    canonical_keccak(&value)
}
