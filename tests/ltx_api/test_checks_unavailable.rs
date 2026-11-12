// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks N2/N5, invariants 1 and 6, and fixes F1 to F4, where no verdict comes back. The job fails with
//! the one `CHECK_UNAVAILABLE` sentence; WHICH check and WHY is in the log line, which these tests read. Whether
//! the breaker opens is the service's doing, not one job's: after a fault the node probes at once, and only a
//! failed probe (or an unreachable socket) opens it.

use super::checks_renderer::*;
use super::checks_svc::*;
use super::ltx_task_support::{mark_pending, pending_count, S5_ENV_LOCK};
use super::relight_stub::mp4_clip;
use fabstir_llm_node::ltx::checks::config::Mode;
use fabstir_llm_node::ltx::checks::{
    CHECK_UNAVAILABLE, MSG_BREAKER, MSG_UNAVAILABLE, MSG_UNCHECKABLE,
};
use fabstir_llm_node::ltx::provenance::SELF_TEST_MP4;
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::Duration;

fn pipeline_failure() -> Reply {
    let e = json!({"error": {"kind": "PIPELINE_FAILURE", "detail": "RuntimeError: decoder produced 3 of 9 frames"}});
    Reply::Raw(200, e)
}

#[tokio::test]
async fn one_jobs_service_fault_does_not_open_the_breaker() {
    // fix F2. Opening the breaker on the fault itself (no probe first) -> the next job is rejected -> red;
    // an alert per failed job (no rate limit) -> two alerts -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _capture) = Logs::capture();
    let rig = rig(Mode::Enforce).await;
    let (renderer, server) = setup(&rig).await;
    // the service reports some decode failures a customer's file can cause as PIPELINE_FAILURE
    rig.svc.set_media("png", 0, pipeline_failure());
    let cids = serve_blobs(&[png_bytes(3)]).await;
    for jid in [110, 111] {
        refused(
            &server,
            &renderer,
            &still_job(&cids[0]),
            jid,
            CHECK_UNAVAILABLE,
            MSG_UNAVAILABLE,
        )
        .await;
    }
    assert!(logs.has("c2", "verdict=NONE cause=Unrecognised"));
    assert_eq!(renderer.count("upload"), 0);
    assert_eq!(
        (rig.svc.count("moderate mp4"), rig.svc.count("probe-text")),
        (3, 3),
        "the benign probe ran after each fault (and once at start-up)"
    );
    assert!(
        !rig.checks.breaker_open(),
        "the probe passed: one customer's file is not an outage"
    );
    assert_eq!(rig.alert_count("breaker-open"), 0);
    assert_eq!(
        rig.alert_count("check-unavailable-Unrecognised"),
        1,
        "reported once in five minutes, not once a job"
    );
    assert!(
        rig.refusals().is_empty(),
        "no verdict, so no refusal record"
    );
    assert!(!rig.checks.cooldown().paused(ADDR));
    let next = run_job(&server, &t2v_job(), "s-next", Some(112), Some(ADDR), false).await;
    assert!(next.accepted, "the next job runs: {:?}", next.terminal);
    assert_eq!(next.message(), "no frames produced");
}

#[tokio::test]
async fn a_broken_service_opens_the_breaker_once_its_probe_fails() {
    // fix F2. No breaker after a failed probe -> the last job is accepted -> red; PASS_RATED treated as a pass ->
    // the job renders -> red
    let _env = S5_ENV_LOCK.lock().await;
    for (i, reply) in [
        Reply::Verdict("PASS_RATED", None),
        pipeline_failure(),
        Reply::NotJson,
        Reply::Raw(503, json!({"detail": "busy"})),
    ]
    .into_iter()
    .enumerate()
    {
        let rig = rig(Mode::Enforce).await;
        let (renderer, server) = setup(&rig).await;
        *rig.svc.fail_all.lock().unwrap() = Some(reply);
        let cids = serve_blobs(&[png_bytes(3)]).await;
        refused(
            &server,
            &renderer,
            &still_job(&cids[0]),
            120 + i as u64,
            CHECK_UNAVAILABLE,
            MSG_UNAVAILABLE,
        )
        .await;
        assert_eq!(renderer.count("upload"), 0);
        assert!(
            rig.checks.breaker_open(),
            "the probe clip failed too: the service is at fault"
        );
        assert_eq!(rig.alert_count("breaker-open"), 1);
        assert!(rig.refusals().is_empty());
        let next = run_job(&server, &t2v_job(), "s-next", Some(130), Some(ADDR), false).await;
        assert!(
            !next.accepted,
            "the breaker rejects the next job before accept"
        );
        assert_eq!(next.message(), MSG_BREAKER);
    }
}

