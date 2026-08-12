// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 2 template store + versioned allow-list bundle tests.

use fabstir_llm_node::checkpoint::delta::sort_json_keys;
use fabstir_llm_node::ltx::template::TemplateStore;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");

/// Golden oracle: locks the wire contract for the pinned fixture. Update ONLY on
/// an intentional template or canonicalisation change.
const HDR_TEMPLATE_HASH: &str =
    "0x74ccf9abe4423f908357cb8da2f3a0f6475ec33b129032795d01efbbc79a9f94";
/// i2v graph hash (prompt-enhance baked ON — the BL2 grey-output fix; matches the
/// template deployed live in bundle v5).
const I2V_TEMPLATE_HASH: &str =
    "0xbb8c30fcd45f372ce4ed75428fb97fb09c9a3a479215cd7b03af99fddc04d1bb";
/// flf2v graph hash (curated: positive CLIPTextEncode retitled Prompt, height/Frame
/// Rate retitled to match the patcher handles).
const FLF2V_TEMPLATE_HASH: &str =
    "0xd09fd1325947906d2de26666e45cebd19256e1b6a4730ff3e621fb4137a3b6bf";
/// iclora graph hash (authored at BL3 U1 from the archive IC-LoRA union-control
/// workflow docs/archive/comfyui/video_ltx2_3_ic_lora_20260701.json: Prompt
/// handle wired to both consumers, Height / Frame Rate retitles, Duration
/// primitive into Video Slice, prompt-enhance baked ON; the U0 preflight
/// 2026-07-06 proved the archive graph live on 3XS-Z — styled motion following
/// the control clip + audible audio).
/// Re-pinned 2026-07-06: `Duration` (node 901) is a `PrimitiveFloat` — the Video
/// Slice `duration` input is FLOAT-typed, and ComfyUI rejects an INT link with
/// "Return type mismatch between linked nodes" (caught live, session 847: the
/// video branch was ignored and the run produced no frames). Widget PATCHING is
/// unaffected — the patcher writes a JSON number into the `value` leaf either way.
const ICLORA_TEMPLATE_HASH: &str =
    "0x3fbd6084d1d0e9a953569632df75e0ed07e95cd96be44894de3921e2ea4316fb";
/// BL4 template hashes (authored at V1 from the live-proven host exports in
/// docs/archive/comfyui/: outpaint ef5d632ed2c5, edit 1fb07c3b4b99, restore
/// 558cc012978d). One shared 30-node spine — dev-fp8 + distilled-384 + mode
/// IC-LoRA, ManualSigmas 8-step, local Gemma encoder, radiance gamma pair,
/// source-audio passthrough — differing only in resize head (outpaint
/// fit+letterbox "pad"; edit/restore centre-"crop"), LoRA stack and strengths.
/// All three graphs proved free on 3XS-Z 2026-07-10 (vertical outpaint fill /
/// retriever insertion / restore identity) BEFORE these hashes were pinned.
const OUTPAINT_TEMPLATE_HASH: &str =
    "0xdf3e88489d4f73b89c6c3081e8a3929fe8b512e2773013199a504fb7a0bc0f6c";
const EDIT_TEMPLATE_HASH: &str =
    "0xf933e8a49781a900c71f19b50b0d704ca7d911645afb488dec213cd5174dfaac";
const RESTORE_TEMPLATE_HASH: &str =
    "0x5c3344c7260549c1aa6fee5daba4e1d9cb19a4c9c81cf546a0a2fac85c6b2b5c";
/// Bundle hash MOVES at each bundle bump (v3 added flf2v; v4 the resolution
/// ladder + 32 MiB image cap; v5 the clip-duration bounds frames {121,751} and
/// corrected fps [24,25,48,50], with the i2v enhance=true re-pin landing within
/// v5 as the LIVE on-chain 0xb44beb2c…; v6 adds ltx-iclora-hdr + the video
/// bounds/videoInputs fields; v7 adds the BL4 trio outpaint/edit/restore +
/// their lora ids); the t2v/i2v/flf2v/iclora graph hashes above must NOT move.
// v8 (2026-07-13): the ladder gained the /64-clean HD+QHD sizes. 1080/1440 floor to an
// ODD latent (1080//32 == 33), which the IC-LoRA guide rejects outright and the VAE
// truncates for everyone else — 1088/1408 are their renderable neighbours.
// v15 (2026-07-18) added the ingredients lora advert; v16 adds ltx-water-hdr +
// ltx-daynight-hdr and their lora ids.
const BUNDLE_HASH: &str = "0x0f0e4c6820dac7fe0d9902c74d263083cdc498818edd786295c6c28501a88697";

fn keccak_hex(bytes: Vec<u8>) -> String {
    format!("0x{}", hex::encode(ethers::utils::keccak256(bytes)))
}

