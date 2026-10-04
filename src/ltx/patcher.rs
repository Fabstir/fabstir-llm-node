// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Param patcher for the pinned ComfyUI graph. Substitutes job values into the
//! graph by the LTX template's OWN node names/types (no operator renaming): the
//! positive prompt box titled `Prompt`, the `RandomNoise` seed node(s), and the
//! `Width`/`Height`/`Frame Rate` primitives. Value substitution ONLY: never
//! add/remove/rewire nodes, never touch `class_type`, never overwrite a wired
//! connection — so the pinned-hash provenance guarantee holds (the graph that
//! runs is the graph that was hashed, with only leaf input scalars changed) —
//! EXCEPT three sanctioned runtime structural edits, each census-first and
//! fail-closed: the `exr_output` sink's removal for non-EXR jobs, the deep
//! loader swap, and (NM1 D23) `drop_silent_audio`'s removal of a clip's audio
//! pass-through when its audio cannot pass. (The EXR lineariser is baked into
//! the templates, not a patcher edit.)

use anyhow::{anyhow, Result};
use ethers::types::U256;
use serde_json::{Map, Value};

use crate::ltx::template::Graph;
use crate::ltx::types::LtxJob;

/// Patch `job`'s params into the pinned `graph`. `image_names` are the ComfyUI
/// stored filenames for image-conditioned templates (M1a), assigned to the
/// `LoadImage` nodes in node-id order; pass `&[]` for t2v (no LoadImage nodes).
/// `video_names` are the stored filenames for video-conditioned templates
/// (BL3/BL4), assigned across the video-loader union (`LoadVideo` /
/// `VHS_LoadVideo`) the same way; pass `&[]` when none.
///
/// Required handles (fail closed if absent): the positive prompt (`_meta.title ==
/// "Prompt"`) and at least one seed node — `RandomNoise` (`noise_seed`) or a
/// plain `KSampler` (`seed`), across both classes (iclora's validated graph uses
/// a plain KSampler; re-plumbing it to the RandomNoise stack would change
/// sampling behaviour, so the patcher widened instead). Optional (patched only if
/// present): `Width`, `Height`, `Frame Rate`, `Duration`. `Duration` = the clip
/// length in whole seconds `(frames-1)/fps`; the pinned graph recomputes
/// `Duration * FrameRate + 1` into `EmptyLTXVLatentVideo.length` (iclora instead
/// slices the control video to `Duration` seconds), so patching both `Duration`
/// and `Frame Rate` makes the rendered length equal the billed `frames`.
pub fn patch(
    graph: &Graph,
    job: &LtxJob,
    image_names: &[String],
    video_names: &[String],
) -> Result<Graph> {
    let mut value = graph.0.clone();
    let obj = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("graph is not a node-id object"))?;

    // Duration derives from (frames, fps); `duration_secs()` fails closed on a
    // zero fps/frames (divide-by-zero / `frames - 1` underflow) before any patch.
    let duration_secs = job
        .duration_secs()
        .ok_or_else(|| anyhow!("invalid frames/fps: frames={}, fps={}", job.frames, job.fps))?;

    // Seed: the wire allows a uint256-sized decimal string, but the sampler takes a
    // bounded integer — reject anything outside ComfyUI's u64 noise_seed range.
    let seed = job.seed_u256().map_err(|e| anyhow!(e))?;
    if seed > U256::from(u64::MAX) {
        return Err(anyhow!("seed {} exceeds the sampler's u64 range", job.seed));
    }
    let seed = seed.as_u64();

    // Required.
    patch_prompt(obj, &job.prompt)?;
    // Seed: stamp every RandomNoise (`noise_seed`) AND every plain KSampler
    // (`seed`) — the same job seed everywhere is the determinism semantics.
    // Exact class_type equality means KSamplerSelect / SamplerCustomAdvanced are
    // untouched. Required ≥1 match across the two classes (fail closed).
    let seed_nodes = patch_by_class(obj, "RandomNoise", "noise_seed", Value::from(seed), false)?
        + patch_by_class(obj, "KSampler", "seed", Value::from(seed), false)?;
    if seed_nodes == 0 {
        return Err(anyhow!(
            "template is missing the required seed handle (RandomNoise or KSampler)"
        ));
    }
    // Optional (patched only where the pinned graph exposes them as literals).
    patch_by_title(obj, "Width", "value", Value::from(job.resolution.w), false)?;
    patch_by_title(obj, "Height", "value", Value::from(job.resolution.h), false)?;
    patch_by_title(obj, "Frame Rate", "value", Value::from(job.fps), false)?;
    // The pinned graph multiplies Duration back by Frame Rate (+1) into
    // EmptyLTXVLatentVideo.length, so patching BOTH makes the rendered clip length
    // equal the billed frame count by construction (the handler's
    // `validate_duration` guarantees (frames-1) % fps == 0). Same optional handle
    // as the dims — a synthetic graph without it is a no-op.
    patch_by_title(obj, "Duration", "value", Value::from(duration_secs), false)?;

    // Guide strength (opt-in): the one tunable the guided family lacked. The
    // pinned graphs carry LTXAddVideoICLoRAGuide.strength = 1.0 — maximum
    // source adherence — which is why "recolour this object" edits could not
    // take. Patched by CLASS, not title: ingredients retitles the node with
    // glyphs but the class is identical across all six guided templates. Fail
    // closed when the job carries a strength and the template has no guide
    // node (t2v/i2v/flf2v/iclora/upscale): billing a paid render whose knob
    // was silently ignored would be worse than rejecting it.
    if let Some(s) = job.strength {
        let n = patch_by_class(
            obj,
            "LTXAddVideoICLoRAGuide",
            "strength",
            Value::from(s),
            false,
        )?;
        if n == 0 {
            return Err(anyhow!(
                "strength was provided but template {} has no IC-LoRA guide node",
                job.template_id
            ));
        }
    }

    // CrossView camera (CV1): azimuth/elevation/distance patched by CLASS onto
    // the template's CrossViewWarp node. Same fail-closed contract as strength:
    // a camera sent to a template with no such node must reject, not bill with
    // the pose silently ignored. Handler validation owns the ranges.
    for (key, v) in [
        ("azimuth", job.azimuth),
        ("elevation", job.elevation),
        ("distance", job.distance),
    ] {
        if let Some(v) = v {
            let n = patch_by_class(obj, "CrossViewWarp", key, Value::from(v), false)?;
            if n == 0 {
                return Err(anyhow!(
                    "{key} was provided but template {} has no CrossViewWarp node",
                    job.template_id
                ));
            }
        }
    }

    // EXR master sink (A2): every template carries a RadianceDigitalCinemaWrite
    // (write_mode "Sequence") titled
    // "exr_output". For jobs that did NOT request `exr-frames` the node is
    // REMOVED from the runtime copy (one of the three sanctioned runtime structural edits — a
    // sink with no consumers, so nothing can dangle); one pinned template
    // serves both deliveries. When `exr-frames` IS requested the handle is
    // REQUIRED — fail closed, a paid EXR request must never quietly render
    // mp4-only.
    let exr_ids: Vec<String> = obj
        .iter()
        .filter(|(_, n)| n.pointer("/_meta/title").and_then(Value::as_str) == Some("exr_output"))
        .map(|(id, _)| id.clone())
        .collect();
    if job.output == crate::ltx::types::OutputKind::ExrFrames {
        if exr_ids.is_empty() {
            return Err(anyhow!(
                "exr-frames was requested but template {} has no exr_output sink",
                job.template_id
            ));
        }
    } else {
        for id in &exr_ids {
            obj.remove(id);
        }
    }

    // "Frame Count" (crossview): one titled INT feeds BOTH the VHS loader's
    // frame_load_cap and the latent length, so patching it makes billed ==
    // loaded == rendered by construction. Optional handle — templates that
    // derive length from the clip (edit family) simply don't have it.
    patch_by_title(obj, "Frame Count", "value", Value::from(job.frames), false)?;

    // Image inputs (M1a) and video inputs (BL3/BL4) bind through the ONE binder:
    // names land on matching loader nodes in id order, count fail-closed, `&[]`
    // a no-op. Videos span the loader-class union (core `LoadVideo` for iclora,
    // `VHS_LoadVideo` for the BL4 trio).
    bind_inputs(obj, &[("LoadImage", "image")], "image", image_names)?;
    bind_inputs(obj, VIDEO_LOADER_CLASSES, "video", video_names)?;

    // BL4: cap the VHS loader at the billed frame count — defence-in-depth atop
    // the handler's stsz gate (which already bounds the clip to [billed-1, billed],
    // so the cap can only trim the +1 case; `skip_first_frames`/`select_every_nth`/
    // `force_rate` are frozen neutral in the pinned graphs). No-op for templates
    // without a `VHS_LoadVideo`.
    patch_by_class(
        obj,
        "VHS_LoadVideo",
        "frame_load_cap",
        Value::from(job.frames),
        false,
    )?;

    // Deep-conform loader swap (v8.44.0, EXECUTION-DEEP-CONFORM.md): a job
    // carrying `inputWire` had its videos[0] staged as an EXR SEQUENCE
    // subfolder, and the pinned VHS_LoadVideo (8-bit cv2 decode) is replaced
    // in place by the float sequence reader. Runs LAST so the binder above
    // has already written the staged path into the loader's `video` input.
    if let Some(wire) = job.input_wire {
        if !DEEP_INPUT_CAPABLE.contains(&job.template_id.as_str()) {
            return Err(anyhow!(
                "inputWire was provided but template {} is not deep-input capable",
                job.template_id
            ));
        }
        // The swap needs the staged sequence path, which only exists on the
        // post-accept call (the pre-accept dry-run passes empty names and
        // stops at the capability check above).
        if !video_names.is_empty() {
            swap_deep_loader(obj, job, wire)?;
        }
    }

    Ok(Graph(value))
}

