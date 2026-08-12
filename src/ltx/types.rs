// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Shared types for the LTX 2.3 generation sidecar: the job contract (A), the
//! keyless frame manifest, and the attestation (B). Wire keys are camelCase to
//! match the authoritative seam (`docs/sdk-reference/LTX-SIDECAR-M0-INTERFACE.md`).

use ethers::types::U256;
use serde::{Deserialize, Serialize};

/// Output resolution in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub w: u32,
    pub h: u32,
}

/// The single output kind supported in M0 (HDR EXR image sequence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputKind {
    ExrSequence,
    /// A2 (EXR masters, v8.42.0): true per-frame 16-bit EXR delivery — wire
    /// value `exr-frames`. The legacy `exr-sequence` keeps meaning "single
    /// H.264 artefact" for every deployed client; behaviour keys ONLY on this
    /// variant, which the 0.15.0+ helper sends when the user opts in.
    ExrFrames,
}

/// Deep-conform input wire (v8.44.0, EXECUTION-DEEP-CONFORM.md). When a job
/// carries one of these, `videos[0]` is NOT an mp4: it is a flat POSIX tar of
/// 16-bit EXR frames — the conform without 8-bit quantisation or 4:2:0 chroma
/// subsampling. The two variants differ ONLY in what the frame values mean;
/// both end display-referred at the graph (the model's training distribution):
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputWire {
    /// Frames carry DISPLAY-encoded Rec.709 values — the loader passes them
    /// through untouched ("Linear (sRGB)" is the Radiance pass-through).
    ExrseqDisplay,
    /// Frames carry Blender scene-linear values — the patcher inserts the
    /// x^(1/2.2) encode shim after the loader so the graph still sees
    /// display-encoded input. Exists because Blender's EXR export semantics
    /// are audited on first deploy, not assumed: the helper flips this
    /// constant, not code, if the audit says linear.
    ExrseqLinear,
}

/// Job contract A (M0, prompt-only). `seed` is a decimal STRING on the wire
/// (a JSON float64 corrupts values above 2^53), parsed to `U256` inside
/// `inputCommitment`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LtxJob {
    pub template_id: String,
    pub template_hash: String,
    pub prompt: String,
    pub seed: String,
    pub frames: u32,
    pub fps: u32,
    pub resolution: Resolution,
    pub lora: String,
    pub output: OutputKind,
    /// Ordered S5 capability CIDs of the input images (M1a). Absent/empty for t2v;
    /// present for image-conditioned templates (i2v/flf2v/style_transition). The
    /// commitment binds `keccak256(plaintext bytes)` of each image, NOT the CID
    /// (see `attestation::commitment_for`). `#[serde(default)]` keeps M0 wire (no
    /// `images` key) parsing to `None`; `skip_serializing_if` keeps M0 output
    /// byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
    /// Ordered S5 capability CIDs of the input videos (BL3). Absent/empty for
    /// everything before iclora; present for video-conditioned templates. Same
    /// transport and commitment rule as `images`: the commitment binds
    /// `keccak256(plaintext bytes)` of each video, NOT the CID. The serde attrs
    /// keep the pre-video wire parsing (`None`) and output (key omitted)
    /// byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub videos: Option<Vec<String>>,
    /// IC-LoRA guide-strength override, (0.0, 1.0]. `None` = the template's
    /// pinned constant (1.0 in every guided graph — maximum source adherence).
    /// Surfaced 2026-07-28 because object edits ("make the green prop gun
    /// black") cannot win against a fully-weighted guide: lowering this hands
    /// the prompt authority over the source. Patched by CLASS onto
    /// `LTXAddVideoICLoRAGuide`; a job carrying it against a template with no
    /// guide node is REJECTED rather than billed with the knob silently
    /// ignored. Serde attrs keep the pre-strength wire (`None`) parsing and
    /// output byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f64>,
    /// CrossView camera pose (CV1, v8.41.0): azimuth/elevation in degrees,
    /// distance as a scale factor. `None` = the template's pinned mild pose
    /// (azimuth 20, elevation 0, distance 1.0). Patched by CLASS onto
    /// `CrossViewWarp`; a job carrying any of them against a template with no
    /// camera node is REJECTED, same fail-closed rule as `strength`. Ranges
    /// enforced at validation: azimuth [-65, 65], elevation [-25, 40],
    /// distance [0.5, 2.0] — the trained yellow-zone envelope. Keyframed
    /// camera paths are CV2, deliberately not on this wire yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azimuth: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    /// Deep-conform wire marker (v8.44.0): `videos[0]` is an EXR tar, not an
    /// mp4, and the patcher swaps the video loader for the float sequence
    /// reader. `None` = every deployed client's behaviour, byte-identical.
    /// A job carrying it against a template outside the deep-capable set is
    /// REJECTED pre-accept (same fail-closed rule as `strength`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_wire: Option<InputWire>,
}

impl LtxJob {
    /// Parse the decimal-string `seed` into the `U256` used inside
    /// `inputCommitment`. Rejects any non-decimal value (e.g. a `0x` hex form).
    pub fn seed_u256(&self) -> Result<U256, String> {
        U256::from_dec_str(&self.seed).map_err(|e| format!("invalid seed {:?}: {e}", self.seed))
    }

    /// Clip length in whole seconds, `(frames - 1) / fps` — the value the pinned
    /// graph's `Duration` handle takes (it recomputes `Duration * fps + 1` back
    /// into the latent length). `None` when `fps == 0` or `frames == 0`, which
    /// would divide by zero / underflow `frames - 1`. Both the patcher and the
    /// handler's `validate_duration` derive the second-count through THIS one
    /// method, so billed frames and rendered length can never drift apart from
    /// two divergent copies of the formula.
    pub fn duration_secs(&self) -> Option<u32> {
        if self.fps == 0 || self.frames == 0 {
            return None;
        }
        Some((self.frames - 1) / self.fps)
    }
}

/// PUBLIC, KEYLESS frame manifest. Commits to ciphertext frame hashes and a
/// Merkle root; carries NO capability CIDs and NO keys (those ride the
/// encrypted `ltx_complete`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameManifest {
    pub frame_count: u32,
    pub fps: u32,
    pub resolution: Resolution,
    pub colour_encoding: String,
    /// Keyless: each entry is `keccak256(ciphertext bytes)` of one frame.
    pub frame_hashes: Vec<String>,
    /// keccak Merkle root over `frame_hashes`.
    pub merkle_root: String,
}

/// Proof/attestation B. Stored PLAINTEXT on S5; its CID is `proofCID` and its
/// exact bytes are SHA256-hashed for the on-chain `proofHash`. Keyless: the
/// key-bearing capability CIDs are delivered only in the encrypted
/// `ltx_complete`, never in this public object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attestation {
    pub model_id: String,
    pub template_hash: String,
    pub env_hash: String,
    pub input_commitment: String,
    /// Public, keyless manifest CID. `camelCase` would wrongly give `outputCid`.
    #[serde(rename = "outputCID")]
    pub output_cid: String,
    pub manifest: FrameManifest,
    pub session_id: String,
    pub host: String,
    pub timestamp: u64,
    /// Off-chain EIP-191 provenance over the fixed-field `sigDigest` (Phase 6),
    /// NOT over this JSON. `None` when the node has no signing key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl Attestation {
    /// The exact bytes uploaded to `proofCID` and SHA256-hashed for the on-chain
    /// `proofHash`. Deterministic: serde serialises struct fields in declaration
    /// order, so repeated calls are byte-identical.
    pub fn stored_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("attestation serialises to JSON")
    }
}
