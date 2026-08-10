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
        videos: None,
        strength: None,
        azimuth: None,
        elevation: None,
        distance: None,
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
    let g = patch(&fixture_graph(), &job(), &[], &[]).unwrap();
    let text = node_by_title(&g, "Prompt")
        .pointer("/inputs/value")
        .unwrap();
    assert_eq!(text.as_str().unwrap(), "a derelict spaceship corridor");
}

#[test]
fn test_patch_seed_into_every_randomnoise() {
    let g = patch(&fixture_graph(), &job(), &[], &[]).unwrap();
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
    let g = patch(&fixture_graph(), &job(), &[], &[]).unwrap();
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
    assert!(patch(&fixture_graph(), &j, &[], &[]).is_err());
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
        patch(&g, &job(), &[], &[]).is_err(),
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
        patch(&g, &job(), &[], &[]).is_err(),
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
        patch(&g, &job(), &[], &[]).is_ok(),
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
        patch(&g, &job(), &[], &[]).is_err(),
        "leaf-only: never overwrite a wired connection"
    );
}

#[test]
fn test_patch_single_loadimage() {
    // i2v: the one image name lands on the single LoadImage node (269).
    let g = patch(&i2v_graph(), &job(), &["egyptian.png".to_string()], &[]).unwrap();
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
    let g = patch(&graph, &job(), &names, &[]).unwrap();
    assert_eq!(g.0.pointer("/31/inputs/image").unwrap(), "first.png");
    assert_eq!(g.0.pointer("/39/inputs/image").unwrap(), "last.png");
}

#[test]
fn test_patch_loadimage_refuses_wired() {
    // A LoadImage.image wired to another node's output must never be overwritten
    // by a value patch (same leaf-only guarantee as the scalar handles).
    let mut g = i2v_graph();
    g.0["269"]["inputs"]["image"] = serde_json::json!(["12", 0]);
    assert!(patch(&g, &job(), &["x.png".to_string()], &[]).is_err());
}

#[test]
fn test_patch_flf2v_prompt_images_and_dims() {
    // flf2v: two images, and the positive prompt is a CLIPTextEncode (.text), not
    // a PrimitiveStringMultiline (.value) — the patcher must drive both.
    let names = vec!["first.png".to_string(), "last.png".to_string()];
    let g = patch(&flf2v_graph(), &job(), &names, &[]).unwrap();
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
        patch(&i2v_graph(), &job(), &two, &[]).is_err(),
        "1 LoadImage vs 2 names -> fail closed"
    );
}

#[test]
fn test_no_images_no_op() {
    // t2v: no image names, no LoadImage nodes. Scalars still patched; nothing added.
    let g = patch(&fixture_graph(), &job(), &[], &[]).unwrap();
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
    let g = patch(&fixture_graph(), &job_frames_fps(241, 24), &[], &[]).unwrap();
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
        &[],
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
    let g = patch(&flf2v_graph(), &job_frames_fps(241, 48), &names, &[]).unwrap();
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
    assert!(patch(&fixture_graph(), &job_frames_fps(121, 0), &[], &[]).is_err());
}

#[test]
fn test_patch_panic_guard_frames_zero() {
    // frames 0 would underflow (frames-1) on u32 -> fail closed, no panic.
    assert!(patch(&fixture_graph(), &job_frames_fps(0, 24), &[], &[]).is_err());
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
        // That math output must feed BOTH the video latent `length` AND the audio
        // `frames_number` (video and audio in lockstep) — a re-export that drops
        // or rewires only one of the two still fails CI.
        let feeds = |class: &str, input: &str| {
            obj.values().any(|n| {
                n.get("class_type").and_then(Value::as_str) == Some(class)
                    && n.pointer(&format!("/inputs/{input}"))
                        .and_then(|x| x.get(0))
                        .and_then(Value::as_str)
                        == Some(math_id.as_str())
            })
        };
        assert!(
            feeds("EmptyLTXVLatentVideo", "length"),
            "{id}: math output feeds EmptyLTXVLatentVideo.length"
        );
        assert!(
            feeds("LTXVEmptyLatentAudio", "frames_number"),
            "{id}: math output feeds LTXVEmptyLatentAudio.frames_number"
        );
    }
}