/// Templates proven compatible with the deep-conform loader swap: each carries
/// exactly one `VHS_LoadVideo` whose non-IMAGE outputs are consumed only by
/// the three known patterns the swap handles (frame_count -> literal billed,
/// audio -> dropped, VHS_VideoInfo fps chain -> literal job fps). iclora
/// (core `LoadVideo` + Video Slice) and crossview (warp pre-pass) are
/// deliberately ABSENT in v1 — their loader shapes differ and nothing here
/// has been audited against them.
pub const DEEP_INPUT_CAPABLE: &[&str] = &[
    "ltx-daynight-hdr",
    "ltx-edit-hdr",
    "ltx-outpaint-hdr",
    "ltx-restore-hdr",
    "ltx-upscale-hdr",
    "ltx-water-hdr",
];

/// The deep-conform structural edit (one of the patcher's three sanctioned
/// runtime edits, beside the exr_output removal and D23's `drop_silent_audio`;
/// the EXR lineariser is baked into the templates): replace the pinned
/// `VHS_LoadVideo` with `RadianceDigitalCinemaRead` reading the staged EXR
/// subfolder at float, pass-through colour ("Linear (sRGB)" applies no
/// transform — the wire already carries what the graph expects). The census
/// runs FIRST: any consumer of a loader output slot without an explicit rule
/// fails the job before the graph is touched.
/// Deep-swap-only variant of `set_input`: OVERWRITES a wired connection with a
/// literal or a new link. The general patcher must never do this — the
/// never-overwrite-a-connection guarantee is load-bearing — but the sanctioned
/// deep swap exists precisely to replace loader links, and only after the
/// census proved every one of them matches an explicit rule.
fn force_input(
    graph: &mut Map<String, Value>,
    node_id: &str,
    key: &str,
    value: Value,
) -> Result<()> {
    let inputs = graph
        .get_mut(node_id)
        .and_then(|n| n.get_mut("inputs"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("node {node_id} has no inputs object"))?;
    if !inputs.contains_key(key) {
        return Err(anyhow!("node {node_id} has no input {:?} to replace", key));
    }
    inputs.insert(key.to_string(), value);
    Ok(())
}

fn swap_deep_loader(
    obj: &mut Map<String, Value>,
    job: &LtxJob,
    wire: crate::ltx::types::InputWire,
) -> Result<()> {
    let loaders: Vec<String> = obj
        .iter()
        .filter(|(_, n)| n.get("class_type").and_then(Value::as_str) == Some("VHS_LoadVideo"))
        .map(|(id, _)| id.clone())
        .collect();
    if loaders.len() != 1 {
        return Err(anyhow!(
            "deep swap expects exactly one VHS_LoadVideo in template {}, found {}",
            job.template_id,
            loaders.len()
        ));
    }
    let loader_id = loaders[0].clone();
    let staged = obj
        .get(&loader_id)
        .and_then(|n| n.pointer("/inputs/video"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("deep swap: staged sequence path missing on loader {loader_id}"))?
        .to_string();

    // Census of every consumer of the loader's outputs.
    let mut videoinfo_ids: Vec<String> = Vec::new();
    let mut count_edits: Vec<(String, String)> = Vec::new();
    let mut audio_drops: Vec<(String, String)> = Vec::new();
    let mut image_edits: Vec<(String, String)> = Vec::new();
    for (nid, n) in obj.iter() {
        let Some(inputs) = n.get("inputs").and_then(Value::as_object) else {
            continue;
        };
        for (key, v) in inputs {
            let Some(arr) = v.as_array() else { continue };
            if arr.len() != 2 || arr[0].as_str() != Some(loader_id.as_str()) {
                continue;
            }
            match arr[1].as_u64() {
                Some(0) => image_edits.push((nid.clone(), key.clone())),
                Some(1) => count_edits.push((nid.clone(), key.clone())),
                Some(2) => {
                    if key != "audio" {
                        return Err(anyhow!(
                            "deep swap: loader audio output consumed by unexpected input {nid}.{key}"
                        ));
                    }
                    audio_drops.push((nid.clone(), key.clone()));
                }
                Some(3) => {
                    if n.get("class_type").and_then(Value::as_str) != Some("VHS_VideoInfo") {
                        return Err(anyhow!(
                            "deep swap: loader video_info consumed by non-VideoInfo node {nid}"
                        ));
                    }
                    videoinfo_ids.push(nid.clone());
                }
                other => {
                    return Err(anyhow!(
                        "deep swap: loader output slot {other:?} consumed by {nid}.{key} — no rule for it"
                    ))
                }
            }
        }
    }
    // Every consumer of a VideoInfo output takes the literal job fps (the
    // chain only ever carried source fps, which the job validates anyway).
    let mut fps_edits: Vec<(String, String)> = Vec::new();
    for vid in &videoinfo_ids {
        for (nid, n) in obj.iter() {
            let Some(inputs) = n.get("inputs").and_then(Value::as_object) else {
                continue;
            };
            for (key, v) in inputs {
                if let Some(arr) = v.as_array() {
                    if arr.len() == 2 && arr[0].as_str() == Some(vid.as_str()) {
                        fps_edits.push((nid.clone(), key.clone()));
                    }
                }
            }
        }
    }

    for (nid, key) in count_edits {
        force_input(obj, &nid, &key, Value::from(job.frames))?;
    }
    for (nid, key) in fps_edits {
        force_input(obj, &nid, &key, Value::from(job.fps))?;
    }
    for (nid, key) in audio_drops {
        if let Some(m) = obj
            .get_mut(&nid)
            .and_then(|n| n.get_mut("inputs"))
            .and_then(Value::as_object_mut)
        {
            m.remove(&key);
        }
    }
    for vid in videoinfo_ids {
        obj.remove(&vid);
    }

    // The swap itself: same node id, so IMAGE consumers keep their links.
    obj.insert(
        loader_id.clone(),
        serde_json::json!({
            "class_type": "RadianceDigitalCinemaRead",
            "inputs": {
                "source_path": staged,
                "start_frame": 1,
                "frame_limit": job.frames,
                "input_colorspace": "Linear (sRGB)",
                "fps_override": f64::from(job.fps),
            },
            "_meta": {"title": "deep_input"}
        }),
    );

    // Linear wire: the graph must still see display-encoded values — insert
    // the x^(1/2.2) encode shim (Radiance gamma g applies x^(1/g), so gamma
    // 2.2 IS the encode) between the reader and every IMAGE consumer.
    if wire == crate::ltx::types::InputWire::ExrseqLinear {
        let shim_id = "92";
        if obj.contains_key(shim_id) {
            return Err(anyhow!(
                "deep swap: shim id {shim_id} already taken in template {}",
                job.template_id
            ));
        }
        for (nid, key) in image_edits {
            force_input(obj, &nid, &key, serde_json::json!([shim_id, 0]))?;
        }
        obj.insert(
            shim_id.to_string(),
            serde_json::json!({
                "class_type": "Float32ColorCorrect",
                "inputs": {
                    "image": [loader_id, 0],
                    "exposure": 0, "contrast": 1, "brightness": 0, "saturation": 1,
                    "gamma": 2.2,
                    "lift_r": 0, "lift_g": 0, "lift_b": 0,
                    "gain_r": 1, "gain_g": 1, "gain_b": 1,
                    "luma_space": "Rec.709 / sRGB", "clamp_output": false
                },
                "_meta": {"title": "deep_input encode (x^1/2.2)"}
            }),
        );
    }
    Ok(())
}

/// The video-loader classes and each one's filename input key. Control clips
/// bind across the UNION of these in node-id order, so a job's `videos[i]`
/// lands deterministically whichever loader class the pinned graph uses
/// (iclora carries a core `LoadVideo`; the BL4 trio a VHS `VHS_LoadVideo`).
const VIDEO_LOADER_CLASSES: &[(&str, &str)] = &[("LoadVideo", "file"), ("VHS_LoadVideo", "video")];

/// The one input binder: assign `names[i]` to the i-th node whose `class_type`
/// is in `classes` (id-ordered lexicographically — i2v has one `LoadImage`;
/// flf2v `31` < `39` binds (first, last)), each through its class's own input
/// key. Fails CLOSED on a count mismatch; an empty `names` is a no-op (t2v has
/// no loader at all). Images pass the one-element slice; videos the union.
fn bind_inputs(
    obj: &mut Map<String, Value>,
    classes: &[(&str, &str)],
    noun: &str,
    names: &[String],
) -> Result<()> {
    if names.is_empty() {
        return Ok(());
    }
    let mut loaders: Vec<(String, &str)> = obj
        .iter()
        .filter_map(|(id, n)| {
            let class = n.get("class_type").and_then(Value::as_str)?;
            classes
                .iter()
                .find(|(c, _)| *c == class)
                .map(|(_, key)| (id.clone(), *key))
        })
        .collect();
    loaders.sort();
    if loaders.len() != names.len() {
        return Err(anyhow!(
            "template has {} {noun} loader node(s) but {} {noun} name(s) supplied",
            loaders.len(),
            names.len()
        ));
    }
    for (name, (id, key)) in names.iter().zip(loaders.iter()) {
        set_input(obj, id, key, Value::from(name.clone()))?;
    }
    Ok(())
}

/// Set the positive prompt on the `Prompt`-titled node(s), writing whichever leaf
/// text input the node exposes: `value` (a `PrimitiveStringMultiline`, as t2v/i2v
/// use) or `text` (a `CLIPTextEncode`, as flf2v's curated positive node uses).
/// Required: fail closed if there is no `Prompt` handle, or it has neither leaf
/// (which also preserves the never-overwrite-a-wired-connection guarantee).
fn patch_prompt(graph: &mut Map<String, Value>, prompt: &str) -> Result<()> {
    let ids: Vec<String> = graph
        .iter()
        .filter(|(_, n)| n.pointer("/_meta/title").and_then(Value::as_str) == Some("Prompt"))
        .map(|(id, _)| id.clone())
        .collect();
    if ids.is_empty() {
        return Err(anyhow!(
            "template is missing the required handle \"Prompt\""
        ));
    }
    for id in ids {
        let key = prompt_input_key(graph, &id)?;
        set_input(graph, &id, key, Value::from(prompt.to_string()))?;
    }
    Ok(())
}

/// The leaf text-input key on a `Prompt` node: `value` if present as a leaf, else
/// `text`. Errors if neither is a patchable leaf, so a wired connection can never
/// be overwritten (same guarantee as [`set_input`]).
fn prompt_input_key(graph: &Map<String, Value>, node_id: &str) -> Result<&'static str> {
    let inputs = graph
        .get(node_id)
        .and_then(|n| n.get("inputs"))
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("node {node_id} has no inputs object"))?;
    for key in ["value", "text"] {
        if inputs.get(key).is_some_and(|v| !v.is_array()) {
            return Ok(key);
        }
    }
    Err(anyhow!(
        "Prompt node {node_id} has no patchable leaf `value`/`text` input"
    ))
}