#[test]
fn test_load_and_hash() {
    let store = TemplateStore::new(DIR).unwrap();
    let h = store
        .template_hash("ltx-t2v-hdr")
        .expect("template present");
    eprintln!("GOLD HDR_TEMPLATE_HASH={h}");
    assert!(h.starts_with("0x") && h.len() == 66, "hash shape: {h}");
    // Independent recompute of the documented algorithm: parse -> sort_json_keys
    // -> compact serialise -> keccak256 (same path the impl uses).
    let raw = std::fs::read(format!("{DIR}/ltx-t2v-hdr/v1.json")).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let expect = keccak_hex(serde_json::to_vec(&sort_json_keys(&v)).unwrap());
    assert_eq!(h, expect);
    if HDR_TEMPLATE_HASH != "0x__TBD__" {
        assert_eq!(h, HDR_TEMPLATE_HASH, "golden templateHash drifted");
    }
}

#[test]
fn test_unknown_template_rejected() {
    let store = TemplateStore::new(DIR).unwrap();
    assert!(store.verify("does-not-exist", "0x00").is_err());
}

#[test]
fn test_hash_mismatch_rejected() {
    let store = TemplateStore::new(DIR).unwrap();
    let good = store.template_hash("ltx-t2v-hdr").unwrap().to_string();
    // right id, valid-shaped but wrong hash -> hard reject.
    let wrong = format!("0x{}", "00".repeat(32));
    assert!(store.verify("ltx-t2v-hdr", &wrong).is_err());
    // right id, right hash (case-insensitive) -> ok.
    assert!(store.verify("ltx-t2v-hdr", &good).is_ok());
    assert!(store.verify("ltx-t2v-hdr", &good.to_uppercase()).is_ok());
}

#[test]
fn test_bundle_hash_stable() {
    let a = TemplateStore::new(DIR).unwrap();
    let b = TemplateStore::new(DIR).unwrap();
    let ha = a.bundle().bundle_hash.clone();
    eprintln!("GOLD BUNDLE_HASH={ha}");
    assert_eq!(ha, b.bundle().bundle_hash, "bundleHash stable across loads");
    assert!(ha.starts_with("0x") && ha.len() == 66);
    // bundleHash == keccak256(canonical bundle without the hash field).
    let mut v = serde_json::to_value(a.bundle()).unwrap();
    v.as_object_mut().unwrap().remove("bundleHash");
    let expect = keccak_hex(serde_json::to_vec(&sort_json_keys(&v)).unwrap());
    assert_eq!(ha, expect);
    if BUNDLE_HASH != "0x__TBD__" {
        assert_eq!(ha, BUNDLE_HASH, "golden bundleHash drifted");
    }
    // The bundle advertises exactly the hash the store computes for the template.
    let adv = &a
        .bundle()
        .templates
        .iter()
        .find(|t| t.template_id == "ltx-t2v-hdr")
        .unwrap()
        .template_hash;
    assert_eq!(adv, a.template_hash("ltx-t2v-hdr").unwrap());
}

#[test]
fn test_i2v_template_hash_stable() {
    let store = TemplateStore::new(DIR).unwrap();
    let h = store
        .template_hash("ltx-i2v-hdr")
        .expect("i2v template present");
    eprintln!("GOLD I2V_TEMPLATE_HASH={h}");
    assert!(h.starts_with("0x") && h.len() == 66, "hash shape: {h}");
    // Independent recompute (same canonical path the impl uses).
    let raw = std::fs::read(format!("{DIR}/ltx-i2v-hdr/v1.json")).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let expect = keccak_hex(serde_json::to_vec(&sort_json_keys(&v)).unwrap());
    assert_eq!(h, expect);
    // The pinned i2v template must ship prompt-enhance ON: LTX 2.3 conditions on
    // the TextGenerateLTX2Prompt rewrite of the prompt; with the switch OFF the raw
    // prompt reaches the encoder and the model renders grey (proven live, sessions
    // 827-830 grey vs 831 real content after this flip). Provenance holds because
    // the patched Prompt node (320:319) — whose text `inputCommitment` binds — is
    // the INPUT to the rewrite (320:325): committed prompt -> deterministic-graph
    // rewrite -> conditioning.
    assert_eq!(
        v.pointer("/320:328/inputs/value"),
        Some(&serde_json::Value::Bool(true)),
        "Enable Prompt Enhance must be baked ON (raw prompt renders grey)"
    );
    // The enhance boolean (320:328) drives the ComfySwitchNode (320:327) whose
    // `on_false` is the patched Prompt node (320:319) and `on_true` is the gemma
    // rewrite (320:325). (The templateHash golden also locks it, but assert intent
    // explicitly.)
    assert_eq!(
        v.pointer("/320:327/inputs/switch").and_then(|s| s.get(0)),
        Some(&serde_json::Value::String("320:328".to_string())),
        "enhance switch must be driven by the enhance boolean"
    );
    assert_eq!(
        v.pointer("/320:327/inputs/on_false").and_then(|s| s.get(0)),
        Some(&serde_json::Value::String("320:319".to_string())),
        "switch=false must route the RAW patched Prompt node"
    );
    if I2V_TEMPLATE_HASH != "0x__TBD__" {
        assert_eq!(h, I2V_TEMPLATE_HASH, "golden i2v templateHash drifted");
    }
    // t2v graph hash is unmoved by adding i2v (the real "t2v untouched" invariant).
    assert_eq!(
        store.template_hash("ltx-t2v-hdr").unwrap(),
        HDR_TEMPLATE_HASH
    );
}

