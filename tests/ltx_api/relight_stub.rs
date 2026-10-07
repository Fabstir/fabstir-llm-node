// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! VFX Passes (VP1.1): stub sidecars for the relight route and admission tests. The relight stub and the LTX (ComfyUI)
//! stub share ONE ordered call log, so a test can assert what reached which sidecar and in what order. Every behaviour
//! is state-driven (atomics read per request), never poll-counted. `mp4_clip` and `cap_cid` are copies of the private
//! helpers in `test_ws.rs`.
#![allow(dead_code)]

use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::ltx::relight::{AdmitCfg, RelightPins};
use fabstir_llm_node::ltx::{ComfyClient, TemplateStore};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

pub type Log = Arc<Mutex<Vec<String>>>;

pub const GIB: u64 = 1 << 30;

pub struct Stub {
    pub name: &'static str,
    pub log: Log,
    pub busy: AtomicBool,
    pub vram_free: AtomicU64,
    /// From the first `/upload/image` on, report this much free VRAM instead (0 = off).
    pub vram_after_upload: AtomicU64,
    pub uploaded: AtomicBool,
    pub stats_503: AtomicBool,
    pub stats_calls: AtomicUsize,
    pub pins: Mutex<Value>,
    /// `/free` answers this status after `free_delay_ms`.
    pub free_status: AtomicU16,
    pub free_delay_ms: AtomicU64,
    pub queue_busy: AtomicBool,
    /// The `/ws` terminal: `Some(msg)` = `execution_error` with that text, `None` = success.
    pub ws_error: Mutex<Option<String>>,
    pub finish_after_ms: AtomicU64,
}

impl Stub {
    fn rec(&self, what: &str) {
        self.log
            .lock()
            .unwrap()
            .push(format!("{} {what}", self.name));
    }
    pub fn calls(&self, what: &str) -> usize {
        let needle = format!("{} {what}", self.name);
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with(&needle))
            .count()
    }
}

pub fn pins_json() -> Value {
    json!({"weights": {"Diffusion_Renderer_Inverse_Cosmos_7B/model.pt": "aa"}, "stack": "stack-1"})
}

pub fn pins() -> RelightPins {
    serde_json::from_value(pins_json()).unwrap()
}

/// 3 s budget, 0.1 s interval (IMPLEMENTATION VP1.1); never via process env.
pub fn test_cfg() -> AdmitCfg {
    AdmitCfg {
        budget: Duration::from_secs(3),
        interval: Duration::from_millis(100),
        min_free_vram: 30 * GIB,
        deadline_secs: 2700,
        watch_timeout_secs: 2100,
        comfy_handshake: false,
    }
}

async fn stats(State(s): State<Arc<Stub>>) -> impl IntoResponse {
    s.stats_calls.fetch_add(1, Ordering::SeqCst);
    if s.stats_503.load(Ordering::SeqCst) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "hashing weights"})),
        );
    }
    let after = s.vram_after_upload.load(Ordering::SeqCst);
    let free = if after > 0 && s.uploaded.load(Ordering::SeqCst) {
        after
    } else {
        s.vram_free.load(Ordering::SeqCst)
    };
    let mut body = json!({"system": {"os": "posix"}, "devices": [{"name": "stub", "vram_total": 96 * GIB, "vram_free": free}]});
    if s.name == "relight" {
        body["relight"] = json!({"busy": s.busy.load(Ordering::SeqCst), "loaded": false,
                                 "pins": s.pins.lock().unwrap().clone(), "gpu": "RTX PRO 6000", "cuda": "12.8"});
    }
    (StatusCode::OK, Json(body))
}

async fn upload(State(s): State<Arc<Stub>>) -> Json<Value> {
    s.rec("/upload/image");
    s.uploaded.store(true, Ordering::SeqCst);
    Json(json!({"name": "staged.mp4", "subfolder": "", "type": "input"}))
}

async fn prompt(State(s): State<Arc<Stub>>) -> Json<Value> {
    s.rec("/prompt");
    Json(json!({"prompt_id": "p1", "number": 0, "node_errors": {}}))
}

async fn interrupt(State(s): State<Arc<Stub>>) -> Json<Value> {
    s.rec("/interrupt");
    Json(json!({}))
}

