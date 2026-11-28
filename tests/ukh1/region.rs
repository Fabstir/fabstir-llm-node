// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! U3 node half: FABSTIR_REGION start check (plan D17, SR1 D3b).

use fabstir_llm_node::region::{check_region_start, Region};
use std::collections::HashMap;

fn check(pairs: &[(&str, &str)]) -> Result<Region, String> {
    let m: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    check_region_start(|k| m.get(k).cloned())
}

/// A UK host as docker-compose.uk-3xsz.yml runs it: search off, no video.
const UK_OK: &[(&str, &str)] = &[("FABSTIR_REGION", "uk"), ("WEB_SEARCH_ENABLED", "false")];

#[test]
fn unset_or_global_is_today_whatever_else_is_set() {
    // today's prod compose: search on by default, LTX on, no alert sink
    let today = [("COMFY_URL", "http://comfy:8188")];
    assert_eq!(check(&today), Ok(Region::Global));
    assert_eq!(
        check(&[("FABSTIR_REGION", ""), today[0]]),
        Ok(Region::Global)
    );
    assert_eq!(
        check(&[
            ("FABSTIR_REGION", "GLOBAL"),
            ("WEB_SEARCH_ENABLED", "true"),
            today[0]
        ]),
        Ok(Region::Global)
    );
}

#[test]
fn uk_accepted_when_clean() {
    assert_eq!(check(UK_OK), Ok(Region::Uk));
    assert_eq!(
        check(&[("FABSTIR_REGION", " UK "), UK_OK[1]]),
        Ok(Region::Uk)
    );
}

#[test]
fn unknown_region_refused() {
    for v in ["eu", "us", "u k", "global2"] {
        let e = check(&[("FABSTIR_REGION", v), UK_OK[1]]).unwrap_err();
        assert!(e.contains("not a configured region"), "{v}: {e}");
    }
}

#[test]
fn uk_refuses_web_search() {
    for v in [Some("true"), Some("1"), Some("yes"), None] {
        let mut env = vec![("FABSTIR_REGION", "uk")];
        if let Some(v) = v {
            env.push(("WEB_SEARCH_ENABLED", v));
        }
        let e = check(&env).unwrap_err();
        assert!(e.contains("web search"), "{v:?}: {e}");
    }
    assert_eq!(
        check(&[("FABSTIR_REGION", "uk"), ("WEB_SEARCH_ENABLED", "FALSE")]),
        Ok(Region::Uk)
    );
}

#[test]
fn uk_refuses_video_without_explicit_alert_sink() {
    for video in [
        ("COMFY_URL", "http://comfy:8188"),
        ("RELIGHT_URL", "http://relight:9000"),
    ] {
        let mut env = UK_OK.to_vec();
        env.push(video);
        let e = check(&env).unwrap_err();
        assert!(e.contains("LTX_CHECKS_ALERT_URL"), "{video:?}: {e}");
        env.push(("LTX_CHECKS_ALERT_URL", "  "));
        assert!(check(&env).is_err(), "an empty sink is not explicit");
        env.pop();
        env.push(("LTX_CHECKS_ALERT_URL", "https://ntfy.example/uk-alerts"));
        assert_eq!(check(&env), Ok(Region::Uk), "{video:?} with a sink");
    }
}

#[test]
fn uk_video_off_needs_no_sink() {
    let mut env = UK_OK.to_vec();
    env.push(("COMFY_URL", "")); // the documented way to switch video off
    assert_eq!(check(&env), Ok(Region::Uk));
}

#[test]
fn main_runs_the_check_before_the_model_load() {
    let src = include_str!("../../src/main.rs");
    let check = src
        .find("region::check_region_start(")
        .expect("main calls the check");
    let load = src.find("llm_engine.load_model(").expect("model load");
    assert!(check < load, "region check must run before the model load");
}