#[test]
fn test_flf2v_template_hash_stable() {
    let store = TemplateStore::new(DIR).unwrap();
    let h = store
        .template_hash("ltx-flf2v-hdr")
        .expect("flf2v template present");
    eprintln!("GOLD FLF2V_TEMPLATE_HASH={h}");
    assert!(h.starts_with("0x") && h.len() == 66, "hash shape: {h}");
    let raw = std::fs::read(format!("{DIR}/ltx-flf2v-hdr/v1.json")).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let expect = keccak_hex(serde_json::to_vec(&sort_json_keys(&v)).unwrap());
    assert_eq!(h, expect);
    // Curated so the patcher's handles match: exactly one "Prompt"-titled node
    // (the POSITIVE CLIPTextEncode 129:128), plus Height / Frame Rate.
    let titles: Vec<&str> = v
        .as_object()
        .unwrap()
        .values()
        .filter_map(|n| n.pointer("/_meta/title").and_then(|t| t.as_str()))
        .collect();
    assert_eq!(
        titles.iter().filter(|t| **t == "Prompt").count(),
        1,
        "exactly one Prompt handle"
    );
    assert_eq!(
        v.pointer("/129:128/_meta/title").and_then(|t| t.as_str()),
        Some("Prompt"),
        "the positive CLIPTextEncode is the Prompt handle"
    );
    if FLF2V_TEMPLATE_HASH != "0x__TBD__" {
        assert_eq!(h, FLF2V_TEMPLATE_HASH, "golden flf2v templateHash drifted");
    }
    // t2v + i2v graph hashes are unmoved by adding flf2v.
    assert_eq!(
        store.template_hash("ltx-t2v-hdr").unwrap(),
        HDR_TEMPLATE_HASH
    );
    assert_eq!(
        store.template_hash("ltx-i2v-hdr").unwrap(),
        I2V_TEMPLATE_HASH
    );
}

#[test]
fn test_iclora_template_hash_stable() {
    let store = TemplateStore::new(DIR).unwrap();
    let h = store
        .template_hash("ltx-iclora-hdr")
        .expect("iclora template present");
    eprintln!("GOLD ICLORA_TEMPLATE_HASH={h}");
    assert!(h.starts_with("0x") && h.len() == 66, "hash shape: {h}");
    // Independent recompute (same canonical path the impl uses).
    let raw = std::fs::read(format!("{DIR}/ltx-iclora-hdr/v1.json")).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let expect = keccak_hex(serde_json::to_vec(&sort_json_keys(&v)).unwrap());
    assert_eq!(h, expect);
    // Prompt-enhance baked ON (the BL2 grey-output lesson; the U0 preflight ran
    // with it ON and produced real styled AV output).
    assert_eq!(
        v.pointer("/129:212/inputs/value"),
        Some(&serde_json::Value::Bool(true)),
        "Enable Prompt Enhance must be baked ON"
    );
    if ICLORA_TEMPLATE_HASH != "0x__TBD__" {
        assert_eq!(
            h, ICLORA_TEMPLATE_HASH,
            "golden iclora templateHash drifted"
        );
    }
    // The three existing graph hashes are unmoved by adding iclora.
    assert_eq!(
        store.template_hash("ltx-t2v-hdr").unwrap(),
        HDR_TEMPLATE_HASH
    );
    assert_eq!(
        store.template_hash("ltx-i2v-hdr").unwrap(),
        I2V_TEMPLATE_HASH
    );
    assert_eq!(
        store.template_hash("ltx-flf2v-hdr").unwrap(),
        FLF2V_TEMPLATE_HASH
    );
}