#[test]
fn test_no_structural_edits() {
    let before = fixture_graph();
    let after = patch(&before, &job(), &[], &[]).unwrap();
    let b = before.0.as_object().unwrap();
    let a = after.0.as_object().unwrap();
    // The ONE sanctioned structural edit (A2): the "exr_output" sink is removed
    // for jobs that did not request exr-frames. Everything else must be
    // substitution-only, so compare the id sets with that node excluded.
    let exr_ids: std::collections::BTreeSet<_> = b
        .iter()
        .filter(|(_, n)| n.pointer("/_meta/title").and_then(Value::as_str) == Some("exr_output"))
        .map(|(id, _)| id)
        .collect();
    assert_eq!(exr_ids.len(), 1, "fixture carries exactly one exr_output");
    assert_eq!(
        b.keys()
            .filter(|k| !exr_ids.contains(k))
            .collect::<std::collections::BTreeSet<_>>(),
        a.keys().collect::<std::collections::BTreeSet<_>>(),
        "same node ids apart from the sanctioned exr_output removal"
    );
    for (id, bn) in b {
        if exr_ids.contains(id) {
            continue; // the removed sink has no after-side counterpart
        }
        assert_eq!(
            a[id].pointer("/class_type"),
            bn.pointer("/class_type"),
            "class_type {id} unchanged"
        );
    }
}

// ---------------------------------------------------------------------------
// BL3 iclora: widened seed handle (RandomNoise OR KSampler) + LoadVideo binding.
// ---------------------------------------------------------------------------

/// The pinned iclora graph (one `LoadImage` 200, one `LoadVideo` 199, seed in
/// the plain `KSampler` 129:704 — no RandomNoise anywhere).
fn iclora_graph() -> Graph {
    let raw = std::fs::read(format!("{DIR}/ltx-iclora-hdr/v1.json")).unwrap();
    Graph(serde_json::from_slice(&raw).unwrap())
}

fn iclora_job() -> LtxJob {
    LtxJob {
        template_id: "ltx-iclora-hdr".to_string(),
        template_hash: "0x00".to_string(),
        prompt: "restyle the control clip as a cartoon child".to_string(),
        seed: "4815162342".to_string(),
        frames: 126,
        fps: 25,
        resolution: Resolution { w: 768, h: 512 },
        lora: "ltx-iclora-hdr@v1".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
        videos: None,
        strength: None,
        azimuth: None,
        elevation: None,
        distance: None,
    }
}

#[test]
fn test_seed_lands_in_ksampler_no_randomnoise() {
    // iclora has NO RandomNoise; the widened seed handle stamps KSampler.seed.
    let g = patch(&iclora_graph(), &iclora_job(), &[], &[]).unwrap();
    assert!(nodes_by_class(&g, "RandomNoise").is_empty());
    let ks = nodes_by_class(&g, "KSampler");
    assert_eq!(ks.len(), 1);
    assert_eq!(
        ks[0].pointer("/inputs/seed").unwrap().as_u64().unwrap(),
        4815162342
    );
}

#[test]
fn test_seed_missing_both_classes_rejected() {
    // Remove the KSampler from iclora -> no seed handle at all -> fail closed.
    let mut v = iclora_graph().0;
    v.as_object_mut().unwrap().remove("129:704");
    assert!(patch(&Graph(v), &iclora_job(), &[], &[]).is_err());
}

