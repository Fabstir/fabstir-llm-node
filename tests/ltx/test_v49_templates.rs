// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! AP3 (IMPLEMENTATION-AI-PROVENANCE.md D8a): the v49 bundle stops ComfyUI writing its graph (prompt, seed,
//! weight names) into delivered mp4s. `fixtures/v49/` is the kit tree (`temp/rs-deploy22`) byte for byte.
use fabstir_llm_node::ltx::template::TemplateStore;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/fixtures/v49");
const HASHES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/ltx/fixtures/v49-template-hashes.json"
);

fn nodes_of(store: &TemplateStore, id: &str, class: &str) -> Vec<serde_json::Value> {
    let g = store.graph(id).unwrap();
    g.0.as_object()
        .unwrap()
        .values()
        .filter(|n| n["class_type"] == class)
        .cloned()
        .collect()
}

#[test]
fn vhs_never_saves_workflow_metadata() {
    let store = TemplateStore::new(DIR).unwrap();
    let mut vhs_templates = 0;
    for e in &store.bundle().templates {
        let vhs = nodes_of(&store, &e.template_id, "VHS_VideoCombine");
        for n in &vhs {
            assert_eq!(
                n["inputs"]["save_metadata"],
                serde_json::Value::Bool(false),
                "{}: VHS_VideoCombine must not embed the workflow",
                e.template_id
            );
        }
        vhs_templates += (!vhs.is_empty()) as usize;
    }
    assert_eq!(vhs_templates, 24);
}

/// The 10 core `SaveVideo` templates cannot be switched off in the graph: the host flag `--disable-metadata`
/// (host1 `.env` LTX_COMFY_ARGS) and the node's leak guard (AP2 `refuses_mp4_with_escaped_comfy_graph`) cover them.
#[test]
fn save_video_templates_rely_on_the_host_flag_and_the_node_guard() {
    let store = TemplateStore::new(DIR).unwrap();
    let n = store
        .bundle()
        .templates
        .iter()
        .filter(|e| !nodes_of(&store, &e.template_id, "SaveVideo").is_empty())
        .count();
    assert_eq!(n, 10);
}

#[test]
fn v49_template_hashes_match_the_kit() {
    // the node's canonical keccak must equal the kit's, or the helper/SDK pins would refuse every job
    let store = TemplateStore::new(DIR).unwrap();
    let golden: serde_json::Value =
        serde_json::from_slice(&std::fs::read(HASHES).unwrap()).unwrap();
    let t = golden["templates"].as_object().unwrap();
    assert_eq!(t.len(), store.bundle().templates.len());
    for (id, h) in t {
        assert_eq!(
            store.template_hash(id).map(|s| s.to_lowercase()),
            h["v49"].as_str().map(|s| s.to_lowercase()),
            "{id}"
        );
    }
}