#[test]
fn test_bl4_template_hashes_stable() {
    let store = TemplateStore::new(DIR).unwrap();
    for (id, golden) in [
        ("ltx-outpaint-hdr", OUTPAINT_TEMPLATE_HASH),
        ("ltx-edit-hdr", EDIT_TEMPLATE_HASH),
        ("ltx-restore-hdr", RESTORE_TEMPLATE_HASH),
    ] {
        let h = store.template_hash(id).expect("template present");
        eprintln!("GOLD {id}={h}");
        assert!(h.starts_with("0x") && h.len() == 66, "hash shape: {h}");
        // Independent recompute (same canonical path the impl uses).
        let raw = std::fs::read(format!("{DIR}/{id}/v1.json")).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        let expect = keccak_hex(serde_json::to_vec(&sort_json_keys(&v)).unwrap());
        assert_eq!(h, expect, "{id}");
        if golden != "0x__TBD__" {
            assert_eq!(h, golden, "golden {id} templateHash drifted");
        }
        // The bundle advertises the video-conditioned commitment selector: one
        // control clip, NO images (the dummy-still branch was stripped — v3
        // commitment with empty imageHashes).
        assert_eq!(store.video_inputs(id), Some(1), "{id}: videoInputs");
        assert_eq!(store.image_inputs(id), Some(0), "{id}: imageInputs");
        // Patcher handles present: one CLIPTextEncode titled Prompt, Width /
        // Height primitives, RandomNoise seed, one VHS_LoadVideo with a
        // frame_load_cap leaf.
        let obj = v.as_object().unwrap();
        let by_title = |t: &str| {
            obj.values()
                .filter(|n| n.pointer("/_meta/title").and_then(|x| x.as_str()) == Some(t))
                .count()
        };
        assert_eq!(by_title("Prompt"), 1, "{id}: one Prompt handle");
        assert_eq!(by_title("Width"), 1, "{id}: Width handle");
        assert_eq!(by_title("Height"), 1, "{id}: Height handle");
        let vhs: Vec<&serde_json::Value> = obj
            .values()
            .filter(|n| n.get("class_type").and_then(|c| c.as_str()) == Some("VHS_LoadVideo"))
            .collect();
        assert_eq!(vhs.len(), 1, "{id}: one VHS_LoadVideo");
        assert!(
            vhs[0].pointer("/inputs/frame_load_cap").is_some(),
            "{id}: frame_load_cap leaf present"
        );
        // Frozen-neutral frame widgets (the billed==rendered precondition).
        assert_eq!(
            vhs[0].pointer("/inputs/select_every_nth"),
            Some(&serde_json::json!(1)),
            "{id}: select_every_nth frozen at 1"
        );
        assert_eq!(
            vhs[0].pointer("/inputs/force_rate"),
            Some(&serde_json::json!(0)),
            "{id}: force_rate frozen at 0"
        );
        assert_eq!(
            vhs[0].pointer("/inputs/skip_first_frames"),
            Some(&serde_json::json!(0)),
            "{id}: skip_first_frames frozen at 0"
        );
    }
    // The four existing graph hashes are unmoved by adding the BL4 trio.
    let store = TemplateStore::new(DIR).unwrap();
    assert_eq!(
        store.template_hash("ltx-t2v-hdr").unwrap(),
        HDR_TEMPLATE_HASH
    );
    assert_eq!(
        store.template_hash("ltx-i2v-hdr").unwrap(),
        I2V_TEMPLATE_HASH
    );
    assert_eq!(
        store.template_hash("ltx-flf2v-hdr").unwrap(),
        FLF2V_TEMPLATE_HASH
    );
    assert_eq!(
        store.template_hash("ltx-iclora-hdr").unwrap(),
        ICLORA_TEMPLATE_HASH
    );
}

#[test]
fn test_iclora_handles() {
    let raw = std::fs::read(format!("{DIR}/ltx-iclora-hdr/v1.json")).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let obj = v.as_object().unwrap();
    let title_of = |n: &serde_json::Value| {
        n.pointer("/_meta/title")
            .and_then(|t| t.as_str())
            .map(String::from)
    };
    let class_of = |n: &serde_json::Value| {
        n.get("class_type")
            .and_then(|c| c.as_str())
            .map(String::from)
    };

    // Exactly one Prompt-titled node: a PrimitiveStringMultiline with a leaf
    // value, wired into BOTH prompt consumers (authoring may rewire; runtime
    // patching may not — the archive graph inlined the prompt as two literals).
    let prompt_ids: Vec<&String> = obj
        .iter()
        .filter(|(_, n)| title_of(n).as_deref() == Some("Prompt"))
        .map(|(id, _)| id)
        .collect();
    assert_eq!(prompt_ids.len(), 1, "exactly one Prompt handle");
    let pid = prompt_ids[0].clone();
    assert_eq!(
        class_of(&v[&pid]).as_deref(),
        Some("PrimitiveStringMultiline"),
        "Prompt node class"
    );
    assert!(
        v.pointer(&format!("/{pid}/inputs/value"))
            .and_then(|s| s.as_str())
            .is_some(),
        "Prompt has a leaf value"
    );
    let link = serde_json::json!([pid, 0]);
    assert_eq!(
        v.pointer("/129:209/inputs/prompt"),
        Some(&link),
        "TextGenerateLTX2Prompt reads the Prompt node"
    );
    assert_eq!(
        v.pointer("/129:211/inputs/on_false"),
        Some(&link),
        "enhance-switch on_false reads the Prompt node"
    );
    assert_eq!(
        v.pointer("/129:211/inputs/switch").and_then(|s| s.get(0)),
        Some(&serde_json::Value::String("129:212".to_string())),
        "switch driven by the enhance boolean"
    );

    // Dim/rate/duration handles: the exact titles the patcher matches, once each.
    for title in ["Width", "Height", "Frame Rate", "Duration"] {
        let n = obj
            .values()
            .filter(|n| title_of(n).as_deref() == Some(title))
            .count();
        assert_eq!(n, 1, "exactly one {title} handle");
    }
    // Duration drives the Video Slice: output frames derive from the sliced
    // control clip, so patching Duration keeps sliced length == billed seconds.
    let did = obj
        .iter()
        .find(|(_, n)| title_of(n).as_deref() == Some("Duration"))
        .map(|(id, _)| id.clone())
        .unwrap();
    assert_eq!(
        v.pointer("/692/inputs/duration"),
        Some(&serde_json::json!([did, 0])),
        "Duration drives Video Slice.duration"
    );

    // Seed lives in the plain KSampler (no RandomNoise anywhere) — the target of
    // the D3 seed-handle widening; the sampler chain is NOT re-plumbed.
    assert!(
        !obj.values()
            .any(|n| class_of(n).as_deref() == Some("RandomNoise")),
        "no RandomNoise in this graph"
    );
    let ksamplers: Vec<&serde_json::Value> = obj
        .values()
        .filter(|n| class_of(n).as_deref() == Some("KSampler"))
        .collect();
    assert_eq!(ksamplers.len(), 1, "exactly one KSampler");
    assert!(
        ksamplers[0]
            .pointer("/inputs/seed")
            .and_then(|s| s.as_u64())
            .is_some(),
        "KSampler seed is a numeric leaf"
    );

    // One reference still + one control video.
    let count = |cls: &str| {
        obj.values()
            .filter(|n| class_of(n).as_deref() == Some(cls))
            .count()
    };
    assert_eq!(count("LoadImage"), 1, "one reference still");
    assert_eq!(count("LoadVideo"), 1, "one control video");
}