/// Set `key` on every node whose `_meta.title` equals `title`. If none match and
/// `required`, error (fail closed); if none match and optional, no-op.
fn patch_by_title(
    graph: &mut Map<String, Value>,
    title: &str,
    key: &str,
    value: Value,
    required: bool,
) -> Result<usize> {
    let ids: Vec<String> = graph
        .iter()
        .filter(|(_, n)| n.pointer("/_meta/title").and_then(Value::as_str) == Some(title))
        .map(|(id, _)| id.clone())
        .collect();
    apply(
        graph,
        ids,
        key,
        value,
        required,
        &format!("handle {title:?}"),
    )
}

/// Set `key` on every node of `class_type` (e.g. all `RandomNoise` seeds get the
/// same job seed — deterministic). Required-if-none like [`patch_by_title`].
/// Returns how many nodes matched, so a caller can require ≥1 match ACROSS
/// several classes (the widened seed handle).
fn patch_by_class(
    graph: &mut Map<String, Value>,
    class: &str,
    key: &str,
    value: Value,
    required: bool,
) -> Result<usize> {
    let ids: Vec<String> = graph
        .iter()
        .filter(|(_, n)| n.get("class_type").and_then(Value::as_str) == Some(class))
        .map(|(id, _)| id.clone())
        .collect();
    apply(graph, ids, key, value, required, &format!("{class} node"))
}

