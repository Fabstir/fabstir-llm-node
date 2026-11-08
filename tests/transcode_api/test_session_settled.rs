// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! 8.63.0 `session-settled-notice`: when the node decides to settle a session,
//! every open connection on it is sent one plaintext `session_settled`, and new
//! work on it is refused with `SESSION_SETTLED`. Inits, the result fetch and the
//! cancels pass. The socket rows run through the real router.

use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::api::session_settled::{
    reason, refused_frame, refused_when_settled, settled_frame, SettledSessions,
};
use fabstir_llm_node::crypto::{derive_shared_key, encrypt_with_aead};
use futures::{SinkExt, StreamExt};
use k256::ecdsa::{signature::Signer, SigningKey};
use k256::SecretKey;
use rand::rngs::OsRng;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

// ---- the registry and the pure rules ----

#[test]
fn the_first_reason_is_kept_and_only_the_first_announcement_is_broadcast() {
    let registry = SettledSessions::new();
    let mut rx = registry.subscribe();
    assert_eq!(registry.reason_for(9), None);
    registry.announce(9, reason::WORK_FINISHED);
    registry.announce(9, reason::EXPIRED);
    assert_eq!(registry.reason_for(9), Some(reason::WORK_FINISHED));
    assert_eq!(rx.try_recv().unwrap(), (9, reason::WORK_FINISHED));
    assert!(
        rx.try_recv().is_err(),
        "a repeat announcement is not broadcast"
    );
    assert_eq!(registry.reason_for(10), None, "other jobs are untouched");
}

#[test]
fn the_notice_and_the_refusal_have_the_agreed_shape() {
    assert_eq!(
        settled_frame(Some("s-1"), 1482, reason::LAST_CONNECTION_CLOSED),
        json!({"type": "session_settled", "session_id": "s-1", "job_id": "1482", "reason": "last_connection_closed"})
    );
    let refused = refused_frame(Some("s-1"), Some(&json!("req-7")), 1482);
    assert_eq!(refused["type"], "error");
    assert_eq!(refused["code"], "SESSION_SETTLED");
    assert_eq!(refused["id"], "req-7");
    assert_eq!(refused["session_id"], "s-1");
    assert!(refused_frame(None, None, 1).get("id").is_none());
}

#[test]
fn work_is_refused_and_inits_fetches_and_cancels_pass() {
    for t in ["prompt", "inference", "uploadVectors", "searchVectors"] {
        assert!(refused_when_settled(t, None), "{t}");
    }
    for a in [
        None,
        Some("image_generation"),
        Some("ltx_generate"),
        Some("relight"),
        Some("train"),
    ] {
        assert!(refused_when_settled("encrypted_message", a), "{a:?}");
    }
    for a in [
        "transcode_result",
        "transcode_cancel",
        "ltx_cancel",
        "train_cancel",
        "transcode",
    ] {
        assert!(!refused_when_settled("encrypted_message", Some(a)), "{a}");
    }
    for t in [
        "session_init",
        "encrypted_session_init",
        "stream_cancel",
        "ping",
    ] {
        assert!(!refused_when_settled(t, None), "{t}");
    }
}

// ---- through the router ----

async fn serve(node_priv: Option<[u8; 32]>) -> (Arc<ApiServer>, u16) {
    let mut server = ApiServer::new_for_test();
    if let Some(key) = node_priv {
        server.set_node_private_key_for_test(key);
    }
    let server = Arc::new(server);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = ApiServer::create_router(server.clone());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (server, port)
}

async fn connect(port: u16) -> Ws {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws"))
        .await
        .expect("connects");
    let _welcome = tokio::time::timeout(Duration::from_secs(5), ws.next()).await;
    ws
}

