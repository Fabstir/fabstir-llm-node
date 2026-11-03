// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SCI-C D16 through the real router: a client that stops reading is closed
//! after one unanswered Ping; a client that reads (and so answers Pings) stays.
//! The knobs are set to their 10 s floors, so each test takes ~25 s.

use fabstir_llm_node::api::server::ApiServer;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

async fn serve() -> u16 {
    std::env::set_var("WS_PING_AFTER_SECS", "10");
    std::env::set_var("WS_PING_UNANSWERED_CLOSE_SECS", "10");
    let server = Arc::new(ApiServer::new_for_test());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let router = ApiServer::create_router(server);
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    port
}

fn clear_env() {
    std::env::remove_var("WS_PING_AFTER_SECS");
    std::env::remove_var("WS_PING_UNANSWERED_CLOSE_SECS");
}

#[tokio::test]
async fn a_client_that_stops_reading_is_closed_after_an_unanswered_ping() {
    let port = serve().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws"))
        .await
        .expect("connects");
    let welcome = tokio::time::timeout(Duration::from_secs(5), ws.next()).await;
    assert!(matches!(welcome, Ok(Some(Ok(Message::Text(_))))));
    // Read nothing for 25 s: the node's Ping (at ~10 s) goes unanswered.
    tokio::time::sleep(Duration::from_secs(25)).await;
    clear_env();
    // The node has given up: what is queued ends in a close, then nothing.
    let mut saw_ping = false;
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(frame) = ws.next().await {
            match frame {
                Ok(Message::Ping(_)) => saw_ping = true,
                Ok(Message::Close(_)) | Err(_) => return true,
                Ok(_) => {}
            }
        }
        true
    })
    .await
    .unwrap_or(false);
    assert!(saw_ping, "the node pinged the idle socket");
    assert!(ended, "the node closed the half-open socket");
}

#[tokio::test]
async fn a_client_that_reads_is_kept() {
    let port = serve().await;
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws"))
        .await
        .expect("connects");
    let (mut tx, mut rx) = ws.split();
    // Read continuously, as a browser does: tungstenite answers each Ping.
    let reader = tokio::spawn(async move {
        let mut pings = 0;
        let mut pongs = Vec::new();
        while let Some(frame) = rx.next().await {
            match frame {
                Ok(Message::Ping(_)) => pings += 1,
                Ok(Message::Pong(p)) => pongs.push(p),
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => {}
            }
        }
        (pings, pongs)
    });
    tokio::time::sleep(Duration::from_secs(25)).await;
    clear_env();
    // Still open: the node answers our own Ping on its outer loop.
    tx.send(Message::Ping(b"alive".to_vec())).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    tx.send(Message::Close(None)).await.unwrap();
    let (pings, pongs) = tokio::time::timeout(Duration::from_secs(5), reader)
        .await
        .unwrap()
        .unwrap();
    assert!(pings >= 1, "the idle socket was pinged");
    assert!(
        pongs.iter().any(|p| p == b"alive"),
        "the node still answers after 25 s: the answered Ping kept the socket"
    );
}
