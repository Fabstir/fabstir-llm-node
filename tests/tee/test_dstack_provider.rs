// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P2.5 — `DstackAttestationProvider` end to end against a fake guest
//! agent (Unix socket) and a fake collector script: the CPU half is asked to
//! sign exactly `sha256(pk_att) ‖ nonce`, the GPU half is collected under the
//! same nonce, both land in `Evidence` unaltered, and either half failing means
//! no evidence at all. Split from `test_gpu_evidence.rs` (400-line cap).

use fabstir_llm_node::tee::dstack::{DstackClient, Endpoint};
use fabstir_llm_node::tee::dstack_provider::DstackAttestationProvider;
use fabstir_llm_node::tee::gpu_evidence::{GpuEvidenceCollector, GpuEvidenceMode};
use fabstir_llm_node::tee::provider::AttestationProvider;
use fabstir_llm_node::tee::types::{report_data, TeeError};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const NONCE: [u8; 32] = [0x42u8; 32];
const PK_ATT: [u8; 33] = [0x02u8; 33];

/// A scratch directory removed when the test ends.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tmpdir() -> Scratch {
    use rand::RngCore;
    let mut b = [0u8; 4];
    rand::rngs::OsRng.fill_bytes(&mut b);
    let d = std::env::temp_dir().join(format!(
        "tee-dsprov-{}-{}",
        std::process::id(),
        hex::encode(b)
    ));
    std::fs::create_dir_all(&d).unwrap();
    Scratch(d)
}

/// A fake collector script that echoes argv[1] as the nonce.
const GOOD: &str = r#"
nonce = sys.argv[1]
payload = {"nonce": nonce, "evidence_list": [{"certificate": "Y2VydA==", "evidence": "ZXY=", "arch": "HOPPER"}], "arch": "HOPPER"}
if os.environ.get("TEE_GPU_EVIDENCE") == "canned":
    payload["canned"] = True
print(json.dumps(payload))
"#;

/// Returns the collector and the guard that keeps its script alive.
fn collector(body: &str) -> (GpuEvidenceCollector, Scratch) {
    let dir = tmpdir();
    let p = dir.0.join("collect.py");
    std::fs::write(&p, format!("import sys, os, json\n{body}\n")).unwrap();
    (
        GpuEvidenceCollector::new("python3", p, GpuEvidenceMode::Real, Duration::from_secs(20)),
        dir,
    )
}

type Handler = Arc<dyn Fn(&str, serde_json::Value) -> (StatusCode, String) + Send + Sync>;

fn spawn_fake_agent(handler: Handler) -> (PathBuf, Scratch) {
    let dir = tmpdir();
    let sock = dir.0.join("dstack.sock");
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                let svc = service_fn(move |req: Request<Incoming>| {
                    let handler = handler.clone();
                    async move {
                        let path = req.uri().path().to_string();
                        let body = req.into_body().collect().await.unwrap().to_bytes();
                        let json: serde_json::Value =
                            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
                        let (status, out) = handler(&path, json);
                        Ok::<_, std::convert::Infallible>(
                            Response::builder()
                                .status(status)
                                .body(Full::new(Bytes::from(out)))
                                .unwrap(),
                        )
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .await;
            });
        }
    });
    (sock, dir)
}

#[tokio::test]
async fn provider_binds_both_halves_to_the_same_challenge() {
    // The fake agent asserts the report_data it is asked to sign is exactly
    // sha256(pk_att) ‖ nonce, and returns a "quote"; the fake collector echoes
    // the same nonce. The provider must package all four pieces unaltered.
    let expected_rd = hex::encode(report_data(&PK_ATT, &NONCE));
    let (sock, _agent_dir) = spawn_fake_agent(Arc::new(move |path, body| {
        assert_eq!(path, "/GetQuote");
        assert_eq!(body["report_data"].as_str().unwrap(), expected_rd);
        (
            StatusCode::OK,
            serde_json::json!({
                "quote": "0400020081",
                "event_log": "[{\"imr\":3,\"event_type\":1,\"digest\":\"aa\",\"event\":\"compose-hash\",\"event_payload\":\"bb\"}]",
                "report_data": expected_rd,
                "vm_config": "{\"cpu_count\":4,\"num_gpus\":1}"
            })
            .to_string(),
        )
    }));
    let (gpu, _script) = collector(GOOD);
    let provider = DstackAttestationProvider::new(
        DstackClient::new(Endpoint::Unix(sock), Duration::from_secs(5)),
        gpu,
    );
    let ev = provider
        .gather_evidence(NONCE, &PK_ATT)
        .await
        .expect("gather_evidence");
    assert_eq!(ev.cpu_quote, vec![0x04, 0x00, 0x02, 0x00, 0x81]);
    assert_eq!(ev.nonce, NONCE);
    assert_eq!(ev.pk_att, PK_ATT.to_vec());
    assert!(String::from_utf8_lossy(&ev.event_log).contains("compose-hash"));
    assert!(String::from_utf8_lossy(&ev.vm_config).contains("num_gpus"));
    let payload: serde_json::Value = serde_json::from_slice(&ev.gpu_report).unwrap();
    assert_eq!(payload["nonce"], hex::encode(NONCE));
    assert_eq!(
        ev.image_measurement, [0u8; 48],
        "node never asserts a measurement"
    );
}

