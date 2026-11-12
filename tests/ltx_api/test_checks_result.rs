// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks N6/N9 and invariants 1, 2, 7 and the node half of 9 at the RESULT stage: one job driven from
//! accept to its terminal frame with a real `CheckpointManager` over a stub chain, the in-memory S5 backend and a
//! provenance signer that really signs. So what is asserted is observed: the proof transaction's claimed tokens,
//! the uploaded frame, the signature count, and the renderer's folders. Each test names the mutation that turns
//! it red.

use super::checks_chain::*;
use super::checks_renderer::*;
use super::checks_svc::*;
use super::ltx_task_support::{pending_count, S5_ENV_LOCK};
use super::relight_stub::store;
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::contracts::checkpoint_manager::CheckpointManager;
use fabstir_llm_node::ltx::checks::config::Mode;
use fabstir_llm_node::ltx::checks::{
    CHECK_UNAVAILABLE, CONTENT_REFUSED, MSG_REFUSED, MSG_UNAVAILABLE,
};
use fabstir_llm_node::ltx::provenance::SELF_TEST_MP4;
use fabstir_llm_node::ltx::submit::ltx_tokens;
use std::sync::atomic::Ordering;
use std::sync::Arc;

const ADDR: &str = "0xd00d000000000000000000000000000000000005";
const FRAME0: &str = "home/ltx/job_{}-p1/frame_00000.bin";

struct World {
    rig: Rig,
    renderer: Arc<Renderer>,
    server: Arc<ApiServer>,
    chain: Arc<Chain>,
    signer: Arc<Signing>,
    cm: Arc<CheckpointManager>,
}

