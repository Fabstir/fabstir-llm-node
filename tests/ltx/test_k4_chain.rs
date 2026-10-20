// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! K4 (8.60.7, IMPLEMENTATION-LTX25-4K.md): fixed-prompt templates with a stored, pre-encoded prompt, the stored-file
//! listing, and chained parts' "Previous part" loader. The graphs are the pinned Refine Details graph
//! (`tiled-fusion-fixture.json`) transformed the way the v2 and chain templates are. Every test names its mutation.

use fabstir_llm_node::ltx::client::parse_conditioning_files;
use fabstir_llm_node::ltx::patcher::{
    patch, patch_with, previous_part_index, stored_conditioning_files, PatchOptions, CHAIN_OVERLAP,
};
use fabstir_llm_node::ltx::template::{check_fixed_prompt, TemplateEntry, TemplateStore};
use fabstir_llm_node::ltx::types::{LtxJob, OutputKind, Resolution};
use fabstir_llm_node::ltx::Graph;
use serde_json::{json, Value};

fn refine_graph() -> Graph {
    let raw = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/ltx/tiled-fusion-fixture.json"
    ))
    .unwrap();
    Graph(serde_json::from_slice(&raw).unwrap())
}

/// v2: the prompt pair stored pre-encoded (`LTXVLoadConditioning`), the text encoder gone (no `Prompt` handle left).
fn stored_graph() -> Graph {
    let mut g = refine_graph();
    let obj = g.0.as_object_mut().unwrap();
    obj.remove("25:clip");
    obj.insert(
        "40".into(),
        json!({"class_type": "LTXVLoadConditioning", "_meta": {"title": "Prompt (stored)"},
               "inputs": {"file_name": "ltx25-refine-pos-v1-aaaa.safetensors", "device": "cpu"}}),
    );
    obj.insert(
        "41".into(),
        json!({"class_type": "LTXVLoadConditioning", "_meta": {"title": "Negative prompt (stored)"},
               "inputs": {"file_name": "ltx25-refine-neg-v1-bbbb.safetensors", "device": "cpu"}}),
    );
    g
}

/// The chain template: v2 + a second `VHS_LoadVideo` titled "Previous part" (id "19", after the source "10") feeding
/// `LTXVImgToVideoInplace` on the empty latent, as the K4.0 probe graphs that passed on host1.
fn chain_graph() -> Graph {
    let mut g = stored_graph();
    let obj = g.0.as_object_mut().unwrap();
    let mut prev = obj["10"].clone();
    prev["_meta"] = json!({"title": "Previous part"});
    obj.insert("19".into(), prev);
    obj.insert(
        "92".into(),
        json!({"class_type": "LTXVImgToVideoInplace",
               "inputs": {"vae": ["25:vae", 0], "image": ["19", 0], "latent": ["50", 0], "strength": 1.0, "bypass": false}}),
    );
    obj.get_mut("52").unwrap()["inputs"]["latent"] = json!(["92", 0]);
    g
}

fn job(frames: u32) -> LtxJob {
    LtxJob {
        template_id: "ltx-refine-chain-hdr".to_string(),
        template_hash: "0x00".to_string(),
        prompt: "sharp photographic detail".to_string(),
        seed: "42".to_string(),
        frames,
        fps: 24,
        resolution: Resolution { w: 3840, h: 2176 },
        lora: "ltx-refine-chain-hdr@v1".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
        videos: None,
        strength: None,
        azimuth: None,
        elevation: None,
        distance: None,
        speed: None,
        input_wire: None,
    }
}

fn fixed() -> PatchOptions {
    PatchOptions {
        prompt_required: false,
        previous_part_samples: None,
    }
}

fn input(g: &Graph, id: &str, key: &str) -> Value {
    g.0[id]["inputs"][key].clone()
}

#[test]
fn a_fixed_prompt_template_patches_without_a_prompt_handle() {
    // mutation: ignore `prompt_required` (always patch the prompt) → the stored graph errors → red
    let g = patch_with(&stored_graph(), &job(49), &[], &[], &fixed()).unwrap();
    assert_eq!(input(&g, "65", "seed"), json!(42));
}

#[test]
fn every_other_template_still_requires_the_prompt_handle() {
    // mutation: skip the prompt for all (default prompt_required false) → no error → red
    let err = patch(&stored_graph(), &job(49), &[], &[])
        .unwrap_err()
        .to_string();
    assert!(err.contains("required handle \"Prompt\""), "{err}");
}

