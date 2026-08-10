// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! OQ-L24 — the LTX progress drain must not park on a wedged client.
//!
//! ## The defect this closes
//!
//! The LTX progress channel is bounded (32, `api/server.rs`). Its drain loop
//! forwards each frame with `ws_sender.send(...).await`, and that await had **no
//! bound**. A client that completes the WebSocket handshake, holds the socket
//! open and then simply stops reading TCP closes its receive window; the write
//! parks for ever; the drain loop stops draining; the channel fills to 32; and
//! then every `send_err`/`send_stage` inside the generation core parks too.
//!
//! The core therefore never returns, so `catch_unwind` never resolves,
//! `drop(_permit)` never runs and the single-exit cleanup never runs. The
//! clip's pending proof stays unresolved for the process lifetime, which makes
//! `LtxTracker::defer_completion` return `true` for ever, so the disconnect
//! path returns WITHOUT `completeSessionJob`: **the session's escrow strands
//! until the user pays an on-chain `triggerSessionTimeout` reclaim**, and the
//! single GPU slot stays pinned the whole time. No panic is required and the
//! trigger is entirely client-controlled.
//!
//! ## Why bounding the DRAIN write is the whole fix
//!
//! The drain loop's own comment already states the intended design: *breaking
//! this loop drops `progress_rx`, which is what the spawn's disconnect gates
//! detect*. That machinery is correct and already exists — a dropped receiver
//! makes every `progress_tx.send()` fail immediately, `send_stage` returns
//! `false`, the core sets `client_gone`, interrupts ComfyUI and takes its
//! error exit, and the cleanup forfeits. The bug was only that a *wedged*
//! client was never classified as gone, because the write never returned.
//!
//! So one bound at the drain, rather than a timeout on each of the core's 13
//! terminal sends, closes every site at once and feeds the existing path.

use fabstir_llm_node::api::websocket::handlers::ltx::{
    parse_ws_write_timeout, send_ws_frame_bounded, WsWriteOutcome,
};
use futures::sink::Sink;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

/// A sink modelling a client whose TCP receive window is shut.
///
/// It parks at **`poll_flush`**, not `poll_ready` — that is the real shape. A
/// live axum/tungstenite sink accepts the frame into its internal buffer
/// (`poll_ready` → Ready, `start_send` → Ok) and only blocks where the socket
/// write actually happens. Modelling the stall at `poll_ready` would leave
/// `SinkExt::send` → `SinkExt::feed` (which skips the flush, and compiles as a
/// drop-in) undetectable, while on a real socket `feed` buffers without
/// writing and never notices the wedged client at all.
struct WedgedSink;

impl<T> Sink<T> for WedgedSink {
    type Error = std::io::Error;
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn start_send(self: Pin<&mut Self>, _: T) -> Result<(), Self::Error> {
        Ok(()) // buffered, exactly as a real sink does
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Pending // the socket write never completes
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }
}

/// A sink that accepts everything, recording what it was given.
#[derive(Default)]
struct OpenSink(Vec<String>);