fn apply(
    graph: &mut Map<String, Value>,
    ids: Vec<String>,
    key: &str,
    value: Value,
    required: bool,
    what: &str,
) -> Result<usize> {
    if ids.is_empty() {
        if required {
            return Err(anyhow!("template is missing the required {what}"));
        }
        return Ok(0);
    }
    let n = ids.len();
    for id in ids {
        set_input(graph, &id, key, value.clone())?;
    }
    Ok(n)
}

/// Overwrite an EXISTING leaf input value. Refuses to create a new input key
/// (substitution only) and refuses to overwrite a `[node, slot]` wired connection,
/// so a value patch can never sever the pinned graph's wiring.
fn set_input(graph: &mut Map<String, Value>, node_id: &str, key: &str, value: Value) -> Result<()> {
    let inputs = graph
        .get_mut(node_id)
        .and_then(|n| n.get_mut("inputs"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("node {node_id} has no inputs object"))?;
    match inputs.get(key) {
        None => return Err(anyhow!("node {node_id} has no input {:?} to patch", key)),
        Some(v) if v.is_array() => {
            return Err(anyhow!(
                "node {node_id} input {:?} is a wired connection, not a leaf",
                key
            ))
        }
        Some(_) => {}
    }
    inputs.insert(key.to_string(), value);
    Ok(())
}

/// Every (consumer node id, input key) linked to output `slot` of node `id`.
fn consumers_of(obj: &Map<String, Value>, id: &str, slot: u64) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (nid, n) in obj {
        let Some(inputs) = n.get("inputs").and_then(Value::as_object) else {
            continue;
        };
        for (key, v) in inputs {
            if let Some(arr) = v.as_array() {
                if arr.len() == 2 && arr[0].as_str() == Some(id) && arr[1].as_u64() == Some(slot) {
                    out.push((nid.clone(), key.clone()));
                }
            }
        }
    }
    out
}

