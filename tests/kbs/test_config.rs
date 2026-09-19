//! Design §2: every rule of `KbsConfig::from_map`; mutation = drop a bound → accepted.

use fabstir_llm_node::kbs::config::{CpuEvidenceMode, GpuEvidenceMode, KbsConfig};
use std::collections::HashMap;
use std::time::Duration;

fn base(dir: &tempfile::TempDir) -> HashMap<String, String> {
    std::fs::create_dir_all(dir.path().join("public/policies")).unwrap();
    let mut m = HashMap::new();
    m.insert(
        "KBS_DATA_DIR".into(),
        dir.path().to_string_lossy().into_owned(),
    );
    m
}

fn with(dir: &tempfile::TempDir, k: &str, v: &str) -> Result<KbsConfig, String> {
    let mut m = base(dir);
    m.insert(k.into(), v.into());
    KbsConfig::from_map(&m).map_err(|e| e.0)
}

#[test]
fn defaults_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let c = KbsConfig::from_map(&base(&dir)).unwrap();
    assert_eq!(c.listen.to_string(), "127.0.0.1:3030");
    assert_eq!(c.keyring_file, dir.path().join("keyring.json"));
    assert_eq!(c.policy_dir, dir.path().join("public/policies"));
    assert_eq!(c.gpu_evidence, GpuEvidenceMode::Real);
    assert_eq!(c.cpu_evidence, CpuEvidenceMode::Real);
    assert!(!c.test_keyring_required());
    assert_eq!(c.nonce_ttl, Duration::from_secs(300));
    assert_eq!(c.nonce_cap, 10_000);
    assert_eq!(c.nonce_per_source_cap, 64);
    assert_eq!(c.request_concurrency, 8);
    assert_eq!(c.inflight_per_source_cap, 2);
    assert_eq!(c.request_wall, Duration::from_secs(170));
    assert_eq!(c.pccs_timeout, Duration::from_secs(15));
    assert_eq!(c.nras_timeout, Duration::from_secs(60));
    assert_eq!(c.jwks_timeout, Duration::from_secs(10));
    assert_eq!(c.collateral_memo_fresh, Duration::from_secs(86_400));
    assert_eq!(c.max_body_bytes, 1_048_576);
    assert_eq!(c.capture_max, 100);
    assert_eq!(c.capture_preverify_max, 20);
    assert_eq!(c.nras_claims_version, "2.0");
    assert_eq!(c.nras_issuer, "https://nras.attestation.nvidia.com");
    assert_eq!(c.memo_dir(), dir.path().join("memo"));
    assert_eq!(c.capture_dir(), dir.path().join("capture"));
}

#[test]
fn missing_policy_dir_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let e = with(&dir, "KBS_POLICY_DIR", "/nonexistent/policy-dir").unwrap_err();
    assert!(e.contains("KBS_POLICY_DIR"), "{e}");
    // the default policy dir must exist too
    let mut m = HashMap::new();
    let empty = tempfile::tempdir().unwrap();
    m.insert(
        "KBS_DATA_DIR".to_string(),
        empty.path().to_string_lossy().into_owned(),
    );
    assert!(KbsConfig::from_map(&m)
        .unwrap_err()
        .0
        .contains("KBS_POLICY_DIR"));
}

#[test]
fn missing_data_dir_refuses() {
    let mut m = HashMap::new();
    m.insert(
        "KBS_DATA_DIR".to_string(),
        "/nonexistent/kbs-data-dir".to_string(),
    );
    let e = KbsConfig::from_map(&m).unwrap_err();
    assert!(e.0.contains("KBS_DATA_DIR"), "{e}");
}

#[test]
fn nonce_ttl_bounds() {
    let dir = tempfile::tempdir().unwrap();
    assert!(with(&dir, "KBS_NONCE_TTL_SECS", "299")
        .unwrap_err()
        .contains("300..=3600"));
    assert!(with(&dir, "KBS_NONCE_TTL_SECS", "3601")
        .unwrap_err()
        .contains("300..=3600"));
    assert!(with(&dir, "KBS_NONCE_TTL_SECS", "abc")
        .unwrap_err()
        .contains("KBS_NONCE_TTL_SECS"));
    assert_eq!(
        with(&dir, "KBS_NONCE_TTL_SECS", "3600").unwrap().nonce_ttl,
        Duration::from_secs(3600)
    );
}

#[test]
fn modes_parse_and_unknown_refuses() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        with(&dir, "KBS_GPU_EVIDENCE", "canned")
            .unwrap()
            .gpu_evidence,
        GpuEvidenceMode::Canned
    );
    assert!(with(&dir, "KBS_GPU_EVIDENCE", "canned")
        .unwrap()
        .test_keyring_required());
    assert_eq!(
        with(&dir, "KBS_CPU_EVIDENCE", "simulator")
            .unwrap()
            .cpu_evidence,
        CpuEvidenceMode::Simulator
    );
    assert!(with(&dir, "KBS_CPU_EVIDENCE", "simulator")
        .unwrap()
        .test_keyring_required());
    assert!(with(&dir, "KBS_GPU_EVIDENCE", "fake")
        .unwrap_err()
        .contains("real|canned"));
    assert!(with(&dir, "KBS_CPU_EVIDENCE", "mock")
        .unwrap_err()
        .contains("real|simulator"));
}

