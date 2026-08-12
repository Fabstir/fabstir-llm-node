// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// The panic seam these tests drive is compiled out when debug_assertions is
// off, so the whole file is gated the same way rather than failing under a
// blanket `cargo test --release`. See the "require `debug_assertions`" note
// below. Nothing in the repo runs this suite in release today.
#![cfg(debug_assertions)]
//! A.1 — panic safety on the LTX generation path (DL16(a)).
//!
//! A panic inside the generation core used to skip the single-exit cleanup, so
//! the clip's pending proof was never forfeited. `pending_count` then stayed at
//! 1 forever, which makes the later disconnect path take
//! `LtxTracker::defer_completion` (returns `true`) and `api/server.rs:4383`'s
//! disconnect handler return WITHOUT calling `completeSessionJob` — stranding
//! the session's escrow until the user pays an on-chain
//! `triggerSessionTimeout` reclaim.
//!
//! Scope honesty: these cover PANIC-induced stranding only. SIGKILL, an OOM
//! kill, `docker stop` (SIGTERM) and a container restart lose `LtxTracker`'s
//! in-memory state and strand the same escrow; `catch_unwind` does nothing for
//! those (IMPLEMENTATION §0.1, R8).
//!
//! ## Why these tests assert on the exact error MESSAGE
//!
//! An earlier draft asserted only `code == "GENERATION_FAILED"` and
//! `pending_count == 0`. That was a **false green**, proven by mutation: with
//! the panic seam deleted entirely, all assertions still passed. The job used
//! here is `t2v` (no inputs), so `prepare_inputs` returns immediately and
//! `client.submit()` fails against the dead endpoint `127.0.0.1:1` — and that
//! ordinary failure exit ALSO sends `GENERATION_FAILED` and ALSO forfeits. The
//! two arms were observationally identical.
//!
//! `"generation failed unexpectedly"` is the unique discriminator: it is the
//! only occurrence of that literal in `src/`, and only the caught-panic branch
//! emits it. `submit_failure_control_arm_is_observably_different` pins the two
//! arms apart so the discriminator cannot silently converge.
//!
//! ## These tests require `debug_assertions`
//!
//! The seam is wrapped in `if cfg!(debug_assertions)` so it is compiled out of
//! every release binary (verified: the panic literals do not appear in
//! `target/release/fabstir-llm-node`). `cargo test` uses the `test` profile,
//! where `debug_assertions` is on, so the normal gate is unaffected. Running
//! this suite under `cargo test --release` would make the seam inert and the
//! panic tests fail; nothing in the repo does that (the only `--release` test
//! runs are named risc0 benchmarks in `scripts/benchmark_risc0_gpu.sh`, each
//! with `--exact`).

use super::ltx_task_support::{mark_pending, pending_count};
use fabstir_llm_node::api::websocket::handlers::ltx::{LtxGenerateTask, LtxPanicSeam};
use fabstir_llm_node::{api::server::ApiServer, ltx::*};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Semaphore};

/// The literal the caught-panic branch sends, and nothing else in `src/` does.
const PANIC_MSG: &str = "generation failed unexpectedly";

fn key() -> [u8; 32] {
    [0x22; 32]
}

/// Decrypt an `encrypted_response` envelope back to its inner JSON.
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

/// Decrypt the next frame on the channel, failing loudly (never hanging) if
/// none arrives. Awaits under a bound rather than `try_recv`: in the
/// through-`spawn` test the forfeit is observable strictly BEFORE the terminal
/// send, so a non-blocking read raced the last send — deterministic on the
/// current-thread runtime, but one inserted yield (or a `multi_thread` flavour)
/// away from a spurious failure that would look like the production bug.
async fn next_frame(rx: &mut mpsc::Receiver<Value>) -> Value {
    let raw = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for the terminal frame")
        .expect("a terminal frame must have been sent to the client");
    decrypt_envelope(&raw, &key())
}

fn job() -> LtxJob {
    LtxJob {
        template_id: "ltx-t2v-hdr".to_string(),
        template_hash: "0x00".to_string(),
        prompt: "panic seam".to_string(),
        seed: "1".to_string(),
        frames: 121,
        fps: 24,
        resolution: Resolution { w: 1280, h: 720 },
        lora: "ltx-iclora-hdr@v1".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
        videos: None,
        strength: None,
        azimuth: None,
        elevation: None,
        distance: None,
        input_wire: None,
    }
}

