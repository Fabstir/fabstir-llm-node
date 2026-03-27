// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for format spec hashing and modelId generation.

use fabstir_llm_node::transcoder::job_validation::{
    canonical_format_spec, compute_model_id_from_preset, compute_transcode_model_id,
    resolve_preset_name,
};
use fabstir_llm_node::transcoder::types::VideoFormat;

fn make_format(id: u32, ext: &str, vcodec: &str) -> VideoFormat {
    VideoFormat {
        id,
        ext: ext.into(),
        label: None,
        type_: None,
        vcodec: Some(vcodec.into()),
        acodec: None,
        preset: None,
        profile: None,
        ch: None,
        vf: None,
        b_v: None,
        ar: None,
        b_a: None,
        c_a: None,
        minrate: None,
        maxrate: None,
        bufsize: None,
        gpu: None,
        compression_level: None,
        dest: None,
        encrypt: None,
        trim_percent: None,
    }
}

#[test]
fn test_preset_model_id_av1_matches_contract() {
    let h1080 = compute_model_id_from_preset("1080p-av1-nvenc");
    let h2160 = compute_model_id_from_preset("2160p-av1-nvenc");
    let h_both = compute_model_id_from_preset("1080p-2160p-av1-nvenc");

    assert_eq!(
        hex::encode(h1080),
        "7b24ba0224d33d514e35824ab5d6af9b5a8852a30bda778e8e428c4321225cdf"
    );
    assert_eq!(
        hex::encode(h2160),
        "5317a5aaba6515d7d91d76361e8a04e14df09b6c1cd923c35123c302d44a4413"
    );
    assert_eq!(
        hex::encode(h_both),
        "48cea869839997a488025e47daa6bbf10d4b373848f798fc5853d0065d4206e6"
    );
}

#[test]
fn test_preset_model_id_h264_matches_contract() {
    let h1080 = compute_model_id_from_preset("1080p-h264-nvenc");
    let h2160 = compute_model_id_from_preset("2160p-h264-nvenc");
    let h_both = compute_model_id_from_preset("1080p-2160p-h264-nvenc");

    assert_eq!(
        hex::encode(h1080),
        "7d10071fcd64f23760ce7c9a7dba99369166f01a5ca7703c4a32828542d3f293"
    );
    assert_eq!(
        hex::encode(h2160),
        "a2d8049f79bd310686dd2f1dface405d2ea099e36c5fda75976cfe816f33722e"
    );
    assert_eq!(
        hex::encode(h_both),
        "8c8ff0cb8beeb997bb979c31e21f6ebdb5073121d4d446679697cc21785f476a"
    );
}

#[test]
fn test_resolve_preset_name_av1() {
    let fmt33 = make_format(33, "mp4", "av1_nvenc");
    let fmt34 = make_format(34, "mp4", "av1_nvenc");

    assert_eq!(
        resolve_preset_name(&[fmt33.clone()]),
        Some("1080p-av1-nvenc")
    );
    assert_eq!(
        resolve_preset_name(&[fmt34.clone()]),
        Some("2160p-av1-nvenc")
    );
    assert_eq!(
        resolve_preset_name(&[fmt33, fmt34]),
        Some("1080p-2160p-av1-nvenc")
    );
}

#[test]
fn test_resolve_preset_name_h264() {
    let fmt1 = make_format(1, "mp4", "h264_nvenc");
    let fmt2 = make_format(2, "mp4", "h264_nvenc");

    assert_eq!(
        resolve_preset_name(&[fmt1.clone()]),
        Some("1080p-h264-nvenc")
    );
    assert_eq!(
        resolve_preset_name(&[fmt2.clone()]),
        Some("2160p-h264-nvenc")
    );
    assert_eq!(
        resolve_preset_name(&[fmt1, fmt2]),
        Some("1080p-2160p-h264-nvenc")
    );
}

#[test]
fn test_resolve_preset_name_order_independent() {
    let fmt33 = make_format(33, "mp4", "av1_nvenc");
    let fmt34 = make_format(34, "mp4", "av1_nvenc");
    assert_eq!(
        resolve_preset_name(&[fmt34, fmt33]),
        Some("1080p-2160p-av1-nvenc")
    );
}

#[test]
fn test_resolve_preset_name_unknown() {
    let unknown = make_format(99, "mp4", "libvpx");
    assert_eq!(resolve_preset_name(&[unknown]), None);
}

#[test]
fn test_compute_transcode_model_id_uses_preset() {
    let fmt1 = make_format(1, "mp4", "h264_nvenc");
    let id = compute_transcode_model_id(&[fmt1]);
    let expected = compute_model_id_from_preset("1080p-h264-nvenc");
    assert_eq!(id, expected);
}

#[test]
fn test_compute_transcode_model_id_falls_back() {
    let unknown = make_format(99, "mp4", "libvpx");
    let id = compute_transcode_model_id(&[unknown.clone()]);
    let spec = canonical_format_spec(&[unknown]);
    let mut hasher = tiny_keccak::Keccak::v256();
    let mut expected = [0u8; 32];
    tiny_keccak::Hasher::update(&mut hasher, spec.as_bytes());
    tiny_keccak::Hasher::finalize(hasher, &mut expected);
    assert_eq!(id, expected);
}
