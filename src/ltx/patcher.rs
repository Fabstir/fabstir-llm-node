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

/// Patch `job`'s params into the pinned `graph`. `control_path` is the M1 hook for
/// control-image conditioning; ignored in M0.
///
/// Required handles (fail closed if absent): the positive prompt (`_meta.title ==
/// "Prompt"`) and at least one `RandomNoise` seed node. Optional (patched only if
/// present): `Width`, `Height`, `Frame Rate`. `frames` is advisory in this pass —
/// the pinned graph controls clip length until the EXR pass exposes a direct
/// frame-count handle.
pub fn patch(graph: &Graph, job: &LtxJob, _control_path: Option<&str>) -> Result<Graph> {
    let mut value = graph.0.clone();
    let obj = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("graph is not a node-id object"))?;

    // Seed: the wire allows a uint256-sized decimal string, but the sampler takes a
    // bounded integer — reject anything outside ComfyUI's u64 noise_seed range.
    let seed = job.seed_u256().map_err(|e| anyhow!(e))?;
    if seed > U256::from(u64::MAX) {
        return Err(anyhow!("seed {} exceeds the sampler's u64 range", job.seed));
    }
    let seed = seed.as_u64();

    // Required.
    patch_by_title(
        obj,
        "Prompt",
        "value",
        Value::from(job.prompt.clone()),
        true,
    )?;
    patch_by_class(obj, "RandomNoise", "noise_seed", Value::from(seed), true)?;
    // Optional (patched only where the pinned graph exposes them as literals).
    patch_by_title(obj, "Width", "value", Value::from(job.resolution.w), false)?;
    patch_by_title(obj, "Height", "value", Value::from(job.resolution.h), false)?;
    patch_by_title(obj, "Frame Rate", "value", Value::from(job.fps), false)?;

    Ok(Graph(value))
}

/// Set `key` on every node whose `_meta.title` equals `title`. If none match and
/// `required`, error (fail closed); if none match and optional, no-op.
fn patch_by_title(
    graph: &mut Map<String, Value>,
    title: &str,
    key: &str,
    value: Value,
    required: bool,
) -> Result<()> {
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
fn patch_by_class(
    graph: &mut Map<String, Value>,
    class: &str,
    key: &str,
    value: Value,
    required: bool,
) -> Result<()> {
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
) -> Result<()> {
    if ids.is_empty() {
        if required {
            return Err(anyhow!("template is missing the required {what}"));
        }
        return Ok(());
    }
    for id in ids {
        set_input(graph, &id, key, value.clone())?;
    }
    Ok(())
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