#[test]
fn test_bundle_v6_has_iclora() {
    let store = TemplateStore::new(DIR).unwrap();
    let b = store.bundle();
    assert!(b.allow_list_version >= 16, "since v16"); // exact pins rot on every bump
    let ic = b
        .templates
        .iter()
        .find(|t| t.template_id == "ltx-iclora-hdr")
        .unwrap();
    assert_eq!(ic.image_inputs, 1);
    assert_eq!(ic.image_semantics, vec!["reference".to_string()]);
    assert_eq!(ic.video_inputs, 1);
    assert_eq!(ic.video_semantics, vec!["controlVideo".to_string()]);
    // First video input on the seam ⇒ video bounds advertised.
    assert_eq!(b.bounds.video_max_bytes, 134_217_728, "128 MiB video cap");
    assert_eq!(b.bounds.video_formats, vec!["mp4"]);
    // The old three carry an explicit videoInputs 0 and no semantics.
    for id in ["ltx-t2v-hdr", "ltx-i2v-hdr", "ltx-flf2v-hdr"] {
        let t = b.templates.iter().find(|t| t.template_id == id).unwrap();
        assert_eq!(t.video_inputs, 0, "{id} has no video input");
        assert!(t.video_semantics.is_empty(), "{id} has no video semantics");
    }
    // The handler's video-count accessor mirrors image_inputs.
    assert_eq!(store.video_inputs("ltx-iclora-hdr"), Some(1));
    assert_eq!(store.video_inputs("ltx-t2v-hdr"), Some(0));
    assert_eq!(store.video_inputs("nope"), None);
    // THE v6 invariant: the three pre-existing templateHash VALUES equal the
    // LIVE v5 values (regression against an accidental re-pin — presupposes the
    // U0 i2v enhance=true reconciliation).
    let hash_of = |id: &str| {
        &b.templates
            .iter()
            .find(|t| t.template_id == id)
            .unwrap()
            .template_hash
    };
    assert_eq!(hash_of("ltx-t2v-hdr"), HDR_TEMPLATE_HASH);
    assert_eq!(hash_of("ltx-i2v-hdr"), I2V_TEMPLATE_HASH);
    assert_eq!(hash_of("ltx-flf2v-hdr"), FLF2V_TEMPLATE_HASH);
}

#[test]
fn test_bundle_v7_has_bl4_trio() {
    let store = TemplateStore::new(DIR).unwrap();
    let b = store.bundle();
    assert!(b.allow_list_version >= 16, "since v16"); // exact pins rot on every bump
    for id in ["ltx-outpaint-hdr", "ltx-edit-hdr", "ltx-restore-hdr"] {
        let t = b.templates.iter().find(|t| t.template_id == id).unwrap();
        assert_eq!(t.video_inputs, 1, "{id}: one control video");
        assert_eq!(t.video_semantics, vec!["controlVideo".to_string()]);
        assert_eq!(
            t.image_inputs, 0,
            "{id}: NO image inputs (v3 selector with empty imageHashes)"
        );
        assert!(t.image_semantics.is_empty(), "{id}: no image semantics");
    }
    // The lora ids ride the bundle for client discovery.
    for lora in [
        "ltx-iclora-hdr@v1",
        "ltx-outpaint-hdr@v1",
        "ltx-edit-hdr@v1",
        "ltx-restore-hdr@v1",
        "ltx-ingredients-hdr@v1",
        "ltx-water-hdr@v1",
        "ltx-daynight-hdr@v1",
    ] {
        assert!(b.loras.iter().any(|l| l == lora), "lora {lora} advertised");
    }
    // Bounds are UNCHANGED by v7 (same ladder, caps, fps set as v6).
    assert_eq!(b.bounds.video_max_bytes, 134_217_728);
    assert_eq!(b.bounds.image_max_bytes, 33_554_432);
}

