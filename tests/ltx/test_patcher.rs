// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 4 param patcher tests, against the real pinned LTX template: substitution
//! only, by the template's own node names/types, no structural edits.

use fabstir_llm_node::ltx::patcher::patch;
use fabstir_llm_node::ltx::types::{LtxJob, OutputKind, Resolution};
use fabstir_llm_node::ltx::Graph;
use serde_json::Value;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");
const ARCHIVE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/archive/comfyui");

fn fixture_graph() -> Graph {
    let raw = std::fs::read(format!("{DIR}/ltx-t2v-hdr/v1.json")).unwrap();
    Graph(serde_json::from_slice(&raw).unwrap())
}

/// The real exported i2v graph (one `LoadImage`, node `269`).
fn i2v_graph() -> Graph {
    let raw = std::fs::read(format!("{ARCHIVE}/video_ltx2_3_i2v_20260701.json")).unwrap();
    Graph(serde_json::from_slice(&raw).unwrap())
}

/// The curated, pinned flf2v graph (two `LoadImage` 31/39; positive prompt is the
/// `CLIPTextEncode` 129:128 retitled `Prompt`; `Height`/`Frame Rate` retitled).
fn flf2v_graph() -> Graph {
    let raw = std::fs::read(format!("{DIR}/ltx-flf2v-hdr/v1.json")).unwrap();
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
    let g = patch(&fixture_graph(), &job(), &[]).unwrap();
    let text = node_by_title(&g, "Prompt")
        .pointer("/inputs/value")
        .unwrap();
    assert_eq!(text.as_str().unwrap(), "a derelict spaceship corridor");
}

#[test]
fn test_patch_seed_into_every_randomnoise() {
    let g = patch(&fixture_graph(), &job(), &[]).unwrap();
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
    let g = patch(&fixture_graph(), &job(), &[]).unwrap();
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
    assert!(patch(&fixture_graph(), &j, &[]).is_err());
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
        patch(&g, &job(), &[]).is_err(),
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
        patch(&g, &job(), &[]).is_err(),
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
        patch(&g, &job(), &[]).is_ok(),
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
        patch(&g, &job(), &[]).is_err(),
        "leaf-only: never overwrite a wired connection"
    );
}

#[test]
fn test_patch_single_loadimage() {
    // i2v: the one image name lands on the single LoadImage node (269).
    let g = patch(&i2v_graph(), &job(), &["egyptian.png".to_string()]).unwrap();
    assert_eq!(
        g.0.pointer("/269/inputs/image").unwrap().as_str().unwrap(),
        "egyptian.png"
    );
}

#[test]
fn test_patch_two_loadimages_by_nodeid_order() {
    // flf2v-shaped: two LoadImage nodes; images[0] -> the lexicographically-first
    // node id (31 = first frame), images[1] -> the next (39 = last frame). A
    // faithful synthetic graph (the real flf2v uses a CLIPTextEncode prompt the M0
    // scalar patcher does not drive; here we isolate the LoadImage ordering rule).
    let graph = Graph(serde_json::json!({
        "39": { "class_type": "LoadImage", "_meta": { "title": "Load Last Frame" },
                "inputs": { "image": "old_last.png" } },
        "31": { "class_type": "LoadImage", "_meta": { "title": "Load First Frame" },
                "inputs": { "image": "old_first.png" } },
        "p":  { "class_type": "PrimitiveStringMultiline", "_meta": { "title": "Prompt" },
                "inputs": { "value": "" } },
        "n":  { "class_type": "RandomNoise", "_meta": { "title": "RandomNoise" },
                "inputs": { "noise_seed": 0 } }
    }));
    let names = vec!["first.png".to_string(), "last.png".to_string()];
    let g = patch(&graph, &job(), &names).unwrap();
    assert_eq!(g.0.pointer("/31/inputs/image").unwrap(), "first.png");
    assert_eq!(g.0.pointer("/39/inputs/image").unwrap(), "last.png");
}

