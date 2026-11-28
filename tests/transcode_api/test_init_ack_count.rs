// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! 8.67.1 `init-ack-connection-count`: every `session_init_ack` carries
//! `other_connections` (the OTHER live connections holding the job) and
//! `work_in_flight`, read from the state the close path reads. 0 and 0 means a
//! close of this connection completes the job. Real router, real sockets.

use fabstir_llm_node::api::job_work::JobState;
use fabstir_llm_node::api::server::ApiServer;
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

async fn ack_frame(ws: &mut Ws) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(frame)) = ws.next().await {
            if let Message::Text(t) = frame {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    if v["type"] == "session_init_ack" {
                        return v;
                    }
                }
            }
        }
        panic!("socket ended without an ack");
    })
    .await
    .expect("an ack arrives")
}

async fn plaintext_init(ws: &mut Ws, job: u64) -> Value {
    ws.send(Message::Text(
        json!({"type": "session_init", "id": "init", "job_id": job, "session_id": format!("s-{job}"), "chain_id": 84532})
            .to_string(),
    ))
    .await
    .unwrap();
    ack_frame(ws).await
}

async fn encrypted_init(ws: &mut Ws, node_secret: &SecretKey, job: u64) -> Value {
    let client_secret = SecretKey::random(&mut OsRng);
    let shared = derive_shared_key(
        node_secret.public_key().to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();
    let inner = json!({
        "jobId": job.to_string(),
        "modelName": "qwen3.8-27b",
        "sessionKey": format!("0x{}", hex::encode([0x42u8; 32])),
        "pricePerToken": 904,
    });
    let nonce = [5u8; 24];
    let ciphertext = encrypt_with_aead(inner.to_string().as_bytes(), &nonce, b"", &shared).unwrap();
    let signature: k256::ecdsa::Signature = SigningKey::random(&mut OsRng).sign(&ciphertext);
    let mut sig = [0u8; 65];
    sig[..64].copy_from_slice(&signature.to_bytes());
    ws.send(Message::Text(
        json!({
            "type": "encrypted_session_init",
            "id": "init",
            "session_id": format!("s-{job}"),
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
    ack_frame(ws).await
}

/// What a probe connection would find on the job right now (the close path's view).
fn state_for_a_closer(server: &ApiServer, job: u64) -> JobState {
    server.job_work().state_excluding(job, "probe")
}

/// Waits until the job's connection count (excluding a probe) reaches `n`.
async fn wait_connections(server: &ApiServer, job: u64, n: usize) {
    for _ in 0..100 {
        if state_for_a_closer(server, job).other_connections == n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("job {job} never reached {n} connection(s)");
}

fn counts(ack: &Value) -> (u64, u64) {
    (
        ack["other_connections"]
            .as_u64()
            .expect("other_connections is an integer"),
        ack["work_in_flight"]
            .as_u64()
            .expect("work_in_flight is an integer"),
    )
}

#[test]
fn the_feature_string_is_advertised() {
    assert!(fabstir_llm_node::version::FEATURES.contains(&"init-ack-connection-count"));
    assert_eq!(fabstir_llm_node::version::VERSION_NUMBER, "8.67.1");
}

#[test]
fn the_close_rule_is_one_function() {
    let s = |w, o| JobState {
        work_in_flight: w,
        other_connections: o,
    };
    assert!(s(0, 0).completes_on_close());
    assert!(!s(1, 0).completes_on_close());
    assert!(!s(0, 1).completes_on_close());
}

#[tokio::test]
async fn one_plaintext_connection_acks_zero_and_zero_and_its_close_completes() {
    let (server, port) = serve(None).await;
    let mut a = connect(port).await;
    let ack = plaintext_init(&mut a, 8101).await;
    assert_eq!(counts(&ack), (0, 0), "{ack}");
    assert_eq!(ack["job_id"], 8101, "existing fields unchanged: {ack}");
    assert_eq!(ack["status"], "success");
    assert_eq!(ack["id"], "init");
    // The close path would find exactly what the ack reported.
    assert!(state_for_a_closer(&server, 8101).other_connections == 1);
    drop(a);
    wait_connections(&server, 8101, 0).await;
    assert!(state_for_a_closer(&server, 8101).completes_on_close());
}

#[tokio::test]
async fn one_encrypted_connection_acks_zero_and_zero_with_exact_field_names() {
    let node_secret = SecretKey::random(&mut OsRng);
    let (server, port) = serve(Some(node_secret.to_bytes().into())).await;
    let mut a = connect(port).await;
    let ack = encrypted_init(&mut a, &node_secret, 8102).await;
    assert_eq!(counts(&ack), (0, 0), "{ack}");
    assert!(ack.get("other_connections").is_some() && ack.get("work_in_flight").is_some());
    assert_eq!(ack["status"], "success");
    drop(a);
    wait_connections(&server, 8102, 0).await;
    assert!(state_for_a_closer(&server, 8102).completes_on_close());
}

#[tokio::test]
async fn a_second_plaintext_connection_sees_one_and_its_close_leaves_the_job_to_the_first() {
    let (server, port) = serve(None).await;
    let mut a = connect(port).await;
    assert_eq!(counts(&plaintext_init(&mut a, 8103).await), (0, 0));
    let mut b = connect(port).await;
    let ack_b = plaintext_init(&mut b, 8103).await;
    assert_eq!(counts(&ack_b), (1, 0), "{ack_b}");
    drop(b);
    wait_connections(&server, 8103, 1).await;
    // B's close found A still holding the job: not a completion. A stays.
    let left = state_for_a_closer(&server, 8103);
    assert_eq!(left.other_connections, 1);
    assert!(!left.completes_on_close());
    drop(a);
    wait_connections(&server, 8103, 0).await;
}

#[tokio::test]
async fn a_second_encrypted_connection_sees_one() {
    let node_secret = SecretKey::random(&mut OsRng);
    let (server, port) = serve(Some(node_secret.to_bytes().into())).await;
    let mut a = connect(port).await;
    assert_eq!(
        counts(&encrypted_init(&mut a, &node_secret, 8104).await),
        (0, 0)
    );
    let mut b = connect(port).await;
    assert_eq!(
        counts(&encrypted_init(&mut b, &node_secret, 8104).await),
        (1, 0)
    );
    // A third connection on ANOTHER job is not counted.
    let mut c = connect(port).await;
    assert_eq!(
        counts(&encrypted_init(&mut c, &node_secret, 8105).await),
        (0, 0)
    );
    wait_connections(&server, 8104, 2).await;
}

#[tokio::test]
async fn work_in_flight_is_reported_in_the_ack() {
    let (server, port) = serve(None).await;
    let guard = server.job_work().begin(8106).expect("work begins");
    let mut a = connect(port).await;
    let ack = plaintext_init(&mut a, 8106).await;
    assert_eq!(counts(&ack), (0, 1), "{ack}");
    assert!(!JobState {
        work_in_flight: 1,
        other_connections: 0
    }
    .completes_on_close());
    drop(guard);
    let mut b = connect(port).await;
    let ack = plaintext_init(&mut b, 8106).await;
    assert_eq!(counts(&ack), (1, 0), "work ended, A still connected: {ack}");
}
