// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P2.4 — the GPU evidence collector (a subprocess contract) against
//! fake collector scripts; the provider built on it is `test_dstack_provider.rs`. What is proven: the nonce is
//! passed verbatim and checked on return, the canned label is enforced in both
//! directions, failures are `GpuEvidence` errors never panics, and the
//! provider's `report_data` is `sha256(pk_att) ‖ nonce`. The real script and
//! NVML are gates A-5 and B-2.

use fabstir_llm_node::tee::gpu_evidence::{GpuEvidenceCollector, GpuEvidenceMode};
use fabstir_llm_node::tee::types::TeeError;
use std::path::PathBuf;
use std::time::Duration;

const NONCE: [u8; 32] = [0x42u8; 32];

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
        "tee-gpuev-{}-{}",
        std::process::id(),
        hex::encode(b)
    ));
    std::fs::create_dir_all(&d).unwrap();
    Scratch(d)
}

/// Write a fake collector: a python script whose behaviour is the given body.
/// The returned guard owns the directory; keep it alive for the test.
fn fake_script(body: &str) -> (PathBuf, Scratch) {
    let dir = tmpdir();
    let p = dir.0.join("collect.py");
    std::fs::write(&p, format!("import sys, os, json\n{body}\n")).unwrap();
    (p, dir)
}

/// A well-behaved fake: echoes argv[1] as the nonce, labels canned per env.
const GOOD: &str = r#"
nonce = sys.argv[1]
payload = {"nonce": nonce, "evidence_list": [{"certificate": "Y2VydA==", "evidence": "ZXY=", "arch": "HOPPER"}], "arch": "HOPPER"}
if os.environ.get("TEE_GPU_EVIDENCE") == "canned":
    payload["canned"] = True
print("gpu-state: cc_enabled=True ppcie=False devtools=False", file=sys.stderr)
print(json.dumps(payload))
"#;