#[test]
fn test_bundle_v3_has_flf2v() {
    let store = TemplateStore::new(DIR).unwrap();
    let b = store.bundle();
    assert!(b.allow_list_version >= 16, "since v16"); // exact pins rot on every bump
    let flf = b
        .templates
        .iter()
        .find(|t| t.template_id == "ltx-flf2v-hdr")
        .unwrap();
    assert_eq!(flf.image_inputs, 2);
    assert_eq!(
        flf.image_semantics,
        vec!["firstFrame".to_string(), "lastFrame".to_string()]
    );
    assert_eq!(store.image_inputs("ltx-flf2v-hdr"), Some(2));
}

#[test]
fn test_bundle_v4_resolution_ladder() {
    // v4: the full ladder up to 4K (LTX 2.3 renders it; the old list was the
    // M0 conservative pair). Landscape + portrait mirrors + square. COST NOTE:
    // 3840×2160×121f = 1,003,623 tokens ≈ $0.91 gross at price 904 — ABOVE the
    // $0.50 floor deposit; the SDK must size deposits from ltxTokens(job).
    let store = TemplateStore::new(DIR).unwrap();
    let b = store.bundle();
    assert!(b.allow_list_version >= 16, "since v16"); // exact pins rot on every bump
    let expect = [
        (768u32, 512u32),
        (1280, 720),
        (1920, 1080),
        (2560, 1440),
        (3840, 2160),
        (512, 768),
        (720, 1280),
        (1080, 1920),
        (1024, 1024),
        // v8: /64-clean HD + QHD. floor(dim/32) is EVEN in both axes, so the IC-LoRA guide
        // accepts them AND the VAE renders them EXACTLY (1088 == 34*32, no truncation).
        (1920, 1088),
        (1088, 1920),
        (2560, 1408),
        (1408, 2560),
    ];
    for (w, h) in expect {
        assert!(
            b.bounds.resolutions.iter().any(|r| r.w == w && r.h == h),
            "resolution {w}x{h} missing from the v4 ladder"
        );
    }
    // 4K input stills for i2v/flf2v need headroom over the old 8 MiB.
    assert_eq!(b.bounds.image_max_bytes, 33_554_432, "32 MiB image cap");
}

#[test]
fn test_bundle_v2_has_image_inputs() {
    let store = TemplateStore::new(DIR).unwrap();
    let b = store.bundle();
    assert!(
        b.allow_list_version >= 16,
        "since v16 (t2v/i2v entries unchanged since v8; exact pins rot on every bump)"
    );
    let t2v = b
        .templates
        .iter()
        .find(|t| t.template_id == "ltx-t2v-hdr")
        .unwrap();
    assert_eq!(t2v.image_inputs, 0, "t2v is the format-0 selector");
    assert!(t2v.image_semantics.is_empty());
    let i2v = b
        .templates
        .iter()
        .find(|t| t.template_id == "ltx-i2v-hdr")
        .unwrap();
    assert_eq!(i2v.image_inputs, 1);
    assert_eq!(i2v.image_semantics, vec!["firstFrame".to_string()]);
    // Image bounds advertised (v4 raised the cap for 4K stills).
    assert_eq!(b.bounds.image_max_bytes, 33_554_432);
    assert_eq!(b.bounds.image_formats, vec!["png", "jpeg", "webp"]);
    // The handler's format-selector accessor.
    assert_eq!(store.image_inputs("ltx-i2v-hdr"), Some(1));
    assert_eq!(store.image_inputs("ltx-t2v-hdr"), Some(0));
    assert_eq!(store.image_inputs("nope"), None);
}

/// Emit `tests/ltx/bundle-fixture.json` — the versioned allow-list + bounds bundle for
/// the SDK to (a) shape its `validateJob` types against and (b) verify its `bundleHash`
/// canonical recompute (remove `bundleHash` -> recursively sort keys -> compact ->
/// keccak256). Self-proves the canonicalisation so a broken rule fails the node test too.
#[test]
fn emit_bundle_fixture() {
    let store = TemplateStore::new(DIR).unwrap();
    let bundle = store.bundle();

    // The exact canonicalisation the SDK must reproduce.
    let mut v = serde_json::to_value(bundle).unwrap();
    v.as_object_mut().unwrap().remove("bundleHash");
    let recomputed = keccak_hex(serde_json::to_vec(&sort_json_keys(&v)).unwrap());
    assert_eq!(
        recomputed, bundle.bundle_hash,
        "bundleHash canonical recompute"
    );

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/bundle-fixture.json");
    std::fs::write(path, serde_json::to_vec_pretty(bundle).unwrap()).unwrap();
    assert!(std::path::Path::new(path).exists());
}

