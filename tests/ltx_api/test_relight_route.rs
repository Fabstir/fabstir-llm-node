// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! VFX Passes (VP1.1) D3 + D5: the pre-accept relight rules, the two sidecar gates, the 3XS-Z topology (relight-only
//! node), routing by family (`client_for`), per-family watch timeouts, the start check and the dispatch/main source
//! checks. Every handler refusal test first HOLDS the server's only permit, so a rule moved after the permit answers
//! CAPACITY instead → red. Each test names the mutation that turns it red.

use super::relight_stub::*;
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::api::websocket::handlers::ltx::handle_encrypted_ltx_generate;
use fabstir_llm_node::ltx::relight::AdmitCfg;
use serde_json::Value;
use std::sync::Arc;

async fn relight_only() -> Arc<ApiServer> {
    let s = Arc::new(ApiServer::new_for_test());
    s.set_relight_client(dead_client()).await;
    s.set_ltx_template_store(store()).await;
    s.set_relight_pins(Some(pins())).await;
    s
}

async fn refused(server: &ApiServer, job: &Value) -> Value {
    let (resp, task) =
        handle_encrypted_ltx_generate(server, job, &key(), "sess-route", Some(5), None).await;
    let inner = decrypt(&resp);
    assert!(task.is_none(), "refused: {inner:?}");
    inner
}

fn clip_cid() -> String {
    cap_cid(&mp4_clip(145)).0
}

#[tokio::test]
async fn relight_rules_refuse_before_the_permit() {
    let server = relight_only().await;
    let _held = server.ltx_semaphore().try_acquire_owned().unwrap();
    let st = store();
    let base = relight_job(&st, 145, &clip_cid());
    let cases: Vec<(Value, &str)> = vec![
        (
            {
                let mut j = base.clone();
                j["output"] = "exr-sequence".into();
                j
            },
            "exr-frames",
        ),
        (
            {
                let mut j = base.clone();
                j["prompt"] = "a lit face".into();
                j
            },
            "no prompt",
        ),
        (
            {
                let mut j = base.clone();
                j["frames"] = 126.into();
                j
            },
            "",
        ),
        (
            {
                let mut j = base.clone();
                j["frames"] = 153.into();
                j
            },
            "",
        ),
        (
            {
                let mut j = base.clone();
                j["resolution"] = serde_json::json!({"w": 1536, "h": 1024});
                j
            },
            "exactly 1920x1088",
        ),
        (
            {
                let mut j = base.clone();
                j["fps"] = 48.into();
                j
            },
            "fps 48",
        ),
    ];
    for (job, needle) in cases {
        let inner = refused(&server, &job).await;
        assert_eq!(
            inner["error"]["code"], "VALIDATION_FAILED",
            "{job:?} -> {inner:?}"
        );
        assert!(
            inner["error"]["message"].as_str().unwrap().contains(needle),
            "{inner:?}"
        );
    }
}

#[tokio::test]
async fn the_valid_relight_job_passes_every_rule() {
    // with the permit held, a job that passed every rule answers CAPACITY (rules passed); an over-strict rule -> red
    let server = relight_only().await;
    let _held = server.ltx_semaphore().try_acquire_owned().unwrap();
    let inner = refused(&server, &relight_job(&store(), 145, &clip_cid())).await;
    assert_eq!(inner["error"]["code"], "CAPACITY", "{inner:?}");
}

#[tokio::test]
async fn relight_job_needs_the_relight_client_even_with_ltx_present() {
    // family gate missing -> CAPACITY (rules passed, permit held) instead -> red
    let server = Arc::new(ApiServer::new_for_test());
    server.set_ltx_client(dead_client()).await;
    server.set_ltx_template_store(store()).await;
    let _held = server.ltx_semaphore().try_acquire_owned().unwrap();
    let inner = refused(&server, &relight_job(&store(), 145, &clip_cid())).await;
    assert_eq!(inner["error"]["code"], "SIDECAR_UNAVAILABLE", "{inner:?}");
}

#[tokio::test]
async fn relight_only_node_refuses_ltx_jobs_and_keeps_its_permit() {
    // the 3XS-Z topology: an LTX job on a relight-only node must not take the slot
    let server = relight_only().await;
    let inner = refused(&server, &ltx_job(&store())).await;
    assert_eq!(inner["error"]["code"], "SIDECAR_UNAVAILABLE", "{inner:?}");
    assert_eq!(server.ltx_semaphore().available_permits(), 1, "permit free");
    let (resp, task) = handle_encrypted_ltx_generate(
        &server,
        &relight_job(&store(), 145, &clip_cid()),
        &key(),
        "sess-r2",
        Some(6),
        None,
    )
    .await;
    assert!(
        task.is_some(),
        "a relight job right after is accepted: {:?}",
        decrypt(&resp)
    );
}