#[tokio::test]
async fn provider_fails_closed_when_either_half_fails() {
    // Dead socket: no evidence, no GPU round trip attempted after it.
    let (gpu, _script) = collector(GOOD);
    let provider = DstackAttestationProvider::new(
        DstackClient::new(
            Endpoint::Unix(PathBuf::from("/nonexistent/dstack.sock")),
            Duration::from_secs(2),
        ),
        gpu,
    );
    assert!(matches!(
        provider.gather_evidence(NONCE, &PK_ATT).await,
        Err(TeeError::Dstack(_))
    ));

    // Good socket, collector refuses (e.g. PPCIe): still no evidence.
    let (sock, _agent_dir) = spawn_fake_agent(Arc::new(|_, body| {
        (
            StatusCode::OK,
            serde_json::json!({"quote": "0400", "event_log": "[]", "report_data": body["report_data"], "vm_config": ""}).to_string(),
        )
    }));
    let (gpu, _script) = collector("sys.exit(75)");
    let provider = DstackAttestationProvider::new(
        DstackClient::new(Endpoint::Unix(sock), Duration::from_secs(5)),
        gpu,
    );
    assert!(matches!(
        provider.gather_evidence(NONCE, &PK_ATT).await,
        Err(TeeError::GpuEvidence(_))
    ));
}

#[tokio::test]
async fn provider_refuses_a_non_compressed_pk_att_before_spending_a_quote() {
    let (sock, _agent_dir) = spawn_fake_agent(Arc::new(|_, _| {
        panic!("the agent must not be called for a malformed pk_att")
    }));
    let (gpu, _script) = collector(GOOD);
    let provider = DstackAttestationProvider::new(
        DstackClient::new(Endpoint::Unix(sock), Duration::from_secs(5)),
        gpu,
    );
    assert!(matches!(
        provider.gather_evidence(NONCE, &[0x04u8; 65]).await,
        Err(TeeError::Crypto(_))
    ));
}

/// Gate A-5 (canned half) against the REAL nvtrust package and the REAL
/// collector script, not a fake: `TEE_NVTRUST_PYTHON=<python with
/// nv-local-gpu-verifier==2.6.3 installed>` selects the interpreter. Ignored
/// by default because the dev container has no nvtrust on PATH.
///
///   TEE_NVTRUST_PYTHON=/path/venv/bin/python cargo test --test tee_tests -- --ignored real_nvtrust
#[tokio::test]
#[ignore = "needs a python with nv-local-gpu-verifier==2.6.3; set TEE_NVTRUST_PYTHON"]
async fn real_nvtrust_canned_collection_is_one_json_line_with_our_nonce() {
    let python = std::env::var("TEE_NVTRUST_PYTHON").expect("set TEE_NVTRUST_PYTHON");
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("deployment/phala/collect_gpu_evidence.py");
    let c = GpuEvidenceCollector::new(
        python,
        script,
        GpuEvidenceMode::Canned,
        Duration::from_secs(60),
    );
    let bytes = c
        .collect(&NONCE)
        .await
        .expect("real nvtrust canned collection");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("one JSON line");
    assert_eq!(v["nonce"], hex::encode(NONCE));
    assert_eq!(v["canned"], true);
    assert_eq!(v["arch"], "HOPPER");
    let entry = &v["evidence_list"].as_array().expect("evidence_list")[0];
    assert_eq!(entry["arch"], "HOPPER");
    assert!(
        entry["certificate"].as_str().unwrap().len() > 1000,
        "base64 cert chain"
    );
    assert!(
        entry["evidence"].as_str().unwrap().len() > 1000,
        "base64 attestation report"
    );
}

