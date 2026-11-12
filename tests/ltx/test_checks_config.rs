// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks N1 (IMPLEMENTATION-CONTENT-CHECKS-LTX.md D12): the mode switch has no silent default, each mode
//! requires what it needs, the tokens exist only through the check step, and the wire mapping of section 10b / A8.
//! Each test names the mutation that turns it red.
use fabstir_llm_node::ltx::checks::client::{parse_outcome, CallError, Verdict, INPUT_KINDS};
use fabstir_llm_node::ltx::checks::config::{ChecksConfig, Mode};
use fabstir_llm_node::ltx::checks::ContentChecks;
use serde_json::json;
use std::collections::HashMap;

const KEY: &str = "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";
const HASH: &str = "sha256:AB12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12";

fn enforce_env() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("LTX_CONTENT_CHECKS", "enforce"),
        ("LTX_CHECKS_SOCKET", "/run/mod/mod.sock"),
        ("LTX_CHECKS_STAGING_DIR", "/staging"),
        ("LTX_CHECKS_STAGING_DIR_SERVICE", "/mnt/staging"),
        ("COMFY_INPUT_DIR", "/comfy/input"),
        ("COMFY_OUTPUT_DIR", "/comfy/output"),
        ("LTX_EVIDENCE_PUBKEY", KEY),
        ("LTX_EVIDENCE_DIR", "/evidence"),
        ("LTX_CHECKS_BUNDLE_HASH", HASH),
    ])
}

fn load(env: &HashMap<&'static str, &'static str>, relight: bool) -> Result<ChecksConfig, String> {
    ChecksConfig::from_lookup(true, relight, |k| env.get(k).map(|v| v.to_string()))
}

#[test]
fn the_mode_is_required_whenever_video_is_configured() {
    // default to `off` when unset -> the first assertion goes red
    let none = HashMap::new();
    let e = load(&none, false).unwrap_err();
    assert!(e.contains("LTX_CONTENT_CHECKS must be set"), "{e}");
    let e = load(&HashMap::from([("LTX_CONTENT_CHECKS", "on")]), false).unwrap_err();
    assert!(e.contains("not one of off, shadow, enforce"), "{e}");
    let e = load(&HashMap::from([("LTX_CONTENT_CHECKS", "  ")]), false).unwrap_err();
    assert!(e.contains("must be set"), "blank is missing: {e}");
    // a node without video generation is not asked
    let cfg = ChecksConfig::from_lookup(false, false, |_| None).unwrap();
    assert_eq!(cfg.mode, Mode::Off);
    assert_eq!(
        load(&HashMap::from([("LTX_CONTENT_CHECKS", "off")]), false)
            .unwrap()
            .mode,
        Mode::Off
    );
}

#[test]
fn enforce_requires_every_setting_it_names() {
    // drop any `need(..)` line in from_lookup -> that key's case goes red
    let full = enforce_env();
    let cfg = load(&full, false).unwrap();
    assert_eq!(cfg.mode, Mode::Enforce);
    assert_eq!(
        cfg.bundle_hash.as_deref(),
        Some(&HASH[7..].to_ascii_lowercase()[..])
    );
    assert!(cfg.evidence_pubkey.is_some());
    assert_eq!(
        cfg.refusals_file.as_deref(),
        Some(std::path::Path::new("/evidence/refusals.jsonl"))
    );
    for key in full.keys().filter(|k| **k != "LTX_CONTENT_CHECKS") {
        let mut env = full.clone();
        env.remove(key);
        let e = load(&env, false).unwrap_err();
        assert!(
            e.contains(key) || e.contains("must be set together"),
            "{key}: {e}"
        );
    }
    // the second renderer's folders, when it is configured (D9)
    let e = load(&full, true).unwrap_err();
    assert!(e.contains("RELIGHT_INPUT_DIR"), "{e}");
    let mut both = full.clone();
    both.insert("RELIGHT_INPUT_DIR", "/relight/in");
    both.insert("RELIGHT_OUTPUT_DIR", "/relight/out");
    assert!(load(&both, true).unwrap().relight_dirs.is_some());
}

