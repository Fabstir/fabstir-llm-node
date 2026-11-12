// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Key handling, the staging chain, the canary and the reconciliation.
mod common;
use common::*;
use provenance_signer::keystore::{encrypt_pkcs8_pem, KeyError, KeyStore};
use provenance_signer::notify::Recorder;
use provenance_signer::reconcile::{reconcile, ProofCounter};
use provenance_signer::{canary, staging_chain};
use std::collections::BTreeMap;
use std::io::{Read, Write};

/// ethers.id("ProofSubmitted(uint256,address,uint256,bytes32,string)"), computed independently (ethers v6).
const PROOF_SUBMITTED_TOPIC: &str =
    "0x0b81f46766672facece96cfcc36bf38c5d7b862ba7174b1ac0b33864b18d3dea";

#[test]
fn key_file_is_encrypted() {
    let sk = p256::ecdsa::SigningKey::random(&mut rand::rngs::OsRng);
    let plain =
        p256::pkcs8::EncodePrivateKey::to_pkcs8_pem(&sk, p256::pkcs8::LineEnding::LF).unwrap();
    assert_eq!(
        KeyStore::from_encrypted_pem(&plain, PASS).unwrap_err(),
        KeyError::NotEncrypted
    );
    let sec1 = p256::SecretKey::from(sk.clone())
        .to_sec1_pem(p256::pkcs8::LineEnding::LF)
        .unwrap();
    assert_eq!(
        KeyStore::from_encrypted_pem(&sec1, PASS).unwrap_err(),
        KeyError::NotEncrypted
    );
    let enc = encrypt_pkcs8_pem(&sk, PASS).unwrap();
    assert!(enc.starts_with("-----BEGIN ENCRYPTED PRIVATE KEY-----"));
    assert_eq!(
        KeyStore::from_encrypted_pem(&enc, b"wrong passphrase!!").unwrap_err(),
        KeyError::Decrypt
    );
    assert_eq!(
        KeyStore::from_encrypted_pem(&enc, b"").unwrap_err(),
        KeyError::NoPassphrase
    );
    let ks = KeyStore::from_encrypted_pem(&enc, PASS).unwrap();
    // signs with the right key
    use p256::ecdsa::signature::Verifier;
    let sig = p256::ecdsa::Signature::from_slice(&ks.sign(b"claim").unwrap()).unwrap();
    sk.verifying_key().verify(b"claim", &sig).unwrap();
    // the in-memory form is sealed: the scalar never appears in the struct's bytes or Debug
    let dbg = format!("{ks:?}");
    assert!(!dbg.contains(&hex::encode(sk.to_bytes())));
}

#[test]
fn staging_chain_meets_the_c2pa_profile_and_never_writes_a_plain_key() {
    let c = staging_chain::generate(true, PASS, 397, None).unwrap();
    assert!(c.intermediate_pem.is_some());
    assert_eq!(
        pem::parse_many(c.chain_pem.as_bytes()).unwrap().len(),
        2,
        "EE + intermediate, root excluded"
    );
    let dir = tempfile::tempdir().unwrap();
    staging_chain::write(dir.path(), &c).unwrap();
    for f in std::fs::read_dir(dir.path()).unwrap() {
        let t = std::fs::read_to_string(f.unwrap().path()).unwrap();
        assert!(!t.contains("BEGIN PRIVATE KEY") && !t.contains("BEGIN EC PRIVATE KEY"));
    }
    assert!(
        staging_chain::write(dir.path(), &c).is_err(),
        "never overwrites"
    );
    assert!(staging_chain::generate(false, b"short", 397, None).is_err());
}

#[test]
fn staging_chain_signs_trusted_manifests() {
    // the default (root-only) env already signs; this checks the intermediate variant end to end
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    let s = e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap();
    let out = fabstir_provenance::flow::patch(&p.prep, &s.manifest).unwrap();
    let (_, ok, bad, _) = validate(&out, "video/mp4", &e.root_pem);
    assert!(bad.is_empty(), "{bad:?}");
    assert!(ok.iter().any(|c| c == "signingCredential.trusted"));
}

#[test]
fn canary_alarms_on_validation_failure() {
    let e = env();
    let other_root = staging_chain::generate(false, PASS, 397, None)
        .unwrap()
        .root_pem;
    let rec = Recorder::default();
    let dir = tempfile::tempdir().unwrap();
    let rep = canary::run(&e.svc, &other_root, &[], dir.path(), &rec);
    assert!(!rep.ok);
    assert!(
        rep.missing.iter().any(|m| m == "signingCredential.trusted"),
        "{rep:?}"
    );
    assert_eq!(rec.alarms.lock().unwrap().len(), 1);
}