/// A collector over a fake script; the guard keeps the script alive.
struct Fake(GpuEvidenceCollector, #[allow(dead_code)] Scratch);
impl std::ops::Deref for Fake {
    type Target = GpuEvidenceCollector;
    fn deref(&self) -> &GpuEvidenceCollector {
        &self.0
    }
}

fn collector((script, guard): (PathBuf, Scratch), mode: GpuEvidenceMode) -> Fake {
    Fake(
        GpuEvidenceCollector::new("python3", script, mode, Duration::from_secs(20)),
        guard,
    )
}

#[tokio::test]
async fn real_mode_passes_the_nonce_and_returns_the_payload_bytes() {
    let c = collector(fake_script(GOOD), GpuEvidenceMode::Real);
    let bytes = c.collect(&NONCE).await.expect("collect");
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["nonce"], hex::encode(NONCE));
    assert_eq!(v["arch"], "HOPPER");
    assert!(
        v.get("canned").is_none(),
        "real mode must not be labelled canned"
    );
    assert_eq!(v["evidence_list"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn library_chatter_on_stdout_before_the_payload_is_tolerated() {
    // What nvtrust actually does: its info_log StreamHandler is bound to stdout
    // and prints three progress lines during collection. The script now routes
    // that to stderr, and the Rust side takes the LAST stdout line regardless,
    // so neither side alone is load-bearing.
    let chatty = r#"
print("Number of GPUs available : 1")
print("Fetching GPU 0 information from GPU driver.")
print("All GPU Evidences fetched successfully")
print(json.dumps({"nonce": sys.argv[1], "evidence_list": [{"certificate": "x", "evidence": "y", "arch": "HOPPER"}], "arch": "HOPPER"}))
"#;
    let bytes = collector(fake_script(chatty), GpuEvidenceMode::Real)
        .collect(&NONCE)
        .await
        .expect("chatter before the JSON must not break collection");
    let v: serde_json::Value =
        serde_json::from_slice(&bytes).expect("payload bytes are the JSON line only");
    assert_eq!(v["nonce"], hex::encode(NONCE));
}

#[tokio::test]
async fn collector_runs_with_a_scrubbed_environment_in_a_scratch_cwd() {
    // Converge rounds 3 and 5 (2026-09-17): nvtrust + pynvml are third-party
    // code; they must not inherit HOST_PRIVATE_KEY or the broker URL, and
    // nvtrust rewrites `verifier.log` in the CWD at import, so each run gets its
    // own scratch dir. No `set_var` here (a data race against C `getenv` on
    // other test threads): the test process env already has plenty of names,
    // and the child must see exactly the allow-list.
    let probe = r#"
payload = {"nonce": sys.argv[1], "evidence_list": [{"certificate": "x", "evidence": "y", "arch": "HOPPER"}], "arch": "HOPPER",
           "env_keys": sorted(os.environ.keys()), "cwd": os.getcwd()}
print(json.dumps(payload))
"#;
    let bytes = collector(fake_script(probe), GpuEvidenceMode::Real)
        .collect(&NONCE)
        .await
        .expect("collect");
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let keys: Vec<&str> = v["env_keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap())
        .collect();
    // LC_CTYPE is added by the Python interpreter itself (locale coercion), not
    // inherited; everything else must be exactly the allow-list.
    let allowed = [
        "LC_CTYPE",
        "PATH",
        "PYTHONDONTWRITEBYTECODE",
        "TEE_GPU_EVIDENCE",
    ];
    assert!(
        keys.iter().all(|k| allowed.contains(k)),
        "the collector must see the allow-list only; got {keys:?} (the node's env has {} names)",
        std::env::vars().count()
    );
    for must in ["PATH", "TEE_GPU_EVIDENCE"] {
        assert!(
            keys.contains(&must),
            "{must} missing from the collector env"
        );
    }
    // The scratch dir is gone by now (removed after the run), so compare the
    // reported path against both spellings of temp_dir (it may be a symlink).
    let cwd = PathBuf::from(v["cwd"].as_str().unwrap());
    let temp = std::env::temp_dir();
    let temp_canon = temp.canonicalize().unwrap_or_else(|_| temp.clone());
    assert!(
        (cwd.starts_with(&temp) || cwd.starts_with(&temp_canon))
            && cwd != temp
            && cwd != temp_canon,
        "collector must run in a per-call scratch dir under temp_dir, got {}",
        cwd.display()
    );
    assert!(
        cwd.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("tee-gpu-evidence-"),
        "scratch dir name, got {}",
        cwd.display()
    );
    assert!(
        !cwd.exists(),
        "the per-call scratch dir is removed after the run"
    );
}

#[tokio::test]
async fn concurrent_collections_for_the_same_nonce_do_not_share_a_scratch_dir() {
    // Converge round 8 (2026-09-17): the scratch dir was keyed on the nonce
    // prefix, so parallel collections with one nonce deleted each other's CWD
    // (spawn ENOENT / FileNotFoundError). Eight at once, same nonce, all must
    // succeed and each must have seen a different CWD.
    let (script, _guard) = fake_script(
        r#"
import time
time.sleep(0.2)
print(json.dumps({"nonce": sys.argv[1], "evidence_list": [{"certificate": "x", "evidence": "y", "arch": "HOPPER"}], "arch": "HOPPER", "cwd": os.getcwd()}))
"#,
    );
    let c = std::sync::Arc::new(GpuEvidenceCollector::new(
        "python3",
        script,
        GpuEvidenceMode::Real,
        Duration::from_secs(30),
    ));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let c = c.clone();
        handles.push(tokio::spawn(async move { c.collect(&NONCE).await }));
    }
    let mut cwds = std::collections::HashSet::new();
    for h in handles {
        let bytes = h
            .await
            .unwrap()
            .expect("every concurrent collection succeeds");
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        cwds.insert(v["cwd"].as_str().unwrap().to_string());
    }
    assert_eq!(cwds.len(), 8, "each call must get its own scratch dir");
}

#[tokio::test]
async fn canned_mode_requires_and_accepts_the_label() {
    let c = collector(fake_script(GOOD), GpuEvidenceMode::Canned);
    let bytes = c.collect(&NONCE).await.expect("collect canned");
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["canned"], true);
}

#[tokio::test]
async fn real_mode_refuses_a_payload_labelled_canned() {
    // A script that labels canned regardless of the mode: a real-mode node must
    // never ship it, whatever the broker would do with it.
    let always_canned = r#"
print(json.dumps({"nonce": sys.argv[1], "evidence_list": [{"certificate": "x", "evidence": "y", "arch": "HOPPER"}], "arch": "HOPPER", "canned": True}))
"#;
    let c = collector(fake_script(always_canned), GpuEvidenceMode::Real);
    match c.collect(&NONCE).await {
        Err(TeeError::GpuEvidence(m)) => assert!(m.contains("CANNED"), "{m}"),
        other => panic!("expected GpuEvidence error, got {other:?}"),
    }
}

#[tokio::test]
async fn canned_mode_refuses_an_unlabelled_payload() {
    let never_labelled = r#"
print(json.dumps({"nonce": sys.argv[1], "evidence_list": [{"certificate": "x", "evidence": "y", "arch": "HOPPER"}], "arch": "HOPPER"}))
"#;
    let c = collector(fake_script(never_labelled), GpuEvidenceMode::Canned);
    match c.collect(&NONCE).await {
        Err(TeeError::GpuEvidence(m)) => assert!(m.contains("not labelled"), "{m}"),
        other => panic!("expected GpuEvidence error, got {other:?}"),
    }
}

#[tokio::test]
async fn refuses_a_payload_for_another_nonce() {
    let wrong_nonce = r#"
print(json.dumps({"nonce": "00" * 32, "evidence_list": [{"certificate": "x", "evidence": "y", "arch": "HOPPER"}], "arch": "HOPPER"}))
"#;
    let c = collector(fake_script(wrong_nonce), GpuEvidenceMode::Real);
    match c.collect(&NONCE).await {
        Err(TeeError::GpuEvidence(m)) => assert!(m.contains("nonce"), "{m}"),
        other => panic!("expected GpuEvidence error, got {other:?}"),
    }
}

#[tokio::test]
async fn refuses_an_empty_evidence_list_and_non_json() {
    let empty =
        r#"print(json.dumps({"nonce": sys.argv[1], "evidence_list": [], "arch": "HOPPER"}))"#;
    assert!(matches!(
        collector(fake_script(empty), GpuEvidenceMode::Real)
            .collect(&NONCE)
            .await,
        Err(TeeError::GpuEvidence(_))
    ));
    let garbage = r#"print("not json at all")"#;
    assert!(matches!(
        collector(fake_script(garbage), GpuEvidenceMode::Real)
            .collect(&NONCE)
            .await,
        Err(TeeError::GpuEvidence(_))
    ));
}

#[tokio::test]
async fn non_zero_exit_surfaces_the_scripts_last_stderr_line() {
    // The image's collector fails closed with a reason on stderr (CC off,
    // PPCIe on, no GPU); the node must carry that reason, not swallow it.
    let ppcie = r#"
print("gpu-state: cc_enabled=True ppcie=True devtools=False", file=sys.stderr)
print("PPCIe (multi-GPU protected PCIe) is ON: failing closed (gate B-1a, gap G-14).", file=sys.stderr)
sys.exit(75)
"#;
    match collector(fake_script(ppcie), GpuEvidenceMode::Real)
        .collect(&NONCE)
        .await
    {
        Err(TeeError::GpuEvidence(m)) => {
            assert!(m.contains("exited") && m.contains("PPCIe"), "{m}")
        }
        other => panic!("expected GpuEvidence error, got {other:?}"),
    }
}

#[tokio::test]
async fn missing_script_and_timeout_are_errors_not_panics() {
    let missing = GpuEvidenceCollector::new(
        "python3",
        "/nonexistent/collect.py",
        GpuEvidenceMode::Real,
        Duration::from_secs(5),
    );
    assert!(matches!(
        missing.collect(&NONCE).await,
        Err(TeeError::GpuEvidence(_))
    ));
    let (slow, _guard) = fake_script("import time\ntime.sleep(5)\n");
    let c = GpuEvidenceCollector::new(
        "python3",
        slow,
        GpuEvidenceMode::Real,
        Duration::from_millis(300),
    );
    match c.collect(&NONCE).await {
        Err(TeeError::GpuEvidence(m)) => assert!(m.contains("timed out"), "{m}"),
        other => panic!("expected timeout, got {other:?}"),
    }
}

#[test]
fn mode_parsing_is_strict() {
    assert_eq!(GpuEvidenceMode::parse(None).unwrap(), GpuEvidenceMode::Real);
    assert_eq!(
        GpuEvidenceMode::parse(Some("")).unwrap(),
        GpuEvidenceMode::Real
    );
    assert_eq!(
        GpuEvidenceMode::parse(Some("real")).unwrap(),
        GpuEvidenceMode::Real
    );
    assert_eq!(
        GpuEvidenceMode::parse(Some(" canned ")).unwrap(),
        GpuEvidenceMode::Canned
    );
    assert!(matches!(
        GpuEvidenceMode::parse(Some("CANNED")),
        Err(TeeError::GpuEvidence(_))
    ));
    assert!(matches!(
        GpuEvidenceMode::parse(Some("yes")),
        Err(TeeError::GpuEvidence(_))
    ));
}
