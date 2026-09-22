//! Design §10 / §10.1 / A-17: a fake NRAS on loopback signing with an rcgen P-384
//! key and serving a fake JWKS; every branch of the token rules; and the captured
//! A-17 tokens verified against the vendored JWKS entry through the real decode path.

use super::fixtures::fixture;
use super::harness::{egress_for, pki, spawn_fake, Fake, FakeHandler, TestPki, HOST};
use fabstir_llm_node::kbs::config::GpuEvidenceMode;
use fabstir_llm_node::kbs::error::Kind;
use fabstir_llm_node::kbs::gpu::{
    attest, check_overall, decide, nras_validation, parse_response, verify_tokens, GpuConfig,
};
use fabstir_llm_node::kbs::memo::MemoHttp;
use fabstir_llm_node::kbs::nras_claims::{map_per_gpu, GpuOutcome, CLAIMS_2_0};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const ISSUER: &str = "https://nras.attestation.nvidia.com";
const NONCE: [u8; 32] = [0x5a; 32];

/// A P-384 signer with its JWK.
#[derive(Clone)]
pub struct Signer {
    pub kid: String,
    pem: String,
    x: String,
    y: String,
}

impl Signer {
    pub fn new(kid: &str) -> Self {
        use base64::Engine;
        let kp = rcgen::KeyPair::generate(&rcgen::PKCS_ECDSA_P384_SHA384).unwrap();
        let raw = kp.public_key_raw(); // 0x04 ‖ x ‖ y
        assert_eq!(raw.len(), 97);
        let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
        Self {
            kid: kid.into(),
            pem: kp.serialize_pem(),
            x: b64(&raw[1..49]),
            y: b64(&raw[49..97]),
        }
    }
    pub fn jwk(&self) -> Value {
        json!({"kty":"EC","crv":"P-384","kid":self.kid,"x":self.x,"y":self.y})
    }
    pub fn jwks(&self) -> Vec<u8> {
        serde_json::to_vec(&json!({"keys":[self.jwk()]})).unwrap()
    }
    pub fn mint(&self, claims: &Map<String, Value>) -> String {
        let mut h = Header::new(Algorithm::ES384);
        h.kid = Some(self.kid.clone());
        encode(
            &h,
            claims,
            &EncodingKey::from_ec_pem(self.pem.as_bytes()).unwrap(),
        )
        .unwrap()
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn base_claims(iss: &str) -> Map<String, Value> {
    let t = now();
    let mut m = Map::new();
    m.insert("iss".into(), json!(iss));
    m.insert("iat".into(), json!(t));
    m.insert("nbf".into(), json!(t - 10));
    m.insert("exp".into(), json!(t + 3600));
    m.insert("jti".into(), json!("jti-1"));
    m
}

pub fn good_per_gpu(nonce_hex: &str) -> Map<String, Value> {
    let mut m = base_claims(ISSUER);
    m.insert("eat_nonce".into(), json!(nonce_hex));
    m.insert("hwmodel".into(), json!("GH100 A01 GSP BROM"));
    m.insert("secboot".into(), json!(true));
    m.insert("dbgstat".into(), json!("disabled"));
    m.insert("x-nvidia-gpu-driver-version".into(), json!("550.90.07"));
    m.insert("x-nvidia-gpu-vbios-version".into(), json!("96.00.74.00.1a"));
    m.insert(
        "x-nvidia-gpu-attestation-report-nonce-match".into(),
        json!(true),
    );
    m.insert(
        "x-nvidia-gpu-attestation-report-cert-chain-validated".into(),
        json!(true),
    );
    m.insert(
        "x-nvidia-gpu-attestation-report-signature-verified".into(),
        json!(true),
    );
    m.insert("x-nvidia-gpu-arch-check".into(), json!(true));
    m.insert("measres".into(), json!("success"));
    m
}

pub fn good_overall(nonce_hex: &str, per_gpu_token: &str, result: Value) -> Map<String, Value> {
    let mut m = base_claims(ISSUER);
    m.insert("eat_nonce".into(), json!(nonce_hex));
    m.insert("x-nvidia-ver".into(), json!("2.0"));
    m.insert("x-nvidia-overall-att-result".into(), result);
    m.insert(
        "submods".into(),
        json!({"GPU-0": ["DIGEST", ["SHA-256", hex::encode(Sha256::digest(per_gpu_token.as_bytes()))]]}),
    );
    m
}

/// How the fake NRAS answers. Every field has a "good" default.
#[derive(Clone, Default)]
pub struct NrasOpts {
    pub per_gpu_edit: Option<Arc<dyn Fn(&mut Map<String, Value>) + Send + Sync>>,
    pub overall_edit: Option<Arc<dyn Fn(&mut Map<String, Value>) + Send + Sync>>,
    /// Sign the tokens with this signer instead of the one the JWKS serves.
    pub sign_with: Option<Signer>,
    /// Serve this JWKS body instead of the signer's.
    pub jwks_body: Option<Vec<u8>>,
    /// The first N POSTs answer 503.
    pub fail_first: usize,
    /// The first N POSTs answer this status instead (overrides `fail_first`).
    pub fail_first_status: Option<(usize, u16)>,
    /// Answer with two per-GPU tokens.
    pub two_gpus: bool,
    /// Overall result value (default `true`).
    pub result: Option<Value>,
    /// Delay every answer (JWKS and POST) by this much.
    pub stall: Option<Duration>,
    /// JWKS requests after the first N answer 503.
    pub jwks_fail_after: Option<usize>,
    /// JWKS requests after the first N answer this (status, body) instead.
    pub jwks_after: Option<(usize, u16, Vec<u8>)>,
}

pub struct Nras {
    pub fake: Fake,
    pub signer: Signer,
    pub posts: Arc<Mutex<Vec<Value>>>,
}

pub async fn spawn_nras(p: &TestPki, opts: NrasOpts) -> Nras {
    let signer = Signer::new("kid-1");
    let jwks_body = opts.jwks_body.clone().unwrap_or_else(|| signer.jwks());
    let sign = opts.sign_with.clone().unwrap_or_else(|| signer.clone());
    let posts: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let posts2 = posts.clone();
    let stall = opts.stall;
    let (fail_n, fail_status) = opts.fail_first_status.unwrap_or((opts.fail_first, 503));
    let failures = Arc::new(Mutex::new(fail_n));
    let jwks_served = Arc::new(Mutex::new(0usize));
    let jwks_after = opts.jwks_after.clone().or_else(|| {
        opts.jwks_fail_after
            .map(|k| (k, 503u16, b"jwks down".to_vec()))
    });
    let handler: FakeHandler = Arc::new(move |method, pq, body| {
        if pq.contains("/jwks") {
            let mut n = jwks_served.lock().unwrap();
            *n += 1;
            if let Some((k, status, body)) = &jwks_after {
                if *n > *k {
                    return (*status, vec![], body.clone());
                }
            }
            return (
                200,
                vec![("content-type".into(), "application/json".into())],
                jwks_body.clone(),
            );
        }
        if method == "POST" && pq.contains("/v3/attest/gpu") {
            let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            posts2.lock().unwrap().push(v.clone());
            {
                let mut f = failures.lock().unwrap();
                if *f > 0 {
                    *f -= 1;
                    return (fail_status, vec![], b"nras says no".to_vec());
                }
            }
            let nonce_hex = v
                .get("nonce")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let mut per = good_per_gpu(&nonce_hex);
            if let Some(e) = &opts.per_gpu_edit {
                e(&mut per);
            }
            let per_tok = sign.mint(&per);
            let mut overall = good_overall(
                &nonce_hex,
                &per_tok,
                opts.result.clone().unwrap_or(json!(true)),
            );
            if let Some(e) = &opts.overall_edit {
                e(&mut overall);
            }
            let overall_tok = sign.mint(&overall);
            let gpus = if opts.two_gpus {
                json!({"GPU-0": per_tok, "GPU-1": per_tok})
            } else {
                json!({"GPU-0": per_tok})
            };
            let resp = json!([["JWT", overall_tok], gpus]);
            return (
                200,
                vec![("content-type".into(), "application/json".into())],
                serde_json::to_vec(&resp).unwrap(),
            );
        }
        (404, vec![], b"no".to_vec())
    });
    let fake = spawn_fake(p, handler, stall.unwrap_or(Duration::ZERO)).await;
    Nras {
        fake,
        signer,
        posts,
    }
}

fn cfg(n: &Nras, mode: GpuEvidenceMode) -> GpuConfig {
    GpuConfig {
        nras_gpu_url: format!("{}/v3/attest/gpu", n.fake.base()),
        nras_jwks_url: format!("{}/jwks", n.fake.base()),
        issuer: ISSUER.into(),
        claims_version: "2.0".into(),
        nras_timeout: Duration::from_secs(5),
        jwks_timeout: Duration::from_secs(5),
        mode,
    }
}

fn payload(canned: Option<Value>) -> Value {
    let mut v = json!({"nonce": hex::encode(NONCE), "evidence_list": [{"certificate": "x", "evidence": "y", "arch": "HOPPER"}], "arch": "HOPPER"});
    if let Some(c) = canned {
        v["canned"] = c;
    }
    v
}

async fn run(
    n: &Nras,
    p: &TestPki,
    mode: GpuEvidenceMode,
    pl: &Value,
) -> Result<GpuOutcome, fabstir_llm_node::kbs::error::KbsError> {
    run_with_deadline(n, p, mode, pl, Instant::now() + Duration::from_secs(170)).await
}

async fn run_with_deadline(
    n: &Nras,
    p: &TestPki,
    mode: GpuEvidenceMode,
    pl: &Value,
    deadline: Instant,
) -> Result<GpuOutcome, fabstir_llm_node::kbs::error::KbsError> {
    let eg = egress_for(p, n.fake.addr, &[]);
    let dir = tempfile::tempdir().unwrap();
    let jwks = MemoHttp::network_first(
        eg.clone(),
        dir.path().to_path_buf(),
        Duration::from_secs(5),
        1_048_576,
    );
    attest(&eg, &jwks, &cfg(n, mode), pl, NONCE, deadline)
        .await
        .0
}

#[tokio::test]
async fn the_raw_nras_body_is_returned_on_a_claim_table_refusal_too() {
    let p = pki(HOST);
    let n = spawn_nras(
        &p,
        NrasOpts {
            per_gpu_edit: Some(Arc::new(|m| {
                m.remove("hwmodel");
            })),
            ..Default::default()
        },
    )
    .await;
    let eg = egress_for(&p, n.fake.addr, &[]);
    let dir = tempfile::tempdir().unwrap();
    let jwks = MemoHttp::network_first(
        eg.clone(),
        dir.path().to_path_buf(),
        Duration::from_secs(5),
        1_048_576,
    );
    let (r, raw) = attest(
        &eg,
        &jwks,
        &cfg(&n, GpuEvidenceMode::Real),
        &payload(None),
        NONCE,
        Instant::now() + Duration::from_secs(170),
    )
    .await;
    assert!(r.unwrap_err().detail.contains("hwmodel"));
    let raw = raw.expect(
        "the NRAS answer is handed back on the refusal path (mutation: drop it on `?` → None)",
    );
    assert!(parse_response(&raw).is_ok());
}

#[tokio::test]
async fn an_unknown_kid_whose_refetch_gets_a_garbage_2xx_is_unavailable_too() {
    let p = pki(HOST);
    let n = spawn_nras(
        &p,
        NrasOpts {
            sign_with: Some(Signer::new("kid-rotated")),
            jwks_after: Some((1, 200, b"<html>maintenance</html>".to_vec())),
            ..Default::default()
        },
    )
    .await;
    let eg = egress_for(&p, n.fake.addr, &[]);
    let dir = tempfile::tempdir().unwrap();
    let url = format!("{}/jwks", n.fake.base());
    let entry = fabstir_llm_node::kbs::memo::MemoEntry {
        url: url.clone(),
        fetched_at: 1,
        status: 200,
        headers: Default::default(),
        body_b64: {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(n.signer.jwks())
        },
    };
    std::fs::write(
        fabstir_llm_node::kbs::memo::memo_path(dir.path(), &url),
        serde_json::to_vec(&entry).unwrap(),
    )
    .unwrap();
    let jwks = MemoHttp::network_first(
        eg.clone(),
        dir.path().to_path_buf(),
        Duration::from_secs(5),
        1_048_576,
    );
    let (r, _) = attest(
        &eg,
        &jwks,
        &cfg(&n, GpuEvidenceMode::Real),
        &payload(None),
        NONCE,
        Instant::now() + Duration::from_secs(170),
    )
    .await;
    let e = r.unwrap_err();
    assert_eq!(
        (e.kind, e.status),
        (Kind::Unavailable, 502),
        "a memo-served key set after a garbage 2xx is an outage, not a bad token: {e}"
    );
}

#[tokio::test]
async fn an_unknown_kid_whose_refetch_cannot_reach_the_network_is_unavailable() {
    let p = pki(HOST);
    // tokens signed by a rotated key; the first JWKS fetch works, the refetch gets 503
    let n = spawn_nras(
        &p,
        NrasOpts {
            sign_with: Some(Signer::new("kid-rotated")),
            jwks_fail_after: Some(1),
            ..Default::default()
        },
    )
    .await;
    let eg = egress_for(&p, n.fake.addr, &[]);
    let dir = tempfile::tempdir().unwrap();
    // a committed memo exists from an earlier good day (commit the first JWKS body by hand)
    let url = format!("{}/jwks", n.fake.base());
    let entry = fabstir_llm_node::kbs::memo::MemoEntry {
        url: url.clone(),
        fetched_at: 1,
        status: 200,
        headers: Default::default(),
        body_b64: {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(n.signer.jwks())
        },
    };
    std::fs::create_dir_all(dir.path()).unwrap();
    std::fs::write(
        fabstir_llm_node::kbs::memo::memo_path(dir.path(), &url),
        serde_json::to_vec(&entry).unwrap(),
    )
    .unwrap();
    let jwks = MemoHttp::network_first(
        eg.clone(),
        dir.path().to_path_buf(),
        Duration::from_secs(5),
        1_048_576,
    );
    let (r, _) = attest(
        &eg,
        &jwks,
        &cfg(&n, GpuEvidenceMode::Real),
        &payload(None),
        NONCE,
        Instant::now() + Duration::from_secs(170),
    )
    .await;
    let e = r.unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 502), "{e}");
    assert_eq!(n.fake.hits_matching("/jwks"), 2);
}

