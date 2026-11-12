// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Bundle v50, second part. T2.1: `frameTiers`, a frame cap that depends on the picture size (the Refine Details
//! size defect: memory use does not follow the token product). And a structural check of the four graphs that lost
//! the adapter: every link resolves, and the prompt step's other inputs are as in v49. Each test names the
//! mutation that turns it red.
use fabstir_llm_node::ltx::template::{
    check_template_rules, frame_tier_cap, FrameTier, TemplateStore,
};

const V49: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/fixtures/v49");
const V50: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/fixtures/v50");
const TIERED: [&str; 4] = [
    "ltx-refine-hdr",
    "ltx-refine-chain-hdr",
    "ltx-upscale25-hdr",
    "ltx-upscale25-chain-hdr",
];
const CHANGED: [&str; 4] = [
    "ltx-t2v-hdr",
    "ltx-i2v-hdr",
    "ltx-iclora-hdr",
    "ltx-iclora-chain-hdr",
];

#[test]
fn large_frames_are_capped_at_49_on_the_four_entries() {
    // the tier not enforced -> the two refusals pass -> red; `>=` for `>` -> 1920x1088 at 121 is refused -> red
    let store = TemplateStore::new(V50).unwrap();
    for id in TIERED {
        let e = store.entry(id).unwrap();
        assert_eq!(
            e.frame_tiers.as_deref(),
            Some(
                &[FrameTier {
                    above_pixels: 2_088_960,
                    max_frames: 49
                }][..]
            ),
            "{id}"
        );
        // the two jobs the token cap let through and the GPU could not hold
        for (w, h, frames) in [(2560, 1408, 113), (3840, 2176, 73)] {
            let err = check_template_rules(e, w, h, 24, frames).unwrap_err();
            assert!(
                err.contains(&format!("{frames} frames at {w}x{h}"))
                    && err.contains("49-frame maximum")
                    && err.contains("2088960 pixels"),
                "{id}: {err}"
            );
        }
        // the one measured size above 1920x1088, and everything at or below it as before
        check_template_rules(e, 3840, 2176, 24, 49).unwrap_or_else(|m| panic!("{id}: {m}"));
        check_template_rules(e, 1920, 1088, 24, 121).unwrap_or_else(|m| panic!("{id}: {m}"));
        check_template_rules(e, 1088, 1920, 24, 121).unwrap_or_else(|m| panic!("{id}: {m}"));
    }
}

