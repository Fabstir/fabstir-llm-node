// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 4 param patcher tests, against the real pinned LTX template: substitution
//! only, by the template's own node names/types, no structural edits.

use fabstir_llm_node::ltx::patcher::patch;
use fabstir_llm_node::ltx::types::{LtxJob, OutputKind, Resolution};
use fabstir_llm_node::ltx::Graph;
use serde_json::Value;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");

fn fixture_graph() -> Graph {
    let raw = std::fs::read(format!("{DIR}/ltx-t2v-hdr/v1.json")).unwrap();
    Graph(serde_json::from_slice(&raw).unwrap())
}

fn job() -> LtxJob {
    LtxJob {
        template_id: "ltx-t2v-hdr".to_string(),
        template_hash: "0x00".to_string(),
        prompt: "a derelict spaceship corridor".to_string(),
        seed: "4815162342".to_string(),
        frames: 121,
        fps: 25,
        resolution: Resolution { w: 1280, h: 720 },
        lora: "ltx-iclora-hdr@v1".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
    }
}

fn node_by_title<'a>(g: &'a Graph, title: &str) -> &'a Value {
    g.0.as_object()
        .unwrap()
        .values()
        .find(|n| n.pointer("/_meta/title").and_then(Value::as_str) == Some(title))
        .unwrap()
}

fn nodes_by_class<'a>(g: &'a Graph, class: &str) -> Vec<&'a Value> {
    g.0.as_object()
        .unwrap()
        .values()
        .filter(|n| n.get("class_type").and_then(Value::as_str) == Some(class))
        .collect()
}

#[test]
fn test_patch_prompt_into_prompt_box() {
    let g = patch(&fixture_graph(), &job(), None).unwrap();
    let text = node_by_title(&g, "Prompt")
        .pointer("/inputs/value")
        .unwrap();
    assert_eq!(text.as_str().unwrap(), "a derelict spaceship corridor");
}

#[test]
fn test_patch_seed_into_every_randomnoise() {
    let g = patch(&fixture_graph(), &job(), None).unwrap();
    let rn = nodes_by_class(&g, "RandomNoise");
    assert!(!rn.is_empty(), "template has RandomNoise seed node(s)");
    for n in rn {
        assert_eq!(
            n.pointer("/inputs/noise_seed").unwrap().as_u64().unwrap(),
            4_815_162_342
        );
    }
}

#[test]
fn test_patch_dims_into_primitives() {
    let g = patch(&fixture_graph(), &job(), None).unwrap();
    assert_eq!(
        node_by_title(&g, "Width")
            .pointer("/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap(),
        1280
    );
    assert_eq!(
        node_by_title(&g, "Height")
            .pointer("/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap(),
        720
    );
    assert_eq!(
        node_by_title(&g, "Frame Rate")
            .pointer("/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap(),
        25
    );
}

#[test]
fn test_seed_out_of_range_rejected() {
    let mut j = job();
    j.seed = "18446744073709551616".to_string(); // > u64::MAX, valid uint256 on the wire
    assert!(patch(&fixture_graph(), &j, None).is_err());
}

#[test]
fn test_missing_required_prompt_is_error() {
    let mut g = fixture_graph();
    for n in g.0.as_object_mut().unwrap().values_mut() {
        if n.pointer("/_meta/title").and_then(Value::as_str) == Some("Prompt") {
            n.as_object_mut().unwrap().remove("_meta");
        }
    }
    assert!(
        patch(&g, &job(), None).is_err(),
        "missing prompt handle -> fail closed"
    );
}

#[test]
fn test_missing_required_seed_is_error() {
    let mut g = fixture_graph();
    for n in g.0.as_object_mut().unwrap().values_mut() {
        if n.get("class_type").and_then(Value::as_str) == Some("RandomNoise") {
            n["class_type"] = Value::from("NotARandomNoise");
        }
    }
    assert!(
        patch(&g, &job(), None).is_err(),
        "no seed node -> fail closed"
    );
}

#[test]
fn test_optional_dims_absent_still_ok() {
    let mut g = fixture_graph();
    for n in g.0.as_object_mut().unwrap().values_mut() {
        if matches!(
            n.pointer("/_meta/title").and_then(Value::as_str),
            Some("Width") | Some("Height") | Some("Frame Rate")
        ) {
            n.as_object_mut().unwrap().remove("_meta");
        }
    }
    assert!(
        patch(&g, &job(), None).is_ok(),
        "optional dims absent -> patch still succeeds"
    );
}

#[test]
fn test_refuses_to_patch_a_wired_connection() {
    let mut g = fixture_graph();
    for n in g.0.as_object_mut().unwrap().values_mut() {
        if n.pointer("/_meta/title").and_then(Value::as_str) == Some("Prompt") {
            n["inputs"]["value"] = serde_json::json!(["999", 0]);
        }
    }
    assert!(
        patch(&g, &job(), None).is_err(),
        "leaf-only: never overwrite a wired connection"
    );
}

#[test]
fn test_no_structural_edits() {
    let before = fixture_graph();
    let after = patch(&before, &job(), None).unwrap();
    let b = before.0.as_object().unwrap();
    let a = after.0.as_object().unwrap();
    assert_eq!(
        b.keys().collect::<std::collections::BTreeSet<_>>(),
        a.keys().collect::<std::collections::BTreeSet<_>>(),
        "same node ids"
    );
    for (id, bn) in b {
        assert_eq!(
            a[id].pointer("/class_type"),
            bn.pointer("/class_type"),
            "class_type {id} unchanged"
        );
    }
}
