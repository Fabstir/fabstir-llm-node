// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Param patcher for the pinned ComfyUI graph. Substitutes job values into the
//! graph by the LTX template's OWN node names/types (no operator renaming): the
//! positive prompt box titled `Prompt`, the `RandomNoise` seed node(s), and the
//! `Width`/`Height`/`Frame Rate` primitives. Value substitution ONLY: never
//! add/remove/rewire nodes, never touch `class_type`, never overwrite a wired
//! connection — so the pinned-hash provenance guarantee holds (the graph that
//! runs is the graph that was hashed, with only leaf input scalars changed).

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
        let n = patch_by_class(obj, "LTXAddVideoICLoRAGuide", "strength", Value::from(s), false)?;
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

    Ok(Graph(value))
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
