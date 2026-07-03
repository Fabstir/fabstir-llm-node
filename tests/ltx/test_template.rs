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
/// i2v graph hash (prompt-enhance baked OFF).
const I2V_TEMPLATE_HASH: &str =
    "0x1074478991672ae0e0c668c1adf13f9e04dcd79edaaebfa5ae25f8d63c7831bd";
/// Bundle hash MOVES at the v2 bump (adds the i2v template + image fields); the
/// t2v graph hash above must NOT move.
const BUNDLE_HASH: &str = "0xa1deab6fcafab6e986788ee55ed4179abd78cf8b3fdb578dae1210dd32ebfe94";

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
    // The pinned i2v template must ship prompt-enhance OFF (provenance honesty).
    assert_eq!(
        v.pointer("/320:328/inputs/value"),
        Some(&serde_json::Value::Bool(false)),
        "Enable Prompt Enhance must be baked OFF"
    );
    // ...and that OFF must actually select the RAW prompt: the enhance boolean
    // (320:328) drives the ComfySwitchNode (320:327) whose `on_false` is the
    // patched Prompt node (320:319) and `on_true` is the gemma rewrite (320:325).
    // This is the provenance contract — `inputCommitment` binds 320:319's text, so
    // switch=false is what makes the committed prompt the one that conditions the
    // render. (The templateHash golden also locks it, but assert intent explicitly.)
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
fn test_bundle_v2_has_image_inputs() {
    let store = TemplateStore::new(DIR).unwrap();
    let b = store.bundle();
    assert_eq!(b.allow_list_version, 2, "v2 allow-list");
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
    // Image bounds advertised.
    assert_eq!(b.bounds.image_max_bytes, 8_388_608);
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
