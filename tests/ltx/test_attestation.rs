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
    assemble, env_hash, input_commitment, output_commitment, proof_hash, sig_digest, EnvMeta,
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