/// The whole node side in-process; the stub renderer "generates" the bundled benign clip.
async fn world() -> World {
    let rig = rig(Mode::Enforce).await;
    let (renderer, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
    *renderer.output.lock().unwrap() = Some(SELF_TEST_MP4.to_vec());
    let server = Arc::new(ApiServer::new_for_test());
    server.set_ltx_client(client).await;
    server.set_ltx_template_store(store()).await;
    server.set_content_checks(rig.checks.clone());
    let signer = install_signing_provenance(&server).await;
    let st = store();
    let model = fabstir_llm_node::ltx::template::expected_session_model(
        st.entry("ltx-t2v-hdr"),
        "ltx-t2v-hdr",
    );
    let (chain, url) = spawn_chain(model).await;
    let cm = install_checkpoint_manager(&server, &url).await;
    World {
        rig,
        renderer,
        server,
        chain,
        signer,
        cm,
    }
}

async fn frame_uploaded(w: &World, jid: u64) -> bool {
    w.cm.get_s5_storage()
        .exists(&FRAME0.replace("{}", &jid.to_string()))
        .await
        .unwrap()
}

#[tokio::test]
async fn a_job_that_passes_bills_exactly_its_estimate() {
    // invariants 2 and 7. A check that alters the job, or a result check that never passes -> red;
    // no job-end delete -> the output stays in the renderer's folder -> red
    let _env = S5_ENV_LOCK.lock().await;
    let w = world().await;
    let ran = run_job(
        &w.server,
        &t2v_job(),
        "s-pass",
        Some(501),
        Some(ADDR),
        false,
    )
    .await;
    assert_eq!(ran.terminal["type"], "ltx_complete", "{:?}", ran.terminal);
    let estimate = ltx_tokens(121, 1280, 720);
    assert_eq!(ran.terminal["billing"]["tokens"], estimate);
    assert_eq!(
        w.chain.claimed_tokens(),
        vec![estimate],
        "one proof, claiming exactly the estimate"
    );
    assert_eq!(
        w.signer.signed.load(Ordering::SeqCst),
        1,
        "the checked clip was marked once"
    );
    assert!(frame_uploaded(&w, 501).await, "and uploaded");
    assert!(ran.terminal["outputCID"]
        .as_str()
        .is_some_and(|c| !c.is_empty()));
    assert_eq!(
        ran.stages.iter().filter(|s| *s == "checking").count(),
        2,
        "{:?}",
        ran.stages
    );
    assert_eq!(
        w.rig.svc.count("moderate-text"),
        1,
        "the prompt was checked"
    );
    assert_eq!(
        w.rig.svc.count("moderate mp4"),
        2,
        "the probe clip and this job's result"
    );
    assert!(w.rig.refusals().is_empty());
    assert_eq!(w.renderer.count("view"), 1);
    assert!(
        w.rig.files_in("out").is_empty(),
        "the job's output is deleted from the renderer at job end"
    );
    assert!(w.rig.files_in("staging").is_empty());
    assert_eq!(pending_count(&w.server, 501).await, 0);
    assert_eq!(w.server.ltx_semaphore().available_permits(), 1);
}

async fn refused_result(reply: Reply, jid: u64, code: &str, message: &str) -> World {
    let w = world().await;
    w.rig.svc.set_media("mp4", 1, reply); // call 0 was the start-up probe clip
    let ran = run_job(
        &w.server,
        &t2v_job(),
        "s-result",
        Some(jid),
        Some(ADDR),
        false,
    )
    .await;
    assert_eq!(
        (ran.code(), ran.message()),
        (code, message),
        "{:?}",
        ran.terminal
    );
    assert_eq!(
        w.renderer.count("prompt"),
        1,
        "the render ran: this is the result check"
    );
    assert_eq!(
        w.renderer.count("view"),
        1,
        "only the first artefact was fetched"
    );
    assert!(
        w.chain.raw_txs.lock().unwrap().is_empty(),
        "no proof transaction: nothing is billed"
    );
    assert_eq!(
        w.signer.signed.load(Ordering::SeqCst),
        0,
        "no provenance signature"
    );
    assert!(!frame_uploaded(&w, jid).await, "no S5 upload");
    assert!(ran.terminal.get("outputCID").is_none() && ran.terminal.get("frames").is_none());
    assert!(
        !ran.stages
            .iter()
            .any(|s| s == "uploading" || s == "finalising"),
        "{:?}",
        ran.stages
    );
    assert_eq!(
        pending_count(&w.server, jid).await,
        0,
        "the pending proof is forfeited"
    );
    assert_eq!(w.server.ltx_semaphore().available_permits(), 1);
    assert!(
        w.rig.files_in("out").is_empty(),
        "the refused result is deleted from the renderer at once"
    );
    assert!(
        w.rig.staging_drained().await,
        "nothing of the job stays staged"
    );
    w
}

#[tokio::test]
async fn a_refused_result_is_never_marked_uploaded_or_billed() {
    // invariant 1. The result check's refusal ignored -> a signature, an upload and a proof transaction appear
    // -> red
    let _env = S5_ENV_LOCK.lock().await;
    let w = refused_result(
        Reply::Verdict("BLOCK_POLICY", Some("sexually_explicit")),
        502,
        CONTENT_REFUSED,
        MSG_REFUSED,
    )
    .await;
    let rec = w.rig.refusals();
    assert_eq!(
        (rec.len(), &rec[0]["stage"], &rec[0]["client"]),
        (1, &serde_json::json!("c5"), &serde_json::json!(ADDR))
    );
    assert!(!w.rig.checks.breaker_open(), "a verdict is not an outage");
    // what the service judged was the rendered clip itself
    let seen = w.rig.svc.files.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "the start-up probe and this job's result");
    assert_eq!(seen[1].len, SELF_TEST_MP4.len());
    assert_eq!(&seen[1].head[4..8], b"ftyp");
}

#[tokio::test]
async fn only_pass_releases_a_result() {
    // invariant 2. PASS_RATED (or a silent service) delivering -> ltx_complete with CIDs -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _capture) = Logs::capture();
    let w = refused_result(
        Reply::Verdict("PASS_RATED", None),
        503,
        CHECK_UNAVAILABLE,
        MSG_UNAVAILABLE,
    )
    .await;
    assert!(logs.has("c5", "verdict=NONE cause=Unrecognised"));
    assert!(
        w.rig.checks.breaker_open(),
        "the probe clip got the same answer: the service is at fault"
    );
    drop(w);
    refused_result(Reply::Hang, 504, CHECK_UNAVAILABLE, MSG_UNAVAILABLE).await;
    assert!(logs.has("c5", "verdict=NONE cause=Timeout"));
    // an answer that names no bundle is not an answer from the pinned bundle (fix F13)
    let unnamed = serde_json::json!({"verdict": "PASS", "category": null,
        "stats": {"framesSampled": 24, "maxScore": 0.01, "escalated": 0}});
    refused_result(
        Reply::Raw(200, unnamed),
        505,
        CHECK_UNAVAILABLE,
        MSG_UNAVAILABLE,
    )
    .await;
}