#[test]
fn test_seed_widening_leaves_old_templates_unchanged() {
    // Pre-BL3 templates keep their behaviour: the i2v graph's RandomNoise nodes
    // get the job seed, and its KSamplerSelect nodes (a DIFFERENT class_type —
    // exact-match equality) are untouched.
    let g = patch(&i2v_graph(), &job(), &[], &[]).unwrap();
    let noise = nodes_by_class(&g, "RandomNoise");
    assert!(!noise.is_empty(), "i2v carries RandomNoise");
    for n in &noise {
        assert_eq!(
            n.pointer("/inputs/noise_seed").unwrap().as_u64().unwrap(),
            4815162342
        );
    }
    let selects = nodes_by_class(&g, "KSamplerSelect");
    assert!(!selects.is_empty(), "i2v carries KSamplerSelect");
    for n in selects {
        assert!(
            n.pointer("/inputs/seed").is_none(),
            "KSamplerSelect must not gain a seed input"
        );
    }
}

#[test]
fn test_iclora_full_patch() {
    // End-to-end handle check on the REAL pinned graph: prompt, seed, dims,
    // duration, the reference still AND the control video all land.
    let g = patch(
        &iclora_graph(),
        &iclora_job(),
        &["ref.png".to_string()],
        &["ctl.mp4".to_string()],
    )
    .unwrap();
    let leaf_u64 = |title: &str| {
        node_by_title(&g, title)
            .pointer("/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap()
    };
    assert_eq!(
        node_by_title(&g, "Prompt")
            .pointer("/inputs/value")
            .unwrap()
            .as_str()
            .unwrap(),
        "restyle the control clip as a cartoon child"
    );
    assert_eq!(leaf_u64("Width"), 768);
    assert_eq!(leaf_u64("Height"), 512);
    assert_eq!(leaf_u64("Frame Rate"), 25);
    // 126 frames @ 25 fps -> 5 s sliced from the control video.
    assert_eq!(leaf_u64("Duration"), 5);
    assert_eq!(
        nodes_by_class(&g, "LoadImage")[0]
            .pointer("/inputs/image")
            .unwrap()
            .as_str()
            .unwrap(),
        "ref.png"
    );
    assert_eq!(
        nodes_by_class(&g, "LoadVideo")[0]
            .pointer("/inputs/file")
            .unwrap()
            .as_str()
            .unwrap(),
        "ctl.mp4"
    );
}

#[test]
fn test_load_video_count_mismatch_rejected() {
    // Two names for one LoadVideo node -> fail closed (mirror of images).
    let two = vec!["a.mp4".to_string(), "b.mp4".to_string()];
    assert!(patch(&iclora_graph(), &iclora_job(), &[], &two).is_err());
    // A video name against a graph with NO LoadVideo -> fail closed too.
    assert!(patch(&fixture_graph(), &job(), &[], &["x.mp4".to_string()]).is_err());
}

// ---------------------------------------------------------------------------
// BL4 video-edit templates: `VHS_LoadVideo` joins the video-binding union and
// the billed frame count lands in its `frame_load_cap`.
// ---------------------------------------------------------------------------

/// One of the three pinned BL4 graphs (same spine; `VHS_LoadVideo` node `10`,
/// seed in `RandomNoise` `60`, positive prompt is the `CLIPTextEncode` titled
/// `Prompt`, dims are `Width`/`Height` primitives, no `LoadImage` at all).
fn bl4_graph(id: &str) -> Graph {
    let raw = std::fs::read(format!("{DIR}/{id}/v1.json")).unwrap();
    Graph(serde_json::from_slice(&raw).unwrap())
}

fn bl4_job(id: &str) -> LtxJob {
    LtxJob {
        template_id: id.to_string(),
        template_hash: "0x00".to_string(),
        prompt: "extend the scene, cinematic, natural lighting".to_string(),
        seed: "4815162342".to_string(),
        frames: 121,
        fps: 24,
        resolution: Resolution { w: 720, h: 1280 },
        lora: format!("{id}@v1"),
        output: OutputKind::ExrSequence,
        images: None,
        videos: None,
        strength: None,
        azimuth: None,
        elevation: None,
        distance: None,
    }
}

#[test]
fn test_bl4_full_patch_all_three_templates() {
    // End-to-end handle check on each REAL pinned graph: prompt (a
    // `CLIPTextEncode`, so the `text` leaf), seed (RandomNoise), dims, the
    // control video into `VHS_LoadVideo.video`, and the billed frame count
    // into its `frame_load_cap`.
    // WA/DN ride the same control-clip shape, so the same handle contract holds.
    for id in [
        "ltx-outpaint-hdr",
        "ltx-edit-hdr",
        "ltx-restore-hdr",
        "ltx-water-hdr",
        "ltx-daynight-hdr",
    ] {
        let g = patch(&bl4_graph(id), &bl4_job(id), &[], &["ctl.mp4".to_string()]).unwrap();
        assert_eq!(
            node_by_title(&g, "Prompt")
                .pointer("/inputs/text")
                .unwrap()
                .as_str()
                .unwrap(),
            "extend the scene, cinematic, natural lighting",
            "{id}: prompt"
        );
        let rn = nodes_by_class(&g, "RandomNoise");
        assert_eq!(rn.len(), 1, "{id}: one RandomNoise");
        assert_eq!(
            rn[0]
                .pointer("/inputs/noise_seed")
                .unwrap()
                .as_u64()
                .unwrap(),
            4_815_162_342,
            "{id}: seed"
        );
        let leaf_u64 = |title: &str| {
            node_by_title(&g, title)
                .pointer("/inputs/value")
                .unwrap()
                .as_u64()
                .unwrap()
        };
        assert_eq!(leaf_u64("Width"), 720, "{id}: width");
        assert_eq!(leaf_u64("Height"), 1280, "{id}: height");
        let vhs = nodes_by_class(&g, "VHS_LoadVideo");
        assert_eq!(vhs.len(), 1, "{id}: one VHS_LoadVideo");
        assert_eq!(
            vhs[0].pointer("/inputs/video").unwrap().as_str().unwrap(),
            "ctl.mp4",
            "{id}: control video bound"
        );
        assert_eq!(
            vhs[0]
                .pointer("/inputs/frame_load_cap")
                .unwrap()
                .as_u64()
                .unwrap(),
            121,
            "{id}: billed frames cap the loader"
        );
    }
}

#[test]
fn test_vhs_load_video_count_mismatch_rejected() {
    // Two names for one VHS_LoadVideo -> fail closed, same rule as LoadVideo.
    let two = vec!["a.mp4".to_string(), "b.mp4".to_string()];
    assert!(patch(
        &bl4_graph("ltx-outpaint-hdr"),
        &bl4_job("ltx-outpaint-hdr"),
        &[],
        &two
    )
    .is_err());
}

#[test]
fn test_video_binding_spans_both_loader_classes() {
    // Synthetic graph with one core `LoadVideo` ("2") AND one `VHS_LoadVideo`
    // ("1"): names bind across the UNION in node-id order, each through its
    // class's own filename key. No pinned template mixes the classes; this
    // pins the union semantics so a future one can't bind ambiguously.
    let raw = serde_json::json!({
        "1": { "inputs": { "video": "x.mp4", "force_rate": 0, "frame_load_cap": 0 },
               "class_type": "VHS_LoadVideo", "_meta": { "title": "Load Video (Upload)" } },
        "2": { "inputs": { "file": "y.mp4" },
               "class_type": "LoadVideo", "_meta": { "title": "Load Video" } },
        "3": { "inputs": { "text": "", "clip": ["9", 0] },
               "class_type": "CLIPTextEncode", "_meta": { "title": "Prompt" } },
        "4": { "inputs": { "noise_seed": 0 },
               "class_type": "RandomNoise", "_meta": { "title": "RandomNoise" } }
    });
    let g = Graph(raw);
    let names = vec!["first.mp4".to_string(), "second.mp4".to_string()];
    let patched = patch(&g, &bl4_job("ltx-outpaint-hdr"), &[], &names).unwrap();
    assert_eq!(
        nodes_by_class(&patched, "VHS_LoadVideo")[0]
            .pointer("/inputs/video")
            .unwrap()
            .as_str()
            .unwrap(),
        "first.mp4",
        "id \"1\" (VHS) takes the first name via its `video` key"
    );
    assert_eq!(
        nodes_by_class(&patched, "LoadVideo")[0]
            .pointer("/inputs/file")
            .unwrap()
            .as_str()
            .unwrap(),
        "second.mp4",
        "id \"2\" (core) takes the second name via its `file` key"
    );
    // One name across a two-loader union -> fail closed.
    assert!(patch(
        &g,
        &bl4_job("ltx-outpaint-hdr"),
        &[],
        &["only.mp4".to_string()]
    )
    .is_err());
}

#[test]
fn test_frame_load_cap_absent_is_noop_for_old_templates() {
    // No VHS_LoadVideo anywhere in the pre-BL4 family: the new frames handle
    // must not invent inputs or fail — t2v and iclora patch exactly as before.
    let g = patch(
        &iclora_graph(),
        &iclora_job(),
        &[],
        &["ctl.mp4".to_string()],
    )
    .unwrap();
    assert!(nodes_by_class(&g, "VHS_LoadVideo").is_empty());
    let g = patch(&fixture_graph(), &job(), &[], &[]).unwrap();
    assert!(nodes_by_class(&g, "VHS_LoadVideo").is_empty());
}

#[test]
fn test_upscale_full_patch() {
    // U-phase: the upscale template has NO Width/Height/Duration handles on purpose
    // (output = input x2, derived from the clip), an EMPTY prompt as a template
    // constant, and the same seed/frame-rate/video/frame-cap handles as the trio.
    // This proves the pinned graph satisfies every fail-closed patcher requirement.
    let mut job = bl4_job("ltx-upscale-hdr");
    job.prompt = String::new(); // the mode has no prompt; "" is committed honestly
    job.resolution = Resolution { w: 1536, h: 1024 }; // OUTPUT dims (= input x2)
    let g = patch(
        &bl4_graph("ltx-upscale-hdr"),
        &job,
        &[],
        &["src.mp4".to_string()],
    )
    .unwrap();

    // the empty prompt LANDS (patch_prompt is fail-closed, so this also proves the
    // handle exists); PrimitiveStringMultiline carries it in inputs.value
    assert_eq!(
        node_by_title(&g, "Prompt")
            .pointer("/inputs/value")
            .unwrap()
            .as_str()
            .unwrap(),
        ""
    );
    // seed reaches the refine's RandomNoise
    let rn = nodes_by_class(&g, "RandomNoise");
    assert_eq!(rn.len(), 1);
    assert_eq!(
        rn[0]
            .pointer("/inputs/noise_seed")
            .unwrap()
            .as_u64()
            .unwrap(),
        4_815_162_342
    );
    // fps lands on the Frame Rate primitive (feeds conditioning + audio latent + mux)
    assert_eq!(
        node_by_title(&g, "Frame Rate")
            .pointer("/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap(),
        24
    );
    // the source clip binds into the loader, capped at the billed frame count
    let vhs = nodes_by_class(&g, "VHS_LoadVideo");
    assert_eq!(vhs.len(), 1);
    assert_eq!(
        vhs[0].pointer("/inputs/video").unwrap().as_str().unwrap(),
        "src.mp4"
    );
    assert_eq!(
        vhs[0]
            .pointer("/inputs/frame_load_cap")
            .unwrap()
            .as_u64()
            .unwrap(),
        121
    );
    // and the sigmas remain the PINNED fidelity constant — not a patch target
    let ms = nodes_by_class(&g, "ManualSigmas");
    assert_eq!(
        ms[0].pointer("/inputs/sigmas").unwrap().as_str().unwrap(),
        "0.60, 0.40, 0.20, 0.0"
    );
}

#[test]
fn test_ingredients_full_patch() {
    // I-phase: ONE reference sheet in (LoadImage binder), committed Width/Height (the
    // sheet is conformed in-graph to the canvas, black pad), Duration -> frames via the
    // t2v math, Frame Rate primitive, prompt patched into the titled CLIPTextEncode,
    // seed by class. Sigmas are the author's distilled ladder, pinned.
    // Every asserted value DIFFERS from the template's pinned literal (960×544 @ 24fps,
    // 10 s) — a title-miss no-op (patch_by_title handles are optional) must FAIL here,
    // not render every production clip at the pins regardless of the committed job.
    let mut job = bl4_job("ltx-ingredients-hdr");
    job.prompt = "Reference sheet: an owl mascot. Generated video: the owl waves.".to_string();
    job.resolution = Resolution { w: 1280, h: 720 };
    job.fps = 25;
    job.frames = 126; // 25 × 5 + 1 → Duration 5, against the pinned 10
    let g = patch(
        &bl4_graph("ltx-ingredients-hdr"),
        &job,
        &["sheet.png".to_string()],
        &[],
    )
    .unwrap();

    assert_eq!(
        node_by_title(&g, "Prompt")
            .pointer("/inputs/text")
            .unwrap()
            .as_str()
            .unwrap(),
        "Reference sheet: an owl mascot. Generated video: the owl waves."
    );
    let leaf_u64 = |title: &str| {
        node_by_title(&g, title)
            .pointer("/inputs/value")
            .unwrap()
            .as_u64()
            .unwrap()
    };
    assert_eq!(leaf_u64("Width"), 1280);
    assert_eq!(leaf_u64("Height"), 720);
    assert_eq!(leaf_u64("Frame Rate"), 25);
    assert_eq!(leaf_u64("Duration"), 5);
    // the sheet binds into the ONE LoadImage
    let li = nodes_by_class(&g, "LoadImage");
    assert_eq!(li.len(), 1);
    assert_eq!(
        li[0].pointer("/inputs/image").unwrap().as_str().unwrap(),
        "sheet.png"
    );
    // seed reaches the refine noise
    let rn = nodes_by_class(&g, "RandomNoise");
    assert_eq!(rn.len(), 1);
    assert_eq!(
        rn[0]
            .pointer("/inputs/noise_seed")
            .unwrap()
            .as_u64()
            .unwrap(),
        4_815_162_342
    );
    // the author's distilled sigma ladder stays pinned — not a patch target
    let ms = nodes_by_class(&g, "ManualSigmas");
    assert_eq!(
        ms[0].pointer("/inputs/sigmas").unwrap().as_str().unwrap(),
        "1.0, 0.99375, 0.9875, 0.98125, 0.975, 0.909375, 0.725, 0.421875, 0.0"
    );
}

// ---------------------------------------------------------------------------
// Guide strength (2026-07-28): `LTXAddVideoICLoRAGuide.strength`, patched by
// CLASS. The pinned constant 1.0 = maximum source adherence — which is why
// object edits ("make the green prop gun black") could not take; the override
// hands the prompt authority over the source. Fail-closed on unguided
// templates so a paid render can never bill with the knob silently ignored.
// ---------------------------------------------------------------------------

#[test]
fn test_strength_patches_the_guide_on_the_real_edit_graph() {
    let mut j = bl4_job("ltx-edit-hdr");
    j.strength = Some(0.6);
    let patched = patch(
        &bl4_graph("ltx-edit-hdr"),
        &j,
        &[],
        &["control.mp4".to_string()],
    )
    .unwrap();
    let guides = nodes_by_class(&patched, "LTXAddVideoICLoRAGuide");
    assert_eq!(guides.len(), 1, "the edit graph carries exactly one guide");
    assert_eq!(
        guides[0]
            .pointer("/inputs/strength")
            .unwrap()
            .as_f64()
            .unwrap(),
        0.6
    );
}

#[test]
fn test_no_strength_leaves_the_pinned_constant() {
    // None must not touch the graph: the pre-strength wire produces an
    // identical patched graph, so old helpers keep byte-identical behaviour.
    let patched = patch(
        &bl4_graph("ltx-edit-hdr"),
        &bl4_job("ltx-edit-hdr"),
        &[],
        &["control.mp4".to_string()],
    )
    .unwrap();
    assert_eq!(
        nodes_by_class(&patched, "LTXAddVideoICLoRAGuide")[0]
            .pointer("/inputs/strength")
            .unwrap()
            .as_f64()
            .unwrap(),
        1.0
    );
}

#[test]
fn test_strength_on_an_unguided_template_fails_closed() {
    // t2v has no guide node. Silently ignoring the knob would bill a paid
    // render whose one requested parameter did nothing.
    let mut j = job();
    j.strength = Some(0.5);
    let err = patch(&fixture_graph(), &j, &[], &[])
        .unwrap_err()
        .to_string();
    assert!(err.contains("no IC-LoRA guide"), "got: {err}");
}

#[test]
fn test_strength_reaches_a_retitled_guide_by_class() {
    // ingredients retitles the node with glyphs ("🅛🅣🅧 Add Video IC-LoRA
    // Guide"); class matching must not care about titles.
    let raw = serde_json::json!({
        "1": { "inputs": { "strength": 1.0 },
               "class_type": "LTXAddVideoICLoRAGuide",
               "_meta": { "title": "🅛🅣🅧 Add Video IC-LoRA Guide" } },
        "3": { "inputs": { "text": "", "clip": ["9", 0] },
               "class_type": "CLIPTextEncode", "_meta": { "title": "Prompt" } },
        "4": { "inputs": { "noise_seed": 0 },
               "class_type": "RandomNoise", "_meta": { "title": "RandomNoise" } }
    });
    let mut j = bl4_job("ltx-ingredients-hdr");
    j.strength = Some(0.35);
    let patched = patch(&Graph(raw), &j, &[], &[]).unwrap();
    assert_eq!(
        nodes_by_class(&patched, "LTXAddVideoICLoRAGuide")[0]
            .pointer("/inputs/strength")
            .unwrap()
            .as_f64()
            .unwrap(),
        0.35
    );
}

// ---------------------------------------------------------------------------
// CrossView camera (CV1, 2026-07-30): azimuth/elevation/distance patched by
// CLASS onto CrossViewWarp; "Frame Count" is the one titled INT that feeds
// BOTH frame_load_cap and the latent length. Probe provenance: the pose
// (40, 30, 1.0) rendered clean on real 1280x704/121f street footage.
// ---------------------------------------------------------------------------

fn crossview_graph() -> Graph {
    let raw = std::fs::read(format!("{DIR}/ltx-crossview-hdr/v1.json")).unwrap();
    Graph(serde_json::from_slice(&raw).unwrap())
}

fn crossview_job() -> LtxJob {
    let mut j = bl4_job("ltx-crossview-hdr");
    j.resolution = Resolution { w: 1280, h: 720 };
    j
}

#[test]
fn test_camera_patches_the_crossview_node() {
    let mut j = crossview_job();
    j.azimuth = Some(40.0);
    j.elevation = Some(30.0);
    j.distance = Some(1.0);
    let patched = patch(&crossview_graph(), &j, &[], &["control.mp4".to_string()]).unwrap();
    let cam = nodes_by_class(&patched, "CrossViewWarp");
    assert_eq!(cam.len(), 1, "one camera node");
    for (k, want) in [("azimuth", 40.0), ("elevation", 30.0), ("distance", 1.0)] {
        assert_eq!(
            cam[0]
                .pointer(&format!("/inputs/{k}"))
                .unwrap()
                .as_f64()
                .unwrap(),
            want
        );
    }
}

#[test]
fn test_no_camera_leaves_the_pinned_mild_pose() {
    let patched = patch(
        &crossview_graph(),
        &crossview_job(),
        &[],
        &["control.mp4".to_string()],
    )
    .unwrap();
    let cam = &nodes_by_class(&patched, "CrossViewWarp")[0];
    assert_eq!(
        cam.pointer("/inputs/azimuth").unwrap().as_f64().unwrap(),
        20.0
    );
    assert_eq!(
        cam.pointer("/inputs/elevation").unwrap().as_f64().unwrap(),
        0.0
    );
}

#[test]
fn test_camera_on_a_cameraless_template_fails_closed() {
    let mut j = job(); // t2v
    j.azimuth = Some(20.0);
    let err = patch(&fixture_graph(), &j, &[], &[])
        .unwrap_err()
        .to_string();
    assert!(err.contains("no CrossViewWarp"), "got: {err}");
}

#[test]
fn test_frame_count_handle_binds_billed_to_loaded_and_rendered() {
    let mut j = crossview_job();
    j.frames = 121;
    j.fps = 24;
    let patched = patch(&crossview_graph(), &j, &[], &["control.mp4".to_string()]).unwrap();
    let g = patched.0.as_object().unwrap();
    let fc = g
        .values()
        .find(|n| n.pointer("/_meta/title").and_then(Value::as_str) == Some("Frame Count"))
        .expect("crossview carries the Frame Count handle");
    assert_eq!(fc.pointer("/inputs/value").unwrap().as_i64().unwrap(), 121);
    // and the strength patch reaches BOTH guides in this template
    let mut j2 = crossview_job();
    j2.strength = Some(0.8);
    let p2 = patch(&crossview_graph(), &j2, &[], &["control.mp4".to_string()]).unwrap();
    let guides = nodes_by_class(&p2, "LTXAddVideoICLoRAGuide");
    assert_eq!(
        guides.len(),
        3,
        "two base-pass guides + the x2 refine pass guide"
    );
    for gd in guides {
        assert_eq!(
            gd.pointer("/inputs/strength").unwrap().as_f64().unwrap(),
            0.8
        );
    }
}


// ---------------------------------------------------------------------------
// A2 — EXR masters (2026-08-10): the "exr_output" RadianceSaveEXR sink rides
// every pinned template; removed for legacy jobs, required for `exr-frames`.
// ---------------------------------------------------------------------------

#[test]
fn test_exr_output_removed_for_legacy_jobs() {
    let patched = patch(&fixture_graph(), &job(), &[], &[]).unwrap();
    assert!(
        nodes_by_class(&patched, "RadianceDigitalCinemaWrite").is_empty(),
        "legacy exr-sequence jobs must not carry the EXR sink"
    );
}

#[test]
fn test_exr_output_kept_and_required_for_exr_frames() {
    let mut j = job();
    j.output = OutputKind::ExrFrames;
    let patched = patch(&fixture_graph(), &j, &[], &[]).unwrap();
    let sinks = nodes_by_class(&patched, "RadianceDigitalCinemaWrite");
    assert_eq!(sinks.len(), 1, "exr-frames keeps the pinned EXR sink");
    assert_eq!(
        sinks[0].pointer("/inputs/bit_depth").unwrap().as_str().unwrap(),
        "16-bit Half Float"
    );

    // fail closed on a template with no sink
    let mut g = fixture_graph();
    let ids: Vec<String> = g.0.as_object().unwrap().iter()
        .filter(|(_, n)| n.pointer("/_meta/title").and_then(Value::as_str) == Some("exr_output"))
        .map(|(id, _)| id.clone()).collect();
    for id in ids { g.0.as_object_mut().unwrap().remove(&id); }
    let err = patch(&g, &j, &[], &[]).unwrap_err().to_string();
    assert!(err.contains("no exr_output"), "got: {err}");
}