/// A task whose core takes `seam` at entry. The permit comes from a throwaway
/// semaphore so nothing here touches the server's real VRAM slot.
async fn task_with(job_id: Option<u64>, seam: Option<LtxPanicSeam>) -> LtxGenerateTask {
    let sem = Arc::new(Semaphore::new(1));
    LtxGenerateTask {
        job: job(),
        patched_graph: Graph(json!({})),
        request_id: Some("r-panic".to_string()),
        deep_total_cap: 4_294_967_296,
        allow_list_version: 16,
        timeout_secs: 5,
        job_id,
        permit: sem.acquire_owned().await.unwrap(),
        pending_marked: true,
        panic_seam: seam,
    }
}

fn comfy() -> Arc<ComfyClient> {
    // Dead endpoint. Never reached when a seam panics at core entry; with no
    // seam it produces the ordinary "submit failed" control arm.
    Arc::new(ComfyClient::new("http://127.0.0.1:1").unwrap())
}

/// Poll until the clip's pending proof resolves, or give up after 5 s.
async fn await_forfeit(server: &ApiServer, jid: u64) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while server.ltx_tracker().has_pending(jid).await && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A.1(i) through the PRODUCTION wiring: `spawn` detaches the task, the core
/// panics, and the pending proof must still be forfeited.
///
/// This is the defect as it reaches production — `spawn` discards its
/// `JoinHandle` (nothing awaits the task), so the assertion polls.
#[tokio::test]
async fn panicking_core_forfeits_pending_proof_through_spawn() {
    let server = Arc::new(ApiServer::new_for_test());
    let jid = 4242u64;
    mark_pending(&server, jid).await;
    assert_eq!(
        pending_count(&server, jid).await,
        1,
        "one clip is in flight before the panic"
    );

    let (tx, mut rx) = mpsc::channel::<Value>(16);
    task_with(Some(jid), Some(LtxPanicSeam::BeforeResolve))
        .await
        .spawn(comfy(), key(), "sess-panic".to_string(), server.clone(), tx);
    await_forfeit(&server, jid).await;

    assert_eq!(
        pending_count(&server, jid).await,
        0,
        "a panicking core must forfeit the clip's pending proof; leaving it pending makes \
         defer_completion() return true forever and strands the session escrow"
    );
    assert_eq!(
        next_frame(&mut rx).await["error"]["message"],
        PANIC_MSG,
        "the forfeit must have come from the PANIC branch — every ordinary error exit also \
         sends GENERATION_FAILED and also forfeits, so the code alone proves nothing"
    );
}

/// A.1(i) deterministically, plus the terminal frame: awaiting `run` directly
/// must RETURN (the panic is caught, not propagated) and must have emitted an
/// `ltx_error` so the client is not left waiting for `LTX_JOB_TIMEOUT_SECS`.
#[tokio::test]
async fn panicking_core_is_caught_and_emits_a_terminal_error_frame() {
    let server = Arc::new(ApiServer::new_for_test());
    let jid = 4343u64;
    mark_pending(&server, jid).await;

    let (tx, mut rx) = mpsc::channel::<Value>(16);
    // If the panic escapes `run`, this await unwinds and the test fails here.
    task_with(Some(jid), Some(LtxPanicSeam::BeforeResolve))
        .await
        .run(
            comfy(),
            key(),
            "sess-panic-2".to_string(),
            server.clone(),
            tx,
        )
        .await;

    assert_eq!(
        pending_count(&server, jid).await,
        0,
        "the caught panic still funnels through the single-exit cleanup"
    );

    let inner = next_frame(&mut rx).await;
    assert_eq!(inner["type"], "ltx_error", "terminal frame is an error");
    assert_eq!(
        inner["error"]["code"], "GENERATION_FAILED",
        "DL16(a)/DL9: panics reuse the existing failure code"
    );
    assert_eq!(
        inner["error"]["message"], PANIC_MSG,
        "the frame comes from the panic branch, and carries no panic payload, path or backtrace"
    );
    assert_eq!(inner["requestId"], "r-panic", "requestId is echoed back");
}

/// The control arm the two tests above are distinguished FROM. With no seam the
/// core runs for real, fails to reach ComfyUI, and takes an ordinary error exit.
/// It also forfeits and also sends `GENERATION_FAILED` — so if this message ever
/// became `PANIC_MSG`, the panic tests would go false-green again.
#[tokio::test]
async fn submit_failure_control_arm_is_observably_different() {
    let server = Arc::new(ApiServer::new_for_test());
    let jid = 4646u64;
    mark_pending(&server, jid).await;

    let (tx, mut rx) = mpsc::channel::<Value>(16);
    task_with(Some(jid), None)
        .await
        .run(comfy(), key(), "sess-ctl".to_string(), server.clone(), tx)
        .await;

    assert_eq!(
        pending_count(&server, jid).await,
        0,
        "the ordinary failure exit forfeits too — which is exactly why the code is not a \
         discriminator"
    );
    let inner = next_frame(&mut rx).await;
    assert_eq!(inner["error"]["code"], "GENERATION_FAILED");
    let msg = inner["error"]["message"].as_str().unwrap();
    assert!(
        msg.starts_with("submit failed:"),
        "the non-panic arm reports its own cause, got {msg:?}"
    );
    assert_ne!(
        msg, PANIC_MSG,
        "the panic branch's message must stay unique to it"
    );
}