#[test]
fn an_output_folder_setting_on_its_own_changes_nothing() {
    // the production compose file has always set COMFY_OUTPUT_DIR: treating it alone as an error (or as consent
    // to delete files) would stop or surprise every host at upgrade -> red
    let env = HashMap::from([
        ("LTX_CONTENT_CHECKS", "off"),
        ("COMFY_OUTPUT_DIR", "/app/comfy-output"),
    ]);
    let cfg = load(&env, false).unwrap();
    assert!(
        cfg.comfy_dirs.is_none(),
        "no job-end deletion without both folders"
    );
    let both = HashMap::from([
        ("LTX_CONTENT_CHECKS", "off"),
        ("COMFY_OUTPUT_DIR", "/app/comfy-output"),
        ("COMFY_INPUT_DIR", "/app/comfy-input"),
    ]);
    assert!(load(&both, false).unwrap().comfy_dirs.is_some());
    // and a node without video generation ignores the folders altogether
    let none =
        ChecksConfig::from_lookup(false, false, |k| env.get(k).map(|v| v.to_string())).unwrap();
    assert!(none.mode == Mode::Off && none.comfy_dirs.is_none());
}

#[test]
fn bad_values_stop_the_node() {
    for (key, bad, needle) in [
        ("LTX_EVIDENCE_PUBKEY", "02deadbeef", "LTX_EVIDENCE_PUBKEY"),
        ("LTX_CHECKS_BUNDLE_HASH", "sha256:1234", "64 hex"),
        ("LTX_CHECKS_STAGING_DIR", "relative/dir", "absolute"),
        ("COMFY_OUTPUT_DIR", "out", "absolute"),
    ] {
        let mut env = enforce_env();
        env.insert(key, bad);
        let e = load(&env, false).unwrap_err();
        assert!(e.contains(needle), "{key}: {e}");
    }
    let mut env = enforce_env();
    env.insert("LTX_CHECKS_ALERT_URL", "ntfy.sh/topic");
    assert!(load(&env, false).unwrap_err().contains("http(s) URL"));
}

#[test]
fn shadow_needs_an_end_date_and_expires_after_it() {
    // no end date accepted, or `>=` instead of `>` -> red
    let mut env = HashMap::from([
        ("LTX_CONTENT_CHECKS", "shadow"),
        ("LTX_CHECKS_SOCKET", "/run/mod/mod.sock"),
        ("LTX_CHECKS_STAGING_DIR", "/staging"),
        ("LTX_CHECKS_STAGING_DIR_SERVICE", "/mnt/staging"),
    ]);
    let e = load(&env, false).unwrap_err();
    assert!(e.contains("LTX_CONTENT_CHECKS_SHADOW_UNTIL"), "{e}");
    env.insert("LTX_CONTENT_CHECKS_SHADOW_UNTIL", "next week");
    assert!(load(&env, false).unwrap_err().contains("YYYY-MM-DD"));
    env.insert("LTX_CONTENT_CHECKS_SHADOW_UNTIL", "2026-11-20");
    let cfg = load(&env, false).unwrap();
    let day = |d| chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
    assert!(!cfg.shadow_expired(day(19)));
    assert!(
        !cfg.shadow_expired(day(20)),
        "the end date itself still runs"
    );
    assert!(cfg.shadow_expired(day(21)));
    env.remove("LTX_CHECKS_SOCKET");
    assert!(load(&env, false).unwrap_err().contains("LTX_CHECKS_SOCKET"));
}

