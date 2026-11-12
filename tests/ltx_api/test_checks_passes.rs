// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks, node 8.65.1 (D5 as amended): a VFX Passes job (the relight route) delivers technical passes
//! computed from an input that the same job has already had checked, so its RESULT is not sent to the check
//! service. Everything else about the job is as before, and every other route keeps its result check. The check
//! service here would refuse ANY result it was shown: only the listed input and the probe clip pass. Each test
//! names the mutation that turns it red.

use super::checks_chain::*;
use super::checks_renderer::{clip_job, exr_frame, run_job, serve_blobs, spawn_renderer, Ran};
use super::checks_svc::*;
use super::ltx_task_support::{pending_count, S5_ENV_LOCK};
use super::relight_stub::{mp4_clip, pins, pins_json, relight_job, store, test_cfg};
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::ltx::checks::config::{Mode, RendererDirs};
use fabstir_llm_node::ltx::checks::{CHECK_UNAVAILABLE, CONTENT_REFUSED, MSG_REFUSED};
use fabstir_llm_node::ltx::provenance::SELF_TEST_MP4;
use fabstir_llm_node::ltx::relight::RelightRoute;
use fabstir_llm_node::ltx::submit::ltx_tokens;
use fabstir_llm_node::ltx::ComfyClient;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

const ADDR: &str = "0xd00d000000000000000000000000000000000006";
const ILLEGAL: Reply = Reply::Verdict("BLOCK_ILLEGAL", Some("csam"));
const FRAMES: u32 = 145;

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// A real clip that is NOT the node's probe clip (the stub spares that one): the same media plus a `free` box.
fn preview() -> Vec<u8> {
    let mut b = SELF_TEST_MP4.to_vec();
    b.extend_from_slice(&[0, 0, 0, 8, b'f', b'r', b'e', b'e']);
    b
}

/// The relight sidecar: stores what it is sent, and "renders" a preview mp4 plus one EXR per billed frame.
struct Passes {
    log: Mutex<Vec<String>>,
    input_dir: PathBuf,
    output_dir: PathBuf,
}

impl Passes {
    fn count(&self, what: &str) -> usize {
        let log = self.log.lock().unwrap();
        log.iter().filter(|l| l.starts_with(what)).count()
    }
    fn outputs() -> Vec<(&'static str, String, Vec<u8>)> {
        let mut out = vec![("video", "PASSES_00001.mp4".to_string(), preview())];
        out.extend((1..=FRAMES).map(|i| ("exr", format!("frame_{i:05}.exr"), exr_frame(i))));
        out
    }
}

async fn passes_stats() -> Json<Value> {
    Json(
        json!({"system": {"os": "posix", "argv": ["main.py", "--disable-metadata"]},
        "devices": [{"name": "stub", "vram_total": 96u64 << 30, "vram_free": 60u64 << 30}],
        "relight": {"busy": false, "loaded": false, "pins": pins_json(), "gpu": "RTX PRO 6000", "cuda": "12.8"}}),
    )
}

async fn passes_upload(
    State(p): State<Arc<Passes>>,
    mut form: axum_extra::extract::Multipart,
) -> Json<Value> {
    let (mut name, mut bytes) = (String::new(), Vec::new());
    while let Ok(Some(field)) = form.next_field().await {
        if field.name() == Some("image") {
            name = field.file_name().unwrap_or("unnamed").to_string();
            bytes = field.bytes().await.unwrap().to_vec();
        }
    }
    std::fs::write(p.input_dir.join(&name), bytes).unwrap();
    p.log.lock().unwrap().push(format!("upload {name}"));
    Json(json!({"name": name, "subfolder": "", "type": "input"}))
}

async fn passes_prompt(State(p): State<Arc<Passes>>) -> Json<Value> {
    p.log.lock().unwrap().push("prompt".into());
    for (sub, name, bytes) in Passes::outputs() {
        std::fs::create_dir_all(p.output_dir.join(sub)).unwrap();
        std::fs::write(p.output_dir.join(sub).join(name), bytes).unwrap();
    }
    Json(json!({"prompt_id": "p1", "number": 0, "node_errors": {}}))
}

