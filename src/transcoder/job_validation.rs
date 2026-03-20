// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Job validation — format spec hashing and modelId generation for contract reuse.
//!
//! Model IDs use the same `keccak256(abi.encodePacked(repo, "/", fileName))` convention
//! as LLM models in the ModelRegistry contract. Transcoding presets are registered under
//! the `fabstir/transcoding` repo with descriptive file names (e.g. `1080p-av1-nvenc`).

use tiny_keccak::{Hasher, Keccak};

use super::types::VideoFormat;

/// Transcoding model repo name (matches ModelRegistry registration).
const TRANSCODE_REPO: &str = "fabstir/transcoding";

/// Known transcoding preset mappings: (resolution label, codec family, fileName).
/// These must match what is registered in ModelRegistry via `addTrustedModel`.
/// The key is (sorted format IDs, primary codec) → fileName.
struct PresetEntry {
    format_ids: &'static [u32],
    codec_prefix: &'static str,
    file_name: &'static str,
}

const KNOWN_PRESETS: &[PresetEntry] = &[
    // AV1 presets
    PresetEntry {
        format_ids: &[33],
        codec_prefix: "av1",
        file_name: "1080p-av1-nvenc",
    },
    PresetEntry {
        format_ids: &[34],
        codec_prefix: "av1",
        file_name: "2160p-av1-nvenc",
    },
    PresetEntry {
        format_ids: &[33, 34],
        codec_prefix: "av1",
        file_name: "1080p-2160p-av1-nvenc",
    },
    // H.264 presets
    PresetEntry {
        format_ids: &[1],
        codec_prefix: "h264",
        file_name: "1080p-h264-nvenc",
    },
    PresetEntry {
        format_ids: &[2],
        codec_prefix: "h264",
        file_name: "2160p-h264-nvenc",
    },
    PresetEntry {
        format_ids: &[1, 2],
        codec_prefix: "h264",
        file_name: "1080p-2160p-h264-nvenc",
    },
];

fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut out = [0u8; 32];
    hasher.update(data);
    hasher.finalize(&mut out);
    out
}

/// Compute modelId using the contract convention: `keccak256(abi.encodePacked(repo, "/", fileName))`.
/// This matches `ModelRegistry.getModelId("fabstir/transcoding", fileName)`.
pub fn compute_model_id_from_preset(file_name: &str) -> [u8; 32] {
    let packed = format!("{}/{}", TRANSCODE_REPO, file_name);
    keccak256(packed.as_bytes())
}

/// Look up the preset fileName from a set of VideoFormats by matching sorted format IDs
/// and primary codec. Returns `None` if the format combination is not a known preset.
pub fn resolve_preset_name(formats: &[VideoFormat]) -> Option<&'static str> {
    let mut ids: Vec<u32> = formats.iter().map(|f| f.id).collect();
    ids.sort();

    // Determine primary codec family from first format's vcodec
    let codec = formats
        .first()
        .and_then(|f| f.vcodec.as_deref())
        .unwrap_or("");
    let codec_prefix = if codec.starts_with("av1") {
        "av1"
    } else if codec.starts_with("h264") || codec.starts_with("libx264") {
        "h264"
    } else if codec.starts_with("hevc") || codec.starts_with("h265") || codec.starts_with("libx265")
    {
        "hevc"
    } else {
        ""
    };

    KNOWN_PRESETS
        .iter()
        .find(|entry| {
            let mut sorted_preset: Vec<u32> = entry.format_ids.to_vec();
            sorted_preset.sort();
            sorted_preset == ids && entry.codec_prefix == codec_prefix
        })
        .map(|entry| entry.file_name)
}

/// Compute the transcoding `modelId` for contract interaction.
///
/// First tries to match against known presets (repo/fileName convention).
/// Falls back to `keccak256(canonicalFormatSpecJSON)` for unregistered combinations.
pub fn compute_transcode_model_id(formats: &[VideoFormat]) -> [u8; 32] {
    if let Some(preset_name) = resolve_preset_name(formats) {
        return compute_model_id_from_preset(preset_name);
    }
    // Fallback: hash the canonical format spec (won't match on-chain for unregistered presets)
    let spec = canonical_format_spec(formats);
    keccak256(spec.as_bytes())
}

/// Produce a canonical JSON string from formats (sorted by `id`).
pub fn canonical_format_spec(formats: &[VideoFormat]) -> String {
    let mut sorted: Vec<&VideoFormat> = formats.iter().collect();
    sorted.sort_by_key(|f| f.id);
    serde_json::to_string(&sorted).unwrap_or_default()
}
