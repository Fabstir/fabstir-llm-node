// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! NM1 D20 — an LTX job must run under its OWN template's model id: settlement
//! pays at the session's model price, so a session opened for a cheaper model
//! (upscale, 300) must not buy a full-price render. And its proof must be able
//! to land: `submitProofOfWork` reverts unless the session is Active, this host's,
//! and its deposit covers the claim — a render past any of those delivers free.

use std::cell::Cell;
use std::time::Duration;

use ethers::types::{Address, U256};
use fabstir_llm_node::ltx::template::{
    check_session_model, check_session_terms, ltx_model_id, read_with_retry, session_gate,
};
use fabstir_llm_node::training::accept::{decode_session_snapshot, SessionSnapshot, SessionStatus};

fn hex32(s: &str) -> [u8; 32] {
    let bytes = hex::decode(s.trim_start_matches("0x")).unwrap();
    bytes.try_into().unwrap()
}

#[test]
fn test_ltx_model_id_matches_live_ids() {
    // Literals copied from an independent source: platformless-helper/src/config.ts.
    assert_eq!(
        ltx_model_id("ltx-t2v-hdr"),
        hex32("0xd1960cd5073ff50278a61fd5a10dc40f14a06297b4359b58a83e9c8767201a84")
    );
    assert_eq!(
        ltx_model_id("ltx-upscale-hdr"),
        hex32("0xcb4d5f018ef3c5c32dd66973f58349d5eaaf64299f7f479e1d3bcc6bb51a4287")
    );
    assert_eq!(
        ltx_model_id("ltx-sdr2hdr-hdr"),
        hex32("0xc9e1f8c7cac07250b1f7197cc625718514a92b036f61171815cdaef59e1f2670")
    );
    // The two new modes' ids, recorded at planning (prefix and suffix).
    let alpha = format!("0x{}", hex::encode(ltx_model_id("ltx-alpha-hdr")));
    assert!(alpha.starts_with("0x0b134c89") && alpha.ends_with("c495"), "{alpha}");
    let layout = format!("0x{}", hex::encode(ltx_model_id("ltx-layout-hdr")));
    assert!(layout.starts_with("0xed9d14af") && layout.ends_with("14a5"), "{layout}");
}

#[test]
fn test_check_session_model() {
    let own = ltx_model_id("ltx-alpha-hdr");
    assert!(check_session_model(Some(own), "ltx-alpha-hdr").is_ok());
    // Another template's id (a cheaper session) is refused.
    let other = ltx_model_id("ltx-upscale-hdr");
    let err = check_session_model(Some(other), "ltx-alpha-hdr").unwrap_err();
    assert!(err.contains("ltx-alpha-hdr"), "{err}");
    // All-zero (a legacy or unset session model) is refused.
    assert!(check_session_model(Some([0u8; 32]), "ltx-alpha-hdr").is_err());
    // No on-chain job id: nobody pays for the GPU work, so it is refused.
    assert!(check_session_model(None, "ltx-alpha-hdr").is_err());
}