#[test]
fn enforce_refuses_to_start_without_write_access() {
    // skip the folder probe in verify_access -> red
    let tmp = tempfile::tempdir().unwrap();
    let p = |n: &str| tmp.path().join(n);
    for d in ["staging", "in", "out"] {
        std::fs::create_dir(p(d)).unwrap();
    }
    let mut cfg = load(&enforce_env(), false).unwrap();
    cfg.staging_node = Some(p("staging"));
    cfg.evidence_dir = Some(p("evidence"));
    cfg.comfy_dirs = Some(fabstir_llm_node::ltx::checks::config::RendererDirs {
        input: p("in"),
        output: p("missing-output"),
    });
    let e = cfg.verify_access().unwrap_err();
    assert!(e.contains("COMFY_OUTPUT_DIR"), "{e}");
    cfg.comfy_dirs.as_mut().unwrap().output = p("out");
    cfg.verify_access().unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(p("evidence"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700, "the evidence directory is owner-only");
}

#[tokio::test]
async fn off_mints_tokens_and_reports_itself() {
    let off = ContentChecks::off();
    let mut job = off.job("s", Some(1), None);
    off.check_prompt(&mut job, "anything", false).await.unwrap();
    off.check_still(&mut job, b"not even an image")
        .await
        .unwrap();
    off.check_clip(&mut job, b"").await.unwrap();
    off.check_deep_frame(&mut job, 0, 10, 24, true, b"")
        .await
        .unwrap();
    off.check_result(&mut job, "frame.exr", b"").await.unwrap();
    assert_eq!(job.spent, std::time::Duration::ZERO, "no check ran");
    assert!(off.pre_accept(None, "s").is_ok());
    assert_eq!(
        off.status_json(),
        json!({"mode": "off", "bundleHash": null, "breaker": "closed"})
    );
}

#[test]
fn the_tokens_are_minted_only_inside_the_checks_module() {
    // a `pub fn` minting a token elsewhere, or a public field -> red
    let read = |p: &str| {
        std::fs::read_to_string(format!("{}/src/{p}", env!("CARGO_MANIFEST_DIR")))
            .unwrap()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    };
    let checks = read("ltx/checks/mod.rs");
    assert!(checks.contains("pubstructCheckedInput{_minted:(),}"));
    assert!(checks.contains("pubstructResultChecked{_minted:(),}"));
    for other in [
        "ltx/client.rs",
        "ltx/provenance/finalise.rs",
        "api/websocket/handlers/ltx.rs",
        "api/server.rs",
    ] {
        assert!(!read(other).contains("_minted"), "{other} builds a token");
    }
    // and the two sinks demand them
    assert!(read("ltx/client.rs")
        .contains("bytes:Vec<u8>,_checked:crate::ltx::checks::CheckedInput,)->Result<String>"));
    assert!(read("ltx/provenance/finalise.rs")
        .contains("bytes:Vec<u8>,_checked:&crate::ltx::checks::ResultChecked,)->Result<Vec<u8>>"));
}

fn media(verdict: &str, category: serde_json::Value, frames: u64) -> serde_json::Value {
    json!({"verdict": verdict, "category": category, "rating": null,
           "stats": {"framesSampled": frames, "maxScore": 0.4, "escalated": 3},
           "bundleHash": HASH.to_ascii_lowercase()})
}

#[test]
fn only_pass_passes_and_every_block_refuses() {
    // map PASS_RATED (or an unknown word) to Pass -> red; seal on BLOCK_POLICY -> red
    let ok = parse_outcome(200, &media("PASS", json!(null), 120), false).unwrap();
    assert_eq!((ok.verdict, ok.frames), (Verdict::Pass, 120));
    assert_eq!(
        ok.bundle_hash.as_deref(),
        Some(&HASH[7..].to_ascii_lowercase()[..])
    );
    for (word, cat, want) in [
        ("BLOCK_POLICY", json!("sexually_explicit"), Verdict::Block),
        ("BLOCK_UNRESOLVED", json!(null), Verdict::Block),
        ("BLOCK_SOMETHING_NEW", json!(null), Verdict::Block),
        (
            "BLOCK_ILLEGAL",
            json!("csam_suspected"),
            Verdict::BlockIllegal,
        ),
    ] {
        assert_eq!(
            parse_outcome(200, &media(word, cat, 9), false)
                .unwrap()
                .verdict,
            want,
            "{word}"
        );
    }
    for body in [
        media("PASS_RATED", json!(null), 9),
        media("pass", json!(null), 9),
        media("ALLOW", json!(null), 9),
        media("PASS", json!("sexually_explicit"), 9),
        media("PASS", json!(null), 0),
        json!({"stats": {"framesSampled": 3}}),
        json!([1, 2, 3]),
    ] {
        assert!(
            matches!(
                parse_outcome(200, &body, false),
                Err(CallError::Unrecognised(_))
            ),
            "{body}"
        );
    }
    assert!(matches!(
        parse_outcome(500, &media("PASS", json!(null), 9), false),
        Err(CallError::Unrecognised(_))
    ));
}

#[test]
fn error_kinds_split_into_input_and_service() {
    // classify by status alone, or treat an unknown kind as INPUT -> red
    assert_eq!(INPUT_KINDS, &["UNSUPPORTED_INPUT", "TEXT_TOO_LONG"]);
    let env =
        |kind: &str| json!({"error": {"kind": kind, "detail": "/staging/abc.png: cannot decode"}});
    assert_eq!(
        parse_outcome(200, &env("UNSUPPORTED_INPUT"), false),
        Err(CallError::Input("UNSUPPORTED_INPUT".into()))
    );
    assert_eq!(
        parse_outcome(413, &env("TEXT_TOO_LONG"), true),
        Err(CallError::Input("TEXT_TOO_LONG".into()))
    );
    for (status, kind) in [
        (200, "PIPELINE_FAILURE"),
        (200, "SOURCE_MUTATED"),
        (200, "CLIENT_GONE"),
        (404, "SOURCE_NOT_FOUND"),
        (403, "SOURCE_UNREADABLE"),
        (403, "SOURCE_OUTSIDE_ROOT"),
        (400, "SOURCE_INVALID"),
        (409, "CONTENT_ID_MISMATCH"),
        (200, "TEXT_CHECK_FAILURE"),
        (404, "TEXT_CHECK_NOT_CONFIGURED"),
        (422, "BAD_REQUEST"),
        (200, "A_KIND_FROM_THE_FUTURE"),
    ] {
        match parse_outcome(status, &env(kind), false) {
            Err(CallError::Unrecognised(why)) => {
                assert!(
                    !why.contains("staging"),
                    "the detail never leaves the client: {why}"
                )
            }
            other => panic!("{kind}: {other:?}"),
        }
    }
    // the framework's own 422 shape
    assert!(matches!(
        parse_outcome(422, &json!({"detail": [{"msg": "field required"}]}), false),
        Err(CallError::Unrecognised(_))
    ));
}

#[test]
fn a_text_pass_needs_its_three_signals_and_a_block_does_not() {
    // fix F13. Requiring the stats on a BLOCK_ILLEGAL -> a damaged answer is no longer refused and sealed -> red
    let body = |v: &str, c: serde_json::Value| {
        json!({"verdict": v, "category": c,
        "stats": {"termRule": true, "explicit": false, "minor": true}})
    };
    let o = parse_outcome(200, &body("BLOCK_ILLEGAL", json!("csam_suspected")), true).unwrap();
    assert_eq!(
        (o.verdict, o.text, o.unknown_block),
        (Verdict::BlockIllegal, Some((true, false, true)), false)
    );
    assert_eq!(
        parse_outcome(200, &body("PASS", json!(null)), true)
            .unwrap()
            .verdict,
        Verdict::Pass
    );
    let bare = |v: &str| json!({"verdict": v, "category": null, "stats": {}});
    assert!(matches!(
        parse_outcome(200, &bare("PASS"), true),
        Err(CallError::Unrecognised(_))
    ));
    let damaged = parse_outcome(200, &bare("BLOCK_ILLEGAL"), true).unwrap();
    assert_eq!(
        (damaged.verdict, damaged.text),
        (Verdict::BlockIllegal, None),
        "still the strongest signal"
    );
    assert_eq!(
        parse_outcome(200, &json!({"verdict": "BLOCK_POLICY"}), true)
            .unwrap()
            .verdict,
        Verdict::Block
    );
    // a BLOCK word this node does not know refuses, and is flagged for an alert
    let odd = parse_outcome(200, &bare("BLOCK_SOMETHING_NEW"), false).unwrap();
    assert_eq!((odd.verdict, odd.unknown_block), (Verdict::Block, true));
    for known in ["BLOCK_POLICY", "BLOCK_UNRESOLVED", "BLOCK_ILLEGAL"] {
        assert!(
            !parse_outcome(200, &bare(known), false)
                .unwrap()
                .unknown_block
        );
    }
}