/// The next text frame as JSON, or `None` when nothing arrives in `secs`.
async fn next_json(ws: &mut Ws, secs: u64) -> Option<Value> {
    tokio::time::timeout(Duration::from_secs(secs), async {
        while let Some(Ok(frame)) = ws.next().await {
            if let Message::Text(t) = frame {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    return Some(v);
                }
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}

/// Reads until a frame matching `want` arrives; `None` after `secs` without one.
async fn wait_for(ws: &mut Ws, secs: u64, want: impl Fn(&Value) -> bool) -> Option<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return None;
        }
        let frame = next_json(ws, left.as_secs().max(1)).await?;
        if want(&frame) {
            return Some(frame);
        }
    }
}

fn is_notice(v: &Value) -> bool {
    v["type"] == "session_settled"
}

fn is_settled_refusal(v: &Value) -> bool {
    v["type"] == "error" && v["code"] == "SESSION_SETTLED"
}

async fn plaintext_init(ws: &mut Ws, job: u64) {
    ws.send(Message::Text(
        json!({"type": "session_init", "id": "init", "job_id": job, "session_id": format!("s-{job}"), "chain_id": 84532})
            .to_string(),
    ))
    .await
    .unwrap();
    let ack = wait_for(ws, 5, |v| v["type"] == "session_init_ack").await;
    assert!(ack.is_some(), "the init is acknowledged");
}

#[tokio::test]
async fn an_idle_connection_is_told_once_and_its_new_work_is_refused() {
    let (server, port) = serve(None).await;
    let mut ws = connect(port).await;
    plaintext_init(&mut ws, 7001).await;

    // The socket is idle: the notice must arrive without the client sending.
    server
        .settled_sessions()
        .announce(7001, reason::WORK_FINISHED);
    let notice = wait_for(&mut ws, 5, is_notice)
        .await
        .expect("session_settled arrives");
    assert_eq!(notice["job_id"], "7001");
    assert_eq!(notice["reason"], "work_finished");
    assert_eq!(notice["session_id"], "s-7001");

    // A prompt: SESSION_SETTLED with its id, then the stream_end it waits for.
    ws.send(Message::Text(
        json!({"type": "prompt", "id": "p-1", "prompt": "hello", "session_id": "s-7001"})
            .to_string(),
    ))
    .await
    .unwrap();
    let refused = next_json(&mut ws, 5).await.expect("a reply");
    assert!(is_settled_refusal(&refused), "got {refused}");
    assert_eq!(refused["id"], "p-1");
    let ended = next_json(&mut ws, 5).await.expect("stream_end");
    assert_eq!(ended["type"], "stream_end");
    assert_eq!(ended["id"], "p-1");

    // A vector upload: refused, with no stream_end (it owns no stream).
    ws.send(Message::Text(
        json!({"type": "uploadVectors", "id": "v-1", "session_id": "s-7001", "vectors": []})
            .to_string(),
    ))
    .await
    .unwrap();
    let refused = next_json(&mut ws, 5).await.expect("a reply");
    assert!(is_settled_refusal(&refused), "got {refused}");
    assert_eq!(refused["id"], "v-1");

    // Told once: a repeat, and another job's settlement, send nothing more.
    server.settled_sessions().announce(7001, reason::EXPIRED);
    server.settled_sessions().announce(7999, reason::EXPIRED);
    assert!(
        wait_for(&mut ws, 2, is_notice).await.is_none(),
        "no second notice"
    );
}

#[tokio::test]
async fn an_init_on_an_already_settled_session_succeeds_and_the_notice_follows() {
    let (server, port) = serve(None).await;
    server
        .settled_sessions()
        .announce(7002, reason::LTX_FINISHED);
    let mut ws = connect(port).await;
    plaintext_init(&mut ws, 7002).await;
    let notice = wait_for(&mut ws, 5, is_notice)
        .await
        .expect("session_settled follows the init");
    assert_eq!(notice["job_id"], "7002");
    assert_eq!(notice["reason"], "ltx_finished");
}

