// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! One liveness rule for every loop that reads a WebSocket (SCI-C D16,
//! IMPLEMENTATION-SESSION-COMPLETION-INTEGRITY).
//!
//! Session 1433 sat open for hours on a socket nobody was reading: the read
//! loops waited on `next()` for ever and never asked whether the peer was
//! there. Now, after `WS_PING_AFTER_SECS` (30) with no activity the node sends
//! one Ping, and closes only if that Ping is unanswered for
//! `WS_PING_UNANSWERED_CLOSE_SECS` (60). Activity is any inbound frame (Pong
//! included) and the top of every outer-loop iteration, so returning from a
//! long inline handler never looks idle; a successful outbound write defers the
//! next Ping but never answers one already sent (it only reached the kernel
//! buffer). A failed Ping send closes at once. The close then runs the ordinary
//! disconnect path, which leaves completion to work still in flight (D15).
//!
//! A browser answers Pings itself, so this closes half-open sockets and dead
//! proxies; an abandoned but live tab is D15's business, not this one's.

use std::time::Duration;

use axum::extract::ws::Message;
use futures::{Sink, Stream, StreamExt};
use tokio::time::Instant;

pub const PING_AFTER_DEFAULT_SECS: u64 = 30;
pub const CLOSE_AFTER_DEFAULT_SECS: u64 = 60;
/// Neither knob may be set below this.
pub const FLOOR_SECS: u64 = 10;
/// Bound on writing the Ping itself.
pub const PING_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivenessAction {
    None,
    Ping,
    Close,
}

/// D16(a), pure: what to do at `now`, given the last activity and when the
/// outstanding Ping (if any) was sent. Activity after a Ping clears `pinged_at`.
pub fn liveness_action(
    now: Instant,
    last_activity: Instant,
    pinged_at: Option<Instant>,
    ping_after: Duration,
    close_after: Duration,
) -> LivenessAction {
    match pinged_at {
        Some(sent) if now >= sent + close_after => LivenessAction::Close,
        Some(_) => LivenessAction::None,
        None if now >= last_activity + ping_after => LivenessAction::Ping,
        None => LivenessAction::None,
    }
}

fn knob(name: &str, default: u64) -> Duration {
    Duration::from_secs(
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(default)
            .max(FLOOR_SECS),
    )
}

/// One connection's liveness clock, shared by its outer loop and every nested
/// loop that reads the same socket.
#[derive(Debug)]
pub struct Liveness {
    last_activity: Instant,
    pinged_at: Option<Instant>,
    ping_after: Duration,
    close_after: Duration,
    dead: bool,
}

impl Liveness {
    pub fn new(ping_after: Duration, close_after: Duration) -> Self {
        Self {
            last_activity: Instant::now(),
            pinged_at: None,
            ping_after: ping_after.max(Duration::from_secs(FLOOR_SECS)),
            close_after: close_after.max(Duration::from_secs(FLOOR_SECS)),
            dead: false,
        }
    }

    /// From `WS_PING_AFTER_SECS` and `WS_PING_UNANSWERED_CLOSE_SECS`.
    pub fn from_env() -> Self {
        Self::new(
            knob("WS_PING_AFTER_SECS", PING_AFTER_DEFAULT_SECS),
            knob("WS_PING_UNANSWERED_CLOSE_SECS", CLOSE_AFTER_DEFAULT_SECS),
        )
    }

    /// Something happened on the socket: an inbound frame, a write, or the
    /// top of an outer-loop iteration.
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
        self.pinged_at = None;
    }

    /// A write reached the socket. It defers the next Ping but does NOT answer
    /// one already sent: a write only reaches the kernel buffer, so a dead peer
    /// would otherwise never be closed while frames keep flowing.
    pub fn note_write(&mut self) {
        self.last_activity = Instant::now();
    }

    /// When the next decision falls due.
    pub fn deadline(&self) -> Instant {
        match self.pinged_at {
            Some(sent) => sent + self.close_after,
            None => self.last_activity + self.ping_after,
        }
    }

    /// Decide now; a `Ping` decision records the Ping as sent.
    pub fn poll(&mut self) -> LivenessAction {
        let now = Instant::now();
        let action = liveness_action(
            now,
            self.last_activity,
            self.pinged_at,
            self.ping_after,
            self.close_after,
        );
        if action == LivenessAction::Ping {
            self.pinged_at = Some(now);
        }
        action
    }

    /// The connection is to be closed (unanswered Ping, failed Ping write).
    pub fn mark_dead(&mut self) {
        self.dead = true;
    }

    pub fn is_dead(&self) -> bool {
        self.dead
    }

    /// Run a due decision: send the Ping (bounded) or mark the connection
    /// dead. Returns `false` once the connection is dead. Call from a
    /// `select!` arm on `sleep_until(self.deadline())`.
    pub async fn on_deadline<K>(&mut self, sink: &mut K) -> bool
    where
        K: Sink<Message> + Unpin,
    {
        match self.poll() {
            LivenessAction::None => {}
            LivenessAction::Ping => {
                let sent = crate::api::websocket::handlers::ltx::send_ws_frame_bounded(
                    sink,
                    Message::Ping(Vec::new()),
                    PING_WRITE_TIMEOUT,
                )
                .await;
                if sent.client_is_gone() {
                    crate::monitoring::session_completion_metrics::SCI_C.liveness_closed();
                    tracing::info!("[SCI-C] liveness: Ping write failed ({sent:?}); closing");
                    self.mark_dead();
                }
            }
            LivenessAction::Close => {
                crate::monitoring::session_completion_metrics::SCI_C.liveness_closed();
                tracing::info!(
                    "[SCI-C] liveness: Ping unanswered for {}s; closing",
                    self.close_after.as_secs()
                );
                self.mark_dead();
            }
        }
        !self.dead
    }
}

/// The outer read loop's wait for its next frame (D16(b)). It resets the
/// clock first (the iteration-top reset: a loop returning from a long handler
/// is not idle), then waits for a frame, pinging when idle. `None` means the
/// stream ended or the connection is dead; every frame of any type counts as
/// activity.
pub async fn next_frame<S, K, E>(
    stream: &mut S,
    sink: &mut K,
    liveness: &mut Liveness,
) -> Option<Result<Message, E>>
where
    S: Stream<Item = Result<Message, E>> + Unpin,
    K: Sink<Message> + Unpin,
{
    liveness.touch();
    loop {
        if liveness.is_dead() {
            return None;
        }
        // Biased, inbound first: a Pong already waiting always wins over a
        // deadline that expired while the loop was not polling.
        tokio::select! {
            biased;
            frame = stream.next() => {
                if frame.is_some() {
                    liveness.touch();
                }
                return frame;
            }
            _ = tokio::time::sleep_until(liveness.deadline()) => {
                if !liveness.on_deadline(sink).await {
                    return None;
                }
            }
        }
    }
}
