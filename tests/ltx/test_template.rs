// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 2 template store + versioned allow-list bundle tests.

use fabstir_llm_node::checkpoint::delta::sort_json_keys;
use fabstir_llm_node::ltx::template::TemplateStore;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/templates");

/// Golden oracle: locks the wire contract for the pinned fixture. Update ONLY on
/// an intentional template or canonicalisation change.
const HDR_TEMPLATE_HASH: &str =
    "0xd67dfae8ea7da02516af56bd39d7bf4dedebb65da09d0e520e7cc1c7bb5fe078";
/// i2v graph hash (prompt-enhance baked ON — the BL2 grey-output fix; matches the
/// template deployed live in bundle v5).
const I2V_TEMPLATE_HASH: &str =
    "0xa4c890fd5f9a24a778c2a2ab00be2141dcb2c801a339d390016924292dff128c";
/// flf2v graph hash (curated: positive CLIPTextEncode retitled Prompt, height/Frame
/// Rate retitled to match the patcher handles).
const FLF2V_TEMPLATE_HASH: &str =
    "0x8bebde0f3bc0bf67f6f8efefe6fa742f2819edf6f70160541c756d62c9f96721";
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
    "0xef5588148be1ef3b34a859149ff3327b6acc70eec0bf1eda7c07e3b884c925e6";
/// Bundle hash MOVES at each bundle bump (v3 added flf2v; v4 the resolution
/// ladder + 32 MiB image cap; v5 the clip-duration bounds frames {121,751} and
/// corrected fps [24,25,48,50], with the i2v enhance=true re-pin landing within
/// v5 as the LIVE on-chain 0xb44beb2c…; v6 adds ltx-iclora-hdr + the video
/// bounds/videoInputs fields); the t2v/i2v/flf2v graph hashes above must NOT move.
const BUNDLE_HASH: &str = "0xaa6192be00b67f948227d4819e4157790322cf99332d3ffd1566b571cc396aa2";

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
    assert_eq!(b.allow_list_version, 6, "v6 allow-list");
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
fn test_bundle_v3_has_flf2v() {
    let store = TemplateStore::new(DIR).unwrap();
    let b = store.bundle();
    assert_eq!(b.allow_list_version, 6, "v6 allow-list");
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
    assert_eq!(b.allow_list_version, 6, "v6 allow-list");
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
    assert_eq!(
        b.allow_list_version, 6,
        "v6 allow-list (t2v/i2v entries unchanged)"
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
