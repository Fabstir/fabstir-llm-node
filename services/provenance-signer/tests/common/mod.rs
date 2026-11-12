// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Test harness: a staging chain generated per test, a local RFC 3161 TSA (`openssl ts`, REQUIRED on PATH:
//! tests fail, never skip, without it), an allow-listed test host key, a controllable clock.
#![allow(dead_code)]
use base64::Engine;
use c2pa::Context;
use fabstir_provenance::contract::{self, ReserveRequest, ReserveResponse, SignRequest, Slots};
use fabstir_provenance::defs::Params;
use fabstir_provenance::fingerprint::{parse_ffmpeg_xml, SignatureSet};
use fabstir_provenance::flow::{self, ReserveOnly};
use k256::ecdsa::SigningKey;
use provenance_signer::keystore::KeyStore;
use provenance_signer::repo::Repo;
use provenance_signer::service::{Service, Settings};
use provenance_signer::staging_chain;
use provenance_signer::store::Store;
use provenance_signer::tsa::TsaConfig;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub const FIX: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../crates/fabstir-provenance/tests/fixtures"
);
pub const PASS: &[u8] = b"test-passphrase-not-secret";
pub const T0: u64 = 1_794_000_000;
pub const URL: &str = "https://platformlessai.org/verify";

pub fn fixture(n: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}/{n}")).unwrap()
}

pub fn fingerprint() -> Vec<u8> {
    parse_ffmpeg_xml(&String::from_utf8(fixture("tiny-signature.xml")).unwrap())
        .unwrap()
        .pack()
        .unwrap()
}

pub fn other_fingerprint() -> Vec<u8> {
    let mut s = SignatureSet::unpack(&fingerprint()).unwrap();
    for f in s.frames.iter_mut() {
        f.trits.reverse();
    }
    s.pack().unwrap()
}

pub fn host_key() -> SigningKey {
    SigningKey::from_bytes((&[1u8; 32]).into()).unwrap()
}
pub fn other_key() -> SigningKey {
    SigningKey::from_bytes((&[2u8; 32]).into()).unwrap()
}
pub fn addr(k: &SigningKey) -> [u8; 20] {
    contract::address_of(k.verifying_key())
}

// ------------------------------------------------------------------------------------------ local TSA

pub struct LocalTsa {
    pub url: String,
    /// The TSA's issuing CA (add to the anchors for `timeStamp.trusted`).
    pub ca_pem: String,
    _dir: tempfile::TempDir,
}