/// A.1(ii) THROUGH THE CALL SITE: a clip whose proof `finalize_clip` already
/// resolved must not be forfeited a second time when the task then panics.
///
/// This pins the ARGUMENT the call site passes, not merely the function's own
/// logic: `run` hands `finish_ltx_task` its own `pending_resolved` local, and
/// hardcoding that argument to `false` (a plausible refactor slip) makes this
/// test fail 2 → 1. Two clips are in flight so the spurious decrement is
/// observable at all — with one clip `mark_proof_forfeited`'s `saturating_sub`
/// is already at its floor.
///
/// Two honest limits. (1) It does NOT pin that the call happens: deleting
/// `finish_ltx_task(...)` from `run` outright leaves the count at 2 and this
/// test green. The `BeforeResolve` tests above are what pin the call. (2) An
/// earlier draft of this comment claimed the guarded mutation was turning
/// `core` into an `async move` block; that is wrong — `async move` also moves
/// `server`, `progress_tx` and `session_id` into the core, so it does not
/// compile. The `&mut` capture of `pending_resolved` is enforced by the borrow
/// checker, not by this test.
#[tokio::test]
async fn panic_after_resolve_does_not_double_forfeit_through_the_call_site() {
    let server = Arc::new(ApiServer::new_for_test());
    let jid = 4747u64;
    mark_pending(&server, jid).await;
    mark_pending(&server, jid).await;
    assert_eq!(pending_count(&server, jid).await, 2, "two clips in flight");

    let (tx, mut rx) = mpsc::channel::<Value>(16);
    task_with(Some(jid), Some(LtxPanicSeam::AfterResolve))
        .await
        .run(comfy(), key(), "sess-after".to_string(), server.clone(), tx)
        .await;

    assert_eq!(
        pending_count(&server, jid).await,
        2,
        "a clip resolved before the panic must NOT be forfeited again — doing so would consume \
         the OTHER in-flight clip's pending mark"
    );
    assert_eq!(
        next_frame(&mut rx).await["error"]["message"],
        PANIC_MSG,
        "and it really was the panic branch that ran"
    );
}

/// The money cleanup must run even when the client has stopped reading.
///
/// `send_err` awaits a send on a BOUNDED channel (capacity 32 in production,
/// `api/server.rs:2916`) whose drain loop blocks inside `ws_sender.send().await`
/// with no write timeout. A client that holds the socket open but stops reading
/// TCP fills that channel, so a terminal send placed BEFORE the cleanup parks
/// for ever and the forfeit never happens — reinstating the exact stranding
/// this slice closes, on an input the client controls.
///
/// Here the channel is capacity 1 and pre-filled, and the receiver is kept alive
/// but never drained, so `send_err` cannot complete. The forfeit must still be
/// observable.
///
/// SCOPE: this covers the PANIC exit only. The core's other terminal exits
/// `send_err(...).await` and then `return` from inside the core, so under the
/// same wedged client they park before the core returns and the cleanup never
/// runs — the same stranding without a panic. That is pre-existing and outside
/// Slice A; the fix is a write timeout or `try_send` on those sends.
#[tokio::test]
async fn cleanup_runs_before_the_panic_terminal_send_so_a_wedged_client_cannot_block_it() {
    let server = Arc::new(ApiServer::new_for_test());
    let jid = 4949u64;
    mark_pending(&server, jid).await;

    let (tx, _rx_kept_alive) = mpsc::channel::<Value>(1);
    tx.send(json!({"filler": true}))
        .await
        .expect("channel is open");
    // The channel is now full and nothing will ever drain it.

    let task = task_with(Some(jid), Some(LtxPanicSeam::BeforeResolve)).await;
    let handle = tokio::spawn(task.run(
        comfy(),
        key(),
        "sess-wedged".to_string(),
        server.clone(),
        tx,
    ));
    await_forfeit(&server, jid).await;
    let pending = pending_count(&server, jid).await;
    handle.abort();

    assert_eq!(
        pending, 0,
        "the pending proof must be forfeited even though the terminal send can never complete; \
         sending first would strand the escrow on a client-controlled input"
    );
}