#[tokio::test]
async fn a_connection_on_another_session_hears_nothing_and_is_not_refused() {
    let (server, port) = serve(None).await;
    let mut ws = connect(port).await;
    plaintext_init(&mut ws, 7003).await;
    server.settled_sessions().announce(7004, reason::EXPIRED);
    assert!(
        wait_for(&mut ws, 2, is_notice).await.is_none(),
        "not this connection's job"
    );
    ws.send(Message::Text(
        json!({"type": "uploadVectors", "id": "v-2", "session_id": "s-7003", "vectors": []})
            .to_string(),
    ))
    .await
    .unwrap();
    assert!(
        wait_for(&mut ws, 3, is_settled_refusal).await.is_none(),
        "an unsettled session's work is not refused"
    );
}

#[tokio::test]
async fn encrypted_work_is_refused_but_the_result_fetch_and_cancels_pass() {
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv: [u8; 32] = node_secret.to_bytes().into();
    let (server, port) = serve(Some(node_priv)).await;
    let session_key = [0x42u8; 32];

    // The encrypted init, sealed to the node as the SDK does.
    let client_secret = SecretKey::random(&mut OsRng);
    let shared = derive_shared_key(
        node_secret.public_key().to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();
    let inner = json!({
        "jobId": "7005",
        "modelName": "qwen3.8-27b",
        "sessionKey": format!("0x{}", hex::encode(session_key)),
        "pricePerToken": 904,
    });
    let nonce = [5u8; 24];
    let ciphertext = encrypt_with_aead(inner.to_string().as_bytes(), &nonce, b"", &shared).unwrap();
    let signature: k256::ecdsa::Signature = SigningKey::random(&mut OsRng).sign(&ciphertext);
    let mut sig = [0u8; 65];
    sig[..64].copy_from_slice(&signature.to_bytes());
    let mut ws = connect(port).await;
    ws.send(Message::Text(
        json!({
            "type": "encrypted_session_init",
            "id": "init",
            "session_id": "s-7005",
            "payload": {
                "ephPubHex": format!("0x{}", hex::encode(client_secret.public_key().to_sec1_bytes())),
                "ciphertextHex": format!("0x{}", hex::encode(&ciphertext)),
                "nonceHex": format!("0x{}", hex::encode(nonce)),
                "signatureHex": format!("0x{}", hex::encode(sig)),
                "aadHex": "",
            },
        })
        .to_string(),
    ))
    .await
    .unwrap();
    assert!(
        wait_for(&mut ws, 5, |v| v["type"] == "session_init_ack")
            .await
            .is_some(),
        "the encrypted init succeeds"
    );

    server
        .settled_sessions()
        .announce(7005, reason::TRAINING_FINISHED);
    let notice = wait_for(&mut ws, 5, is_notice)
        .await
        .expect("session_settled arrives");
    assert_eq!(notice["job_id"], "7005");
    assert_eq!(notice["reason"], "training_finished");

    let mut counter = 0u8;
    let mut seal = |body: Value, id: &str| {
        counter += 1;
        let nonce = [counter; 24];
        let ct = encrypt_with_aead(body.to_string().as_bytes(), &nonce, b"", &session_key).unwrap();
        json!({
            "type": "encrypted_message",
            "id": id,
            "session_id": "s-7005",
            "payload": {
                "ciphertextHex": hex::encode(ct),
                "nonceHex": hex::encode(nonce),
                "aadHex": "",
            },
        })
        .to_string()
    };

    // An encrypted prompt: refused, then stream_end.
    ws.send(Message::Text(seal(json!({"prompt": "hello"}), "e-1")))
        .await
        .unwrap();
    let refused = next_json(&mut ws, 5).await.expect("a reply");
    assert!(is_settled_refusal(&refused), "got {refused}");
    assert_eq!(refused["id"], "e-1");
    let ended = next_json(&mut ws, 5).await.expect("stream_end");
    assert_eq!(ended["type"], "stream_end");

    // New LTX work: refused.
    ws.send(Message::Text(seal(
        json!({"action": "ltx_generate"}),
        "e-2",
    )))
    .await
    .unwrap();
    let refused = wait_for(&mut ws, 5, |v| v["id"] == "e-2")
        .await
        .expect("a reply");
    assert!(is_settled_refusal(&refused), "got {refused}");

    // The result fetch and a cancel are answered by their own handlers.
    for (action, id) in [("transcode_result", "e-3"), ("transcode_cancel", "e-4")] {
        ws.send(Message::Text(seal(
            json!({"action": action, "taskId": "t-1"}),
            id,
        )))
        .await
        .unwrap();
        let reply = wait_for(&mut ws, 5, |v| v["id"] == id || is_settled_refusal(v))
            .await
            .expect("the handler answers");
        assert!(
            !is_settled_refusal(&reply),
            "{action} must not be refused: {reply}"
        );
    }
}

#[tokio::test]
async fn a_frame_naming_a_settled_job_is_refused_without_an_init() {
    let (server, port) = serve(None).await;
    server.settled_sessions().announce(7006, reason::EXPIRED);
    let mut ws = connect(port).await;
    // No init: the plaintext prompt adopts its wire job (non-vault host).
    ws.send(Message::Text(
        json!({"type": "prompt", "id": "p-6", "prompt": "hello", "job_id": 7006}).to_string(),
    ))
    .await
    .unwrap();
    let refused = wait_for(&mut ws, 5, |v| v["type"] == "error")
        .await
        .expect("a refusal");
    assert!(is_settled_refusal(&refused), "got {refused}");
    assert_eq!(refused["id"], "p-6");
}

#[tokio::test]
async fn an_inference_request_naming_a_settled_job_is_refused() {
    let (server, port) = serve(None).await;
    server.settled_sessions().announce(7008, reason::EXPIRED);
    let mut ws = connect(port).await;
    plaintext_init(&mut ws, 7007).await;
    ws.send(Message::Text(
        json!({"type": "inference", "id": "i-8", "request": {"model": "m", "prompt": "hello", "job_id": 7008, "max_tokens": 4}})
            .to_string(),
    ))
    .await
    .unwrap();
    let refused = wait_for(&mut ws, 5, |v| v["type"] == "error")
        .await
        .expect("a refusal");
    assert!(is_settled_refusal(&refused), "got {refused}");
    assert_eq!(refused["id"], "i-8");
}

/// Settlements of other jobs must not restart an idle socket's liveness clock:
/// a client that stops reading is still closed after one unanswered Ping
/// (floors of 10 s each, as in `test_ws_liveness`).
#[tokio::test]
async fn other_sessions_settling_do_not_keep_a_dead_peer_alive() {
    std::env::set_var("WS_PING_AFTER_SECS", "10");
    std::env::set_var("WS_PING_UNANSWERED_CLOSE_SECS", "10");
    let (server, port) = serve(None).await;
    let mut ws = connect(port).await;
    std::env::remove_var("WS_PING_AFTER_SECS");
    std::env::remove_var("WS_PING_UNANSWERED_CLOSE_SECS");
    plaintext_init(&mut ws, 7009).await;
    // Stop reading (so the Ping goes unanswered) while other jobs settle every 2 s.
    let registry = server.settled_sessions().clone();
    let churn = tokio::spawn(async move {
        for j in 0..15u64 {
            tokio::time::sleep(Duration::from_secs(2)).await;
            registry.announce(80_000 + j, reason::WORK_FINISHED);
        }
    });
    tokio::time::sleep(Duration::from_secs(26)).await;
    churn.abort();
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(frame) = ws.next().await {
            match frame {
                Ok(Message::Close(_)) | Err(_) => return true,
                Ok(_) => {}
            }
        }
        true
    })
    .await
    .unwrap_or(false);
    assert!(
        ended,
        "the half-open socket is closed despite other settlements"
    );
}
