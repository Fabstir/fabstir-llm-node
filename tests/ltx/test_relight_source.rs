// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// VFX Passes (VP1.1): source checks pinning the call sites of the pure relight decisions inside `run()` (a test server
// has no checkpoint manager, so D20, D18 and the proof path cannot run end to end; precedent
// `tests/ltx_api/test_ws_write_bound.rs`). Every check strips ALL whitespace from both sides first.

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn run_body() -> String {
    let src = squash(include_str!("../../src/api/websocket/handlers/ltx.rs"));
    let at = src.find("pubasyncfnrun(").expect("run() present");
    src[at..].to_string()
}

fn window(src: &str, from: &str, to: &str) -> String {
    let (from, to) = (squash(from), squash(to));
    let a = src
        .find(&from)
        .unwrap_or_else(|| panic!("anchor {from:?} missing"));
    let b = a + src[a..]
        .find(&to)
        .unwrap_or_else(|| panic!("anchor {to:?} missing after {from:?}"));
    src[a..b].to_string()
}

#[test]
fn d20_gates_on_the_family_model() {
    // a bare session_gate( (always the Lightricks id) -> red
    let run = run_body();
    let w = window(
        &run,
        "if let Some(cm) = server.get_checkpoint_manager().await",
        "prepare_inputs(",
    );
    assert!(w.contains("session_gate_for_model(") && w.contains("expected_session_model("));
    assert!(!w.contains("template::session_gate("));
}

#[test]
fn admission_sits_after_d20_and_before_staging() {
    // admission after prepare_inputs -> red; no pre-submit re-poll -> red
    let run = run_body();
    let w = window(&run, "session_gate_for_model(", "prepare_inputs(");
    assert!(w.contains("admit_relight(") && w.contains("free_relight_before_ltx("));
    let w = window(&run, "prepare_inputs(", "client.submit(");
    assert!(w.contains("repoll_vram("));
}

#[test]
fn the_finalising_gate_rereads_the_session() {
    // hard-coding terms = Ok(()) -> red; no Abandon return before finalize_clip -> red
    let run = run_body();
    let w = window(
        &run,
        "send_stage(&progress_tx, \"finalising\"",
        "attestation::assemble(",
    );
    for needle in [
        "read_with_retry(",
        "query_session_jobs_raw(",
        "decode_session_snapshot(",
        "check_session_terms(",
        "relight_finalise_decision(",
    ] {
        assert!(w.contains(needle), "{needle}");
    }
    let abandon = w
        .find("FinaliseDecision::Abandon(")
        .expect("an Abandon arm");
    assert!(w[abandon..].contains("return;"));
    let w = window(
        &run,
        "send_stage(&progress_tx, \"finalising\"",
        "submit::finalize_clip(",
    );
    assert!(
        w.contains(&squash(
            "tokens_before = snap.as_ref().map(|s| s.tokens_used)"
        )),
        "tokens_before from the pre-submit read"
    );
}

#[test]
fn delivery_goes_through_completion_outcome() {
    // a second hand-built ltx_complete frame, or a delivery that ignores `submitted` -> red
    let src = squash(include_str!("../../src/api/websocket/handlers/ltx.rs"));
    assert_eq!(
        src.matches("ltx_complete_inner(").count(),
        2,
        "its definition and inside completion_outcome"
    );
    let run = run_body();
    let w = window(
        &run,
        "submit::finalize_clip(",
        ".send(build_encrypted_ltx_response(",
    );
    for needle in [
        "read_with_retry(",
        "query_session_jobs_raw(",
        "completion_outcome(",
    ] {
        assert!(w.contains(needle), "{needle}");
    }
    assert!(!w.contains("_submitted)"));
}

#[test]
fn the_attestation_takes_relight_ids_and_env() {
    // LTX_MODEL_ID / LTX env for relight -> red
    let run = run_body();
    let w = window(
        &run,
        "send_stage(&progress_tx, \"finalising\"",
        "attestation::assemble(",
    );
    assert!(w.contains("attestation_model_id(") && w.contains("relight_env_meta("));
}

#[test]
fn relight_frames_upload_with_retry() {
    let run = run_body();
    let w = window(
        &run,
        "send_stage(&progress_tx, \"encrypting\"",
        "send_stage(&progress_tx, \"uploading\"",
    );
    assert!(w.contains("upload_frame_with_retry(") && w.contains("RELIGHT_UPLOAD_ATTEMPTS"));
}

#[test]
fn deadline_is_checked_after_watch() {
    let run = run_body();
    let w = window(
        &run,
        "match watch_handle.await",
        "client.outputs(&prompt_id)",
    );
    assert!(w.contains("relight_finalise_decision(") && w.contains("deadline_secs"));
}