#[tokio::test]
async fn client_for_routes_by_family() {
    // always the LTX client -> ptr_eq fails -> red
    let server = Arc::new(ApiServer::new_for_test());
    let (rc, lc) = (dead_client(), dead_client());
    server.set_relight_client(rc.clone()).await;
    server.set_ltx_client(lc.clone()).await;
    server.set_ltx_template_store(store()).await;
    server.set_relight_pins(Some(pins())).await;
    let (_r, rt) = handle_encrypted_ltx_generate(
        &server,
        &relight_job(&store(), 145, &clip_cid()),
        &key(),
        "s1",
        Some(7),
        None,
    )
    .await;
    let rt = rt.expect("relight accepted");
    assert!(Arc::ptr_eq(&server.client_for(&rt).await.unwrap(), &rc));
    drop(rt);
    let (_l, lt) =
        handle_encrypted_ltx_generate(&server, &ltx_job(&store()), &key(), "s2", Some(8), None)
            .await;
    let mut lt = lt.expect("ltx accepted");
    assert!(Arc::ptr_eq(&server.client_for(&lt).await.unwrap(), &lc));
    // relight-only server + an LTX task built as a literal -> None
    let ro = relight_only().await;
    lt.sidecar = None;
    assert!(ro.client_for(&lt).await.is_none());
}

#[tokio::test]
async fn each_family_keeps_its_own_watch_timeout() {
    // one timeout for all -> the relight task carries 1800 -> red
    let server = Arc::new(ApiServer::new_for_test());
    server.set_relight_client(dead_client()).await;
    server.set_ltx_client(dead_client()).await;
    server.set_ltx_template_store(store()).await;
    server.set_relight_pins(Some(pins())).await;
    server
        .set_admit_cfg(AdmitCfg {
            watch_timeout_secs: 1234,
            ..test_cfg()
        })
        .await;
    let (_r, rt) = handle_encrypted_ltx_generate(
        &server,
        &relight_job(&store(), 145, &clip_cid()),
        &key(),
        "t1",
        Some(9),
        None,
    )
    .await;
    let rt = rt.expect("relight accepted");
    assert_eq!(rt.timeout_secs, 1234);
    drop(rt);
    let (_l, lt) =
        handle_encrypted_ltx_generate(&server, &ltx_job(&store()), &key(), "t2", Some(10), None)
            .await;
    let expected: u64 = std::env::var("LTX_JOB_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1800);
    assert_eq!(lt.expect("ltx accepted").timeout_secs, expected);
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn window<'a>(src: &'a str, from: &str, to: &str) -> &'a str {
    let a = src
        .find(from)
        .unwrap_or_else(|| panic!("anchor {from:?} missing"));
    let b = a + src[a..]
        .find(to)
        .unwrap_or_else(|| panic!("anchor {to:?} missing after {from:?}"));
    &src[a..b]
}

#[test]
fn dispatch_routes_spawn_and_cancel_through_client_for() {
    // a cancel wired to get_ltx_client -> red
    let src = squash(include_str!("../../src/api/server.rs"));
    let w = window(
        &src,
        &squash("if let Some(task) = gen_task {"),
        &squash("// Drain progress until the generation task completes."),
    );
    assert!(
        w.contains(&squash("let Some(lc) = server.client_for(&task).await")),
        "dispatch uses client_for"
    );
    assert!(w.contains(&squash("let cancel_lc = lc.clone()")));
    assert!(!w.contains("get_ltx_client()"));
}

#[test]
fn main_refuses_to_start_with_relight_and_two_slots() {
    // `let _ =` on the start check -> red; no startup interrupt/free -> red; store under COMFY_URL only -> red
    let src = squash(include_str!("../../src/main.rs"));
    let at = src
        .find("check_relight_start(")
        .expect("main calls check_relight_start");
    let tail = &src[at..at + 200.min(src.len() - at)];
    assert!(tail.contains(")?;") || tail.contains("return"), "{tail}");
    assert!(src.contains("client.interrupt().await") && src.contains("client.free(None).await"));
    assert!(src.contains(&squash("if comfy_url.is_some() || relight_url.is_some()")));
}

#[test]
fn relight_interrupt_sits_between_catch_unwind_and_the_permit_drop() {
    // interrupt after drop(_permit) (could hit the next job) or unconditional -> red
    let src = squash(include_str!("../../src/api/websocket/handlers/ltx.rs"));
    let w = window(&src, "catch_unwind().await", "drop(_permit)");
    assert!(
        w.contains("submitted_prompt") && w.contains(".interrupt()"),
        "{w}"
    );
    assert!(w.contains("is_relight&&submitted_prompt"));
}
