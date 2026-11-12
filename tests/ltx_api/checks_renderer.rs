// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks (N10) test support: a stub renderer (ComfyUI) that really stores what it is sent in an input
//! folder and writes its result into an output folder (the same folders the checker is given, as on host1), a blob
//! server for inputs, the jobs, and one helper that drives a job from accept to its terminal frame.
#![allow(dead_code)]

use super::checks_svc::{Reply, Rig};
use super::ltx_task_support::{mark_pending, pending_count};
use super::relight_stub::{cap_cid, decrypt, key, store};
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::ltx::{ComfyClient, TemplateStore};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

pub struct Renderer {
    /// `upload <sub>/<name>`, `prompt`, `history`, `view`, `free`, `interrupt`.
    pub log: Mutex<Vec<String>>,
    pub input_dir: PathBuf,
    pub output_dir: PathBuf,
    /// The clip this renderer "generates" (None = no output at all).
    pub output: Mutex<Option<Vec<u8>>>,
}

impl Renderer {
    pub fn count(&self, what: &str) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with(what))
            .count()
    }
    fn rec(&self, what: String) {
        self.log.lock().unwrap().push(what);
    }
}

async fn stats() -> Json<Value> {
    Json(
        json!({"system": {"os": "posix", "argv": ["main.py", "--disable-metadata"]},
                "devices": [{"name": "stub", "vram_total": 96u64 << 30, "vram_free": 60u64 << 30}]}),
    )
}

async fn upload(
    State(r): State<Arc<Renderer>>,
    mut form: axum_extra::extract::Multipart,
) -> Json<Value> {
    let (mut name, mut sub, mut bytes) = (String::new(), String::new(), Vec::new());
    while let Ok(Some(field)) = form.next_field().await {
        match field.name().unwrap_or_default() {
            "image" => {
                name = field.file_name().unwrap_or("unnamed").to_string();
                bytes = field.bytes().await.unwrap().to_vec();
            }
            "subfolder" => sub = field.text().await.unwrap(),
            _ => {}
        }
    }
    let dir = r.input_dir.join(&sub);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(&name), bytes).unwrap();
    r.rec(format!("upload {sub}/{name}"));
    Json(json!({"name": name, "subfolder": sub, "type": "input"}))
}

async fn prompt(State(r): State<Arc<Renderer>>) -> Json<Value> {
    r.rec("prompt".into());
    if let Some(clip) = r.output.lock().unwrap().clone() {
        std::fs::create_dir_all(r.output_dir.join("video")).unwrap();
        std::fs::write(r.output_dir.join("video/LTX_00001.mp4"), clip).unwrap();
    }
    Json(json!({"prompt_id": "p1", "number": 0, "node_errors": {}}))
}

async fn history(State(r): State<Arc<Renderer>>) -> Json<Value> {
    r.rec("history".into());
    if r.output.lock().unwrap().is_none() {
        return Json(json!({}));
    }
    Json(json!({"p1": {"outputs": {"75": {"images": [
        {"filename": "LTX_00001.mp4", "subfolder": "video", "type": "output"}]}}}}))
}

async fn view(State(r): State<Arc<Renderer>>) -> Vec<u8> {
    r.rec("view".into());
    r.output.lock().unwrap().clone().unwrap_or_default()
}

async fn ws(up: WebSocketUpgrade) -> impl IntoResponse {
    up.on_upgrade(|mut sock| async move {
        let done = json!({"type": "executing", "data": {"node": null, "prompt_id": "p1"}});
        let _ = sock.send(Message::Text(done.to_string())).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    })
}

/// Start the stub renderer over the two folders; returns its state and a client pointed at it.
pub async fn spawn_renderer(
    input_dir: PathBuf,
    output_dir: PathBuf,
) -> (Arc<Renderer>, Arc<ComfyClient>) {
    let r = Arc::new(Renderer {
        log: Mutex::new(vec![]),
        input_dir,
        output_dir,
        output: Mutex::new(None),
    });
    let note = |what: &'static str| {
        let r = r.clone();
        move || {
            r.rec(what.into());
            async { Json(json!({})) }
        }
    };
    let app = Router::new()
        .route("/system_stats", get(stats))
        .route("/upload/image", post(upload))
        .route("/prompt", post(prompt))
        .route("/history/:id", get(history))
        .route("/view", get(view))
        .route("/ws", get(ws))
        .route("/free", post(note("free")))
        .route("/interrupt", post(note("interrupt")))
        .layer(axum::extract::DefaultBodyLimit::max(64 << 20))
        .with_state(r.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (
        r,
        Arc::new(ComfyClient::new(&format!("http://{addr}")).unwrap()),
    )
}

