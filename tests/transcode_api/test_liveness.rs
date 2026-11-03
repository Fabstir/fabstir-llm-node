// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SCI-C D16: one liveness rule for every loop that reads a socket. Idle for
//! 30 s ⇒ one Ping; that Ping unanswered for 60 s ⇒ close. Any frame, Pong
//! included, is activity; so is the top of every outer-loop iteration.

use axum::extract::ws::Message;
use fabstir_llm_node::api::websocket::liveness::{
    liveness_action, next_frame, Liveness, LivenessAction,
};
use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::time::Instant;

const PING_AFTER: Duration = Duration::from_secs(30);
const CLOSE_AFTER: Duration = Duration::from_secs(60);

type Inbound = Result<Message, std::io::Error>;

fn pings(rx: &mut mpsc::Receiver<Message>) -> usize {
    let mut n = 0;
    while let Ok(Some(m)) = rx.try_next() {
        if matches!(m, Message::Ping(_)) {
            n += 1;
        }
    }
    n
}

#[tokio::test(start_paused = true)]
async fn the_pure_rule() {
    let t0 = Instant::now();
    let s = Duration::from_secs;
    let act = |now, pinged| liveness_action(now, t0, pinged, PING_AFTER, CLOSE_AFTER);
    assert_eq!(act(t0 + s(29), None), LivenessAction::None);
    assert_eq!(act(t0 + s(30), None), LivenessAction::Ping);
    assert_eq!(act(t0 + s(89), Some(t0 + s(30))), LivenessAction::None);
    assert_eq!(act(t0 + s(90), Some(t0 + s(30))), LivenessAction::Close);
    // Idle is never a reason to close without a Ping first.
    assert_eq!(act(t0 + s(10_000), None), LivenessAction::Ping);
}

#[tokio::test(start_paused = true)]
async fn the_knobs_have_a_floor() {
    let mut l = Liveness::new(Duration::from_secs(1), Duration::from_secs(1));
    let start = Instant::now();
    assert_eq!(l.deadline(), start + Duration::from_secs(10));
    tokio::time::advance(Duration::from_secs(10)).await;
    assert_eq!(l.poll(), LivenessAction::Ping);
    assert_eq!(l.deadline(), start + Duration::from_secs(20));
}

#[tokio::test(start_paused = true)]
async fn a_busy_socket_is_never_pinged() {
    let (mut in_tx, mut in_rx) = mpsc::channel::<Inbound>(8);
    let (mut out_tx, mut out_rx) = mpsc::channel::<Message>(8);
    let mut l = Liveness::new(PING_AFTER, CLOSE_AFTER);
    for _ in 0..5 {
        let mut tx = in_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let _ = tx.send(Ok(Message::Text("hi".into()))).await;
        });
        let frame = next_frame(&mut in_rx, &mut out_tx, &mut l).await;
        assert!(matches!(frame, Some(Ok(Message::Text(_)))));
    }
    assert_eq!(pings(&mut out_rx), 0);
    in_tx.close_channel();
}

#[tokio::test(start_paused = true)]
async fn an_answered_ping_keeps_the_socket() {
    let (mut in_tx, mut in_rx) = mpsc::channel::<Inbound>(8);
    let (mut out_tx, mut out_rx) = mpsc::channel::<Message>(8);
    let mut l = Liveness::new(PING_AFTER, CLOSE_AFTER);
    // The peer answers 5 s after the Ping (sent at +30 s).
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(35)).await;
        let _ = in_tx.send(Ok(Message::Pong(Vec::new()))).await;
        tokio::time::sleep(Duration::from_secs(3_600)).await;
    });
    let frame = next_frame(&mut in_rx, &mut out_tx, &mut l).await;
    assert!(
        matches!(frame, Some(Ok(Message::Pong(_)))),
        "a Pong is a frame"
    );
    assert_eq!(pings(&mut out_rx), 1);
    assert!(!l.is_dead());
    // The Pong reset the clock: the next Ping is 30 s after it, not sooner.
    assert_eq!(l.deadline(), Instant::now() + PING_AFTER);
}

#[tokio::test(start_paused = true)]
async fn an_unanswered_ping_closes_the_half_open_socket() {
    let (_in_tx, mut in_rx) = mpsc::channel::<Inbound>(8);
    let (mut out_tx, mut out_rx) = mpsc::channel::<Message>(8);
    let mut l = Liveness::new(PING_AFTER, CLOSE_AFTER);
    let start = Instant::now();
    let frame = next_frame(&mut in_rx, &mut out_tx, &mut l).await;
    assert!(frame.is_none());
    assert!(l.is_dead());
    assert_eq!(Instant::now() - start, PING_AFTER + CLOSE_AFTER);
    assert_eq!(pings(&mut out_rx), 1, "exactly one Ping before closing");
}

#[tokio::test(start_paused = true)]
async fn a_failed_ping_write_closes_at_once() {
    let (_in_tx, mut in_rx) = mpsc::channel::<Inbound>(8);
    let (mut out_tx, out_rx) = mpsc::channel::<Message>(8);
    drop(out_rx);
    let mut l = Liveness::new(PING_AFTER, CLOSE_AFTER);
    let start = Instant::now();
    assert!(next_frame(&mut in_rx, &mut out_tx, &mut l).await.is_none());
    assert!(l.is_dead());
    assert_eq!(Instant::now() - start, PING_AFTER);
}

#[tokio::test(start_paused = true)]
async fn a_socket_returning_from_a_long_handler_is_not_idle() {
    let (mut in_tx, mut in_rx) = mpsc::channel::<Inbound>(8);
    let (mut out_tx, mut out_rx) = mpsc::channel::<Message>(8);
    let mut l = Liveness::new(PING_AFTER, CLOSE_AFTER);
    // A frame was handled inline for 10 minutes (an LTX render, a training
    // run): nothing touched the clock meanwhile.
    tokio::time::advance(Duration::from_secs(600)).await;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let _ = in_tx.send(Ok(Message::Text("next".into()))).await;
        tokio::time::sleep(Duration::from_secs(3_600)).await;
    });
    let frame = next_frame(&mut in_rx, &mut out_tx, &mut l).await;
    assert!(matches!(frame, Some(Ok(Message::Text(_)))));
    assert_eq!(pings(&mut out_rx), 0, "no Ping right after a long handler");
    assert!(!l.is_dead());
}

#[tokio::test(start_paused = true)]
async fn a_dead_connection_stays_dead() {
    let (mut in_tx, mut in_rx) = mpsc::channel::<Inbound>(8);
    let (mut out_tx, _out_rx) = mpsc::channel::<Message>(8);
    let mut l = Liveness::new(PING_AFTER, CLOSE_AFTER);
    l.mark_dead();
    in_tx.send(Ok(Message::Text("late".into()))).await.unwrap();
    assert!(next_frame(&mut in_rx, &mut out_tx, &mut l).await.is_none());
}

#[tokio::test(start_paused = true)]
async fn writes_defer_the_ping_but_never_answer_one() {
    let mut l = Liveness::new(PING_AFTER, CLOSE_AFTER);
    let start = Instant::now();
    tokio::time::advance(Duration::from_secs(20)).await;
    l.note_write();
    assert_eq!(l.deadline(), start + Duration::from_secs(20) + PING_AFTER);
    tokio::time::advance(PING_AFTER).await;
    assert_eq!(l.poll(), LivenessAction::Ping);
    // Frames keep being written to a peer that never answers: still closed.
    for _ in 0..6 {
        tokio::time::advance(Duration::from_secs(10)).await;
        l.note_write();
    }
    assert_eq!(l.poll(), LivenessAction::Close);
}
