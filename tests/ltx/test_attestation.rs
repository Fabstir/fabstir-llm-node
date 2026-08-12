// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 6 attestation tests. Fixed-field `inputCommitment`/`sigDigest`, SHA256
//! `proofHash`, EIP-191 recovery, and the `vectors.json` conformance fixture.

use ethers::abi::{encode, Token};
use ethers::types::U256;
use ethers::utils::keccak256;

use fabstir_llm_node::crypto::proof_signer::{eip191_prehash, sign_eip191_digest};
use fabstir_llm_node::crypto::recover_client_address;
use fabstir_llm_node::ltx::attestation::{
    assemble, commitment_for, env_hash, input_commitment, input_commitment_v2, input_commitment_v3,
    output_commitment, proof_hash, sig_digest, EnvMeta,
};
use fabstir_llm_node::ltx::submit::{ltx_tokens, submit_calldata};
use fabstir_llm_node::ltx::types::{Attestation, FrameManifest, LtxJob, OutputKind, Resolution};
use fabstir_llm_node::transcoder::merkle::MerkleTree;

// Anvil/Hardhat account #0 — a public throwaway used across the repo's tests;
// leaks nothing and makes the fixture reproducible.
const ANVIL0_KEY: &str = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const ANVIL0_ADDR: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

fn key_bytes() -> [u8; 32] {
    let mut k = [0u8; 32];
    k.copy_from_slice(&hex::decode(ANVIL0_KEY).unwrap());
    k
}

fn b32(byte: u8) -> String {
    format!("0x{}", hex::encode([byte; 32]))
}

fn sample_job() -> LtxJob {
    LtxJob {
        template_id: "ltx-t2v-hdr".to_string(),
        template_hash: b32(0x02),
        prompt: "interior of a derelict spaceship corridor".to_string(),
        seed: "4815162342".to_string(),
        frames: 121,
        fps: 24,
        resolution: Resolution { w: 1280, h: 720 },
        lora: "ltx-iclora-hdr@v1".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
        videos: None,
        strength: None,
        azimuth: None,
        elevation: None,
        distance: None,
        input_wire: None,
    }
}

fn sample_meta() -> EnvMeta {
    EnvMeta {
        weights_hash: "0xweights".to_string(),
        lora_hash: "0xlora".to_string(),
        comfy_commit: "comfy@abc123".to_string(),
        node_commit: "node@def456".to_string(),
        cuda_version: "12.4".to_string(),
        gpu_class: "L40S".to_string(),
    }
}

fn frame_hashes() -> Vec<String> {
    vec![b32(0xaa), b32(0xbb), b32(0xcc)]
}

fn merkle_root(hashes: &[String]) -> String {
    let mut tree = MerkleTree::new();
    for h in hashes {
        let mut leaf = [0u8; 32];
        leaf.copy_from_slice(&hex::decode(h.strip_prefix("0x").unwrap()).unwrap());
        tree.add_leaf(leaf);
    }
    format!("0x{}", hex::encode(tree.root()))
}

fn sample_manifest() -> FrameManifest {
    let fh = frame_hashes();
    FrameManifest {
        frame_count: fh.len() as u32,
        fps: 24,
        resolution: Resolution { w: 1280, h: 720 },
        colour_encoding: "linear-HDR-from-LogC3".to_string(),
        merkle_root: merkle_root(&fh),
        frame_hashes: fh,
    }
}

const OUTPUT_CID: &str = "uManifestCidPlaceholder";

fn sample_attestation(signed: bool) -> Attestation {
    let node_key = signed.then(key_bytes);
    assemble(
        b32(0x01),
        b32(0x02),
        env_hash(&sample_meta()),
        &sample_job(),
        &[],
        &[],
        OUTPUT_CID.to_string(),
        sample_manifest(),
        "0x05".to_string(),
        ANVIL0_ADDR.to_string(),
        1_790_000_000,
        node_key,
    )
    .unwrap()
}

#[test]
fn test_assemble_empty_images_byte_identical() {
    // t2v path: no images ⇒ assemble's commitment IS the M0 seven-field form.
    let att = sample_attestation(false);
    assert_eq!(
        att.input_commitment,
        input_commitment(&sample_job()).unwrap(),
        "empty image_hashes ⇒ byte-identical M0 commitment"
    );
}

