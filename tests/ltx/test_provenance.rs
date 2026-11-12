// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! AP2 (IMPLEMENTATION-AI-PROVENANCE.md): the node's `ltx::provenance` against the REAL signer run in-process
//! (`provenance-signer` testkit: staging chain, local RFC 3161 TSA). The pinned ffmpeg comes from
//! `PROVENANCE_FFMPEG`; these tests FAIL, never skip, when it is absent (D3).
use async_trait::async_trait;
use fabstir_llm_node::ltx::provenance::ffmpeg::Ffmpeg;
use fabstir_llm_node::ltx::provenance::finalise::{
    finalise_exr, finalise_mp4, finalise_sequence, mdat_bytes, ProvenanceCtx,
};
use fabstir_llm_node::ltx::provenance::ingredient_formats;
use fabstir_llm_node::ltx::provenance::signer_client::{
    with_retry, HttpSigner, ProvenanceSigner, SignerError,
};
use fabstir_llm_node::ltx::types::{LtxJob, OutputKind, Resolution};
use fabstir_provenance::c2pa::{Context, Reader};
use fabstir_provenance::contract::{
    self, Limits, ReserveRequest, ReserveResponse, SignRequest, SignSequenceRequest, Slots,
};
use fabstir_provenance::{class::lookup, flow, frames_index};
use provenance_signer::service::Fail;
use provenance_signer::testkit::TestSigner;
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const FIX: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/crates/fabstir-provenance/tests/fixtures"
);
const NODE_VERSION: &str = "8.64.0-provenance-test";
const HOST_KEY: [u8; 32] = [7u8; 32];
const PROMPT: &str = "a lighthouse keeper feeding seagulls at dawn";

fn fixture(n: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}/{n}")).unwrap()
}

async fn ffmpeg() -> Ffmpeg {
    Ffmpeg::from_env()
        .await
        .expect("PROVENANCE_FFMPEG must point at the pinned ffmpeg 7.0.2 (tests never skip)")
}

fn host_addr() -> [u8; 20] {
    contract::address_of(
        k256::ecdsa::SigningKey::from_bytes((&HOST_KEY).into())
            .unwrap()
            .verifying_key(),
    )
}

/// The real signer Service, called in-process (counts calls).
struct InProc {
    ts: TestSigner,
    host: String,
    signs: AtomicUsize,
}

fn err(f: Fail) -> SignerError {
    SignerError {
        code: Some(f.0),
        message: f.1,
        retryable: f.0.retryable(),
        limits: None,
    }
}

#[async_trait]
impl ProvenanceSigner for InProc {
    async fn limits(&self) -> Result<Limits, SignerError> {
        Ok(self.ts.svc.limits())
    }
    async fn reserve(&self, req: &ReserveRequest) -> Result<ReserveResponse, SignerError> {
        self.ts.svc.reserve(&self.host, req).map_err(err)
    }
    async fn sign(&self, req: &SignRequest) -> Result<Vec<u8>, SignerError> {
        self.signs.fetch_add(1, Ordering::SeqCst);
        let body = serde_json::to_vec(req).unwrap();
        self.ts
            .svc
            .sign_mp4(&self.host, &body, req)
            .map(|s| s.manifest)
            .map_err(err)
    }
    async fn sign_sequence(&self, req: &SignSequenceRequest) -> Result<Vec<u8>, SignerError> {
        let body = serde_json::to_vec(req).unwrap();
        self.ts
            .svc
            .sign_sequence(&self.host, &body, req)
            .map(|s| s.manifest)
            .map_err(err)
    }
}

fn in_proc() -> InProc {
    InProc {
        ts: TestSigner::new(
            host_addr(),
            &[NODE_VERSION, fabstir_llm_node::version::VERSION],
        ),
        host: hex::encode(host_addr()),
        signs: AtomicUsize::new(0),
    }
}

async fn reserve(s: &dyn ProvenanceSigner, template: &str, exr: bool) -> ReserveResponse {
    s.reserve(&ReserveRequest {
        template_id: template.into(),
        bundle_version: 49,
        node_version: NODE_VERSION.into(),
        slots: Slots {
            mp4: 1,
            sequence: exr as u8,
        },
        deadline_secs: 1800,
    })
    .await
    .unwrap()
}

fn ctx<'a>(
    s: &'a dyn ProvenanceSigner,
    f: &'a Ffmpeg,
    r: ReserveResponse,
    template: &str,
    formats: Vec<&'static str>,
) -> ProvenanceCtx<'a> {
    ProvenanceCtx {
        reservation: r,
        ingredient_formats: formats,
        row: lookup(template).unwrap(),
        signer: s,
        ffmpeg: f,
        reserve: 12_000,
        deadline: Instant::now() + Duration::from_secs(60),
        time: "2026-11-09T12:00:00Z".into(),
        resolver_url: "https://platformlessai.org/verify".into(),
        exr_hashes: vec![],
        mp4_done: false,
        sequence_signed: false,
    }
}

