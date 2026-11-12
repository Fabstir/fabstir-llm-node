// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks N2/N3 (D2, D11, D12, D13) and invariant 5: the instant gates before accept, the breaker and its
//! probe, the cool-down, shadow mode, the version endpoint, and `off`. Every refusal asserts its unique MESSAGE
//! (many exits share a code). Each test names the mutation that turns it red.

use super::checks_renderer::*;
use super::checks_svc::*;
use fabstir_llm_node::ltx::checks::config::Mode;
use fabstir_llm_node::ltx::checks::{
    ContentChecks, CHECK_UNAVAILABLE, MSG_BREAKER, MSG_PAUSED, MSG_SHADOW_ENDED, MSG_UNAVAILABLE,
};
use std::time::Duration;

const ADDR_A: &str = "0xa11ce00000000000000000000000000000000001";
const ADDR_B: &str = "0xb0b0000000000000000000000000000000000002";

#[tokio::test]
async fn the_instant_gates_refuse_before_the_slot_is_taken() {
    // gate moved after try_acquire_owned -> CAPACITY instead of the breaker message -> red;
    // breaker starting closed -> the first job is accepted before any probe -> red
    let rig = rig_with(Mode::Enforce, false, |_| {}).await;
    let (_r, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
    let server = server_for(client).await;
    server.set_content_checks(rig.checks.clone());
    let _held = server.ltx_semaphore().try_acquire_owned().unwrap();

    let ran = run_job(&server, &t2v_job(), "s-gate", Some(1), Some(ADDR_A), false).await;
    assert!(!ran.accepted);
    assert_eq!(
        (ran.code(), ran.message()),
        (CHECK_UNAVAILABLE, MSG_BREAKER)
    );
    assert_eq!(
        rig.svc.files.lock().unwrap().len() + rig.svc.texts.lock().unwrap().len(),
        0,
        "no check ran before accept"
    );

    assert!(
        rig.checks.probe_now().await,
        "the start-up probe closes the breaker"
    );
    assert_eq!(
        rig.svc.count("moderate mp4"),
        1,
        "the built-in probe clip was judged"
    );
    assert_eq!(
        rig.svc.count("probe-text"),
        1,
        "and the benign sentence went through the text check"
    );
    assert!(
        rig.files_in("staging").is_empty(),
        "the probe clip left the staging area"
    );
    let ran = run_job(&server, &t2v_job(), "s-gate", Some(1), Some(ADDR_A), false).await;
    assert_eq!(
        ran.code(),
        "CAPACITY",
        "gates pass; only the held slot refuses: {:?}",
        ran.terminal
    );

    // D13: the third refusal in 24 hours pauses that address, and only that address
    for _ in 0..2 {
        rig.checks.cooldown().record(ADDR_A);
    }
    let ran = run_job(&server, &t2v_job(), "s-gate", Some(1), Some(ADDR_A), false).await;
    assert_eq!(ran.code(), "CAPACITY", "two refusals are free");
    rig.checks
        .cooldown()
        .record(&ADDR_A.to_uppercase().replace("0X", "0x"));
    let ran = run_job(&server, &t2v_job(), "s-gate", Some(1), Some(ADDR_A), false).await;
    assert_eq!((ran.code(), ran.message()), (CHECK_UNAVAILABLE, MSG_PAUSED));
    let ran = run_job(&server, &t2v_job(), "s-gate", Some(1), Some(ADDR_B), false).await;
    assert_eq!(ran.code(), "CAPACITY", "another address is not paused");
}

#[test]
fn the_gates_sit_before_the_slot_and_the_signer_reservation() {
    // source order (the signer has no release call, D13): a gate after the reserve burns reservations -> red
    let src: String = include_str!("../../src/api/websocket/handlers/ltx.rs")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let gate = src
        .find(".pre_accept(client_address,session_id)")
        .expect("the gate");
    let slot = src.find("ltx_semaphore().try_acquire_owned()").unwrap();
    let reserve = src.find("rt.signer.reserve(&req)").unwrap();
    assert!(gate < slot && slot < reserve);
    assert!(
        !src[gate..slot].contains(".await"),
        "nothing between the gate and the slot waits"
    );
    // the dispatch hands the authenticated client address to the handler
    let server: String = include_str!("../../src/api/server.rs")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let call = server.find("handle_encrypted_ltx_generate_from(").unwrap();
    assert!(server[call..call + 400].contains("init_signer.as_deref(),"));
}

#[tokio::test]
async fn the_breaker_opens_on_a_dead_service_and_closes_when_it_is_back() {
    // no trip on an unreachable socket -> the second job is accepted -> red; no recovery loop -> never closes -> red
    let (logs, _capture) = Logs::capture();
    let rig = rig_with(Mode::Enforce, true, |c| {
        c.probe_interval = Duration::from_millis(150)
    })
    .await;
    let (renderer, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
    let server = server_for(client).await;
    server.set_content_checks(rig.checks.clone());
    rig.svc.stop();

    super::ltx_task_support::mark_pending(&server, 21).await;
    let ran = run_job(&server, &t2v_job(), "s-down", Some(21), Some(ADDR_A), true).await;
    assert!(ran.accepted);
    assert_eq!(
        (ran.code(), ran.message()),
        (CHECK_UNAVAILABLE, MSG_UNAVAILABLE)
    );
    assert!(
        logs.has("c1", "verdict=NONE cause=Unreachable"),
        "the log says which check and why"
    );
    assert_eq!(
        super::ltx_task_support::pending_count(&server, 21).await,
        0,
        "nothing billed"
    );
    assert_eq!(renderer.count("prompt"), 0, "nothing rendered");
    assert_eq!(
        server.ltx_semaphore().available_permits(),
        1,
        "the slot is free"
    );
    assert!(
        rig.checks.breaker_open(),
        "an unreachable socket opens the breaker at once"
    );
    assert_eq!(rig.alert_count("breaker-open"), 1);
    assert_eq!(rig.alert_count("check-unavailable"), 1);

    let ran = run_job(&server, &t2v_job(), "s-down", Some(22), Some(ADDR_B), false).await;
    assert!(
        !ran.accepted,
        "video jobs are rejected before accept while the breaker is open"
    );
    assert_eq!(
        (ran.code(), ran.message()),
        (CHECK_UNAVAILABLE, MSG_BREAKER)
    );

    // the service comes back: health, the pinned bundle, the probe clip and the probe sentence close the breaker
    let back = spawn_service(rig.svc.socket.clone(), rig.dir("svc-view"));
    let mut waited = 0;
    while rig.checks.breaker_open() && waited < 100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        waited += 1;
    }
    assert!(
        !rig.checks.breaker_open(),
        "the breaker closed after the probe passed"
    );
    assert_eq!(
        (back.count("moderate mp4"), back.count("probe-text")),
        (1, 1),
        "it was the probe that closed it"
    );
    assert_eq!(rig.alert_count("breaker-closed"), 1);
    for (kind, detail) in rig.alerts.all() {
        assert!(
            !detail.contains(PROMPT) && !detail.contains("staging"),
            "{kind}: {detail}"
        );
    }
}

#[tokio::test]
async fn the_probe_needs_health_the_pinned_bundle_a_passing_clip_and_a_passing_sentence() {
    // drop the profile or hash comparison, accept any probe verdict, or skip the text probe -> red
    let rig = rig_with(Mode::Enforce, false, |_| {}).await;
    *rig.svc.bundle_hash.lock().unwrap() = "00".repeat(32);
    assert!(!rig.checks.probe_now().await, "another bundle");
    assert_eq!(
        rig.svc.count("moderate mp4"),
        0,
        "no clip is sent to a service on the wrong bundle"
    );
    *rig.svc.bundle_hash.lock().unwrap() = BUNDLE.to_string();
    *rig.svc.profile.lock().unwrap() = "transcode".to_string();
    assert!(!rig.checks.probe_now().await, "the transcode profile");
    *rig.svc.profile.lock().unwrap() = "generation".to_string();
    rig.svc.set_media(
        "mp4",
        0,
        Reply::Verdict("BLOCK_POLICY", Some("sexually_explicit")),
    );
    assert!(
        !rig.checks.probe_now().await,
        "a probe clip that does not pass"
    );
    rig.svc
        .set_media("mp4", 0, Reply::Verdict("PASS_RATED", None));
    assert!(!rig.checks.probe_now().await, "PASS_RATED is not a pass");
    // a media answer that names no bundle is not an answer from the pinned bundle
    let unnamed = serde_json::json!({"verdict": "PASS", "category": null,
        "stats": {"framesSampled": 24, "maxScore": 0.01, "escalated": 0}});
    rig.svc.set_media("mp4", 0, Reply::Raw(200, unnamed));
    assert!(
        !rig.checks.probe_now().await,
        "no bundle hash in the answer"
    );
    *rig.svc.media_rule.lock().unwrap() = None;
    // the text check is part of being ready: without it every job would fail at its prompt
    let missing = serde_json::json!({"error": {"kind": "TEXT_CHECK_NOT_CONFIGURED", "detail": "no text model"}});
    rig.svc.set_text(Reply::Raw(404, missing));
    assert!(!rig.checks.probe_now().await, "no text check");
    assert!(rig.checks.breaker_open());
    rig.svc.set_text(Reply::Pass);
    assert!(rig.checks.probe_now().await);
    assert!(rig.files_in("staging").is_empty());
}

#[tokio::test]
async fn the_version_endpoint_reports_mode_bundle_and_breaker() {
    // contentChecks missing from /v1/version, or a breaker state that never moves -> red
    let rig = rig_with(Mode::Enforce, false, |_| {}).await;
    let (_r, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
    let server = server_for(client).await;
    let v = fabstir_llm_node::api::server::version_body(&server);
    assert_eq!(v["contentChecks"]["mode"], "off");
    assert_eq!(v["version"], fabstir_llm_node::version::VERSION_NUMBER);
    server.set_content_checks(rig.checks.clone());
    let v = fabstir_llm_node::api::server::version_body(&server);
    assert_eq!(
        v["contentChecks"],
        serde_json::json!({"mode": "enforce", "bundleHash": format!("sha256:{BUNDLE}"), "breaker": "open"})
    );
    rig.checks.probe_now().await;
    let v = fabstir_llm_node::api::server::version_body(&server);
    assert_eq!(v["contentChecks"]["breaker"], "closed");
}

#[tokio::test]
async fn shadow_only_logs_then_rejects_after_its_end_date() {
    // shadow refusing -> CONTENT_REFUSED instead of the render's own ending -> red; shadow sealing, recording or
    // alerting on a verdict -> red; no expiry -> accepted -> red; an alert on every rejected job -> count 2 -> red
    let (logs, _capture) = Logs::capture();
    let rig = rig(Mode::Shadow).await;
    let (renderer, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
    let server = server_for(client).await;
    server.set_content_checks(rig.checks.clone());
    // the strongest signal: in `enforce` this seals, records, counts and alerts
    rig.svc
        .set_text(Reply::Verdict("BLOCK_ILLEGAL", Some("csam_suspected")));
    let ran = run_job(
        &server,
        &t2v_job(),
        "s-shadow",
        Some(31),
        Some(ADDR_A),
        false,
    )
    .await;
    assert_eq!(
        ran.message(),
        "no frames produced",
        "the job ran on to the stub's empty render"
    );
    assert_eq!(renderer.count("prompt"), 1);
    assert_eq!(rig.svc.count("moderate-text"), 1, "the check did run");
    assert!(
        logs.has("c1", "refused=BLOCK_ILLEGAL") && logs.has("c1", "shadow=true"),
        "the verdict is in the log"
    );
    assert!(
        rig.files_in("evidence").is_empty(),
        "nothing sealed, no refusals file: {:?}",
        rig.files_in("evidence")
    );
    assert!(rig.alerts.all().is_empty(), "{:?}", rig.alerts.all());
    assert!(
        !rig.checks.cooldown().paused(ADDR_A),
        "shadow counts nothing"
    );

    let ended = rig_with(Mode::Shadow, true, |c| {
        c.shadow_until = Some(chrono::Utc::now().date_naive() - chrono::Duration::days(1))
    })
    .await;
    server.set_content_checks(ended.checks.clone());
    for jid in [32, 33] {
        let ran = run_job(
            &server,
            &t2v_job(),
            &format!("s-shadow-{jid}"),
            Some(jid),
            Some(ADDR_A),
            false,
        )
        .await;
        assert!(!ran.accepted);
        assert_eq!(
            (ran.code(), ran.message()),
            (CHECK_UNAVAILABLE, MSG_SHADOW_ENDED)
        );
    }
    assert_eq!(
        ended.alert_count("shadow-ended"),
        1,
        "the operator is told once an hour, not once a job"
    );
}

#[tokio::test]
async fn with_off_no_check_code_runs() {
    // invariant 5. `checks_on` forced true -> a "checking" stage appears -> red
    let _env = super::ltx_task_support::S5_ENV_LOCK.lock().await;
    let tmp = tempfile::tempdir().unwrap();
    let (input, output) = (tmp.path().join("in"), tmp.path().join("out"));
    let (renderer, client) = spawn_renderer(input.clone(), output).await;
    let server = server_for(client).await;
    assert_eq!(
        server.content_checks().mode(),
        Mode::Off,
        "a server is `off` unless main says otherwise"
    );
    assert_eq!(ContentChecks::off().mode(), Mode::Off);
    assert_eq!(ContentChecks::off().max_check_secs(), 0);
    // an image the checks could never read is uploaded untouched, exactly as before this release
    let cids = serve_blobs(&[b"not an image at all".to_vec()]).await;
    let ran = run_job(
        &server,
        &still_job(&cids[0]),
        "s-off",
        Some(41),
        Some(ADDR_A),
        false,
    )
    .await;
    assert_eq!(ran.message(), "no frames produced", "{:?}", ran.terminal);
    assert_eq!(renderer.count("upload"), 1);
    assert!(
        !ran.stages.iter().any(|s| s == "checking"),
        "no checking stage: {:?}",
        ran.stages
    );
    assert_eq!(
        std::fs::read_dir(&input).unwrap().count(),
        1,
        "no folder is touched without the setting"
    );
    assert!(server
        .content_checks()
        .pre_accept(Some(ADDR_A), "s-off")
        .is_ok());
}