#[test]
fn test_patch_loadimage_refuses_wired() {
    // A LoadImage.image wired to another node's output must never be overwritten
    // by a value patch (same leaf-only guarantee as the scalar handles).
    let mut g = i2v_graph();
    g.0["269"]["inputs"]["image"] = serde_json::json!(["12", 0]);
    assert!(patch(&g, &job(), &["x.png".to_string()]).is_err());
}

#[test]
fn test_patch_flf2v_prompt_images_and_dims() {
    // flf2v: two images, and the positive prompt is a CLIPTextEncode (.text), not
    // a PrimitiveStringMultiline (.value) — the patcher must drive both.
    let names = vec!["first.png".to_string(), "last.png".to_string()];
    let g = patch(&flf2v_graph(), &job(), &names).unwrap();
    // Prompt -> the POSITIVE CLIPTextEncode's .text (129:128).
    assert_eq!(
        g.0.pointer("/129:128/inputs/text")
            .unwrap()
            .as_str()
            .unwrap(),
        "a derelict spaceship corridor"
    );
    // The negative prompt (129:112) is baked into the pin, untouched.
    assert_ne!(
        g.0.pointer("/129:112/inputs/text")
            .unwrap()
            .as_str()
            .unwrap(),
        "a derelict spaceship corridor"
    );
    // Images by node-id ascending: 31 = first, 39 = last.
    assert_eq!(g.0.pointer("/31/inputs/image").unwrap(), "first.png");
    assert_eq!(g.0.pointer("/39/inputs/image").unwrap(), "last.png");
    // Dims/fps via the retitled Width / Height / Frame Rate handles.
    assert_eq!(
        g.0.pointer("/129:113/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap(),
        1280
    );
    assert_eq!(
        g.0.pointer("/129:98/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap(),
        720
    );
    assert_eq!(
        g.0.pointer("/129:114/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap(),
        25
    );
    // Seed into RandomNoise.
    assert_eq!(
        g.0.pointer("/129:100/inputs/noise_seed")
            .unwrap()
            .as_u64()
            .unwrap(),
        4_815_162_342
    );
}

#[test]
fn test_patch_loadimage_count_mismatch_errors() {
    // Defence in depth: image name count must equal the template's LoadImage count.
    let two = vec!["a.png".to_string(), "b.png".to_string()];
    assert!(
        patch(&i2v_graph(), &job(), &two).is_err(),
        "1 LoadImage vs 2 names -> fail closed"
    );
}

#[test]
fn test_no_images_no_op() {
    // t2v: no image names, no LoadImage nodes. Scalars still patched; nothing added.
    let g = patch(&fixture_graph(), &job(), &[]).unwrap();
    assert_eq!(
        node_by_title(&g, "Prompt")
            .pointer("/inputs/value")
            .unwrap()
            .as_str()
            .unwrap(),
        "a derelict spaceship corridor"
    );
    assert!(
        nodes_by_class(&g, "LoadImage").is_empty(),
        "t2v has (and gains) no LoadImage node"
    );
}

/// Clone the base job with a specific (frames, fps) so the Duration derivation
/// `(frames-1)/fps` is exercised with clean, whole-second values.
fn job_frames_fps(frames: u32, fps: u32) -> LtxJob {
    LtxJob {
        frames,
        fps,
        ..job()
    }
}

#[test]
fn test_patch_duration_t2v() {
    // 10 s @ 24 fps -> frames 241 -> Duration 10 (the pinned graph's a*b+1 then
    // recomputes length = 10*24+1 = 241, equal to the billed frame count).
    let g = patch(&fixture_graph(), &job_frames_fps(241, 24), &[]).unwrap();
    assert_eq!(
        node_by_title(&g, "Duration")
            .pointer("/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap(),
        10
    );
}

#[test]
fn test_patch_duration_i2v() {
    // 15 s @ 25 fps -> frames 376 -> Duration 15, on the real i2v graph.
    let g = patch(
        &i2v_graph(),
        &job_frames_fps(376, 25),
        &["egyptian.png".to_string()],
    )
    .unwrap();
    assert_eq!(
        node_by_title(&g, "Duration")
            .pointer("/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap(),
        15
    );
}

#[test]
fn test_patch_duration_flf2v() {
    // 5 s @ 48 fps -> frames 241 -> Duration 5, on the curated flf2v graph.
    let names = vec!["first.png".to_string(), "last.png".to_string()];
    let g = patch(&flf2v_graph(), &job_frames_fps(241, 48), &names).unwrap();
    assert_eq!(
        node_by_title(&g, "Duration")
            .pointer("/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap(),
        5
    );
}

#[test]
fn test_patch_panic_guard_fps_zero() {
    // fps 0 would divide-by-zero in (frames-1)/fps -> fail closed, no panic.
    assert!(patch(&fixture_graph(), &job_frames_fps(121, 0), &[]).is_err());
}

#[test]
fn test_patch_panic_guard_frames_zero() {
    // frames 0 would underflow (frames-1) on u32 -> fail closed, no panic.
    assert!(patch(&fixture_graph(), &job_frames_fps(0, 24), &[]).is_err());
}

/// Every pinned template must carry the Duration wiring the patcher relies on: a
/// `Duration`-titled PrimitiveInt feeding an `a * b + 1` ComfyMathExpression whose
/// other input is the `Frame Rate` node, whose output drives an
/// `EmptyLTXVLatentVideo.length`. If a re-pin ever severs this, billed frames and
/// rendered length would diverge silently — so guard it structurally.
#[test]
fn test_all_templates_have_duration_wiring() {
    for id in ["ltx-t2v-hdr", "ltx-i2v-hdr", "ltx-flf2v-hdr"] {
        let raw = std::fs::read(format!("{DIR}/{id}/v1.json")).unwrap();
        let v: Value = serde_json::from_slice(&raw).unwrap();
        let obj = v.as_object().unwrap();
        let id_by_title = |title: &str| -> Option<String> {
            obj.iter()
                .find(|(_, n)| n.pointer("/_meta/title").and_then(Value::as_str) == Some(title))
                .map(|(nid, _)| nid.clone())
        };
        let dur = id_by_title("Duration").unwrap_or_else(|| panic!("{id}: Duration node"));
        let fr = id_by_title("Frame Rate").unwrap_or_else(|| panic!("{id}: Frame Rate node"));
        assert_eq!(
            obj[&dur].pointer("/class_type").and_then(Value::as_str),
            Some("PrimitiveInt"),
            "{id}: Duration is a PrimitiveInt leaf"
        );
        // The a*b+1 math node fed by Duration (a) and Frame Rate (b).
        let math_id = obj
            .iter()
            .find_map(|(nid, n)| {
                if n.get("class_type").and_then(Value::as_str) != Some("ComfyMathExpression") {
                    return None;
                }
                let expr = n
                    .pointer("/inputs/expression")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !expr.replace(' ', "").contains("a*b+1") {
                    return None;
                }
                let a = n
                    .pointer("/inputs/values.a")
                    .and_then(|x| x.get(0))
                    .and_then(Value::as_str);
                let b = n
                    .pointer("/inputs/values.b")
                    .and_then(|x| x.get(0))
                    .and_then(Value::as_str);
                (a == Some(dur.as_str()) && b == Some(fr.as_str())).then(|| nid.clone())
            })
            .unwrap_or_else(|| panic!("{id}: a*b+1 math fed by Duration and Frame Rate"));
        // That math output feeds at least one EmptyLTXVLatentVideo.length.
        let feeds_length = obj.values().any(|n| {
            n.get("class_type").and_then(Value::as_str) == Some("EmptyLTXVLatentVideo")
                && n.pointer("/inputs/length")
                    .and_then(|x| x.get(0))
                    .and_then(Value::as_str)
                    == Some(math_id.as_str())
        });
        assert!(
            feeds_length,
            "{id}: math output feeds EmptyLTXVLatentVideo.length"
        );
    }
}

#[test]
fn test_no_structural_edits() {
    let before = fixture_graph();
    let after = patch(&before, &job(), &[]).unwrap();
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
