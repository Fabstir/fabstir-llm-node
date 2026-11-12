// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Templates v50 (IMPLEMENTATION-CONTENT-CHECKS-LTX.md D16): the refusal-removing Gemma adapter leaves the prompt
//! improvement step. `fixtures/v50/` is `fixtures/v49/` with ONE change in each of four graphs (the adapter's
//! `LoraLoader` is gone and the Gemma text encoder feeds `TextGenerateLTX2Prompt` directly) and the allow-list
//! version. Each test names the mutation that turns it red.
use fabstir_llm_node::ltx::template::{check_bundle_floor, TemplateStore, MIN_BUNDLE_VERSION};
use fabstir_llm_node::ltx::weights::weight_files;

const V49: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/fixtures/v49");
const V50: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/fixtures/v50");
const HASHES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/ltx/fixtures/v50-template-hashes.json"
);
const ADAPTER: &str = "gemma-3-12b-it-abliterated_lora_rank64_bf16.safetensors";
const CHANGED: [&str; 4] = [
    "ltx-t2v-hdr",
    "ltx-i2v-hdr",
    "ltx-iclora-hdr",
    "ltx-iclora-chain-hdr",
];

#[test]
fn no_v50_graph_loads_the_adapter() {
    // put the LoraLoader back in any graph -> red
    let store = TemplateStore::new(V50).unwrap();
    assert_eq!(store.bundle().templates.len(), 37);
    for e in &store.bundle().templates {
        let g = store.graph(&e.template_id).unwrap();
        assert!(
            !g.0.to_string().contains("abliterated"),
            "{} still names the adapter",
            e.template_id
        );
        // (the Cosmos relight entries carry no ComfyUI weight loaders, so their manifest does not derive)
        assert!(
            !weight_files(&g)
                .unwrap_or_default()
                .iter()
                .any(|f| f == ADAPTER),
            "{} still loads the adapter",
            e.template_id
        );
    }
}

#[test]
fn the_text_encoder_feeds_the_prompt_step_directly() {
    // wire `clip` to anything but the Gemma encoder loader (or leave a LoraLoader in between) -> red
    let store = TemplateStore::new(V50).unwrap();
    for id in CHANGED {
        let g = store.graph(id).unwrap();
        let nodes = g.0.as_object().unwrap();
        let gen: Vec<_> = nodes
            .values()
            .filter(|n| n["class_type"] == "TextGenerateLTX2Prompt")
            .collect();
        assert_eq!(gen.len(), 1, "{id}: one prompt improvement step");
        let src = gen[0]["inputs"]["clip"][0].as_str().unwrap();
        assert_eq!(
            gen[0]["inputs"]["clip"][1], 0,
            "{id}: the encoder's first output"
        );
        let loader = &nodes[src];
        assert_eq!(loader["class_type"], "LTXAVTextEncoderLoader", "{id}");
        assert_eq!(
            loader["inputs"]["text_encoder"], "gemma_3_12B_it_fp4_mixed.safetensors",
            "{id}: plain Gemma"
        );
        assert!(
            !nodes.values().any(|n| n["class_type"] == "LoraLoader"),
            "{id}: no model-and-CLIP LoRA loader remains"
        );
    }
}

#[test]
fn only_the_four_graphs_and_the_version_changed() {
    // any other graph or allow-list field edited -> red
    let (old, new) = (
        TemplateStore::new(V49).unwrap(),
        TemplateStore::new(V50).unwrap(),
    );
    assert_eq!(old.bundle().allow_list_version, 49);
    assert_eq!(new.bundle().allow_list_version, 50);
    let mut a = serde_json::to_value(old.bundle()).unwrap();
    let mut b = serde_json::to_value(new.bundle()).unwrap();
    for v in [&mut a, &mut b] {
        let o = v.as_object_mut().unwrap();
        o.remove("allowListVersion");
        o.remove("bundleHash");
        for t in o["templates"].as_array_mut().unwrap() {
            let t = t.as_object_mut().unwrap();
            t.remove("templateHash");
            // T2.1: the one allow-list addition of v50 (its own tests are in test_v50_rules.rs)
            t.remove("frameTiers");
        }
    }
    assert_eq!(
        a, b,
        "the allow-list differs only in its version and the four frameTiers entries"
    );
    for e in &new.bundle().templates {
        let id = e.template_id.as_str();
        let same = old.template_hash(id) == new.template_hash(id);
        assert_eq!(same, !CHANGED.contains(&id), "{id}");
        if !same {
            // exactly one node fewer, and every other node byte-identical apart from the rewired `clip`
            let (og, ng) = (old.graph(id).unwrap(), new.graph(id).unwrap());
            let (on, nn) = (og.0.as_object().unwrap(), ng.0.as_object().unwrap());
            assert_eq!(on.len(), nn.len() + 1, "{id}");
            let differing = nn.iter().filter(|(k, v)| on.get(*k) != Some(*v)).count();
            assert_eq!(differing, 1, "{id}: only the prompt step's node changed");
        }
    }
}

