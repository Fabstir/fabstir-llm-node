// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P4.5 — the node's `GET /info` pre-check (`KeyBrokerClient::preflight`)
//! and the `t5t:` witness rule in `request_key`, against the TLS fake broker.
//! Design: `docs/development/DESIGN-PHASE5-P45-NODE-BUNDLE.md` §3, §4, §7.

use super::kbs_fixture::{
    client, client_with_timeout, info_broker, pki, release_broker, spawn_tls_broker,
    spawn_tls_broker_delayed, spawn_tls_broker_with_hits, spawn_tls_raw, Hits, DEK, HOST, MODEL,
    NONCE, TEST_MODEL,
};
use fabstir_llm_node::tee::kbs_http::{HttpKeyBrokerClient, MAX_BODY};
use fabstir_llm_node::tee::key_broker::KeyBrokerClient;
use fabstir_llm_node::tee::keywrap::{generate_ephemeral_keypair, unwrap_key};
use fabstir_llm_node::tee::mock::MockAttestationProvider;
use fabstir_llm_node::tee::provider::AttestationProvider;
use fabstir_llm_node::tee::types::{CcMode, Evidence, TeeError};
use hyper::StatusCode;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const FAST: Duration = Duration::from_millis(50);

fn hits() -> Hits {
    Arc::new(Mutex::new(Vec::new()))
}

fn info(keyring: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "keyring": keyring,
        "gpu_evidence": "real",
        "cpu_evidence": "real",
        "nonce_ttl_seconds": 300,
        "nras_claims_version": "2.0",
        "version": "v-test"
    }))
    .unwrap()
}

/// A client aimed at an `info_broker` answering `body`, recording hits.
async fn preflight_client(keyring_body: Vec<u8>, accept: bool) -> (HttpKeyBrokerClient, Hits) {
    let p = pki(HOST);
    let h = hits();
    let addr =
        spawn_tls_broker_with_hits(&p, info_broker(StatusCode::OK, keyring_body), h.clone()).await;
    (
        client(&p, addr)
            .with_accept_test_release(accept)
            .with_preflight_retry_interval(FAST),
        h,
    )
}

fn kbs_err(r: Result<(), TeeError>) -> String {
    match r {
        Err(TeeError::Kbs(m)) => m,
        other => panic!("expected TeeError::Kbs, got {other:?}"),
    }
}

#[tokio::test]
async fn preflight_refuses_a_test_keyring_broker_without_the_opt_in() {
    // Row 1: for BOTH ids, and with the message only row 1 produces (row 2 could
    // refuse MODEL too, but never TEST_MODEL).
    for id in [MODEL, TEST_MODEL] {
        let (kbs, h) = preflight_client(info("test"), false).await;
        let m = kbs_err(kbs.preflight(id).await);
        assert!(m.starts_with("broker /info:"), "{m}");
        assert!(m.contains("TEE_ACCEPT_TEST_RELEASE"), "{m}");
        // the modes ride in the refusal (an info! line is gone under RUST_LOG=warn)
        assert!(m.contains("gpu_evidence=real cpu_evidence=real"), "{m}");
        assert_eq!(
            *h.lock().unwrap(),
            vec![("GET".to_string(), "/v1/kbs/info".to_string())]
        );
    }
}

#[tokio::test]
async fn preflight_refuses_a_test_keyring_for_a_non_t5t_model_even_when_accepting() {
    let (kbs, _) = preflight_client(info("test"), true).await;
    let m = kbs_err(kbs.preflight(MODEL).await);
    assert!(m.contains("does not carry"), "{m}");
}