fn read(bytes: &[u8], anchors: &str) -> (String, Vec<String>, Vec<String>, serde_json::Value) {
    let c = Context::new()
        .with_settings(flow::settings(Some(anchors), false))
        .unwrap();
    let r = Reader::from_context(c)
        .with_stream("video/mp4", Cursor::new(bytes.to_vec()))
        .unwrap();
    let j: serde_json::Value = serde_json::from_str(&r.json()).unwrap();
    let codes = |k: &str| {
        j["validation_results"]["activeManifest"][k]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|c| c["code"].as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    (
        j["validation_state"].as_str().unwrap_or("").to_string(),
        codes("success"),
        codes("failure"),
        j,
    )
}

fn active(j: &serde_json::Value) -> &serde_json::Value {
    &j["manifests"][j["active_manifest"].as_str().unwrap()]
}

#[tokio::test]
async fn delivered_mp4_carries_valid_manifest() {
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-t2v-hdr", false).await;
    let mut c = ctx(&s, &f, r, "ltx-t2v-hdr", vec![]);
    let out = finalise_mp4(&mut c, fixture("tiny.mp4")).await.unwrap();
    let (state, ok, bad, j) = read(&out, &s.ts.anchors_pem);
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(state, "Trusted");
    assert!(ok.iter().any(|c| c == "assertion.bmffHash.match"));
    let acts = &active(&j)["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["label"] == "c2pa.actions.v2")
        .unwrap()["data"]["actions"];
    assert_eq!(acts[0]["action"], "c2pa.created");
}

#[tokio::test]
async fn manifest_is_timestamped() {
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-t2v-hdr", false).await;
    let out = finalise_mp4(
        &mut ctx(&s, &f, r, "ltx-t2v-hdr", vec![]),
        fixture("tiny.mp4"),
    )
    .await
    .unwrap();
    let (_, ok, _, _) = read(&out, &s.ts.anchors_pem);
    assert!(ok.iter().any(|c| c == "timeStamp.validated"), "{ok:?}");
}

#[tokio::test]
async fn mp4_manifest_has_one_hard_binding() {
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-sdr2hdr-hdr", true).await;
    let out = finalise_mp4(
        &mut ctx(&s, &f, r, "ltx-sdr2hdr-hdr", vec!["video/mp4"]),
        fixture("tiny.mp4"),
    )
    .await
    .unwrap();
    let (_, _, _, j) = read(&out, &s.ts.anchors_pem);
    let hard: Vec<_> = active(&j)["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["label"].as_str().unwrap().starts_with("c2pa.hash."))
        .collect();
    assert_eq!(hard.len(), 1);
    assert_eq!(hard[0]["label"], "c2pa.hash.bmff.v3");
}

#[tokio::test]
async fn pixels_untouched() {
    let s = in_proc();
    let f = ffmpeg().await;
    let src = fixture("tiny.mp4");
    let r = reserve(&s, "ltx-t2v-hdr", false).await;
    let out = finalise_mp4(&mut ctx(&s, &f, r, "ltx-t2v-hdr", vec![]), src.clone())
        .await
        .unwrap();
    assert!(!mdat_bytes(&src).is_empty());
    assert_eq!(mdat_bytes(&out), mdat_bytes(&src));
}

#[tokio::test]
async fn fingerprint_is_of_delivered_bytes() {
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-t2v-hdr", false).await;
    let uuid = r.params.manifest_uuid.clone();
    let out = finalise_mp4(
        &mut ctx(&s, &f, r, "ltx-t2v-hdr", vec![]),
        fixture("tiny.mp4"),
    )
    .await
    .unwrap();
    // recomputed on the FINAL embedded file
    let final_set = f.fingerprint(&out).await.unwrap().pack().unwrap();
    let digest = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        fabstir_provenance::fingerprint::digest(&final_set),
    );
    let (_, _, _, j) = read(&out, &s.ts.anchors_pem);
    let sb = active(&j)["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["label"] == "c2pa.soft-binding")
        .unwrap();
    assert_eq!(sb["data"]["blocks"][0]["value"], digest);
    assert_eq!(
        s.ts.svc.repo.get(&uuid).unwrap().fingerprint.unwrap(),
        final_set,
        "the registered set is the delivered file's"
    );
}

#[tokio::test]
async fn fingerprint_only_for_picture_role() {
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "cosmos-passes-key", false).await;
    let uuid = r.params.manifest_uuid.clone();
    let out = finalise_mp4(
        &mut ctx(&s, &f, r, "cosmos-passes-key", vec![]),
        fixture("tiny.mp4"),
    )
    .await
    .unwrap();
    let (_, _, bad, j) = read(&out, &s.ts.anchors_pem);
    assert!(bad.is_empty());
    assert!(!active(&j)["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["label"] == "c2pa.soft-binding"));
    assert!(s.ts.svc.repo.get(&uuid).unwrap().fingerprint.is_none());
    // a Picture template's reservation handled as Data (or the reverse) is refused, not signed
    let r2 = reserve(&s, "ltx-t2v-hdr", false).await;
    assert!(finalise_mp4(
        &mut ctx(&s, &f, r2, "cosmos-passes-key", vec![]),
        fixture("tiny.mp4")
    )
    .await
    .is_err());
}

#[tokio::test]
async fn fingerprint_failure_fails_closed() {
    let s = in_proc();
    let r = reserve(&s, "ltx-t2v-hdr", false).await;
    let missing = Ffmpeg {
        path: "/nonexistent/ffmpeg".into(),
    };
    assert!(finalise_mp4(
        &mut ctx(&s, &missing, r, "ltx-t2v-hdr", vec![]),
        fixture("tiny.mp4")
    )
    .await
    .is_err());
    // an unreadable picture gives no signature at all
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-t2v-hdr", false).await;
    let junk = [&fixture("tiny.mp4")[..64], &[0u8; 512][..]].concat();
    assert!(
        finalise_mp4(&mut ctx(&s, &f, r, "ltx-t2v-hdr", vec![]), junk)
            .await
            .is_err()
    );
    assert_eq!(
        s.signs.load(Ordering::SeqCst),
        0,
        "nothing was sent to the signer"
    );
}

#[tokio::test]
async fn ffmpeg_must_be_the_pinned_binary() {
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("ffmpeg");
    std::fs::write(&fake, b"#!/bin/sh\necho MPEG-7 video signature\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // runnable and claims the filter: only the pinned hash refuses it
    assert!(Ffmpeg { path: fake }.check().await.is_err());
    ffmpeg().await.check().await.unwrap();
}

#[tokio::test]
async fn refuses_mp4_with_escaped_comfy_graph() {
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-t2v-hdr", false).await;
    // VHS's double-encoded graph in a udta box before mdat
    let src = fixture("tiny.mp4");
    let boxes = flow::top_boxes(&src);
    let (_, moov_off, moov_sz) = boxes.iter().find(|b| b.0 == "moov").cloned().unwrap();
    let mut moov = src[moov_off..moov_off + moov_sz].to_vec();
    let graph = format!(
        r#"{{\"6\": {{\"inputs\": {{\"text\": \"{PROMPT}\"}}, \"class_type\": \"CLIPTextEncode\"}}}}"#
    );
    let mut udta = ((graph.len() + 8) as u32).to_be_bytes().to_vec();
    udta.extend_from_slice(b"udta");
    udta.extend_from_slice(graph.as_bytes());
    let new_size = (moov.len() + udta.len()) as u32;
    moov[..4].copy_from_slice(&new_size.to_be_bytes());
    moov.extend_from_slice(&udta);
    let leaky = [&src[..moov_off], &moov[..], &src[moov_off + moov_sz..]].concat();
    let e = finalise_mp4(&mut ctx(&s, &f, r, "ltx-t2v-hdr", vec![]), leaky)
        .await
        .unwrap_err();
    assert!(e.to_string().contains("leak guard"), "{e}");
    assert_eq!(s.signs.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn delivered_file_has_no_prompt_or_identity() {
    // what the node ADDS to a delivered file (the manifest) carries none of the job's identity
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-i2v-hdr", false).await;
    let rid = r.reservation_id.clone();
    let src = fixture("tiny.mp4");
    let out = finalise_mp4(
        &mut ctx(&s, &f, r, "ltx-i2v-hdr", vec!["image/png"]),
        src.clone(),
    )
    .await
    .unwrap();
    let (off, sz) = flow::find_c2pa_box(&out).unwrap();
    let added = String::from_utf8_lossy(&out[off..off + sz]).to_string();
    for needle in [
        PROMPT,
        &rid,
        &hex::encode(host_addr()),
        "seed",
        "job",
        "session",
        "0x",
    ] {
        assert!(!added.contains(needle), "manifest contains {needle:?}");
    }
}

#[tokio::test]
async fn finalise_never_lands_on_a_256k_multiple() {
    // pad the source with a `free` box so that the default reserve would land exactly on a multiple
    let s = in_proc();
    let f = ffmpeg().await;
    let src = fixture("tiny.mp4");
    let r = reserve(&s, "ltx-t2v-hdr", false).await;
    let probe_ctx = Context::new()
        .with_settings(flow::settings(None, false))
        .unwrap()
        .with_signer(Box::new(flow::ReserveOnly(12_000)));
    let digest = [0u8; 32];
    let (_, prep) =
        flow::prepare_mp4(probe_ctx, &r.params, Some(&digest), &[], &src, 12_000).unwrap();
    let added = flow::final_len(&prep) - src.len();
    let target = (src.len() + added + 8).div_ceil(262_144) * 262_144;
    let free = target - src.len() - added;
    let mut fb = (free as u32).to_be_bytes().to_vec();
    fb.extend_from_slice(b"free");
    fb.resize(free, 0);
    let padded = [&src[..], &fb[..]].concat();
    assert_eq!((padded.len() + added) % 262_144, 0, "test construction");
    let out = finalise_mp4(&mut ctx(&s, &f, r, "ltx-t2v-hdr", vec![]), padded)
        .await
        .unwrap();
    assert_ne!(out.len() % 262_144, 0);
    let (_, _, bad, _) = read(&out, &s.ts.anchors_pem);
    assert!(bad.is_empty(), "{bad:?}");
}

// ------------------------------------------------------------------------------------------ EXR

fn exr_frame(seed: u8) -> Vec<u8> {
    // a minimal real single-part EXR, written by the `exr` crate
    use exr::prelude::*;
    let pixels = SpecificChannels::rgb(|pos: Vec2<usize>| {
        (
            f32::from(seed) / 255.0,
            pos.x() as f32 / 16.0,
            pos.y() as f32 / 8.0,
        )
    });
    let img = Image::from_channels((16, 8), pixels);
    let mut c = Cursor::new(Vec::new());
    img.write().to_buffered(&mut c).unwrap();
    c.into_inner()
}

#[tokio::test]
async fn exr_sequence_manifest_lists_every_frame() {
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-sdr2hdr-hdr", true).await;
    let seq_uuid = r.sequence.clone().unwrap().manifest_uuid;
    let mut c = ctx(&s, &f, r, "ltx-sdr2hdr-hdr", vec!["video/mp4"]);
    finalise_mp4(&mut c, fixture("tiny.mp4")).await.unwrap();
    let frames: Vec<Vec<u8>> = (0..3)
        .map(|i| finalise_exr(&mut c, &exr_frame(i)).unwrap())
        .collect();
    finalise_sequence(&mut c).await.unwrap();
    let idx = s.ts.svc.repo.get(&seq_uuid).unwrap().frames_index.unwrap();
    for (i, fr) in frames.iter().enumerate() {
        assert_eq!(
            frames_index::lookup(&idx, &frames_index::frame_name(i + 1)).unwrap(),
            hex::encode(<sha2::Sha256 as sha2::Digest>::digest(fr))
        );
        assert!(
            fabstir_provenance::exr_header::read_records(fr).unwrap()[0].is_some(),
            "exr_frame_has_provenance_record"
        );
    }
    assert!(frames_index::lookup(&idx, &frames_index::frame_name(4)).is_none());
}

#[tokio::test]
async fn alpha_mattes_are_indexed_as_data() {
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-alpha-hdr", true).await;
    let seq_uuid = r.sequence.clone().unwrap().manifest_uuid;
    let mut c = ctx(&s, &f, r, "ltx-alpha-hdr", vec!["video/mp4"]);
    finalise_mp4(&mut c, fixture("tiny.mp4")).await.unwrap();
    finalise_exr(&mut c, &exr_frame(1)).unwrap();
    finalise_sequence(&mut c).await.unwrap();
    let idx = s.ts.svc.repo.get(&seq_uuid).unwrap().frames_index.unwrap();
    assert!(String::from_utf8_lossy(&idx).contains("\"class\":\"data\""));
}

// ------------------------------------------------------------------------------------------ signer client

#[tokio::test]
async fn sign_retry_fresh_nonce_and_bounded() {
    // retryable failures are retried (each a fresh request), non-retryable ones are not, and the window ends
    let n = AtomicUsize::new(0);
    let r: Result<u32, SignerError> =
        with_retry(Instant::now() + Duration::from_secs(30), || async {
            if n.fetch_add(1, Ordering::SeqCst) < 2 {
                Err(SignerError::transport("down"))
            } else {
                Ok(7)
            }
        })
        .await;
    assert_eq!(r.unwrap(), 7);
    assert_eq!(n.load(Ordering::SeqCst), 3);
    let m = AtomicUsize::new(0);
    let r: Result<u32, SignerError> =
        with_retry(Instant::now() + Duration::from_secs(30), || async {
            m.fetch_add(1, Ordering::SeqCst);
            Err(SignerError {
                code: Some(contract::ErrorCode::AssertionMismatch),
                message: "x".into(),
                retryable: false,
                limits: None,
            })
        })
        .await;
    assert!(r.is_err());
    assert_eq!(m.load(Ordering::SeqCst), 1, "a refusal is final");
    let t0 = Instant::now();
    let r: Result<u32, SignerError> =
        with_retry(Instant::now() + Duration::from_secs(3), || async {
            Err(SignerError::transport("down"))
        })
        .await;
    assert!(r.is_err());
    assert!(
        t0.elapsed() < Duration::from_secs(5),
        "never past the job deadline"
    );
}

#[tokio::test]
async fn http_signer_round_trip_with_fresh_nonces() {
    // the real HTTP route end to end: two signed requests in a row must not collide on a nonce
    let ts = TestSigner::new(
        host_addr(),
        &[NODE_VERSION, fabstir_llm_node::version::VERSION],
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let app = provenance_signer::app::api_router(ts.svc.clone());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let h = HttpSigner::new(&base, &HOST_KEY).unwrap();
    let a = reserve(&h, "ltx-t2v-hdr", false).await;
    let b = reserve(&h, "ltx-t2v-hdr", false).await;
    assert_ne!(a.reservation_id, b.reservation_id);
    assert_eq!(h.limits().await.unwrap().reserve_recommended, 12_000);
    let wrong = HttpSigner::new(&base, &[9u8; 32]).unwrap();
    let e = wrong
        .reserve(&ReserveRequest {
            template_id: "ltx-t2v-hdr".into(),
            bundle_version: 49,
            node_version: NODE_VERSION.into(),
            slots: Slots {
                mp4: 1,
                sequence: 0,
            },
            deadline_secs: 60,
        })
        .await
        .unwrap_err();
    assert_eq!(e.code, Some(contract::ErrorCode::Unauthorized));
}

// ------------------------------------------------------------------------------------------ ingredients

fn job(template: &str, images: usize, videos: usize) -> LtxJob {
    serde_json::from_value(serde_json::json!({
        "templateId": template, "templateHash": "0x00", "prompt": PROMPT, "seed": "1", "frames": 49, "fps": 24,
        "resolution": {"w": 1280, "h": 704}, "lora": "", "output": "exr-sequence",
        "images": (images > 0).then(|| vec!["c"; images]), "videos": (videos > 0).then(|| vec!["v"; videos]),
    }))
    .unwrap()
}

#[test]
fn ingredient_formats_follow_the_class() {
    let r = |t: &str| lookup(t).unwrap();
    assert_eq!(
        ingredient_formats(r("ltx-t2v-hdr"), &job("ltx-t2v-hdr", 0, 0)).unwrap(),
        Vec::<&str>::new()
    );
    assert_eq!(
        ingredient_formats(r("ltx-i2v-hdr"), &job("ltx-i2v-hdr", 1, 0)).unwrap(),
        vec!["image/png"]
    );
    assert_eq!(
        ingredient_formats(
            r("ltx-layout-chain-hdr"),
            &job("ltx-layout-chain-hdr", 1, 2)
        )
        .unwrap(),
        vec!["video/mp4", "video/mp4", "image/png"]
    );
    assert_eq!(
        ingredient_formats(
            r("ltx-restore25-ref-hdr"),
            &job("ltx-restore25-ref-hdr", 1, 1)
        )
        .unwrap(),
        vec!["video/mp4", "image/png"]
    );
    assert_eq!(
        ingredient_formats(r("cosmos-passes-key"), &job("cosmos-passes-key", 0, 1)).unwrap(),
        Vec::<&str>::new()
    );
    assert!(
        ingredient_formats(r("ltx-edit-hdr"), &job("ltx-edit-hdr", 0, 0)).is_err(),
        "a modified output needs the clip"
    );
    let _ = OutputKind::ExrSequence;
    let _ = Resolution { w: 1, h: 1 };
}

#[tokio::test]
async fn outputs_are_routed_by_type_and_fail_closed() {
    use fabstir_llm_node::ltx::provenance::finalise::finalise_output;
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-sdr2hdr-hdr", true).await;
    let mut c = ctx(&s, &f, r, "ltx-sdr2hdr-hdr", vec!["video/mp4"]);
    finalise_output(&mut c, "LTX_00001.mp4", fixture("tiny.mp4"))
        .await
        .unwrap();
    let second = finalise_output(&mut c, "LTX_00002.mp4", fixture("tiny.mp4"))
        .await
        .unwrap_err()
        .to_string();
    assert!(
        second.contains("no reserved manifest slot"),
        "the node refuses before the signer: {second}"
    );
    finalise_output(&mut c, "frame_00001.exr", exr_frame(3))
        .await
        .unwrap();
    assert_eq!(c.exr_hashes.len(), 1);
    assert!(
        finalise_output(&mut c, "preview.webm", vec![1, 2, 3])
            .await
            .is_err(),
        "unmarkable type"
    );
}

// ------------------------------------------------------------------------------------------ D18 startup invariant

#[test]
fn template_load_refuses_unclassed() {
    use fabstir_llm_node::ltx::provenance::check_templates_classed;
    use fabstir_llm_node::ltx::TemplateStore;
    let v49 = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/fixtures/v49");
    check_templates_classed(&TemplateStore::new(v49).unwrap()).unwrap();
    // the same bundle with one template renamed to an id the class table does not know
    let dir = tempfile::tempdir().unwrap();
    let copy = |from: &std::path::Path, to: &std::path::Path| {
        for e in walk(from) {
            let rel = e.strip_prefix(from).unwrap();
            let dest = to.join(rel);
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::copy(&e, &dest).unwrap();
        }
    };
    copy(std::path::Path::new(v49), dir.path());
    std::fs::rename(
        dir.path().join("ltx-t2v-hdr"),
        dir.path().join("ltx-newmode-hdr"),
    )
    .unwrap();
    let al = dir.path().join("allowlist.json");
    let text = std::fs::read_to_string(&al)
        .unwrap()
        .replace("\"ltx-t2v-hdr\"", "\"ltx-newmode-hdr\"");
    std::fs::write(&al, text).unwrap();
    let e = check_templates_classed(&TemplateStore::new(dir.path()).unwrap())
        .unwrap_err()
        .to_string();
    assert!(e.contains("ltx-newmode-hdr"), "{e}");
}

fn walk(d: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

struct OldSigner;
#[async_trait]
impl ProvenanceSigner for OldSigner {
    async fn limits(&self) -> Result<Limits, SignerError> {
        let ts = TestSigner::new(host_addr(), &[NODE_VERSION]);
        let mut l = ts.svc.limits();
        l.c2pa_crate = "0.91.0".into();
        Ok(l)
    }
    async fn reserve(&self, _: &ReserveRequest) -> Result<ReserveResponse, SignerError> {
        unreachable!()
    }
    async fn sign(&self, _: &SignRequest) -> Result<Vec<u8>, SignerError> {
        unreachable!()
    }
    async fn sign_sequence(&self, _: &SignSequenceRequest) -> Result<Vec<u8>, SignerError> {
        unreachable!()
    }
}

#[tokio::test]
async fn ltx_refuses_to_enable_unmarked() {
    use fabstir_llm_node::ltx::provenance::Runtime;
    // a signer on another c2pa version would make every comparison fail: refuse at startup
    assert!(Runtime::check(Arc::new(OldSigner), ffmpeg().await)
        .await
        .is_err());
    // the real one passes
    let s = Arc::new(in_proc());
    assert!(Runtime::check(s, ffmpeg().await).await.is_ok());
    // no ffmpeg, no LTX
    assert!(Ffmpeg {
        path: "/nonexistent".into()
    }
    .check()
    .await
    .is_err());
    // and main gates the sidecars on it (source pin: no ComfyClient is installed outside the Ok arm)
    let main =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs")).unwrap();
    let gate = main
        .find("provenance::Runtime::from_env()")
        .expect("main checks provenance");
    let first_client = main.find("set_ltx_client(").unwrap();
    assert!(
        gate < first_client,
        "provenance is checked before LTX is enabled"
    );
    assert!(
        main[gate..first_client].contains("(None, None)"),
        "a failed check disables LTX"
    );
}

// ------------------------------------------------------------------------------------------ run() source pins

fn run_body() -> String {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/api/websocket/handlers/ltx.rs"
    ))
    .unwrap();
    let start = src.find("pub async fn run(").unwrap();
    // whitespace-free, so rustfmt line breaks never move a pin
    src[start..]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

#[test]
fn output_cid_binds_marked_bytes() {
    // the marked bytes are what is uploaded and hashed: finalise sits between the fetch and the upload
    let run = run_body();
    let dl = run.find("client.download(r)").unwrap();
    let fin = run
        .find("finalise_output(&mutprov,&r.filename,bytes")
        .expect("outputs are finalised");
    let up = run.find("upload_frame_with_retry(").unwrap();
    assert!(dl < fin && fin < up);
}

#[test]
fn no_proof_submitted_when_marking_fails() {
    let run = run_body();
    let fin = run
        .find("finalise_output(&mutprov,&r.filename,bytes")
        .unwrap();
    let arm = &run[fin..fin + 400];
    assert!(
        arm.contains("PROVENANCE_FAILED") && arm.contains("return;"),
        "a marking failure ends the job"
    );
    let seq = run
        .find("finalise_sequence(&mutprov)")
        .expect("the sequence manifest is signed");
    let seq_arm = &run[seq..seq + 250];
    assert!(seq_arm.contains("return;"));
    let manifest = run.find("exr::build_manifest(").unwrap();
    let submit = run.find("submit::finalize_clip(").unwrap();
    assert!(
        seq < manifest && manifest < submit,
        "signed before the proof is built"
    );
    // and a task without a grant never reaches an upload
    let none = run
        .find("letSome(grant)=provenanceelse")
        .expect("fails closed without a grant");
    assert!(none < run.find("upload_frame_with_retry(").unwrap());
}

#[tokio::test]
async fn a_reserved_sequence_must_be_signed_before_the_proof() {
    use fabstir_llm_node::ltx::provenance::finalise::check_complete;
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-sdr2hdr-hdr", true).await;
    let mut c = ctx(&s, &f, r, "ltx-sdr2hdr-hdr", vec!["video/mp4"]);
    assert!(check_complete(&c).is_err(), "nothing marked yet");
    finalise_mp4(&mut c, fixture("tiny.mp4")).await.unwrap();
    c.mp4_done = true;
    assert!(check_complete(&c).is_err(), "sequence reserved, not signed");
    assert!(
        finalise_sequence(&mut c).await.is_err(),
        "a reserved sequence with no frames is an error"
    );
    finalise_exr(&mut c, &exr_frame(2)).unwrap();
    finalise_sequence(&mut c).await.unwrap();
    check_complete(&c).unwrap();
}

#[test]
fn signing_deadline_follows_the_reservation_and_the_job() {
    use fabstir_llm_node::ltx::provenance::finalise::signing_deadline;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let d = signing_deadline(now + 1_000, None);
    let left = d.saturating_duration_since(Instant::now()).as_secs();
    assert!((870..=880).contains(&left), "expiry less 120 s: {left}");
    let job = Instant::now() + Duration::from_secs(30);
    assert!(
        signing_deadline(now + 1_000, Some(job)) <= job,
        "never past the job deadline"
    );
    assert!(
        signing_deadline(now.saturating_sub(10), None) <= Instant::now() + Duration::from_secs(1),
        "an expired reservation leaves no time"
    );
}

#[test]
fn signer_url_must_be_https_or_exact_loopback() {
    assert!(HttpSigner::new("https://provenance.fabstir.net", &HOST_KEY).is_ok());
    assert!(HttpSigner::new("http://127.0.0.1:8601", &HOST_KEY).is_ok());
    assert!(HttpSigner::new("http://127.0.0.1.evil.example", &HOST_KEY).is_err());
    assert!(HttpSigner::new("http://provenance.fabstir.net", &HOST_KEY).is_err());
}

#[test]
fn accept_reserves_after_the_permit_and_before_the_pending_mark() {
    // a busy GPU must not burn reservations (review AP2 R1), and a failed reserve must need no unmark
    let src: String = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/api/websocket/handlers/ltx.rs"
    ))
    .unwrap()
    .chars()
    .filter(|c| !c.is_whitespace())
    .collect();
    let accept = src.find("pubasyncfnhandle_encrypted_ltx_generate").unwrap();
    let permit = accept
        + src[accept..]
            .find("ltx_semaphore().try_acquire_owned()")
            .unwrap();
    let reserve = accept + src[accept..].find("rt.signer.reserve(&req)").unwrap();
    let pending = accept + src[accept..].find("mark_proof_pending(jid").unwrap();
    assert!(permit < reserve && reserve < pending);
    let run = run_body();
    let complete = run
        .find("check_complete(&prov)")
        .expect("completeness is checked");
    assert!(complete < run.find("exr::build_manifest(").unwrap());
}

#[test]
fn deep_wire_marks_only_the_first_video_as_exr() {
    let mut j = job("ltx-layout-chain-hdr", 1, 2);
    j.input_wire = Some(fabstir_llm_node::ltx::types::InputWire::ExrseqDisplay);
    assert_eq!(
        ingredient_formats(lookup("ltx-layout-chain-hdr").unwrap(), &j).unwrap(),
        vec!["image/x-exr", "video/mp4", "image/png"]
    );
    let mut m = job("ltx-edit-chain-hdr", 0, 2);
    m.input_wire = Some(fabstir_llm_node::ltx::types::InputWire::ExrseqDisplay);
    assert_eq!(
        ingredient_formats(lookup("ltx-edit-chain-hdr").unwrap(), &m).unwrap(),
        vec!["image/x-exr", "video/mp4"]
    );
}

#[tokio::test]
async fn delivered_file_is_verified_offline() {
    use fabstir_llm_node::ltx::provenance::finalise::verify_delivered;
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-t2v-hdr", false).await;
    let uuid = r.params.manifest_uuid.clone();
    let out = finalise_mp4(
        &mut ctx(&s, &f, r, "ltx-t2v-hdr", vec![]),
        fixture("tiny.mp4"),
    )
    .await
    .unwrap();
    verify_delivered(&out, &uuid).unwrap();
    assert!(
        verify_delivered(&out, "00000000-0000-4000-8000-000000000000").is_err(),
        "another reservation's manifest"
    );
    let mut tampered = out.clone();
    let (_, mdat_off, _) = flow::top_boxes(&out)
        .into_iter()
        .find(|b| b.0 == "mdat")
        .unwrap();
    tampered[mdat_off + 100] ^= 0xff;
    assert!(
        verify_delivered(&tampered, &uuid).is_err(),
        "bytes that are not the signed ones"
    );
}

#[tokio::test]
async fn startup_refuses_a_signer_that_does_not_allow_this_node() {
    use fabstir_llm_node::ltx::provenance::Runtime;
    // the signer answers /v1/limits but does not allow-list this node's version: refused at boot, not at the
    // first job after its GPU run
    let s = Arc::new(InProc {
        ts: TestSigner::new(host_addr(), &["8.0.0-other"]),
        host: hex::encode(host_addr()),
        signs: AtomicUsize::new(0),
    });
    let Err(e) = Runtime::check(s, ffmpeg().await).await else {
        panic!("a signer that does not allow this node was accepted")
    };
    assert!(e.to_string().contains("refuses this node"), "{e}");
}

#[test]
fn finalise_verifies_before_it_returns_the_marked_file() {
    let src: String = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ltx/provenance/finalise.rs"
    ))
    .unwrap()
    .chars()
    .filter(|c| !c.is_whitespace())
    .collect();
    let patch = src
        .find("letout=flow::patch(&prep,&manifest)?;")
        .expect("finalise patches the manifest");
    let verify = patch
        + src[patch..]
            .find("verify_delivered(&o2,&u2)")
            .expect("and verifies it");
    let ret = patch + src[patch..].find("returnOk(out);").unwrap();
    assert!(verify < ret);
}

#[test]
fn startup_sweep_removes_only_stale_scratch() {
    use fabstir_llm_node::ltx::provenance::ffmpeg::sweep_scratch;
    let tmp = std::env::temp_dir();
    let tag = hex::encode(rand::random::<[u8; 6]>());
    let (old, live) = (
        tmp.join(format!("prov-fp-old-{tag}")),
        tmp.join(format!("prov-fp-live-{tag}")),
    );
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&live).unwrap();
    let two_hours_ago = std::time::SystemTime::now() - Duration::from_secs(7200);
    std::fs::File::open(&old)
        .unwrap()
        .set_modified(two_hours_ago)
        .unwrap();
    sweep_scratch();
    assert!(
        !old.exists(),
        "a dead process's scratch (decrypted output) is removed"
    );
    assert!(live.exists(), "a live fingerprint's scratch is left alone");
    let _ = std::fs::remove_dir_all(&live);
}

#[test]
fn signer_supplied_values_are_bounded() {
    use fabstir_llm_node::ltx::provenance::clamp_reserve;
    use fabstir_llm_node::ltx::provenance::finalise::signing_deadline;
    assert_eq!(clamp_reserve(usize::MAX), 65_536);
    assert_eq!(clamp_reserve(10), 9_216);
    assert_eq!(clamp_reserve(12_000), 12_000);
    // a hostile expiry must not overflow Instant (it would panic inside run())
    let d = signing_deadline(u64::MAX, None);
    assert!(d.saturating_duration_since(Instant::now()) <= Duration::from_secs(6 * 3600 + 900));
}

#[tokio::test]
async fn deep_wire_modified_job_signs_end_to_end() {
    // a deep-conform edit: the parent arrives as an EXR frame list, the previous part as an mp4
    let s = in_proc();
    let f = ffmpeg().await;
    let r = reserve(&s, "ltx-edit-chain-hdr", false).await;
    let out = finalise_mp4(
        &mut ctx(
            &s,
            &f,
            r,
            "ltx-edit-chain-hdr",
            vec!["image/x-exr", "video/mp4"],
        ),
        fixture("tiny.mp4"),
    )
    .await
    .unwrap();
    let (_, _, bad, j) = read(&out, &s.ts.anchors_pem);
    assert!(bad.is_empty(), "{bad:?}");
    let ings = active(&j)["ingredients"].as_array().unwrap();
    assert_eq!(ings[0]["format"], "image/x-exr");
    assert_eq!(ings[0]["relationship"], "parentOf");
    assert_eq!(ings[1]["relationship"], "inputTo");
}

#[tokio::test]
async fn the_startup_self_test_fingerprints_the_bundled_clip() {
    let set = ffmpeg()
        .await
        .fingerprint(fabstir_llm_node::ltx::provenance::SELF_TEST_MP4)
        .await
        .unwrap();
    assert!(set.frames.len() > 10);
}

/// Not a gate: writes the AP4 detector's fixtures (signed by a per-run staging chain + local TSA):
/// `PROV_DETECTOR_FIXTURES=<dir> cargo test --test ltx_tests export_detector_fixtures -- --ignored`
#[tokio::test]
#[ignore]
async fn export_detector_fixtures() {
    let dir = std::path::PathBuf::from(
        std::env::var("PROV_DETECTOR_FIXTURES").expect("PROV_DETECTOR_FIXTURES"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let s = in_proc();
    let f = ffmpeg().await;
    // the detector trusts the Fabstir signing root ONLY (a TSA root in the anchors would let a TSA-issued leaf pass)
    let end = "-----END CERTIFICATE-----\n";
    let root = &s.ts.anchors_pem[..s.ts.anchors_pem.find(end).unwrap() + end.len()];
    std::fs::write(dir.join("anchors.pem"), root).unwrap();
    // the TSA root on its own: the detector's test that such a root is REFUSED as a signing anchor
    std::fs::write(dir.join("tsa-root.pem"), &s.ts.anchors_pem[root.len()..]).unwrap();
    let mut registry = vec![];
    for (name, t, formats, exr) in [
        ("t2v", "ltx-t2v-hdr", vec![], false),
        ("i2v", "ltx-i2v-hdr", vec!["image/png"], false),
        ("edit", "ltx-edit-hdr", vec!["video/mp4"], false),
        ("sdr2hdr", "ltx-sdr2hdr-hdr", vec!["video/mp4"], true),
        ("passes", "cosmos-passes-key", vec![], false),
    ] {
        let r = reserve(&s, t, exr).await;
        let uuid = r.params.manifest_uuid.clone();
        let seq = r.sequence.clone();
        let mut c = ctx(&s, &f, r, t, formats);
        let out = finalise_mp4(&mut c, fixture("tiny.mp4")).await.unwrap();
        std::fs::write(dir.join(format!("{name}.mp4")), &out).unwrap();
        if exr {
            let frame = finalise_exr(&mut c, &exr_frame(5)).unwrap();
            std::fs::write(dir.join(format!("{name}-frame_000001.exr")), &frame).unwrap();
            finalise_sequence(&mut c).await.unwrap();
            let g = seq.unwrap();
            let rec = s.ts.svc.repo.get(&g.manifest_uuid).unwrap();
            std::fs::write(dir.join(format!("{name}-sequence.c2pa")), &rec.manifest).unwrap();
            std::fs::write(
                dir.join(format!("{name}-frames-index.json")),
                rec.frames_index.unwrap(),
            )
            .unwrap();
            registry.push(serde_json::json!({"kind": "sequence", "uuid": g.manifest_uuid}));
        }
        let rec = s.ts.svc.repo.get(&uuid).unwrap();
        if let Some(fp) = rec.fingerprint {
            std::fs::write(dir.join(format!("{name}.pfp")), fp).unwrap();
        }
        registry.push(serde_json::json!({"kind": "mp4", "name": name, "uuid": uuid, "class": rec.meta.class, "has_visual_input": rec.meta.has_visual_input, "time": rec.meta.time}));
    }
    std::fs::write(
        dir.join("registry.json"),
        serde_json::to_vec_pretty(&registry).unwrap(),
    )
    .unwrap();
}