#[test]
fn a_fixed_prompt_template_that_still_has_a_prompt_node_gets_it_patched() {
    // mutation: never patch when not required → the v1 graph keeps its baked text → red
    let mut j = job(49);
    j.prompt = "the fixed text".into();
    let g = patch_with(&refine_graph(), &j, &[], &[], &fixed()).unwrap();
    assert_eq!(input(&g, "40", "text"), json!("the fixed text"));
}

fn entry(fixed_prompt: Option<&str>) -> TemplateEntry {
    TemplateEntry {
        template_id: "ltx-refine-hdr".to_string(),
        template_hash: "0x00".to_string(),
        image_inputs: 0,
        image_semantics: vec![],
        video_inputs: 1,
        video_semantics: vec![],
        fps: None,
        max_frames: None,
        min_frames: None,
        max_tokens: None,
        resolution_rule: None,
        exact_control: None,
        frame_grid: None,
        sidecar: None,
        fixed_prompt: fixed_prompt.map(str::to_string),
    }
}

#[test]
fn a_fixed_prompt_entry_takes_exactly_its_prompt() {
    // mutation: drop the comparison → a job commits a prompt whose encoding never ran → red
    let e = entry(Some("sharp photographic detail"));
    assert!(check_fixed_prompt(Some(&e), "sharp photographic detail").is_ok());
    let err = check_fixed_prompt(Some(&e), "a different prompt").unwrap_err();
    assert!(
        err.contains("ltx-refine-hdr") && err.contains("fixed prompt"),
        "{err}"
    );
    assert!(check_fixed_prompt(Some(&e), "").is_err());
    // no fixedPrompt, or no entry: the prompt is free (patched in, as before)
    assert!(check_fixed_prompt(Some(&entry(None)), "anything").is_ok());
    assert!(check_fixed_prompt(None, "anything").is_ok());
}

#[test]
fn an_entry_without_a_fixed_prompt_serialises_without_the_key() {
    // mutation: serialise `None` as null → every existing bundle hash moves → red
    let wire = serde_json::to_value(entry(None)).unwrap();
    assert!(wire.get("fixedPrompt").is_none(), "{wire}");
    let wire = serde_json::to_value(entry(Some("x"))).unwrap();
    assert_eq!(wire["fixedPrompt"], "x");
}

#[test]
fn the_store_carries_fixed_prompt_from_the_allowlist() {
    // mutation: drop the copy into TemplateEntry → the bundle never advertises it and the node never checks it → red
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("fp-t")).unwrap();
    std::fs::write(
        dir.path().join("fp-t/v2.json"),
        br#"{"1":{"class_type":"RandomNoise","inputs":{"noise_seed":0}}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("allowlist.json"),
        br#"{"allowListVersion":43,"templates":[{"templateId":"fp-t","version":"v2","fixedPrompt":"sharp detail"}],
            "loras":[],"bounds":{"frames":{"min":9,"max":751},"fps":[24],"resolutions":[{"w":768,"h":512}]}}"#,
    )
    .unwrap();
    let store = TemplateStore::new(dir.path()).unwrap();
    assert_eq!(
        store.entry("fp-t").unwrap().fixed_prompt.as_deref(),
        Some("sharp detail")
    );
    let wire = serde_json::to_value(store.bundle()).unwrap();
    assert_eq!(wire["templates"][0]["fixedPrompt"], "sharp detail");
}

#[test]
fn the_stored_files_a_template_loads() {
    // mutation: return nothing → the handler's pre-accept existence check never runs → red
    assert_eq!(
        stored_conditioning_files(&stored_graph()),
        vec![
            "ltx25-refine-neg-v1-bbbb.safetensors".to_string(),
            "ltx25-refine-pos-v1-aaaa.safetensors".to_string()
        ]
    );
    assert!(stored_conditioning_files(&refine_graph()).is_empty());
}

#[test]
fn comfyui_lists_stored_files_in_either_combo_shape() {
    // mutation: read only one shape → host1's v3 schema (or an older node) yields None → red
    let v3 = json!({"LTXVLoadConditioning": {"input": {"required": {"file_name":
        ["COMBO", {"multiselect": false, "options": ["a.safetensors", "b.safetensors"]}]}}}});
    assert_eq!(
        parse_conditioning_files(&v3).unwrap(),
        vec!["a.safetensors", "b.safetensors"]
    );
    let legacy = json!({"LTXVLoadConditioning": {"input": {"required": {"file_name": [["c.safetensors"], {}]}}}});
    assert_eq!(
        parse_conditioning_files(&legacy).unwrap(),
        vec!["c.safetensors"]
    );
    assert!(parse_conditioning_files(&json!({})).is_none());
}

