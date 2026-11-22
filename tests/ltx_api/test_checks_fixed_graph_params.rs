// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks, node 8.65.3, second half: the Cut-out exemption needs every picture-shaping parameter at the
//! template's own default, and the pinned alpha graphs must expose nothing the rule does not constrain.
//! Each test names the mutation that turns it red.

use super::checks_svc::*;
use super::ltx_task_support::S5_ENV_LOCK;
use super::relight_stub::mp4_clip;
use super::test_checks_fixed_graph::{assert_checked, assert_skipped, go_with, CHAIN, HDR};
use fabstir_llm_node::ltx::checks::config::Mode;
use fabstir_llm_node::ltx::checks::PINNED_GUIDE_STRENGTH;
use fabstir_llm_node::ltx::types::LtxJob;
use serde_json::{json, Value};

fn lj(template: &str, extra: Value, videos: usize) -> LtxJob {
    let cids: Vec<String> = (0..videos).map(|i| format!("cid{i}")).collect();
    let mut wire = json!({"templateId": template, "templateHash": "0x00", "prompt": "", "seed": "1",
        "frames": 121, "fps": 24, "resolution": {"w": 1920, "h": 1088}, "lora": "l", "output": "exr-sequence",
        "videos": cids});
    for (k, v) in extra.as_object().into_iter().flatten() {
        wire[k] = v.clone();
    }
    serde_json::from_value(wire).unwrap()
}

#[tokio::test]
async fn the_pinned_default_strength_is_exempt_and_any_other_is_checked() {
    // `shapes_the_picture` ignoring `strength` -> the 0.05 job completes unjudged -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _c) = Logs::capture();
    let src = [mp4_clip(145)];
    let r = go_with(
        Mode::Enforce,
        HDR,
        "",
        &src,
        &[0],
        None,
        721,
        &json!({"strength": 1.0}),
    )
    .await;
    assert_skipped(&r, &logs, HDR, 1);
    let (logs, _c) = Logs::capture();
    let r = go_with(
        Mode::Enforce,
        HDR,
        "",
        &src,
        &[0],
        None,
        722,
        &json!({"strength": 0.05}),
    )
    .await;
    assert_checked(&r, &logs);
    let (logs, _c) = Logs::capture();
    let ins = [mp4_clip(145), mp4_clip(146)];
    let r = go_with(
        Mode::Enforce,
        CHAIN,
        "",
        &ins,
        &[0, 1],
        None,
        723,
        &json!({"strength": 0.999}),
    )
    .await;
    assert_checked(&r, &logs);
}

#[tokio::test]
async fn every_input_and_every_parameter_must_be_in_order_for_the_rule() {
    // Counting an unchecked clip, dropping the count equality, or any parameter test -> Some -> red
    let rig = rig(Mode::Enforce).await;
    let mut j = rig.checks.job("s", Some(9), None);
    let none = |j: &_, job: &LtxJob| rig.checks.result_of_fixed_graph_template(j, job).is_none();
    let one = lj(HDR, json!({}), 1);
    let two = lj(CHAIN, json!({}), 2);
    assert!(none(&j, &one), "no input checked yet");
    assert!(none(&j, &lj(HDR, json!({}), 0)), "no input at all");
    rig.checks.check_clip(&mut j, &mp4_clip(8)).await.unwrap();
    assert!(!none(&j, &one));
    assert!(none(&j, &two), "the previous part was never checked");
    rig.checks.check_clip(&mut j, &mp4_clip(9)).await.unwrap();
    assert!(!none(&j, &two));
    assert!(
        none(&j, &lj(CHAIN, json!({}), 3)),
        "a third input was not checked"
    );
    assert!(none(&j, &one), "two were checked for a one-input job");
    for prompt in ["x", " ", "\n", "\u{200b}"] {
        assert!(
            none(&j, &lj(CHAIN, json!({"prompt": prompt}), 2)),
            "{prompt:?}"
        );
    }
    for template in [
        "ltx-edit-hdr",
        "ltx-t2v-hdr",
        "ltx-layout-hdr",
        "cosmos-passes-std",
        "ltx-alpha-hdr2",
        "",
    ] {
        assert!(none(&j, &lj(template, json!({}), 2)), "{template}");
    }
    // the picture-shaping parameters: at the default they pass, anywhere else they do not
    assert!(!none(
        &j,
        &lj(CHAIN, json!({"strength": PINNED_GUIDE_STRENGTH}), 2)
    ));
    for extra in [
        json!({"strength": 0.05}),
        json!({"strength": 0.9999}),
        json!({"azimuth": 20.0}),
        json!({"elevation": 0.0}),
        json!({"distance": 1.0}),
        json!({"speed": 1.0}),
        json!({"inputWire": "exrseq-linear"}),
    ] {
        assert!(none(&j, &lj(CHAIN, extra.clone(), 2)), "{extra}");
    }
    // a refused input is not a checked input
    let mut k = rig.checks.job("s", Some(10), None);
    rig.svc.set_media("mp4", 1, ILLEGAL_REPLY);
    rig.checks
        .check_clip(&mut k, &mp4_clip(8))
        .await
        .unwrap_err();
    assert!(none(&k, &one));
}