#[test]
fn canary_requires_a_trusted_timestamp() {
    // our own root trusted, but the local TSA is not on the anchors: timeStamp.trusted missing -> alarm
    let e = env();
    let rec = Recorder::default();
    let dir = tempfile::tempdir().unwrap();
    let rep = canary::run(&e.svc, &e.root_pem, &[], dir.path(), &rec);
    assert!(!rep.ok, "{rep:?}");
    assert_eq!(
        rep.missing,
        vec!["timeStamp.trusted".to_string()],
        "{rep:?}"
    );
    assert_eq!(rec.alarms.lock().unwrap().len(), 1);
}

#[test]
fn canary_passes_with_a_trusted_chain_and_tsa() {
    let e = env();
    let rec = Recorder::default();
    let dir = tempfile::tempdir().unwrap();
    let anchors = format!("{}{}", e.root_pem, e.tsa.ca_pem);
    let rep = canary::run(&e.svc, &anchors, &[], dir.path(), &rec);
    assert!(rep.ok, "{rep:?}");
    assert!(rec.alarms.lock().unwrap().is_empty());
}

#[test]
fn canary_failing_external_validator_alarms() {
    let e = env();
    let rec = Recorder::default();
    let dir = tempfile::tempdir().unwrap();
    let anchors = format!("{}{}", e.root_pem, e.tsa.ca_pem);
    let rep = canary::run(&e.svc, &anchors, &["false".to_string()], dir.path(), &rec);
    assert!(
        rep.failures.iter().any(|f| f.starts_with("false")),
        "{rep:?}"
    );
    assert!(
        rep.missing.is_empty(),
        "only the external validator fails: {rep:?}"
    );
    assert!(!rep.ok);
    assert_eq!(rec.alarms.lock().unwrap().len(), 1);
}

struct Fake(BTreeMap<String, u64>);
impl ProofCounter for Fake {
    fn count(&self, _hosts: &[String]) -> anyhow::Result<(u64, u64, BTreeMap<String, u64>)> {
        Ok((T0, T0 + 86_400, self.0.clone()))
    }
}

#[test]
fn reconciliation_alarms_on_count_mismatch() {
    let e = env();
    for _ in 0..2 {
        let r = e.reserve("ltx-t2v-hdr", false);
        let p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
        e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap();
    }
    let hosts = vec![e.host()];
    let sigs = |a, b| e.svc.store.signatures_per_host(a, b);
    let rec = Recorder::default();
    let m = reconcile(
        &Fake([(e.host(), 2)].into_iter().collect()),
        sigs,
        &hosts,
        &rec,
    )
    .unwrap();
    assert!(m.is_empty());
    assert!(rec.alarms.lock().unwrap().is_empty());
    let m = reconcile(
        &Fake([(e.host(), 3)].into_iter().collect()),
        sigs,
        &hosts,
        &rec,
    )
    .unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!((m[0].proofs, m[0].signatures), (3, 2));
    assert_eq!(rec.alarms.lock().unwrap().len(), 1);
    // a host with signatures and no proofs at all
    let m = reconcile(&Fake(BTreeMap::new()), sigs, &hosts, &rec).unwrap();
    assert_eq!(m.len(), 1);
}

#[test]
fn proof_submitted_topic_matches_the_abi() {
    // keccak256("ProofSubmitted(uint256,address,uint256,bytes32,string)") per contracts/JobMarketplaceWithModels-CLIENT-ABI.json
    let t = provenance_signer::reconcile::proof_submitted_topic();
    assert_eq!(t.len(), 66);
    assert_eq!(
        t,
        format!(
            "0x{}",
            hex::encode(fabstir_provenance::contract::keccak(
                b"ProofSubmitted(uint256,address,uint256,bytes32,string)"
            ))
        )
    );
}