/// Emit `tests/ltx/bundle-fixture-v2.json` — the M1a allow-list bundle. Adds
/// `template.imageInputs` (the commitment format selector), `imageSemantics`
/// (advisory ordering), and `bounds.imageMaxBytes` / `imageFormats`. Carries a
/// t2v (imageInputs 0) AND image templates so format-selection is fixture-tested.
///
/// Template hashes here are DETERMINISTIC PLACEHOLDERS, not production pins: the
/// real i2v hash lands at the v2 re-pin (once prompt-enhance is baked off). What
/// this fixture proves is (a) the v2 schema shape and (b) that `bundleHash`
/// recomputes under the SAME canonical rule the node uses (drop `bundleHash` ->
/// recursively sort keys -> compact -> keccak256).
#[test]
fn emit_bundle_fixture_v2() {
    let th_t2v = format!("0x{}", "11".repeat(32));
    let th_i2v = format!("0x{}", "22".repeat(32));
    let th_flf = format!("0x{}", "33".repeat(32));

    let mut bundle = serde_json::json!({
        "allowListVersion": 2,
        "bundleHash": "",
        // Sorted by templateId, mirroring the node's `templates.sort_by` before hashing.
        "templates": [
            { "templateId": "ltx-flf2v-hdr", "templateHash": th_flf, "imageInputs": 2,
              "imageSemantics": ["firstFrame", "lastFrame"] },
            { "templateId": "ltx-i2v-hdr",   "templateHash": th_i2v, "imageInputs": 1,
              "imageSemantics": ["firstFrame"] },
            { "templateId": "ltx-t2v-hdr",   "templateHash": th_t2v, "imageInputs": 0 }
        ],
        "loras": ["ltx-iclora-hdr@v1"],
        "bounds": {
            // Delivered = 5·fps + 1, so 121 at fps 24 .. 126 at fps 25 (§G honesty).
            "frames": { "min": 121, "max": 126 },
            "fps": [24, 25, 30],
            "resolutions": [ { "w": 768, "h": 512 }, { "w": 1280, "h": 720 } ],
            "imageMaxBytes": 8388608,
            "imageFormats": ["png", "jpeg", "webp"]
        }
    });

    // Canonical bundleHash: remove the hash field -> sort keys -> compact -> keccak.
    let mut without = bundle.clone();
    without.as_object_mut().unwrap().remove("bundleHash");
    let hash = keccak_hex(serde_json::to_vec(&sort_json_keys(&without)).unwrap());
    bundle.as_object_mut().unwrap().insert(
        "bundleHash".to_string(),
        serde_json::Value::String(hash.clone()),
    );

    // Self-prove the canonical recompute, so a broken rule fails the node test too.
    let mut re = bundle.clone();
    re.as_object_mut().unwrap().remove("bundleHash");
    assert_eq!(
        keccak_hex(serde_json::to_vec(&sort_json_keys(&re)).unwrap()),
        hash,
        "v2 bundleHash canonical recompute"
    );

    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/ltx/bundle-fixture-v2.json"
    );
    std::fs::write(path, serde_json::to_vec_pretty(&bundle).unwrap()).unwrap();
    assert!(std::path::Path::new(path).exists());
}

/// Bundle v10: the upscale template (v9 + the INT->FLOAT fps shim ComfyUI demands) plus its x2 output rungs. The graph is the
/// proven ComfyUI probe of 2026-07-15 with patcher handles added; sigmas 0.60/0.40/0.20/0
/// are pinned as a fidelity constant (0.85 = the t2v generator setting = redesigns content).
const UPSCALE_TEMPLATE_HASH: &str =
    "0xa36ed9d96fc264b8d97cd03a4628dd1c30161a6a83a7442185c9f58945aaffb5";

#[test]
fn test_bundle_v11_has_upscale() {
    let store = TemplateStore::new(DIR).expect("store loads");
    assert!(store.bundle().allow_list_version >= 16); // since v16; exact pins rot on every bump
    eprintln!("GOLD ltx-upscale-hdr={}", store.template_hash("ltx-upscale-hdr").unwrap());
    assert_eq!(
        store
            .template_hash("ltx-upscale-hdr")
            .expect("upscale template pinned"),
        UPSCALE_TEMPLATE_HASH
    );
    // videoInputs routes the control-clip plumbing; imageInputs stays zero.
    assert_eq!(store.video_inputs("ltx-upscale-hdr"), Some(1));
    assert_eq!(store.image_inputs("ltx-upscale-hdr"), Some(0));
    // the x2 output rungs are in the ladder
    let rs = &store.bundle().bounds.resolutions;
    for (w, h) in [(1536u32, 1024u32), (1024, 1536), (3840, 2176), (2176, 3840)] {
        assert!(
            rs.iter().any(|r| r.w == w && r.h == h),
            "missing rung {w}x{h}"
        );
    }
}

/// Bundle v14: the Ingredients (cast-consistency) template — I-phase. The graph is the
/// author distilled single-stage recipe proven on 3XS-Z 2026-07-18 (Lightricks' own
/// reference sheet reproduced their demo on the fp8 stack), with the four house patcher
/// handles added and the canvas made COMMITTED (sheet conformed in-graph, black pad).
/// Version history: v12 and v13 BURNED on the ComfyMathExpression output-index bug
/// ([0] is FLOAT, [1] is INT — never re-pin to them); v14 went live (session 921 paid
/// to the token); v15 adds the ingredients lora id to the bundle's loras advert so
/// clients commit the LoRA that actually ran, not the iclora fallback.
const INGREDIENTS_TEMPLATE_HASH: &str =
    "0x9c1a9a0cc84bfd3e79c325cec593e8f838fca4a3eb0f7079b9529f6c53a7c9dc";