/// A local RFC 3161 TSA answering every POST with `openssl ts -reply` (its own self-signed cert).
pub fn local_tsa() -> LocalTsa {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_path_buf();
    let run = |args: &[&str]| {
        let o = std::process::Command::new("openssl")
            .args(args)
            .current_dir(&d)
            .output()
            .expect("openssl must be on PATH for the signer tests");
        assert!(
            o.status.success(),
            "openssl {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    };
    // a test CA issues the TSA certificate (c2pa refuses self-signed TSA certificates)
    run(&[
        "ecparam",
        "-name",
        "prime256v1",
        "-genkey",
        "-noout",
        "-out",
        "ca.key",
    ]);
    run(&[
        "req",
        "-new",
        "-x509",
        "-sha256",
        "-key",
        "ca.key",
        "-days",
        "30",
        "-subj",
        "/CN=Local Test TSA Root",
        "-addext",
        "basicConstraints=critical,CA:TRUE",
        "-addext",
        "keyUsage=critical,keyCertSign,cRLSign",
        "-out",
        "ca.pem",
    ]);
    run(&[
        "ecparam",
        "-name",
        "prime256v1",
        "-genkey",
        "-noout",
        "-out",
        "tsa.key",
    ]);
    run(&[
        "req",
        "-new",
        "-sha256",
        "-key",
        "tsa.key",
        "-subj",
        "/CN=Local Test TSA",
        "-out",
        "tsa.csr",
    ]);
    std::fs::write(d.join("tsa.ext"), "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=critical,timeStamping\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid:always\n").unwrap();
    run(&[
        "x509",
        "-req",
        "-sha256",
        "-in",
        "tsa.csr",
        "-CA",
        "ca.pem",
        "-CAkey",
        "ca.key",
        "-CAcreateserial",
        "-days",
        "30",
        "-extfile",
        "tsa.ext",
        "-out",
        "tsa.pem",
    ]);
    let ca_pem = std::fs::read_to_string(d.join("ca.pem")).unwrap();
    std::fs::write(d.join("tsa.serial"), "01\n").unwrap();
    std::fs::write(
        d.join("tsa.cnf"),
        "oid_section = o\n[o]\np1 = 1.2.3.4.1\n[tsa]\ndefault_tsa = t\n[t]\ndir = .\nserial = $dir/tsa.serial\ncrypto_device = builtin\nsigner_cert = $dir/tsa.pem\ncerts = $dir/tsa.pem\nsigner_key = $dir/tsa.key\nsigner_digest = sha256\ndefault_policy = p1\nother_policies = p1\ndigests = sha256, sha384, sha512\naccuracy = secs:1\nordering = no\ntsa_name = yes\ness_cert_id_chain = no\ness_cert_id_alg = sha256\n",
    )
    .unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut n = 0u64;
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            n += 1;
            let mut buf = vec![];
            let mut tmp = [0u8; 4096];
            let (hdr_end, len) = loop {
                let k = s.read(&mut tmp).unwrap_or(0);
                if k == 0 {
                    break (0, 0);
                }
                buf.extend_from_slice(&tmp[..k]);
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let h = String::from_utf8_lossy(&buf[..p]).to_ascii_lowercase();
                    let len = h
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    break (p + 4, len);
                }
            };
            while buf.len() < hdr_end + len {
                let k = s.read(&mut tmp).unwrap_or(0);
                if k == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..k]);
            }
            let q = d.join(format!("q{n}.tsq"));
            let r = d.join(format!("r{n}.tsr"));
            std::fs::write(&q, &buf[hdr_end..]).unwrap();
            let ok = std::process::Command::new("openssl")
                .args([
                    "ts",
                    "-reply",
                    "-queryfile",
                    q.to_str().unwrap(),
                    "-config",
                    "tsa.cnf",
                    "-section",
                    "t",
                    "-out",
                    r.to_str().unwrap(),
                ])
                .current_dir(&d)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            let body = std::fs::read(&r).unwrap_or_default();
            let resp = if ok {
                let mut v = format!("HTTP/1.1 200 OK\r\nContent-Type: application/timestamp-reply\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
                v.extend_from_slice(&body);
                v
            } else {
                b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
            };
            let _ = s.write_all(&resp);
        }
    });
    LocalTsa {
        url: format!("http://127.0.0.1:{port}/"),
        ca_pem,
        _dir: dir,
    }
}

/// A URL nobody listens on.
pub fn dead_url() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    format!("http://127.0.0.1:{port}/")
}

// ------------------------------------------------------------------------------------------ env

pub struct Env {
    pub svc: Arc<Service>,
    pub clock: Arc<AtomicU64>,
    pub root_pem: String,
    pub tsa: LocalTsa,
    pub dir: tempfile::TempDir,
}

pub fn env_with(tsa_urls: impl FnOnce(&str) -> Vec<String>) -> Env {
    env_full(tsa_urls, |_| {})
}

pub fn env_full(
    tsa_urls: impl FnOnce(&str) -> Vec<String>,
    tweak: impl FnOnce(&mut Settings),
) -> Env {
    let chain = staging_chain::generate(false, PASS, 397, None).unwrap();
    let key = KeyStore::from_encrypted_pem(&chain.ee_key_encrypted_pem, PASS).unwrap();
    let chain_der: Vec<Vec<u8>> = pem::parse_many(chain.chain_pem.as_bytes())
        .unwrap()
        .into_iter()
        .map(|p| p.into_contents())
        .collect();
    let tsa = local_tsa();
    let urls = tsa_urls(&tsa.url);
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(AtomicU64::new(T0));
    let c2 = clock.clone();
    let settings = Settings {
        allowlist: [addr(&host_key())].into_iter().collect(),
        informational_url: URL.into(),
        reserve_min: 9_216,
        reserve_max: 16_384,
        reserve_recommended: 12_000,
        min_bundle_version: 49,
        reservations_per_minute: 600,
        chain_epoch: 1,
        trust_anchors: Some(chain.root_pem.clone()),
        node_versions: ["8.64.0".to_string()].into_iter().collect(),
        requests_per_minute: 1_200,
    };
    let mut settings = settings;
    tweak(&mut settings);
    let svc = Service::new(
        settings,
        Store::in_memory().unwrap(),
        Repo::open(&dir.path().join("repo")).unwrap(),
        Arc::new(key),
        chain_der,
        TsaConfig {
            urls,
            timeout: std::time::Duration::from_secs(5),
        },
        Arc::new(move || c2.load(Ordering::SeqCst)),
    )
    .unwrap();
    Env {
        svc: Arc::new(svc),
        clock,
        root_pem: chain.root_pem.clone(),
        tsa,
        dir,
    }
}

