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
    assert!(w.contains("relight_finalise_decision(") && w.contains("deadline_for("));
}

// ---- Batch D21 wiring (a test server has no checkpoint manager, so these
// call sites cannot run end to end; the decisions are tested in
// test_keepalive / test_final_proof / test_relight_admission) ----

#[test]
fn d21_admission_runs_on_the_gate_snapshot_before_gpu_admission() {
    // admission after admit_relight (GPU held) or without the gate's snapshot -> red
    let run = run_body();
    let w = window(&run, "session_gate_for_model(", "admit_relight(");
    assert!(w.contains("Ok(snap)=>admission_snap=Some(snap)"), "{w}");
    assert!(w.contains("relight_session_admission("));
}

#[test]
fn d21_keepalive_starts_after_submit_and_feeds_on_watch_progress() {
    // started before the prompt was accepted -> red; progress never shared -> red
    let run = run_body();
    let w = window(&run, "client.submit(&patched_graph)", "wc.watch(");
    assert!(w.contains("start_keepalive("));
    // review round 1: its inputs come from the pure, tested keepalive_cfg_for,
    // fed the D20 admission snapshot
    // round 2: the FULL argument list, so the job's start (not "now") feeds the
    // keep-alive's deadline
    assert!(w.contains(&squash(
        "keepalive_cfg_for( crate::ltx::keepalive::KeepAliveIds { job_id: jid, job_tag: format!(\"{jid}-{prompt_id}\"), session_id: session_id.clone(), host: cm.get_host_address(), model_id, }, snap, &admit_cfg, &job.template_id, job.frames, submit::ltx_tokens(job.frames, job.resolution.w, job.resolution.h), started_at, crate::ltx::keepalive::unix_now(), );"
    )));
    assert!(w.contains(&squash("start_keepalive( ka_cfg,")));
    assert!(w.contains(&squash(
        "let (Some(jid), Some(snap)) = (job_id, admission_snap.as_ref())"
    )));
    // round 3: relight jobs ONLY (an LTX final claims its full tokens under the
    // Ltx policy — interims there would over-claim), and only this one start
    assert!(w.contains(&squash(
        "if is_relight && pending_marked { if let (Some(jid), Some(snap)) = (job_id, admission_snap.as_ref()) {"
    )));
    assert_eq!(
        run.matches("start_keepalive(").count(),
        1,
        "one start in run"
    );
    let w = window(
        &run,
        "while let Some(p) = watch_rx.recv().await",
        "if client_gone",
    );
    assert!(w.contains("set_progress("));
}

#[test]
fn d21_keepalive_stops_before_the_finalising_read_and_on_every_exit() {
    // the stop after the re-read (a tick could add after it) -> red; the claim
    // gated on the full tokens -> red; no stop in the single-exit cleanup -> red
    let run = run_body();
    let w = window(
        &run,
        "send_stage(&progress_tx, \"finalising\"",
        "submit::finalize_clip(",
    );
    let stop = w
        .find("g.stop().await")
        .expect("stopped on the success path");
    let read = w.find("read_with_retry(").expect("the finalising re-read");
    assert!(
        stop < read,
        "stop (abort + await) BEFORE the finalising re-read"
    );
    // review round 1: the claim and the final's timing come from the pure,
    // tested relight_final_inputs (admission baseline, interim_sent + flag)
    assert!(w.contains(&squash(
        "relight_inputs = snap.as_ref().ok().map(|s| { crate::ltx::final_proof::relight_final_inputs( tokens, admission_snap.as_ref(), s, interim_sent, interim_in_flight, &admit_cfg, &job.template_id, job.frames, started_at, ) });"
    )));
    // round 3: every claim (relight or not) goes through the pure final_claim,
    // which also subtracts interim_sent on the Ltx policy
    assert!(w.contains(&squash(
        "claim = crate::ltx::final_proof::final_claim( tokens, relight_inputs.as_ref(), interim_sent, );"
    )));
    assert_eq!(
        run.matches("final_claim(").count(),
        2,
        "before and inside the relight read"
    );
    assert!(w.contains(&squash("(g.stop().await, g.shared().in_flight())")));
    assert!(w.contains(&squash("check_session_terms(s, host, claim, tracked)")));
    let w = window(&run, "catch_unwind()", "finish_ltx_task(");
    assert!(w.contains(&squash("if let Some(mut guard) = keepalive.take()")));
    assert!(w.contains("guard.stop().await"));
    // declared outside the core
    let decl = run.find("letmutkeepalive:").expect("declared");
    let core = run.find("letcore=async").expect("core");
    assert!(decl < core);
}

#[test]
fn d21_the_final_claim_and_wire_total_reach_their_own_places() {
    // the tracker or the on-chain claim fed the wire total (or vice versa) -> red
    let run = run_body();
    let w = window(
        &run,
        "submit::finalize_clip(",
        ".send(build_encrypted_ltx_response(",
    );
    assert!(w.contains(&squash("&att, claim, policy,")));
    let w2 = window(&run, "CmSessionReads(&cm)", "submit::finalize_clip(");
    assert!(w2.contains(&squash("final_policy( relight_inputs.as_ref(),")));
    assert!(w.contains(&squash("tokens_before, claim, tokens,")));
    assert!(w.contains(&squash(
        ".track(jid, Some(session_id.clone()), claim, cost)"
    )));
}

#[test]
fn d21_started_at_is_the_jobs_start_set_once_before_the_core() {
    // round 2: re-basing started_at on "now" anywhere later would stretch the
    // keep-alive deadline and the final budget by the render time -> red
    let run = run_body();
    let set = squash("let started_at = tokio::time::Instant::now();");
    assert_eq!(run.matches(&set).count(), 1, "set exactly once");
    let at = run.find(&set).unwrap();
    let core = run.find("letcore=async").expect("core");
    assert!(at < core, "set before the core starts");
    assert_eq!(
        run.matches("started_at=").count(),
        1,
        "no later `started_at =` assignment"
    );
    // round 3: any binding, typed shadows included
    assert_eq!(
        run.matches("letstarted_at").count(),
        1,
        "bound exactly once"
    );
}