/// Serve encrypted blobs and point `ENHANCED_S5_URL` at the server. The caller holds `S5_ENV_LOCK`.
async fn serve_triples(triples: Vec<(String, Vec<u8>, String)>) {
    let served: std::collections::HashMap<String, Vec<u8>> = triples
        .into_iter()
        .map(|(_, ct, path)| (path, ct))
        .collect();
    let blobs = Router::new().fallback(move |uri: axum::http::Uri| {
        let body = served.get(uri.path()).cloned().unwrap_or_default();
        async move { body }
    });
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, blobs).await.unwrap() });
    std::env::set_var("ENHANCED_S5_URL", format!("http://{addr}"));
}

/// Serve these plaintexts as S5 blobs; returns one capability CID per item, in order.
pub async fn serve_blobs(items: &[Vec<u8>]) -> Vec<String> {
    let triples: Vec<_> = items.iter().map(|i| cap_cid(i)).collect();
    let cids = triples.iter().map(|t| t.0.clone()).collect();
    serve_triples(triples).await;
    cids
}

/// Serve a deep-EXR input: every frame as its own blob plus the manifest that lists them. Returns the
/// manifest's CID (the job's `videos[0]`).
pub async fn serve_deep(frames: &[Vec<u8>]) -> String {
    let mut triples: Vec<_> = frames.iter().map(|f| cap_cid(f)).collect();
    let cids: Vec<&str> = triples.iter().map(|t| t.0.as_str()).collect();
    let manifest = cap_cid(&serde_json::to_vec(&json!({ "deepFrames": cids })).unwrap());
    let manifest_cid = manifest.0.clone();
    triples.push(manifest);
    serve_triples(triples).await;
    manifest_cid
}

pub const PROMPT: &str = "a lighthouse keeper climbs the stairs at dusk, storm outside";

fn base(st: &TemplateStore, template: &str, frames: u32, fps: u32, w: u32, h: u32) -> Value {
    json!({
        "action": "ltx_generate", "requestId": "r-check", "templateId": template,
        "templateHash": st.template_hash(template).unwrap(), "prompt": PROMPT, "seed": "42",
        "frames": frames, "fps": fps, "resolution": {"w": w, "h": h},
        "lora": format!("{template}@v1"), "output": "exr-sequence"
    })
}

/// Text to video: no inputs.
pub fn t2v_job() -> Value {
    base(&store(), "ltx-t2v-hdr", 121, 24, 1280, 720)
}

/// Image to video: one still.
pub fn still_job(image_cid: &str) -> Value {
    let mut j = base(&store(), "ltx-i2v-hdr", 126, 25, 1280, 720);
    j["images"] = json!([image_cid]);
    j
}

/// Edit: one clip (or, with `deep`, a deep-EXR manifest on the scene-linear wire).
pub fn clip_job(video_cid: &str, deep: bool) -> Value {
    let mut j = base(&store(), "ltx-edit-hdr", 121, 24, 768, 512);
    j["videos"] = json!([video_cid]);
    if deep {
        j["inputWire"] = json!("exrseq-linear");
    }
    j
}

/// A png by its magic (the node and the service both detect stills by their first bytes).
pub fn png_bytes(tag: u8) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
    b.extend_from_slice(&[tag; 64]);
    b
}

/// A small real EXR frame. One bright pixel, at a place that depends on `tag`, makes every frame of a sequence
/// (and its PNG) different from every other.
pub fn exr_frame(tag: u32) -> Vec<u8> {
    use exr::prelude::*;
    let pixels = SpecificChannels::rgb(move |pos: Vec2<usize>| {
        let lit = pos.x() as u32 == tag % 16 && pos.y() as u32 == (tag / 16) % 8;
        (
            if lit { 1.0 } else { 0.1 },
            pos.x() as f32 / 16.0,
            pos.y() as f32 / 8.0,
        )
    });
    let mut c = std::io::Cursor::new(Vec::new());
    Image::from_channels((16, 8), pixels)
        .write()
        .to_buffered(&mut c)
        .unwrap();
    c.into_inner()
}

