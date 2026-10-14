// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Batch D21: relight deadlines scale with frames (`deadline_for` /
//! `watch_timeout_for` replace the flat `AdmitCfg` fields), the six compiled
//! defaults may only be LOWERED by env, the proof rate may not be lowered, and
//! the keep-alive interval is capped at 900 s. Env is injected (`from_vars`),
//! never set on the process.

use std::collections::HashMap;

use fabstir_llm_node::ltx::relight::AdmitCfg;
use fabstir_llm_node::ltx::submit::ltx_tokens;

const PRESETS: [&str; 3] = [
    "cosmos-passes-key",
    "cosmos-passes-std",
    "cosmos-passes-full",
];

const SIX: [&str; 6] = [
    "RELIGHT_TIMEOUT_145_SECS",
    "RELIGHT_SECS_PER_EXTRA_FRAME_KEY",
    "RELIGHT_SECS_PER_EXTRA_FRAME_STD",
    "RELIGHT_SECS_PER_EXTRA_FRAME_FULL",
    "RELIGHT_DEADLINE_MARGIN_SECS",
    "RELIGHT_UPLOAD_SECS_PER_EXTRA_FRAME",
];

fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |k: &str| map.get(k).cloned()
}

fn compiled() -> AdmitCfg {
    AdmitCfg::from_vars(&vars(&[])).expect("no env boots").0
}

#[test]
fn at_or_below_145_frames_the_timeout_is_2600_and_the_deadline_3200() {
    // the margin is 600 at ≤ 145 (a 361-sized margin at every length → Full 145
    // refused on a 3600 s session, see test_relight_admission)
    let cfg = compiled();
    for preset in PRESETS {
        for frames in [121, 129, 137, 145] {
            assert_eq!(
                cfg.watch_timeout_for(preset, frames),
                2600,
                "{preset} {frames}"
            );
            assert_eq!(cfg.deadline_for(preset, frames), 3200, "{preset} {frames}");
        }
    }
}

#[test]
fn full_361_adds_216_extra_frames_of_compute_and_upload() {
    let cfg = compiled();
    let slope = cfg.secs_per_extra_frame_full;
    let upload = cfg.upload_secs_per_extra_frame;
    assert!(slope > 0 && upload > 0, "placeholders are non-zero");
    assert_eq!(
        cfg.watch_timeout_for("cosmos-passes-full", 361),
        2600 + 216 * slope
    );
    assert_eq!(
        cfg.deadline_for("cosmos-passes-full", 361),
        2600 + 216 * slope + 600 + 216 * upload
    );
    assert!(cfg.deadline_for("cosmos-passes-full", 361) > 3200);
}

#[test]
fn every_preset_and_length_equals_the_shared_vector() {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("relight-deadline-vector.json")).unwrap();
    let cfg = compiled();
    let d = &v["defaults"];
    assert_eq!(cfg.timeout_145_secs, d["timeout145"].as_u64().unwrap());
    assert_eq!(
        cfg.secs_per_extra_frame_key,
        d["slopeKey"].as_u64().unwrap()
    );
    assert_eq!(
        cfg.secs_per_extra_frame_std,
        d["slopeStd"].as_u64().unwrap()
    );
    assert_eq!(
        cfg.secs_per_extra_frame_full,
        d["slopeFull"].as_u64().unwrap()
    );
    assert_eq!(cfg.deadline_margin_secs, d["margin"].as_u64().unwrap());
    assert_eq!(
        cfg.upload_secs_per_extra_frame,
        d["uploadSlope"].as_u64().unwrap()
    );
    assert_eq!(cfg.rate_tokens_per_sec, d["rate"].as_u64().unwrap());
    assert_eq!(cfg.keepalive_secs, d["keepalive"].as_u64().unwrap());
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        3 * 31,
        "three presets × every 8k+1 from 121 to 361"
    );
    for r in rows {
        let (id, frames) = (
            r["templateId"].as_str().unwrap(),
            r["frames"].as_u64().unwrap() as u32,
        );
        let tokens = ltx_tokens(frames, 1920, 1088);
        assert_eq!(tokens, r["tokens"].as_u64().unwrap());
        assert_eq!(
            cfg.watch_timeout_for(id, frames),
            r["watchTimeoutSecs"].as_u64().unwrap()
        );
        assert_eq!(
            cfg.deadline_for(id, frames),
            r["deadlineSecs"].as_u64().unwrap()
        );
        assert_eq!(
            cfg.final_budget_secs(id, frames, tokens),
            r["finalBudgetSecs"].as_u64().unwrap(),
            "{id} {frames}"
        );
    }
}

#[test]
fn an_env_value_above_the_compiled_default_refuses_boot_for_each_of_the_six() {
    let base = compiled();
    let defaults = [
        base.timeout_145_secs,
        base.secs_per_extra_frame_key,
        base.secs_per_extra_frame_std,
        base.secs_per_extra_frame_full,
        base.deadline_margin_secs,
        base.upload_secs_per_extra_frame,
    ];
    for (name, default) in SIX.iter().zip(defaults) {
        let above = (default + 1).to_string();
        let err = AdmitCfg::from_vars(&vars(&[(name, &above)]))
            .err()
            .unwrap_or_else(|| panic!("{name}={above} must refuse boot"));
        assert!(err.contains(name), "{err}");
        // equal is allowed
        let same = default.to_string();
        assert!(
            AdmitCfg::from_vars(&vars(&[(name, &same)])).is_ok(),
            "{name}"
        );
        // unparsable refuses too (never a silent default)
        assert!(
            AdmitCfg::from_vars(&vars(&[(name, "lots")])).is_err(),
            "{name}"
        );
    }
}