pub fn env() -> Env {
    env_with(|good| vec![good.to_string()])
}

impl Env {
    pub fn advance(&self, secs: u64) {
        self.clock.fetch_add(secs, Ordering::SeqCst);
    }
    pub fn host(&self) -> String {
        hex::encode(addr(&host_key()))
    }
    pub fn reserve(&self, template: &str, sequence: bool) -> ReserveResponse {
        self.svc
            .reserve(&self.host(), &reserve_req(template, sequence))
            .unwrap()
    }
}

pub fn reserve_req(template: &str, sequence: bool) -> ReserveRequest {
    ReserveRequest {
        template_id: template.into(),
        bundle_version: 49,
        node_version: "8.64.0".into(),
        slots: Slots {
            mp4: 1,
            sequence: sequence as u8,
        },
        deadline_secs: 1800,
    }
}

pub struct Prepared {
    pub req: SignRequest,
    pub body: Vec<u8>,
    pub prep: flow::Prepared,
}

/// The honest node for reservation params `p` (inputs per class), with fingerprint `fp`.
pub fn node_sign_request(rid: &str, p: &Params, fp: Option<&[u8]>, n_inputs: usize) -> Prepared {
    let src = fixture("tiny.mp4");
    let parent = fixture("tiny-parent.mp4");
    let still = fixture("still.png");
    let ctx = Context::new()
        .with_settings(flow::settings(None, false))
        .unwrap()
        .with_signer(Box::new(ReserveOnly(12_000)));
    let digest = fp.map(fabstir_provenance::fingerprint::digest);
    let mut formats: Vec<&str> = vec![];
    if p.class == fabstir_provenance::Class::Modified {
        formats.push("video/mp4");
    }
    formats.extend(std::iter::repeat_n("image/png", n_inputs));
    let _ = (&parent, &still);
    let (h, prep) = flow::prepare_mp4(ctx, p, digest.as_ref(), &formats, &src, 12_000).unwrap();
    let b64 = base64::engine::general_purpose::STANDARD;
    let req = SignRequest {
        reservation_id: rid.into(),
        slot: "mp4".into(),
        reserve_size: h.reserve,
        placeholder_len: h.placeholder_len,
        bmff_hash_sha256_hex: hex::encode(&h.bmff_hash),
        builder_archive_b64: b64.encode(&h.archive),
        fingerprint_b64: fp.map(|f| b64.encode(f)),
    };
    let body = serde_json::to_vec(&req).unwrap();
    Prepared { req, body, prep }
}

/// Reader validation of a final file against the staging root: (state, success codes, failure codes).
pub fn validate(
    bytes: &[u8],
    fmt: &str,
    anchors: &str,
) -> (String, Vec<String>, Vec<String>, c2pa::Reader) {
    let ctx = Context::new()
        .with_settings(flow::settings(Some(anchors), false))
        .unwrap();
    let r = c2pa::Reader::from_context(ctx)
        .with_stream(fmt, std::io::Cursor::new(bytes.to_vec()))
        .unwrap();
    let vr = r
        .validation_results()
        .map(|x| serde_json::to_value(x).unwrap())
        .unwrap_or_default();
    let codes = |k: &str| {
        vr["activeManifest"][k]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| c["code"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    (
        format!("{:?}", r.validation_state()),
        codes("success"),
        codes("failure"),
        r,
    )
}
