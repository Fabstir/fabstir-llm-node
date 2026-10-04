// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! NM1 D23 through the HANDLER: a control clip whose audio cannot pass through reaches `/prompt`
//! without the pass-through link, a clip with mono/stereo audio keeps it, and an unreadable audio walk
//! drops the audio rather than refusing the job. One test function, one blob server, holding
//! `S5_ENV_LOCK` (ENHANCED_S5_URL is process-global).
//!
//! Mutations that turn it red: remove the `drop_silent_audio` call; call it before `patch` (the
//! template's `control.mp4` never equals the stub's staged name); invert the flag; use the REQUESTED
//! name instead of the one ComfyUI answered (`staged`); propagate the walk error with `?` (the overrun
//! clip is refused before `/prompt`); `unwrap_or(true)` (the overrun clip keeps `audio`).

use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::api::websocket::handlers::ltx::handle_encrypted_ltx_generate;
use fabstir_llm_node::ltx::{ComfyClient, TemplateStore};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

fn key() -> [u8; 32] {
    [0x23; 32]
}

fn decrypt_envelope(resp: &Value, session_key: &[u8; 32]) -> Value {
    let p = &resp["payload"];
    let ct = hex::decode(p["ciphertextHex"].as_str().unwrap()).unwrap();
    let nb = hex::decode(p["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(p["aadHex"].as_str().unwrap()).unwrap();
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&nb);
    let pt = fabstir_llm_node::crypto::decrypt_with_aead(&ct, &nonce, &aad, session_key).unwrap();
    serde_json::from_slice(&pt).unwrap()
}

fn boxed(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = ((8 + payload.len()) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(typ);
    out.extend_from_slice(payload);
    out
}

fn trak(handler: &[u8; 4], stbl: &[u8]) -> Vec<u8> {
    let mut hd = vec![0u8; 8];
    hd.extend_from_slice(handler);
    hd.extend_from_slice(&[0u8; 13]);
    let mut mdia = boxed(b"hdlr", &hd);
    mdia.extend_from_slice(&boxed(b"minf", &boxed(b"stbl", stbl)));
    boxed(b"trak", &boxed(b"mdia", &mdia))
}

fn stsz(n: u32) -> Vec<u8> {
    let mut sz = vec![0u8; 8];
    sz.extend_from_slice(&n.to_be_bytes());
    boxed(b"stsz", &sz)
}

fn audio_trak(channels: u16) -> Vec<u8> {
    let mut e = vec![0u8; 6];
    e.extend_from_slice(&1u16.to_be_bytes());
    e.extend_from_slice(&[0u8; 8]);
    e.extend_from_slice(&channels.to_be_bytes());
    e.extend_from_slice(&[0u8; 8]);
    let mut stsd = vec![0u8; 4];
    stsd.extend_from_slice(&1u32.to_be_bytes());
    stsd.extend_from_slice(&boxed(b"mp4a", &e));
    let mut stbl = boxed(b"stsd", &stsd);
    stbl.extend_from_slice(&stsz(237));
    trak(b"soun", &stbl)
}

/// A 121-frame clip (edit is exactControl at 24 fps: 121 billed) with the given extra traks.
fn clip(extra: &[Vec<u8>]) -> Vec<u8> {
    let mut moov = trak(b"vide", &stsz(121));
    for t in extra {
        moov.extend_from_slice(t);
    }
    let mut out = boxed(b"ftyp", b"isom\x00\x00\x02\x00isomiso2");
    out.extend_from_slice(&boxed(b"moov", &moov));
    out
}

/// A clip that passes the control gate (its vide trak comes first) but whose NEXT box overruns moov.
fn overrun_clip() -> Vec<u8> {
    let mut bad = boxed(b"free", &[0u8; 8]);
    bad[0..4].copy_from_slice(&1000u32.to_be_bytes());
    clip(&[bad])
}

fn cap_cid(plaintext: &[u8]) -> (String, Vec<u8>, String) {
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

#[tokio::test]
async fn test_handler_drops_audio_only_for_clips_whose_audio_cannot_pass() {
    use axum::Router;
    use tokio::sync::mpsc;

    let _env = super::ltx_task_support::S5_ENV_LOCK.lock().await;

    let cases: Vec<(&str, Vec<u8>, bool)> = vec![
        ("silent", clip(&[]), false),
        ("stereo", clip(&[audio_trak(2)]), true),
        ("quad", clip(&[audio_trak(4)]), false),
        ("overrun", overrun_clip(), false),
    ];
    let blobs: Vec<(String, Vec<u8>, String)> =
        cases.iter().map(|(_, bytes, _)| cap_cid(bytes)).collect();
    let served: Vec<(String, Vec<u8>)> = blobs
        .iter()
        .map(|(_, ct, path)| (path.clone(), ct.clone()))
        .collect();
    let blob_app = Router::new().fallback(move |uri: axum::http::Uri| {
        let body = served
            .iter()
            .find(|(p, _)| p == uri.path())
            .map(|(_, ct)| ct.clone())
            .unwrap_or_default();
        async move { body }
    });
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let blob_addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, blob_app).await.unwrap() });
    std::env::set_var("ENHANCED_S5_URL", format!("http://{blob_addr}"));

    // A stub ComfyUI: every upload is stored as "staged" (≠ the template's control.mp4 and ≠ the
    // requested content name), and every /prompt body is recorded, then failed.
    let prompts: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = prompts.clone();
    let comfy_app = Router::new()
        .route(
            "/upload/image",
            axum::routing::post(|| async {
                axum::Json(json!({"name": "staged", "subfolder": "", "type": "input"}))
            }),
        )
        .route(
            "/prompt",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                seen.lock().unwrap().push(body);
                async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "stub") }
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let comfy_addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, comfy_app).await.unwrap() });
    let stub = Arc::new(ComfyClient::new(&format!("http://{comfy_addr}")).unwrap());

    let server = Arc::new(ApiServer::new_for_test());
    let k = key();
    let store = TemplateStore::new(concat!(env!("CARGO_MANIFEST_DIR"), "/templates")).unwrap();
    let hash = store.template_hash("ltx-edit-hdr").unwrap().to_string();
    server.set_ltx_client(stub.clone()).await;
    server.set_ltx_template_store(Arc::new(store)).await;

    for ((name, _, want_audio), (cid, _, _)) in cases.iter().zip(&blobs) {
        let before = prompts.lock().unwrap().len();
        let job = json!({
            "action": "ltx_generate",
            "requestId": format!("r-{name}"),
            "templateId": "ltx-edit-hdr",
            "templateHash": hash,
            "prompt": "make it rain",
            "seed": "42",
            "frames": 121,
            "fps": 24,
            "resolution": { "w": 768, "h": 512 },
            "lora": "ltx-edit-hdr@v1",
            "output": "exr-sequence",
            "videos": [cid]
        });
        let sid = format!("sess-{name}");
        let (resp, task) = handle_encrypted_ltx_generate(&server, &job, &k, &sid, None, None).await;
        let task =
            task.unwrap_or_else(|| panic!("{name}: accepted: {:?}", decrypt_envelope(&resp, &k)));
        let (tx, mut rx) = mpsc::channel::<Value>(16);
        task.run(stub.clone(), k, sid, server.clone(), tx).await;
        let mut frames = Vec::new();
        while let Ok(Some(raw)) =
            tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv()).await
        {
            frames.push(decrypt_envelope(&raw, &k));
        }
        let got = prompts.lock().unwrap();
        assert_eq!(
            got.len(),
            before + 1,
            "{name}: reached /prompt exactly once: {frames:?}"
        );
        let graph = &got[before]["prompt"];
        assert_eq!(
            graph["10"]["inputs"]["video"],
            json!("staged"),
            "{name}: patched with the staged name"
        );
        let has_audio = graph["80"]["inputs"].get("audio").is_some();
        assert_eq!(
            has_audio, *want_audio,
            "{name}: VideoCombine audio present = {has_audio}"
        );
    }
}