#[tokio::test]
async fn success_path_maps_the_claim_table_and_strips_the_label() {
    let p = pki(HOST);
    let n = spawn_nras(&p, NrasOpts::default()).await;
    let out = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap();
    let GpuOutcome::Real(f) = out else {
        panic!("real")
    };
    assert_eq!(f.hwmodel, "GH100 A01 GSP BROM");
    assert!(f.secure_boot && f.debug_disabled);
    assert_eq!(f.driver_version, "550.90.07");
    assert_eq!(f.nonce, NONCE);
    let posted = n.posts.lock().unwrap()[0].clone();
    assert!(posted.get("canned").is_none());
    assert_eq!(posted["nonce"], json!(hex::encode(NONCE)));
    assert_eq!(
        n.fake.hits_matching("/jwks"),
        1,
        "JWKS fetched once, before the POST"
    );
    assert_eq!(n.fake.hits.lock().unwrap()[0].1.contains("/jwks"), true);
}

#[tokio::test]
async fn wrong_signature_expired_missing_iss_and_wrong_issuer_are_refused() {
    let p = pki(HOST);
    // wrong key
    let n = spawn_nras(
        &p,
        NrasOpts {
            sign_with: Some(Signer::new("kid-1")),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    assert!(e.detail.contains("InvalidSignature"), "{e}");
    // expired
    let n = spawn_nras(
        &p,
        NrasOpts {
            per_gpu_edit: Some(Arc::new(|m| {
                m.insert("exp".into(), json!(now() - 1000));
            })),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert!(e.detail.contains("ExpiredSignature"), "{e}");
    // missing iss (mutation: drop required_spec_claims → green)
    let n = spawn_nras(
        &p,
        NrasOpts {
            overall_edit: Some(Arc::new(|m| {
                m.remove("iss");
            })),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert!(e.detail.contains("iss"), "{e}");
    // wrong issuer
    let n = spawn_nras(
        &p,
        NrasOpts {
            overall_edit: Some(Arc::new(|m| {
                m.insert("iss".into(), json!("https://evil"));
            })),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert!(e.detail.contains("InvalidIssuer"), "{e}");
}

#[tokio::test]
async fn an_aud_claim_is_still_accepted() {
    let p = pki(HOST);
    let n = spawn_nras(
        &p,
        NrasOpts {
            overall_edit: Some(Arc::new(|m| {
                m.insert("aud".into(), json!("someone"));
            })),
            per_gpu_edit: Some(Arc::new(|m| {
                m.insert("aud".into(), json!(["a", "b"]));
            })),
            ..Default::default()
        },
    )
    .await;
    assert!(matches!(
        run(&n, &p, GpuEvidenceMode::Real, &payload(None))
            .await
            .unwrap(),
        GpuOutcome::Real(_)
    ));
}

#[tokio::test]
async fn unknown_kid_refetches_once_then_refuses() {
    let p = pki(HOST);
    let other = Signer::new("kid-rotated");
    let n = spawn_nras(
        &p,
        NrasOpts {
            sign_with: Some(other),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert_eq!(
        (e.kind, e.status),
        (Kind::Unavailable, 502),
        "a key rotation the JWKS does not reflect yet is an outage, not a bad token: {e}"
    );
    assert!(e.detail.contains("unknown kid"), "{e}");
    assert_eq!(
        n.fake.hits_matching("/jwks"),
        2,
        "fetched, then refetched once"
    );
}

#[tokio::test]
async fn overall_rows_submods_nonce_result_type_and_version() {
    let p = pki(HOST);
    // submods digest mismatch
    let n = spawn_nras(
        &p,
        NrasOpts {
            overall_edit: Some(Arc::new(|m| {
                m["submods"] = json!({"GPU-0": ["DIGEST", ["SHA-256", "00"]]});
            })),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert!(e.detail.contains("submods"), "{e}");
    // per-GPU nonce differs while the wrapper matched (the expert's test)
    let n = spawn_nras(
        &p,
        NrasOpts {
            per_gpu_edit: Some(Arc::new(|m| {
                m["eat_nonce"] = json!(hex::encode([9u8; 32]));
            })),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert!(e.detail.contains("gpu nonce (signed)"), "{e}");
    // the overall nonce differs
    let n = spawn_nras(
        &p,
        NrasOpts {
            overall_edit: Some(Arc::new(|m| {
                m["eat_nonce"] = json!(hex::encode([9u8; 32]));
            })),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert!(e.detail.contains("overall eat_nonce"), "{e}");
    // result as the string "true"
    let n = spawn_nras(
        &p,
        NrasOpts {
            result: Some(json!("true")),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert!(e.detail.contains("not a boolean"), "{e}");
    // x-nvidia-ver mismatch
    let n = spawn_nras(
        &p,
        NrasOpts {
            overall_edit: Some(Arc::new(|m| {
                m["x-nvidia-ver"] = json!("3.0");
            })),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert!(e.detail.contains("claims version: 3.0"), "{e}");
}

#[test]
fn a_malformed_nras_2xx_is_an_outage_not_a_refused_gpu() {
    for body in [
        b"<html>challenge</html>".as_slice(),
        b"[[\"JWT\"".as_slice(),
        b"{}".as_slice(),
        b"[[\"JWT\", 1], {}]".as_slice(),
    ] {
        let e = parse_response(body).unwrap_err();
        assert_eq!((e.kind, e.status), (Kind::Unavailable, 502), "{e}");
    }
    // a well-formed answer with two tokens is a real refusal
    let two = serde_json::to_vec(&json!([["JWT", "a.b.c"], {"GPU-0": "a.b.c", "GPU-1": "a.b.c"}]))
        .unwrap();
    assert_eq!(parse_response(&two).unwrap_err().kind, Kind::Verification);
}

#[tokio::test]
async fn two_gpu_tokens_and_a_missing_never_row_are_refused() {
    let p = pki(HOST);
    let n = spawn_nras(
        &p,
        NrasOpts {
            two_gpus: true,
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert!(e.detail.contains("gpu count"), "{e}");
    let n = spawn_nras(
        &p,
        NrasOpts {
            per_gpu_edit: Some(Arc::new(|m| {
                m.remove("x-nvidia-gpu-attestation-report-nonce-match");
            })),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert!(e.detail.contains("nonce match"), "{e}");
}

#[tokio::test]
async fn every_row_is_listed_not_just_the_first() {
    let p = pki(HOST);
    let n = spawn_nras(
        &p,
        NrasOpts {
            per_gpu_edit: Some(Arc::new(|m| {
                m.remove("hwmodel");
                m["measres"] = json!("fail");
                m["secboot"] = json!("yes");
            })),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert!(
        e.detail.contains("hwmodel")
            && e.detail.contains("measres")
            && e.detail.contains("secboot"),
        "{e}"
    );
}

/// D14a: the verdict is DERIVED from the two claims `map_per_gpu` maps, at
/// every use site, so it cannot disagree with them. The claims JSON is the
/// input here, `cc_assertion()` the output.
#[test]
fn cc_assertion_is_derived_from_the_mapped_secboot_and_dbgstat() {
    use fabstir_llm_node::kbs::nras_claims::CcAssertion;
    let want = hex::encode(NONCE);

    let f = map_per_gpu(&good_per_gpu(&want), &CLAIMS_2_0, NONCE).unwrap();
    assert_eq!(f.cc_assertion(), CcAssertion::SignedNotDevTools);
    assert_eq!(f.cc_assertion().label(), "signed-not-devtools");

    // DevTools: attests, but with the debug facilities enabled.
    let mut m = good_per_gpu(&want);
    m["dbgstat"] = json!("enabled");
    assert_eq!(
        map_per_gpu(&m, &CLAIMS_2_0, NONCE).unwrap().cc_assertion(),
        CcAssertion::SignedDevToolsOrNoSecureBoot {
            secure_boot: true,
            debug_disabled: false,
        }
    );

    // Secure boot off does not rule DevTools out either.
    let mut m = good_per_gpu(&want);
    m["secboot"] = json!(false);
    assert_eq!(
        map_per_gpu(&m, &CLAIMS_2_0, NONCE).unwrap().cc_assertion(),
        CcAssertion::SignedDevToolsOrNoSecureBoot {
            secure_boot: false,
            debug_disabled: true,
        }
    );
}

#[test]
fn dbgstat_vocabulary() {
    let want = hex::encode(NONCE);
    for (v, ok) in [
        ("disabled", true),
        ("disabled-since-boot", true),
        ("disabled-permanently", true),
        ("disabled-fully-and-permanently", true),
        ("enabled", false),
        ("unknown", false),
    ] {
        let mut m = good_per_gpu(&want);
        m["dbgstat"] = json!(v);
        let r = map_per_gpu(&m, &CLAIMS_2_0, NONCE);
        match (v, ok) {
            ("enabled", _) => assert!(!r.unwrap().debug_disabled),
            (_, true) => assert!(r.unwrap().debug_disabled),
            _ => assert!(r.unwrap_err().iter().any(|row| row.contains("dbgstat"))),
        }
    }
}

#[tokio::test]
async fn canned_outcome_three_arms() {
    let p = pki(HOST);
    let canned_edit: Arc<dyn Fn(&mut Map<String, Value>) + Send + Sync> =
        Arc::new(|m: &mut Map<String, Value>| {
            m.remove("eat_nonce");
            m.insert(
                "x-nvidia-error-details".into(),
                json!({"code": 4010, "message": "NONCE_NOT_MATCHING"}),
            );
        });
    let opts = NrasOpts {
        per_gpu_edit: Some(canned_edit),
        result: Some(json!(false)),
        ..Default::default()
    };
    // canned mode with the label → tolerated
    let n = spawn_nras(&p, opts.clone()).await;
    assert_eq!(
        run(&n, &p, GpuEvidenceMode::Canned, &payload(Some(json!(true))))
            .await
            .unwrap(),
        GpuOutcome::CannedTolerated
    );
    // canned mode without the label → refused (the label is required by the pre-filter; attest sees label_present=false)
    let n = spawn_nras(&p, opts.clone()).await;
    let e = run(&n, &p, GpuEvidenceMode::Canned, &payload(None))
        .await
        .unwrap_err();
    assert!(e.detail.contains("gpu attestation: overall false"), "{e}");
    // real mode: a real overall-false is refused (the label itself is refused earlier, by the pre-filter)
    let n = spawn_nras(&p, opts).await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
}

#[tokio::test]
async fn nras_503_then_200_is_retried_and_a_short_wall_is_not() {
    let p = pki(HOST);
    let n = spawn_nras(
        &p,
        NrasOpts {
            fail_first: 1,
            ..Default::default()
        },
    )
    .await;
    assert!(matches!(
        run(&n, &p, GpuEvidenceMode::Real, &payload(None))
            .await
            .unwrap(),
        GpuOutcome::Real(_)
    ));
    assert_eq!(n.posts.lock().unwrap().len(), 2);
    // The threshold is derived from the configured timeouts (5 s + 5 s + 5 s margin = 15 s
    // here; 75 s at the production defaults): 14 s left → no retry, 16 s left → retry.
    assert_eq!(
        fabstir_llm_node::kbs::gpu::retry_min_remaining(
            Duration::from_secs(60),
            Duration::from_secs(10)
        ),
        Duration::from_secs(75)
    );
    let n = spawn_nras(
        &p,
        NrasOpts {
            fail_first: 1,
            ..Default::default()
        },
    )
    .await;
    let e = run_with_deadline(
        &n,
        &p,
        GpuEvidenceMode::Real,
        &payload(None),
        Instant::now() + Duration::from_secs(14),
    )
    .await
    .unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 502), "{e}");
    assert_eq!(
        n.posts.lock().unwrap().len(),
        1,
        "no retry below the threshold"
    );
    let n = spawn_nras(
        &p,
        NrasOpts {
            fail_first: 1,
            ..Default::default()
        },
    )
    .await;
    assert!(matches!(
        run_with_deadline(
            &n,
            &p,
            GpuEvidenceMode::Real,
            &payload(None),
            Instant::now() + Duration::from_secs(16)
        )
        .await
        .unwrap(),
        GpuOutcome::Real(_)
    ));
    assert_eq!(
        n.posts.lock().unwrap().len(),
        2,
        "retry above the threshold"
    );
}

#[tokio::test]
async fn a_wall_timeout_mid_retry_still_leaves_the_received_body_in_the_sink() {
    use fabstir_llm_node::kbs::gpu::{attest_with_sink, RawSink};
    // NRAS answers 503 immediately, then stalls the retry; a short wall drops the future.
    let p = pki(HOST);
    let n = spawn_nras(
        &p,
        NrasOpts {
            fail_first: 1,
            stall: Some(Duration::from_millis(1500)),
            ..Default::default()
        },
    )
    .await;
    let eg = egress_for(&p, n.fake.addr, &[]);
    let dir = tempfile::tempdir().unwrap();
    let jwks = MemoHttp::network_first(
        eg.clone(),
        dir.path().to_path_buf(),
        Duration::from_secs(5),
        1_048_576,
    );
    let sink = RawSink::default();
    // The stall applies to every answer: JWKS at 1.5 s, the 503 at 3.0 s, the retry
    // would answer at 4.5 s. A 4.0 s wall drops the future mid-retry, after the 503.
    // The retry threshold (5 + 5 + 5 s) is met by the 20 s deadline.
    let deadline = Instant::now() + Duration::from_secs(20);
    let c = cfg(&n, GpuEvidenceMode::Real);
    let pl = payload(None);
    let fut = attest_with_sink(&eg, &jwks, &c, &pl, NONCE, deadline, &sink);
    let r = tokio::time::timeout(Duration::from_millis(4000), fut).await;
    assert!(r.is_err(), "the wall dropped the future mid-retry");
    assert_eq!(sink.take().as_deref(), Some(b"nras says no".as_slice()), "the 503 body received before the wall is in the caller's sink (mutation: body only on return → None)");
}

#[tokio::test]
async fn a_jwks_entry_of_an_unknown_kind_is_skipped_not_fatal() {
    use fabstir_llm_node::kbs::gpu::parse_jwks;
    let signer = Signer::new("kid-good");
    let body = serde_json::to_vec(&json!({"keys": [
        {"kty": "WEIRD", "kid": "kid-new", "crv": "P-999", "x": "AA", "y": "AA"},
        signer.jwk(),
    ]}))
    .unwrap();
    let set = parse_jwks(&body)
        .expect("one unknown entry does not sink the set (mutation: whole-set deserialise → Err)");
    assert_eq!(set.keys.len(), 1);
    assert!(set.find("kid-good").is_some());
    assert!(
        parse_jwks(br#"{"keys": [{"kty": "WEIRD"}]}"#).is_err(),
        "no usable key at all is an error"
    );
    assert!(parse_jwks(b"<html>").is_err());
    // and end to end: a fake serving the mixed set still verifies
    let p = pki(HOST);
    let n = spawn_nras(
        &p,
        NrasOpts {
            jwks_body: Some(body.clone()),
            sign_with: Some(signer.clone()),
            ..Default::default()
        },
    )
    .await;
    // the fake's own signer is unused; tokens are signed by `signer`, whose key is in the served set
    assert!(matches!(
        run(&n, &p, GpuEvidenceMode::Real, &payload(None))
            .await
            .unwrap(),
        GpuOutcome::Real(_)
    ));
}

#[tokio::test]
async fn a_jwks_that_does_not_parse_falls_back_to_the_memo() {
    let p = pki(HOST);
    // first: a good JWKS gets committed through a successful attest
    let good = spawn_nras(&p, NrasOpts::default()).await;
    let eg = egress_for(&p, good.fake.addr, &[]);
    let dir = tempfile::tempdir().unwrap();
    let jwks = MemoHttp::network_first(
        eg.clone(),
        dir.path().to_path_buf(),
        Duration::from_secs(5),
        1_048_576,
    );
    attest(
        &eg,
        &jwks,
        &cfg(&good, GpuEvidenceMode::Real),
        &payload(None),
        NONCE,
        Instant::now() + Duration::from_secs(170),
    )
    .await
    .0
    .unwrap();
    // now the same signer, but the JWKS endpoint answers a 200 that does not parse
    let bad = spawn_nras(
        &p,
        NrasOpts {
            jwks_body: Some(b"<html>oops</html>".to_vec()),
            sign_with: Some(good.signer.clone()),
            ..Default::default()
        },
    )
    .await;
    // re-key the memo to the new port
    let old_url = format!("{}/jwks", good.fake.base());
    let new_url = format!("{}/jwks", bad.fake.base());
    let entry_path = fabstir_llm_node::kbs::memo::memo_path(dir.path(), &old_url);
    let mut entry: fabstir_llm_node::kbs::memo::MemoEntry =
        serde_json::from_slice(&std::fs::read(&entry_path).unwrap()).unwrap();
    entry.url = new_url.clone();
    std::fs::write(
        fabstir_llm_node::kbs::memo::memo_path(dir.path(), &new_url),
        serde_json::to_vec(&entry).unwrap(),
    )
    .unwrap();
    let eg2 = egress_for(&p, bad.fake.addr, &[]);
    let jwks2 = MemoHttp::network_first(
        eg2.clone(),
        dir.path().to_path_buf(),
        Duration::from_secs(5),
        1_048_576,
    );
    let out = attest(
        &eg2,
        &jwks2,
        &cfg(&bad, GpuEvidenceMode::Real),
        &payload(None),
        NONCE,
        Instant::now() + Duration::from_secs(170),
    )
    .await
    .0
    .unwrap();
    assert!(matches!(out, GpuOutcome::Real(_)));
    // the garbage 200 never reached the memo: the committed entry is still the good JWKS
    // (mutation: commit the staged garbage after token verification → "<html>" in the memo)
    let after: fabstir_llm_node::kbs::memo::MemoEntry = serde_json::from_slice(
        &std::fs::read(fabstir_llm_node::kbs::memo::memo_path(dir.path(), &new_url)).unwrap(),
    )
    .unwrap();
    use base64::Engine;
    let body = base64::engine::general_purpose::STANDARD
        .decode(&after.body_b64)
        .unwrap();
    assert!(
        serde_json::from_slice::<JwkSet>(&body).is_ok(),
        "the memo still parses as a JwkSet"
    );
    assert!(!body.starts_with(b"<html>"));
    // and a third request (still garbage upstream) is served from that memo again
    let jwks3 = MemoHttp::network_first(
        eg2.clone(),
        dir.path().to_path_buf(),
        Duration::from_secs(5),
        1_048_576,
    );
    let out = attest(
        &eg2,
        &jwks3,
        &cfg(&bad, GpuEvidenceMode::Real),
        &payload(None),
        NONCE,
        Instant::now() + Duration::from_secs(170),
    )
    .await
    .0
    .unwrap();
    assert!(matches!(out, GpuOutcome::Real(_)));
}

#[tokio::test]
async fn nras_429_is_retried_and_a_400_is_a_verification_refusal() {
    let p = pki(HOST);
    let n = spawn_nras(
        &p,
        NrasOpts {
            fail_first_status: Some((1, 429)),
            ..Default::default()
        },
    )
    .await;
    assert!(matches!(
        run(&n, &p, GpuEvidenceMode::Real, &payload(None))
            .await
            .unwrap(),
        GpuOutcome::Real(_)
    ));
    assert_eq!(n.posts.lock().unwrap().len(), 2, "429 retried once");
    // a persistent 429 ends as unavailable (not verification)
    let n = spawn_nras(
        &p,
        NrasOpts {
            fail_first_status: Some((5, 429)),
            ..Default::default()
        },
    )
    .await;
    let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
        .await
        .unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 502), "{e}");
    // a 400 is the evidence being refused: verification, no retry, body captured
    let n = spawn_nras(
        &p,
        NrasOpts {
            fail_first_status: Some((1, 400)),
            ..Default::default()
        },
    )
    .await;
    let eg = egress_for(&p, n.fake.addr, &[]);
    let dir = tempfile::tempdir().unwrap();
    let jwks = MemoHttp::network_first(
        eg.clone(),
        dir.path().to_path_buf(),
        Duration::from_secs(5),
        1_048_576,
    );
    let (r, raw) = attest(
        &eg,
        &jwks,
        &cfg(&n, GpuEvidenceMode::Real),
        &payload(None),
        NONCE,
        Instant::now() + Duration::from_secs(170),
    )
    .await;
    let e = r.unwrap_err();
    assert_eq!(e.kind, Kind::Verification, "{e}");
    assert_eq!(n.posts.lock().unwrap().len(), 1, "no retry on 400");
    assert_eq!(
        raw.as_deref(),
        Some(b"nras says no".as_slice()),
        "the 400 body is captured (mutation: return before raw_out → None)"
    );
    // a 404 (wrong endpoint) or 403 (NVIDIA-side) is not the GPU's fault: unavailable, no retry
    for status in [404u16, 403, 401] {
        let n = spawn_nras(
            &p,
            NrasOpts {
                fail_first_status: Some((1, status)),
                ..Default::default()
            },
        )
        .await;
        let e = run(&n, &p, GpuEvidenceMode::Real, &payload(None))
            .await
            .unwrap_err();
        assert_eq!(
            (e.kind, e.status),
            (Kind::Unavailable, 502),
            "{status}: {e}"
        );
        assert_eq!(n.posts.lock().unwrap().len(), 1, "no retry on {status}");
    }
}

// ---------- the captured A-17 bytes through the real decode path ----------

fn a17() -> (String, String, JwkSet, [u8; 32], Map<String, Value>) {
    let resp = fixture("a17-nras-canned-response.json");
    let (overall, name, per) = parse_response(&resp).unwrap();
    assert_eq!(name, "GPU-0");
    let jwks: JwkSet = serde_json::from_slice(&fixture("a17-jwks-entry.json")).unwrap();
    let payload: Value = serde_json::from_slice(&fixture("a17-canned-payload.json")).unwrap();
    let nonce: [u8; 32] = hex::decode(payload["nonce"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    (
        overall,
        per,
        jwks,
        nonce,
        payload.as_object().unwrap().clone(),
    )
}

#[test]
fn a17_tokens_verify_against_the_vendored_jwks_entry() {
    let (overall, per, jwks, nonce, _) = a17();
    let v = nras_validation(ISSUER, false);
    let (ov, pg) = verify_tokens(&overall, &per, &jwks, &v).unwrap();
    assert_eq!(check_overall(&ov, "GPU-0", &per, nonce, "2.0"), Ok(false));
    assert!(
        pg.get("eat_nonce").is_none(),
        "the captured per-GPU token has no eat_nonce"
    );
    assert_eq!(
        pg["x-nvidia-error-details"]["message"],
        json!("NONCE_NOT_MATCHING")
    );
    // one flipped character → InvalidSignature
    let mut bad = per.clone();
    let i = bad.len() - 5;
    let c = bad.as_bytes()[i];
    let replacement = if c == b'A' { 'B' } else { 'A' };
    bad.replace_range(i..i + 1, &replacement.to_string());
    let e = verify_tokens(&overall, &bad, &jwks, &v)
        .unwrap_err()
        .unwrap();
    assert!(e.detail.contains("InvalidSignature"), "{e}");
    // with time validation on they are expired (captured 2026-09-18, one-hour tokens)
    let e = verify_tokens(&overall, &per, &jwks, &nras_validation(ISSUER, true))
        .unwrap_err()
        .unwrap();
    assert!(e.detail.contains("ExpiredSignature"), "{e}");
}

#[test]
fn a17_decision_is_tolerated_only_in_canned_mode_with_the_label() {
    let (overall, per, jwks, nonce, payload) = a17();
    let (_, pg) = verify_tokens(&overall, &per, &jwks, &nras_validation(ISSUER, false)).unwrap();
    assert!(payload.get("canned") == Some(&json!(true)));
    assert_eq!(
        decide(false, &pg, nonce, "2.0", GpuEvidenceMode::Canned, true).unwrap(),
        GpuOutcome::CannedTolerated
    );
    assert!(decide(false, &pg, nonce, "2.0", GpuEvidenceMode::Canned, false).is_err());
    assert!(decide(false, &pg, nonce, "2.0", GpuEvidenceMode::Real, true).is_err());
    assert!(
        decide(true, &pg, nonce, "2.0", GpuEvidenceMode::Real, false).is_err(),
        "no claim table rows on the captured per-GPU token"
    );
}
