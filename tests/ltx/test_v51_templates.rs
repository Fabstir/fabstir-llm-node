// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Templates v51 (fix-91, session 1620: Image to Video turned a street still into a wheat field). The Image to Video
//! prompt step (`TextGenerateLTX2Prompt`) rewrote the user's prompt from text alone, so it invented a scene. In v51 the
//! job's still is wired into that step's `image` input, as Restyle (`ltx-iclora-hdr`) already does. `fixtures/v51/` is
//! `fixtures/v50/` with that ONE link added in ONE graph and the allow-list version. Each test names the mutation that
//! turns it red.
use fabstir_llm_node::ltx::template::{check_bundle_floor, TemplateStore, MIN_BUNDLE_VERSION};

const V50: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/fixtures/v50");
const V51: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/fixtures/v51");
const HASHES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/ltx/fixtures/v51-template-hashes.json"
);

#[test]
fn the_i2v_prompt_step_sees_the_still() {
    // delete the `image` input of the i2v prompt step, or point it at anything but the job's LoadImage -> red
    let store = TemplateStore::new(V51).unwrap();
    let g = store.graph("ltx-i2v-hdr").unwrap();
    let nodes = g.0.as_object().unwrap();
    let gen: Vec<_> = nodes
        .values()
        .filter(|n| n["class_type"] == "TextGenerateLTX2Prompt")
        .collect();
    assert_eq!(gen.len(), 1, "one prompt improvement step");
    let image = gen[0]["inputs"]
        .get("image")
        .expect("the prompt step has an image input");
    let (src, slot) = (image[0].as_str().unwrap(), image[1].as_u64().unwrap());
    assert_eq!(slot, 0, "the loader's image output");
    assert_eq!(nodes[src]["class_type"], "LoadImage");
    // the one still the job binds is that very loader (the patcher binds LoadImage nodes; there is exactly one)
    let loaders: Vec<_> = nodes
        .iter()
        .filter(|(_, n)| n["class_type"] == "LoadImage")
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(loaders, vec![src], "the still is the graph's only LoadImage");
    // the enhancer stays on and still reads the user's prompt
    assert_eq!(gen[0]["inputs"]["use_default_template"], true);
    assert!(gen[0]["inputs"]["prompt"].is_array());
}

#[test]
fn v50_i2v_had_no_image_input() {
    // the defect itself: the v50 graph's prompt step had none (this fixture must keep documenting the old state)
    let store = TemplateStore::new(V50).unwrap();
    let g = store.graph("ltx-i2v-hdr").unwrap();
    let gen = g
        .0
        .as_object()
        .unwrap()
        .values()
        .find(|n| n["class_type"] == "TextGenerateLTX2Prompt")
        .unwrap();
    assert!(gen["inputs"].get("image").is_none());
}

#[test]
fn only_the_i2v_graph_and_the_version_changed() {
    // any other graph or allow-list field edited -> red
    let (old, new) = (
        TemplateStore::new(V50).unwrap(),
        TemplateStore::new(V51).unwrap(),
    );
    assert_eq!(old.bundle().allow_list_version, 50);
    assert_eq!(new.bundle().allow_list_version, 51);
    let mut a = serde_json::to_value(old.bundle()).unwrap();
    let mut b = serde_json::to_value(new.bundle()).unwrap();
    for v in [&mut a, &mut b] {
        let o = v.as_object_mut().unwrap();
        o.remove("allowListVersion");
        o.remove("bundleHash");
        for t in o["templates"].as_array_mut().unwrap() {
            t.as_object_mut().unwrap().remove("templateHash");
        }
    }
    assert_eq!(a, b, "the allow-list differs only in its version and the i2v hash");
    for e in &new.bundle().templates {
        let id = e.template_id.as_str();
        assert_eq!(
            old.template_hash(id) == new.template_hash(id),
            id != "ltx-i2v-hdr",
            "{id}"
        );
    }
    // exactly one node of the i2v graph differs, and only by the added link
    let (og, ng) = (
        old.graph("ltx-i2v-hdr").unwrap(),
        new.graph("ltx-i2v-hdr").unwrap(),
    );
    let (on, nn) = (og.0.as_object().unwrap(), ng.0.as_object().unwrap());
    assert_eq!(on.len(), nn.len());
    let differing: Vec<_> = nn.iter().filter(|(k, v)| on.get(*k) != Some(*v)).collect();
    assert_eq!(differing.len(), 1);
    let mut stripped = differing[0].1.clone();
    stripped["inputs"].as_object_mut().unwrap().remove("image");
    assert_eq!(&stripped, &on[differing[0].0.as_str()]);
}

#[test]
fn v51_hashes_match_the_golden_and_the_node_routine() {
    // the helper pins the new i2v hash: an edit that moves a hash without the golden -> red. The bundle hash here is
    // the node's own canonical routine (TemplateStore), not the offline replica.
    let store = TemplateStore::new(V51).unwrap();
    let golden: serde_json::Value =
        serde_json::from_slice(&std::fs::read(HASHES).unwrap()).unwrap();
    assert_eq!(
        golden["v51TreeHash"].as_str(),
        Some(store.bundle().bundle_hash.as_str())
    );
    assert_eq!(
        golden["v50TreeHash"].as_str(),
        Some(TemplateStore::new(V50).unwrap().bundle().bundle_hash.as_str())
    );
    let t = golden["templates"].as_object().unwrap();
    assert_eq!(t.len(), store.bundle().templates.len());
    let old = TemplateStore::new(V50).unwrap();
    for (id, h) in t {
        assert_eq!(
            store.template_hash(id).map(|s| s.to_lowercase()),
            h["v51"].as_str().map(|s| s.to_lowercase()),
            "{id}"
        );
        assert_eq!(
            old.template_hash(id).map(|s| s.to_lowercase()),
            h["v50"].as_str().map(|s| s.to_lowercase()),
            "{id}"
        );
    }
}

#[test]
fn the_floor_stays_50_so_both_bundles_load() {
    // floor raised to 51 (rollback to v50 would then stop every video job) -> red
    assert_eq!(MIN_BUNDLE_VERSION, 50);
    check_bundle_floor(&TemplateStore::new(V50).unwrap()).unwrap();
    check_bundle_floor(&TemplateStore::new(V51).unwrap()).unwrap();
}