#[tokio::test]
async fn a_first_artefact_that_is_not_a_video_is_not_delivered() {
    // D5: every kind delivers its mp4 first. A token minted for an unjudged EXR -> Ok -> red
    let (logs, _capture) = Logs::capture();
    let rig = rig(Mode::Enforce).await;
    let mut job = rig.checks.job("s", Some(9), None);
    let r = rig
        .checks
        .check_result(&mut job, "frame_00001.exr", b"v/1\x01")
        .await
        .unwrap_err();
    assert_eq!(
        (r.code, r.message.as_str()),
        (CHECK_UNAVAILABLE, MSG_UNAVAILABLE)
    );
    assert!(logs.has("c5", "verdict=NONE cause=NotVideo"));
    assert_eq!(
        rig.svc.files.lock().unwrap().len(),
        1,
        "only the start-up probe reached the service"
    );
    assert!(!rig.checks.breaker_open());
    rig.checks
        .check_result(&mut job, "LTX_00001.MP4", SELF_TEST_MP4)
        .await
        .unwrap();
}

#[tokio::test]
async fn slow_checks_do_not_cost_a_job_its_signing_or_delivery() {
    // fix F5. The reservation without the checks' allowance -> the deadline assertion -> red
    let _env = S5_ENV_LOCK.lock().await;
    let w = world().await;
    // a healthy but slow service: each answer takes most of a second
    w.rig.svc.delay_ms.store(700, Ordering::SeqCst);
    let ran = run_job(
        &w.server,
        &t2v_job(),
        "s-slow",
        Some(506),
        Some(ADDR),
        false,
    )
    .await;
    assert_eq!(ran.terminal["type"], "ltx_complete", "{:?}", ran.terminal);
    assert_eq!(w.chain.claimed_tokens(), vec![ltx_tokens(121, 1280, 720)]);
    // the signer was asked to hold the slot for the render, the delivery tail AND the longest the checks can take
    let job_timeout: u64 = std::env::var("LTX_JOB_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1800);
    let allowance = w.rig.checks.max_check_secs();
    assert_eq!(
        allowance,
        5 + 2 + 60 + 10,
        "budget, result limit, seal, record"
    );
    assert_eq!(
        w.signer.reserved_secs.load(Ordering::SeqCst),
        job_timeout + 900 + allowance
    );
}

#[test]
fn check_time_is_kept_out_of_the_render_and_delivery_deadlines() {
    // fix F5. A relight deadline read from the wall clock alone -> the source assertions -> red
    use fabstir_llm_node::api::websocket::handlers::ltx::{
        reservation_deadline_secs, work_elapsed,
    };
    use std::time::Duration;
    assert_eq!(
        reservation_deadline_secs(1800, 0),
        2700,
        "checks off: as before"
    );
    assert_eq!(reservation_deadline_secs(1800, 490), 3190);
    assert_eq!(reservation_deadline_secs(u64::MAX, 490), 6 * 3600, "capped");
    assert_eq!(
        work_elapsed(Duration::from_secs(2800), Duration::from_secs(300)),
        Duration::from_secs(2500),
        "a relight job that spent 300 s in checks is inside its 2700 s deadline"
    );
    assert_eq!(
        work_elapsed(Duration::from_secs(10), Duration::from_secs(30)),
        Duration::ZERO
    );
    let src: String = include_str!("../../src/api/websocket/handlers/ltx.rs")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let run = &src[src.find("pubasyncfnrun(").unwrap()..];
    assert_eq!(
        run.matches("relight_finalise_decision(work_elapsed(started.elapsed(),cjob.total),")
            .count(),
        2,
        "both relight deadline checks leave check time out"
    );
    assert!(!run.contains("relight_finalise_decision(started.elapsed(),"));
    assert!(run.contains("admit_cfg.deadline_secs.saturating_add(checks.max_check_secs())"));
    assert!(src.contains(
        "reservation_deadline_secs(timeout_secs,server.content_checks().max_check_secs(),)"
    ));
}