/// P4.5 (design P8): the REAL collector script's real branch, driven through
/// `GpuEvidenceCollector`, against a GENERATED stub `verifier` package placed
/// beside a copy of the script (the script's directory is `sys.path[0]`, so the
/// stub resolves with no `PYTHONPATH`; the collector clears the child's
/// environment, so the three NVML states are baked into the stub per case).
/// The stub echoes its nonce into `evidence` and reports `arch` by the
/// `no_gpu_mode` it was called with, so the one dangerous mutation (real mode
/// asking nvtrust for sample evidence) is visible.
fn stubbed_collector(cc_on: bool, ppcie: bool, devtools: bool) -> (GpuEvidenceCollector, Scratch) {
    let s = tmpdir();
    let real =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("deployment/phala/collect_gpu_evidence.py");
    let script = s.0.join("collect_gpu_evidence.py");
    std::fs::copy(&real, &script).unwrap();
    let pkg = s.0.join("verifier");
    std::fs::create_dir_all(pkg.join("nvml")).unwrap();
    std::fs::write(pkg.join("__init__.py"), b"").unwrap();
    std::fs::write(
        pkg.join("cc_admin.py"),
        b"def collect_gpu_evidence_remote(nonce, no_gpu_mode=False, ppcie_mode=True):\n\
          \x20   arch = 'STUB-CANNED' if no_gpu_mode else 'STUB-REAL'\n\
          \x20   return [{'certificate': 'Y2VydA==', 'evidence': 'ev:' + nonce, 'arch': arch}]\n",
    )
    .unwrap();
    let py = |b: bool| if b { "True" } else { "False" };
    std::fs::write(
        pkg.join("nvml").join("__init__.py"),
        format!(
            "class NvmlHandler:\n\
             \x20   @staticmethod\n    def init_nvml():\n        pass\n\
             \x20   @staticmethod\n    def is_cc_enabled():\n        return {}\n\
             \x20   @staticmethod\n    def is_ppcie_mode_enabled():\n        return {}\n\
             \x20   @staticmethod\n    def is_cc_dev_mode():\n        return {}\n",
            py(cc_on),
            py(ppcie),
            py(devtools)
        ),
    )
    .unwrap();
    (
        GpuEvidenceCollector::new(
            "python3",
            script,
            GpuEvidenceMode::Real,
            Duration::from_secs(20),
        ),
        s,
    )
}

#[tokio::test]
async fn real_collector_refuses_devtools_cc_off_and_ppcie_and_collects_when_clear() {
    let refused = |m: &str, needle: &str| {
        assert!(m.contains("collector exited"), "{m}");
        assert!(
            m.contains(needle),
            "the last stderr line names the reason: {m}"
        );
    };
    // DevTools on (mutation: drop the refusal → the script exits 0 and collects)
    let (c, _s) = stubbed_collector(true, false, true);
    match c.collect(&NONCE).await {
        Err(TeeError::GpuEvidence(m)) => refused(&m, "DevTools"),
        other => panic!("DevTools mode must be refused, got {other:?}"),
    }
    // CC off
    let (c, _s) = stubbed_collector(false, false, false);
    match c.collect(&NONCE).await {
        Err(TeeError::GpuEvidence(m)) => refused(&m, "confidential computing is OFF"),
        other => panic!("CC off must be refused, got {other:?}"),
    }
    // PPCIe on
    let (c, _s) = stubbed_collector(true, true, false);
    match c.collect(&NONCE).await {
        Err(TeeError::GpuEvidence(m)) => refused(&m, "PPCIe"),
        other => panic!("PPCIe must be refused, got {other:?}"),
    }
    // all clear: one JSON line, our nonce passed through to nvtrust, real mode
    let (c, _s) = stubbed_collector(true, false, false);
    let bytes = c.collect(&NONCE).await.expect("clear host collects");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("one JSON line");
    assert_eq!(v["nonce"], hex::encode(NONCE));
    assert_eq!(
        v["arch"], "STUB-REAL",
        "real mode must not ask for sample evidence"
    );
    let e = &v["evidence_list"].as_array().unwrap()[0];
    assert_eq!(
        e["evidence"],
        format!("ev:{}", hex::encode(NONCE)),
        "the nonce reached nvtrust"
    );
    assert!(v.get("canned").is_none());
}
