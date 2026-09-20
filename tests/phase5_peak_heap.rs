// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P5.5 (design S5): the attested load's peak heap is bounded by two
//! chunks, never by the model. A 512 MiB container is sealed to a FILE (never
//! a `Vec`), served by a loopback fake that STREAMS it from disk, fetched by
//! the real `HttpBlobSource` and decrypted by the loader; `VmHWM` (the
//! process's high-water mark of resident pages) is reset with
//! `/proc/self/clear_refs` right before the measured call and read after.
//!
//! Own binary on purpose: `VmHWM` is monotonic per process, so a shared test
//! binary that ever held 512 MiB would make the assertion vacuous; a file
//! written with `write(2)` is page cache, not RSS, so the plaintext does not
//! count; a fake holding the body in-process would count.

use fabstir_llm_node::tee::container::encrypt_model_to_writer;
use fabstir_llm_node::tee::http_sources::HttpBlobSource;
use fabstir_llm_node::tee::mock::{MockAttestationProvider, MockKeyBroker};
use fabstir_llm_node::tee::model_source::{EncryptedModelLoader, EncryptedModelSpec};
use fabstir_llm_node::tee::types::{CcMode, CvmPolicy, GpuPolicy, Policy};
use futures::TryStreamExt;
use http_body_util::{combinators::BoxBody, BodyExt, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::io::Read;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

const MIB: u64 = 1024 * 1024;
const MODEL_LEN: u64 = 512 * MIB;
const CHUNK: u32 = 4 * 1024 * 1024;
const BOUND: u64 = 128 * MIB;
const SKU: &str = "H100";
const MEASUREMENT: [u8; 48] = [0x42u8; 48];

fn policy(model_id: [u8; 32]) -> Policy {
    Policy {
        schema_version: 2,
        policy_version: 1,
        model_id,
        not_before: 0,
        expiry: u64::MAX - 1,
        cvm: CvmPolicy {
            mrtd: hex::encode(MEASUREMENT),
            rtmr0: "00".repeat(48),
            rtmr1: "00".repeat(48),
            rtmr2: "00".repeat(48),
            os_image_hash: "00".repeat(32),
            compose_hash: "00".repeat(32),
            app_id: None,
            key_provider: None,
            require_td_debug_off: true,
            allowed_tcb_status: vec!["UpToDate".to_string()],
            allowed_advisory_ids: vec![],
        },
        gpu: GpuPolicy {
            allowed_hwmodels: vec![SKU.to_string()],
            require_cc_mode: Some(CcMode::On),
            require_secure_boot: true,
            require_debug_disabled: true,
            min_driver_version: None,
            min_vbios_version: None,
        },
    }
}

/// `VmHWM` from `/proc/self/status`, in bytes.
fn vm_hwm() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .expect("VmHWM");
    let kb: u64 = line.split_whitespace().nth(1).unwrap().parse().unwrap();
    kb * 1024
}

/// Reset the peak (`5` resets VmHWM/VmRSS peaks on Linux ≥ 4.0).
fn reset_peak() {
    std::fs::write("/proc/self/clear_refs", b"5\n").expect("clear_refs writable");
}

async fn spawn_streaming(file: PathBuf) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            let file = file.clone();
            tokio::spawn(async move {
                let svc = service_fn(move |_req: Request<Incoming>| {
                    let file = file.clone();
                    async move {
                        let len = std::fs::metadata(&file).unwrap().len();
                        let f = tokio::fs::File::open(&file).await.unwrap();
                        let stream = tokio_util::io::ReaderStream::new(f).map_ok(Frame::data);
                        let body: BoxBody<Bytes, std::io::Error> =
                            BodyExt::boxed(StreamBody::new(stream));
                        Ok::<_, std::convert::Infallible>(
                            Response::builder()
                                .status(200)
                                .header("content-length", len.to_string())
                                .body(body)
                                .unwrap(),
                        )
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(tcp), svc)
                    .await;
            });
        }
    });
    addr
}

#[tokio::test]
async fn peak_heap_stays_below_the_container_size() {
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let model_id = [0x77u8; 32];
    let policy_hash = [0x88u8; 32];
    let dek = [0x99u8; 32];
    // Seal 512 MiB from a repeating reader to a file: never a Vec.
    let sealed = tmp.path().join("target.enc");
    {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&sealed).unwrap());
        let reader = std::io::repeat(0xAB).take(MODEL_LEN);
        encrypt_model_to_writer(
            reader,
            MODEL_LEN,
            &mut out,
            &dek,
            model_id,
            policy_hash,
            CHUNK,
            [5u8; 16],
        )
        .unwrap();
    }
    let addr = spawn_streaming(sealed.clone()).await;
    let blob = HttpBlobSource::new(
        &format!("http://{addr}"),
        MODEL_LEN + 64 * MIB,
        Duration::from_secs(600),
    )
    .unwrap();
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, policy(model_id)))]));
    let provider = MockAttestationProvider::new(SKU, MEASUREMENT, CcMode::On);
    let loader = EncryptedModelLoader::new(tmp.path().join("decrypt"))
        .with_tee_enabled(true)
        .with_container_dir(tmp.path().join("containers"));
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: "target.enc".to_string(),
    };

    reset_peak();
    let before = vm_hwm();
    // The reset worked: the baseline is the debug binary's own resident pages
    // (code + runtime, ~150 MiB here), never a model-sized peak.
    assert!(
        before < MODEL_LEN / 2,
        "precondition: the process already sits at {} MiB before the measured call \
         (clear_refs did not reset the peak?)",
        before / MIB
    );
    eprintln!("VmHWM before the load: {} MiB", before / MIB);
    let (path, digest, _) = loader
        .prepare_encrypted_model_with_digest(&blob, &kbs, &provider, &spec)
        .await
        .expect("the attested load");
    let after = vm_hwm();
    eprintln!("VmHWM after the load: {} MiB", after / MIB);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), MODEL_LEN);
    let delta = after.saturating_sub(before);
    assert!(
        delta < BOUND,
        "peak heap grew by {} MiB during a {} MiB load (a Vec<u8> download would be ≥ {} MiB)",
        delta / MIB,
        MODEL_LEN / MIB,
        MODEL_LEN / MIB
    );
    // And the digest is the model's: 512 MiB of 0xAB.
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    let block = vec![0xABu8; 1 << 20];
    for _ in 0..(MODEL_LEN >> 20) {
        sha2::Digest::update(&mut hasher, &block);
    }
    let want: [u8; 32] = sha2::Digest::finalize(hasher).into();
    assert_eq!(digest, want);
    loader.release(&model_id, &policy_hash);
    loader.evict_unreferenced();
}
