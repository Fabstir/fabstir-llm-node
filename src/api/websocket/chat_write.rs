// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Bounded chat streaming writes (8.62.3).
//!
//! The chat streaming loops awaited `ws_sender.send(chunk)` outside their
//! `select!`. A client that held the socket open but stopped reading parked that
//! write once the TCP buffers filled, so neither the liveness deadline nor a
//! `stream_cancel` could fire, generation stalled on its channel holding the
//! model, and the session waited for the expiry sweeper. Every chat stream write
//! now goes through [`send_chat_frame`], the chat counterpart of the bounded LTX
//! and transcode writes (`handlers::ltx::send_ws_frame_bounded`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::extract::ws::Message;
use futures::Sink;

use super::handlers::ltx::send_ws_frame_bounded;
use super::liveness::Liveness;

/// Default for `CHAT_WS_WRITE_TIMEOUT_SECS`: the liveness rule's 30 s idle Ping
/// plus its 60 s unanswered wait, so a client that stops reading is cut off no
/// later than one that stops answering. Loose enough for a laptop that sleeps
/// briefly mid-reply (a false positive only loses the rest of that reply), and
/// finite, which is the fix. LTX (300 s) and training (900 s) keep their own,
/// longer bounds: their final frames carry outputs a stall must not lose.
pub const CHAT_WS_WRITE_TIMEOUT_DEFAULT_SECS: u64 = 90;

/// The parse half of [`chat_ws_write_timeout`], testable without the
/// environment. `0` and anything unparsable fall back to the default: neither
/// "unbounded" nor "cut off every chunk" is a meaning the setting can have.
pub fn parse_chat_ws_write_timeout(raw: Option<&str>) -> Duration {
    Duration::from_secs(
        raw.and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|s| *s > 0)
            .unwrap_or(CHAT_WS_WRITE_TIMEOUT_DEFAULT_SECS),
    )
}

/// How long one chat stream write may take (`CHAT_WS_WRITE_TIMEOUT_SECS`), read
/// once per process: it is consulted for every streamed chunk.
pub fn chat_ws_write_timeout() -> Duration {
    static TIMEOUT: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();
    *TIMEOUT.get_or_init(|| {
        parse_chat_ws_write_timeout(std::env::var("CHAT_WS_WRITE_TIMEOUT_SECS").ok().as_deref())
    })
}

/// Send one chat stream frame, bounded by `timeout`. Returns `true` when it was
/// written. When the write fails or is still parked at the bound, the client is
/// gone: generation is told to stop (`cancel`, as for `stream_cancel` or a dead
/// peer), the connection is marked dead so its read loop ends and the disconnect
/// path settles the session, and `false` is returned so the caller ends the
/// stream.
pub async fn send_chat_frame<K>(
    sink: &mut K,
    frame: Message,
    timeout: Duration,
    liveness: &mut Liveness,
    cancel: Option<&AtomicBool>,
) -> bool
where
    K: Sink<Message> + Unpin,
{
    let outcome = send_ws_frame_bounded(sink, frame, timeout).await;
    if outcome.client_is_gone() {
        tracing::warn!(
            "[SCI-C] chat stream write {:?} (bound {}s): client gone, generation stopped",
            outcome,
            timeout.as_secs()
        );
        if let Some(flag) = cancel {
            flag.store(true, Ordering::Release);
        }
        liveness.mark_dead();
        return false;
    }
    true
}
