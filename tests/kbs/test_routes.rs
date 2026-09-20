//! Design §4 / §6 end to end: the broker's router over TLS with the node's own
//! `HttpKeyBrokerClient` as the client (the frozen wire proven from both sides);
//! simulator CPU mode + a fake NRAS for the GPU half; error kinds and codes; body
//! bound; unknown field; mirror encoding; `/info`; busy; wall; canned under real.

use super::harness::{egress_for, kbs_fixture, pki, spawn_tls_axum, TestPki, HOST};
use super::policy_fixture::{
    event_log, gpu_payload, patched_quote, recording_policy, sign, signer, test_model_id,
};
use super::test_gpu::{spawn_nras, Nras, NrasOpts};
use fabstir_llm_node::kbs::config::KbsConfig;
use fabstir_llm_node::kbs::keyring::Keyring;
use fabstir_llm_node::kbs::routes::{
    release_gate, router, AppState, EvidenceWireIn, RequestKeyRequestWire, Shared,
};
use fabstir_llm_node::kbs::verify::{CcRecord, Verified};
use fabstir_llm_node::tee::kbs_http::{EvidenceWire, HttpKeyBrokerClient, RequestKeyRequest};
use fabstir_llm_node::tee::key_broker::KeyBrokerClient;
use fabstir_llm_node::tee::keywrap::{generate_ephemeral_keypair, unwrap_key};
use fabstir_llm_node::tee::types::{Evidence, TeeError};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

const DEK: [u8; 32] = [0x5c; 32];

pub struct Broker {
    pub pki: TestPki,
    pub addr: SocketAddr,
    pub state: Shared,
    pub nras: Nras,
    pub model_id: [u8; 32],
    /// The provider key the keyring entry is bound to (`signer()` is random per call).
    pub signer: k256::ecdsa::SigningKey,
    pub _dir: tempfile::TempDir,
}