/// A test server: the stub renderer as the LTX sidecar, the repository bundle, the in-process signer.
pub async fn server_for(client: Arc<ComfyClient>) -> Arc<ApiServer> {
    let s = Arc::new(ApiServer::new_for_test());
    s.set_ltx_client(client).await;
    s.set_ltx_template_store(store()).await;
    super::provenance_support::install_provenance(&s).await;
    s
}

/// What a job did: its terminal frame (an `ltx_error`, whether sent before or after accept, or `ltx_complete`),
/// the progress stages it sent, and whether it was accepted.
pub struct Ran {
    pub terminal: Value,
    pub stages: Vec<String>,
    pub accepted: bool,
}

impl Ran {
    pub fn code(&self) -> &str {
        self.terminal["error"]["code"].as_str().unwrap_or_default()
    }
    pub fn message(&self) -> &str {
        self.terminal["error"]["message"]
            .as_str()
            .unwrap_or_default()
    }
}

/// Accept `job` and run it to its end. `pending` = the test marked this job's proof pending (the handler does so
/// itself only when a checkpoint manager exists).
pub async fn run_job(
    server: &Arc<ApiServer>,
    job: &Value,
    sid: &str,
    jid: Option<u64>,
    client_address: Option<&str>,
    pending: bool,
) -> Ran {
    use fabstir_llm_node::api::websocket::handlers::ltx::handle_encrypted_ltx_generate_from;
    let (resp, task) =
        handle_encrypted_ltx_generate_from(server, job, &key(), sid, jid, None, client_address)
            .await;
    let Some(mut task) = task else {
        return Ran {
            terminal: decrypt(&resp),
            stages: vec![],
            accepted: false,
        };
    };
    task.pending_marked = task.pending_marked || pending;
    let client = server
        .client_for(&task)
        .await
        .expect("a client for the task");
    let (tx, mut rx) = mpsc::channel::<Value>(1024);
    tokio::time::timeout(
        Duration::from_secs(60),
        task.run(client, key(), sid.to_string(), server.clone(), tx),
    )
    .await
    .expect("the job ends: no await in it is unbounded");
    let mut stages = vec![];
    while let Ok(raw) = rx.try_recv() {
        let inner = decrypt(&raw);
        match inner["type"].as_str().unwrap_or_default() {
            "ltx_progress" => stages.push(inner["stage"].as_str().unwrap_or_default().to_string()),
            "ltx_error" | "ltx_complete" => {
                return Ran {
                    terminal: inner,
                    stages,
                    accepted: true,
                }
            }
            _ => {}
        }
    }
    panic!("the job sent no terminal frame (stages: {stages:?})")
}

// ---------------------------------------------------------------------------------- refusal tests: shared

pub const ADDR: &str = "0xc0ffee0000000000000000000000000000000003";
pub const POLICY: Reply = Reply::Verdict("BLOCK_POLICY", Some("sexually_explicit"));

pub async fn setup(rig: &Rig) -> (Arc<Renderer>, Arc<ApiServer>) {
    let (renderer, client) = spawn_renderer(rig.dir("in"), rig.dir("out")).await;
    let server = server_for(client).await;
    server.set_content_checks(rig.checks.clone());
    (renderer, server)
}

/// Run `job` with its proof marked pending, then assert what every refused or unchecked job must show.
pub async fn refused(
    server: &Arc<ApiServer>,
    renderer: &Renderer,
    job: &Value,
    jid: u64,
    code: &str,
    message: &str,
) -> Ran {
    mark_pending(server, jid).await;
    // one session per job: the handler's own rate limit is per session and is not what these tests are about
    let ran = run_job(
        server,
        job,
        &format!("s-refuse-{jid}"),
        Some(jid),
        Some(ADDR),
        true,
    )
    .await;
    assert!(ran.accepted, "{:?}", ran.terminal);
    assert_eq!(
        (ran.code(), ran.message()),
        (code, message),
        "{:?}",
        ran.terminal
    );
    assert_eq!(
        pending_count(server, jid).await,
        0,
        "the pending proof is forfeited: nothing is billed"
    );
    assert_eq!(
        server.ltx_semaphore().available_permits(),
        1,
        "the slot is released"
    );
    assert_eq!(renderer.count("prompt"), 0, "nothing was rendered");
    assert!(
        ran.stages.iter().all(|s| s == "checking"),
        "only the checking stage: {:?}",
        ran.stages
    );
    ran
}