#[tokio::test]
async fn test_session_model_read_retries() {
    // A stub read that fails twice, then answers: three attempts succeed.
    let calls = Cell::new(0u32);
    let want = ltx_model_id("ltx-t2v-hdr");
    let got = read_with_retry(3, Duration::ZERO, Duration::from_secs(10), || {
        calls.set(calls.get() + 1);
        let n = calls.get();
        async move {
            if n < 3 {
                Err(anyhow::anyhow!("rpc blip {n}"))
            } else {
                Ok(want)
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(got, want);
    assert_eq!(calls.get(), 3);

    // Every attempt failing → the last error, after exactly `attempts` reads.
    let calls = Cell::new(0u32);
    let err = read_with_retry(3, Duration::ZERO, Duration::from_secs(10), || {
        calls.set(calls.get() + 1);
        async { Err::<[u8; 32], _>(anyhow::anyhow!("rpc down")) }
    })
    .await
    .unwrap_err();
    assert!(err.contains("rpc down"), "{err}");
    assert_eq!(calls.get(), 3);
}

#[tokio::test(start_paused = true)]
async fn test_chain_read_attempt_times_out() {
    // An RPC that never answers (rather than one that errors) must not hold the
    // job task — and with it the VRAM permit and the pending-proof mark — forever.
    let calls = Cell::new(0u32);
    let outcome = tokio::time::timeout(
        Duration::from_secs(3600),
        read_with_retry(3, Duration::from_secs(2), Duration::from_secs(10), || {
            calls.set(calls.get() + 1);
            std::future::pending::<anyhow::Result<[u8; 32]>>()
        }),
    )
    .await
    .expect("every attempt is bounded, so the read gives up well inside an hour");
    let err = outcome.unwrap_err();
    assert!(err.contains("timed out"), "{err}");
    assert_eq!(calls.get(), 3);
}

/// The live `sessionJobs(931)` return (training's pinned fixture): deposit 695,977,
/// price 904 → capacity 695,977 × 1000 / 904 = 769,886 tokens; host 0x4594…;
/// tokensUsed 733,225; status Completed.
fn live_931() -> SessionSnapshot {
    let raw = hex::decode(include_str!("../training_api/fixtures/sessionjobs_931.hex").trim()).unwrap();
    decode_session_snapshot(&raw).unwrap()
}

/// The same session as if open and unused.
fn open_931() -> SessionSnapshot {
    let mut s = live_931();
    s.status = SessionStatus::Active;
    s.tokens_used = U256::zero();
    s
}

fn host_931() -> Address {
    "0x4594f755f593b517bb3194f4dec20c48a3f04504".parse().unwrap()
}

#[test]
fn test_session_terms_accept_an_open_session_of_this_host() {
    assert!(check_session_terms(&open_931(), host_931(), 50_000, 0).is_ok());
}

#[test]
fn test_session_terms_refuse_a_settled_session() {
    // The real bytes of a settled session: a job naming it would render, its proof
    // revert, and the clip deliver free.
    let err = check_session_terms(&live_931(), host_931(), 1_000, 0).unwrap_err();
    assert!(err.contains("not Active"), "{err}");
    let mut timed_out = open_931();
    timed_out.status = SessionStatus::TimedOut;
    assert!(check_session_terms(&timed_out, host_931(), 1_000, 0).is_err());
}

#[test]
fn test_session_terms_refuse_another_hosts_session() {
    let other: Address = "0x048afa7126a3b684832886b78e7cc1dd4019557e".parse().unwrap();
    let err = check_session_terms(&open_931(), other, 1_000, 0).unwrap_err();
    assert!(err.contains("host"), "{err}");
}

#[test]
fn test_session_terms_deposit_boundary() {
    // Exactly the capacity passes (the contract's cumulative <= deposit x 1000 / price);
    // one token more is refused.
    assert!(check_session_terms(&open_931(), host_931(), 769_886, 0).is_ok());
    let err = check_session_terms(&open_931(), host_931(), 769_887, 0).unwrap_err();
    assert!(err.contains("deposit"), "{err}");
    // Net of tokensUsed: 769,886 − 733,225 = 36,661 left.
    let mut used = open_931();
    used.tokens_used = U256::from(733_225u64);
    assert!(check_session_terms(&used, host_931(), 36_661, 0).is_ok());
    assert!(check_session_terms(&used, host_931(), 36_662, 0).is_err());
}

#[test]
fn test_session_terms_count_this_nodes_unproven_clips() {
    // The chain's tokensUsed can lag this node's own record (a proof not yet landed,
    // or a read from a lagging RPC): the larger of the two counts.
    assert!(check_session_terms(&open_931(), host_931(), 669_886, 100_000).is_ok());
    assert!(check_session_terms(&open_931(), host_931(), 669_887, 100_000).is_err());
    // The LARGER, never the sum: both non-zero, the chain's 733,225 already counts this
    // node's 100,000, so 36,661 tokens are still covered (a sum would refuse it).
    let mut used = open_931();
    used.tokens_used = U256::from(733_225u64);
    assert!(check_session_terms(&used, host_931(), 36_661, 100_000).is_ok());
}

#[test]
fn test_session_terms_refuse_a_zero_price() {
    let mut free = open_931();
    free.price_per_token = U256::zero();
    assert!(check_session_terms(&free, host_931(), 1_000, 0).is_err());
}

// ---- session_gate: the whole pre-staging D20 decision, the two chain reads injected ----

const HOST_931: &str = "0x4594f755f593b517bb3194f4dec20c48a3f04504";

/// The raw `sessionJobs` return with its status word (w12) set to Active and
/// tokensUsed (w6) zeroed — `open_931()` as bytes.
fn open_931_raw() -> Vec<u8> {
    let mut raw = hex::decode(include_str!("../training_api/fixtures/sessionjobs_931.hex").trim()).unwrap();
    raw[12 * 32 + 31] = 0;
    raw[6 * 32..7 * 32].fill(0);
    raw
}

#[tokio::test]
async fn test_session_gate_refuses_a_job_without_an_id_and_reads_nothing() {
    let reads = Cell::new(0u32);
    let err = session_gate(
        None,
        "ltx-t2v-hdr",
        50_000,
        0,
        HOST_931,
        |_| {
            reads.set(reads.get() + 1);
            async { Ok(ltx_model_id("ltx-t2v-hdr")) }
        },
        |_| {
            reads.set(reads.get() + 1);
            async { Ok(open_931_raw()) }
        },
    )
    .await
    .unwrap_err();
    assert!(err.contains("no on-chain job id"), "{err}");
    assert_eq!(reads.get(), 0, "nothing to read without an id");
}

#[tokio::test]
async fn test_session_gate_accepts_a_matching_open_funded_session() {
    let got = session_gate(
        Some(931),
        "ltx-t2v-hdr",
        50_000,
        0,
        HOST_931,
        |jid| async move {
            assert_eq!(jid, 931);
            Ok(ltx_model_id("ltx-t2v-hdr"))
        },
        |jid| async move {
            assert_eq!(jid, 931);
            Ok(open_931_raw())
        },
    )
    .await;
    assert!(got.is_ok(), "{got:?}");
}

#[tokio::test]
async fn test_session_gate_refuses_each_failing_part() {
    let model_ok = |_| async { Ok(ltx_model_id("ltx-t2v-hdr")) };
    let raw_ok = |_| async { Ok(open_931_raw()) };
    // Another template's model.
    let err = session_gate(Some(931), "ltx-alpha-hdr", 50_000, 0, HOST_931, model_ok, raw_ok).await.unwrap_err();
    assert!(err.contains("session was opened for model"), "{err}");
    // The live, settled session's own bytes.
    let settled = |_| async { Ok(hex::decode(include_str!("../training_api/fixtures/sessionjobs_931.hex").trim()).unwrap()) };
    let err = session_gate(Some(931), "ltx-t2v-hdr", 50_000, 0, HOST_931, model_ok, settled).await.unwrap_err();
    assert!(err.contains("not Active"), "{err}");
    // Another host (this node is TEST_HOST_4).
    let err = session_gate(Some(931), "ltx-t2v-hdr", 50_000, 0, "0x048afa7126a3b684832886b78e7cc1dd4019557e", model_ok, raw_ok)
        .await
        .unwrap_err();
    assert!(err.contains("not this host"), "{err}");
    // More tokens than the deposit covers (capacity 769,886).
    let err = session_gate(Some(931), "ltx-t2v-hdr", 769_887, 0, HOST_931, model_ok, raw_ok).await.unwrap_err();
    assert!(err.contains("deposit"), "{err}");
    // An undecodable session record fails closed.
    let short = |_| async { Ok(vec![0u8; 64]) };
    let err = session_gate(Some(931), "ltx-t2v-hdr", 50_000, 0, HOST_931, model_ok, short).await.unwrap_err();
    assert!(err.contains("too short"), "{err}");
    // An unreadable host address fails closed.
    let err = session_gate(Some(931), "ltx-t2v-hdr", 50_000, 0, "not-an-address", model_ok, raw_ok).await.unwrap_err();
    assert!(err.contains("host address"), "{err}");
}

#[tokio::test(start_paused = true)]
async fn test_session_gate_refuses_when_a_read_keeps_failing() {
    let err = session_gate(
        Some(931),
        "ltx-t2v-hdr",
        50_000,
        0,
        HOST_931,
        |_| async { Err::<[u8; 32], _>(anyhow::anyhow!("rpc down")) },
        |_| async { Ok(open_931_raw()) },
    )
    .await
    .unwrap_err();
    assert!(err.contains("rpc down"), "{err}");
    let err = session_gate(
        Some(931),
        "ltx-t2v-hdr",
        50_000,
        0,
        HOST_931,
        |_| async { Ok(ltx_model_id("ltx-t2v-hdr")) },
        |_| async { Err::<Vec<u8>, _>(anyhow::anyhow!("sessionJobs down")) },
    )
    .await
    .unwrap_err();
    assert!(err.contains("sessionJobs down"), "{err}");
}