async fn free(State(s): State<Arc<Stub>>, body: String) -> impl IntoResponse {
    s.rec(&format!("/free {}", body.trim()));
    tokio::time::sleep(Duration::from_millis(
        s.free_delay_ms.load(Ordering::SeqCst),
    ))
    .await;
    StatusCode::from_u16(s.free_status.load(Ordering::SeqCst)).unwrap()
}

async fn queue(State(s): State<Arc<Stub>>) -> Json<Value> {
    s.rec("/queue");
    let running: Vec<Value> = if s.queue_busy.load(Ordering::SeqCst) {
        vec![json!([0, "x"])]
    } else {
        vec![]
    };
    Json(json!({"queue_running": running, "queue_pending": []}))
}

async fn history(State(s): State<Arc<Stub>>) -> Json<Value> {
    s.rec("/history");
    Json(json!({}))
}

async fn ws(State(s): State<Arc<Stub>>, up: WebSocketUpgrade) -> impl IntoResponse {
    s.rec("/ws");
    up.on_upgrade(move |mut sock| async move {
        tokio::time::sleep(Duration::from_millis(s.finish_after_ms.load(Ordering::SeqCst))).await;
        let frame = match s.ws_error.lock().unwrap().clone() {
            Some(msg) => json!({"type": "execution_error", "data": {"prompt_id": "p1", "exception_message": msg}}),
            None => json!({"type": "executing", "data": {"node": null, "prompt_id": "p1"}}),
        };
        let _ = sock.send(Message::Text(frame.to_string())).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    })
}

/// Start one stub sidecar; returns its state and a client pointed at it.
pub async fn spawn_stub(name: &'static str, log: Log) -> (Arc<Stub>, Arc<ComfyClient>) {
    let s = Arc::new(Stub {
        name,
        log,
        busy: AtomicBool::new(false),
        vram_free: AtomicU64::new(60 * GIB),
        vram_after_upload: AtomicU64::new(0),
        uploaded: AtomicBool::new(false),
        stats_503: AtomicBool::new(false),
        stats_calls: AtomicUsize::new(0),
        pins: Mutex::new(pins_json()),
        free_status: AtomicU16::new(200),
        free_delay_ms: AtomicU64::new(0),
        queue_busy: AtomicBool::new(false),
        ws_error: Mutex::new(None),
        finish_after_ms: AtomicU64::new(0),
    });
    let app = Router::new()
        .route("/system_stats", get(stats))
        .route("/upload/image", post(upload))
        .route("/prompt", post(prompt))
        .route("/interrupt", post(interrupt))
        .route("/free", post(free))
        .route("/queue", get(queue))
        .route("/history/:id", get(history))
        .route("/ws", get(ws))
        .with_state(s.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (
        s,
        Arc::new(ComfyClient::new(&format!("http://{addr}")).unwrap()),
    )
}

/// A client whose endpoint refuses connections.
pub fn dead_client() -> Arc<ComfyClient> {
    Arc::new(ComfyClient::new("http://127.0.0.1:1").unwrap())
}

/// A minimal ISO BMFF clip whose one video track claims `samples` frames (copy of `test_ws.rs`).
pub fn mp4_clip(samples: u32) -> Vec<u8> {
    fn boxed(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((8 + payload.len()) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(typ);
        out.extend_from_slice(payload);
        out
    }
    let mut hd = vec![0u8; 8];
    hd.extend_from_slice(b"vide");
    hd.extend_from_slice(&[0u8; 13]);
    let mut sz = vec![0u8; 8];
    sz.extend_from_slice(&samples.to_be_bytes());
    let stbl = boxed(b"stbl", &boxed(b"stsz", &sz));
    let mut mdia = boxed(b"hdlr", &hd);
    mdia.extend_from_slice(&boxed(b"minf", &stbl));
    let trak = boxed(b"trak", &boxed(b"mdia", &mdia));
    let mut out = boxed(b"ftyp", b"isom\x00\x00\x02\x00isomiso2");
    out.extend_from_slice(&boxed(b"moov", &trak));
    out
}

/// A real capability CID for `plaintext`, plus the ciphertext and the blob path (copy of `test_ws.rs`).
pub fn cap_cid(plaintext: &[u8]) -> (String, Vec<u8>, String) {
    use fabstir_llm_node::ltx::exr::{capability_cid, encrypt_frame, padding_for};
    use fabstir_llm_node::ltx::input_image::{blob_download_cid, parse_capability_cid};
    let key = [0x24u8; 32];
    let ct = encrypt_frame(plaintext, &key).unwrap();
    let cid = capability_cid(plaintext, &ct, &key, padding_for(plaintext.len()) as u32);
    let path = format!(
        "/s5/blob/{}",
        blob_download_cid(&parse_capability_cid(&cid).unwrap().ct_hash)
    );
    (cid, ct, path)
}

/// Serve one clip's ciphertext as the S5 blob store and point `ENHANCED_S5_URL` at it. The caller must hold
/// `S5_ENV_LOCK` for as long as the variable must stay its own. Returns the clip's capability CID.
pub async fn serve_clip(frames: u32) -> String {
    let (cid, ct, path) = cap_cid(&mp4_clip(frames));
    let blobs = Router::new().fallback(move |uri: axum::http::Uri| {
        let body = if uri.path() == path {
            ct.clone()
        } else {
            Vec::new()
        };
        async move { body }
    });
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, blobs).await.unwrap() });
    std::env::set_var("ENHANCED_S5_URL", format!("http://{addr}"));
    cid
}