#[test]
fn test_assemble_i2v_uses_v2() {
    // i2v path: assemble binds the v2 (eight-field) commitment, not the M0 form.
    let job = sample_i2v_job();
    let h0 = img_hash(&img_plain(0));
    let att = assemble(
        b32(0x01),
        b32(0x02),
        env_hash(&sample_meta()),
        &job,
        &[h0],
        &[],
        OUTPUT_CID.to_string(),
        sample_manifest(),
        "0x05".to_string(),
        ANVIL0_ADDR.to_string(),
        1_790_000_000,
        None,
    )
    .unwrap();
    assert_eq!(
        att.input_commitment,
        input_commitment_v2(&job, &[h0]).unwrap()
    );
    assert_ne!(
        att.input_commitment,
        input_commitment(&job).unwrap(),
        "i2v must not fall back to the seven-field form"
    );
}

#[test]
fn test_input_commitment_fixed_field() {
    let job = sample_job();
    let got = input_commitment(&job).unwrap();
    let pre = encode(&[
        Token::String(job.prompt.clone()),
        Token::Uint(U256::from_dec_str("4815162342").unwrap()),
        Token::Uint(U256::from(121u32)),
        Token::Uint(U256::from(24u32)),
        Token::Uint(U256::from(1280u32)),
        Token::Uint(U256::from(720u32)),
        Token::String(job.lora.clone()),
    ]);
    assert_eq!(got, format!("0x{}", hex::encode(keccak256(pre))));
}

#[test]
fn test_sig_digest_fixed_field() {
    let att = sample_attestation(false);
    let got = sig_digest(&att).unwrap();
    // outputCommitment hashes the CID string bytes (incl. multibase prefix).
    let oc = keccak256(OUTPUT_CID.as_bytes());
    assert_eq!(oc, output_commitment(OUTPUT_CID));
    let pre = encode(&[
        Token::FixedBytes(hex::decode(&att.model_id[2..]).unwrap()),
        Token::FixedBytes(hex::decode(&att.template_hash[2..]).unwrap()),
        Token::FixedBytes(hex::decode(&att.env_hash[2..]).unwrap()),
        Token::FixedBytes(hex::decode(&att.input_commitment[2..]).unwrap()),
        Token::FixedBytes(oc.to_vec()),
        Token::Uint(U256::from(5u64)), // sessionId 0x05
        Token::Address(att.host.parse().unwrap()),
        Token::Uint(U256::from(1_790_000_000u64)),
    ]);
    assert_eq!(got, keccak256(pre));
}

#[test]
fn test_proof_hash_is_sha256_of_stored() {
    use sha2::{Digest, Sha256};
    let att = sample_attestation(true);
    let expect: [u8; 32] = Sha256::digest(att.stored_bytes()).into();
    assert_eq!(proof_hash(&att), expect);
}

#[test]
fn test_signature_eip191_recovers_host() {
    let signed = sample_attestation(true);
    assert!(signed.signature.is_some());
    let digest = sig_digest(&signed).unwrap();
    let sig = hex::decode(
        signed
            .signature
            .as_ref()
            .unwrap()
            .strip_prefix("0x")
            .unwrap(),
    )
    .unwrap();
    let recovered = recover_client_address(&sig, &eip191_prehash(&digest)).unwrap();
    assert_eq!(recovered.to_lowercase(), ANVIL0_ADDR.to_lowercase());
    // No node key -> unsigned, but submission (proofHash) still works.
    let unsigned = sample_attestation(false);
    assert!(unsigned.signature.is_none());
    assert_eq!(proof_hash(&unsigned).len(), 32);
}

#[test]
fn test_env_hash_covers_all_fields() {
    let base = env_hash(&sample_meta());
    let mutators: [fn(&mut EnvMeta); 6] = [
        |m| m.weights_hash.push('!'),
        |m| m.lora_hash.push('!'),
        |m| m.comfy_commit.push('!'),
        |m| m.node_commit.push('!'),
        |m| m.cuda_version.push('!'),
        |m| m.gpu_class.push('!'),
    ];
    for mutate in mutators {
        let mut m = sample_meta();
        mutate(&mut m);
        assert_ne!(env_hash(&m), base);
    }
}

#[test]
fn test_ltx_tokens_megapixel_frame() {
    assert_eq!(ltx_tokens(121, 1280, 720), 111_514);
    assert!(ltx_tokens(1, 768, 512) >= 100); // smallest allowed clip clears the floor
}

