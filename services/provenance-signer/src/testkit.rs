// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `testkit` feature: an in-process signer for OTHER crates' tests (the node's AP2 tests): a staging chain
//! generated per call, a local RFC 3161 TSA (`openssl ts`, REQUIRED on PATH: tests fail, never skip), a
//! ready `Service`. Never enabled in a release build.
use crate::keystore::KeyStore;
use crate::repo::Repo;
use crate::service::{Service, Settings};
use crate::staging_chain;
use crate::store::Store;
use crate::tsa::TsaConfig;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub const PASS: &[u8] = b"testkit-passphrase-not-secret";

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


pub struct TestSigner {
    pub svc: Arc<Service>,
    pub clock: Arc<AtomicU64>,
    /// Our staging root + the local TSA's root.
    pub anchors_pem: String,
    pub tsa: LocalTsa,
    pub dir: tempfile::TempDir,
}

impl TestSigner {
    /// A signer allow-listing `host` (20-byte address) and `node_versions`.
    pub fn new(host: [u8; 20], node_versions: &[&str]) -> TestSigner {
        Self::new_with_min_bundle(host, node_versions, 49)
    }

    /// Same, accepting bundles from `min_bundle_version` (the node repo's own test templates are older).
    pub fn new_with_min_bundle(host: [u8; 20], node_versions: &[&str], min_bundle_version: u32) -> TestSigner {
        let chain = staging_chain::generate(false, PASS, 397, None).unwrap();
        let key = KeyStore::from_encrypted_pem(&chain.ee_key_encrypted_pem, PASS).unwrap();
        let chain_der: Vec<Vec<u8>> = pem::parse_many(chain.chain_pem.as_bytes()).unwrap().into_iter().map(|p| p.into_contents()).collect();
        let tsa = local_tsa();
        let dir = tempfile::tempdir().unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let clock = Arc::new(AtomicU64::new(now));
        let c2 = clock.clone();
        let settings = Settings {
            allowlist: [host].into_iter().collect(),
            informational_url: "https://platformlessai.org/verify".into(),
            reserve_min: 9_216,
            reserve_max: 16_384,
            reserve_recommended: 12_000,
            min_bundle_version,
            reservations_per_minute: 600,
            chain_epoch: 1,
            trust_anchors: Some(chain.root_pem.clone()),
            node_versions: node_versions.iter().map(|s| s.to_string()).collect(),
            requests_per_minute: 6_000,
        };
        let svc = Service::new(
            settings,
            Store::in_memory().unwrap(),
            Repo::open(&dir.path().join("repo")).unwrap(),
            Arc::new(key),
            chain_der,
            TsaConfig { urls: vec![tsa.url.clone()], timeout: std::time::Duration::from_secs(5) },
            Arc::new(move || c2.load(Ordering::SeqCst)),
        )
        .unwrap();
        TestSigner { svc: Arc::new(svc), clock, anchors_pem: format!("{}{}", chain.root_pem, tsa.ca_pem), tsa, dir }
    }
}
