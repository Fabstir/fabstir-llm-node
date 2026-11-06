// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! 8.62.3: chat streaming writes are bounded (the one gap the SCI-C handoff left
//! open). The streaming loops awaited `ws_sender.send(chunk)` OUTSIDE their
//! `select!`, so a client that held the socket but stopped reading parked the
//! write once the TCP buffers filled: no liveness deadline or `stream_cancel`
//! could fire, generation stalled on its channel holding the model, and the
//! session waited for the expiry sweeper. Now every chat stream write goes
//! through `send_chat_frame`: a write stuck past CHAT_WS_WRITE_TIMEOUT_SECS (or
//! failed) stops generation and marks the connection dead, so the normal
//! disconnect path settles the session.

use axum::extract::ws::Message;
use fabstir_llm_node::api::websocket::chat_write::{
    parse_chat_ws_write_timeout, send_chat_frame, CHAT_WS_WRITE_TIMEOUT_DEFAULT_SECS,
};
use fabstir_llm_node::api::websocket::liveness::Liveness;
use futures::Sink;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

/// A client that holds the socket but never reads: the write never completes.
struct StalledSink;
impl Sink<Message> for StalledSink {
    type Error = axum::Error;
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }
    fn start_send(self: Pin<&mut Self>, _: Message) -> Result<(), Self::Error> {
        Ok(())
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }
}

/// A healthy client: every frame is accepted at once and counted.
#[derive(Default)]
struct OpenSink(usize);
impl Sink<Message> for OpenSink {
    type Error = axum::Error;
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn start_send(mut self: Pin<&mut Self>, _: Message) -> Result<(), Self::Error> {
        self.0 += 1;
        Ok(())
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

/// A closed socket: the write fails at once.
struct BrokenSink;
impl Sink<Message> for BrokenSink {
    type Error = axum::Error;
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Err(axum::Error::new(std::io::Error::other("closed"))))
    }
    fn start_send(self: Pin<&mut Self>, _: Message) -> Result<(), Self::Error> {
        Ok(())
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

fn liveness() -> Liveness {
    Liveness::new(Duration::from_secs(30), Duration::from_secs(60))
}

#[tokio::test(start_paused = true)]
async fn a_client_that_stops_reading_is_cut_off_and_generation_stops() {
    let mut sink = StalledSink;
    let mut live = liveness();
    let cancel = AtomicBool::new(false);
    let started = tokio::time::Instant::now();
    let ok = send_chat_frame(
        &mut sink,
        Message::Text("chunk".into()),
        Duration::from_secs(CHAT_WS_WRITE_TIMEOUT_DEFAULT_SECS),
        &mut live,
        Some(&cancel),
    )
    .await;
    assert!(!ok, "a parked write must report the client gone");
    assert!(
        live.is_dead(),
        "the connection is marked dead so its loop ends"
    );
    assert!(cancel.load(Ordering::Acquire), "generation is told to stop");
    let waited = started.elapsed().as_secs();
    assert!(
        (CHAT_WS_WRITE_TIMEOUT_DEFAULT_SECS..CHAT_WS_WRITE_TIMEOUT_DEFAULT_SECS + 2)
            .contains(&waited),
        "gave up after {waited}s, not at the bound"
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_write_also_stops_the_stream() {
    let mut live = liveness();
    let cancel = AtomicBool::new(false);
    let ok = send_chat_frame(
        &mut BrokenSink,
        Message::Text("x".into()),
        Duration::from_secs(90),
        &mut live,
        Some(&cancel),
    )
    .await;
    assert!(!ok);
    assert!(live.is_dead());
    assert!(cancel.load(Ordering::Acquire));
}

#[tokio::test(start_paused = true)]
async fn a_healthy_client_streams_normally() {
    let mut sink = OpenSink::default();
    let mut live = liveness();
    let cancel = AtomicBool::new(false);
    for i in 0..50 {
        let ok = send_chat_frame(
            &mut sink,
            Message::Text(format!("c{i}")),
            Duration::from_secs(90),
            &mut live,
            Some(&cancel),
        )
        .await;
        assert!(ok);
    }
    assert_eq!(sink.0, 50);
    assert!(!live.is_dead());
    assert!(!cancel.load(Ordering::Acquire));
}

#[tokio::test(start_paused = true)]
async fn a_stream_without_a_cancel_flag_is_still_cut_off() {
    let mut live = liveness();
    let ok = send_chat_frame(
        &mut StalledSink,
        Message::Text("x".into()),
        Duration::from_secs(5),
        &mut live,
        None,
    )
    .await;
    assert!(!ok);
    assert!(live.is_dead());
}

#[test]
fn test_the_bound_is_finite_and_matches_the_liveness_rule() {
    // 90 s = the liveness rule's 30 s idle Ping plus 60 s unanswered: a client
    // that stops reading is cut off no later than one that stops answering.
    assert_eq!(CHAT_WS_WRITE_TIMEOUT_DEFAULT_SECS, 90);
    assert_eq!(parse_chat_ws_write_timeout(None), Duration::from_secs(90));
    assert_eq!(
        parse_chat_ws_write_timeout(Some("45")),
        Duration::from_secs(45)
    );
    // 0 or garbage never means "unbounded" (or "cut off every chunk").
    for raw in ["0", "", "abc", "-5"] {
        assert_eq!(
            parse_chat_ws_write_timeout(Some(raw)),
            Duration::from_secs(90),
            "{raw:?}"
        );
    }
}