#[test]
fn urls_must_be_https_without_userinfo() {
    let dir = tempfile::tempdir().unwrap();
    assert!(with(&dir, "KBS_PCCS_URL", "http://pccs.example")
        .unwrap_err()
        .contains("https"));
    assert!(
        with(&dir, "KBS_NRAS_GPU_URL", "https://u:p@nras.example/v3")
            .unwrap_err()
            .contains("userinfo")
    );
    assert!(
        with(&dir, "KBS_NRAS_JWKS_URL", "https://nras.example/jwks#x")
            .unwrap_err()
            .contains("fragment")
    );
    assert!(with(&dir, "KBS_PCCS_URL", "not a url")
        .unwrap_err()
        .contains("KBS_PCCS_URL"));
    let c = with(
        &dir,
        "KBS_PCCS_URL",
        "https://api.trustedservices.intel.com",
    )
    .unwrap();
    assert_eq!(c.pccs_url.host_str(), Some("api.trustedservices.intel.com"));
}

#[test]
fn caps_and_relations() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = base(&dir);
    m.insert("KBS_NONCE_CAP".into(), "10".into());
    m.insert("KBS_NONCE_PER_SOURCE_CAP".into(), "11".into());
    assert!(KbsConfig::from_map(&m)
        .unwrap_err()
        .0
        .contains("must not exceed KBS_NONCE_CAP"));
    let mut m = base(&dir);
    m.insert("KBS_REQUEST_CONCURRENCY".into(), "2".into());
    m.insert("KBS_INFLIGHT_PER_SOURCE_CAP".into(), "3".into());
    assert!(KbsConfig::from_map(&m)
        .unwrap_err()
        .0
        .contains("must not exceed KBS_REQUEST_CONCURRENCY"));
    assert!(with(&dir, "KBS_REQUEST_CONCURRENCY", "0")
        .unwrap_err()
        .contains("1..=1024"));
    assert_eq!(with(&dir, "KBS_CAPTURE_MAX", "0").unwrap().capture_max, 0);
    assert!(with(&dir, "KBS_MAX_BODY_BYTES", "10")
        .unwrap_err()
        .contains("KBS_MAX_BODY_BYTES"));
}

#[test]
fn claims_version_without_a_table_refuses_at_start() {
    let dir = tempfile::tempdir().unwrap();
    let e = with(&dir, "KBS_NRAS_CLAIMS_VERSION", "2.1").unwrap_err();
    assert!(e.contains("no claim table"), "{e}");
    assert_eq!(
        with(&dir, "KBS_NRAS_CLAIMS_VERSION", "2.0")
            .unwrap()
            .nras_claims_version,
        "2.0"
    );
}

#[test]
fn probe_writable_refuses_a_read_only_dir_and_creates_a_missing_one() {
    use fabstir_llm_node::kbs::config::probe_writable;
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let fresh = dir.path().join("memo");
    probe_writable(&fresh).unwrap();
    assert!(fresh.is_dir());
    let ro = dir.path().join("capture");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let e = probe_writable(&ro).unwrap_err();
    assert!(e.0.contains("not writable"), "{e}");
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn unknown_kbs_variable_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let e = with(&dir, "KBS_LOG_JSON", "true").unwrap_err();
    assert!(e.contains("unknown variable KBS_LOG_JSON"), "{e}");
    // the tooling's env-file pointer is not a server setting, but must not refuse the server
    assert!(with(&dir, "KBS_ENV_FILE", "/etc/fabstir-kbs/env").is_ok());
}

#[test]
fn allowed_hosts_carry_configured_ports_and_intel_hosts() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = base(&dir);
    m.insert("KBS_PCCS_URL".into(), "https://kbs.test:8443/pccs".into());
    m.insert(
        "KBS_NRAS_GPU_URL".into(),
        "https://Kbs.Test:8443/v3/attest/gpu".into(),
    );
    let c = KbsConfig::from_map(&m).unwrap();
    let hosts = c.allowed_hosts();
    assert!(hosts.contains(&("kbs.test".to_string(), 8443)));
    assert!(hosts.contains(&("nras.attestation.nvidia.com".to_string(), 443)));
    assert!(hosts.contains(&("api.trustedservices.intel.com".to_string(), 443)));
    assert!(hosts.contains(&("certificates.trustedservices.intel.com".to_string(), 443)));
    assert_eq!(
        hosts.iter().filter(|(h, _)| h == "kbs.test").count(),
        1,
        "deduped, case-folded"
    );
}
