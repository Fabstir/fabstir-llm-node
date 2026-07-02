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
const BUNDLE_HASH: &str = "0x2d3367be958b2f6132b0b7e090f7e0db3ff508b3a6feac099c0d86b8ff43241d";

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