#[test]
fn v50_template_hashes_match_the_golden() {
    // the helper pins these four new hashes: a graph edit that moves a hash without the golden -> red
    let store = TemplateStore::new(V50).unwrap();
    let golden: serde_json::Value =
        serde_json::from_slice(&std::fs::read(HASHES).unwrap()).unwrap();
    assert_eq!(
        golden["v50TreeHash"].as_str(),
        Some(store.bundle().bundle_hash.as_str())
    );
    let t = golden["templates"].as_object().unwrap();
    assert_eq!(t.len(), store.bundle().templates.len());
    let old = TemplateStore::new(V49).unwrap();
    for (id, h) in t {
        assert_eq!(
            store.template_hash(id).map(|s| s.to_lowercase()),
            h["v50"].as_str().map(|s| s.to_lowercase()),
            "{id}"
        );
        assert_eq!(
            old.template_hash(id).map(|s| s.to_lowercase()),
            h["v49"].as_str().map(|s| s.to_lowercase()),
            "{id}"
        );
    }
}

#[test]
fn the_node_requires_bundle_50() {
    // floor lowered to 49, or the check dropped from main -> red
    assert_eq!(MIN_BUNDLE_VERSION, 50);
    let err = check_bundle_floor(&TemplateStore::new(V49).unwrap()).unwrap_err();
    assert!(
        err.to_string().contains("bundle 49") && err.to_string().contains("requires bundle 50"),
        "{err}"
    );
    check_bundle_floor(&TemplateStore::new(V50).unwrap()).unwrap();
    let main: String = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs"))
        .unwrap()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let load = main.find("TemplateStore::new(&template_dir)").unwrap();
    let install = main.find("set_ltx_template_store(").unwrap();
    assert!(
        main[load..install].contains("check_bundle_floor(&st)"),
        "main loads no bundle below the floor"
    );
}

// ---------------------------------------------------------------------------
// Weights (W1 on bundle v50): the Gemma adapter is no longer a weight any template loads.
// ---------------------------------------------------------------------------

fn v50_files_of(id: &str) -> Vec<String> {
    let store = TemplateStore::new(V50).expect("v50 store loads");
    weight_files(&store.graph(id).expect("template exists")).expect("manifest derives")
}

/// Golden for the two graph families that lost the adapter: the lists above minus that one file.
#[test]
fn v50_t2v_and_iclora_manifests_are_golden() {
    assert_eq!(
        v50_files_of("ltx-t2v-hdr"),
        vec![
            "gemma_3_12B_it_fp4_mixed.safetensors",
            "ltx-2.3-22b-dev-fp8.safetensors",
            "ltx-2.3-spatial-upscaler-x2-1.1.safetensors",
            "ltx_2.3_22b_distilled_1.1_lora_dynamic_fro09_avg_rank_111_bf16.safetensors",
        ]
    );
    assert_eq!(v50_files_of("ltx-i2v-hdr"), v50_files_of("ltx-t2v-hdr"));
    assert_eq!(
        v50_files_of("ltx-iclora-hdr"),
        vec![
            "gemma_3_12B_it_fp4_mixed.safetensors",
            "ltx-2.3-22b-distilled-fp8.safetensors",
            "ltx-2.3-22b-ic-lora-union-control-ref0.5.safetensors",
            "moge_2_vitl_normal_fp16.safetensors",
        ]
    );
    assert_eq!(
        v50_files_of("ltx-iclora-chain-hdr"),
        v50_files_of("ltx-iclora-hdr")
    );
}

/// No v50 template loads the adapter, and neither host list (the download script, the Phala weight manifest) still
/// asks a host to fetch it. Mutation: restore the line in either list, or the loader in any graph -> red.
#[test]
fn v50_drops_the_adapter_from_every_graph_and_host_list() {
    let store = TemplateStore::new(V50).expect("v50 store loads");
    for e in &store.bundle().templates {
        // the Cosmos relight templates carry no ComfyUI graph weights of ours; every other one must derive
        let files = weight_files(&store.graph(&e.template_id).unwrap()).unwrap_or_default();
        assert!(!files.iter().any(|f| f == ADAPTER), "{}", e.template_id);
    }
    for list in [
        "scripts/download_ltx_models.sh",
        "deployment/phala/run2/ltx/ltx-weights.txt",
    ] {
        let text =
            std::fs::read_to_string(format!("{}/{list}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert!(
            !text
                .lines()
                .any(|l| !l.trim_start().starts_with('#') && l.contains(ADAPTER)),
            "{list} still lists the adapter"
        );
        assert!(
            text.contains("gemma_3_12B_it_fp4_mixed.safetensors"),
            "{list} keeps the plain Gemma text encoder"
        );
    }
}