async fn passes_history() -> Json<Value> {
    let files: Vec<Value> = Passes::outputs()
        .into_iter()
        .map(|(sub, name, _)| json!({"filename": name, "subfolder": sub, "type": "output"}))
        .collect();
    Json(json!({"p1": {"outputs": {"75": {"images": files}}}}))
}

async fn passes_view(
    State(p): State<Arc<Passes>>,
    Query(q): Query<HashMap<String, String>>,
) -> Vec<u8> {
    p.log.lock().unwrap().push("view".into());
    std::fs::read(p.output_dir.join(&q["subfolder"]).join(&q["filename"])).unwrap_or_default()
}

async fn passes_ws(up: axum::extract::ws::WebSocketUpgrade) -> impl axum::response::IntoResponse {
    up.on_upgrade(|mut sock| async move {
        let done = json!({"type": "executing", "data": {"node": null, "prompt_id": "p1"}});
        let _ = sock
            .send(axum::extract::ws::Message::Text(done.to_string()))
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    })
}

async fn spawn_passes(input_dir: PathBuf, output_dir: PathBuf) -> (Arc<Passes>, Arc<ComfyClient>) {
    let p = Arc::new(Passes {
        log: Mutex::new(vec![]),
        input_dir,
        output_dir,
    });
    let app = Router::new()
        .route("/system_stats", get(passes_stats))
        .route("/upload/image", post(passes_upload))
        .route("/prompt", post(passes_prompt))
        .route("/history/:id", get(passes_history))
        .route("/view", get(passes_view))
        .route("/ws", get(passes_ws))
        .fallback(|| async { Json(json!({})) })
        .layer(axum::extract::DefaultBodyLimit::max(64 << 20))
        .with_state(p.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let client = Arc::new(ComfyClient::new(&format!("http://{addr}")).unwrap());
    (p, client)
}

struct Money {
    chain: Arc<Chain>,
    signer: Arc<Signing>,
}

/// Checks, a signer that really signs, a stub chain whose session is open for `template`, and `input` served as
/// the job's one clip (after the checkpoint manager, which clears `ENHANCED_S5_URL` for its own storage).
async fn money(
    server: &Arc<ApiServer>,
    rig: &Rig,
    template: &str,
    input: &[u8],
) -> (Money, String) {
    server.set_ltx_template_store(store()).await;
    server.set_content_checks(rig.checks.clone());
    let signer = install_signing_provenance(server).await;
    let st = store();
    let model =
        fabstir_llm_node::ltx::template::expected_session_model(st.entry(template), template);
    let (chain, url) = spawn_chain(model).await;
    install_checkpoint_manager(server, &url).await;
    let cid = serve_blobs(&[input.to_vec()]).await.remove(0);
    (Money { chain, signer }, cid)
}

/// A VFX Passes job on the relight route, in `enforce`, whose check service passes only what `spared` lists.
async fn passes_job(spared: Vec<String>, input: &[u8], jid: u64) -> (Rig, Arc<Passes>, Money, Ran) {
    let rig = rig_with(Mode::Enforce, true, |cfg| {
        let tmp = cfg.staging_node.as_ref().unwrap().parent().unwrap();
        let (input, output) = (tmp.join("rin"), tmp.join("rout"));
        std::fs::create_dir_all(&input).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        cfg.relight_dirs = Some(RendererDirs { input, output });
    })
    .await;
    *rig.svc.block_all_but.lock().unwrap() = Some((spared, ILLEGAL));
    let (passes, client) = spawn_passes(rig.dir("rin"), rig.dir("rout")).await;
    let server = Arc::new(ApiServer::new_for_test());
    server.set_relight_client(client).await;
    server.set_admit_cfg(test_cfg()).await;
    server.set_relight_pins(Some(pins())).await;
    let (money, cid) = money(&server, &rig, "cosmos-passes-std", input).await;
    let job = relight_job(&store(), FRAMES, &cid);
    let ran = run_job(&server, &job, "s-passes", Some(jid), Some(ADDR), false).await;
    assert_eq!(pending_count(&server, jid).await, 0);
    assert_eq!(server.ltx_semaphore().available_permits(), 1);
    (rig, passes, money, ran)
}

#[tokio::test]
async fn a_passes_result_is_not_checked_and_the_job_bills_its_estimate() {
    // The relight branch sent to `check_result` like every other route -> CONTENT_REFUSED, nothing billed -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _capture) = Logs::capture();
    let input = mp4_clip(FRAMES);
    let (rig, passes, money, ran) = passes_job(vec![sha(&input)], &input, 601).await;
    assert_eq!(ran.terminal["type"], "ltx_complete", "{:?}", ran.terminal);
    let estimate = ltx_tokens(FRAMES, 1920, 1088);
    assert_eq!(ran.terminal["billing"]["tokens"], estimate);
    assert_eq!(
        money.chain.claimed_tokens(),
        vec![estimate],
        "one proof, claiming exactly the estimate"
    );
    // the service was asked about the INPUT of this job and about nothing else of it
    let seen: Vec<String> = rig.svc.seen("mp4").into_iter().map(|f| f.sha256).collect();
    assert_eq!(seen, vec![sha(&input)], "the input clip, never the preview");
    assert_eq!(rig.svc.count("moderate mp4"), 2, "the probe and the input");
    assert_eq!(
        rig.svc.count("moderate exr") + rig.svc.count("moderate png"),
        0
    );
    assert_eq!(rig.svc.count("moderate-text"), 0, "the prompt is empty");
    assert!(logs.has("c3", "verdict=PASS"), "the input check ran");
    assert!(logs.has("c5", "verdict=SKIPPED_TECHNICAL inputs=1"));
    assert!(!logs.text().contains("stage=c5 refused"));
    assert!(rig.refusals().is_empty());
    // the rest of the job is as before: marked, delivered whole, and its files deleted at job end
    assert_eq!(passes.count("view"), 1 + FRAMES as usize);
    assert!(money.signer.signed.load(Ordering::SeqCst) >= 1, "marked");
    assert_eq!(
        ran.stages.iter().filter(|s| *s == "checking").count(),
        1,
        "the input check only: {:?}",
        ran.stages
    );
    assert!(rig.files_in("rin").is_empty() && rig.files_in("rout").is_empty());
    assert!(rig.files_in("staging").is_empty());
}

#[tokio::test]
async fn a_passes_job_whose_input_is_refused_is_refused() {
    // The skip widened to the whole job (a refused clip let through on the relight route) -> a render -> red
    let _env = S5_ENV_LOCK.lock().await;
    let input = mp4_clip(FRAMES);
    let (rig, passes, money, ran) = passes_job(vec![], &input, 602).await;
    assert_eq!(
        (ran.code(), ran.message()),
        (CONTENT_REFUSED, MSG_REFUSED),
        "{:?}",
        ran.terminal
    );
    assert_eq!(passes.count("upload") + passes.count("prompt"), 0);
    assert!(money.chain.raw_txs.lock().unwrap().is_empty(), "no proof");
    assert_eq!(money.signer.signed.load(Ordering::SeqCst), 0);
    let rec = rig.refusals();
    assert_eq!((rec.len(), &rec[0]["stage"]), (1, &json!("c3")));
    assert_eq!(rig.svc.seen("mp4").len(), 1);
}

#[tokio::test]
async fn a_comfyui_job_with_the_same_service_is_refused_at_its_result() {
    // The skip reachable from the ComfyUI route (the route witness given for any sidecar) -> ltx_complete -> red
    let _env = S5_ENV_LOCK.lock().await;
    let (logs, _capture) = Logs::capture();
    let rig = rig(Mode::Enforce).await;
    let input = mp4_clip(121);
    *rig.svc.block_all_but.lock().unwrap() = Some((vec![sha(&input)], ILLEGAL));
    let (renderer, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
    *renderer.output.lock().unwrap() = Some(preview());
    let server = Arc::new(ApiServer::new_for_test());
    server.set_ltx_client(client).await;
    let (money, cid) = money(&server, &rig, "ltx-edit-hdr", &input).await;
    let ran = run_job(
        &server,
        &clip_job(&cid, false),
        "s-comfy",
        Some(603),
        Some(ADDR),
        false,
    )
    .await;
    assert_eq!(
        (ran.code(), ran.message()),
        (CONTENT_REFUSED, MSG_REFUSED),
        "{:?}",
        ran.terminal
    );
    assert_eq!(
        renderer.count("prompt"),
        1,
        "the render ran: this is the result check"
    );
    let seen: Vec<String> = rig.svc.seen("mp4").into_iter().map(|f| f.sha256).collect();
    assert_eq!(seen, vec![sha(&input), sha(&preview())]);
    let rec = rig.refusals();
    assert_eq!((rec.len(), &rec[0]["stage"]), (1, &json!("c5")));
    assert!(!logs.text().contains("SKIPPED_TECHNICAL"));
    assert!(money.chain.raw_txs.lock().unwrap().is_empty(), "no proof");
    assert_eq!(money.signer.signed.load(Ordering::SeqCst), 0);
    assert_eq!(pending_count(&server, 603).await, 0);
}

#[tokio::test]
async fn the_skip_needs_the_relight_route_and_a_checked_input() {
    // Minting without a checked input -> Ok where CHECK_UNAVAILABLE is asserted -> red
    let (logs, _capture) = Logs::capture();
    assert!(RelightRoute::of(None).is_none() && RelightRoute::of(Some("comfy")).is_none());
    let route = RelightRoute::of(Some("relight")).expect("the relight route");
    let rig = rig(Mode::Enforce).await;
    let mut job = rig.checks.job("s", Some(9), None);
    let r = rig
        .checks
        .result_of_technical_passes(&mut job, &route)
        .await
        .unwrap_err();
    assert_eq!(r.code, CHECK_UNAVAILABLE);
    assert!(logs.has("c5", "verdict=NONE cause=NoCheckedInput"));
    assert!(!rig.checks.breaker_open(), "this job's own affair");
    // a refused input is not a checked input
    rig.svc.set_media("mp4", 1, ILLEGAL);
    rig.checks
        .check_clip(&mut job, &mp4_clip(8))
        .await
        .unwrap_err();
    let again = rig
        .checks
        .result_of_technical_passes(&mut job, &route)
        .await;
    assert_eq!(again.unwrap_err().code, CHECK_UNAVAILABLE);
    *rig.svc.media_rule.lock().unwrap() = None;
    rig.checks.check_clip(&mut job, &mp4_clip(9)).await.unwrap();
    rig.checks
        .result_of_technical_passes(&mut job, &route)
        .await
        .expect("an input of this job passed its check");
    assert!(logs.has("c5", "verdict=SKIPPED_TECHNICAL inputs=1"));
    assert_eq!(
        rig.svc.files.lock().unwrap().len(),
        3,
        "the probe and two clips"
    );
    // the token for this route is minted by one private function, reached only through the route's witness
    let checks = include_str!("../../src/ltx/checks/mod.rs");
    assert_eq!(checks.matches("technical_passes_input_checked(").count(), 2);
    assert!(!checks.contains("pub fn technical_passes_input_checked"));
    let handler: String = include_str!("../../src/api/websocket/handlers/ltx.rs")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(handler
        .contains("letrelight_route=crate::ltx::relight::RelightRoute::of(sidecar.as_deref());"));
    assert_eq!(handler.matches("result_of_technical_passes(").count(), 1);
    assert!(handler.contains("letoutcome=matchrelight_route.as_ref(){"));
    assert!(
        handler.contains("Some(route)=>{checks.result_of_technical_passes(&mutcjob,route).await}")
    );
}