#[test]
fn the_previous_part_binds_the_second_video() {
    // videos[i] binds the i-th loader by id across the class UNION bind_inputs uses (core LoadVideo + VHS_LoadVideo).
    assert_eq!(previous_part_index(&chain_graph()), Some(1));
    assert_eq!(previous_part_index(&refine_graph()), None);
    // mutation: rank among VHS_LoadVideo only → a core LoadVideo "15" between them is skipped, 1 instead of 2 → red
    let mut mixed = chain_graph();
    mixed.0.as_object_mut().unwrap().insert(
        "15".into(),
        json!({"class_type": "LoadVideo", "inputs": {"file": "x.mp4"}}),
    );
    assert_eq!(previous_part_index(&mixed), Some(2));
    // a lower id binds earlier ("09" < "10"). serde_json here has no `preserve_order`, so maps already iterate in key
    // order; the explicit sort in `previous_part_index` guards against that ever changing.
    let mut g = chain_graph();
    let prev = g.0.as_object_mut().unwrap().remove("19").unwrap();
    g.0.as_object_mut().unwrap().insert("09".into(), prev);
    assert_eq!(previous_part_index(&g), Some(0));
}

#[test]
fn entry_and_graph_agree_on_the_previous_part() {
    use fabstir_llm_node::ltx::patcher::check_previous_part;
    let sem = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // the chain template as shipped: sourceVideo, previousPart ↔ loaders "10", "19"
    assert!(check_previous_part(&sem(&["sourceVideo", "previousPart"]), &chain_graph()).is_ok());
    // every other template: neither declares nor carries one
    assert!(check_previous_part(&sem(&["sourceVideo"]), &refine_graph()).is_ok());
    // mutation: no check → a chain template whose loader lost its title decodes the whole previous part, keeps its
    // FIRST frames and still bills → red
    let mut untitled = chain_graph();
    untitled.0.as_object_mut().unwrap().get_mut("19").unwrap()["_meta"] =
        json!({"title": "Previous Part"});
    let err = check_previous_part(&sem(&["sourceVideo", "previousPart"]), &untitled).unwrap_err();
    assert!(err.contains("previousPart"), "{err}");
    // a "Previous part" loader the entry does not declare (or declares at another slot) is refused too
    assert!(check_previous_part(&sem(&["sourceVideo", "controlVideo"]), &chain_graph()).is_err());
    assert!(check_previous_part(&sem(&["previousPart", "sourceVideo"]), &chain_graph()).is_err());
}

// The handler's call sites (a test server has no ComfyUI or checkpoint manager, so the pre-accept path cannot run end
// to end here; precedent tests/ltx/test_relight_source.rs). Every check strips ALL whitespace from both sides.
fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn handler() -> String {
    squash(include_str!("../../src/api/websocket/handlers/ltx.rs"))
}

fn window(src: &str, from: &str, to: &str) -> String {
    let (from, to) = (squash(from), squash(to));
    let a = src
        .find(&from)
        .unwrap_or_else(|| panic!("anchor {from:?} missing"));
    let b = a + src[a..]
        .find(&to)
        .unwrap_or_else(|| panic!("anchor {to:?} missing after {from:?}"));
    src[a..b].to_string()
}

#[test]
fn before_accept_the_handler_checks_the_prompt_the_previous_part_and_the_stored_files() {
    // mutation: delete any of these calls, or move it after the permit → a job is accepted (deposit locked) that
    // cannot run as committed → red
    let src = handler();
    let w = window(
        &src,
        "check_template_rules(e, job.resolution.w",
        "server.ltx_semaphore().try_acquire_owned()",
    );
    for needle in [
        "check_fixed_prompt(entry,&job.prompt)",
        "patcher::check_previous_part(",
        "prompt_required:entry.and_then(|e|e.fixed_prompt.as_ref()).is_none()",
        "patcher::patch_with(&graph,&job,&[],&[],&patch_opts)",
        "patcher::stored_conditioning_files(&graph)",
        "c.conditioning_files().await",
        "STORED_ENCODING_MISSING:",
        "STORED_ENCODING_UNVERIFIED:",
    ] {
        assert!(w.contains(needle), "pre-accept window lacks {needle}");
    }
}