/// A JSON-RPC stand-in: head 20_000, timestamps = block number * 2, two ProofSubmitted logs for the host in
/// the first chunk, one in a later chunk, one for another host (filtered by the topic list the counter sends).
fn mock_rpc(
    host: &str,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    let seen = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let seen2 = seen.clone();
    let host_topic = format!("0x{host:0>64}");
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut buf = vec![];
            let mut tmp = [0u8; 8192];
            loop {
                let n = s.read(&mut tmp).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let h = String::from_utf8_lossy(&buf[..p]).to_ascii_lowercase();
                    let len: usize = h
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    while buf.len() < p + 4 + len {
                        let n = s.read(&mut tmp).unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let req: serde_json::Value =
                        serde_json::from_slice(&buf[p + 4..p + 4 + len]).unwrap();
                    seen2.lock().unwrap().push(req.clone());
                    let result = match req["method"].as_str().unwrap() {
                        "eth_blockNumber" => serde_json::json!("0x4e20"),
                        "eth_getBlockByNumber" => {
                            let n = u64::from_str_radix(
                                req["params"][0].as_str().unwrap().trim_start_matches("0x"),
                                16,
                            )
                            .unwrap();
                            serde_json::json!({"timestamp": format!("0x{:x}", n * 2)})
                        }
                        "eth_getLogs" => {
                            let from = u64::from_str_radix(
                                req["params"][0]["fromBlock"]
                                    .as_str()
                                    .unwrap()
                                    .trim_start_matches("0x"),
                                16,
                            )
                            .unwrap();
                            let log = serde_json::json!({"topics": [PROOF_SUBMITTED_TOPIC, "0x01", host_topic]});
                            if from == 0 {
                                serde_json::json!([log, log])
                            } else if from == 15_000 {
                                serde_json::json!([log])
                            } else {
                                serde_json::json!([])
                            }
                        }
                        m => panic!("unexpected {m}"),
                    };
                    let body = serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": result})
                        .to_string();
                    let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes());
                    break;
                }
            }
        }
    });
    (url, seen)
}

#[test]
fn rpc_counter_counts_proofs_per_host_across_chunks() {
    let host = "1a642f0e3c3af545e7acbd38b07251b3990914f1".to_string();
    let (url, seen) = mock_rpc(&host);
    let c = provenance_signer::reconcile::RpcCounter {
        rpc_url: url,
        job_marketplace: "0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4".into(),
        window_blocks: 43_200,
        chunk_blocks: 5_000,
    };
    let (t0, t1, counts) = c.count(std::slice::from_ref(&host)).unwrap();
    assert_eq!((t0, t1), (0, 40_001));
    assert_eq!(counts.get(&host), Some(&3));
    let logs: Vec<_> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r["method"] == "eth_getLogs")
        .cloned()
        .collect();
    assert_eq!(logs.len(), 5, "blocks 0..=20,000 in 5,000-block chunks");
    assert_eq!(logs[0]["params"][0]["topics"][0], PROOF_SUBMITTED_TOPIC);
    assert_eq!(
        logs[0]["params"][0]["topics"][2][0],
        format!("0x{host:0>64}")
    );
}

#[test]
fn reconciliation_tolerates_small_differences_only() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap();
    let hosts = vec![e.host()];
    let sigs = |a, b| e.svc.store.signatures_per_host(a, b);
    let rec = Recorder::default();
    let m = provenance_signer::reconcile::reconcile_with_tolerance(
        &Fake([(e.host(), 3)].into_iter().collect()),
        sigs,
        &hosts,
        2,
        &rec,
    )
    .unwrap();
    assert!(
        m.is_empty() && rec.alarms.lock().unwrap().is_empty(),
        "1 vs 3 is within 2"
    );
    let m = provenance_signer::reconcile::reconcile_with_tolerance(
        &Fake([(e.host(), 4)].into_iter().collect()),
        sigs,
        &hosts,
        2,
        &rec,
    )
    .unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(rec.alarms.lock().unwrap().len(), 1);
}

#[test]
fn reconciliation_alarm_never_carries_the_rpc_url() {
    struct Broken;
    impl ProofCounter for Broken {
        fn count(&self, _: &[String]) -> anyhow::Result<(u64, u64, BTreeMap<String, u64>)> {
            anyhow::bail!("https://base-sepolia.example/v2/SECRETKEY: connection refused")
        }
    }
    let rec = Recorder::default();
    assert!(reconcile(&Broken, |_, _| Ok(vec![]), &[], &rec).is_err());
    let a = rec.alarms.lock().unwrap();
    assert_eq!(a.len(), 1);
    assert!(!a[0].1.contains("SECRETKEY") && !a[0].1.contains("https://"));
}