impl Sink<String> for OpenSink {
    type Error = std::io::Error;
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn start_send(mut self: Pin<&mut Self>, item: String) -> Result<(), Self::Error> {
        self.0.push(item);
        Ok(())
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

/// A sink whose peer has gone. Errors at `poll_flush` for the same
/// real-socket-shape reason as [`WedgedSink`].
struct BrokenSink;

impl<T> Sink<T> for BrokenSink {
    type Error = std::io::Error;
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn start_send(self: Pin<&mut Self>, _: T) -> Result<(), Self::Error> {
        Ok(())
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "peer gone",
        )))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

/// THE DEFECT. A wedged client must not park the write for ever.
///
/// The outer `timeout` is the falsifiability harness, not the assertion: with
/// the bound removed from the implementation this test FAILS (the helper never
/// returns) rather than hanging the suite.
#[tokio::test]
async fn a_wedged_client_times_out_instead_of_parking_for_ever() {
    let mut sink = WedgedSink;
    let outcome = tokio::time::timeout(
        Duration::from_secs(5),
        send_ws_frame_bounded(&mut sink, "frame".to_string(), Duration::from_millis(50)),
    )
    .await
    .expect(
        "the bounded write must RETURN on a wedged client; parking here is what strands the \
         session escrow and pins the GPU slot",
    );

    assert_eq!(
        outcome,
        WsWriteOutcome::TimedOut,
        "a client that stops reading must be classified as gone, so the drain loop breaks and \
         drops progress_rx"
    );
}

/// A healthy client is untouched: the frame is delivered verbatim.
#[tokio::test]
async fn an_open_client_receives_the_frame_and_is_not_classified_as_gone() {
    let mut sink = OpenSink::default();
    let outcome = send_ws_frame_bounded(
        &mut sink,
        "{\"type\":\"ltx_progress\"}".to_string(),
        Duration::from_secs(60),
    )
    .await;

    assert_eq!(outcome, WsWriteOutcome::Sent, "a normal write succeeds");
    assert_eq!(
        sink.0,
        vec!["{\"type\":\"ltx_progress\"}".to_string()],
        "the frame reaches the socket unmodified"
    );
}

/// A genuinely disconnected client is still distinguished from a wedged one.
/// Both break the drain loop, but conflating them would lose the diagnosis.
#[tokio::test]
async fn a_broken_pipe_reports_failed_not_timed_out() {
    let mut sink = BrokenSink;
    let outcome =
        send_ws_frame_bounded(&mut sink, "frame".to_string(), Duration::from_secs(60)).await;

    assert_eq!(
        outcome,
        WsWriteOutcome::Failed,
        "an errored write is a disconnect, not a wedge — the operator needs to tell them apart"
    );
}

/// The outcome enum must drive a break in BOTH abnormal cases. A future edit
/// that breaks only on `Failed` would reopen OQ-L24 exactly.
#[test]
fn both_abnormal_outcomes_mean_the_client_is_gone() {
    assert!(!WsWriteOutcome::Sent.client_is_gone());
    assert!(WsWriteOutcome::Failed.client_is_gone());
    assert!(WsWriteOutcome::TimedOut.client_is_gone());
}

/// WIRING. A correct helper the drain loop does not call fixes nothing, and the
/// loop itself sits ~60 columns deep inside `server.rs`'s WebSocket handler,
/// unreachable from an integration crate without a live socket. Pin the wiring
/// structurally instead (IMPLEMENTATION §10 sanctions `include_str!` for
/// exactly this).
///
/// This fails if someone reinstates the unbounded `ws_sender.send(...).await`
/// on the LTX progress path, or drops the `client_is_gone()` break.
#[test]
fn the_ltx_progress_drain_uses_the_bounded_write() {
    let server_rs = include_str!("../../src/api/server.rs");

    let after_marker = server_rs
        .split("// Drain progress until the generation task completes.")
        .nth(1)
        .expect("the LTX progress drain loop must still be identifiable by its comment");
    // Window over the WHOLE loop — from the marker to the `continue;` that
    // closes the ltx_generate block — not a fixed character count. An earlier
    // draft used a 3000-char window and silently covered less than half of the
    // 6300-char loop, so a regression in its second half would have slipped
    // through the negative assertion below.
    let end = after_marker
        .find("continue;")
        .expect("the ltx_generate block must still end in `continue;`");
    let drain = &after_marker[..end];
    assert!(
        drain.len() > 2000,
        "the matched region ({} chars) is too small to be the drain loop — the marker comment \
         has probably moved, and these assertions would be vacuous",
        drain.len()
    );

    assert!(
        drain.contains("else => break"),
        "the window must reach the end of the select loop, or the negative assertion below \
         only checks part of it"
    );
    assert!(
        drain.contains("send_ws_frame_bounded"),
        "the LTX progress drain must write through the bounded helper (OQ-L24)"
    );
    assert!(
        drain.contains("client_is_gone()"),
        "it must consult client_is_gone(), not `== Failed` — breaking only on a write error \
         leaves the wedged-client case open, which IS OQ-L24"
    );
    assert!(
        !drain.contains("ws_sender.send("),
        "an unbounded ws_sender.send(...).await on this path reopens OQ-L24: a client that \
         stops reading parks it for ever and strands the session escrow"
    );
}

/// The bounded write only helps because breaking the loop DROPS `progress_rx`,
/// which is what makes the core's parked sends fail and take the `client_gone`
/// exit. Hoisting that binding to an outer scope would leave the channel open,
/// the core would still park, and the fix would be completely inert — with
/// every other test in this file still green. Pin its position.
#[test]
fn the_progress_channel_is_scoped_so_breaking_the_loop_drops_it() {
    let server_rs = include_str!("../../src/api/server.rs");

    let block = server_rs
        .split("if let Some(task) = gen_task {")
        .nth(1)
        .expect("the LTX task block must still exist");
    let end = block
        .find("// Drain progress until the generation task completes.")
        .expect("the drain loop must still follow the channel declaration in the same block");

    assert!(
        block[..end].contains("let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel"),
        "progress_rx must be declared INSIDE the block containing the drain loop, so breaking \
         the loop drops it. Hoisted out, the channel stays open, the core's sends keep parking \
         and the OQ-L24 fix is inert"
    );
}

/// WIRING, the ACCEPT path — where the first cut of this fix left the hole open.
///
/// The ack write runs after `handle_encrypted_ltx_generate` has taken the VRAM
/// permit and marked the proof pending, but BEFORE the task is spawned. Parking
/// there is strictly worse than the original bug: nothing exists yet to release
/// the permit (and `MAX_CONCURRENT_GENERATIONS` defaults to 1, so LTX stops
/// working node-wide until restart) or to resolve the pending, and the WS loop
/// never exits so the settlement path never runs either.
#[test]
fn the_ltx_accept_path_ack_write_is_bounded_too() {
    let server_rs = include_str!("../../src/api/server.rs");

    let accept = server_rs
        .split("// OQ-L24 (accept path).")
        .nth(1)
        .expect("the bounded ack write must still be identifiable by its comment");
    let end = accept
        .find("if let Some(task) = gen_task {")
        .expect("the ack write must still sit immediately before the task-spawn block");
    let accept = &accept[..end];

    assert!(
        accept.contains("send_ws_frame_bounded"),
        "the ack write must be bounded — it holds the permit and the pending mark"
    );
    assert!(
        accept.contains("mark_proof_forfeited"),
        "a gone client must resolve the pending the handler marked at accept, per \
         LtxGenerateTask's drop-without-spawn contract"
    );
    assert!(
        !accept.contains("ws_sender.send("),
        "an unbounded ack write leaks the VRAM permit with no owning task"
    );
}

/// The DEFAULT is the fix. Widening it to hours (or `u64::MAX`) silently
/// restores the unbounded behaviour with every other test still green, so it is
/// pinned here. Split from the env read so this needs no process-global mutation.
#[test]
fn the_write_bound_defaults_to_five_minutes_and_is_never_zero() {
    assert_eq!(
        parse_ws_write_timeout(None),
        Duration::from_secs(300),
        "the default bound IS the fix — widening it reopens OQ-L24 in production"
    );
    assert_eq!(
        parse_ws_write_timeout(Some("0")),
        Duration::from_secs(300),
        "0 reads as 'disabled' to an operator; honoured literally it would abort every render \
         at its first frame with a 0-token refund"
    );
    assert_eq!(
        parse_ws_write_timeout(Some("abc")),
        Duration::from_secs(300)
    );
    assert_eq!(parse_ws_write_timeout(Some("")), Duration::from_secs(300));
    assert_eq!(
        parse_ws_write_timeout(Some("45")),
        Duration::from_secs(45),
        "a valid override still wins"
    );
}