#[tokio::test]
async fn a_hung_check_on_one_clip_times_out_and_releases_everything() {
    // invariant 6. The check's limit removed -> run_job's own guard panics -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _capture) = Logs::capture();
    let rig = rig(Mode::Enforce).await;
    let (renderer, server) = setup(&rig).await;
    // a heavy clip the service never answers; the benign probe clip still passes
    rig.svc.spare_probe.store(true, Ordering::SeqCst);
    rig.svc.set_media("mp4", 0, Reply::Hang);
    let cids = serve_blobs(&[mp4_clip(121)]).await;
    let started = std::time::Instant::now();
    refused(
        &server,
        &renderer,
        &clip_job(&cids[0], false),
        105,
        CHECK_UNAVAILABLE,
        MSG_UNAVAILABLE,
    )
    .await;
    assert!(started.elapsed() < Duration::from_secs(15));
    assert!(logs.has("c3", "verdict=NONE cause=Timeout"));
    assert_eq!(renderer.count("upload"), 0);
    assert!(
        rig.files_in("staging").is_empty(),
        "a timed-out check still removes its staged copy"
    );
    assert!(
        !rig.checks.breaker_open(),
        "the probe passed after the timeout: the breaker stays closed (fix F2)"
    );
}

#[tokio::test]
async fn a_client_that_leaves_mid_check_does_not_hold_the_slot() {
    // invariant 6. The check's limit removed -> the job never ends -> the 15 s wait below -> red
    use fabstir_llm_node::api::websocket::handlers::ltx::handle_encrypted_ltx_generate_from;
    let _env = S5_ENV_LOCK.lock().await;
    let rig = rig(Mode::Enforce).await;
    let (renderer, server) = setup(&rig).await;
    rig.svc.set_text(Reply::Hang);
    mark_pending(&server, 106).await;
    let (_ack, task) = handle_encrypted_ltx_generate_from(
        &server,
        &t2v_job(),
        &super::relight_stub::key(),
        "s-gone",
        Some(106),
        None,
        Some(ADDR),
    )
    .await;
    let mut task = task.expect("accepted");
    task.pending_marked = true;
    let client = server.client_for(&task).await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let running = tokio::spawn(task.run(
        client,
        super::relight_stub::key(),
        "s-gone".into(),
        server.clone(),
        tx,
    ));
    assert!(
        rx.recv().await.is_some(),
        "the checking stage arrives, then the check hangs"
    );
    assert_eq!(
        server.ltx_semaphore().available_permits(),
        0,
        "the job holds the slot while it checks"
    );
    drop(rx); // the client disconnects mid-check
    tokio::time::timeout(Duration::from_secs(15), running)
        .await
        .expect("the job ends within its bounds")
        .unwrap();
    assert_eq!(server.ltx_semaphore().available_permits(), 1);
    assert_eq!(pending_count(&server, 106).await, 0);
    assert_eq!(renderer.count("prompt"), 0);
}

#[tokio::test]
async fn an_item_the_check_cannot_read_fails_only_its_own_job() {
    // INPUT class. An INPUT kind handled as a service fault -> CHECK_UNAVAILABLE instead -> red
    let _env = S5_ENV_LOCK.lock().await;
    let rig = rig(Mode::Enforce).await;
    let (renderer, server) = setup(&rig).await;
    let unsupported = json!({"error": {"kind": "UNSUPPORTED_INPUT", "detail": "animated image"}});
    rig.svc.set_media("png", 0, Reply::Raw(200, unsupported));
    let cids = serve_blobs(&[png_bytes(4), b"GIF89a not a still the check reads".to_vec()]).await;
    for jid in 130..133 {
        refused(
            &server,
            &renderer,
            &still_job(&cids[0]),
            jid,
            "VALIDATION_FAILED",
            MSG_UNCHECKABLE,
        )
        .await;
    }
    // the node's own first-bytes test gives the same answer without a call
    let before = rig.svc.count("moderate png");
    refused(
        &server,
        &renderer,
        &still_job(&cids[1]),
        133,
        "VALIDATION_FAILED",
        MSG_UNCHECKABLE,
    )
    .await;
    assert_eq!(rig.svc.count("moderate png"), before);
    assert_eq!(renderer.count("upload"), 0);
    assert_eq!(
        rig.svc.count("moderate mp4"),
        1,
        "no probe: the item is at fault, not the service"
    );
    assert!(!rig.checks.breaker_open());
    assert!(rig.alerts.all().is_empty(), "{:?}", rig.alerts.all());
    assert!(
        rig.refusals().is_empty(),
        "not a content refusal: nothing counted"
    );
    // and the next job runs
    *rig.svc.media_rule.lock().unwrap() = None;
    let next = run_job(
        &server,
        &still_job(&cids[0]),
        "s-next",
        Some(134),
        Some(ADDR),
        false,
    )
    .await;
    assert!(next.accepted);
    assert_eq!(next.message(), "no frames produced", "{:?}", next.terminal);
    assert_eq!(renderer.count("upload"), 1, "a checked still is uploaded");
    assert!(
        rig.files_in("in").is_empty(),
        "and deleted at job end (question 3)"
    );
}