pub fn key() -> [u8; 32] {
    [0x33; 32]
}

pub fn store() -> Arc<TemplateStore> {
    Arc::new(TemplateStore::new(concat!(env!("CARGO_MANIFEST_DIR"), "/templates")).unwrap())
}

pub fn decrypt(resp: &Value) -> Value {
    let p = &resp["payload"];
    let ct = hex::decode(p["ciphertextHex"].as_str().unwrap()).unwrap();
    let nb = hex::decode(p["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(p["aadHex"].as_str().unwrap()).unwrap();
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&nb);
    let pt = fabstir_llm_node::crypto::decrypt_with_aead(&ct, &nonce, &aad, &key()).unwrap();
    serde_json::from_slice(&pt).unwrap()
}

/// A relight job on the wire (`cosmos-passes-std`, 1920×1088, 25 fps).
pub fn relight_job(store: &TemplateStore, frames: u32, video_cid: &str) -> Value {
    json!({
        "action": "ltx_generate",
        "requestId": "r-relight",
        "templateId": "cosmos-passes-std",
        "templateHash": store.template_hash("cosmos-passes-std").unwrap(),
        "prompt": "",
        "seed": "42",
        "frames": frames,
        "fps": 25,
        "resolution": { "w": 1920, "h": 1088 },
        "lora": "cosmos-passes-std@v1",
        "output": "exr-frames",
        "videos": [video_cid]
    })
}

/// A plain t2v LTX job on the wire.
pub fn ltx_job(store: &TemplateStore) -> Value {
    json!({
        "action": "ltx_generate",
        "requestId": "r-ltx",
        "templateId": "ltx-t2v-hdr",
        "templateHash": store.template_hash("ltx-t2v-hdr").unwrap(),
        "prompt": "a cat in a hat",
        "seed": "42",
        "frames": 121,
        "fps": 24,
        "resolution": { "w": 1280, "h": 720 },
        "lora": "ltx-iclora-hdr@v1",
        "output": "exr-sequence"
    })
}

/// Accept `job` on `server` and run the task to completion against the client `client_for` picks; returns the
/// terminal frame (the first `ltx_error` or `ltx_complete`).
pub async fn accept_and_run(
    server: &Arc<ApiServer>,
    job: &Value,
    jid: Option<u64>,
    pending: bool,
) -> Value {
    use fabstir_llm_node::api::websocket::handlers::ltx::handle_encrypted_ltx_generate;
    let (resp, task) =
        handle_encrypted_ltx_generate(server, job, &key(), "sess-relight", jid, None).await;
    let mut task = task.unwrap_or_else(|| panic!("accepted: {:?}", decrypt(&resp)));
    task.pending_marked = pending;
    let client = server
        .client_for(&task)
        .await
        .expect("a client for the task");
    let (tx, mut rx) = mpsc::channel::<Value>(256);
    task.run(
        client,
        key(),
        "sess-relight".to_string(),
        server.clone(),
        tx,
    )
    .await;
    loop {
        let raw = tokio::time::timeout(Duration::from_secs(20), rx.recv())
            .await
            .expect("a terminal frame")
            .expect("a terminal frame");
        let inner = decrypt(&raw);
        let t = inner["type"].as_str().unwrap_or_default();
        if t == "ltx_error" || t == "ltx_complete" {
            return inner;
        }
    }
}
