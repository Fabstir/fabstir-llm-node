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

    /// The number of input images template `id` consumes — the `inputCommitment`
    /// format selector the handler validates `job.images.len()` against (M1a).
    /// `None` for an unknown id.
    pub fn image_inputs(&self, id: &str) -> Option<u32> {
        self.bundle
            .templates
            .iter()
            .find(|t| t.template_id == id)
            .map(|t| t.image_inputs)
    }

    /// The number of input videos template `id` consumes — the BL3 analogue of
    /// `image_inputs` the handler validates `job.videos.len()` against. `None`
    /// for an unknown id.
    pub fn video_inputs(&self, id: &str) -> Option<u32> {
        self.bundle
            .templates
            .iter()
            .find(|t| t.template_id == id)
            .map(|t| t.video_inputs)
    }

    pub fn bundle(&self) -> &AllowListBundle {
        &self.bundle
    }
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