#[tokio::test]
async fn preflight_accepts_a_test_keyring_for_a_t5t_model_when_accepting() {
    let (kbs, h) = preflight_client(info("test"), true).await;
    kbs.preflight(TEST_MODEL).await.expect("row 3");
    assert_eq!(h.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn preflight_refuses_a_real_keyring_for_a_t5t_model() {
    // Row 4 says `accept` = any: both arms (mutation: write the rule for one).
    for accept in [false, true] {
        let (kbs, _) = preflight_client(info("real"), accept).await;
        let m = kbs_err(kbs.preflight(TEST_MODEL).await);
        assert!(m.contains("REAL"), "{m}");
    }
}

#[tokio::test]
async fn preflight_allows_the_opt_in_against_a_real_keyring() {
    // Row 5 (P6): harmless, WARN only.
    let (kbs, _) = preflight_client(info("real"), true).await;
    kbs.preflight(MODEL).await.expect("row 5");
    // and the production row 6
    let (kbs, _) = preflight_client(info("real"), false).await;
    kbs.preflight(MODEL).await.expect("row 6");
}

#[tokio::test]
async fn preflight_tolerates_a_minimal_or_extended_info_body_and_refuses_the_rest() {
    // minimal body: every optional field defaulted (mutation: a missing
    // #[serde(default)] → Err)
    let (kbs, _) = preflight_client(br#"{"keyring":"real"}"#.to_vec(), false).await;
    kbs.preflight(MODEL).await.expect("minimal body");
    // extended body: an additive field from a newer broker (mutation:
    // deny_unknown_fields → Err)
    let mut v: serde_json::Value = serde_json::from_slice(&info("real")).unwrap();
    v["extra"] = json!(1);
    let (kbs, _) = preflight_client(serde_json::to_vec(&v).unwrap(), false).await;
    kbs.preflight(MODEL).await.expect("extended body");
    // an unknown keyring class is final on the first attempt
    let (kbs, h) = preflight_client(info("staging"), false).await;
    let m = kbs_err(kbs.preflight(MODEL).await);
    assert!(m.contains("staging"), "{m}");
    assert_eq!(h.lock().unwrap().len(), 1);
    // an HTML 503 (nginx's shape while the broker restarts) is transport-class:
    // three attempts, then the status in the message
    let p = pki(HOST);
    let h = hits();
    let addr = spawn_tls_broker_with_hits(
        &p,
        info_broker(
            StatusCode::SERVICE_UNAVAILABLE,
            b"<html><body>503 Service Unavailable</body></html>".to_vec(),
        ),
        h.clone(),
    )
    .await;
    let kbs = client(&p, addr).with_preflight_retry_interval(FAST);
    let m = kbs_err(kbs.preflight(MODEL).await);
    assert!(m.contains("503"), "{m}");
    assert_eq!(h.lock().unwrap().len(), 3, "three attempts on a 5xx");
    // a 404 is final (mutation: retry 4xx → 3 hits)
    let p = pki(HOST);
    let h = hits();
    let addr = spawn_tls_broker_with_hits(
        &p,
        info_broker(StatusCode::NOT_FOUND, b"no".to_vec()),
        h.clone(),
    )
    .await;
    let kbs = client(&p, addr).with_preflight_retry_interval(FAST);
    let m = kbs_err(kbs.preflight(MODEL).await);
    assert!(m.contains("404"), "{m}");
    assert_eq!(h.lock().unwrap().len(), 1);
    // an over-bound 200 is final and names the bound
    let mut big = info("real");
    big.resize(MAX_BODY + 1, b' ');
    let (kbs, h) = preflight_client(big, false).await;
    let m = kbs_err(kbs.preflight(MODEL).await);
    assert!(m.contains("bound"), "{m}");
    assert_eq!(h.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn preflight_rides_out_a_transient_502_but_not_a_verdict() {
    // 502, 502, then the answer → Ok in three hits (mutation: drop the retry → Err)
    let p = pki(HOST);
    let h = hits();
    let n = Arc::new(AtomicUsize::new(0));
    let body = info("real");
    let handler: super::kbs_fixture::Handler = {
        let n = n.clone();
        Arc::new(move |path, _| match path {
            "/v1/kbs/info" if n.fetch_add(1, Ordering::SeqCst) < 2 => (
                StatusCode::BAD_GATEWAY,
                b"<html>502 Bad Gateway</html>".to_vec(),
            ),
            "/v1/kbs/info" => (StatusCode::OK, body.clone()),
            _ => (StatusCode::NOT_FOUND, Vec::new()),
        })
    };
    let addr = spawn_tls_broker_with_hits(&p, handler, h.clone()).await;
    let kbs = client(&p, addr).with_preflight_retry_interval(FAST);
    kbs.preflight(MODEL)
        .await
        .expect("the third attempt answers");
    assert_eq!(h.lock().unwrap().len(), 3);
    // a verdict is never retried (mutation: retry verdicts → 3 hits)
    let (kbs, h) = preflight_client(info("test"), false).await;
    assert!(kbs.preflight(MODEL).await.is_err());
    assert_eq!(h.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn preflight_retries_a_200_whose_body_is_cut_mid_stream() {
    // The broker (or nginx) restarts while streaming a 200: the declared body is
    // longer than what arrives before the close. That is the transient class the
    // retry exists for, whatever the status (mutation: classify a 2xx read
    // failure as Final → Err after one connection).
    let p = pki(HOST);
    let h = hits();
    let full = info("real");
    let cut = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{{\"keyr",
        full.len()
    )
    .into_bytes();
    let ok = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
        full.len()
    )
    .into_bytes()
    .into_iter()
    .chain(full.iter().copied())
    .collect::<Vec<u8>>();
    let addr = spawn_tls_raw(&p, vec![cut.clone(), cut, ok], h.clone()).await;
    let kbs = client(&p, addr).with_preflight_retry_interval(FAST);
    kbs.preflight(MODEL)
        .await
        .expect("the third connection delivers the whole body");
    let hits = h.lock().unwrap();
    assert_eq!(hits.len(), 3, "{hits:?}");
    assert!(
        hits.iter()
            .all(|(m, path)| m == "GET" && path == "/v1/kbs/info"),
        "{hits:?}"
    );
}

#[tokio::test]
async fn preflight_has_its_own_budget() {
    // The client's whole-request budget is 300 ms; the broker answers after 1.5 s;
    // INFO_TIMEOUT (10 s) is what governs /info, so the FIRST attempt succeeds
    // (mutation: use self.timeout → three 300 ms timeouts → Err).
    let p = pki(HOST);
    let count = Arc::new(AtomicUsize::new(0));
    let handler: super::kbs_fixture::Handler = {
        let count = count.clone();
        let body = info("real");
        Arc::new(move |path, _| {
            count.fetch_add(1, Ordering::SeqCst);
            match path {
                "/v1/kbs/info" => (StatusCode::OK, body.clone()),
                _ => (StatusCode::NOT_FOUND, Vec::new()),
            }
        })
    };
    let addr = spawn_tls_broker_delayed(&p, handler, Duration::from_millis(1500)).await;
    let kbs = client_with_timeout(&p, addr, Duration::from_millis(300))
        .with_preflight_retry_interval(FAST);
    let started = std::time::Instant::now();
    kbs.preflight(MODEL)
        .await
        .expect("INFO_TIMEOUT, not the 300 ms client budget");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "the first attempt succeeded"
    );
}

async fn evidence() -> (Evidence, Vec<u8>) {
    let (sk, pk) = generate_ephemeral_keypair();
    let ev = MockAttestationProvider::new("H100", [9u8; 48], CcMode::On)
        .gather_evidence(NONCE, &pk)
        .await
        .unwrap();
    (ev, sk)
}

#[tokio::test]
async fn request_key_refuses_a_release_whose_label_disagrees_with_the_model_id() {
    // test_release: true for a REAL id (the case the label exists for)
    let p = pki(HOST);
    let addr = spawn_tls_broker(&p, release_broker(true)).await;
    let kbs = client(&p, addr).with_accept_test_release(true);
    let (ev, _) = evidence().await;
    match kbs.request_key(MODEL, &ev).await {
        Err(TeeError::Kbs(m)) => assert!(m.contains("witness labelling"), "{m}"),
        other => panic!("mislabelled release must be refused: {other:?}"),
    }
    assert!(
        !kbs.last_release_was_test(),
        "a refused release remembers nothing"
    );
    // test_release: false for a TEST id (a broker bug or a forged response)
    let addr = spawn_tls_broker(&p, release_broker(false)).await;
    let kbs = client(&p, addr).with_accept_test_release(true);
    let (ev, _) = evidence().await;
    match kbs.request_key(TEST_MODEL, &ev).await {
        Err(TeeError::Kbs(m)) => assert!(m.contains("witness labelling"), "{m}"),
        other => panic!("mislabelled release must be refused: {other:?}"),
    }
    // the matching pairs release, and the label is remembered as given
    let addr = spawn_tls_broker(&p, release_broker(true)).await;
    let kbs = client(&p, addr).with_accept_test_release(true);
    let (ev, sk) = evidence().await;
    let w = kbs
        .request_key(TEST_MODEL, &ev)
        .await
        .expect("test release for a test id");
    assert_eq!(unwrap_key(&w, &sk).unwrap(), DEK);
    assert!(kbs.last_release_was_test());
    let addr = spawn_tls_broker(&p, release_broker(false)).await;
    let kbs = client(&p, addr);
    let (ev, sk) = evidence().await;
    let w = kbs
        .request_key(MODEL, &ev)
        .await
        .expect("real release for a real id");
    assert_eq!(unwrap_key(&w, &sk).unwrap(), DEK);
    assert!(!kbs.last_release_was_test());
}