const ILLEGAL_REPLY: Reply = Reply::Verdict("BLOCK_ILLEGAL", Some("csam"));

/// What the patcher writes of the client's into a graph, by title and by class (src/ltx/patcher.rs `patch_with`).
#[test]
fn the_pinned_alpha_graphs_expose_only_what_the_rule_constrains() {
    // A second Prompt node, a Speed/Frames/reference handle, a CrossViewWarp, a sampler the seed pass also patches,
    // a loader count that differs from the entry's, or a guide strength other than the constant -> red
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/fixtures/v50");
    let allow: Value =
        serde_json::from_slice(&std::fs::read(format!("{dir}/allowlist.json")).unwrap()).unwrap();
    let mut seen = 0;
    for t in allow["templates"].as_array().unwrap() {
        let id = t["templateId"].as_str().unwrap();
        if !["ltx-alpha-hdr", "ltx-alpha-chain-hdr"].contains(&id) {
            continue;
        }
        seen += 1;
        let path = format!("{dir}/{id}/{}.json", t["version"].as_str().unwrap());
        let g: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let nodes: Vec<(&str, &str)> = g
            .as_object()
            .unwrap()
            .values()
            .map(|n| {
                (
                    n["class_type"].as_str().unwrap(),
                    n.pointer("/_meta/title")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                )
            })
            .collect();
        let count = |f: &dyn Fn(&(&str, &str)) -> bool| nodes.iter().filter(|n| f(n)).count();
        assert_eq!(count(&|n| n.1 == "Prompt"), 1, "{id}: one text input");
        assert_eq!(count(&|n| n.0 == "RandomNoise"), 1, "{id}");
        for class in [
            "KSampler",
            "LTXVTiledFusionSampler",
            "CrossViewWarp",
            "LoadImage",
            "LoadVideo",
        ] {
            assert_eq!(count(&|n| n.0 == class), 0, "{id}: {class}");
        }
        for title in ["Speed", "Frames", "Reference guide (frame -1)"] {
            assert_eq!(count(&|n| n.1 == title), 0, "{id}: {title}");
        }
        assert_eq!(
            count(&|n| n.0 == "VHS_LoadVideo") as u64,
            t["videoInputs"].as_u64().unwrap()
        );
        assert_eq!(count(&|n| n.0 == "LTXAddVideoICLoRAGuide"), 1, "{id}");
        let guide = g
            .as_object()
            .unwrap()
            .values()
            .find(|n| n["class_type"] == "LTXAddVideoICLoRAGuide")
            .unwrap();
        assert_eq!(
            guide["inputs"]["strength"].as_f64(),
            Some(PINNED_GUIDE_STRENGTH),
            "{id}"
        );
        assert!(
            t.get("fixedPrompt").is_none() && t.get("sidecar").is_none(),
            "{id}"
        );
    }
    assert_eq!(seen, 2);
}