/// A simulator-CPU, real-GPU (fake NRAS) test broker with one test entry and a
/// signed policy for it. `extra_env` overrides the config.
pub async fn spawn_broker(extra_env: &[(&str, &str)], nras_opts: NrasOpts) -> Broker {
    let p = pki(HOST);
    let nras = spawn_nras(&p, nras_opts).await;
    let dir = tempfile::tempdir().unwrap();
    let model_id = test_model_id(1);
    let sk = signer();
    let (signed, provider) = sign(&recording_policy(model_id, 1), &sk);
    let policies = dir.path().join("public/policies");
    std::fs::create_dir_all(&policies).unwrap();
    std::fs::write(
        policies.join(format!("{}.json", hex::encode(model_id))),
        serde_json::to_vec(&signed).unwrap(),
    )
    .unwrap();
    let keyring_path = dir.path().join("keyring.json");
    let keyring_json = json!({"schema": 1, "keys": [{"model_id": hex::encode(model_id), "provider": provider, "dek": hex::encode(DEK), "test": true}]});
    std::fs::write(&keyring_path, serde_json::to_vec(&keyring_json).unwrap()).unwrap();
    std::fs::set_permissions(&keyring_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let mut env: HashMap<String, String> = HashMap::new();
    env.insert(
        "KBS_DATA_DIR".into(),
        dir.path().to_string_lossy().into_owned(),
    );
    env.insert("KBS_CPU_EVIDENCE".into(), "simulator".into());
    env.insert("KBS_GPU_EVIDENCE".into(), "real".into());
    env.insert(
        "KBS_NRAS_GPU_URL".into(),
        format!("{}/v3/attest/gpu", nras.fake.base()),
    );
    env.insert(
        "KBS_NRAS_JWKS_URL".into(),
        format!("{}/jwks", nras.fake.base()),
    );
    env.insert("KBS_NRAS_TIMEOUT_SECS".into(), "5".into());
    env.insert("KBS_JWKS_TIMEOUT_SECS".into(), "5".into());
    for (k, v) in extra_env {
        env.insert((*k).into(), (*v).into());
    }
    let cfg = KbsConfig::from_map(&env).unwrap();
    let keyring = Keyring::load(&cfg.keyring_file, cfg.test_keyring_required()).unwrap();
    let egress = egress_for(&p, nras.fake.addr, &[]);
    let state: Shared = Arc::new(AppState::new(cfg, keyring, egress).unwrap());
    let addr = spawn_tls_axum(&p, router(state.clone())).await;
    Broker {
        pki: p,
        addr,
        state,
        nras,
        model_id,
        signer: sk,
        _dir: dir,
    }
}

fn node_client(b: &Broker) -> HttpKeyBrokerClient {
    kbs_fixture::client(&b.pki, b.addr).with_accept_test_release(true)
}

fn evidence(pk_att: &[u8], nonce: [u8; 32], canned: Option<serde_json::Value>) -> Evidence {
    let pk: [u8; 33] = pk_att.try_into().unwrap();
    Evidence {
        gpu_report: gpu_payload(&nonce, canned),
        cpu_quote: patched_quote(&pk, &nonce),
        event_log: event_log(),
        vm_config: b"{}".to_vec(),
        image_measurement: [0; 48],
        pk_att: pk_att.to_vec(),
        nonce,
    }
}

/// A raw reqwest client (for headers the node client never sets, and raw bodies).
fn raw_client(b: &Broker) -> reqwest::Client {
    reqwest::Client::builder()
        .use_rustls_tls()
        .add_root_certificate(reqwest::Certificate::from_pem(b.pki.root_pem.as_bytes()).unwrap())
        .resolve(HOST, b.addr)
        .build()
        .unwrap()
}

fn base(b: &Broker) -> String {
    format!("https://{HOST}:{}/v1/kbs", b.addr.port())
}

#[tokio::test]
async fn challenge_then_request_key_releases_the_test_dek_end_to_end() {
    let b = spawn_broker(&[], NrasOpts::default()).await;
    let client = node_client(&b);
    let (sk, pk) = generate_ephemeral_keypair();
    let nonce = client.challenge(b.model_id, &pk).await.unwrap();
    let wrapped = client
        .request_key(b.model_id, &evidence(&pk, nonce, None))
        .await
        .unwrap();
    assert_eq!(unwrap_key(&wrapped, &sk).unwrap(), DEK);
    assert!(client.last_release_was_test());
    // the capture landed in the verified ring
    let verified = b.state.cfg.capture_dir().join("verified");
    assert_eq!(std::fs::read_dir(&verified).unwrap().count(), 1);
    // a replay of the same nonce is freshness (burned)
    let e = client
        .request_key(b.model_id, &evidence(&pk, nonce, None))
        .await
        .unwrap_err();
    assert!(matches!(e, TeeError::FreshnessFailure), "{e}");
}

#[tokio::test]
async fn error_kinds_and_codes() {
    let b = spawn_broker(&[], NrasOpts::default()).await;
    let client = node_client(&b);
    let (_, pk) = generate_ephemeral_keypair();
    // unknown model → no_provider (which the node's client maps to VerificationFailed("broker: no key/policy …"))
    let e = client.challenge([0x99; 32], &pk).await.unwrap_err();
    assert!(
        matches!(e, TeeError::VerificationFailed(ref m) if m.contains("no key/policy")),
        "{e}"
    );
    // unissued nonce → freshness
    let e = client
        .request_key(b.model_id, &evidence(&pk, [7; 32], None))
        .await
        .unwrap_err();
    assert!(matches!(e, TeeError::FreshnessFailure), "{e}");
    // a nonce issued for another key → freshness
    let (_, other) = generate_ephemeral_keypair();
    let nonce = client.challenge(b.model_id, &other).await.unwrap();
    let e = client
        .request_key(b.model_id, &evidence(&pk, nonce, None))
        .await
        .unwrap_err();
    assert!(matches!(e, TeeError::FreshnessFailure), "{e}");
    // a quote for the wrong pk_att → verification (identity row)
    let nonce = client.challenge(b.model_id, &pk).await.unwrap();
    let mut ev = evidence(&pk, nonce, None);
    ev.cpu_quote = patched_quote(&[0x03; 33], &nonce);
    let e = client.request_key(b.model_id, &ev).await.unwrap_err();
    assert!(
        matches!(e, TeeError::VerificationFailed(ref m) if m.contains("identity")),
        "{e}"
    );
    // every refused request_key (unissued nonce, other key, identity) left a preverify capture
    assert_eq!(
        std::fs::read_dir(b.state.cfg.capture_dir().join("preverify"))
            .unwrap()
            .count(),
        3
    );
    assert!(!b.state.cfg.capture_dir().join("verified").exists());
}

#[tokio::test]
async fn a_33_byte_non_point_pk_att_is_invalid_before_any_nonce_is_touched() {
    let b = spawn_broker(&[], NrasOpts::default()).await;
    let c = raw_client(&b);
    let base = base(&b);
    // 0x02 ‖ 32×0xff: x is above the field prime, not a point
    let bad = format!("02{}", "ff".repeat(32));
    let r = c
        .post(format!("{base}/challenge"))
        .json(&json!({"model_id": hex::encode(b.model_id), "pk_att": bad}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    assert_eq!(
        r.json::<serde_json::Value>().await.unwrap()["error"]["kind"],
        json!("invalid")
    );
    assert_eq!(b.state.nonces.outstanding(), 0, "nothing was issued");
    // request_key with a non-point key: 400 invalid, not 401 freshness (the burn comes after)
    let (_, pk) = generate_ephemeral_keypair();
    let client = node_client(&b);
    let nonce = client.challenge(b.model_id, &pk).await.unwrap();
    let mut ev = evidence(&pk, nonce, None);
    ev.pk_att = hex::decode(&bad).unwrap();
    let body = serde_json::to_value(RequestKeyRequest {
        model_id: hex::encode(b.model_id),
        evidence: EvidenceWire::encode(&ev).unwrap(),
    })
    .unwrap();
    let r = c
        .post(format!("{base}/request_key"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    assert_eq!(
        r.json::<serde_json::Value>().await.unwrap()["error"]["kind"],
        json!("invalid")
    );
    assert_eq!(
        b.state.nonces.outstanding(),
        1,
        "the issued nonce was not burned"
    );
    assert_eq!(b.nras.fake.hit_count(), 0);
}

#[tokio::test]
async fn a_keyring_model_without_a_policy_file_is_refused_at_challenge_before_any_nonce() {
    let b = spawn_broker(&[], NrasOpts::default()).await;
    let c = raw_client(&b);
    let base = base(&b);
    // remove the policy file behind the keyring entry
    std::fs::remove_file(b.state.policy.path_for(&b.model_id)).unwrap();
    let r = c.post(format!("{base}/challenge")).json(&json!({"model_id": hex::encode(b.model_id), "pk_att": hex::encode(generate_ephemeral_keypair().1)})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 404);
    assert_eq!(
        r.json::<serde_json::Value>().await.unwrap()["error"]["kind"],
        json!("no_provider")
    );
    assert_eq!(
        b.state.nonces.outstanding(),
        0,
        "no nonce was issued (mutation: check only at request_key → 200 here)"
    );
}

#[tokio::test]
async fn an_expired_or_foreign_signed_policy_is_refused_at_challenge_before_any_nonce() {
    // The policy file exists but request_key would refuse it; the node must not spend
    // a nonce and an evidence cycle to learn that (mutation: stat the file only → 200).
    let b = spawn_broker(&[], NrasOpts::default()).await;
    let c = raw_client(&b);
    let base = base(&b);
    let path = b.state.policy.path_for(&b.model_id);
    let ask = || {
        c.post(format!("{base}/challenge")).json(&json!({"model_id": hex::encode(b.model_id), "pk_att": hex::encode(generate_ephemeral_keypair().1)}))
    };
    // expired window, right signer
    let mut expired = recording_policy(b.model_id, 1);
    expired.expiry = expired.not_before + 1;
    let (signed, _) = sign(&expired, &b.signer);
    std::fs::write(&path, serde_json::to_vec(&signed).unwrap()).unwrap();
    let r = ask().send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    assert_eq!(
        r.json::<serde_json::Value>().await.unwrap()["error"]["kind"],
        json!("verification")
    );
    // valid window, a signer the keyring does not bind
    let (foreign, _) = sign(&recording_policy(b.model_id, 1), &signer());
    std::fs::write(&path, serde_json::to_vec(&foreign).unwrap()).unwrap();
    let r = ask().send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    // a half-written file is the broker's fault, not a permanent refusal
    std::fs::write(&path, b"{\"policy\":").unwrap();
    let r = ask().send().await.unwrap();
    assert_eq!(r.status().as_u16(), 500);
    assert_eq!(
        b.state.nonces.outstanding(),
        0,
        "no nonce was issued on any refusal"
    );
    // the good policy back: a nonce is issued
    let (good, _) = sign(&recording_policy(b.model_id, 1), &b.signer);
    std::fs::write(&path, serde_json::to_vec(&good).unwrap()).unwrap();
    assert_eq!(ask().send().await.unwrap().status().as_u16(), 200);
    assert_eq!(b.state.nonces.outstanding(), 1);
}

#[tokio::test]
async fn raw_wire_invalid_400_413_unknown_field_and_info() {
    let b = spawn_broker(&[("KBS_MAX_BODY_BYTES", "2048")], NrasOpts::default()).await;
    let c = raw_client(&b);
    let base = base(&b);
    // unknown field → 400 invalid (mutation: drop deny_unknown_fields → the challenge succeeds)
    let r = c
        .post(format!("{base}/challenge"))
        .json(&json!({"model_id": hex::encode(b.model_id), "pk_att": "02".repeat(33), "extra": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["error"]["kind"], json!("invalid"));
    // syntax error → 400 invalid
    let r = c
        .post(format!("{base}/challenge"))
        .header("content-type", "application/json")
        .body("{not json")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    // missing content type → 400 invalid (not axum's 415)
    let r = c
        .post(format!("{base}/challenge"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    assert_eq!(
        r.json::<serde_json::Value>().await.unwrap()["error"]["kind"],
        json!("invalid")
    );
    // over the body limit → 413 invalid
    let big = json!({"model_id": hex::encode(b.model_id), "pk_att": "02".repeat(33), "note": "x".repeat(4096)});
    let r = c
        .post(format!("{base}/challenge"))
        .json(&big)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 413);
    assert_eq!(
        r.json::<serde_json::Value>().await.unwrap()["error"]["kind"],
        json!("invalid")
    );
    // /info
    let r = c.get(format!("{base}/info")).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let info: serde_json::Value = r.json().await.unwrap();
    assert_eq!(info["keyring"], json!("test"));
    assert_eq!(info["cpu_evidence"], json!("simulator"));
    assert_eq!(info["gpu_evidence"], json!("real"));
    assert_eq!(info["nonce_ttl_seconds"], json!(300));
    assert_eq!(info["nras_claims_version"], json!("2.0"));
    assert!(info["version"].as_str().unwrap().starts_with("v"));
}

#[test]
fn mirror_structs_encode_identically_to_the_nodes() {
    let node = RequestKeyRequest {
        model_id: "ab".repeat(32),
        evidence: EvidenceWire {
            gpu_report_b64: "Zw==".into(),
            cpu_quote_hex: "00".into(),
            event_log: "[]".into(),
            vm_config: "{}".into(),
            image_measurement_hex: "00".repeat(48),
            pk_att_hex: "02".repeat(33),
            nonce_hex: "11".repeat(32),
        },
    };
    let mirror = RequestKeyRequestWire {
        model_id: node.model_id.clone(),
        evidence: EvidenceWireIn {
            gpu_report_b64: node.evidence.gpu_report_b64.clone(),
            cpu_quote_hex: node.evidence.cpu_quote_hex.clone(),
            event_log: node.evidence.event_log.clone(),
            vm_config: node.evidence.vm_config.clone(),
            image_measurement_hex: node.evidence.image_measurement_hex.clone(),
            pk_att_hex: node.evidence.pk_att_hex.clone(),
            nonce_hex: node.evidence.nonce_hex.clone(),
        },
    };
    assert_eq!(
        serde_json::to_value(&node).unwrap(),
        serde_json::to_value(&mirror).unwrap()
    );
    let back: RequestKeyRequestWire =
        serde_json::from_value(serde_json::to_value(&node).unwrap()).unwrap();
    assert_eq!(back.into_node().model_id, node.model_id);
    let mut with_extra = serde_json::to_value(&node).unwrap();
    with_extra["evidence"]["extra"] = json!(1);
    assert!(
        serde_json::from_value::<RequestKeyRequestWire>(with_extra).is_err(),
        "nested unknown field refused"
    );
    // P4.5: the node's lenient `/info` mirror reads the broker's `InfoResponse`
    // field for field (every field populated non-default and compared: with the
    // node's serde defaults, a rename on either side would come back ""/0).
    let info = fabstir_llm_node::kbs::routes::InfoResponse {
        keyring: "test".into(),
        gpu_evidence: "canned".into(),
        cpu_evidence: "simulator".into(),
        nonce_ttl_seconds: 123,
        nras_claims_version: "2.0".into(),
        version: "v-x".into(),
    };
    let node: fabstir_llm_node::tee::kbs_http::BrokerInfo =
        serde_json::from_slice(&serde_json::to_vec(&info).unwrap()).unwrap();
    assert_eq!(node.keyring, "test");
    assert_eq!(node.gpu_evidence, "canned");
    assert_eq!(node.cpu_evidence, "simulator");
    assert_eq!(node.nonce_ttl_seconds, 123);
    assert_eq!(node.nras_claims_version, "2.0");
    assert_eq!(node.version, "v-x");
}

#[tokio::test]
async fn a_claim_table_refusal_captures_the_raw_nras_answer() {
    let b = spawn_broker(
        &[],
        NrasOpts {
            per_gpu_edit: Some(Arc::new(|m| {
                m.remove("hwmodel");
            })),
            ..Default::default()
        },
    )
    .await;
    let client = node_client(&b);
    let (_, pk) = generate_ephemeral_keypair();
    let nonce = client.challenge(b.model_id, &pk).await.unwrap();
    let e = client
        .request_key(b.model_id, &evidence(&pk, nonce, None))
        .await
        .unwrap_err();
    assert!(
        matches!(e, TeeError::VerificationFailed(ref m) if m.contains("hwmodel")),
        "{e}"
    );
    let verified = b.state.cfg.capture_dir().join("verified");
    let dir = std::fs::read_dir(&verified)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(
        dir.join("nras.json").is_file(),
        "the raw NRAS answer is in the verified ring on a claim refusal"
    );
    assert!(std::fs::read_to_string(dir.join("decision.txt"))
        .unwrap()
        .contains("hwmodel"));
}

#[tokio::test]
async fn canned_label_under_real_mode_is_refused_with_zero_nras_hits() {
    let b = spawn_broker(&[], NrasOpts::default()).await;
    let client = node_client(&b);
    let (_, pk) = generate_ephemeral_keypair();
    let nonce = client.challenge(b.model_id, &pk).await.unwrap();
    let e = client
        .request_key(b.model_id, &evidence(&pk, nonce, Some(json!(true))))
        .await
        .unwrap_err();
    assert!(
        matches!(e, TeeError::VerificationFailed(ref m) if m.contains("canned evidence under real mode")),
        "{e}"
    );
    assert_eq!(b.nras.fake.hit_count(), 0, "refused before any egress");
}

#[tokio::test]
async fn busy_global_per_source_and_wall_exceeded() {
    // A fake NRAS that stalls 3 s on the POST; concurrency 2, per-source 1, wall 2 s.
    let b = spawn_broker(
        &[
            ("KBS_REQUEST_CONCURRENCY", "2"),
            ("KBS_INFLIGHT_PER_SOURCE_CAP", "1"),
            ("KBS_REQUEST_WALL_SECS", "2"),
            ("KBS_NRAS_TIMEOUT_SECS", "10"),
        ],
        NrasOpts {
            stall: Some(Duration::from_secs(3)),
            ..Default::default()
        },
    )
    .await;
    let c = raw_client(&b);
    let base = base(&b);
    let client = node_client(&b);
    let mk = |pk: &[u8], nonce: [u8; 32]| -> serde_json::Value {
        let ev = evidence(pk, nonce, None);
        serde_json::to_value(RequestKeyRequest {
            model_id: hex::encode(b.model_id),
            evidence: EvidenceWire::encode(&ev).unwrap(),
        })
        .unwrap()
    };
    let (_, pk1) = generate_ephemeral_keypair();
    let n1 = client.challenge(b.model_id, &pk1).await.unwrap();
    let (_, pk2) = generate_ephemeral_keypair();
    let n2 = client.challenge(b.model_id, &pk2).await.unwrap();
    let (_, pk3) = generate_ephemeral_keypair();
    let n3 = client.challenge(b.model_id, &pk3).await.unwrap();

    // source A: one in flight (stalls at NRAS), a second from A → 503 busy
    let a1 = {
        let c = c.clone();
        let body = mk(&pk1, n1);
        let url = format!("{base}/request_key");
        tokio::spawn(async move {
            c.post(url)
                .header("x-forwarded-for", "10.0.0.1")
                .json(&body)
                .send()
                .await
                .unwrap()
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let r = c
        .post(format!("{base}/request_key"))
        .header("x-forwarded-for", "10.0.0.1")
        .json(&mk(&pk2, n2))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 503, "per-source busy");
    assert_eq!(
        r.json::<serde_json::Value>().await.unwrap()["error"]["kind"],
        json!("unavailable")
    );
    // source B proceeds (second global permit) and stalls too
    let b1 = {
        let c = c.clone();
        let body = mk(&pk3, n3);
        let url = format!("{base}/request_key");
        tokio::spawn(async move {
            c.post(url)
                .header("x-forwarded-for", "10.0.0.2")
                .json(&body)
                .send()
                .await
                .unwrap()
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    // source C: both global permits held → 503 busy
    let r = c
        .post(format!("{base}/request_key"))
        .header("x-forwarded-for", "10.0.0.3")
        .json(&mk(&pk2, n2))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 503, "global busy");
    // the stalled ones hit the 2 s wall: 502 unavailable "request wall exceeded"
    let ra = a1.await.unwrap();
    assert_eq!(ra.status().as_u16(), 502);
    let body: serde_json::Value = ra.json().await.unwrap();
    assert_eq!(body["error"]["kind"], json!("unavailable"));
    assert_eq!(
        body["error"]["detail"],
        json!("request wall exceeded during gpu (NRAS)"),
        "the detail names the step the wall caught"
    );
    let rb = b1.await.unwrap();
    assert_eq!(rb.status().as_u16(), 502);
    // both stalled at the GPU half with no verdict: an outage's captures go to the
    // preverify ring, so a node looping through an outage can never evict the first
    // real EAT from `verified/` (mutation: route by stage → 2 in verified)
    assert_eq!(
        std::fs::read_dir(b.state.cfg.capture_dir().join("preverify"))
            .unwrap()
            .count(),
        2
    );
    assert!(
        std::fs::read_dir(b.state.cfg.capture_dir().join("verified"))
            .map(|r| r.count())
            .unwrap_or(0)
            == 0
    );
}

#[tokio::test]
async fn an_nras_outage_is_captured_in_preverify_and_a_refusal_in_verified() {
    // Two 503s exhaust the single retry: no verdict → preverify (with the 503 body);
    // a 400 is NRAS's verdict on the evidence → verified (mutation: one ring for the
    // whole GPU half → the outage lands in verified).
    let b = spawn_broker(
        &[],
        NrasOpts {
            fail_first: 2,
            ..Default::default()
        },
    )
    .await;
    let client = node_client(&b);
    let (_, pk) = generate_ephemeral_keypair();
    let nonce = client.challenge(b.model_id, &pk).await.unwrap();
    let e = client
        .request_key(b.model_id, &evidence(&pk, nonce, None))
        .await
        .unwrap_err();
    assert!(
        matches!(e, TeeError::Kbs(ref m) if m.contains("unavailable") && m.contains("503")),
        "{e}"
    );
    let cap = b.state.cfg.capture_dir();
    assert_eq!(std::fs::read_dir(cap.join("preverify")).unwrap().count(), 1);
    assert!(
        std::fs::read_dir(cap.join("verified"))
            .map(|r| r.count())
            .unwrap_or(0)
            == 0
    );
    let dir = std::fs::read_dir(cap.join("preverify"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(
        dir.join("nras.json").is_file(),
        "the 503 body is still captured"
    );

    let b = spawn_broker(
        &[],
        NrasOpts {
            fail_first_status: Some((1, 400)),
            ..Default::default()
        },
    )
    .await;
    let client = node_client(&b);
    let (_, pk) = generate_ephemeral_keypair();
    let nonce = client.challenge(b.model_id, &pk).await.unwrap();
    let e = client
        .request_key(b.model_id, &evidence(&pk, nonce, None))
        .await
        .unwrap_err();
    assert!(matches!(e, TeeError::VerificationFailed(_)), "{e}");
    let cap = b.state.cfg.capture_dir();
    assert_eq!(std::fs::read_dir(cap.join("verified")).unwrap().count(), 1);
}

#[tokio::test]
async fn header_absent_falls_back_to_the_peer_address() {
    let b = spawn_broker(&[("KBS_NONCE_PER_SOURCE_CAP", "1")], NrasOpts::default()).await;
    let c = raw_client(&b);
    let base = base(&b);
    let body = json!({"model_id": hex::encode(b.model_id), "pk_att": "02".repeat(33)});
    // no X-Forwarded-For: the source is the loopback peer; the second challenge trips the per-source cap
    let r = c
        .post(format!("{base}/challenge"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let r = c
        .post(format!("{base}/challenge"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 401);
    assert_eq!(
        r.json::<serde_json::Value>().await.unwrap()["error"]["kind"],
        json!("freshness")
    );
    // a different forwarded source is a different budget
    let r = c
        .post(format!("{base}/challenge"))
        .header("x-forwarded-for", "1.2.3.4, 10.9.9.9")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
}

#[test]
fn source_of_honours_the_header_only_from_a_loopback_peer() {
    use fabstir_llm_node::kbs::routes::source_of;
    let mut h = axum::http::HeaderMap::new();
    h.insert("x-forwarded-for", "1.2.3.4, 10.9.9.9".parse().unwrap());
    let lo: SocketAddr = "127.0.0.1:5000".parse().unwrap();
    let remote: SocketAddr = "10.0.0.5:5000".parse().unwrap();
    assert_eq!(
        source_of(&h, lo),
        "10.9.9.9",
        "last element behind the local proxy"
    );
    assert_eq!(
        source_of(&h, remote),
        "10.0.0.5",
        "a direct peer never names its own source"
    );
    assert_eq!(source_of(&axum::http::HeaderMap::new(), lo), "127.0.0.1");
    // a dual-stack listener sees the local proxy as the IPv4-mapped loopback
    let mapped: SocketAddr = "[::ffff:127.0.0.1]:5000".parse().unwrap();
    assert_eq!(source_of(&h, mapped), "10.9.9.9");
    let v6lo: SocketAddr = "[::1]:5000".parse().unwrap();
    assert_eq!(source_of(&h, v6lo), "10.9.9.9");
}

#[test]
fn release_gate_is_its_own_check() {
    let v = Verified {
        tcb_status: "Simulator".into(),
        advisory_ids: vec![],
        hwmodel: None,
        driver_version: None,
        vbios_version: None,
        cc_mode: CcRecord::Canned,
        td_debug_off: true,
    };
    assert!(release_gate(&v, true).is_ok());
    let e = release_gate(&v, false).unwrap_err();
    assert!(e.detail.contains("release gate"), "{e}");
    let real = Verified {
        cc_mode: CcRecord::NodeAsserted,
        ..v
    };
    assert!(release_gate(&real, false).is_ok());
}