#[test]
fn test_bundle_v14_has_ingredients() {
    let store = TemplateStore::new(DIR).expect("store loads");
    assert!(store.bundle().allow_list_version >= 16); // since v16; exact pins rot on every bump
    eprintln!("GOLD ltx-ingredients-hdr={}", store.template_hash("ltx-ingredients-hdr").unwrap());
    assert_eq!(
        store
            .template_hash("ltx-ingredients-hdr")
            .expect("ingredients template pinned"),
        INGREDIENTS_TEMPLATE_HASH
    );
    // ONE reference sheet, bound in the commitment; no control video, no conform render.
    assert_eq!(store.image_inputs("ltx-ingredients-hdr"), Some(1));
    assert_eq!(store.video_inputs("ltx-ingredients-hdr"), Some(0));
    // v15: the mode's OWN lora id is advertised — the helper's templateLora prefers
    // `<templateId>@v1` when the bundle carries it, so the attestation commits the
    // ingredients LoRA (the one new weight) instead of the loras[0] iclora fallback.
    assert!(
        store
            .bundle()
            .loras
            .iter()
            .any(|l| l == "ltx-ingredients-hdr@v1"),
        "ingredients lora advertised"
    );
}

/// Bundle v16: Water Simulation + Day-To-Night — the WA/DN combined ladder. Both
/// graphs are the PROVEN edit spine (same 30-node shape, same distilled recipe:
/// cfg 1, 8-step sigmas, guide factor 1) with only the mode IC-LoRA swapped —
/// water at the card's 1.2 sweet spot, day-to-night at 1.0. Discovery 2026-07-18
/// (headless POST runs on 3XS-Z): water GO incl. 1920x1088 exact delivery, 10 s
/// alive, (F-1)%8 rule SOFT; day-to-night GO on the distilled spine (the card's
/// 30-step full recipe NOT needed — clean sources show no artifacting). Both lora
/// ids advertised from day one (the v15 lesson).
const WATER_TEMPLATE_HASH: &str =
    "0xc19ed1d99ae6e7fd214afb9ecb10b50f096a137b0c9a0f207e83fc982828ba39";
const DAYNIGHT_TEMPLATE_HASH: &str =
    "0xcc1ea6b5eec43f3896fbcd70d2d174c558fcf11546ca73300de328bbad260c74";

#[test]
fn test_bundle_v16_has_water_and_daynight() {
    let store = TemplateStore::new(DIR).expect("store loads");
    assert!(store.bundle().allow_list_version >= 16); // since v16; exact pins rot on every bump
    for (id, golden) in [
        ("ltx-water-hdr", WATER_TEMPLATE_HASH),
        ("ltx-daynight-hdr", DAYNIGHT_TEMPLATE_HASH),
    ] {
        let h = store.template_hash(id).expect("template pinned");
        eprintln!("GOLD {id}={h}");
        if golden != "0x__TBD__" {
            assert_eq!(h, golden, "{id} golden hash drifted");
        }
        // Control-clip shape: one video in, no stills, no conform render needed
        // client-side beyond the standard BL4 path.
        assert_eq!(store.image_inputs(id), Some(0), "{id} imageInputs");
        assert_eq!(store.video_inputs(id), Some(1), "{id} videoInputs");
        // The mode's OWN lora id is advertised so the attestation commits the
        // LoRA that actually ran (the v15 lesson, applied from day one).
        let own = format!("{id}@v1");
        assert!(
            store.bundle().loras.iter().any(|l| l == &own),
            "{id} lora advertised"
        );
    }
}

const CROSSVIEW_TEMPLATE_HASH: &str =
    "0x411f192cffc8417788f2710eb305477427a84a94c3b2e79ae4f22276d80c35ad";

/// v17 (CV1): the crossview novel-view template — control-clip shape, camera
/// pose pinned mild (azimuth 20 / elevation 0 / distance 1.0), single pass at
/// the picked resolution (upscale is its own paid mode; no second pass).
#[test]
fn test_bundle_v17_has_crossview() {
    let store = TemplateStore::new(DIR).expect("store loads");
    assert!(store.bundle().allow_list_version >= 18); // v18: crossview regained its x2 refine pass
    let h = store
        .template_hash("ltx-crossview-hdr")
        .expect("template pinned");
    eprintln!("GOLD ltx-crossview-hdr={h}");
    if CROSSVIEW_TEMPLATE_HASH != "0x__TBD__" {
        assert_eq!(h, CROSSVIEW_TEMPLATE_HASH, "crossview golden hash drifted");
    }
    assert_eq!(store.video_inputs("ltx-crossview-hdr"), Some(1));
    assert_eq!(store.image_inputs("ltx-crossview-hdr").unwrap_or(0), 0);
}