#[test]
fn env_may_lower_each_of_the_six() {
    let cfg = AdmitCfg::from_vars(&vars(&[
        ("RELIGHT_TIMEOUT_145_SECS", "2"),
        ("RELIGHT_SECS_PER_EXTRA_FRAME_KEY", "0"),
        ("RELIGHT_SECS_PER_EXTRA_FRAME_STD", "1"),
        ("RELIGHT_SECS_PER_EXTRA_FRAME_FULL", "3"),
        ("RELIGHT_DEADLINE_MARGIN_SECS", "0"),
        ("RELIGHT_UPLOAD_SECS_PER_EXTRA_FRAME", "1"),
    ]))
    .unwrap()
    .0;
    assert_eq!(cfg.watch_timeout_for("cosmos-passes-key", 361), 2);
    assert_eq!(cfg.watch_timeout_for("cosmos-passes-std", 361), 2 + 216);
    assert_eq!(
        cfg.watch_timeout_for("cosmos-passes-full", 361),
        2 + 3 * 216
    );
    assert_eq!(
        cfg.deadline_for("cosmos-passes-full", 361),
        2 + 3 * 216 + 216
    );
    assert_eq!(cfg.deadline_for("cosmos-passes-key", 145), 2);
}

#[test]
fn a_rate_env_other_than_the_compiled_default_refuses_boot() {
    // below: the final's wait outruns the helper's; above: every final send is
    // "Too many" and forfeits (review round 1) — only the default boots
    let rate = compiled().rate_tokens_per_sec;
    assert_eq!(rate, 10_000, "probe 15's registered rate");
    for wrong in [rate - 1, rate + 1, 20_000] {
        let v = wrong.to_string();
        let err = AdmitCfg::from_vars(&vars(&[("RELIGHT_RATE_LIMIT_TOKENS_PER_SEC", &v)]))
            .err()
            .unwrap_or_else(|| panic!("rate {v} must refuse boot"));
        assert!(err.contains("RELIGHT_RATE_LIMIT_TOKENS_PER_SEC"), "{err}");
    }
    let same = rate.to_string();
    let cfg = AdmitCfg::from_vars(&vars(&[("RELIGHT_RATE_LIMIT_TOKENS_PER_SEC", &same)]))
        .unwrap()
        .0;
    assert_eq!(cfg.rate_tokens_per_sec, rate);
}

#[test]
fn the_old_flat_variables_only_warn() {
    let (cfg, warnings) = AdmitCfg::from_vars(&vars(&[
        ("RELIGHT_JOB_DEADLINE_SECS", "2"),
        ("RELIGHT_JOB_TIMEOUT_SECS", "1"),
    ]))
    .expect("the old flat variables never refuse boot");
    assert_eq!(cfg.deadline_for("cosmos-passes-full", 145), 3200, "ignored");
    assert_eq!(
        cfg.watch_timeout_for("cosmos-passes-full", 145),
        2600,
        "ignored"
    );
    assert!(warnings
        .iter()
        .any(|w| w.contains("RELIGHT_JOB_DEADLINE_SECS")));
    assert!(warnings
        .iter()
        .any(|w| w.contains("RELIGHT_JOB_TIMEOUT_SECS")));
    assert!(AdmitCfg::from_vars(&vars(&[])).unwrap().1.is_empty());
}

#[test]
fn the_keepalive_interval_is_capped_at_900_and_never_zero() {
    let (cfg, warnings) =
        AdmitCfg::from_vars(&vars(&[("RELIGHT_KEEPALIVE_SECS", "3600")])).unwrap();
    assert_eq!(cfg.keepalive_secs, 900);
    assert!(warnings
        .iter()
        .any(|w| w.contains("RELIGHT_KEEPALIVE_SECS")));
    let cfg = AdmitCfg::from_vars(&vars(&[("RELIGHT_KEEPALIVE_SECS", "600")]))
        .unwrap()
        .0;
    assert_eq!(cfg.keepalive_secs, 600);
    assert!(AdmitCfg::from_vars(&vars(&[("RELIGHT_KEEPALIVE_SECS", "0")])).is_err());
    // review round 1: the boot floor is 300 s (a faster keep-alive pays gas for
    // nothing and burns the job in claims)
    assert!(AdmitCfg::from_vars(&vars(&[("RELIGHT_KEEPALIVE_SECS", "299")])).is_err());
    let cfg = AdmitCfg::from_vars(&vars(&[("RELIGHT_KEEPALIVE_SECS", "300")]))
        .unwrap()
        .0;
    assert_eq!(cfg.keepalive_secs, 300);
}

#[test]
fn an_unknown_relight_template_takes_the_largest_slope() {
    let cfg = AdmitCfg::from_vars(&vars(&[
        ("RELIGHT_SECS_PER_EXTRA_FRAME_KEY", "1"),
        ("RELIGHT_SECS_PER_EXTRA_FRAME_STD", "2"),
        ("RELIGHT_SECS_PER_EXTRA_FRAME_FULL", "3"),
    ]))
    .unwrap()
    .0;
    assert_eq!(
        cfg.watch_timeout_for("cosmos-passes-new", 153),
        cfg.timeout_145_secs + 8 * 3
    );
}