#[test]
fn entries_without_the_key_are_unchanged() {
    // a tier applied to every template -> an untiered entry refuses -> red
    let (old, new) = (
        TemplateStore::new(V49).unwrap(),
        TemplateStore::new(V50).unwrap(),
    );
    for e in &new.bundle().templates {
        let id = e.template_id.as_str();
        assert_eq!(e.frame_tiers.is_some(), TIERED.contains(&id), "{id}");
        let before = old.entry(id).unwrap();
        assert!(before.frame_tiers.is_none(), "v49 has no tiers");
        if !TIERED.contains(&id) {
            // the same answer as v49 for a spread of jobs, the two failing sizes included
            for (w, h, frames) in [
                (2560, 1408, 113),
                (3840, 2176, 73),
                (1920, 1088, 121),
                (1280, 704, 49),
            ] {
                assert_eq!(
                    check_template_rules(e, w, h, 24, frames),
                    check_template_rules(before, w, h, 24, frames),
                    "{id} {w}x{h} x{frames}"
                );
            }
        }
    }
    // absent on the wire where it is absent in the entry: untiered entries serialise as they did
    let wire = serde_json::to_value(new.bundle()).unwrap();
    let with_key = wire["templates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t.get("frameTiers").is_some())
        .count();
    assert_eq!(with_key, 4);
}

#[test]
fn the_smallest_applicable_tier_wins() {
    // taking the first or the largest tier -> red
    let tiers = [
        FrameTier {
            above_pixels: 1_000_000,
            max_frames: 97,
        },
        FrameTier {
            above_pixels: 4_000_000,
            max_frames: 25,
        },
        FrameTier {
            above_pixels: 2_000_000,
            max_frames: 49,
        },
    ];
    assert_eq!(
        frame_tier_cap(&tiers, 1000, 1000),
        None,
        "not above the lowest"
    );
    assert_eq!(frame_tier_cap(&tiers, 1001, 1000), Some((97, 1_000_000)));
    assert_eq!(frame_tier_cap(&tiers, 2000, 1500), Some((49, 2_000_000)));
    assert_eq!(frame_tier_cap(&tiers, 3840, 2176), Some((25, 4_000_000)));
    assert_eq!(frame_tier_cap(&[], 3840, 2176), None);
    // an unknown key inside a tier is a typo, not a rule to ignore
    assert!(serde_json::from_str::<FrameTier>(r#"{"abovePixels": 1, "maxFrames": 2}"#).is_ok());
    assert!(serde_json::from_str::<FrameTier>(r#"{"abovePixels": 1, "maxFrame": 2}"#).is_err());
}

fn links(node: &serde_json::Value) -> Vec<(String, String, u64)> {
    node["inputs"]
        .as_object()
        .map(|inputs| {
            inputs
                .iter()
                .filter_map(|(name, v)| {
                    let a = v.as_array()?;
                    match (a.len(), a.first()?.as_str(), a.get(1)?.as_u64()) {
                        (2, Some(src), Some(slot)) => Some((name.clone(), src.to_string(), slot)),
                        _ => None,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The one link that has never resolved, in v49 as in v50: the chained graph keeps the prompt step switched off
/// (its `PrimitiveBoolean` is false) and has no still to show it, so the step's `image` input names a loader the
/// graph does not have. It is pinned here so that it cannot be joined by another.
const KNOWN_DANGLING: [(&str, &str, &str, &str); 1] =
    [("ltx-iclora-chain-hdr", "129:209", "image", "200")];

fn dangling(store: &TemplateStore, id: &str) -> (Vec<(String, String, String)>, usize) {
    let g = store.graph(id).unwrap();
    let nodes = g.0.as_object().unwrap();
    let (mut out, mut walked) = (vec![], 0);
    for (nid, node) in nodes {
        for (input, src, _slot) in links(node) {
            walked += 1;
            if !nodes.contains_key(&src) {
                out.push((nid.clone(), input, src));
            }
        }
    }
    out.sort();
    (out, walked)
}

#[test]
fn every_link_in_the_four_graphs_resolves() {
    // a node removed while something still reads from it -> a dangling link -> red
    let (old, new) = (
        TemplateStore::new(V49).unwrap(),
        TemplateStore::new(V50).unwrap(),
    );
    for id in CHANGED {
        let (found, walked) = dangling(&new, id);
        assert!(
            walked > 40,
            "{id}: the graph's links were walked ({walked})"
        );
        let known: Vec<(String, String, String)> = KNOWN_DANGLING
            .iter()
            .filter(|k| k.0 == id)
            .map(|k| (k.1.to_string(), k.2.to_string(), k.3.to_string()))
            .collect();
        assert_eq!(
            found, known,
            "{id}: a link reads from a node that is not in the graph"
        );
        // and the pinned one is inherited, not made by the adapter's removal
        assert_eq!(dangling(&old, id).0, known, "{id}: as in v49");
    }
    // the pinned link belongs to a prompt step that is switched off
    let g = new.graph("ltx-iclora-chain-hdr").unwrap();
    let switch = g
        .0
        .as_object()
        .unwrap()
        .values()
        .find(|n| n["class_type"] == "ComfySwitchNode" && n["inputs"]["on_true"][0] == "129:209")
        .expect("the prompt step feeds a switch");
    let flag = switch["inputs"]["switch"][0].as_str().unwrap();
    assert_eq!(g.0[flag]["inputs"]["value"], false, "the step is off");
}

#[test]
fn the_prompt_step_differs_from_v49_only_in_where_its_encoder_comes_from() {
    // any other input of the prompt step edited with the adapter's removal -> red
    let (old, new) = (
        TemplateStore::new(V49).unwrap(),
        TemplateStore::new(V50).unwrap(),
    );
    for id in CHANGED {
        let step = |store: &TemplateStore| {
            let g = store.graph(id).unwrap();
            let (nid, node) =
                g.0.as_object()
                    .unwrap()
                    .iter()
                    .find(|(_, n)| n["class_type"] == "TextGenerateLTX2Prompt")
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .unwrap();
            (nid, node, g)
        };
        let ((old_id, mut before, old_graph), (new_id, mut after, _)) = (step(&old), step(&new));
        assert_eq!(old_id, new_id, "{id}: the same node");
        // in v49 the encoder came through the adapter's second output; in v50 it comes from where the adapter
        // itself took it
        let adapter = before["inputs"]["clip"][0].as_str().unwrap().to_string();
        assert_eq!(old_graph.0[&adapter]["class_type"], "LoraLoader", "{id}");
        assert_eq!(
            after["inputs"]["clip"], old_graph.0[&adapter]["inputs"]["clip"],
            "{id}: the encoder the adapter was fed"
        );
        before["inputs"].as_object_mut().unwrap().remove("clip");
        after["inputs"].as_object_mut().unwrap().remove("clip");
        assert_eq!(
            before, after,
            "{id}: nothing else about the prompt step moved"
        );
    }
}