#[test]
fn after_accept_the_fixed_prompt_and_the_previous_part_reach_the_patcher() {
    // mutation: prompt_required always true in the spawn → every fixed-prompt job fails after accept; drop the sample
    // count → every chained part fails after accept → red
    let src = handler();
    let w = window(&src, "let (exact, prompt_required) =", "prepare_inputs(");
    assert!(
        w.contains("e.and_then(|e|e.fixed_prompt.as_ref()).is_none()"),
        "{w}"
    );
    let w = window(&src, "let patched_graph = match prepare_inputs(", ".await");
    assert!(w.contains("prompt_required,"), "{w}");
    let w = window(
        &src,
        "async fn prepare_inputs(",
        "patcher::drop_silent_audio(",
    );
    for needle in [
        "samples=check_control_video_exact(plaintext,job.frames,exact)?",
        "video_samples.push(samples)",
        "patcher::previous_part_index(&graph).and_then(|i|video_samples.get(i).copied())",
        "patcher::patch_with(&graph,job,&image_names,&video_names,&opts)",
    ] {
        assert!(w.contains(needle), "prepare_inputs lacks {needle}");
    }
}

#[test]
fn after_accept_the_previous_part_reads_only_its_last_17_frames() {
    // mutation: skip the override → "19" decodes the whole previous part (skip 0, cap = billed frames) → red
    let names = vec!["src.mp4".to_string(), "prev.mp4".to_string()];
    let opts = PatchOptions {
        prompt_required: false,
        previous_part_samples: Some(49),
    };
    let g = patch_with(&chain_graph(), &job(49), &[], &names, &opts).unwrap();
    assert_eq!(input(&g, "10", "video"), json!("src.mp4"));
    assert_eq!(input(&g, "10", "frame_load_cap"), json!(49));
    assert_eq!(input(&g, "19", "video"), json!("prev.mp4"));
    assert_eq!(
        input(&g, "19", "skip_first_frames"),
        json!(49 - CHAIN_OVERLAP)
    );
    assert_eq!(input(&g, "19", "frame_load_cap"), json!(CHAIN_OVERLAP));
    // a short last part (25 billed) still reads the previous part's LAST 17 of its 49
    let short = patch_with(&chain_graph(), &job(25), &[], &names, &opts).unwrap();
    assert_eq!(input(&short, "10", "frame_load_cap"), json!(25));
    assert_eq!(input(&short, "19", "skip_first_frames"), json!(32));
    assert_eq!(input(&short, "19", "frame_load_cap"), json!(17));
}

#[test]
fn before_accept_the_chain_template_patches_with_no_sample_count() {
    // mutation: demand the sample count on the dry run too → every chained job is refused pre-accept → red
    let g = patch_with(&chain_graph(), &job(49), &[], &[], &fixed()).unwrap();
    assert_eq!(input(&g, "19", "skip_first_frames"), json!(0));
}

#[test]
fn a_previous_part_too_short_or_uncounted_fails_after_accept() {
    // mutation: no length check → a previous part of 17 frames or fewer is read from frame 0 → red
    let names = vec!["src.mp4".to_string(), "prev.mp4".to_string()];
    for samples in [0, 17] {
        let opts = PatchOptions {
            prompt_required: false,
            previous_part_samples: Some(samples),
        };
        let err = patch_with(&chain_graph(), &job(49), &[], &names, &opts)
            .unwrap_err()
            .to_string();
        assert!(err.contains("at least 18"), "{err}");
    }
    let opts = PatchOptions {
        prompt_required: false,
        previous_part_samples: Some(18),
    };
    assert!(patch_with(&chain_graph(), &job(49), &[], &names, &opts).is_ok());
    // mutation: treat a missing count as zero-skip → red
    let err = patch_with(&chain_graph(), &job(49), &[], &names, &fixed())
        .unwrap_err()
        .to_string();
    assert!(err.contains("frame count is unknown"), "{err}");
}

#[test]
fn a_chain_template_has_exactly_one_previous_part_and_it_is_a_vhs_loader() {
    // mutation: allow two (or a core LoadVideo) → which frames are kept becomes ambiguous → red
    let mut two = chain_graph();
    let dup = two.0["19"].clone();
    two.0.as_object_mut().unwrap().insert("18".into(), dup);
    let err = patch_with(&two, &job(49), &[], &[], &fixed())
        .unwrap_err()
        .to_string();
    assert!(err.contains("exactly one"), "{err}");
    let mut core = chain_graph();
    core.0.as_object_mut().unwrap().get_mut("19").unwrap()["class_type"] = json!("LoadVideo");
    let err = patch_with(&core, &job(49), &[], &[], &fixed())
        .unwrap_err()
        .to_string();
    assert!(err.contains("not a VHS_LoadVideo"), "{err}");
}
