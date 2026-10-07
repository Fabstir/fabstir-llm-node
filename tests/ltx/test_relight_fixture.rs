// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// VFX Passes (VP1.0): emit the relight graphs patched by the node's REAL patcher, for the sidecar's strict validator tests
// (fabstir-relight/tests). Ignored by default; run with
//   cargo test --test ltx_tests -- --ignored emit_relight_patched_fixture --test-threads=1
// Writes tests/ltx/relight-patched-fixture.json. The patcher is unmodified (8.59.1): it already patches every handle the graphs use.

use fabstir_llm_node::ltx::patcher::patch;
use fabstir_llm_node::ltx::types::{LtxJob, OutputKind, Resolution};
use fabstir_llm_node::ltx::Graph;
use serde_json::{json, Value};

fn relight_job(template_id: &str, frames: u32) -> LtxJob {
    LtxJob {
        template_id: template_id.to_string(),
        template_hash: "0x00".to_string(),
        prompt: String::new(),
        seed: u64::MAX.to_string(),
        frames,
        fps: 25,
        resolution: Resolution { w: 1920, h: 1088 },
        lora: format!("{template_id}@v1"),
        output: OutputKind::ExrFrames,
        images: None,
        videos: Some(vec!["cid".to_string()]),
        strength: None,
        azimuth: None,
        elevation: None,
        distance: None,
        input_wire: None,
    }
}

#[test]
#[ignore]
fn emit_relight_patched_fixture() {
    let mut out = serde_json::Map::new();
    for (id, frames) in [
        ("cosmos-passes-key", 145u32),
        ("cosmos-passes-std", 145),
        ("cosmos-passes-full", 145),
    ] {
        let raw = std::fs::read_to_string(format!("templates/{id}/v1.json")).unwrap();
        let graph = Graph(serde_json::from_str::<Value>(&raw).unwrap());
        let patched = patch(
            &graph,
            &relight_job(id, frames),
            &[],
            &["abc.mp4".to_string()],
        )
        .unwrap();
        out.insert(id.to_string(), patched.0);
    }
    let text = serde_json::to_string_pretty(&json!(out)).unwrap() + "\n";
    std::fs::write("tests/ltx/relight-patched-fixture.json", text).unwrap();
}
