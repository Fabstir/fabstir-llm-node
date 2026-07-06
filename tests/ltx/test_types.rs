// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 1 type serialization tests for the LTX sidecar.

use ethers::types::U256;
use fabstir_llm_node::ltx::{Attestation, FrameManifest, LtxJob, OutputKind, Resolution};
use serde_json::json;
use std::collections::BTreeSet;

fn sample_job() -> LtxJob {
    LtxJob {
        template_id: "ltx-t2v-hdr".to_string(),
        template_hash: "0x9f2c".to_string(),
        prompt: "a derelict spaceship corridor".to_string(),
        seed: "4815162342".to_string(),
        frames: 121,
        fps: 24,
        resolution: Resolution { w: 1280, h: 720 },
        lora: "ltx-iclora-hdr@v1".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
        videos: None,
    }
}

fn sample_attestation() -> Attestation {
    Attestation {
        model_id: "0x01".to_string(),
        template_hash: "0x02".to_string(),
        env_hash: "0x03".to_string(),
        input_commitment: "0x04".to_string(),
        output_cid: "u-public-manifest".to_string(),
        manifest: FrameManifest {
            frame_count: 1,
            fps: 24,
            resolution: Resolution { w: 1280, h: 720 },
            colour_encoding: "linear-HDR-from-LogC3".to_string(),
            frame_hashes: vec!["0xaaaa".to_string()],
            merkle_root: "0xaaaa".to_string(),
        },
        session_id: "0x05".to_string(),
        host: "0x06".to_string(),
        timestamp: 1_790_000_000,
        signature: Some("0xsig".to_string()),
    }
}

#[test]
fn test_job_roundtrip() {
    let job = sample_job();
    let v = serde_json::to_value(&job).unwrap();
    // camelCase wire keys present.
    assert!(v.get("templateId").is_some());
    assert!(v.get("templateHash").is_some());
    // seed is a JSON string, not a number (float64 would corrupt large seeds).
    assert!(v.get("seed").unwrap().is_string());
    assert_eq!(v["seed"], "4815162342");
    let back: LtxJob = serde_json::from_value(v).unwrap();
    assert_eq!(back, job);
}

#[test]
fn test_job_images_optional_and_roundtrips() {
    // M0 (t2v): no images -> the key is OMITTED on the wire (byte-identical output),
    // and absent-key input parses back to None.
    let t2v = sample_job();
    let v = serde_json::to_value(&t2v).unwrap();
    assert!(v.get("images").is_none(), "t2v must not emit an images key");
    let back: LtxJob = serde_json::from_value(v).unwrap();
    assert_eq!(back.images, None);

    // M1a (image template): ordered capability CIDs round-trip under the `images` key.
    let mut i2v = sample_job();
    i2v.images = Some(vec!["uCidFirst".to_string(), "uCidSecond".to_string()]);
    let v = serde_json::to_value(&i2v).unwrap();
    assert_eq!(v["images"], json!(["uCidFirst", "uCidSecond"]));
    let back: LtxJob = serde_json::from_value(v).unwrap();
    assert_eq!(back, i2v);
}

#[test]
fn test_output_kind_enum() {
    assert_eq!(
        serde_json::to_value(OutputKind::ExrSequence).unwrap(),
        json!("exr-sequence")
    );
    let back: OutputKind = serde_json::from_value(json!("exr-sequence")).unwrap();
    assert_eq!(back, OutputKind::ExrSequence);
}

#[test]
fn test_seed_u256_parse() {
    assert_eq!(
        sample_job().seed_u256().unwrap(),
        U256::from(4_815_162_342u64)
    );
    let mut hex = sample_job();
    hex.seed = "0xdead".to_string();
    assert!(hex.seed_u256().is_err());
    let mut words = sample_job();
    words.seed = "not-a-number".to_string();
    assert!(words.seed_u256().is_err());
}

#[test]
fn test_manifest_is_keyless() {
    let m = sample_attestation().manifest;
    let v = serde_json::to_value(&m).unwrap();
    assert!(v.get("frameHashes").is_some());
    assert!(v.get("merkleRoot").is_some());
    // The manifest carries exactly the keyless fields — no capability CID / key field.
    let keys: BTreeSet<String> = v.as_object().unwrap().keys().cloned().collect();
    let expected: BTreeSet<String> = [
        "frameCount",
        "fps",
        "resolution",
        "colourEncoding",
        "frameHashes",
        "merkleRoot",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(keys, expected);
}

#[test]
fn test_stored_bytes_deterministic() {
    let att = sample_attestation();
    assert_eq!(att.stored_bytes(), att.stored_bytes());
    // The stored bytes are what proofCID holds and proofHash hashes; outputCID
    // must serialise with the exact (non-camelCase) key.
    let s = String::from_utf8(att.stored_bytes()).unwrap();
    assert!(s.contains("outputCID"));
    assert!(s.contains("signature"));
}

#[test]
fn test_job_videos_optional_and_roundtrips() {
    // Pre-video wire: no videos -> the key is OMITTED (byte-identical output),
    // absent-key input parses back to None.
    let t2v = sample_job();
    let v = serde_json::to_value(&t2v).unwrap();
    assert!(v.get("videos").is_none(), "t2v must not emit a videos key");
    let back: LtxJob = serde_json::from_value(v).unwrap();
    assert_eq!(back.videos, None);

    // BL3 (video template): ordered capability CIDs round-trip under `videos`,
    // alongside the reference image.
    let mut iclora = sample_job();
    iclora.images = Some(vec!["uCidReference".to_string()]);
    iclora.videos = Some(vec!["uCidControl".to_string()]);
    let v = serde_json::to_value(&iclora).unwrap();
    assert_eq!(v["videos"], json!(["uCidControl"]));
    let back: LtxJob = serde_json::from_value(v).unwrap();
    assert_eq!(back, iclora);
}