fn class_of<'a>(obj: &'a Map<String, Value>, id: &str) -> Option<&'a str> {
    obj.get(id)
        .and_then(|n| n.get("class_type"))
        .and_then(Value::as_str)
}

/// NM1 D23 — a runtime structural edit beside the patcher's other two (`exr_output` removal and the
/// deep loader swap; the EXR lineariser is baked into templates), applied AFTER [`patch`]: for a
/// control clip whose audio cannot pass through (`mp4::audio_passthrough_ok` false or unreadable),
/// remove the clip's audio pass-through instead of letting the job fail. ComfyUI 0.38 iterates a
/// linked `VHS_LoadVideo` audio output and VHS raises on silent/empty/non-mono-stereo audio; core
/// `SaveVideo` fails after the full render on 3/4/8 channels (sdr2hdr).
///
/// - VHS path: for every `VHS_LoadVideo` whose `inputs.video` is in `silent_videos`, each consumer of
///   its slot-2 (audio) output must be a `VHS_VideoCombine` input named `audio` (anything else fails
///   closed — census first, as the deep swap's) and is removed (`audio` is optional on VideoCombine).
/// - Core path: for every core `LoadVideo` whose `inputs.file` is in `silent_videos`, every
///   `GetVideoComponents` fed by it — directly or through `Video Slice` nodes — has each consumer of
///   its slot-1 (audio) output censused (must be `CreateVideo.audio`) and removed. Only sdr2hdr links
///   it; iclora's sits behind `Video Slice` with slot 1 unlinked (its delivered audio is the model's).
///
/// Nothing else is touched; an empty `silent_videos` returns the graph unchanged.
pub fn drop_silent_audio(graph: &Graph, silent_videos: &[String]) -> Result<Graph> {
    let mut value = graph.0.clone();
    if silent_videos.is_empty() {
        return Ok(Graph(value));
    }
    let obj = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("graph is not a node-id object"))?;
    let names_silent = |n: &Value, key: &str| {
        n.get("inputs")
            .and_then(|i| i.get(key))
            .and_then(Value::as_str)
            .is_some_and(|v| silent_videos.iter().any(|s| s == v))
    };

    let mut removals: Vec<(String, String)> = Vec::new();
    let vhs: Vec<String> = obj
        .iter()
        .filter(|(_, n)| {
            n.get("class_type").and_then(Value::as_str) == Some("VHS_LoadVideo")
                && names_silent(n, "video")
        })
        .map(|(id, _)| id.clone())
        .collect();
    for id in &vhs {
        for (nid, key) in consumers_of(obj, id, 2) {
            if class_of(obj, &nid) != Some("VHS_VideoCombine") || key != "audio" {
                return Err(anyhow!(
                    "silent-audio drop: VHS_LoadVideo {id}'s audio feeds {nid}.{key} — no rule for it"
                ));
            }
            removals.push((nid, key));
        }
    }

    let core: Vec<String> = obj
        .iter()
        .filter(|(_, n)| {
            n.get("class_type").and_then(Value::as_str) == Some("LoadVideo")
                && names_silent(n, "file")
        })
        .map(|(id, _)| id.clone())
        .collect();
    for id in &core {
        // Follow the video through any `Video Slice` chain to the GetVideoComponents it reaches.
        let mut frontier = vec![id.clone()];
        let mut seen: Vec<String> = Vec::new();
        let mut components: Vec<String> = Vec::new();
        while let Some(node) = frontier.pop() {
            if seen.contains(&node) {
                continue;
            }
            seen.push(node.clone());
            for (nid, _) in consumers_of(obj, &node, 0) {
                match class_of(obj, &nid) {
                    Some("Video Slice") => frontier.push(nid),
                    Some("GetVideoComponents") if !components.contains(&nid) => {
                        components.push(nid)
                    }
                    _ => {}
                }
            }
        }
        for gvc in &components {
            for (nid, key) in consumers_of(obj, gvc, 1) {
                if class_of(obj, &nid) != Some("CreateVideo") || key != "audio" {
                    return Err(anyhow!(
                        "silent-audio drop: GetVideoComponents {gvc}'s audio feeds {nid}.{key} — no rule for it"
                    ));
                }
                removals.push((nid, key));
            }
        }
    }

    for (nid, key) in removals {
        if let Some(m) = obj
            .get_mut(&nid)
            .and_then(|n| n.get_mut("inputs"))
            .and_then(Value::as_object_mut)
        {
            m.remove(&key);
        }
    }
    Ok(Graph(value))
}