#[test]
fn test_submit_calldata_selector() {
    let data = submit_calldata(42, 111_514, [7u8; 32], "uProofCid".to_string());
    let selector = &keccak256(b"submitProofOfWork(uint256,uint256,bytes32,string,string)")[..4];
    assert_eq!(&data[..4], selector);
}

/// Emit `tests/ltx/vectors.json` from the SAME code paths, so the SDK
/// conformance-checks one fixture set (sub-phase 6.3). Both `abiEncoded` and
/// `hash` are emitted so a divergence localises to encoder vs hasher.
#[test]
fn emit_vectors_json() {
    let job = sample_job();
    let att = sample_attestation(true);
    let ic_pre = encode(&[
        Token::String(job.prompt.clone()),
        Token::Uint(U256::from_dec_str(&job.seed).unwrap()),
        Token::Uint(U256::from(job.frames)),
        Token::Uint(U256::from(job.fps)),
        Token::Uint(U256::from(job.resolution.w)),
        Token::Uint(U256::from(job.resolution.h)),
        Token::String(job.lora.clone()),
    ]);
    let digest = sig_digest(&att).unwrap();
    // sigDigest pre-image, so a divergence localises to encoder vs hasher (like inputCommitment).
    let oc = output_commitment(OUTPUT_CID);
    let sd_pre = encode(&[
        Token::FixedBytes(hex::decode(&att.model_id[2..]).unwrap()),
        Token::FixedBytes(hex::decode(&att.template_hash[2..]).unwrap()),
        Token::FixedBytes(hex::decode(&att.env_hash[2..]).unwrap()),
        Token::FixedBytes(hex::decode(&att.input_commitment[2..]).unwrap()),
        Token::FixedBytes(oc.to_vec()),
        Token::Uint(U256::from(5u64)), // sessionId 0x05
        Token::Address(att.host.parse().unwrap()),
        Token::Uint(U256::from(1_790_000_000u64)),
    ]);
    assert_eq!(
        keccak256(&sd_pre),
        digest,
        "sigDigest abiEncoded is its pre-image"
    );
    let ph = proof_hash(&att);
    let vectors = serde_json::json!({
        "_note": "Generated by tests/ltx/test_attestation.rs::emit_vectors_json. Do not hand-edit.",
        "job": job,
        "inputCommitment": {
            "abiEncoded": format!("0x{}", hex::encode(&ic_pre)),
            "hash": att.input_commitment,
        },
        "tokens": { "frames": 121, "w": 1280, "h": 720, "value": ltx_tokens(121, 1280, 720) },
        "merkle": { "frameHashes": frame_hashes(), "root": merkle_root(&frame_hashes()) },
        "outputCID": OUTPUT_CID,
        "outputCommitment": format!("0x{}", hex::encode(output_commitment(OUTPUT_CID))),
        "attestation": att,
        "sigDigest": { "abiEncoded": format!("0x{}", hex::encode(&sd_pre)), "hash": format!("0x{}", hex::encode(digest)) },
        "signature": att.signature,
        "signer": ANVIL0_ADDR,
        // The EXACT proofHash pre-image bytes. The `attestation` block above is
        // pretty/alphabetised for readability and is NOT the pre-image; the SDK
        // must hash THIS (or reproduce the canonical rule below), never re-serialise
        // the displayed object.
        "proofHashInput": format!("0x{}", hex::encode(att.stored_bytes())),
        "proofHashCanonical": "SHA256 of compact JSON, keys in struct-declaration order: modelId,templateHash,envHash,inputCommitment,outputCID,manifest{frameCount,fps,resolution{w,h},colourEncoding,frameHashes,merkleRoot},sessionId,host,timestamp,signature; signature omitted when null.",
        "proofHash": format!("0x{}", hex::encode(ph)),
    });
    // The pre-image must hash to proofHash (guards the canonical-bytes contract).
    {
        use sha2::{Digest, Sha256};
        let recomputed: [u8; 32] = Sha256::digest(att.stored_bytes()).into();
        assert_eq!(
            format!("0x{}", hex::encode(recomputed)),
            vectors["proofHash"]
        );
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/vectors.json");
    std::fs::write(path, serde_json::to_vec_pretty(&vectors).unwrap()).unwrap();
    assert!(std::path::Path::new(path).exists());
}

// ---------------------------------------------------------------------------
// M1a image-to-video: inputCommitment v2 conformance + `vectors-i2v.json`.
// ---------------------------------------------------------------------------

/// Deterministic stand-in plaintext for input image `i`. Self-contained so the
/// SDK can recompute `keccak256` without decrypting an 819KB capability blob;
/// the same machinery points at real decrypted bytes in production.
fn img_plain(i: u8) -> Vec<u8> {
    format!("fabstir-ltx-i2v-vector::image-{i}").into_bytes()
}

fn img_hash(plain: &[u8]) -> [u8; 32] {
    keccak256(plain)
}

fn hx(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// The v2 pre-image (seven M0 fields + trailing `bytes32[] imageHashes`), so a
/// divergence localises to encoder vs hasher exactly as the M0 vector does.
fn v2_preimage(job: &LtxJob, hashes: &[[u8; 32]]) -> Vec<u8> {
    encode(&[
        Token::String(job.prompt.clone()),
        Token::Uint(U256::from_dec_str(&job.seed).unwrap()),
        Token::Uint(U256::from(job.frames)),
        Token::Uint(U256::from(job.fps)),
        Token::Uint(U256::from(job.resolution.w)),
        Token::Uint(U256::from(job.resolution.h)),
        Token::String(job.lora.clone()),
        Token::Array(
            hashes
                .iter()
                .map(|h| Token::FixedBytes(h.to_vec()))
                .collect(),
        ),
    ])
}

/// i2v (one LoadImage). 720p / fps 25 / 126 frames = the i2v template's own
/// 5s·fps+1 default; seed exceeds 2^32 to exercise the wide range.
fn sample_i2v_job() -> LtxJob {
    LtxJob {
        template_id: "ltx-i2v-hdr".to_string(),
        template_hash: b32(0x03),
        prompt: "egyptian royal walking forward through desert, robot soldiers".to_string(),
        seed: "60540193790228".to_string(),
        frames: 126,
        fps: 25,
        resolution: Resolution { w: 1280, h: 720 },
        lora: "ltx-iclora-hdr@v1".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
        videos: None,
        strength: None,
        azimuth: None,
        elevation: None,
        distance: None,
        input_wire: None,
    }
}

/// flf2v / style_transition (two LoadImage). The two-image case is the
/// highest-value vector: `bytes32[]` ordering is the one place the ABI encoders
/// could disagree.
fn sample_flf2v_job() -> LtxJob {
    LtxJob {
        template_id: "ltx-flf2v-hdr".to_string(),
        template_hash: b32(0x04),
        prompt: "morph smoothly from the first still to the last".to_string(),
        seed: "42".to_string(),
        frames: 121,
        fps: 24,
        resolution: Resolution { w: 768, h: 512 },
        lora: "ltx-iclora-hdr@v1".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
        videos: None,
        strength: None,
        azimuth: None,
        elevation: None,
        distance: None,
        input_wire: None,
    }
}

#[test]
fn test_input_commitment_v2_appends_image_hashes() {
    let job = sample_i2v_job();
    let h0 = img_hash(&img_plain(0));
    let got = input_commitment_v2(&job, &[h0]).unwrap();
    assert_eq!(got, hx(&keccak256(v2_preimage(&job, &[h0]))));
}

#[test]
fn test_commitment_v2_empty_differs_from_v1() {
    // The trap: an eight-field encode with an EMPTY array is not the M0
    // seven-field encode (appending a dynamic field shifts prompt/lora offsets).
    let job = sample_i2v_job();
    assert_ne!(
        input_commitment_v2(&job, &[]).unwrap(),
        input_commitment(&job).unwrap(),
        "empty-array v2 must differ from v1 — t2v must keep the seven-field path"
    );
}

#[test]
fn test_commitment_v2_order_sensitive() {
    let job = sample_flf2v_job();
    let a = img_hash(&img_plain(0));
    let b = img_hash(&img_plain(1));
    assert_ne!(
        input_commitment_v2(&job, &[a, b]).unwrap(),
        input_commitment_v2(&job, &[b, a]).unwrap(),
        "swapping images[0]/[1] must change the commitment"
    );
}

#[test]
fn test_commitment_for_dispatches_on_image_count() {
    let job = sample_i2v_job();
    // zero images -> byte-identical M0 seven-field
    assert_eq!(
        commitment_for(&job, &[], &[]).unwrap(),
        input_commitment(&job).unwrap()
    );
    // one image -> v2 eight-field
    let h0 = img_hash(&img_plain(0));
    assert_eq!(
        commitment_for(&job, &[h0], &[]).unwrap(),
        input_commitment_v2(&job, &[h0]).unwrap()
    );
}

/// Emit `tests/ltx/vectors-i2v.json` from the SAME code paths (mirror of
/// `emit_vectors_json`): single-image, two-image ordering, and the
/// format-selection guard, so the SDK conformance-checks one fixture set.
#[test]
fn emit_i2v_vectors_json() {
    // -- single image (i2v) --
    let s_job = sample_i2v_job();
    let s_plain = img_plain(0);
    let s_hash = img_hash(&s_plain);
    let s_pre = v2_preimage(&s_job, &[s_hash]);
    let s_commit = input_commitment_v2(&s_job, &[s_hash]).unwrap();
    assert_eq!(s_commit, hx(&keccak256(&s_pre)));

    // -- two images (flf2v / style_transition) --
    let d_job = sample_flf2v_job();
    let d_plain0 = img_plain(0);
    let d_plain1 = img_plain(1);
    let d_h0 = img_hash(&d_plain0);
    let d_h1 = img_hash(&d_plain1);
    let d_pre = v2_preimage(&d_job, &[d_h0, d_h1]);
    let d_commit = input_commitment_v2(&d_job, &[d_h0, d_h1]).unwrap();
    let d_swapped = input_commitment_v2(&d_job, &[d_h1, d_h0]).unwrap();
    assert_ne!(d_commit, d_swapped);

    // -- format guard: v2-empty is NOT the M0 seven-field encoding --
    let g_v1 = input_commitment(&s_job).unwrap();
    let g_v2_empty = input_commitment_v2(&s_job, &[]).unwrap();
    assert_ne!(g_v1, g_v2_empty);

    let vectors = serde_json::json!({
        "_note": "Generated by tests/ltx/test_attestation.rs::emit_i2v_vectors_json. Do not hand-edit.",
        "_scheme": "inputCommitment v2 = keccak256(abi.encode(string prompt, uint256 seed, uint32 frames, uint32 fps, uint32 w, uint32 h, string lora, bytes32[] imageHashes)). imageHashes[i]=keccak256(plaintext bytes of images[i]). Format is selected by the template's bundle entry imageInputs: 0 -> M0 seven-field (see vectors.json), >0 -> this eight-field form. The capability CID is transport only and is NOT hashed into the commitment.",
        "singleImage": {
            "imageInputs": 1,
            "job": s_job,
            "images": ["uCapabilityCidPlaceholder0"],
            "imagePlaintext": [
                { "utf8": String::from_utf8(s_plain.clone()).unwrap(), "hex": hx(&s_plain) }
            ],
            "imageHashes": [hx(&s_hash)],
            "inputCommitment": { "abiEncoded": hx(&s_pre), "hash": s_commit },
        },
        "dualImage": {
            "imageInputs": 2,
            "imageSemantics": ["firstFrame", "lastFrame"],
            "job": d_job,
            "images": ["uCapabilityCidPlaceholder0", "uCapabilityCidPlaceholder1"],
            "imagePlaintext": [
                { "utf8": String::from_utf8(d_plain0.clone()).unwrap(), "hex": hx(&d_plain0) },
                { "utf8": String::from_utf8(d_plain1.clone()).unwrap(), "hex": hx(&d_plain1) }
            ],
            "imageHashes": [hx(&d_h0), hx(&d_h1)],
            "inputCommitment": { "abiEncoded": hx(&d_pre), "hash": d_commit },
            "orderMatters": {
                "swappedImageHashes": [hx(&d_h1), hx(&d_h0)],
                "swappedHash": d_swapped,
                "note": "images[0]/images[1] are order-significant. The node pins images[i] to a fixed LoadImage slot per imageSemantics; a swap here binds the wrong image and changes the commitment."
            },
        },
        "formatGuard": {
            "note": "v2 with an EMPTY imageHashes array is NOT byte-equal to the M0 seven-field commitment (a trailing dynamic field shifts the prompt/lora offsets). t2v (imageInputs 0) MUST use the seven-field path; only imageInputs>0 uses v2.",
            "v1SevenField": g_v1,
            "v2EmptyArray": g_v2_empty,
            "equal": false
        }
    });

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/vectors-i2v.json");
    std::fs::write(path, serde_json::to_vec_pretty(&vectors).unwrap()).unwrap();
    assert!(std::path::Path::new(path).exists());
}

// ---------------------------------------------------------------------------
// BL3 video-conditioned: inputCommitment v3 conformance + `vectors-iclora.json`.
// ---------------------------------------------------------------------------

/// Deterministic stand-in plaintext for input video `i` (same convention as
/// [`img_plain`]; production points this machinery at real decrypted bytes).
fn vid_plain(i: u8) -> Vec<u8> {
    format!("fabstir-ltx-iclora-vector::video-{i}").into_bytes()
}

/// The v3 pre-image: the v2 eight fields + trailing `bytes32[] videoHashes`.
fn v3_preimage(job: &LtxJob, image_hashes: &[[u8; 32]], video_hashes: &[[u8; 32]]) -> Vec<u8> {
    let arr = |hashes: &[[u8; 32]]| {
        Token::Array(
            hashes
                .iter()
                .map(|h| Token::FixedBytes(h.to_vec()))
                .collect(),
        )
    };
    encode(&[
        Token::String(job.prompt.clone()),
        Token::Uint(U256::from_dec_str(&job.seed).unwrap()),
        Token::Uint(U256::from(job.frames)),
        Token::Uint(U256::from(job.fps)),
        Token::Uint(U256::from(job.resolution.w)),
        Token::Uint(U256::from(job.resolution.h)),
        Token::String(job.lora.clone()),
        arr(image_hashes),
        arr(video_hashes),
    ])
}

/// iclora (one LoadImage reference + one LoadVideo control). 768x512 / fps 25 /
/// 126 frames = the BL3 money shape (5s @ 25fps).
fn sample_iclora_job() -> LtxJob {
    LtxJob {
        template_id: "ltx-iclora-hdr".to_string(),
        template_hash: b32(0x05),
        prompt: "restyle the control clip as a hand-painted cartoon child, jaunty whistling"
            .to_string(),
        seed: "60540193790228".to_string(),
        frames: 126,
        fps: 25,
        resolution: Resolution { w: 768, h: 512 },
        lora: "ltx-iclora-hdr@v1".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
        videos: None,
        strength: None,
        azimuth: None,
        elevation: None,
        distance: None,
        input_wire: None,
    }
}

#[test]
fn test_input_commitment_v3_appends_video_hashes() {
    let job = sample_iclora_job();
    let ih = img_hash(&img_plain(0));
    let vh = img_hash(&vid_plain(0));
    let got = input_commitment_v3(&job, &[ih], &[vh]).unwrap();
    assert_eq!(got, hx(&keccak256(v3_preimage(&job, &[ih], &[vh]))));
}

#[test]
fn test_commitment_v3_empty_videos_differs_from_v2() {
    // The v2/v3 trap, same as v1/v2: a ten-field encode with an EMPTY videoHashes
    // is not the v2 eight-field encode. Image templates must keep the v2 path.
    let job = sample_iclora_job();
    let ih = img_hash(&img_plain(0));
    assert_ne!(
        input_commitment_v3(&job, &[ih], &[]).unwrap(),
        input_commitment_v2(&job, &[ih]).unwrap(),
        "empty-videos v3 must differ from v2 — image templates keep the v2 path"
    );
}

#[test]
fn test_commitment_v3_arrays_not_interchangeable() {
    // imageHashes and videoHashes are distinct slots: moving a hash from one
    // array to the other must change the commitment.
    let job = sample_iclora_job();
    let a = img_hash(&img_plain(0));
    let b = img_hash(&vid_plain(0));
    assert_ne!(
        input_commitment_v3(&job, &[a], &[b]).unwrap(),
        input_commitment_v3(&job, &[b], &[a]).unwrap(),
        "swapping image/video arrays must change the commitment"
    );
}

#[test]
fn test_commitment_for_dispatches_on_video_count() {
    let job = sample_iclora_job();
    let ih = img_hash(&img_plain(0));
    let vh = img_hash(&vid_plain(0));
    // any video -> v3 (with or without images)
    assert_eq!(
        commitment_for(&job, &[ih], &[vh]).unwrap(),
        input_commitment_v3(&job, &[ih], &[vh]).unwrap()
    );
    assert_eq!(
        commitment_for(&job, &[], &[vh]).unwrap(),
        input_commitment_v3(&job, &[], &[vh]).unwrap()
    );
    // no videos -> the pre-BL3 selector is unchanged
    assert_eq!(
        commitment_for(&job, &[ih], &[]).unwrap(),
        input_commitment_v2(&job, &[ih]).unwrap()
    );
    assert_eq!(
        commitment_for(&job, &[], &[]).unwrap(),
        input_commitment(&job).unwrap()
    );
}

/// Emit `tests/ltx/vectors-iclora.json` from the SAME code paths (mirror of
/// `emit_i2v_vectors_json`): the 1-image+1-video iclora shape, the v2/v3 format
/// guard, and the array-slot guard, so the SDK conformance-checks one fixture.
#[test]
fn emit_iclora_vectors_json() {
    let job = sample_iclora_job();
    let i_plain = img_plain(0);
    let v_plain = vid_plain(0);
    let ih = img_hash(&i_plain);
    let vh = img_hash(&v_plain);
    let pre = v3_preimage(&job, &[ih], &[vh]);
    let commit = input_commitment_v3(&job, &[ih], &[vh]).unwrap();
    assert_eq!(commit, hx(&keccak256(&pre)));

    let guard_v2 = input_commitment_v2(&job, &[ih]).unwrap();
    let guard_v3_empty = input_commitment_v3(&job, &[ih], &[]).unwrap();
    assert_ne!(guard_v2, guard_v3_empty);
    let swapped = input_commitment_v3(&job, &[vh], &[ih]).unwrap();
    assert_ne!(commit, swapped);

    let vectors = serde_json::json!({
        "_note": "Generated by tests/ltx/test_attestation.rs::emit_iclora_vectors_json. Do not hand-edit.",
        "_scheme": "inputCommitment v3 = keccak256(abi.encode(string prompt, uint256 seed, uint32 frames, uint32 fps, uint32 w, uint32 h, string lora, bytes32[] imageHashes, bytes32[] videoHashes)). imageHashes[i]=keccak256(plaintext bytes of images[i]); videoHashes[i]=keccak256(plaintext bytes of videos[i]). Format is selected by the template's bundle entry: videoInputs > 0 -> this nine-field v3; else imageInputs > 0 -> the eight-field v2 (vectors-i2v.json); else the M0 seven-field (vectors.json). The capability CIDs are transport only and are NOT hashed into the commitment.",
        "referencePlusControl": {
            "imageInputs": 1,
            "imageSemantics": ["reference"],
            "videoInputs": 1,
            "videoSemantics": ["controlVideo"],
            "imageHashes": [hx(&ih)],
            "videoHashes": [hx(&vh)],
            "imagePlaintext": [{ "utf8": String::from_utf8(i_plain.clone()).unwrap(), "hex": hx(&i_plain) }],
            "videoPlaintext": [{ "utf8": String::from_utf8(v_plain.clone()).unwrap(), "hex": hx(&v_plain) }],
            "images": ["uCapabilityCidPlaceholder0"],
            "videos": ["uCapabilityCidPlaceholderV0"],
            "job": {
                "templateId": job.template_id,
                "templateHash": job.template_hash,
                "prompt": job.prompt,
                "seed": job.seed,
                "frames": job.frames,
                "fps": job.fps,
                "resolution": { "w": job.resolution.w, "h": job.resolution.h },
                "lora": job.lora,
                "output": "exr-sequence",
            },
            "inputCommitment": { "abiEncoded": hx(&pre), "hash": commit },
        },
        "formatGuard": {
            "note": "v3 with an EMPTY videoHashes array is NOT byte-equal to the v2 eight-field commitment (the trailing dynamic field shifts the earlier heads). Templates with videoInputs == 0 MUST use the v1/v2 paths; only videoInputs > 0 uses v3.",
            "v2EightField": guard_v2,
            "v3EmptyVideos": guard_v3_empty,
            "equal": false,
        },
        "arraySlotGuard": {
            "note": "imageHashes and videoHashes are distinct ABI slots. Binding the video hash as an image hash (or vice versa) changes the commitment.",
            "swappedHash": swapped,
        },
    });

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ltx/vectors-iclora.json");
    std::fs::write(path, serde_json::to_vec_pretty(&vectors).unwrap()).unwrap();
    assert!(std::path::Path::new(path).exists());
}