#[tokio::test]
async fn staging_faults_follow_the_probe() {
    // fix F3. No probe after a staging fault -> a broken staging area never opens the breaker -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _capture) = Logs::capture();
    let rig = rig_with(Mode::Enforce, true, |c| c.staging_cap_bytes = 100_000).await;
    let (renderer, server) = setup(&rig).await;
    // one item too large for the staging area: the probe clip still fits, so only this job fails
    let mut big = png_bytes(5);
    big.resize(120_000, 7);
    let cids = serve_blobs(&[big, png_bytes(6)]).await;
    refused(
        &server,
        &renderer,
        &still_job(&cids[0]),
        140,
        CHECK_UNAVAILABLE,
        MSG_UNAVAILABLE,
    )
    .await;
    assert!(logs.has("c2", "verdict=NONE cause=StagingFull"));
    assert_eq!(rig.svc.count("moderate png"), 0);
    assert!(!rig.checks.breaker_open(), "the staging area itself works");
    // the staging area itself is full (or broken): the probe cannot stage its clip either
    std::fs::write(
        rig.dir("staging").join("left-by-something-else.bin"),
        vec![0u8; 100_000],
    )
    .unwrap();
    refused(
        &server,
        &renderer,
        &still_job(&cids[1]),
        141,
        CHECK_UNAVAILABLE,
        MSG_UNAVAILABLE,
    )
    .await;
    assert!(
        rig.checks.breaker_open(),
        "a staging area nothing can be written to stops video jobs before accept"
    );
    assert_eq!(rig.alert_count("breaker-open"), 1);
}

#[tokio::test]
async fn the_result_check_has_its_own_limit_and_a_used_budget_never_opens_the_breaker() {
    // fix F1. The result check drawing on the inputs' budget -> the last call fails -> red;
    // a used-up budget opening the breaker (or probing) -> red
    let (logs, _capture) = Logs::capture();
    let rig = rig(Mode::Enforce).await;
    let mut job = rig.checks.job("s-budget", Some(7), Some(ADDR));
    job.spent = rig.checks.config().job_budget; // the inputs used every second of the shared budget
    let r = rig
        .checks
        .check_still(&mut job, &png_bytes(8))
        .await
        .unwrap_err();
    assert_eq!(
        (r.code, r.message.as_str()),
        (CHECK_UNAVAILABLE, MSG_UNAVAILABLE)
    );
    assert!(logs.has("c2", "verdict=NONE cause=BudgetUsed"));
    assert_eq!(
        (rig.svc.count("moderate png"), rig.svc.count("moderate mp4")),
        (0, 1),
        "nothing was sent, and no probe was needed"
    );
    assert!(!rig.checks.breaker_open());
    assert_eq!(rig.alert_count("breaker-open"), 0);
    // after the render, the result is still checked, with a full limit of its own
    rig.checks
        .check_result(&mut job, "LTX_00001.mp4", SELF_TEST_MP4)
        .await
        .expect("the result check does not draw on the inputs' budget");
    assert_eq!(rig.svc.count("moderate mp4"), 2);
    assert_eq!(job.spent, rig.checks.config().job_budget);
    assert!(job.total > Duration::ZERO, "all check time is accounted");
}

#[tokio::test]
async fn a_slow_staging_write_leaves_no_file_behind() {
    // fix F4. A writer that does not clean up after an abandoned write -> the clip stays in staging -> red
    let rig = rig_full(
        Mode::Enforce,
        false,
        |c| c.check_timeout = Duration::from_millis(300),
        |mut s| {
            s.write_delay = Duration::from_millis(900);
            s
        },
    )
    .await;
    let mut job = rig.checks.job("s-slow", Some(8), Some(ADDR));
    let r = rig
        .checks
        .check_clip(&mut job, &mp4_clip(121))
        .await
        .unwrap_err();
    assert_eq!(r.message, MSG_UNAVAILABLE);
    // the check gave up at 300 ms; the blocking write finishes 600 ms after that
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(
        rig.files_in("staging").is_empty(),
        "a write that finished after its check was abandoned removed its own file: {:?}",
        rig.files_in("staging")
    );
}
